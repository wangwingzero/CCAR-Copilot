//! Windows Graphics Capture (WGC) 截图引擎
//!
//! 使用 WGC API 实现高性能屏幕捕获，替代 DXGI Desktop Duplication。
//!
//! # 优势
//!
//! - 通过 HMONITOR 精确匹配显示器，无 ID 不一致问题
//! - 原生支持多显示器和 DPI 缩放
//! - 无需处理 DXGI_ERROR_ACCESS_LOST
//! - D3D11 设备缓存，重复截图极快
//!
//! # 要求
//!
//! - Windows 10 1903 (Build 18362) 或更高版本

use std::sync::{mpsc, Mutex, OnceLock};
use std::time::Duration;
use tracing::{debug, info};

use crate::error::{HuGeError, HuGeResult};

use windows::core::Interface;
use windows::Graphics::Capture::Direct3D11CaptureFramePool;
use windows::Graphics::Capture::GraphicsCaptureItem;
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_FEATURE_LEVEL_11_0};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
    D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
    D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::Graphics::Gdi::{HMONITOR, MonitorFromPoint, MONITOR_DEFAULTTONEAREST};
use windows::Win32::System::WinRT::Direct3D11::CreateDirect3D11DeviceFromDXGIDevice;
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Foundation::TypedEventHandler;

// ============================================================================
// D3D11 设备缓存（避免每次截图都重新创建）
// ============================================================================

struct CachedDevices {
    d3d_device: ID3D11Device,
    winrt_device: IDirect3DDevice,
    context: ID3D11DeviceContext,
}

// COM 对象是引用计数的，可以安全地跨线程传递
// D3D11 设备默认是多线程安全的
unsafe impl Send for CachedDevices {}
unsafe impl Sync for CachedDevices {}

// 全局缓存（线程安全）
static CACHED_DEVICES: OnceLock<Mutex<Option<CachedDevices>>> = OnceLock::new();

fn get_or_create_devices() -> HuGeResult<(ID3D11Device, IDirect3DDevice, ID3D11DeviceContext)> {
    let cache = CACHED_DEVICES.get_or_init(|| Mutex::new(None));
    let mut guard = cache.lock().map_err(|e| {
        HuGeError::CaptureError(format!("设备缓存锁获取失败: {}", e))
    })?;

    if let Some(ref cached) = *guard {
        debug!("WGC: 使用缓存的 D3D11 设备");
        return Ok((
            cached.d3d_device.clone(),
            cached.winrt_device.clone(),
            cached.context.clone(),
        ));
    }

    info!("WGC: 首次创建 D3D11 设备（将被缓存）");
    let (d3d_device, winrt_device) = create_d3d_devices()?;
    let context: ID3D11DeviceContext = unsafe {
        d3d_device.GetImmediateContext().map_err(|e| {
            HuGeError::CaptureError(format!("获取 D3D11 上下文失败: {:?}", e))
        })?
    };

    *guard = Some(CachedDevices {
        d3d_device: d3d_device.clone(),
        winrt_device: winrt_device.clone(),
        context: context.clone(),
    });

    Ok((d3d_device, winrt_device, context))
}

// ============================================================================
// 核心函数
// ============================================================================

/// WGC 截图结果
pub struct WgcCaptureResult {
    /// BGRA 像素数据
    pub data: Vec<u8>,
    /// 图像宽度
    pub width: u32,
    /// 图像高度
    pub height: u32,
}

/// 通过屏幕坐标获取 HMONITOR
fn get_hmonitor_from_position(x: i32, y: i32) -> HuGeResult<HMONITOR> {
    let point = POINT { x, y };
    let hmonitor = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) };
    if hmonitor.is_invalid() {
        return Err(HuGeError::CaptureError(format!(
            "无法获取坐标 ({}, {}) 对应的 HMONITOR", x, y
        )));
    }
    Ok(hmonitor)
}

