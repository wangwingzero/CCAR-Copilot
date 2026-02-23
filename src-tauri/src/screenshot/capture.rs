//! 屏幕捕获功能
//!
//! 支持三种捕获引擎（按优先级排序）：
//! 1. **WGC (Windows Graphics Capture)**（首选）- 快速稳定，推荐引擎
//! 2. **screenshots-rs (GDI)**（回退）- 基于 GDI 的通用捕获
//! 3. **DXGI Desktop Duplication API** - 仅用于录屏模块
//!
//! # 功能特性
//!
//! - 单显示器捕获
//! - 多显示器同时捕获
//! - 高 DPI 支持（物理像素分辨率）
//! - 预分配 Buffer 减少内存分配开销
//! - 自动错误恢复（DXGI AccessLost 自动重新初始化）
//!
//! # 重要说明
//!
//! - 返回的是**物理像素**分辨率的图像
//! - `display_info.width/height` 是物理像素尺寸
//! - `display_info.scale_factor` 是 DPR（设备像素比）
//! - 多显示器场景下，副屏可能有负坐标（位于主屏左侧时）

use screenshots::Screen;
use serde::{Deserialize, Serialize};
use std::env;
use std::io::BufWriter;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tracing::{debug, error, info, warn};

use crate::error::{HuGeError, HuGeResult};
use super::image_hash::compute_bytes_hash;

// ============================================================================
// 高性能 PNG 保存和哈希计算
// ============================================================================

/// 高性能 PNG 保存（使用快速压缩级别）
///
/// 默认 `image.save()` 使用 PNG 压缩级别 6（中等），对于 3120x2080 图像编码可能需要 1-2 秒。
/// 此函数使用压缩级别 1（最快），牺牲约 10-20% 的文件体积换取 3-5 倍的编码速度。
///
/// 对于临时截图文件，文件体积不是关键因素，速度更重要。
///
/// # 参数
/// - `image`: RGBA 图像数据
/// - `path`: 保存路径
///
/// # 返回
/// - `Ok(png_bytes_len)`: PNG 文件大小（字节）
fn save_png_fast(image: &image::RgbaImage, path: &std::path::Path) -> HuGeResult<u64> {
    let file = std::fs::File::create(path).map_err(|e| {
        error!("创建文件失败: {:?}: {}", path, e);
        HuGeError::CaptureError(format!("创建文件失败: {}", e))
    })?;
    let writer = BufWriter::new(file);

    let encoder = image::codecs::png::PngEncoder::new_with_quality(
        writer,
        image::codecs::png::CompressionType::Fast,
        image::codecs::png::FilterType::Sub,
    );

    image::ImageEncoder::write_image(
        encoder,
        image.as_raw(),
        image.width(),
        image.height(),
        image::ExtendedColorType::Rgba8,
    )
    .map_err(|e| {
        error!("PNG 编码失败: {}", e);
        HuGeError::CaptureError(format!("PNG 编码失败: {}", e))
    })?;

    let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    Ok(file_size)
}

/// 高性能 PNG 保存并计算哈希（避免重新读取文件）
///
/// 同时完成两个操作：
/// 1. 将 RGBA 图像编码为 PNG 并保存到磁盘
/// 2. 从内存中的 RGBA 原始数据计算 MD5 哈希
///
/// 比 `save + compute_file_hash` 快约 30%，因为避免了一次完整的文件读取。
/// 公开版本的 save_png_fast_with_hash（供 screenshot_cmd 调用）
pub fn save_png_fast_with_hash_pub(
    image: &image::RgbaImage,
    path: &std::path::Path,
) -> HuGeResult<(u64, Option<String>)> {
    save_png_fast_with_hash(image, path)
}

fn save_png_fast_with_hash(
    image: &image::RgbaImage,
    path: &std::path::Path,
) -> HuGeResult<(u64, Option<String>)> {
    // 保存 PNG（快速压缩）
    let file_size = save_png_fast(image, path)?;

    // 从内存中的原始像素数据计算哈希（避免重新读取文件）
    let hash = compute_bytes_hash(image.as_raw());
    debug!("计算内存哈希: {:?} -> {}", path, hash);

    Ok((file_size, Some(hash)))
}

// ============================================================================
// DXGI 截图引擎（Windows 专用）
// ============================================================================

