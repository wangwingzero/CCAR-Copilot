//! FFmpeg 编码器模块
//!
//! 通过 FFmpeg 子进程将原始帧数据编码为 H.264/MP4 视频。
//! 使用 stdin 管道传输 BGRA 帧数据。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tracing::{error, info, warn};

use crate::error::{HuGeError, HuGeResult};
use super::frame_capture::CapturedFrame;

/// FFmpeg 编码器配置
#[derive(Debug, Clone)]
pub struct EncoderConfig {
    /// 输出文件路径
    pub output_path: PathBuf,
    /// 视频宽度（物理像素，必须为偶数）
    pub width: u32,
    /// 视频高度（物理像素，必须为偶数）
    pub height: u32,
    /// 帧率
    pub fps: u32,
    /// 恒定质量因子 (CRF)，越低质量越好，范围 0-51，默认 23
    pub crf: u32,
    /// 编码预设：ultrafast, superfast, veryfast, faster, fast, medium, slow
    pub preset: String,
    /// 像素格式输入 (bgra, 录屏使用原始 BGRA 不做转换以提升性能)
    pub input_pixel_format: String,
    /// 是否包含音频输入文件
    pub audio_input: Option<PathBuf>,
}

impl Default for EncoderConfig {
    fn default() -> Self {
        Self {
            output_path: PathBuf::from("output.mp4"),
            width: 1920,
            height: 1080,
            fps: 30,
            crf: 23,
            preset: "fast".to_string(),
            input_pixel_format: "bgra".to_string(),
            audio_input: None,
        }
    }
}

/// FFmpeg 编码器
///
/// 管理 FFmpeg 子进程的生命周期，接收原始帧数据并编码为视频。
pub struct FfmpegEncoder {
    config: EncoderConfig,
    process: Option<Child>,
    frame_count: Arc<AtomicU64>,
    is_running: Arc<AtomicBool>,
}

impl FfmpegEncoder {
    /// 创建 FFmpeg 编码器
    pub fn new(config: EncoderConfig) -> Self {
        Self {
            config,
            process: None,
            frame_count: Arc::new(AtomicU64::new(0)),
            is_running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// 获取帧计数器（可共享）
    pub fn frame_count(&self) -> Arc<AtomicU64> {
        self.frame_count.clone()
    }

    /// 获取运行状态标志
    pub fn is_running(&self) -> Arc<AtomicBool> {
        self.is_running.clone()
    }

    /// 查找 FFmpeg 可执行文件路径
    fn find_ffmpeg() -> HuGeResult<PathBuf> {
        // 1. 检查应用资源目录（打包时捆绑的 ffmpeg）
        if let Ok(exe_path) = std::env::current_exe() {
            let app_dir = exe_path.parent().unwrap_or(Path::new("."));
            let bundled_ffmpeg = app_dir.join("ffmpeg.exe");
            if bundled_ffmpeg.exists() {
                info!("使用捆绑的 FFmpeg: {:?}", bundled_ffmpeg);
                return Ok(bundled_ffmpeg);
            }
            // 也检查 resources 子目录
            let resources_ffmpeg = app_dir.join("resources").join("ffmpeg.exe");
            if resources_ffmpeg.exists() {
                info!("使用资源目录的 FFmpeg: {:?}", resources_ffmpeg);
                return Ok(resources_ffmpeg);
            }
        }

        // 2. 检查 PATH 中的 ffmpeg
        if let Ok(output) = Command::new("where").arg("ffmpeg").output() {
            if output.status.success() {
                let path_str = String::from_utf8_lossy(&output.stdout);
                if let Some(first_line) = path_str.lines().next() {
                    let path = PathBuf::from(first_line.trim());
                    if path.exists() {
                        info!("使用系统 PATH 中的 FFmpeg: {:?}", path);
                        return Ok(path);
                    }
                }
            }
        }

        // 3. 直接尝试 "ffmpeg" 命令（依赖 PATH）
        info!("尝试使用 PATH 中的 ffmpeg 命令");
        Ok(PathBuf::from("ffmpeg"))
    }

    /// 启动 FFmpeg 子进程
    pub fn start(&mut self) -> HuGeResult<()> {
        let ffmpeg_path = Self::find_ffmpeg()?;

        // 确保输出目录存在
        if let Some(parent) = self.config.output_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        // 构建 FFmpeg 命令
        let mut cmd = Command::new(&ffmpeg_path);

        // 全局选项
        cmd.arg("-y") // 覆盖输出文件
            .arg("-hide_banner")
            .arg("-loglevel").arg("warning");

        // 视频输入（从 stdin 读取原始帧）
        cmd.arg("-f").arg("rawvideo")
            .arg("-pix_fmt").arg(&self.config.input_pixel_format)
            .arg("-s").arg(format!("{}x{}", self.config.width, self.config.height))
            .arg("-r").arg(self.config.fps.to_string())
            .arg("-i").arg("pipe:0");

        // 音频输入（如果有）
        if let Some(ref audio_path) = self.config.audio_input {
            cmd.arg("-i").arg(audio_path);
        }

        // 视频编码选项
        cmd.arg("-c:v").arg("libx264")
            .arg("-preset").arg(&self.config.preset)
            .arg("-crf").arg(self.config.crf.to_string())
            .arg("-pix_fmt").arg("yuv420p")
            .arg("-tune").arg("zerolatency");

        // 音频编码选项（如果有音频输入）
        if self.config.audio_input.is_some() {
            cmd.arg("-c:a").arg("aac")
                .arg("-b:a").arg("128k");
        }

        // 输出文件
        cmd.arg(&self.config.output_path);

        // 设置 stdin 为管道，stdout/stderr 管道用于错误检测
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());

        // Windows: 隐藏控制台窗口
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }

        info!(
            "启动 FFmpeg 编码器: {:?}, {}x{} @ {}fps, CRF={}, preset={}",
            self.config.output_path,
            self.config.width,
            self.config.height,
            self.config.fps,
            self.config.crf,
            self.config.preset,
        );

        let process = cmd.spawn().map_err(|e| {
            error!("启动 FFmpeg 失败: {}", e);
            if e.kind() == std::io::ErrorKind::NotFound {
                HuGeError::CaptureError(
                    "FFmpeg 未找到。请安装 FFmpeg 或将 ffmpeg.exe 放入应用目录。".to_string(),
                )
            } else {
                HuGeError::CaptureError(format!("启动 FFmpeg 失败: {}", e))
            }
        })?;

        self.process = Some(process);
        self.is_running.store(true, Ordering::SeqCst);
        self.frame_count.store(0, Ordering::SeqCst);

        info!("FFmpeg 编码器已启动");
        Ok(())
    }