/// 创建 D3D11 设备和 WinRT Direct3D 设备
fn create_d3d_devices() -> HuGeResult<(ID3D11Device, IDirect3DDevice)> {
    let mut d3d_device: Option<ID3D11Device> = None;

    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            None,
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            Some(&[D3D_FEATURE_LEVEL_11_0]),
            D3D11_SDK_VERSION,
            Some(&mut d3d_device),
            None,
            None,
        )
        .map_err(|e| HuGeError::CaptureError(format!("D3D11 设备创建失败: {:?}", e)))?;
    }

    let d3d_device = d3d_device.ok_or_else(|| {
        HuGeError::CaptureError("D3D11 设备为空".to_string())
    })?;

    let dxgi_device: IDXGIDevice = d3d_device.cast()
        .map_err(|e| HuGeError::CaptureError(format!("转换为 IDXGIDevice 失败: {:?}", e)))?;

    let winrt_device = unsafe {
        CreateDirect3D11DeviceFromDXGIDevice(&dxgi_device)
            .map_err(|e| HuGeError::CaptureError(format!("创建 WinRT D3D 设备失败: {:?}", e)))?
    };

    let winrt_d3d_device: IDirect3DDevice = winrt_device.cast()
        .map_err(|e| HuGeError::CaptureError(format!("转换为 IDirect3DDevice 失败: {:?}", e)))?;

    Ok((d3d_device, winrt_d3d_device))
}

/// 从 HMONITOR 创建 GraphicsCaptureItem
fn create_capture_item_for_monitor(hmonitor: HMONITOR) -> HuGeResult<GraphicsCaptureItem> {
    let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
        .map_err(|e| HuGeError::CaptureError(format!("获取 Interop 接口失败: {:?}", e)))?;

    let item: GraphicsCaptureItem = unsafe {
        interop.CreateForMonitor(hmonitor)
            .map_err(|e| HuGeError::CaptureError(format!("CreateForMonitor 失败: {:?}", e)))?
    };

    Ok(item)
}