#[cfg(windows)]
mod dxgi {
    use super::*;
    use windows::Win32::Graphics::Direct3D::{
        D3D_DRIVER_TYPE_UNKNOWN, D3D_FEATURE_LEVEL, D3D_FEATURE_LEVEL_11_0, D3D_FEATURE_LEVEL_11_1,
    };
    use windows::Win32::Graphics::Direct3D11::{
        D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Resource, ID3D11Texture2D,
        D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
        D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
        D3D11_USAGE_STAGING,
    };
    use windows::Win32::Graphics::Dxgi::Common::{
        DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
    };
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, IDXGIAdapter1, IDXGIFactory1, IDXGIOutput1,
        IDXGIOutputDuplication, IDXGIResource, DXGI_ERROR_ACCESS_LOST, DXGI_ERROR_WAIT_TIMEOUT,
        DXGI_ERROR_DEVICE_REMOVED, DXGI_OUTDUPL_FRAME_INFO,
    };
    use windows::core::Interface;

    /// DXGI 截图引擎配置
    #[derive(Debug, Clone)]
    pub struct DxgiCaptureConfig {
        /// 捕获超时（毫秒）
        pub timeout_ms: u32,
        /// 是否包含鼠标指针（暂未实现）
        pub include_cursor: bool,
        /// 预分配 Buffer 大小（字节）
        pub buffer_size: usize,
    }

    impl Default for DxgiCaptureConfig {
        fn default() -> Self {
            Self {
                timeout_ms: 100,
                include_cursor: false,
                // 默认预分配 4K 分辨率的 BGRA 缓冲区：3840 * 2160 * 4 ≈ 33MB
                buffer_size: 3840 * 2160 * 4,
            }
        }
    }

    /// 捕获帧数据
    #[derive(Debug)]
    pub struct CaptureFrame {
        /// RGBA 像素数据
        pub data: Vec<u8>,
        /// 宽度（物理像素）
        pub width: u32,
        /// 高度（物理像素）
        pub height: u32,
        /// 捕获耗时（微秒）
        pub capture_time_us: u64,
    }

    /// DXGI 截图引擎
    ///
    /// 使用 DXGI Desktop Duplication API 实现高性能屏幕捕获。
    /// 
    /// # 性能特点
    /// 
    /// - 硬件加速：直接从 GPU 获取帧数据
    /// - 预分配 Buffer：减少内存分配开销
    /// - Staging Texture 复用：避免重复创建纹理
    ///
    /// # 错误处理
    ///
    /// - `DXGI_ERROR_ACCESS_LOST`：自动调用 `reinitialize()` 重新初始化
    /// - `DXGI_ERROR_WAIT_TIMEOUT`：返回超时错误，不重试
    pub struct DxgiCaptureEngine {
        /// D3D11 设备
        device: ID3D11Device,
        /// D3D11 上下文
        context: ID3D11DeviceContext,
        /// 输出复制接口
        duplication: IDXGIOutputDuplication,
        /// Staging 纹理（预分配，复用）
        staging_texture: Option<ID3D11Texture2D>,
        /// 预分配 Buffer
        buffer: Vec<u8>,
        /// 配置
        config: DxgiCaptureConfig,
        /// 显示器 ID
        monitor_id: u32,
        /// 显示器 X 坐标
        screen_x: i32,
        /// 显示器 Y 坐标
        screen_y: i32,
        /// 显示器宽度
        width: u32,
        /// 显示器高度
        height: u32,
        /// DXGI 适配器（用于重新初始化）
        adapter: IDXGIAdapter1,
        /// DXGI 输出索引（用于重新初始化）
        output_index: u32,
    }

    impl DxgiCaptureEngine {
        /// 创建 DXGI 截图引擎
        ///
        /// # 参数
        ///
        /// - `monitor_id`: 显示器 ID（对应 DXGI 输出索引）
        /// - `config`: 引擎配置
        ///
        /// # 返回
        ///
        /// 成功返回 `DxgiCaptureEngine` 实例，失败返回错误
        ///
        /// # 错误
        ///
        /// - DXGI Factory 创建失败
        /// - 找不到指定的显示器
        /// - D3D11 设备创建失败
        /// - DuplicateOutput 失败
        pub fn new(monitor_id: u32, screen_x: i32, screen_y: i32, screen_width: u32, screen_height: u32, config: DxgiCaptureConfig) -> HuGeResult<Self> {
            info!("初始化 DXGI 截图引擎，显示器 ID: {}, 坐标: ({}, {}), 尺寸: {}x{}", monitor_id, screen_x, screen_y, screen_width, screen_height);
            let start = Instant::now();

            unsafe {
                // 1. 创建 DXGI Factory
                let factory: IDXGIFactory1 = CreateDXGIFactory1().map_err(|e| {
                    error!("创建 DXGI Factory 失败: {:?}", e);
                    HuGeError::CaptureError(format!("创建 DXGI Factory 失败: {:?}", e))
                })?;

                // 2. 枚举适配器和输出，通过坐标匹配找到目标显示器
                let (adapter, output, output_index, width, height) =
                    Self::find_output(&factory, monitor_id, screen_x, screen_y, screen_width, screen_height)?;

                // 3. 创建 D3D11 设备
                let (device, context) = Self::create_d3d11_device(&adapter)?;

                // 4. 创建 DuplicateOutput
                let duplication = Self::create_duplication(&output, &device)?;

                // 5. 预分配 Buffer
                let buffer = vec![0u8; config.buffer_size];

                let elapsed = start.elapsed();
                info!(
                    "DXGI 截图引擎初始化完成，显示器 {}: {}x{}，耗时: {:?}",
                    monitor_id, width, height, elapsed
                );

                Ok(Self {
                    device,
                    context,
                    duplication,
                    staging_texture: None,
                    buffer,
                    config,
                    monitor_id,
                    screen_x,
                    screen_y,
                    width,
                    height,
                    adapter,
                    output_index,
                })
            }
        }

        /// 枚举 DXGI 输出，找到目标显示器
        ///
        /// 通过屏幕坐标匹配 DXGI 输出，而非依赖 ID 匹配。
        /// 这解决了 screenshots-rs 原生 ID 与 DXGI 枚举索引不一致的问题。
        unsafe fn find_output(
            factory: &IDXGIFactory1,
            monitor_id: u32,
            target_x: i32,
            target_y: i32,
            target_width: u32,
            target_height: u32,
        ) -> HuGeResult<(IDXGIAdapter1, IDXGIOutput1, u32, u32, u32)> {
            let mut adapter_index = 0u32;
            let mut total_outputs = 0u32;

            loop {
                // 枚举适配器
                let adapter = match factory.EnumAdapters1(adapter_index) {
                    Ok(a) => a,
                    Err(_) => break,
                };

                let mut output_index = 0u32;
                loop {
                    // 枚举输出
                    let output = match adapter.EnumOutputs(output_index) {
                        Ok(o) => o,
                        Err(_) => break,
                    };

                    // 获取输出描述
                    let desc = output.GetDesc().map_err(|e| {
                        HuGeError::CaptureError(format!("获取输出描述失败: {:?}", e))
                    })?;

                    let out_x = desc.DesktopCoordinates.left;
                    let out_y = desc.DesktopCoordinates.top;
                    let out_width = (desc.DesktopCoordinates.right - desc.DesktopCoordinates.left) as u32;
                    let out_height = (desc.DesktopCoordinates.bottom - desc.DesktopCoordinates.top) as u32;

                    // 通过屏幕坐标和尺寸匹配目标显示器
                    if out_x == target_x && out_y == target_y
                        && out_width == target_width && out_height == target_height
                    {
                        // 转换为 IDXGIOutput1
                        let output1: IDXGIOutput1 = output.cast().map_err(|e| {
                            HuGeError::CaptureError(format!("转换为 IDXGIOutput1 失败: {:?}", e))
                        })?;

                        debug!(
                            "找到目标显示器 {} (坐标匹配): {}x{} @ ({}, {})",
                            monitor_id,
                            out_width,
                            out_height,
                            out_x,
                            out_y
                        );

                        return Ok((adapter, output1, output_index, out_width, out_height));
                    }

                    total_outputs += 1;
                    output_index += 1;
                }

                adapter_index += 1;
            }

            Err(HuGeError::CaptureError(format!(
                "未找到显示器 ID: {} (坐标: ({}, {}), 尺寸: {}x{})，共检测到 {} 个 DXGI 输出",
                monitor_id, target_x, target_y, target_width, target_height, total_outputs
            )))
        }

        /// 创建 D3D11 设备
        unsafe fn create_d3d11_device(
            adapter: &IDXGIAdapter1,
        ) -> HuGeResult<(ID3D11Device, ID3D11DeviceContext)> {
            let feature_levels = [D3D_FEATURE_LEVEL_11_1, D3D_FEATURE_LEVEL_11_0];
            let mut device: Option<ID3D11Device> = None;
            let mut context: Option<ID3D11DeviceContext> = None;
            let mut actual_feature_level: D3D_FEATURE_LEVEL = D3D_FEATURE_LEVEL_11_0;

            D3D11CreateDevice(
                adapter,
                D3D_DRIVER_TYPE_UNKNOWN, // 使用指定的适配器时必须设为 UNKNOWN
                None,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT, // 必须支持 BGRA 以便与 DXGI 兼容
                Some(&feature_levels),
                D3D11_SDK_VERSION,
                Some(&mut device),
                Some(&mut actual_feature_level),
                Some(&mut context),
            )
            .map_err(|e| {
                error!("创建 D3D11 设备失败: {:?}", e);
                HuGeError::CaptureError(format!("创建 D3D11 设备失败: {:?}", e))
            })?;

            let device = device.ok_or_else(|| {
                HuGeError::CaptureError("D3D11 设备创建返回 None".to_string())
            })?;
            let context = context.ok_or_else(|| {
                HuGeError::CaptureError("D3D11 上下文创建返回 None".to_string())
            })?;

            debug!("D3D11 设备创建成功，Feature Level: {:?}", actual_feature_level);

            Ok((device, context))
        }

        /// 创建 DuplicateOutput
        unsafe fn create_duplication(
            output: &IDXGIOutput1,
            device: &ID3D11Device,
        ) -> HuGeResult<IDXGIOutputDuplication> {
            output.DuplicateOutput(device).map_err(|e| {
                error!("创建 DuplicateOutput 失败: {:?}", e);
                HuGeError::CaptureError(format!(
                    "创建 DuplicateOutput 失败: {:?}。可能原因：\n\
                     1. 另一个程序正在使用桌面复制\n\
                     2. 显示器处于独占全屏模式\n\
                     3. 需要管理员权限",
                    e
                ))
            })
        }

        /// 创建或获取 Staging 纹理
        ///
        /// Staging 纹理用于从 GPU 复制数据到 CPU 可读内存
        unsafe fn get_or_create_staging_texture(
            &mut self,
            width: u32,
            height: u32,
        ) -> HuGeResult<&ID3D11Texture2D> {
            // 检查是否需要重新创建
            let need_recreate = match &self.staging_texture {
                Some(tex) => {
                    let mut desc = D3D11_TEXTURE2D_DESC::default();
                    tex.GetDesc(&mut desc);
                    desc.Width != width || desc.Height != height
                }
                None => true,
            };

            if need_recreate {
                debug!("创建 Staging 纹理: {}x{}", width, height);

                let desc = D3D11_TEXTURE2D_DESC {
                    Width: width,
                    Height: height,
                    MipLevels: 1,
                    ArraySize: 1,
                    Format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    SampleDesc: DXGI_SAMPLE_DESC {
                        Count: 1,
                        Quality: 0,
                    },
                    Usage: D3D11_USAGE_STAGING,
                    BindFlags: 0, // No bind flags for staging texture
                    CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                    MiscFlags: 0,
                };

                let mut texture: Option<ID3D11Texture2D> = None;
                self.device
                    .CreateTexture2D(&desc, None, Some(&mut texture))
                    .map_err(|e| {
                        error!("创建 Staging 纹理失败: {:?}", e);
                        HuGeError::CaptureError(format!("创建 Staging 纹理失败: {:?}", e))
                    })?;

                self.staging_texture = texture;
            }

            self.staging_texture.as_ref().ok_or_else(|| {
                HuGeError::CaptureError("Staging 纹理为空".to_string())
            })
        }

        /// 重新初始化引擎
        ///
        /// 当遇到 `DXGI_ERROR_ACCESS_LOST` 错误时调用此方法重新初始化。
        /// 常见触发场景：
        /// - 分辨率更改
        /// - UAC 弹窗
        /// - 全屏游戏切换
        /// - 显示器连接/断开
        pub fn reinitialize(&mut self) -> HuGeResult<()> {
            info!("重新初始化 DXGI 截图引擎，显示器 ID: {}", self.monitor_id);

            unsafe {
                // 重新创建 DXGI Factory
                let factory: IDXGIFactory1 = CreateDXGIFactory1().map_err(|e| {
                    error!("重新创建 DXGI Factory 失败: {:?}", e);
                    HuGeError::CaptureError(format!("重新创建 DXGI Factory 失败: {:?}", e))
                })?;

                // 重新查找输出（通过坐标匹配）
                let (adapter, output, output_index, width, height) =
                    Self::find_output(&factory, self.monitor_id, self.screen_x, self.screen_y, self.width, self.height)?;

                // 重新创建 D3D11 设备
                let (device, context) = Self::create_d3d11_device(&adapter)?;

                // 重新创建 DuplicateOutput
                let duplication = Self::create_duplication(&output, &device)?;

                // 更新状态
                self.device = device;
                self.context = context;
                self.duplication = duplication;
                self.staging_texture = None; // 清除旧的 Staging 纹理
                self.adapter = adapter;
                self.output_index = output_index;
                self.width = width;
                self.height = height;

                info!(
                    "DXGI 截图引擎重新初始化完成，显示器 {}: {}x{}",
                    self.monitor_id, width, height
                );

                Ok(())
            }
        }

        /// 获取显示器宽度
        pub fn width(&self) -> u32 {
            self.width
        }

        /// 获取显示器高度
        pub fn height(&self) -> u32 {
            self.height
        }

        /// 获取显示器 ID
        pub fn monitor_id(&self) -> u32 {
            self.monitor_id
        }

        /// 获取配置
        pub fn config(&self) -> &DxgiCaptureConfig {
            &self.config
        }

        /// 捕获屏幕并自动处理错误恢复
        ///
        /// 此方法封装了 `capture()` 并自动处理以下错误：
        /// - `DXGI_ERROR_ACCESS_LOST`：自动重新初始化并重试
        /// - `DXGI_ERROR_DEVICE_REMOVED`：重新创建 D3D11 设备并重试
        ///
        /// # 参数
        ///
        /// - `max_retries`: 最大重试次数（默认 2）
        ///
        /// # 返回
        ///
        /// 成功返回 `CaptureFrame`，失败返回错误
        ///
        /// # 注意
        ///
        /// 如果多次重试后仍然失败，调用者应考虑回退到 screenshots-rs
        pub fn capture_with_auto_recovery(&mut self, max_retries: u32) -> HuGeResult<CaptureFrame> {
            let mut retries = 0;
            
            loop {
                match self.capture() {
                    Ok(frame) => return Ok(frame),
                    Err(e) => {
                        let error_msg = e.to_string();
                        
                        // 检查是否是可恢复的错误
                        let is_access_lost = error_msg.contains("访问丢失") || error_msg.contains("ACCESS_LOST");
                        let is_device_removed = error_msg.contains("设备已移除") || error_msg.contains("DEVICE_REMOVED");
                        
                        if (is_access_lost || is_device_removed) && retries < max_retries {
                            retries += 1;
                            warn!(
                                "DXGI 捕获失败 ({}), 尝试重新初始化 ({}/{})",
                                if is_access_lost { "ACCESS_LOST" } else { "DEVICE_REMOVED" },
                                retries,
                                max_retries
                            );
                            
                            // 尝试重新初始化
                            match self.reinitialize() {
                                Ok(_) => {
                                    info!("DXGI 引擎重新初始化成功，继续捕获");
                                    // 短暂延迟，让系统稳定
                                    std::thread::sleep(std::time::Duration::from_millis(50));
                                    continue;
                                }
                                Err(reinit_err) => {
                                    error!("DXGI 引擎重新初始化失败: {}", reinit_err);
                                    return Err(HuGeError::CaptureError(format!(
                                        "DXGI 重新初始化失败: {}",
                                        reinit_err
                                    )));
                                }
                            }
                        }
                        
                        // 不可恢复的错误或重试次数用尽
                        return Err(e);
                    }
                }
            }
        }

        /// 捕获屏幕（返回 RGBA 数据）
        ///
        /// # 返回
        ///
        /// 成功返回 `CaptureFrame`，包含 RGBA 像素数据和元信息
        ///
        /// # 错误
        ///
        /// - `DXGI_ERROR_WAIT_TIMEOUT`：超时，屏幕无变化
        /// - `DXGI_ERROR_ACCESS_LOST`：需要重新初始化（建议使用 `capture_with_auto_recovery`）
        /// - `DXGI_ERROR_DEVICE_REMOVED`：设备已移除（建议使用 `capture_with_auto_recovery`）
        /// - 其他 DXGI/D3D11 错误
        ///
        /// # 性能
        ///
        /// - 典型耗时：< 10ms（硬件加速）
        /// - 超时设置：由 `config.timeout_ms` 控制（默认 100ms）
        pub fn capture(&mut self) -> HuGeResult<CaptureFrame> {
            let start = Instant::now();

            unsafe {
                // 1. 获取下一帧（AcquireNextFrame）
                let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
                let mut desktop_resource: Option<IDXGIResource> = None;

                let acquire_result = self.duplication.AcquireNextFrame(
                    self.config.timeout_ms,
                    &mut frame_info,
                    &mut desktop_resource,
                );

                match acquire_result {
                    Ok(_) => {}
                    Err(e) => {
                        let code = e.code();
                        if code == DXGI_ERROR_WAIT_TIMEOUT {
                            debug!("DXGI 捕获超时（屏幕无变化），超时设置: {}ms", self.config.timeout_ms);
                            return Err(HuGeError::CaptureError(
                                "捕获超时：屏幕无变化".to_string(),
                            ));
                        } else if code == DXGI_ERROR_ACCESS_LOST {
                            warn!("DXGI 访问丢失 (ACCESS_LOST)，需要重新初始化");
                            return Err(HuGeError::CaptureError(
                                "DXGI 访问丢失 (ACCESS_LOST)，需要重新初始化".to_string(),
                            ));
                        } else if code == DXGI_ERROR_DEVICE_REMOVED {
                            warn!("DXGI 设备已移除 (DEVICE_REMOVED)，需要重新初始化");
                            return Err(HuGeError::CaptureError(
                                "DXGI 设备已移除 (DEVICE_REMOVED)，需要重新初始化".to_string(),
                            ));
                        } else {
                            error!("AcquireNextFrame 失败: {:?}", e);
                            return Err(HuGeError::CaptureError(format!(
                                "AcquireNextFrame 失败: {:?}",
                                e
                            )));
                        }
                    }
                }

                // 2. 获取桌面纹理
                let desktop_resource = desktop_resource.ok_or_else(|| {
                    HuGeError::CaptureError("桌面资源为空".to_string())
                })?;

                let desktop_texture: ID3D11Texture2D = match desktop_resource.cast() {
                    Ok(tex) => tex,
                    Err(e) => {
                        // 释放帧
                        if let Err(re) = self.duplication.ReleaseFrame() {
                            tracing::warn!("ReleaseFrame 失败: {:?}", re);
                        }
                        return Err(HuGeError::CaptureError(format!("转换为 ID3D11Texture2D 失败: {:?}", e)));
                    }
                };

                // 3. 获取纹理描述
                let mut desc = D3D11_TEXTURE2D_DESC::default();
                desktop_texture.GetDesc(&mut desc);

                let width = desc.Width;
                let height = desc.Height;

                // 4. 获取或创建 Staging 纹理
                let staging_texture = match self.get_or_create_staging_texture(width, height) {
                    Ok(tex) => tex,
                    Err(e) => {
                        if let Err(re) = self.duplication.ReleaseFrame() {
                            tracing::warn!("ReleaseFrame 失败: {:?}", re);
                        }
                        return Err(e);
                    }
                };

                // 5. 复制纹理到 Staging
                let desktop_resource: ID3D11Resource = match desktop_texture.cast() {
                    Ok(res) => res,
                    Err(e) => {
                        if let Err(re) = self.duplication.ReleaseFrame() {
                            tracing::warn!("ReleaseFrame 失败: {:?}", re);
                        }
                        return Err(HuGeError::CaptureError(format!("转换为 ID3D11Resource 失败: {:?}", e)));
                    }
                };

                let staging_resource: ID3D11Resource = match staging_texture.cast() {
                    Ok(res) => res,
                    Err(e) => {
                        let _ = self.duplication.ReleaseFrame();
                        return Err(HuGeError::CaptureError(format!("Staging 转换为 ID3D11Resource 失败: {:?}", e)));
                    }
                };

                self.context.CopyResource(&staging_resource, &desktop_resource);

                // 6. 释放帧（ReleaseFrame - 必须在 Map 之前释放）
                if let Err(e) = self.duplication.ReleaseFrame() {
                    error!("ReleaseFrame 失败: {:?}", e);
                    return Err(HuGeError::CaptureError(format!("ReleaseFrame 失败: {:?}", e)));
                }

                // 7. Map Staging 纹理读取像素数据
                let mut mapped_resource = D3D11_MAPPED_SUBRESOURCE::default();
                if let Err(e) = self.context.Map(
                    &staging_resource, 
                    0, 
                    D3D11_MAP_READ, 
                    0,
                    Some(&mut mapped_resource)
                ) {
                    error!("Map Staging 纹理失败: {:?}", e);
                    return Err(HuGeError::CaptureError(format!("Map Staging 纹理失败: {:?}", e)));
                }

                // 8. 读取像素数据并转换 BGRA -> RGBA
                // 注意：RowPitch 可能大于 width * 4（内存对齐填充）
                let row_pitch = mapped_resource.RowPitch as usize;
                let expected_row_size = (width * 4) as usize;
                let total_size = (width * height * 4) as usize;

                // 确保预分配 Buffer 足够大
                if self.buffer.len() < total_size {
                    self.buffer.resize(total_size, 0);
                }

                // 获取源数据指针
                let src_ptr = mapped_resource.pData as *const u8;

                // 逐行复制并转换 BGRA -> RGBA
                // BGRA: [B, G, R, A] -> RGBA: [R, G, B, A]
                for y in 0..height as usize {
                    let src_row = std::slice::from_raw_parts(
                        src_ptr.add(y * row_pitch),
                        expected_row_size,
                    );
                    let dst_offset = y * expected_row_size;

                    // BGRA 到 RGBA 通道转换
                    // 每 4 字节为一个像素：[B, G, R, A] -> [R, G, B, A]
                    for x in 0..(width as usize) {
                        let src_idx = x * 4;
                        let dst_idx = dst_offset + x * 4;

                        // BGRA -> RGBA 转换
                        // src: [B, G, R, A] at indices [0, 1, 2, 3]
                        // dst: [R, G, B, A] at indices [0, 1, 2, 3]
                        self.buffer[dst_idx] = src_row[src_idx + 2];     // R = B位置的值
                        self.buffer[dst_idx + 1] = src_row[src_idx + 1]; // G = G位置的值
                        self.buffer[dst_idx + 2] = src_row[src_idx];     // B = R位置的值
                        self.buffer[dst_idx + 3] = src_row[src_idx + 3]; // A = A位置的值
                    }
                }

                // 9. Unmap Staging 纹理
                self.context.Unmap(&staging_resource, 0);

                // 10. 计算捕获耗时
                let capture_time_us = start.elapsed().as_micros() as u64;

                debug!(
                    "DXGI 捕获完成: {}x{}, 耗时: {}μs ({}ms)",
                    width, height, capture_time_us, capture_time_us / 1000
                );

                // 返回捕获帧数据
                Ok(CaptureFrame {
                    data: self.buffer[..total_size].to_vec(),
                    width,
                    height,
                    capture_time_us,
                })
            }
        }

        /// 捕获指定区域的原始 BGRA 帧数据（不做颜色转换）
    ///
    /// 专为录屏优化：
    /// - 不做 BGRA→RGBA 转换（FFmpeg 直接接受 BGRA）
    /// - 只读取指定区域的像素（不读全屏再裁剪）
    /// - 将数据写入调用者提供的缓冲区（避免分配）
    ///
    /// # 参数
    /// - `region`: 要捕获的区域 (x, y, width, height)，物理像素坐标
    /// - `buffer`: 输出缓冲区，大小至少为 width * height * 4
    ///
    /// # 返回
    /// 成功返回实际写入的字节数，失败返回错误
    pub fn capture_region_bgra(
        &mut self,
        region: Option<(u32, u32, u32, u32)>,
        buffer: &mut Vec<u8>,
    ) -> HuGeResult<(u32, u32, u64)> {
        let start = Instant::now();

        unsafe {
            // 1. AcquireNextFrame
            let mut frame_info = DXGI_OUTDUPL_FRAME_INFO::default();
            let mut desktop_resource: Option<IDXGIResource> = None;

            match self.duplication.AcquireNextFrame(
                self.config.timeout_ms,
                &mut frame_info,
                &mut desktop_resource,
            ) {
                Ok(_) => {}
                Err(e) => {
                    let code = e.code();
                    if code == DXGI_ERROR_WAIT_TIMEOUT {
                        return Err(HuGeError::CaptureError("捕获超时：屏幕无变化".to_string()));
                    } else if code == DXGI_ERROR_ACCESS_LOST {
                        return Err(HuGeError::CaptureError("DXGI 访问丢失 (ACCESS_LOST)，需要重新初始化".to_string()));
                    } else {
                        return Err(HuGeError::CaptureError(format!("AcquireNextFrame 失败: {:?}", e)));
                    }
                }
            }

            // 2. 获取桌面纹理
            let desktop_resource = desktop_resource.ok_or_else(|| {
                HuGeError::CaptureError("桌面资源为空".to_string())
            })?;

            let desktop_texture: ID3D11Texture2D = match desktop_resource.cast() {
                Ok(tex) => tex,
                Err(e) => {
                    let _ = self.duplication.ReleaseFrame();
                    return Err(HuGeError::CaptureError(format!("转换纹理失败: {:?}", e)));
                }
            };

            // 3. 获取纹理描述
            let mut desc = D3D11_TEXTURE2D_DESC::default();
            desktop_texture.GetDesc(&mut desc);

            let full_width = desc.Width;
            let full_height = desc.Height;

            // 确定读取区域
            let (rx, ry, rw, rh) = match region {
                Some((x, y, w, h)) => {
                    let x = x.min(full_width);
                    let y = y.min(full_height);
                    let w = w.min(full_width.saturating_sub(x)) & !1; // 偶数
                    let h = h.min(full_height.saturating_sub(y)) & !1;
                    (x, y, w, h)
                }
                None => (0, 0, full_width & !1, full_height & !1),
            };

            if rw == 0 || rh == 0 {
                let _ = self.duplication.ReleaseFrame();
                return Err(HuGeError::CaptureError("区域尺寸为零".to_string()));
            }

            // 4. 先释放桌面帧（避免后续借用冲突）
            // 在释放之前，先把桌面纹理转为 ID3D11Resource
            let desktop_resource: ID3D11Resource = match desktop_texture.cast() {
                Ok(r) => r,
                Err(e) => {
                    let _ = self.duplication.ReleaseFrame();
                    return Err(HuGeError::CaptureError(format!("转换资源失败: {:?}", e)));
                }
            };

            let _ = self.duplication.ReleaseFrame();

            // 5. 获取或创建 Staging 纹理（此时帧已释放，无借用冲突）
            let staging_texture = self.get_or_create_staging_texture(full_width, full_height)?;
            let staging_resource: ID3D11Resource = staging_texture.cast()
                .map_err(|e| HuGeError::CaptureError(format!("转换资源失败: {:?}", e)))?;

            // 6. 复制桌面纹理到 Staging
            self.context.CopyResource(&staging_resource, &desktop_resource);

            // 7. Map Staging 纹理
            let mut mapped_resource = D3D11_MAPPED_SUBRESOURCE::default();
            if let Err(e) = self.context.Map(
                &staging_resource, 0, D3D11_MAP_READ, 0, Some(&mut mapped_resource),
            ) {
                return Err(HuGeError::CaptureError(format!("Map 失败: {:?}", e)));
            }

            // 8. 只读取指定区域的 BGRA 数据（不做颜色转换！）
            let row_pitch = mapped_resource.RowPitch as usize;
            let dst_stride = (rw * 4) as usize;
            let total_size = dst_stride * rh as usize;

            buffer.resize(total_size, 0);

            let src_ptr = mapped_resource.pData as *const u8;

            for y in 0..rh as usize {
                let src_offset = (ry as usize + y) * row_pitch + rx as usize * 4;
                let dst_offset = y * dst_stride;

                std::ptr::copy_nonoverlapping(
                    src_ptr.add(src_offset),
                    buffer.as_mut_ptr().add(dst_offset),
                    dst_stride,
                );
            }

            // 9. Unmap
            self.context.Unmap(&staging_resource, 0);

            let capture_time_us = start.elapsed().as_micros() as u64;

            Ok((rw, rh, capture_time_us))
        }
    }
    }

    /// BGRA 到 RGBA 通道转换（独立函数，用于测试）
    ///
    /// # 参数
    ///
    /// - `bgra`: BGRA 格式的像素数据（长度必须是 4 的倍数）
    ///
    /// # 返回
    ///
    /// RGBA 格式的像素数据
    ///
    /// # 转换规则
    ///
    /// - `rgba[0] = bgra[2]` (R = B)
    /// - `rgba[1] = bgra[1]` (G = G)
    /// - `rgba[2] = bgra[0]` (B = R)
    /// - `rgba[3] = bgra[3]` (A = A)
    ///
    /// # 示例
    ///
    /// ```ignore
    /// let bgra = vec![255, 128, 64, 255]; // B=255, G=128, R=64, A=255
    /// let rgba = bgra_to_rgba(&bgra);
    /// assert_eq!(rgba, vec![64, 128, 255, 255]); // R=64, G=128, B=255, A=255
    /// ```
    pub fn bgra_to_rgba(bgra: &[u8]) -> Vec<u8> {
        let pixel_count = bgra.len() / 4;
        let mut rgba = vec![0u8; bgra.len()];

        for i in 0..pixel_count {
            let src_idx = i * 4;
            let dst_idx = i * 4;

            // BGRA -> RGBA 转换
            // src: [B, G, R, A] at indices [0, 1, 2, 3]
            // dst: [R, G, B, A] at indices [0, 1, 2, 3]
            rgba[dst_idx] = bgra[src_idx + 2];     // R = B位置的值
            rgba[dst_idx + 1] = bgra[src_idx + 1]; // G = G位置的值
            rgba[dst_idx + 2] = bgra[src_idx];     // B = R位置的值
            rgba[dst_idx + 3] = bgra[src_idx + 3]; // A = A位置的值
        }

        rgba
    }

    /// BGRA 到 RGBA 单像素转换（用于测试）
    ///
    /// # 参数
    ///
    /// - `bgra`: 单个像素的 BGRA 值 [B, G, R, A]
    ///
    /// # 返回
    ///
    /// 单个像素的 RGBA 值 [R, G, B, A]
    #[inline]
    pub fn bgra_pixel_to_rgba(bgra: [u8; 4]) -> [u8; 4] {
        [bgra[2], bgra[1], bgra[0], bgra[3]]
    }

    /// DXGI 捕获错误类型
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum DxgiCaptureError {
        /// 访问丢失，需要重新初始化
        AccessLost,
        /// 设备已移除，需要重新创建设备
        DeviceRemoved,
        /// 捕获超时（屏幕无变化）
        Timeout,
        /// 初始化失败
        InitializationFailed,
        /// 其他错误
        Other,
    }

    impl DxgiCaptureError {
        /// 从错误消息解析错误类型
        pub fn from_error_message(msg: &str) -> Self {
            if msg.contains("ACCESS_LOST") || msg.contains("访问丢失") {
                Self::AccessLost
            } else if msg.contains("DEVICE_REMOVED") || msg.contains("设备已移除") {
                Self::DeviceRemoved
            } else if msg.contains("超时") || msg.contains("TIMEOUT") {
                Self::Timeout
            } else if msg.contains("初始化") || msg.contains("创建") {
                Self::InitializationFailed
            } else {
                Self::Other
            }
        }

        /// 是否是可恢复的错误（可以通过重新初始化恢复）
        pub fn is_recoverable(&self) -> bool {
            matches!(self, Self::AccessLost | Self::DeviceRemoved)
        }
    }
}