    /// 写入一帧数据
    pub fn write_frame(&mut self, frame: &CapturedFrame) -> HuGeResult<()> {
        if let Some(ref mut process) = self.process {
            if let Some(ref mut stdin) = process.stdin {
                // 写入 BGRA 帧数据
                stdin.write_all(&frame.data).map_err(|e| {
                    error!("写入帧数据到 FFmpeg 失败: {}", e);
                    HuGeError::CaptureError(format!("写入帧失败: {}", e))
                })?;

                self.frame_count.fetch_add(1, Ordering::Relaxed);
                Ok(())
            } else {
                Err(HuGeError::CaptureError("FFmpeg stdin 不可用".to_string()))
            }
        } else {
            Err(HuGeError::CaptureError("FFmpeg 未启动".to_string()))
        }
    }

    /// 停止编码器并等待 FFmpeg 完成
    ///
    /// 关闭 stdin 管道触发 FFmpeg 刷新并完成编码。
    /// 返回最终帧计数。
    pub fn stop(&mut self) -> HuGeResult<u64> {
        let final_count = self.frame_count.load(Ordering::SeqCst);
        info!("停止 FFmpeg 编码器，共 {} 帧", final_count);

        if let Some(mut process) = self.process.take() {
            // 关闭 stdin 管道，触发 FFmpeg 刷新
            drop(process.stdin.take());

            // 等待 FFmpeg 完成（最多 30 秒）
            let start = Instant::now();
            let timeout = std::time::Duration::from_secs(30);

            loop {
                match process.try_wait() {
                    Ok(Some(status)) => {
                        if status.success() {
                            info!("FFmpeg 编码完成");
                        } else {
                            // 读取 stderr 获取错误信息
                            let stderr_output = if let Some(mut stderr) = process.stderr.take() {
                                use std::io::Read;
                                let mut buf = String::new();
                                stderr.read_to_string(&mut buf).unwrap_or(0);
                                buf
                            } else {
                                String::new()
                            };
                            warn!("FFmpeg 退出码异常: {:?}, stderr: {}", status.code(), stderr_output);
                        }
                        break;
                    }
                    Ok(None) => {
                        if start.elapsed() > timeout {
                            warn!("FFmpeg 超时未完成，强制终止");
                            let _ = process.kill();
                            break;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(100));
                    }
                    Err(e) => {
                        error!("等待 FFmpeg 完成失败: {}", e);
                        break;
                    }
                }
            }
        }

        self.is_running.store(false, Ordering::SeqCst);
        Ok(final_count)
    }

    /// 获取已编码帧数
    pub fn get_frame_count(&self) -> u64 {
        self.frame_count.load(Ordering::SeqCst)
    }

    /// 获取输出文件路径
    pub fn output_path(&self) -> &Path {
        &self.config.output_path
    }
}

impl Drop for FfmpegEncoder {
    fn drop(&mut self) {
        if self.process.is_some() {
            let _ = self.stop();
        }
    }
}