/// 使用 WGC 捕获单帧屏幕截图
///
/// 使用缓存的 D3D11 设备，首次调用后性能显著提升。
pub fn capture_monitor_wgc(
    screen_x: i32,
    screen_y: i32,
    expected_width: u32,
    expected_height: u32,
) -> HuGeResult<WgcCaptureResult> {
    let total_start = std::time::Instant::now();
    let mut t = std::time::Instant::now();

    // 1. 获取 HMONITOR（< 1ms）
    let center_x = screen_x + (expected_width as i32) / 2;
    let center_y = screen_y + (expected_height as i32) / 2;
    let hmonitor = get_hmonitor_from_position(center_x, center_y)?;
    let t_hmonitor = t.elapsed();
    t = std::time::Instant::now();

    // 2. 获取/创建 D3D 设备（缓存后 < 1ms）
    let (d3d_device, winrt_device, context) = get_or_create_devices()?;
    let t_device = t.elapsed();
    t = std::time::Instant::now();

    // 3. 创建 CaptureItem + FramePool（~5ms）
    let item = create_capture_item_for_monitor(hmonitor)?;
    let size = item.Size().map_err(|e| {
        HuGeError::CaptureError(format!("获取 CaptureItem 尺寸失败: {:?}", e))
    })?;

    let frame_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &winrt_device,
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        1,
        SizeInt32 { Width: size.Width, Height: size.Height },
    )
    .map_err(|e| HuGeError::CaptureError(format!("创建 FramePool 失败: {:?}", e)))?;

    let t_setup = t.elapsed();
    t = std::time::Instant::now();

    // 4. 同步捕获一帧
    let (tx, rx) = mpsc::channel();
    let session = frame_pool.CreateCaptureSession(&item)
        .map_err(|e| HuGeError::CaptureError(format!("创建 CaptureSession 失败: {:?}", e)))?;

    frame_pool.FrameArrived(&TypedEventHandler::new(
        move |pool: &Option<Direct3D11CaptureFramePool>, _| {
            if let Some(pool) = pool {
                if let Ok(frame) = pool.TryGetNextFrame() {
                    let _ = tx.send(frame);
                }
            }
            Ok(())
        },
    ))
    .map_err(|e| HuGeError::CaptureError(format!("注册回调失败: {:?}", e)))?;

    session.StartCapture()
        .map_err(|e| HuGeError::CaptureError(format!("StartCapture 失败: {:?}", e)))?;

    let frame = rx.recv_timeout(Duration::from_secs(3))
        .map_err(|e| HuGeError::CaptureError(format!("WGC 捕获超时: {:?}", e)))?;

    let _ = session.Close();
    let _ = frame_pool.Close();

    let t_capture = t.elapsed();
    t = std::time::Instant::now();

    // 5. 提取像素数据（GPU → CPU）
    let surface = frame.Surface()
        .map_err(|e| HuGeError::CaptureError(format!("获取 Surface 失败: {:?}", e)))?;

    let access: windows::Win32::System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess =
        surface.cast()
            .map_err(|e| HuGeError::CaptureError(format!("转换接口失败: {:?}", e)))?;

    let source_texture: ID3D11Texture2D = unsafe {
        access.GetInterface()
            .map_err(|e| HuGeError::CaptureError(format!("获取 D3D11 纹理失败: {:?}", e)))?
    };

    let mut tex_desc = D3D11_TEXTURE2D_DESC::default();
    unsafe { source_texture.GetDesc(&mut tex_desc) };
    let width = tex_desc.Width;
    let height = tex_desc.Height;

    // 创建 Staging 纹理
    let staging_desc = D3D11_TEXTURE2D_DESC {
        Width: width,
        Height: height,
        MipLevels: 1,
        ArraySize: 1,
        Format: tex_desc.Format,
        SampleDesc: windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_STAGING,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        ..Default::default()
    };

    let staging_texture: ID3D11Texture2D = unsafe {
        let mut tex = None;
        d3d_device.CreateTexture2D(&staging_desc, None, Some(&mut tex))
            .map_err(|e| HuGeError::CaptureError(format!("创建 Staging 纹理失败: {:?}", e)))?;
        tex.unwrap()
    };

    unsafe {
        context.CopyResource(&staging_texture, &source_texture);
    }

    // Map 并读取像素数据
    let data = unsafe {
        let mut mapped = windows::Win32::Graphics::Direct3D11::D3D11_MAPPED_SUBRESOURCE::default();
        context.Map(&staging_texture, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .map_err(|e| HuGeError::CaptureError(format!("Map 纹理失败: {:?}", e)))?;

        let row_pitch = mapped.RowPitch as usize;
        let pixel_width = (width * 4) as usize;
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);

        let src = mapped.pData as *const u8;
        for row in 0..height as usize {
            let row_start = src.add(row * row_pitch);
            pixels.extend_from_slice(std::slice::from_raw_parts(row_start, pixel_width));
        }

        context.Unmap(&staging_texture, 0);
        pixels
    };

    let t_copy = t.elapsed();
    let t_total = total_start.elapsed();

    info!(
        "WGC: 截图完成 {}x{}, 总耗时: {:?} (HMONITOR: {:?}, 设备: {:?}, 初始化: {:?}, 捕获: {:?}, 拷贝: {:?})",
        width, height, t_total, t_hmonitor, t_device, t_setup, t_capture, t_copy
    );

    Ok(WgcCaptureResult { data, width, height })
}

/// 检测系统是否支持 WGC
pub fn is_wgc_supported() -> bool {
    windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().is_ok()
}

/// 预热 D3D11 设备（应用启动时后台调用）
///
/// 首次创建 D3D11 设备约需 1.3 秒，后续调用因缓存而几乎为零。
/// 在应用启动时预热可以消除首次截图的延迟。
///
/// 此函数是幂等的，多次调用不会重复创建设备。
pub fn pre_warm_d3d_devices() {
    let start = std::time::Instant::now();
    info!("WGC: 开始预热 D3D11 设备...");

    match get_or_create_devices() {
        Ok(_) => {
            info!("WGC: D3D11 设备预热完成，耗时: {:?}", start.elapsed());
        }
        Err(e) => {
            tracing::warn!("WGC: D3D11 设备预热失败（首次截图时会重试）: {}", e);
        }
    }
}
