//! 截图相关 Tauri 命令
//!
//! 封装截图引擎功能，暴露给前端调用。

use std::fs;
use std::path::PathBuf;
use chrono::Local;
use image::GenericImageView;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tracing::{debug, error, info, warn};

use crate::commands::history_cmd::{AddHistoryItemParams, HistoryMetadata, HistoryState};
use crate::database::settings::{get_config_path, load_config};
use crate::error::{HuGeError, HuGeResult};
use crate::screenshot::capture::{capture_region_impl, capture_screen, CaptureResult, Rect};
use crate::screenshot::image_hash::compute_bytes_hash;

#[cfg(windows)]
use crate::screenshot::window_detect::get_window_info_by_hwnd;

/// 截取指定区域
///
/// # 参数
///
/// - `rect`: 截取区域（物理像素坐标，虚拟屏幕坐标系）
///
/// # 返回
///
/// 返回截图结果，包含临时文件路径和元数据
///
/// # 示例
///
/// ```ignore
/// let result = capture_region(Rect { x: 0, y: 0, width: 800, height: 600 }).await?;
/// println!("截图保存到: {}", result.path);
/// ```
///
/// # 注意事项
///
/// - 坐标使用虚拟屏幕坐标系（多显示器场景下，副屏可能有负坐标）
/// - 如果区域跨越多个显示器，只会截取主要显示器上的部分
#[tauri::command]
pub async fn capture_region(rect: Rect) -> HuGeResult<CaptureResult> {
    info!(
        "命令调用: capture_region({}, {}, {}, {})",
        rect.x, rect.y, rect.width, rect.height
    );

    // 【修复穿透】优先从预缓存的全屏截图裁剪
    // 预缓存截图在 overlay 显示前捕获，包含所有窗口（如聊天窗口）
    if let Some(pre_capture_path) = crate::window::overlay::get_pre_capture_path() {
        info!("从预缓存截图裁剪区域（修复穿透）: {}", pre_capture_path);
        
        let start = std::time::Instant::now();
        
        // 加载预截图
        let img = image::open(&pre_capture_path).map_err(|e| {
            warn!("打开预缓存截图失败，回退到实时截图: {}", e);
            crate::error::HuGeError::CaptureError(format!("打开预缓存截图失败: {}", e))
        });
        
        if let Ok(img) = img {
            // 计算裁剪坐标（确保不越界）
            let crop_x = (rect.x.max(0) as u32).min(img.width().saturating_sub(1));
            let crop_y = (rect.y.max(0) as u32).min(img.height().saturating_sub(1));
            let crop_w = rect.width.min(img.width() - crop_x);
            let crop_h = rect.height.min(img.height() - crop_y);
            
            if crop_w >= 2 && crop_h >= 2 {
                let cropped = img.crop_imm(crop_x, crop_y, crop_w, crop_h);
                let cropped_rgba = cropped.to_rgba8();
                
                // 保存裁剪结果
                let path = crate::screenshot::capture::generate_temp_path_pub(0)?;
                let (file_size, image_hash) = crate::screenshot::capture::save_png_fast_with_hash_pub(&cropped_rgba, &path)?;
                
                let elapsed = start.elapsed().as_millis() as u64;
                info!("预缓存裁剪完成: {:?}, {}x{}, 耗时: {}ms", path, crop_w, crop_h, elapsed);
                
                return Ok(CaptureResult {
                    path: path.to_string_lossy().to_string(),
                    width: crop_w,
                    height: crop_h,
                    dpr: 1.0,
                    x: rect.x,
                    y: rect.y,
                    monitor_id: 0,
                    image_hash,
                    file_size: Some(file_size as i64),
                    capture_time_ms: Some(elapsed),
                    capture_engine: Some("pre_capture_crop".to_string()),
                });
            }
        }
        
        // 裁剪失败，回退到实时截图
        warn!("预缓存裁剪失败，回退到实时截图");
    }

    capture_region_impl(&rect)
}