// ============================================================================
// 带回退的截图捕获（DXGI 优先，失败时回退到 screenshots-rs）
// ============================================================================

/// 使用 WGC/DXGI 捕获单个屏幕，失败时回退到 screenshots-rs
///
/// # 参数
///
/// - `screen`: 目标屏幕
///
/// # 返回
///
/// 返回 `CaptureResult`，包含截图文件路径和元数据
///
/// # 回退策略
///
/// 1. 首先尝试使用 WGC (Windows Graphics Capture)（推荐，快速稳定）
/// 2. 如果 WGC 失败，回退到 screenshots-rs (GDI)
///
/// 注：DXGI 引擎仅保留给录屏模块使用
#[cfg(windows)]
fn capture_single_screen_with_fallback(screen: &Screen) -> HuGeResult<CaptureResult> {
    use super::wgc_capture;
    
    let display_info = &screen.display_info;
    let monitor_id = display_info.id;
    let start = Instant::now();

    info!(
        "开始捕获显示器 {} (WGC 优先): {}x{} @ ({}, {}), DPR: {}",
        monitor_id,
        display_info.width,
        display_info.height,
        display_info.x,
        display_info.y,
        display_info.scale_factor
    );

    // === 策略 1：尝试 WGC ===
    let wgc_result = wgc_capture::capture_monitor_wgc(
        display_info.x,
        display_info.y,
        display_info.width,
        display_info.height,
    );

    match wgc_result {
        Ok(wgc_frame) => {
            let capture_time_ms = start.elapsed().as_millis() as u64;
            
            // 保存 WGC 捕获的 BGRA 数据为 PNG 文件
            let path = generate_temp_path(monitor_id)?;
            
            // BGRA → RGBA 转换
            let mut rgba_data = wgc_frame.data;
            for chunk in rgba_data.chunks_exact_mut(4) {
                chunk.swap(0, 2); // B ↔ R
            }
            
            let image = image::RgbaImage::from_raw(wgc_frame.width, wgc_frame.height, rgba_data)
                .ok_or_else(|| HuGeError::CaptureError("WGC: 无法创建图像".to_string()))?;
            
            // 【性能优化】使用快速 PNG 编码 + 内存哈希（避免重读文件）
            let (file_size_u64, image_hash) = save_png_fast_with_hash(&image, &path)?;
            let file_size = Some(file_size_u64 as i64);
            
            info!(
                "显示器 {} WGC 截图完成: {:?}, 尺寸: {}x{}, 耗时: {}ms",
                monitor_id, path, wgc_frame.width, wgc_frame.height, capture_time_ms
            );

            if capture_time_ms > 100 {
                warn!("显示器 {} 截图耗时 {}ms，超过 100ms 阈值", monitor_id, capture_time_ms);
            }

            return Ok(CaptureResult {
                path: path.to_string_lossy().to_string(),
                width: wgc_frame.width,
                height: wgc_frame.height,
                dpr: display_info.scale_factor as f64,
                x: display_info.x,
                y: display_info.y,
                monitor_id,
                image_hash,
                file_size,
                capture_time_ms: Some(capture_time_ms),
                capture_engine: Some("wgc".to_string()),
            });
        }
        Err(wgc_err) => {
            warn!("显示器 {} WGC 捕获失败，回退到 DXGI: {}", monitor_id, wgc_err);
        }
    }

    // === 策略 2：直接回退到 screenshots-rs (GDI) ===
    // 注：DXGI 引擎仅保留给录屏模块使用
    warn!("显示器 {} 回退到 screenshots-rs (GDI)", monitor_id);
    capture_single_screen(screen)
}

