//! 录屏命令
//!
//! 提供原生 Rust 录屏功能的 Tauri 命令接口。
//! 替代原先通过 Python Sidecar 实现的录屏命令。

use std::path::PathBuf;
use std::sync::Arc;
use tauri::{Manager, State, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::Mutex;
use tracing::info;

use crate::error::{HuGeError, HuGeResult};
use crate::recording::engine::{RecordingConfig, RecordingEngine};
use crate::recording::frame_capture::CaptureRegion;

// ============================================
// 录屏全局状态
// ============================================

/// 录屏引擎全局状态
pub struct RecordingEngineState {
    pub engine: Arc<Mutex<RecordingEngine>>,
}

impl RecordingEngineState {
    pub fn new() -> Self {
        Self {
            engine: Arc::new(Mutex::new(RecordingEngine::new())),
        }
    }
}

impl Default for RecordingEngineState {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================
// 命令参数和结果类型
// ============================================

/// 录制区域
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RecordingRegion {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// 录屏开始参数
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RecordStartParams {
    /// 录制区域（None 表示全屏）
    pub region: Option<RecordingRegion>,
    /// 帧率
    pub fps: Option<i32>,
    /// 质量 (low, medium, high)
    pub quality: Option<String>,
    /// 是否录制系统音频
    #[serde(rename = "systemAudio")]
    pub system_audio: Option<bool>,
    /// 是否录制麦克风
    #[serde(rename = "micAudio")]
    pub mic_audio: Option<bool>,
    /// 输出路径
    #[serde(rename = "outputPath")]
    pub output_path: Option<String>,
}

/// 录屏开始结果
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RecordStartResult {
    /// 输出文件路径
    #[serde(rename = "outputPath")]
    pub output_path: String,
    /// 帧率
    pub fps: i32,
    /// 质量
    pub quality: String,
}

/// 录屏停止结果
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RecordStopResult {
    /// 输出文件路径
    #[serde(rename = "outputPath")]
    pub output_path: String,
    /// 时长（秒）
    pub duration: f64,
    /// 帧数
    #[serde(rename = "frameCount")]
    pub frame_count: i32,
    /// 文件大小（字节）
    #[serde(rename = "fileSize")]
    pub file_size: i64,
}

/// 录屏状态结果
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RecordStatusResult {
    /// 状态 (idle, recording, paused, encoding, finished, error)
    pub state: String,
    /// 已录制时间（秒）
    #[serde(rename = "elapsedTime")]
    pub elapsed_time: f64,
    /// 输出路径
    #[serde(rename = "outputPath")]
    pub output_path: Option<String>,
    /// 帧数
    #[serde(rename = "frameCount")]
    pub frame_count: i32,
    /// 文件大小（字节）
    #[serde(rename = "fileSize")]
    pub file_size: i64,
}

// ============================================
// Tauri 命令实现
// ============================================

/// 开始录屏（原生 Rust 实现）
#[tauri::command]
pub async fn start_recording(
    state: State<'_, RecordingEngineState>,
    params: RecordStartParams,
) -> HuGeResult<RecordStartResult> {
    info!("命令调用: start_recording, 参数: {:?}", params);

    let fps = params.fps.unwrap_or(30) as u32;
    let quality = params.quality.clone().unwrap_or_else(|| "medium".to_string());
    let crf = RecordingConfig::crf_from_quality(&quality);

    // 构建录制区域
    let region = params.region.map(|r| CaptureRegion {
        x: r.x,
        y: r.y,
        width: r.width as u32,
        height: r.height as u32,
    });

    // 确定输出路径
    let output_path = params.output_path
        .map(PathBuf::from)
        .unwrap_or_else(RecordingConfig::default_output_path);

    let config = RecordingConfig {
        region,
        fps,
        crf,
        preset: "fast".to_string(),
        output_path: output_path.clone(),
        monitor_index: 0,
        system_audio: params.system_audio.unwrap_or(false),
        mic_audio: params.mic_audio.unwrap_or(false),
    };

    // 在独立任务中启动录制（因为 start 涉及试捕获等阻塞操作）
    let engine = state.engine.clone();
    tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Handle::current();
        rt.block_on(async {
            let mut engine_guard = engine.lock().await;
            engine_guard.start(config)
        })
    })
    .await
    .map_err(|e| HuGeError::CaptureError(format!("启动录制任务失败: {}", e)))??;

    let output_str = output_path.to_string_lossy().to_string();
    info!("录屏已启动: {}", output_str);

    Ok(RecordStartResult {
        output_path: output_str,
        fps: fps as i32,
        quality,
    })
}

/// 停止录屏
#[tauri::command]
pub async fn stop_recording(
    state: State<'_, RecordingEngineState>,
) -> HuGeResult<RecordStopResult> {
    info!("命令调用: stop_recording");

    let engine = state.engine.clone();
    let stats = tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Handle::current();
        rt.block_on(async {
            let mut engine_guard = engine.lock().await;
            engine_guard.stop()
        })
    })
    .await
    .map_err(|e| HuGeError::CaptureError(format!("停止录制任务失败: {}", e)))??;

    info!("录屏已停止: {:.1}s, {} 帧", stats.elapsed_time, stats.frame_count);

    Ok(RecordStopResult {
        output_path: stats.output_path,
        duration: stats.elapsed_time,
        frame_count: stats.frame_count as i32,
        file_size: stats.file_size,
    })
}