/// 为 Overlay 捕获全屏截图
///
/// 在显示截图 overlay 之前调用，捕获指定显示器的全屏截图。
/// 返回的图片路径可以通过 `convertFileSrc()` 转换为前端可用的 URL。
///
/// # 参数
///
/// - `monitor_id`: 显示器 ID（可选，默认为主显示器）
///
/// # 返回
///
/// 返回截图结果，包含：
/// - `path`: 临时文件路径（需要用 `convertFileSrc()` 转换）
/// - `width`: 图片宽度（物理像素）
/// - `height`: 图片高度（物理像素）
/// - `dpr`: 设备像素比
/// - `x`, `y`: 显示器在虚拟屏幕中的位置
///
/// # 使用场景
///
/// 1. 热键触发截图
/// 2. 调用此命令捕获全屏
/// 3. 显示 overlay 窗口
/// 4. 将截图设置为 overlay 背景
/// 5. 用户在静态背景上选择区域
///
/// # 性能
///
/// - Windows: 使用 DXGI Desktop Duplication API，< 50ms
/// - 其他平台: 使用 screenshots-rs
#[tauri::command]
pub async fn capture_screen_for_overlay(monitor_id: Option<u32>) -> HuGeResult<CaptureResult> {
    info!("命令调用: capture_screen_for_overlay(monitor_id={:?})", monitor_id);
    
    // 【修复穿透】优先使用预缓存的截图（在 overlay 显示前捕获，包含所有窗口）
    if let Some(cached) = crate::window::overlay::take_pre_capture_cache() {
        info!(
            "使用预缓存截图（overlay 显示前已捕获）: {}x{} @ ({}, {})",
            cached.width, cached.height, cached.x, cached.y
        );
        return Ok(cached);
    }

    // 无缓存时回退到实时截图
    warn!("无预缓存截图，回退到实时截图");
    let start = std::time::Instant::now();
    let result = capture_screen(monitor_id).await;
    let elapsed = start.elapsed();
    
    match &result {
        Ok(r) => {
            info!(
                "全屏截图完成: {}x{} @ ({}, {}), DPR={}, 耗时: {:?}",
                r.width, r.height, r.x, r.y, r.dpr, elapsed
            );
        }
        Err(e) => {
            error!("全屏截图失败: {}", e);
        }
    }
    
    result
}

/// 截取指定窗口
///
/// # 参数
///
/// - `hwnd`: 窗口句柄（从 `detect_window_at` 或 `get_all_windows` 获取）
///
/// # 返回
///
/// 返回截图结果，包含临时文件路径和元数据
///
/// # 示例
///
/// ```ignore
/// // 先检测窗口
/// let window = detect_window_at(500, 300).await?.unwrap();
/// // 然后截取该窗口
/// let result = capture_window(window.hwnd).await?;
/// ```
///
/// # 注意事项
///
/// - 使用窗口的可视边界（排除 Windows 阴影边框）
/// - 如果窗口被其他窗口遮挡，截图可能包含遮挡内容
/// - 如果窗口无效或已关闭，返回错误
#[tauri::command]
pub async fn capture_window(hwnd: isize) -> HuGeResult<CaptureResult> {
    info!("命令调用: capture_window(hwnd={})", hwnd);

    #[cfg(windows)]
    {
        capture_window_impl(hwnd)
    }

    #[cfg(not(windows))]
    {
        let _ = hwnd;
        Err(HuGeError::CaptureError(
            "窗口截图仅支持 Windows 平台".to_string(),
        ))
    }
}

/// Windows 平台窗口截图实现
#[cfg(windows)]
fn capture_window_impl(hwnd: isize) -> HuGeResult<CaptureResult> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::IsWindow;

    // 验证窗口句柄
    let hwnd_win = HWND(hwnd as *mut std::ffi::c_void);

    let is_valid = unsafe { IsWindow(hwnd_win).as_bool() };
    if !is_valid {
        error!("无效的窗口句柄: {}", hwnd);
        return Err(HuGeError::CaptureError(format!(
            "无效的窗口句柄: {}",
            hwnd
        )));
    }

    // 获取窗口信息（包含真实边界）
    let window_info = get_window_info_by_hwnd(hwnd_win)?;

    debug!(
        "窗口截图: {} ({}), 物理边界: ({}, {}) {}x{}",
        window_info.title,
        window_info.class_name,
        window_info.physical_rect.x,
        window_info.physical_rect.y,
        window_info.physical_rect.width,
        window_info.physical_rect.height
    );

    // 使用窗口的物理边界进行区域截图
    capture_region_impl(&window_info.physical_rect)
}