/// 截图捕获结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureResult {
    /// 临时文件路径（可通过 asset:// 协议访问）
    pub path: String,
    /// 图像宽度（物理像素）
    pub width: u32,
    /// 图像高度（物理像素）
    pub height: u32,
    /// 设备像素比 (DPR)
    pub dpr: f64,
    /// 显示器 ID
    pub monitor_id: u32,
    /// 显示器 X 坐标（虚拟屏幕坐标系，可能为负）
    pub x: i32,
    /// 显示器 Y 坐标（虚拟屏幕坐标系，可能为负）
    pub y: i32,
    /// 图片 MD5 哈希（用于去重）
    pub image_hash: Option<String>,
    /// 文件大小（字节）
    pub file_size: Option<i64>,
    /// 捕获耗时（毫秒）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_time_ms: Option<u64>,
    /// 使用的捕获引擎（"dxgi" 或 "screenshots-rs"）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_engine: Option<String>,
}

// Re-export DXGI types for external use
#[cfg(windows)]
pub use dxgi::{DxgiCaptureConfig, DxgiCaptureEngine, CaptureFrame, DxgiCaptureError, bgra_to_rgba, bgra_pixel_to_rgba};

// 注意：capture_region_bgra 是 DxgiCaptureEngine 的方法，通过 DxgiCaptureEngine 类型自动可用