/// 暂停录屏
#[tauri::command]
pub async fn pause_recording(
    state: State<'_, RecordingEngineState>,
) -> HuGeResult<RecordStatusResult> {
    info!("命令调用: pause_recording");

    let mut engine = state.engine.lock().await;
    engine.pause()?;

    let stats = engine.stats();
    Ok(RecordStatusResult {
        state: stats.state.to_string(),
        elapsed_time: stats.elapsed_time,
        output_path: Some(stats.output_path),
        frame_count: stats.frame_count as i32,
        file_size: stats.file_size,
    })
}

/// 恢复录屏
#[tauri::command]
pub async fn resume_recording(
    state: State<'_, RecordingEngineState>,
) -> HuGeResult<RecordStatusResult> {
    info!("命令调用: resume_recording");

    let mut engine = state.engine.lock().await;
    engine.resume()?;

    let stats = engine.stats();
    Ok(RecordStatusResult {
        state: stats.state.to_string(),
        elapsed_time: stats.elapsed_time,
        output_path: Some(stats.output_path),
        frame_count: stats.frame_count as i32,
        file_size: stats.file_size,
    })
}

/// 获取录屏状态
#[tauri::command]
pub async fn get_recording_status(
    state: State<'_, RecordingEngineState>,
) -> HuGeResult<RecordStatusResult> {
    let engine = state.engine.lock().await;
    let stats = engine.stats();

    Ok(RecordStatusResult {
        state: stats.state.to_string(),
        elapsed_time: stats.elapsed_time,
        output_path: if stats.output_path.is_empty() { None } else { Some(stats.output_path) },
        frame_count: stats.frame_count as i32,
        file_size: stats.file_size,
    })
}

/// 打开录制控制面板窗口
#[tauri::command]
pub async fn open_recording_control(app: tauri::AppHandle) -> HuGeResult<()> {
    const WINDOW_LABEL: &str = "recording-control";

    info!("打开录制控制面板窗口...");

    // 检查窗口是否已存在
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        window.show().map_err(|e| HuGeError::WindowError(format!("显示窗口失败: {}", e)))?;
        window.set_focus().map_err(|e| HuGeError::WindowError(format!("聚焦窗口失败: {}", e)))?;
        return Ok(());
    }

    // 创建录制控制窗口
    let _window = WebviewWindowBuilder::new(
        &app,
        WINDOW_LABEL,
        WebviewUrl::App("recording-control.html".into()),
    )
    .title("录制控制")
    .inner_size(220.0, 46.0)
    .resizable(false)
    .decorations(false)
    .always_on_top(true)
    .transparent(true)
    .skip_taskbar(true)
    .center()
    .focused(true)
    .build()
    .map_err(|e| HuGeError::WindowError(format!("创建录制控制窗口失败: {}", e)))?;

    // 设置窗口不被录制捕获（Windows API: WDA_EXCLUDEFROMCAPTURE）
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::SetWindowDisplayAffinity;

        if let Ok(hwnd_raw) = _window.hwnd() {
            let hwnd = HWND(hwnd_raw.0);
            unsafe {
                // WDA_EXCLUDEFROMCAPTURE = 0x00000011
                let _ = SetWindowDisplayAffinity(hwnd, windows::Win32::UI::WindowsAndMessaging::WDA_EXCLUDEFROMCAPTURE);
            }
            info!("录制控制窗口已设置为排除捕获");
        }
    }

    info!("录制控制面板窗口创建成功");
    Ok(())
}