/// 自动保存截图到历史目录
///
/// 根据配置的保存位置，自动创建 `历史截图/YYYY年M月D日/` 目录结构，
/// 并保存截图文件。
///
/// # 参数
///
/// - `app`: Tauri AppHandle
/// - `image_data`: PNG 图片数据（字节数组）
/// - `format`: 保存格式 ("png" 或 "jpg")
///
/// # 返回
///
/// 返回保存的文件完整路径
///
/// # 目录结构
///
/// ```text
/// {保存位置}/
/// └── 历史截图/
///     └── 2026年1月25日/
///         ├── screenshot_20260125_143052.png
///         └── screenshot_20260125_143105.png
/// ```
#[tauri::command]
pub async fn auto_save_screenshot(
    app: AppHandle,
    image_data: Vec<u8>,
    format: String,
) -> HuGeResult<String> {
    info!("自动保存截图，格式: {}", format);

    // 获取配置
    let config_path = get_config_path(&app)?;
    let config = load_config(&config_path)?;

    // 确定基础保存目录
    let base_dir = if config.screenshot.save_location.is_empty() {
        // 默认使用图片目录
        app.path().picture_dir().map_err(|e| {
            error!("获取图片目录失败: {}", e);
            HuGeError::CaptureError(format!("获取图片目录失败: {}", e))
        })?
    } else {
        PathBuf::from(&config.screenshot.save_location)
    };

    // 创建 历史截图/YYYY年M月D日/ 目录结构
    let now = Local::now();
    let date_folder = now.format("%Y年%-m月%-d日").to_string();
    let history_dir = base_dir.join("历史截图").join(&date_folder);

    // 确保目录存在
    if !history_dir.exists() {
        fs::create_dir_all(&history_dir).map_err(|e| {
            error!("创建历史截图目录失败: {:?}, 错误: {}", history_dir, e);
            HuGeError::CaptureError(format!("创建目录失败: {}", e))
        })?;
        info!("创建历史截图目录: {:?}", history_dir);
    }

    // 生成文件名: screenshot_YYYYMMDD_HHMMSS.{format}
    let timestamp = now.format("%Y%m%d_%H%M%S").to_string();
    let ext = if format == "jpg" { "jpg" } else { "png" };
    let filename = format!("screenshot_{}.{}", timestamp, ext);
    let file_path = history_dir.join(&filename);

    // 写入文件
    fs::write(&file_path, &image_data).map_err(|e| {
        error!("保存截图失败: {:?}, 错误: {}", file_path, e);
        HuGeError::CaptureError(format!("保存截图失败: {}", e))
    })?;

    let path_str = file_path.to_string_lossy().to_string();
    info!("截图已保存到: {}", path_str);

    Ok(path_str)
}

/// 获取截图保存配置
///
/// 返回当前的截图保存设置，包括保存位置、是否自动保存等。
#[tauri::command]
pub async fn get_screenshot_save_config(app: AppHandle) -> HuGeResult<ScreenshotSaveConfig> {
    let config_path = get_config_path(&app)?;
    let config = load_config(&config_path)?;

    Ok(ScreenshotSaveConfig {
        save_location: config.screenshot.save_location,
        auto_save: config.screenshot.auto_save,
        default_format: config.screenshot.default_format,
    })
}

/// 截图保存配置
#[derive(serde::Serialize, serde::Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ScreenshotSaveConfig {
    pub save_location: String,
    pub auto_save: bool,
    pub default_format: String,
}

/// 保存截图时的元数据
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveScreenshotMetadata {
    /// 截图模式：region, window, fullscreen
    pub capture_mode: Option<String>,
    /// 显示器 ID
    pub monitor_id: Option<u32>,
    /// 是否有标注
    pub has_annotations: Option<bool>,
    /// 应用名称
    pub app_name: Option<String>,
    /// 窗口标题
    pub window_title: Option<String>,
}

/// 保存截图并添加历史记录的结果
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveScreenshotResult {
    /// 保存的文件路径
    pub file_path: String,
    /// 历史记录 ID
    pub history_id: i64,
    /// 缩略图路径
    pub thumbnail_path: Option<String>,
}