/// 矩形区域
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// 生成唯一的临时文件路径
///
/// 文件保存在系统临时目录的 `hugescreenshot` 子目录下
/// 公开版本的 generate_temp_path（供 screenshot_cmd 调用）
pub fn generate_temp_path_pub(monitor_id: u32) -> HuGeResult<PathBuf> {
    generate_temp_path(monitor_id)
}

fn generate_temp_path(monitor_id: u32) -> HuGeResult<PathBuf> {
    let temp_dir = env::temp_dir().join("hugescreenshot");

    // 确保目录存在
    if !temp_dir.exists() {
        std::fs::create_dir_all(&temp_dir).map_err(|e| {
            error!("创建临时目录失败: {:?}, 错误: {}", temp_dir, e);
            HuGeError::FileError(e)
        })?;
    }

    // 生成唯一文件名：screenshot_{monitor_id}_{timestamp}.png
    // 使用较短的时间戳格式减少路径长度
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let filename = format!("s{}_{}.png", monitor_id, timestamp);
    let path = temp_dir.join(filename);

    debug!("生成临时文件路径: {:?}", path);
    Ok(path)
}

/// 获取所有屏幕信息
pub fn get_all_screens() -> HuGeResult<Vec<Screen>> {
    Screen::all().map_err(|e| {
        error!("获取屏幕列表失败: {}", e);
        HuGeError::CaptureError(format!("获取屏幕列表失败: {}", e))
    })
}

/// 捕获单个屏幕并保存到文件
fn capture_single_screen(screen: &Screen) -> HuGeResult<CaptureResult> {
    let display_info = &screen.display_info;
    let monitor_id = display_info.id;
    let start = Instant::now();

    info!(
        "开始捕获显示器 {}: {}x{} @ ({}, {}), DPR: {}",
        monitor_id,
        display_info.width,
        display_info.height,
        display_info.x,
        display_info.y,
        display_info.scale_factor
    );

    // 捕获屏幕
    let image = screen.capture().map_err(|e| {
        error!("捕获显示器 {} 失败: {}", monitor_id, e);
        HuGeError::CaptureError(format!("捕获显示器 {} 失败: {}", monitor_id, e))
    })?;

    // 生成临时文件路径
    let path = generate_temp_path(monitor_id)?;

    // 获取实际图像尺寸（物理像素）
    let (width, height) = (image.width(), image.height());

    // 【性能优化】使用快速 PNG 编码 + 内存哈希
    // screenshots crate 使用 image 0.24，需要通过 raw bytes 桥接到 image 0.25
    let rgba_image = image::RgbaImage::from_raw(width, height, image.into_raw())
        .ok_or_else(|| HuGeError::CaptureError("无法转换截图数据到 RgbaImage".to_string()))?;
    let (file_size_u64, image_hash) = save_png_fast_with_hash(&rgba_image, &path)?;
    let file_size = Some(file_size_u64 as i64);

    let capture_time_ms = start.elapsed().as_millis() as u64;

    info!(
        "显示器 {} 截图完成: {:?}, 尺寸: {}x{}, 哈希: {:?}, 耗时: {}ms",
        monitor_id, path, width, height, image_hash, capture_time_ms
    );

    // 性能警告：超过 100ms 记录警告
    if capture_time_ms > 100 {
        warn!(
            "显示器 {} 截图耗时 {}ms，超过 100ms 阈值",
            monitor_id, capture_time_ms
        );
    }

    Ok(CaptureResult {
        path: path.to_string_lossy().to_string(),
        width,
        height,
        dpr: display_info.scale_factor as f64,
        monitor_id,
        x: display_info.x,
        y: display_info.y,
        image_hash,
        file_size,
        capture_time_ms: Some(capture_time_ms),
        capture_engine: Some("screenshots-rs".to_string()),
    })
}

/// 捕获指定显示器的屏幕
///
/// # 参数
///
/// - `monitor`: 显示器 ID，None 表示主显示器（ID 最小的显示器）
///
/// # 返回
///
/// 返回 `CaptureResult`，包含截图文件路径和元数据
///
/// # 示例
///
/// ```ignore
/// let result = capture_screen(Some(0)).await?;
/// println!("截图保存到: {}", result.path);
/// ```
///
/// # 注意事项
///
/// - 返回的 `path` 是绝对路径，前端需要使用 `convertFileSrc()` 转换为 asset:// URL
/// - `width` 和 `height` 是物理像素尺寸
/// - `dpr` 是设备像素比，用于逻辑坐标和物理像素的转换
/// - `x` 和 `y` 是显示器在虚拟屏幕坐标系中的位置，可能为负值
///
/// # 捕获引擎
///
/// - **Windows**: 优先使用 WGC (Windows Graphics Capture)，失败时回退到 screenshots-rs (GDI)
/// - **其他平台**: 使用 screenshots-rs
/// - **注意**: DXGI 引擎仅用于录屏模块，截图不使用
///
/// **Validates: Requirements 5.2, 5.5**
#[tauri::command]
pub async fn capture_screen(monitor: Option<u32>) -> HuGeResult<CaptureResult> {
    let screens = get_all_screens()?;

    if screens.is_empty() {
        return Err(HuGeError::CaptureError("未检测到任何显示器".to_string()));
    }

    // 查找目标显示器
    let target_screen = match monitor {
        Some(id) => {
            // 优先按原生显示器 ID 查找
            if let Some(screen) = screens.iter().find(|s| s.display_info.id == id) {
                screen
            } else {
                // 回退：按索引查找（overlay 传入的是索引而非原生 ID）
                warn!(
                    "未找到原生显示器 ID: {}，尝试按索引查找（共 {} 个显示器）",
                    id,
                    screens.len()
                );
                screens.get(id as usize).ok_or_else(|| {
                    error!("显示器索引 {} 超出范围（共 {} 个显示器）", id, screens.len());
                    HuGeError::CaptureError(format!(
                        "未找到显示器 ID: {}，索引也超出范围（共 {} 个显示器）",
                        id, screens.len()
                    ))
                })?
            }
        }
        None => {
            // 使用主显示器（is_primary 为 true 的，或者 ID 最小的）
            screens
                .iter()
                .find(|s| s.display_info.is_primary)
                .or_else(|| screens.iter().min_by_key(|s| s.display_info.id))
                .ok_or_else(|| {
                    error!("未找到主显示器");
                    HuGeError::CaptureError("未找到主显示器".to_string())
                })?
        }
    };

    // Windows: 使用 DXGI 优先的捕获策略（Requirements 5.2）
    // 其他平台: 使用 screenshots-rs
    #[cfg(windows)]
    {
        capture_single_screen_with_fallback(target_screen)
    }
    
    #[cfg(not(windows))]
    {
        capture_single_screen(target_screen)
    }
}