/// 将 overlay 窗口设为录屏模式（排除捕获，但不穿透鼠标）
///
/// 利用 Tauri 透明窗口的原生能力：CSS 透明区域自动穿透点击，
/// 不需要 WS_EX_TRANSPARENT（它会让整个窗口包括按钮都无法点击）。
/// 只需要 WDA_EXCLUDEFROMCAPTURE 让 overlay 不被录进视频。
#[tauri::command]
pub async fn set_overlay_recording_mode(
    app: tauri::AppHandle,
    enabled: bool,
) -> HuGeResult<()> {
    info!("设置 overlay 录屏模式: {}", enabled);

    for i in 0..4 {
        let label = format!("overlay-{}", i);
        if let Some(window) = app.get_webview_window(&label) {
            #[cfg(windows)]
            {
                use windows::Win32::Foundation::HWND;
                use windows::Win32::UI::WindowsAndMessaging::{
                    SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE,
                };

                if let Ok(hwnd_raw) = window.hwnd() {
                    let hwnd = HWND(hwnd_raw.0);
                    unsafe {
                        if enabled {
                            // 仅排除捕获（不设穿透，让 Tauri 透明窗口自然处理点击穿透）
                            let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
                            info!("overlay-{} 已设为录屏模式（排除捕获）", i);
                        } else {
                            // 恢复捕获
                            let _ = SetWindowDisplayAffinity(hwnd, windows::Win32::UI::WindowsAndMessaging::WINDOW_DISPLAY_AFFINITY(0));
                            info!("overlay-{} 已恢复正常模式", i);
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// 关闭录制控制面板窗口
#[tauri::command]
pub async fn close_recording_control(app: tauri::AppHandle) -> HuGeResult<()> {
    const WINDOW_LABEL: &str = "recording-control";

    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        window.close()
            .map_err(|e| HuGeError::WindowError(format!("关闭录制控制窗口失败: {}", e)))?;
    }

    Ok(())
}

/// 打开录制预览窗口
#[tauri::command]
pub async fn open_recording_preview(
    app: tauri::AppHandle,
    output_path: String,
    duration: f64,
    file_size: i64,
) -> HuGeResult<()> {
    const WINDOW_LABEL: &str = "recording-preview";

    info!("打开录制预览窗口: {}", output_path);

    // 关闭已有的预览窗口
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        let _ = window.close();
        // 等待旧窗口关闭
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    }

    // 将数据编码到 URL hash 中（最可靠的传递方式，不依赖事件时序）
    let data = serde_json::json!({
        "outputPath": output_path,
        "duration": duration,
        "fileSize": file_size,
    });
    // 简单的 percent-encoding（替换特殊字符）
    let data_str = data.to_string();
    let encoded: String = data_str.chars().map(|c| {
        match c {
            ' ' => "%20".to_string(),
            '#' => "%23".to_string(),
            '%' => "%25".to_string(),
            '&' => "%26".to_string(),
            '+' => "%2B".to_string(),
            _ => c.to_string(),
        }
    }).collect();
    let url_str = format!("recording-preview.html#{}", encoded);

    // 创建预览窗口
    let _window = WebviewWindowBuilder::new(
        &app,
        WINDOW_LABEL,
        WebviewUrl::App(url_str.into()),
    )
    .title("录制预览")
    .inner_size(640.0, 520.0)
    .min_inner_size(400.0, 350.0)
    .decorations(false)
    .center()
    .focused(true)
    .build()
    .map_err(|e| HuGeError::WindowError(format!("创建录制预览窗口失败: {}", e)))?;

    // 同时用事件发送（作为备用，多次重试确保前端收到）
    let app_handle = app.clone();
    let output_path_clone = output_path.clone();
    tokio::spawn(async move {
        use tauri::Emitter;
        for delay_ms in &[500, 1000, 2000] {
            tokio::time::sleep(tokio::time::Duration::from_millis(*delay_ms)).await;
            let _ = app_handle.emit("recording-completed", serde_json::json!({
                "outputPath": output_path_clone,
                "duration": duration,
                "fileSize": file_size,
            }));
        }
    });

    Ok(())
}

/// 关闭录制预览窗口
#[tauri::command]
pub async fn close_recording_preview(app: tauri::AppHandle) -> HuGeResult<()> {
    const WINDOW_LABEL: &str = "recording-preview";

    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        window.close()
            .map_err(|e| HuGeError::WindowError(format!("关闭预览窗口失败: {}", e)))?;
    }

    Ok(())
}