/// 保存截图并添加到历史记录
///
/// 组合命令，原子化处理：
/// 1. 保存截图文件到历史截图目录
/// 2. 生成缩略图 (200px max, JPEG)
/// 3. 写入历史记录数据库
/// 4. 返回 { filePath, historyId, thumbnailPath }
///
/// # 参数
///
/// - `app`: Tauri AppHandle
/// - `state`: 历史记录状态
/// - `image_data`: PNG 图片数据（字节数组）
/// - `format`: 保存格式 ("png" 或 "jpg")
/// - `metadata`: 可选的元数据
/// - `ocr_text`: 可选的 OCR 文本（如果已经执行过 OCR）
#[tauri::command]
pub async fn save_screenshot_with_history(
    app: AppHandle,
    state: State<'_, HistoryState>,
    image_data: Vec<u8>,
    format: String,
    metadata: Option<SaveScreenshotMetadata>,
    ocr_text: Option<String>,
) -> HuGeResult<SaveScreenshotResult> {
    info!("保存截图并添加历史记录，格式: {}", format);

    // 获取配置
    let config_path = get_config_path(&app)?;
    let config = load_config(&config_path)?;

    // 确定基础保存目录
    let base_dir = if config.screenshot.save_location.is_empty() {
        // 默认使用图片目录
        app.path().picture_dir().map_err(|e| {
            error!("获取图片目录失败: {}", e);
            HuGeError::CaptureError(format!("获取图片目录失败: {}", e))
        })?
    } else {
        PathBuf::from(&config.screenshot.save_location)
    };

    // 创建 历史截图/YYYY年M月D日/ 目录结构
    let now = Local::now();
    let date_folder = now.format("%Y年%-m月%-d日").to_string();
    let history_dir = base_dir.join("历史截图").join(&date_folder);

    // 确保目录存在
    if !history_dir.exists() {
        fs::create_dir_all(&history_dir).map_err(|e| {
            error!("创建历史截图目录失败: {:?}, 错误: {}", history_dir, e);
            HuGeError::CaptureError(format!("创建目录失败: {}", e))
        })?;
        info!("创建历史截图目录: {:?}", history_dir);
    }

    // 生成文件名: screenshot_YYYYMMDD_HHMMSS.{format}
    let timestamp = now.format("%Y%m%d_%H%M%S").to_string();
    let ext = if format == "jpg" { "jpg" } else { "png" };
    let filename = format!("screenshot_{}.{}", timestamp, ext);
    let file_path = history_dir.join(&filename);

    // 写入截图文件
    fs::write(&file_path, &image_data).map_err(|e| {
        error!("保存截图失败: {:?}, 错误: {}", file_path, e);
        HuGeError::CaptureError(format!("保存截图失败: {}", e))
    })?;

    let file_path_str = file_path.to_string_lossy().to_string();
    info!("截图已保存到: {}", file_path_str);

    // 【性能优化】只解码一次图像，同时获取尺寸和生成缩略图
    let img = image::load_from_memory(&image_data).map_err(|e| {
        error!("解析图片失败: {}", e);
        HuGeError::CaptureError(format!("解析图片失败: {}", e))
    })?;
    let (width, height) = img.dimensions();
    let file_size = image_data.len() as i64;

    // 【性能优化】直接从内存生成缩略图，避免重新读取文件
    let thumbnail_path = generate_thumbnail_from_memory(&img, &history_dir, &timestamp)?;

    // 构建历史记录元数据
    let history_metadata = metadata.map(|m| HistoryMetadata {
        capture_mode: m.capture_mode,
        monitor_id: m.monitor_id,
        app_name: m.app_name,
        window_title: m.window_title,
        has_annotations: m.has_annotations,
    });

    // 添加到历史记录数据库
    let history_params = AddHistoryItemParams {
        file_path: file_path_str.clone(),
        thumbnail_path: thumbnail_path.clone(),
        width,
        height,
        file_size: Some(file_size),
        ocr_text: ocr_text.clone(), // 使用传入的 OCR 文本
        tags: None,
        metadata: history_metadata,
        content_type: None, // 默认 image
        text_content: None,
    };

    // 计算图片哈希（用于去重）
    let image_hash = compute_bytes_hash(&image_data);
    debug!("图片哈希: {}", image_hash);

    // 获取数据库连接并插入记录（带去重逻辑）
    let history_id = {
        let db_guard = state.db.lock().await;
        let db = db_guard
            .as_ref()
            .ok_or_else(|| HuGeError::ConfigError("历史记录数据库未初始化".to_string()))?;

        // 检查是否存在相同哈希的记录（连续复制去重）
        if let Some(existing_id) = db.find_by_hash(&image_hash)? {
            info!("检测到重复图片（哈希: {}），删除旧记录 ID: {}", image_hash, existing_id);

            // 获取旧记录以删除文件
            if let Some(old_record) = db.get(existing_id)? {
                // 删除旧的截图文件
                if let Err(e) = fs::remove_file(&old_record.file_path) {
                    warn!("删除旧截图文件失败: {} - {}", old_record.file_path, e);
                }
                // 删除旧的缩略图
                if let Some(ref thumb_path) = old_record.thumbnail_path {
                    if let Err(e) = fs::remove_file(thumb_path) {
                        warn!("删除旧缩略图失败: {} - {}", thumb_path, e);
                    }
                }
            }

            // 删除数据库记录
            db.delete(existing_id)?;
        }

        // 转换参数为数据库记录
        let tags_json = history_params.tags.as_ref().map(|t| serde_json::to_string(t).unwrap_or_default());
        let metadata_json = history_params.metadata.as_ref().map(|m| serde_json::to_string(m).unwrap_or_default());

        let record = crate::database::history::ScreenshotRecord {
            id: 0,
            created_at: String::new(),
            file_path: history_params.file_path.clone(),
            thumbnail_path: history_params.thumbnail_path.clone(),
            width: history_params.width,
            height: history_params.height,
            file_size: history_params.file_size,
            ocr_text: history_params.ocr_text.clone(),
            tags: tags_json,
            metadata: metadata_json,
            image_hash: Some(image_hash),
            is_pinned: false,
            ocr_cached_at: None,
            content_type: "image".to_string(),
            text_content: None,
        };

        db.insert(&record)?
    };

    info!("历史记录已添加，ID: {}", history_id);

    Ok(SaveScreenshotResult {
        file_path: file_path_str,
        history_id,
        thumbnail_path,
    })
}