/// 同步版本的 `capture_screen`（供 `spawn_blocking` 调用）
///
/// 与 `capture_screen` 逻辑完全相同，但是同步函数，
/// 可以在 `tokio::task::spawn_blocking` 中安全调用。
pub fn capture_screen_sync(monitor: Option<u32>) -> HuGeResult<CaptureResult> {
    let screens = get_all_screens()?;

    if screens.is_empty() {
        return Err(HuGeError::CaptureError("未检测到任何显示器".to_string()));
    }

    let target_screen = match monitor {
        Some(id) => {
            if let Some(screen) = screens.iter().find(|s| s.display_info.id == id) {
                screen
            } else {
                warn!("未找到原生显示器 ID: {}，尝试按索引查找", id);
                screens.get(id as usize).ok_or_else(|| {
                    HuGeError::CaptureError(format!("未找到显示器 ID: {}", id))
                })?
            }
        }
        None => {
            screens
                .iter()
                .find(|s| s.display_info.is_primary)
                .or_else(|| screens.iter().min_by_key(|s| s.display_info.id))
                .ok_or_else(|| HuGeError::CaptureError("未找到主显示器".to_string()))?
        }
    };

    #[cfg(windows)]
    {
        capture_single_screen_with_fallback(target_screen)
    }
    
    #[cfg(not(windows))]
    {
        capture_single_screen(target_screen)
    }
}

/// 捕获所有显示器的屏幕
///
/// # 返回
///
/// 返回所有显示器的 `CaptureResult` 列表，按显示器 ID 排序
///
/// # 示例
///
/// ```ignore
/// let results = capture_all_monitors().await?;
/// for result in results {
///     println!("显示器 {}: {}x{} @ ({}, {})",
///         result.monitor_id, result.width, result.height, result.x, result.y);
/// }
/// ```
///
/// # 注意事项
///
/// - 多显示器场景下，副屏可能有负坐标（位于主屏左侧时）
/// - 每个显示器的 DPR 可能不同，需要分别处理
/// - 返回结果按 monitor_id 排序，便于前端处理
///
/// # 捕获引擎
///
/// - **Windows**: 优先使用 DXGI Desktop Duplication API（高性能），失败时回退到 screenshots-rs
/// - **其他平台**: 使用 screenshots-rs
///
/// **Validates: Requirements 5.2, 5.5**
#[tauri::command]
pub async fn capture_all_monitors() -> HuGeResult<Vec<CaptureResult>> {
    let screens = get_all_screens()?;

    if screens.is_empty() {
        return Err(HuGeError::CaptureError("未检测到任何显示器".to_string()));
    }

    info!("开始捕获所有显示器，共 {} 个", screens.len());

    let mut results = Vec::with_capacity(screens.len());

    for screen in &screens {
        // Windows: 使用 DXGI 优先的捕获策略（Requirements 5.2）
        // 其他平台: 使用 screenshots-rs
        #[cfg(windows)]
        let capture_result = capture_single_screen_with_fallback(screen);
        
        #[cfg(not(windows))]
        let capture_result = capture_single_screen(screen);
        
        match capture_result {
            Ok(result) => results.push(result),
            Err(e) => {
                // 单个显示器失败不影响其他显示器
                error!(
                    "捕获显示器 {} 失败，跳过: {}",
                    screen.display_info.id, e
                );
            }
        }
    }

    if results.is_empty() {
        return Err(HuGeError::CaptureError(
            "所有显示器捕获均失败".to_string(),
        ));
    }

    // 按 monitor_id 排序
    results.sort_by_key(|r| r.monitor_id);

    info!("所有显示器截图完成，成功 {} 个", results.len());

    Ok(results)
}

/// 获取所有显示器信息（不截图）
///
/// 用于前端获取显示器布局信息
#[tauri::command]
pub async fn get_screen_info() -> HuGeResult<Vec<ScreenInfo>> {
    let screens = get_all_screens()?;

    let infos: Vec<ScreenInfo> = screens
        .iter()
        .map(|s| {
            let di = &s.display_info;
            ScreenInfo {
                id: di.id,
                x: di.x,
                y: di.y,
                width: di.width,
                height: di.height,
                scale_factor: di.scale_factor as f64,
                is_primary: di.is_primary,
            }
        })
        .collect();

    Ok(infos)
}

/// 显示器信息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenInfo {
    /// 显示器 ID
    pub id: u32,
    /// X 坐标（虚拟屏幕坐标系）
    pub x: i32,
    /// Y 坐标（虚拟屏幕坐标系）
    pub y: i32,
    /// 宽度（物理像素）
    pub width: u32,
    /// 高度（物理像素）
    pub height: u32,
    /// 缩放因子 (DPR)
    pub scale_factor: f64,
    /// 是否为主显示器
    pub is_primary: bool,
}