/// 从文件路径保存截图并添加历史记录
///
/// 【性能优化】前端只传递文件路径字符串，后端直接从磁盘读取图像数据。
/// 避免了 Array.from() 产生的巨型 JSON 序列化开销。
///
/// 适用场景：
/// - 有标注时：前端先将合成图像写入临时文件，再传递路径
/// - 无标注时：直接传递原始截图的路径
///
/// # 参数
///
/// * `file_path` - 要保存的 PNG/JPG 图像文件路径
/// * `format` - 保存格式 ("png" 或 "jpg")
/// * `metadata` - 可选的截图元数据
/// * `ocr_text` - 可选的 OCR 文本
#[tauri::command]
pub async fn save_screenshot_with_history_from_file(
    app: AppHandle,
    state: State<'_, HistoryState>,
    file_path: String,
    format: String,
    metadata: Option<SaveScreenshotMetadata>,
    ocr_text: Option<String>,
) -> HuGeResult<SaveScreenshotResult> {
    info!("从文件保存截图并添加历史记录: {}", file_path);

    // 从磁盘读取图像数据
    let image_data = fs::read(&file_path).map_err(|e| {
        error!("读取图像文件失败: {} - {}", file_path, e);
        HuGeError::CaptureError(format!("读取图像文件失败: {}", e))
    })?;

    // 复用已有逻辑
    save_screenshot_with_history(app, state, image_data, format, metadata, ocr_text).await
}

/// 生成缩略图（从内存中的图像数据）
///
/// 缩略图存放在 thumbnails/ 子目录，最大尺寸 200px，JPEG 格式
/// 
/// # 性能优化
/// 直接从内存中的图像数据生成缩略图，避免重新从磁盘读取
fn generate_thumbnail_from_memory(
    img: &image::DynamicImage,
    history_dir: &std::path::Path,
    timestamp: &str,
) -> HuGeResult<Option<String>> {
    // 创建缩略图目录
    let thumbnails_dir = history_dir.join("thumbnails");
    if !thumbnails_dir.exists() {
        if let Err(e) = fs::create_dir_all(&thumbnails_dir) {
            warn!("创建缩略图目录失败: {}", e);
            return Ok(None);
        }
    }

    // 计算缩略图尺寸（最大边 200px）
    const MAX_THUMBNAIL_SIZE: u32 = 200;
    let (width, height) = img.dimensions();
    let (thumb_width, thumb_height) = if width > height {
        let ratio = MAX_THUMBNAIL_SIZE as f64 / width as f64;
        (MAX_THUMBNAIL_SIZE, (height as f64 * ratio) as u32)
    } else {
        let ratio = MAX_THUMBNAIL_SIZE as f64 / height as f64;
        ((width as f64 * ratio) as u32, MAX_THUMBNAIL_SIZE)
    };

    // 生成缩略图
    let thumbnail = img.thumbnail(thumb_width, thumb_height);

    // 保存为 JPEG
    let thumb_filename = format!("screenshot_{}_thumb.jpg", timestamp);
    let thumb_path = thumbnails_dir.join(&thumb_filename);

    if let Err(e) = thumbnail.save(&thumb_path) {
        warn!("保存缩略图失败: {}", e);
        return Ok(None);
    }

    let thumb_path_str = thumb_path.to_string_lossy().to_string();
    debug!("缩略图已生成: {}", thumb_path_str);

    Ok(Some(thumb_path_str))
}


/// 保存图像数据到临时文件
///
/// 用于钉图功能：将合成后的截图（包含标注）保存到临时文件，
/// 然后创建钉图窗口显示该图像。
///
/// # 参数
///
/// - `image_data`: PNG 图片数据（字节数组）
/// - `format`: 保存格式 ("png" 或 "jpg")
///
/// # 返回
///
/// 返回临时文件的完整路径
///
/// # 示例
///
/// ```ignore
/// let temp_path = save_temp_image(image_data, "png").await?;
/// create_pin_window(temp_path, rect).await?;
/// ```
#[tauri::command]
pub async fn save_temp_image(image_data: Vec<u8>, format: String) -> HuGeResult<String> {
    use std::env;
    use std::time::{SystemTime, UNIX_EPOCH};

    info!("保存临时图像，格式: {}, 大小: {} bytes", format, image_data.len());

    // 获取临时目录
    let temp_dir = env::temp_dir().join("hugescreenshot");

    // 确保目录存在
    if !temp_dir.exists() {
        fs::create_dir_all(&temp_dir).map_err(|e| {
            error!("创建临时目录失败: {:?}, 错误: {}", temp_dir, e);
            HuGeError::FileError(e)
        })?;
    }

    // 生成唯一文件名
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);

    let ext = if format == "jpg" { "jpg" } else { "png" };
    let filename = format!("pin_{}.{}", timestamp, ext);
    let file_path = temp_dir.join(&filename);

    // 写入文件
    fs::write(&file_path, &image_data).map_err(|e| {
        error!("保存临时图像失败: {:?}, 错误: {}", file_path, e);
        HuGeError::FileError(e)
    })?;

    let path_str = file_path.to_string_lossy().to_string();
    info!("临时图像已保存到: {}", path_str);

    Ok(path_str)
}

/// 裁剪指定区域的截图
#[derive(Debug, serde::Deserialize)]
pub struct CropRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// 裁剪截图并保存到临时文件
///
/// 用于 Anki 高亮 OCR：从原始截图中裁剪出高亮区域，
/// 保存为临时文件后交给 OCR 引擎识别。
///
/// # 参数
///
/// - `source_path`: 原始截图路径
/// - `rect`: 裁剪区域（物理像素坐标）
///
/// # 返回
///
/// 返回裁剪后临时文件的路径
#[tauri::command]
pub async fn crop_and_save_temp(source_path: String, rect: CropRect) -> HuGeResult<String> {
    use std::env;
    use std::time::{SystemTime, UNIX_EPOCH};

    debug!(
        "裁剪截图: {} -> ({}, {}) {}x{}",
        source_path, rect.x, rect.y, rect.width, rect.height
    );

    // 加载原始图像
    let img = image::open(&source_path).map_err(|e| {
        error!("打开截图失败: {} - {}", source_path, e);
        HuGeError::Unknown(format!("打开截图失败: {}", e))
    })?;

    // 确保裁剪区域在图像范围内
    let img_width = img.width();
    let img_height = img.height();
    let crop_x = rect.x.min(img_width.saturating_sub(1));
    let crop_y = rect.y.min(img_height.saturating_sub(1));
    let crop_w = rect.width.min(img_width - crop_x);
    let crop_h = rect.height.min(img_height - crop_y);

    if crop_w < 2 || crop_h < 2 {
        return Err(HuGeError::Unknown("裁剪区域太小".to_string()));
    }

    // 裁剪
    let cropped = img.crop_imm(crop_x, crop_y, crop_w, crop_h);

    // 保存到临时文件
    let temp_dir = env::temp_dir().join("hugescreenshot");
    if !temp_dir.exists() {
        fs::create_dir_all(&temp_dir).map_err(HuGeError::FileError)?;
    }

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);

    let filename = format!("crop_{}.png", timestamp);
    let file_path = temp_dir.join(&filename);

    cropped.save(&file_path).map_err(|e| {
        error!("保存裁剪图像失败: {:?} - {}", file_path, e);
        HuGeError::Unknown(format!("保存裁剪图像失败: {}", e))
    })?;

    let path_str = file_path.to_string_lossy().to_string();
    debug!("裁剪图像已保存: {} ({}x{})", path_str, crop_w, crop_h);

    Ok(path_str)
}