/// 捕获屏幕的指定区域
///
/// # 参数
///
/// - `rect`: 截取区域（物理像素坐标，虚拟屏幕坐标系）
///
/// # 返回
///
/// 返回 `CaptureResult`，包含截图文件路径和元数据
///
/// # 注意事项
///
/// - 区域坐标是虚拟屏幕坐标系（可能跨多显示器）
/// - 目前只支持单显示器内的区域截图
/// - 跨显示器的区域会被裁剪到主要显示器范围内
pub fn capture_region_impl(rect: &Rect) -> HuGeResult<CaptureResult> {
    let start = Instant::now();
    
    info!(
        "开始区域截图: ({}, {}) {}x{}",
        rect.x, rect.y, rect.width, rect.height
    );

    // 验证区域尺寸
    if rect.width == 0 || rect.height == 0 {
        return Err(HuGeError::CaptureError("截图区域尺寸不能为零".to_string()));
    }

    let screens = get_all_screens()?;

    if screens.is_empty() {
        return Err(HuGeError::CaptureError("未检测到任何显示器".to_string()));
    }

    // 查找区域中心点所在的显示器
    let center_x = rect.x + (rect.width as i32 / 2);
    let center_y = rect.y + (rect.height as i32 / 2);

    let target_screen = screens
        .iter()
        .find(|s| {
            let di = &s.display_info;
            center_x >= di.x
                && center_x < di.x + di.width as i32
                && center_y >= di.y
                && center_y < di.y + di.height as i32
        })
        .or_else(|| {
            // 如果中心点不在任何显示器上，尝试用左上角
            screens.iter().find(|s| {
                let di = &s.display_info;
                rect.x >= di.x
                    && rect.x < di.x + di.width as i32
                    && rect.y >= di.y
                    && rect.y < di.y + di.height as i32
            })
        })
        .or_else(|| {
            // 最后使用主显示器
            screens.iter().find(|s| s.display_info.is_primary)
        })
        .or_else(|| screens.first())
        .ok_or_else(|| HuGeError::CaptureError("无法确定目标显示器".to_string()))?;

    let display_info = &target_screen.display_info;

    // 计算区域相对于显示器的本地坐标
    let local_x = rect.x - display_info.x;
    let local_y = rect.y - display_info.y;

    // 确保坐标在显示器范围内（裁剪）
    let clipped_x = local_x.max(0);
    let clipped_y = local_y.max(0);

    // 计算裁剪后的宽高
    let available_width = (display_info.width as i32 - clipped_x).max(0) as u32;
    let available_height = (display_info.height as i32 - clipped_y).max(0) as u32;

    let final_width = rect.width.min(available_width);
    let final_height = rect.height.min(available_height);

    if final_width == 0 || final_height == 0 {
        return Err(HuGeError::CaptureError(
            "截图区域完全位于显示器范围外".to_string(),
        ));
    }

    debug!(
        "区域截图本地坐标: ({}, {}) {}x{} 在显示器 {}",
        clipped_x, clipped_y, final_width, final_height, display_info.id
    );

    // 使用 capture_area 截取指定区域
    let image = target_screen
        .capture_area(clipped_x, clipped_y, final_width, final_height)
        .map_err(|e| {
            error!("区域截图失败: {}", e);
            HuGeError::CaptureError(format!("区域截图失败: {}", e))
        })?;

    // 生成临时文件路径
    let path = generate_temp_path(display_info.id)?;

    let (width, height) = (image.width(), image.height());

    // 【性能优化】使用快速 PNG 编码 + 内存哈希
    // screenshots crate 使用 image 0.24，需要通过 raw bytes 桥接到 image 0.25
    let rgba_image = image::RgbaImage::from_raw(width, height, image.into_raw())
        .ok_or_else(|| HuGeError::CaptureError("无法转换截图数据到 RgbaImage".to_string()))?;
    let (file_size_u64, image_hash) = save_png_fast_with_hash(&rgba_image, &path)?;
    let file_size = Some(file_size_u64 as i64);

    let capture_time_ms = start.elapsed().as_millis() as u64;

    info!(
        "区域截图完成: {:?}, 尺寸: {}x{}, 哈希: {:?}, 耗时: {}ms",
        path, width, height, image_hash, capture_time_ms
    );

    // 性能警告：超过 100ms 记录警告
    if capture_time_ms > 100 {
        warn!(
            "区域截图耗时 {}ms，超过 100ms 阈值",
            capture_time_ms
        );
    }

    Ok(CaptureResult {
        path: path.to_string_lossy().to_string(),
        width,
        height,
        dpr: display_info.scale_factor as f64,
        monitor_id: display_info.id,
        x: display_info.x + clipped_x,
        y: display_info.y + clipped_y,
        image_hash,
        file_size,
        capture_time_ms: Some(capture_time_ms),
        capture_engine: Some("screenshots-rs".to_string()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capture_result_serialize() {
        let result = CaptureResult {
            path: "/tmp/screenshot.png".to_string(),
            width: 1920,
            height: 1080,
            dpr: 1.5,
            monitor_id: 0,
            x: 0,
            y: 0,
            image_hash: Some("abc123def456".to_string()),
            file_size: Some(12345),
            capture_time_ms: Some(42),
            capture_engine: Some("screenshots-rs".to_string()),
        };
        let json = serde_json::to_string(&result).expect("CaptureResult 序列化失败");
        assert!(json.contains("1920"));
        assert!(json.contains("1.5"));
        // 注意：使用 camelCase 序列化
        assert!(json.contains("monitorId"));
        assert!(json.contains("imageHash"));
        assert!(json.contains("abc123def456"));
        assert!(json.contains("captureTimeMs"));
        assert!(json.contains("captureEngine"));
    }

    #[test]
    fn test_capture_result_serialize_without_optional_fields() {
        // 测试可选字段为 None 时不序列化
        let result = CaptureResult {
            path: "/tmp/screenshot.png".to_string(),
            width: 1920,
            height: 1080,
            dpr: 1.5,
            monitor_id: 0,
            x: 0,
            y: 0,
            image_hash: None,
            file_size: None,
            capture_time_ms: None,
            capture_engine: None,
        };
        let json = serde_json::to_string(&result).expect("CaptureResult 序列化失败");
        // 注意：使用 camelCase 序列化
        assert!(!json.contains("captureTimeMs"));
        assert!(!json.contains("captureEngine"));
    }

    #[test]
    fn test_rect_serialize() {
        let rect = Rect {
            x: 100,
            y: 200,
            width: 800,
            height: 600,
        };
        let json = serde_json::to_string(&rect).expect("Rect 序列化失败");
        assert!(json.contains("100"));
        assert!(json.contains("800"));
    }

    #[test]
    fn test_screen_info_serialize() {
        let info = ScreenInfo {
            id: 0,
            x: -1920,
            y: 0,
            width: 1920,
            height: 1080,
            scale_factor: 1.25,
            is_primary: false,
        };
        let json = serde_json::to_string(&info).expect("ScreenInfo 序列化失败");
        assert!(json.contains("-1920")); // 负坐标
        assert!(json.contains("1.25"));
        // 注意：使用 camelCase 序列化
        assert!(json.contains("isPrimary"));
        assert!(json.contains("scaleFactor"));
    }

    #[test]
    fn test_generate_temp_path() {
        let path = generate_temp_path(0).unwrap();
        assert!(path.to_string_lossy().contains("hugescreenshot"));
        // 使用较短的文件名格式：s{monitor_id}_{timestamp}.png
        assert!(path.to_string_lossy().contains("s0_"));
        assert!(path.to_string_lossy().ends_with(".png"));
    }

    #[test]
    fn test_capture_result_with_negative_coords() {
        // 测试副屏在主屏左侧的场景
        let result = CaptureResult {
            path: "/tmp/screenshot.png".to_string(),
            width: 2560,
            height: 1440,
            dpr: 1.5,
            monitor_id: 1,
            x: -2560, // 副屏在主屏左侧
            y: 0,
            image_hash: None,
            file_size: None,
            capture_time_ms: Some(35),
            capture_engine: Some("dxgi".to_string()),
        };
        let json = serde_json::to_string(&result).expect("CaptureResult 序列化失败");
        assert!(json.contains("-2560"));
        assert!(json.contains("dxgi"));
    }

    #[test]
    fn test_capture_region_zero_size() {
        // 测试零尺寸区域应该返回错误
        let rect = Rect {
            x: 0,
            y: 0,
            width: 0,
            height: 100,
        };
        let result = capture_region_impl(&rect);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("尺寸不能为零"));
    }

    #[test]
    fn test_capture_region_valid() {
        // 测试有效的区域截图（实际截图，需要显示器）
        let rect = Rect {
            x: 0,
            y: 0,
            width: 100,
            height: 100,
        };
        // 这个测试可能因为无头环境失败，所以我们只检查它不会 panic
        let result = capture_region_impl(&rect);
        // 在 CI 环境可能没有显示器，所以不强制要求成功
        if let Ok(capture) = result {
            assert!(capture.width > 0);
            assert!(capture.height > 0);
            assert!(!capture.path.is_empty());
            assert!(capture.capture_time_ms.is_some());
            assert_eq!(capture.capture_engine, Some("screenshots-rs".to_string()));
        }
    }

    #[cfg(windows)]
    mod dxgi_tests {
        use super::super::dxgi::*;

        #[test]
        fn test_dxgi_capture_config_default() {
            let config = DxgiCaptureConfig::default();
            assert_eq!(config.timeout_ms, 100);
            assert!(!config.include_cursor);
            // 默认 4K 缓冲区大小
            assert_eq!(config.buffer_size, 3840 * 2160 * 4);
        }

        #[test]
        fn test_dxgi_capture_config_custom() {
            let config = DxgiCaptureConfig {
                timeout_ms: 50,
                include_cursor: true,
                buffer_size: 1920 * 1080 * 4,
            };
            assert_eq!(config.timeout_ms, 50);
            assert!(config.include_cursor);
            assert_eq!(config.buffer_size, 1920 * 1080 * 4);
        }

        #[test]
        fn test_dxgi_engine_creation() {
            // 测试 DXGI 引擎创建（可能因为无显示器而失败）
            let config = DxgiCaptureConfig::default();
            let result = DxgiCaptureEngine::new(0, 0, 0, 1920, 1080, config);
            
            // 在 CI 环境可能没有显示器，所以不强制要求成功
            match result {
                Ok(engine) => {
                    assert_eq!(engine.monitor_id(), 0);
                    assert!(engine.width() > 0);
                    assert!(engine.height() > 0);
                }
                Err(err) => {
                    // 确保错误信息有意义
                    println!("DXGI 引擎创建失败（预期在无显示器环境）: {}", err);
                }
            }
        }

        // ================================================================
        // BGRA 到 RGBA 转换测试
        // ================================================================

        #[test]
        fn test_bgra_pixel_to_rgba_basic() {
            // 测试基本的单像素转换
            // BGRA: [B=255, G=128, R=64, A=255]
            // RGBA: [R=64, G=128, B=255, A=255]
            let bgra = [255, 128, 64, 255];
            let rgba = bgra_pixel_to_rgba(bgra);
            assert_eq!(rgba, [64, 128, 255, 255]);
        }

        #[test]
        fn test_bgra_pixel_to_rgba_all_zeros() {
            // 测试全零像素
            let bgra = [0, 0, 0, 0];
            let rgba = bgra_pixel_to_rgba(bgra);
            assert_eq!(rgba, [0, 0, 0, 0]);
        }

        #[test]
        fn test_bgra_pixel_to_rgba_all_max() {
            // 测试全 255 像素
            let bgra = [255, 255, 255, 255];
            let rgba = bgra_pixel_to_rgba(bgra);
            assert_eq!(rgba, [255, 255, 255, 255]);
        }

        #[test]
        fn test_bgra_pixel_to_rgba_preserves_alpha() {
            // 测试 Alpha 通道保持不变
            let bgra = [100, 150, 200, 128]; // 半透明
            let rgba = bgra_pixel_to_rgba(bgra);
            assert_eq!(rgba[3], 128); // Alpha 不变
            assert_eq!(rgba[0], 200); // R = 原 B 位置的值
            assert_eq!(rgba[1], 150); // G = 原 G 位置的值
            assert_eq!(rgba[2], 100); // B = 原 R 位置的值
        }

        #[test]
        fn test_bgra_to_rgba_single_pixel() {
            // 测试单像素数组转换
            let bgra = vec![255, 128, 64, 255];
            let rgba = bgra_to_rgba(&bgra);
            assert_eq!(rgba, vec![64, 128, 255, 255]);
        }

        #[test]
        fn test_bgra_to_rgba_multiple_pixels() {
            // 测试多像素转换
            // 像素 1: BGRA [255, 0, 0, 255] -> RGBA [0, 0, 255, 255] (蓝色)
            // 像素 2: BGRA [0, 255, 0, 255] -> RGBA [0, 255, 0, 255] (绿色)
            // 像素 3: BGRA [0, 0, 255, 255] -> RGBA [255, 0, 0, 255] (红色)
            let bgra = vec![
                255, 0, 0, 255,   // 蓝色 (BGRA)
                0, 255, 0, 255,   // 绿色 (BGRA)
                0, 0, 255, 255,   // 红色 (BGRA)
            ];
            let rgba = bgra_to_rgba(&bgra);
            assert_eq!(rgba, vec![
                0, 0, 255, 255,   // 蓝色 (RGBA)
                0, 255, 0, 255,   // 绿色 (RGBA)
                255, 0, 0, 255,   // 红色 (RGBA)
            ]);
        }

        #[test]
        fn test_bgra_to_rgba_empty() {
            // 测试空数组
            let bgra: Vec<u8> = vec![];
            let rgba = bgra_to_rgba(&bgra);
            assert!(rgba.is_empty());
        }

        #[test]
        fn test_bgra_to_rgba_boundary_values() {
            // 测试边界值 (0 和 255)
            let bgra = vec![0, 0, 0, 0, 255, 255, 255, 255];
            let rgba = bgra_to_rgba(&bgra);
            assert_eq!(rgba, vec![0, 0, 0, 0, 255, 255, 255, 255]);
        }

        #[test]
        fn test_bgra_to_rgba_channel_mapping() {
            // 验证通道映射正确性
            // 设置每个通道为不同的值以验证映射
            // BGRA: [B=10, G=20, R=30, A=40]
            // RGBA: [R=30, G=20, B=10, A=40]
            let bgra = vec![10, 20, 30, 40];
            let rgba = bgra_to_rgba(&bgra);
            
            // 验证 Requirements 1.5 的转换规则：
            // rgba[0] = bgra[2] (R = B位置的值)
            assert_eq!(rgba[0], bgra[2], "R 通道应该等于 BGRA 的第 3 个字节");
            // rgba[1] = bgra[1] (G = G)
            assert_eq!(rgba[1], bgra[1], "G 通道应该保持不变");
            // rgba[2] = bgra[0] (B = R位置的值)
            assert_eq!(rgba[2], bgra[0], "B 通道应该等于 BGRA 的第 1 个字节");
            // rgba[3] = bgra[3] (A = A)
            assert_eq!(rgba[3], bgra[3], "A 通道应该保持不变");
        }

        // ================================================================
        // DXGI 错误类型测试（Requirements 1.3, 5.8）
        // ================================================================

        #[test]
        fn test_dxgi_capture_error_from_access_lost() {
            // 测试 ACCESS_LOST 错误识别
            let error = DxgiCaptureError::from_error_message("DXGI 访问丢失 (ACCESS_LOST)，需要重新初始化");
            assert_eq!(error, DxgiCaptureError::AccessLost);
            assert!(error.is_recoverable());
        }

        #[test]
        fn test_dxgi_capture_error_from_device_removed() {
            // 测试 DEVICE_REMOVED 错误识别
            let error = DxgiCaptureError::from_error_message("DXGI 设备已移除 (DEVICE_REMOVED)，需要重新初始化");
            assert_eq!(error, DxgiCaptureError::DeviceRemoved);
            assert!(error.is_recoverable());
        }

        #[test]
        fn test_dxgi_capture_error_from_timeout() {
            // 测试超时错误识别
            let error = DxgiCaptureError::from_error_message("捕获超时：屏幕无变化");
            assert_eq!(error, DxgiCaptureError::Timeout);
            assert!(!error.is_recoverable());
        }

        #[test]
        fn test_dxgi_capture_error_from_initialization_failed() {
            // 测试初始化失败错误识别
            let error = DxgiCaptureError::from_error_message("创建 DXGI Factory 失败");
            assert_eq!(error, DxgiCaptureError::InitializationFailed);
            assert!(!error.is_recoverable());
        }

        #[test]
        fn test_dxgi_capture_error_from_other() {
            // 测试其他错误识别
            let error = DxgiCaptureError::from_error_message("未知错误");
            assert_eq!(error, DxgiCaptureError::Other);
            assert!(!error.is_recoverable());
        }

        #[test]
        fn test_dxgi_capture_error_recoverable() {
            // 测试可恢复错误判断
            assert!(DxgiCaptureError::AccessLost.is_recoverable());
            assert!(DxgiCaptureError::DeviceRemoved.is_recoverable());
            assert!(!DxgiCaptureError::Timeout.is_recoverable());
            assert!(!DxgiCaptureError::InitializationFailed.is_recoverable());
            assert!(!DxgiCaptureError::Other.is_recoverable());
        }
    }

    // ========================================================================
    // 属性测试 (Property-Based Testing)
    // ========================================================================

    #[cfg(windows)]
    mod property_tests {
        use super::super::dxgi::*;
        use proptest::prelude::*;

        // ====================================================================
        // Feature: rust-performance-optimization
        // Property 2: BGRA 到 RGBA 通道转换正确性
        // Validates: Requirements 1.5
        // ====================================================================

        proptest! {
            #![proptest_config(ProptestConfig::with_cases(1000))]

            /// Property: BGRA 到 RGBA 转换是可逆的
            /// 
            /// 对于任意 BGRA 像素，转换为 RGBA 后再转换回 BGRA 应该得到原始值。
            /// 这验证了转换逻辑的正确性。
            #[test]
            fn prop_bgra_rgba_conversion_is_reversible(
                b in 0u8..=255,
                g in 0u8..=255,
                r in 0u8..=255,
                a in 0u8..=255,
            ) {
                let bgra = [b, g, r, a];
                let rgba = bgra_pixel_to_rgba(bgra);
                
                // RGBA 转回 BGRA（手动实现逆转换）
                let bgra_back = [rgba[2], rgba[1], rgba[0], rgba[3]];
                
                prop_assert_eq!(bgra, bgra_back, 
                    "BGRA->RGBA->BGRA 转换应该是可逆的");
            }

            /// Property: Alpha 通道在转换中保持不变
            /// 
            /// 对于任意 BGRA 像素，转换为 RGBA 后 Alpha 通道值应该保持不变。
            #[test]
            fn prop_alpha_channel_preserved(
                b in 0u8..=255,
                g in 0u8..=255,
                r in 0u8..=255,
                a in 0u8..=255,
            ) {
                let bgra = [b, g, r, a];
                let rgba = bgra_pixel_to_rgba(bgra);
                
                prop_assert_eq!(rgba[3], a, 
                    "Alpha 通道应该在转换中保持不变");
            }

            /// Property: Green 通道在转换中保持不变
            /// 
            /// 对于任意 BGRA 像素，转换为 RGBA 后 Green 通道值应该保持不变。
            #[test]
            fn prop_green_channel_preserved(
                b in 0u8..=255,
                g in 0u8..=255,
                r in 0u8..=255,
                a in 0u8..=255,
            ) {
                let bgra = [b, g, r, a];
                let rgba = bgra_pixel_to_rgba(bgra);
                
                prop_assert_eq!(rgba[1], g, 
                    "Green 通道应该在转换中保持不变");
            }

            /// Property: Red 和 Blue 通道正确交换
            /// 
            /// 对于任意 BGRA 像素，转换为 RGBA 后：
            /// - RGBA[0] (R) = BGRA[2] (原 R 位置)
            /// - RGBA[2] (B) = BGRA[0] (原 B 位置)
            #[test]
            fn prop_red_blue_channels_swapped(
                b in 0u8..=255,
                g in 0u8..=255,
                r in 0u8..=255,
                a in 0u8..=255,
            ) {
                let bgra = [b, g, r, a];
                let rgba = bgra_pixel_to_rgba(bgra);
                
                // BGRA 格式: [B, G, R, A] 索引 [0, 1, 2, 3]
                // RGBA 格式: [R, G, B, A] 索引 [0, 1, 2, 3]
                // 转换规则: rgba[0]=bgra[2], rgba[1]=bgra[1], rgba[2]=bgra[0], rgba[3]=bgra[3]
                prop_assert_eq!(rgba[0], r, 
                    "RGBA[0] (R) 应该等于 BGRA[2] (原 R 位置的值)");
                prop_assert_eq!(rgba[2], b, 
                    "RGBA[2] (B) 应该等于 BGRA[0] (原 B 位置的值)");
            }

            /// Property: 批量转换与单像素转换结果一致
            /// 
            /// 对于任意像素数组，使用 bgra_to_rgba 批量转换的结果
            /// 应该与逐个使用 bgra_pixel_to_rgba 转换的结果一致。
            #[test]
            fn prop_batch_conversion_matches_single(
                pixels in prop::collection::vec(
                    (0u8..=255, 0u8..=255, 0u8..=255, 0u8..=255),
                    0..100
                )
            ) {
                // 构建 BGRA 数组
                let bgra: Vec<u8> = pixels.iter()
                    .flat_map(|(b, g, r, a)| vec![*b, *g, *r, *a])
                    .collect();
                
                // 批量转换
                let rgba_batch = bgra_to_rgba(&bgra);
                
                // 逐个转换
                let rgba_single: Vec<u8> = pixels.iter()
                    .flat_map(|(b, g, r, a)| {
                        let pixel = bgra_pixel_to_rgba([*b, *g, *r, *a]);
                        vec![pixel[0], pixel[1], pixel[2], pixel[3]]
                    })
                    .collect();
                
                prop_assert_eq!(rgba_batch, rgba_single,
                    "批量转换结果应该与逐个转换结果一致");
            }

            /// Property: 转换后数组长度保持不变
            /// 
            /// 对于任意长度的 BGRA 数组，转换为 RGBA 后长度应该保持不变。
            #[test]
            fn prop_conversion_preserves_length(
                len in 0usize..1000
            ) {
                let bgra = vec![128u8; len * 4]; // 每个像素 4 字节
                let rgba = bgra_to_rgba(&bgra);
                
                prop_assert_eq!(rgba.len(), bgra.len(),
                    "转换后数组长度应该保持不变");
            }

            /// Property: 纯色图像转换正确性
            /// 
            /// 对于纯色图像（所有像素相同），转换后所有像素也应该相同。
            #[test]
            fn prop_solid_color_conversion(
                b in 0u8..=255,
                g in 0u8..=255,
                r in 0u8..=255,
                a in 0u8..=255,
                pixel_count in 1usize..100
            ) {
                // 创建纯色 BGRA 图像
                let bgra: Vec<u8> = (0..pixel_count)
                    .flat_map(|_| vec![b, g, r, a])
                    .collect();
                
                let rgba = bgra_to_rgba(&bgra);
                
                // 验证所有像素都相同
                let expected_rgba = bgra_pixel_to_rgba([b, g, r, a]);
                for i in 0..pixel_count {
                    let offset = i * 4;
                    prop_assert_eq!(rgba[offset], expected_rgba[0]);
                    prop_assert_eq!(rgba[offset + 1], expected_rgba[1]);
                    prop_assert_eq!(rgba[offset + 2], expected_rgba[2]);
                    prop_assert_eq!(rgba[offset + 3], expected_rgba[3]);
                }
            }
        }
    }
}
