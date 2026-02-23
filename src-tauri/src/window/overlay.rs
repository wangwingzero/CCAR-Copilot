//! 覆盖窗口管理
//!
//! 创建全屏透明覆盖窗口，用于截图选区显示。
//!
//! # 设计原则
//!
//! - 每个显示器创建一个独立的覆盖窗口
//! - 窗口透明、无边框、置顶显示
//! - 窗口覆盖整个显示器区域
//! - 支持捕获鼠标事件进行选区操作
//!
//! # 性能优化：窗口预加载
//!
//! 为了避免热键触发时的延迟，采用"预加载隐藏窗口"模式：
//! 1. 应用启动时预创建隐藏的覆盖窗口（WebView 提前加载）
//! 2. 热键触发时只需调用 show() 即可立即显示（~16ms）
//! 3. 截图完成后隐藏窗口而不是关闭，以便下次快速显示
//!
//! # 坐标系统
//!
//! - 窗口位置使用物理像素（虚拟屏幕坐标系）
//! - 窗口尺寸使用物理像素
//! - 前端 Vue 使用逻辑像素，需要通过 scale_factor 转换
//!
//! # 焦点管理（关键！）
//!
//! Windows 系统对焦点抢夺有严格限制，需要使用特殊技巧：
//! 1. AttachThreadInput - 将当前线程与前台窗口线程关联
//! 2. SetForegroundWindow - 强制设置前台窗口
//! 3. eval("window.focus()") - 确保 WebView 内部获得焦点
//! 4. DwmFlush - 刷新 DWM 合成器，避免渲染问题

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use tauri::{Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tracing::{debug, error, info, warn};

#[cfg(windows)]
use windows::Win32::{
    Foundation::HWND,
    Graphics::Dwm::DwmFlush,
    Graphics::Gdi::{InvalidateRect, UpdateWindow},
    System::Threading::{AttachThreadInput, GetCurrentThreadId},
    UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowThreadProcessId,
        SetForegroundWindow, SetWindowPos, HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE,
        SWP_SHOWWINDOW,
        SetWindowDisplayAffinity, WINDOW_DISPLAY_AFFINITY,
    },
};

use crate::error::{HuGeError, HuGeResult};
use crate::screenshot::snapshot::capture_static_snapshot;

/// 全局覆盖窗口管理器
///
/// 存储所有已创建的覆盖窗口标签，用于统一管理和关闭
static OVERLAY_WINDOWS: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// 窗口是否已预加载的标志
static OVERLAYS_PRELOADED: AtomicBool = AtomicBool::new(false);

/// WebView 就绪状态（每个窗口的就绪标志）
static OVERLAY_READY: std::sync::LazyLock<Mutex<std::collections::HashSet<String>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashSet::new()));

/// 静态快照是否正在后台捕获
static SNAPSHOT_CAPTURE_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

/// 预截图缓存（在 overlay 显示前捕获的全屏截图）
///
/// 修复截图穿透问题：在 overlay 显示前截图，确保包含所有窗口。
/// `capture_screen_for_overlay` 和 `capture_region` 会优先使用此缓存。
static PRE_CAPTURE_CACHE: Mutex<Option<crate::screenshot::CaptureResult>> = Mutex::new(None);

/// 存储预截图缓存
pub fn set_pre_capture_cache(result: crate::screenshot::CaptureResult) {
    if let Ok(mut cache) = PRE_CAPTURE_CACHE.lock() {
        *cache = Some(result);
    }
}

/// 获取预截图缓存的副本（不清空缓存，供 capture_region 复用）
///
/// 注意：之前使用 take() 会导致竞态条件——capture_screen_for_overlay 取出后，
/// capture_region 就找不到缓存了。现在改为 clone()，缓存在 overlay 关闭时
/// 通过 clear_pre_capture_cache() 统一清除。
pub fn take_pre_capture_cache() -> Option<crate::screenshot::CaptureResult> {
    if let Ok(cache) = PRE_CAPTURE_CACHE.lock() {
        cache.clone()
    } else {
        None
    }
}

/// 获取预截图缓存的路径（不取出，用于裁剪）
pub fn get_pre_capture_path() -> Option<String> {
    if let Ok(cache) = PRE_CAPTURE_CACHE.lock() {
        cache.as_ref().map(|r| r.path.clone())
    } else {
        None
    }
}

/// 清除预截图缓存
pub fn clear_pre_capture_cache() {
    if let Ok(mut cache) = PRE_CAPTURE_CACHE.lock() {
        *cache = None;
    }
}

/// Escape 快捷键是否已注册的标志
static ESCAPE_REGISTERED: AtomicBool = AtomicBool::new(false);

/// 后台捕获静态快照并广播事件（不阻塞 overlay 显示）
fn spawn_snapshot_capture(app: tauri::AppHandle) {
    // 避免重复并发捕获
    if SNAPSHOT_CAPTURE_IN_PROGRESS.swap(true, Ordering::SeqCst) {
        debug!("静态快照捕获已在进行，跳过本次触发");
        return;
    }

    tauri::async_runtime::spawn(async move {
        let start = Instant::now();
        info!("后台开始捕获静态快照...");

        match capture_static_snapshot(app.clone()).await {
            Ok(result) => {
                if let Err(e) = app.emit("snapshot-ready", &result) {
                    warn!("发送 snapshot-ready 事件失败: {}", e);
                } else {
                    debug!("snapshot-ready 事件已发送: path={}", result.path);
                }
                info!("静态快照后台捕获完成，耗时 {:?}", start.elapsed());
            }
            Err(e) => {
                error!("静态快照后台捕获失败: {}", e);
                let error_payload = serde_json::json!({
                    "error": format!("{}", e)
                });
                if let Err(emit_err) = app.emit("snapshot-error", error_payload) {
                    warn!("发送 snapshot-error 事件失败: {}", emit_err);
                }
            }
        }

        SNAPSHOT_CAPTURE_IN_PROGRESS.store(false, Ordering::SeqCst);
    });
}

/// 注册临时 Escape 快捷键（overlay 显示时）
///
/// 作为键盘事件捕获的备用方案，确保用户始终能通过 Escape 取消截图。
///
/// # 工作流程
///
/// 1. 先发送 `overlay-force-close` 事件通知前端
/// 2. 前端收到事件后会自动复制截图到剪贴板（如果有截图结果）
/// 3. 然后前端自行关闭窗口
/// 4. 如果前端未能在 1.5 秒内响应，Rust 端作为兜底直接隐藏窗口
fn register_escape_shortcut(app: &tauri::AppHandle) {
    // 避免重复注册
    if ESCAPE_REGISTERED.load(Ordering::SeqCst) {
        return;
    }

    let global_shortcut = app.global_shortcut();

    // 检查 Escape 是否已被注册
    if global_shortcut.is_registered("Escape") {
        debug!("Escape 快捷键已被注册，跳过");
        return;
    }

    // 克隆 app handle 用于闭包
    let app_clone = app.clone();

    // 注册 Escape 快捷键
    match global_shortcut.on_shortcut("Escape", move |_app, _shortcut, event| {
        // 只处理 Pressed 状态，避免双触发
        if event.state != ShortcutState::Pressed {
            return;
        }

        info!("[备用] Escape 快捷键触发，通知前端处理关闭");

        let app_handle = app_clone.clone();
        tauri::async_runtime::spawn(async move {
            // 第一步：发送事件通知前端，让前端处理剪贴板复制和关闭
            let mut frontend_handled = false;
            if let Ok(windows) = OVERLAY_WINDOWS.lock() {
                for label in windows.iter() {
                    if let Some(window) = app_handle.get_webview_window(label) {
                        if window.is_visible().unwrap_or(false) {
                            if let Err(e) = window.emit("overlay-force-close", ()) {
                                warn!("[备用] 发送 overlay-force-close 事件失败: {}", e);
                            } else {
                                debug!("[备用] 已发送 overlay-force-close 事件到 {}", label);
                                frontend_handled = true;
                            }
                        }
                    }
                }
            }

            // 第二步：等待 1.5 秒，给前端处理剪贴板复制 + 关闭的时间
            if frontend_handled {
                tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

                // 检查 overlay 是否仍然可见（前端可能已经关闭了）
                let still_visible = if let Ok(windows) = OVERLAY_WINDOWS.lock() {
                    windows.iter().any(|label| {
                        app_handle.get_webview_window(label)
                            .and_then(|w| w.is_visible().ok())
                            .unwrap_or(false)
                    })
                } else {
                    false
                };

                if still_visible {
                    warn!("[备用] 前端未在 1.5s 内关闭 overlay，兜底直接隐藏");
                    if let Err(e) = hide_overlay_windows(app_handle).await {
                        error!("[备用] 兜底隐藏 overlay 失败: {}", e);
                    }
                } else {
                    info!("[备用] 前端已成功处理关闭");
                }
            } else {
                // 无法通知前端，直接隐藏
                warn!("[备用] 无法通知前端，直接隐藏 overlay");
                if let Err(e) = hide_overlay_windows(app_handle).await {
                    error!("[备用] 隐藏 overlay 失败: {}", e);
                }
            }
        });
    }) {
        Ok(()) => {
            ESCAPE_REGISTERED.store(true, Ordering::SeqCst);
            info!("临时 Escape 快捷键注册成功");
        }
        Err(e) => {
            warn!("注册 Escape 快捷键失败: {}", e);
        }
    }
}

/// 取消注册临时 Escape 快捷键（overlay 隐藏时）
fn unregister_escape_shortcut(app: &tauri::AppHandle) {
    if !ESCAPE_REGISTERED.load(Ordering::SeqCst) {
        return;
    }

    let global_shortcut = app.global_shortcut();

    if global_shortcut.is_registered("Escape") {
        match global_shortcut.unregister("Escape") {
            Ok(()) => {
                ESCAPE_REGISTERED.store(false, Ordering::SeqCst);
                info!("临时 Escape 快捷键已取消注册");
            }
            Err(e) => {
                warn!("取消注册 Escape 快捷键失败: {}", e);
            }
        }
    }
}

/// 强制窗口获取前台焦点（Windows 专用）
///
/// Windows 系统为防止"焦点抢夺"，对 SetForegroundWindow 有严格限制。
/// 此函数使用 AttachThreadInput 技巧绕过限制。
#[cfg(windows)]
fn force_foreground_window(hwnd: HWND) {
    unsafe {
        // 获取当前前台窗口的线程 ID
        let foreground_hwnd = GetForegroundWindow();
        let foreground_thread = GetWindowThreadProcessId(foreground_hwnd, None);
        let current_thread = GetCurrentThreadId();

        // 如果不是同一线程，需要 AttachThreadInput
        if foreground_thread != current_thread {
            // 将当前线程与前台窗口线程关联
            if !AttachThreadInput(current_thread, foreground_thread, true).as_bool() {
                warn!("AttachThreadInput(attach) 失败: {:?}", hwnd);
            }

            // 设置为前台窗口
            // 注意：SetForegroundWindow 返回 BOOL，FALSE 表示系统拒绝了焦点请求（非致命）
            if !SetForegroundWindow(hwnd).as_bool() {
                debug!("SetForegroundWindow 未能设置前台窗口（系统可能拒绝了焦点请求）: {:?}", hwnd);
            }

            // 强制置顶
            if let Err(e) = SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0, 0, 0, 0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
            ) {
                warn!("SetWindowPos(TOPMOST) 失败: {:?}, error: {}", hwnd, e);
            }

            // 解除线程关联
            if !AttachThreadInput(current_thread, foreground_thread, false).as_bool() {
                warn!("AttachThreadInput(detach) 失败: {:?}", hwnd);
            }
        } else if !SetForegroundWindow(hwnd).as_bool() {
            debug!("SetForegroundWindow 未能设置前台窗口: {:?}", hwnd);
        }

        debug!("强制前台焦点设置完成: {:?}", hwnd);
    }
}

/// 刷新窗口和 DWM 合成器（Windows 专用）
///
/// 强制刷新窗口内容和 DWM 合成队列，避免渲染缓存导致的显示问题。
#[cfg(windows)]
fn refresh_window_dwm(hwnd: HWND) {
    unsafe {
        // 无效化窗口区域，强制重绘
        if !InvalidateRect(hwnd, None, true).as_bool() {
            warn!("InvalidateRect 失败: {:?}", hwnd);
        }
        // 立即更新窗口
        if !UpdateWindow(hwnd).as_bool() {
            warn!("UpdateWindow 失败: {:?}", hwnd);
        }
        // 刷新 DWM 合成器队列
        if let Err(e) = DwmFlush() {
            warn!("DwmFlush 失败: {}", e);
        }

        debug!("DWM 刷新完成: {:?}", hwnd);
    }
}

/// WDA_EXCLUDEFROMCAPTURE 常量值 (Windows 10 2004+)
///
/// 将窗口标记为"从截图捕获中排除"。设置此标志后：
/// - 窗口对用户仍然可见（遮罩层正常显示）
/// - 但 DXGI/WGC 等截图 API 不会捕获此窗口
/// - 这确保截图结果是原始屏幕内容，不包含遮罩层的变暗效果
///
/// 参考: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setwindowdisplayaffinity
#[cfg(windows)]
const WDA_EXCLUDEFROMCAPTURE: WINDOW_DISPLAY_AFFINITY = WINDOW_DISPLAY_AFFINITY(0x11);

/// 设置窗口为"从截图捕获中排除"（Windows 专用）
///
/// 调用 SetWindowDisplayAffinity 使 overlay 窗口对截图 API 不可见。
/// 这是解决"截图范围内屏幕变暗"问题的核心方案：
/// - 用户仍然能看到半透明遮罩层（视觉提示正在截图）
/// - 但 DXGI/WGC 截图不会包含遮罩层，保持原始屏幕颜色
///
/// 如果设置失败（如系统版本不支持），仅输出警告，不影响功能。
#[cfg(windows)]
fn set_exclude_from_capture(hwnd: HWND) {
    unsafe {
        match SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) {
            Ok(()) => {
                info!("已设置 WDA_EXCLUDEFROMCAPTURE（overlay 窗口将从截图捕获中排除）: {:?}", hwnd);
            }
            Err(e) => {
                warn!(
                    "设置 WDA_EXCLUDEFROMCAPTURE 失败: {}。\
                     截图可能包含遮罩层导致颜色偏暗。\
                     此功能需要 Windows 10 2004 (Build 19041) 或更高版本。",
                    e
                );
            }
        }
    }
}

/// 覆盖窗口配置
#[derive(Debug, Clone)]
pub struct OverlayConfig {
    /// 窗口标签前缀
    pub label_prefix: &'static str,
    /// 前端页面 URL
    pub url: &'static str,
    /// 是否跳过任务栏
    pub skip_taskbar: bool,
    /// 是否可调整大小
    pub resizable: bool,
    /// 是否可最大化
    pub maximizable: bool,
    /// 是否可最小化
    pub minimizable: bool,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        Self {
            label_prefix: "overlay",
            url: "overlay.html",
            skip_taskbar: true,
            resizable: false,
            maximizable: false,
            minimizable: false,
        }
    }
}

/// 预加载所有显示器的覆盖窗口（应用启动时调用）
///
/// 在应用启动时预创建隐藏的覆盖窗口，以便热键触发时能够立即显示。
/// 这是性能优化的关键：WebView 初始化是耗时操作，提前完成可避免热键延迟。
///
/// # 参数
///
/// - `app`: Tauri 应用句柄
///
/// # 返回
///
/// 成功返回预加载的窗口数量，失败返回错误信息
pub fn preload_overlay_windows(app: &tauri::AppHandle) -> HuGeResult<u32> {
    // 检查是否已预加载
    if OVERLAYS_PRELOADED.load(Ordering::SeqCst) {
        debug!("覆盖窗口已预加载，跳过");
        return Ok(0);
    }

    info!("预加载覆盖窗口...");

    let monitors = app
        .available_monitors()
        .map_err(|e| HuGeError::WindowError(format!("获取显示器列表失败: {}", e)))?;

    let monitor_count = monitors.len() as u32;
    info!("检测到 {} 个显示器，开始预加载覆盖窗口", monitor_count);

    let mut created_count = 0u32;
    let config = OverlayConfig::default();

    for (index, monitor) in monitors.iter().enumerate() {
        let monitor_id = index as u32;
        let window_label = format!("overlay-{}", monitor_id);

        // 检查窗口是否已存在
        if app.get_webview_window(&window_label).is_some() {
            debug!("覆盖窗口 {} 已存在，跳过", window_label);
            continue;
        }

        // 获取显示器信息
        let position = monitor.position();
        let size = monitor.size();
        let scale_factor = monitor.scale_factor();
        let monitor_name = monitor
            .name()
            .cloned()
            .unwrap_or_else(|| format!("显示器 {}", monitor_id));

        // 关键：将物理像素转换为逻辑像素
        // Tauri 的 position() 和 inner_size() 接受逻辑像素
        let logical_width = size.width as f64 / scale_factor;
        let logical_height = size.height as f64 / scale_factor;
        let logical_x = position.x as f64 / scale_factor;
        let logical_y = position.y as f64 / scale_factor;

        debug!(
            "预加载覆盖窗口: {} @ ({}, {}), 物理尺寸: {}x{}, 逻辑尺寸: {:.0}x{:.0}, DPR: {:.2}",
            monitor_name, position.x, position.y, size.width, size.height, logical_width, logical_height, scale_factor
        );

        // 创建隐藏的覆盖窗口
        match WebviewWindowBuilder::new(
            app,
            &window_label,
            WebviewUrl::App(config.url.into()),
        )
        // 核心属性：透明、无边框、置顶、无阴影
        .transparent(true)
        .decorations(false)
        .shadow(false)  // 关键：禁用窗口阴影，避免位置偏移
        .always_on_top(true)
        // 窗口位置和尺寸（逻辑像素）
        .position(logical_x, logical_y)
        .inner_size(logical_width, logical_height)
        // 窗口行为
        .skip_taskbar(config.skip_taskbar)
        .resizable(config.resizable)
        .maximizable(config.maximizable)
        .minimizable(config.minimizable)
        // 关键：预加载时隐藏窗口
        .visible(false)
        .focused(false)
        // 窗口标题（调试用）
        .title(format!("截图覆盖 - {}", monitor_name))
        .build() {
            Ok(window) => {
                // 关键：设置 WDA_EXCLUDEFROMCAPTURE，使 overlay 不被截图捕获
                // 这样 DXGI/WGC 截图不会包含遮罩层的变暗效果
                #[cfg(windows)]
                {
                    if let Ok(hwnd) = window.hwnd() {
                        set_exclude_from_capture(HWND(hwnd.0));
                    }
                }

                // 记录窗口标签
                if let Ok(mut windows) = OVERLAY_WINDOWS.lock() {
                    windows.push(window_label.clone());
                }
                created_count += 1;
                debug!("覆盖窗口 {} 预加载成功", window_label);
            }
            Err(e) => {
                error!("预加载覆盖窗口 {} 失败: {}", window_label, e);
            }
        }
    }

    // 标记为已预加载
    OVERLAYS_PRELOADED.store(true, Ordering::SeqCst);
    info!("覆盖窗口预加载完成，成功 {}/{} 个", created_count, monitor_count);

    Ok(created_count)
}

/// 显示所有预加载的覆盖窗口（热键触发时调用）
///
/// 如果窗口已预加载，直接显示；否则创建新窗口。
/// 这是热键响应的入口点，性能关键路径。
///
/// # 流程（修复截图穿透问题）
///
/// 1. **先截图**：在显示 overlay 之前，使用 WGC 捕获全屏截图
///    - 确保截图包含所有窗口（聊天窗口等不会被 overlay 遮挡）
/// 2. **再显示 overlay**：将预截的图像路径传递给前端
/// 3. 前端用预截的图像作为冻结背景
/// 4. 区域截图从预截的图像裁剪，而不是重新截图
///
/// # 参数
///
/// - `app`: Tauri 应用句柄
///
/// # 返回
///
/// 成功返回显示的窗口数量，失败返回错误信息
///
/// **Validates: Requirements 1.1, 3.2**
#[tauri::command]
pub async fn show_overlay_windows(app: tauri::AppHandle) -> HuGeResult<u32> {
    let start = Instant::now();
    info!("显示覆盖窗口...");

    // === 第一步：启动预截图（后台线程，不阻塞 overlay 显示） ===
    // 修复截图穿透问题：在 overlay 显示前启动截图，确保包含所有窗口。
    // 使用 spawn_blocking 在独立线程中执行，与 overlay 显示并行。
    info!("启动预截图（后台线程）...");
    let capture_handle = tokio::task::spawn_blocking(|| {
        use crate::screenshot::capture::capture_screen_sync;
        capture_screen_sync(None)
    });

    // === 第二步：显示覆盖窗口（与预截图并行） ===
    // 如果未预加载，先创建窗口
    // 使用 Box::pin 避免 async 递归导致的无限大小 Future
    if !OVERLAYS_PRELOADED.load(Ordering::SeqCst) {
        warn!("覆盖窗口未预加载，将创建新窗口（可能有延迟）");
        let shown = Box::pin(create_all_overlay_windows(app.clone())).await?;
        spawn_snapshot_capture(app);
        return Ok(shown);
    }

    // 获取所有已记录的覆盖窗口
    let window_labels: Vec<String> = if let Ok(windows) = OVERLAY_WINDOWS.lock() {
        windows.clone()
    } else {
        Vec::new()
    };

    if window_labels.is_empty() {
        warn!("没有预加载的覆盖窗口，将创建新窗口");
        return Box::pin(create_all_overlay_windows(app)).await;
    }

    let mut shown_count = 0u32;

    // 获取显示器列表，用于发送 overlay-init 事件
    let monitors = app
        .available_monitors()
        .unwrap_or_default();

    for label in &window_labels {
        if let Some(window) = app.get_webview_window(label) {
            // 从窗口标签解析显示器 ID（overlay-0 → 0）
            let monitor_id: usize = label
                .strip_prefix("overlay-")
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);

            // 发送重置事件，通知前端准备新的截图会话
            if let Err(e) = window.emit("overlay-reset", ()) {
                warn!("发送 overlay-reset 事件失败: {}", e);
            }

            // 发送显示器信息到前端（关键！前端需要 monitorInfo 进行坐标转换）
            if let Some(monitor) = monitors.get(monitor_id) {
                let position = monitor.position();
                let size = monitor.size();
                let scale_factor = monitor.scale_factor();
                let monitor_name = monitor
                    .name()
                    .cloned()
                    .unwrap_or_else(|| format!("显示器 {}", monitor_id));

                let monitor_info = serde_json::json!({
                    "monitorId": monitor_id,
                    "position": { "x": position.x, "y": position.y },
                    "size": { "width": size.width, "height": size.height },
                    "scaleFactor": scale_factor,
                    "name": monitor_name,
                });

                debug!("发送 overlay-init 事件: {} -> {:?}", label, monitor_info);
                if let Err(e) = window.emit("overlay-init", monitor_info) {
                    warn!("发送 overlay-init 事件失败: {}", e);
                }
            } else {
                warn!("无法获取显示器 {} 的信息，monitors.len()={}", monitor_id, monitors.len());
            }

            // === 显示窗口并强制获取焦点 ===
            // 这是解决"卡死"问题的关键：确保键盘事件能被 WebView 捕获

            // 1. 显示窗口
            if let Err(e) = window.show() {
                error!("显示覆盖窗口 {} 失败: {}", label, e);
                continue;
            }

            // 2. Windows 专用：刷新 DWM 和强制前台焦点
            #[cfg(windows)]
            {
                // 获取窗口句柄
                if let Ok(hwnd) = window.hwnd() {
                    let hwnd = HWND(hwnd.0);

                    // 刷新 DWM 合成器
                    refresh_window_dwm(hwnd);

                    // 强制获取前台焦点
                    force_foreground_window(hwnd);

                    // 确保 WDA_EXCLUDEFROMCAPTURE 生效（防止截图包含遮罩层变暗）
                    set_exclude_from_capture(hwnd);
                }
            }

            // 3. Tauri 层面设置焦点
            if let Err(e) = window.set_focus() {
                warn!("设置覆盖窗口 {} 焦点失败: {}", label, e);
            }

            // 4. 双重聚焦：通过 eval 确保 WebView 内部获得焦点（关键！）
            if let Err(e) = window.eval("window.focus(); document.body.focus(); if(document.querySelector('.overlay-mask')) document.querySelector('.overlay-mask').focus();") {
                warn!("执行 WebView 焦点脚本失败: {}", e);
            }

            shown_count += 1;
            debug!("覆盖窗口 {} 已显示并获取焦点", label);
        } else {
            warn!("覆盖窗口 {} 不存在，可能已被销毁", label);
        }
    }

    let elapsed = start.elapsed();
    info!("覆盖窗口显示完成，显示 {} 个窗口，耗时 {:?}", shown_count, elapsed);

    // === 第三步：等待预截图完成并缓存 ===
    // 预截图在后台线程执行，通常在 overlay 显示的同时已经完成
    match capture_handle.await {
        Ok(Ok(result)) => {
            info!(
                "预截图完成: {}x{}, 总耗时: {:?}",
                result.width, result.height, start.elapsed()
            );
            set_pre_capture_cache(result);
        }
        Ok(Err(e)) => {
            warn!("预截图失败（将回退到实时截图）: {}", e);
            clear_pre_capture_cache();
        }
        Err(e) => {
            warn!("预截图任务异常: {}", e);
            clear_pre_capture_cache();
        }
    }

    // === 第四步：后台捕获静态快照（不阻塞） ===
    // 仍然后台捕获快照（用于像素级精确匹配，如 OCR 等功能）
    spawn_snapshot_capture(app.clone());

    // 注册临时 Escape 快捷键作为备用取消方式
    if shown_count > 0 {
        register_escape_shortcut(&app);
    }

    Ok(shown_count)
}

/// 隐藏所有覆盖窗口（截图完成或取消时调用）
///
/// 隐藏窗口而不是关闭，以便下次热键触发时能够立即显示。
///
/// # 参数
///
/// - `app`: Tauri 应用句柄
///
/// # 返回
///
/// 成功返回 `Ok(())`，失败返回错误信息
#[tauri::command]
pub async fn hide_overlay_windows(app: tauri::AppHandle) -> HuGeResult<()> {
    info!("隐藏覆盖窗口...");

    // 清除预截图缓存（此次截图会话结束）
    clear_pre_capture_cache();

    // 取消注册临时 Escape 快捷键
    unregister_escape_shortcut(&app);

    // 获取所有已记录的覆盖窗口
    let window_labels: Vec<String> = if let Ok(windows) = OVERLAY_WINDOWS.lock() {
        windows.clone()
    } else {
        Vec::new()
    };

    let mut hidden_count = 0;

    for label in &window_labels {
        if let Some(window) = app.get_webview_window(label) {
            if let Err(e) = window.hide() {
                error!("隐藏覆盖窗口 {} 失败: {}", label, e);
            } else {
                hidden_count += 1;
                debug!("覆盖窗口 {} 已隐藏", label);
            }
        }
    }

    // 同步关闭 OCR 结果窗口
    if let Some(ocr_window) = app.get_webview_window("ocr-result") {
        if let Err(e) = ocr_window.close() {
            warn!("关闭 OCR 结果窗口失败: {}", e);
        }
    }

    info!("已隐藏 {} 个覆盖窗口", hidden_count);
    Ok(())
}

/// 创建覆盖窗口
///
/// 在指定显示器上创建全屏透明覆盖窗口，用于截图选区。
///
/// # 参数
///
/// - `app`: Tauri 应用句柄
/// - `monitor_id`: 目标显示器 ID（从 `get_monitors()` 获取）
///
/// # 返回
///
/// 成功返回 `Ok(())`，失败返回错误信息
///
/// # 窗口属性
///
/// - `transparent`: true - 允许窗口透明
/// - `decorations`: false - 无边框、无标题栏
/// - `always_on_top`: true - 置顶显示
/// - `skip_taskbar`: true - 不在任务栏显示
/// - `resizable`: false - 不可调整大小
/// - `focused`: true - 创建后获取焦点
///
/// # 高 DPI 处理
///
/// 窗口位置和尺寸都使用物理像素，确保在高 DPI 显示器上正确覆盖整个屏幕。
///
/// # 示例
///
/// ```ignore
/// // 在主显示器上创建覆盖窗口
/// create_overlay_window(app, 0).await?;
///
/// // 在所有显示器上创建覆盖窗口
/// let monitors = get_monitors(app.clone()).await?;
/// for monitor in monitors {
///     create_overlay_window(app.clone(), monitor.id).await?;
/// }
/// ```
#[tauri::command]
pub async fn create_overlay_window(
    app: tauri::AppHandle,
    monitor_id: u32,
) -> HuGeResult<()> {
    info!("创建覆盖窗口，目标显示器: {}", monitor_id);

    // 获取所有显示器
    let monitors = app
        .available_monitors()
        .map_err(|e| HuGeError::WindowError(format!("获取显示器列表失败: {}", e)))?;

    if monitors.is_empty() {
        return Err(HuGeError::WindowError("未检测到任何显示器".to_string()));
    }

    // 查找目标显示器
    let target_monitor = monitors
        .get(monitor_id as usize)
        .ok_or_else(|| {
            HuGeError::WindowError(format!(
                "显示器 {} 不存在，可用显示器数量: {}",
                monitor_id,
                monitors.len()
            ))
        })?;

    // 获取显示器位置和尺寸（物理像素）
    let position = target_monitor.position();
    let size = target_monitor.size();
    let scale_factor = target_monitor.scale_factor();
    let monitor_name = target_monitor
        .name()
        .cloned()
        .unwrap_or_else(|| format!("显示器 {}", monitor_id));

    // 关键：将物理像素转换为逻辑像素
    // Tauri 的 position() 和 inner_size() 接受逻辑像素
    let logical_width = size.width as f64 / scale_factor;
    let logical_height = size.height as f64 / scale_factor;
    let logical_x = position.x as f64 / scale_factor;
    let logical_y = position.y as f64 / scale_factor;

    debug!(
        "目标显示器: {} @ ({}, {}), 物理尺寸: {}x{}, 逻辑尺寸: {:.0}x{:.0}, DPR: {:.2}",
        monitor_name, position.x, position.y, size.width, size.height, logical_width, logical_height, scale_factor
    );

    // 生成唯一的窗口标签
    let window_label = format!("overlay-{}", monitor_id);

    // 检查窗口是否已存在
    if let Some(window) = app.get_webview_window(&window_label) {
        warn!("覆盖窗口 {} 已存在，直接显示", window_label);
        // 发送重置事件
        if let Err(e) = window.emit("overlay-reset", ()) {
            warn!("发送 overlay-reset 事件失败: {}", e);
        }
        window.show().map_err(|e| HuGeError::WindowError(format!("显示窗口失败: {}", e)))?;
        window.set_focus().map_err(|e| HuGeError::WindowError(format!("聚焦窗口失败: {}", e)))?;
        return Ok(());
    }

    // 创建覆盖窗口
    let config = OverlayConfig::default();

    let window = WebviewWindowBuilder::new(
        &app,
        &window_label,
        WebviewUrl::App(config.url.into()),
    )
    // 核心属性：透明、无边框、置顶、无阴影
    .transparent(true)
    .decorations(false)
    .shadow(false)  // 关键：禁用窗口阴影，避免位置偏移
    .always_on_top(true)
    // 窗口位置和尺寸（逻辑像素）
    .position(logical_x, logical_y)
    .inner_size(logical_width, logical_height)
    // 窗口行为
    .skip_taskbar(config.skip_taskbar)
    .resizable(config.resizable)
    .maximizable(config.maximizable)
    .minimizable(config.minimizable)
    // 初始状态：先隐藏，等前端加载完成后再显示（避免 WebView2 白屏问题）
    .visible(false)
    .focused(true)
    // 窗口标题（调试用）
    .title(format!("截图覆盖 - {}", monitor_name))
    .build()
    .map_err(|e| HuGeError::WindowError(format!("创建覆盖窗口失败: {}", e)))?;

    // 关键：设置 WDA_EXCLUDEFROMCAPTURE，使 overlay 不被截图捕获
    #[cfg(windows)]
    {
        if let Ok(hwnd) = window.hwnd() {
            set_exclude_from_capture(HWND(hwnd.0));
        }
    }

    // 记录窗口标签
    if let Ok(mut windows) = OVERLAY_WINDOWS.lock() {
        if !windows.contains(&window_label) {
            windows.push(window_label.clone());
        }
    }

    info!(
        "覆盖窗口 {} 创建成功，逻辑位置: ({:.0}, {:.0}), 逻辑尺寸: {:.0}x{:.0}",
        window_label, logical_x, logical_y, logical_width, logical_height
    );

    // 发送显示器信息到前端
    let monitor_info = serde_json::json!({
        "monitorId": monitor_id,
        "position": { "x": position.x, "y": position.y },
        "size": { "width": size.width, "height": size.height },
        "scaleFactor": scale_factor,
        "name": monitor_name,
    });

    if let Err(e) = window.emit("overlay-init", monitor_info) {
        warn!("发送 overlay-init 事件失败: {}", e);
    }

    // 等待前端加载完成后显示窗口（避免白屏）
    // 注意：这是非预加载场景，需要在这里显示窗口
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // 显示窗口
    if let Err(e) = window.show() {
        error!("显示覆盖窗口 {} 失败: {}", window_label, e);
    }

    // Windows 专用：刷新 DWM 和强制前台焦点
    #[cfg(windows)]
    {
        if let Ok(hwnd) = window.hwnd() {
            let hwnd = HWND(hwnd.0);
            refresh_window_dwm(hwnd);
            force_foreground_window(hwnd);
        }
    }

    // Tauri 层面设置焦点
    if let Err(e) = window.set_focus() {
        warn!("设置覆盖窗口 {} 焦点失败: {}", window_label, e);
    }

    // 双重聚焦：通过 eval 确保 WebView 内部获得焦点
    if let Err(e) = window.eval("window.focus(); document.body.focus(); if(document.querySelector('.overlay-mask')) document.querySelector('.overlay-mask').focus();") {
        warn!("执行 WebView 焦点脚本失败: {}", e);
    }

    Ok(())
}

/// 创建所有显示器的覆盖窗口
///
/// 在所有连接的显示器上创建覆盖窗口，用于多显示器截图场景。
/// 如果窗口已预加载，则直接显示。
///
/// # 参数
///
/// - `app`: Tauri 应用句柄
///
/// # 返回
///
/// 成功返回创建/显示的窗口数量，失败返回错误信息
#[tauri::command]
pub async fn create_all_overlay_windows(app: tauri::AppHandle) -> HuGeResult<u32> {
    // 如果已预加载，直接显示
    // 使用 Box::pin 避免 async 递归导致的无限大小 Future
    if OVERLAYS_PRELOADED.load(Ordering::SeqCst) {
        return Box::pin(show_overlay_windows(app)).await;
    }

    info!("创建所有显示器的覆盖窗口...");

    let monitors = app
        .available_monitors()
        .map_err(|e| HuGeError::WindowError(format!("获取显示器列表失败: {}", e)))?;

    let monitor_count = monitors.len() as u32;
    info!("检测到 {} 个显示器", monitor_count);

    let mut created_count = 0u32;
    let mut errors = Vec::new();

    for (index, _monitor) in monitors.iter().enumerate() {
        match create_overlay_window(app.clone(), index as u32).await {
            Ok(()) => {
                created_count += 1;
            }
            Err(e) => {
                error!("创建显示器 {} 的覆盖窗口失败: {}", index, e);
                errors.push(format!("显示器 {}: {}", index, e));
            }
        }
    }

    if !errors.is_empty() {
        warn!("部分覆盖窗口创建失败: {:?}", errors);
    }

    info!("成功创建 {}/{} 个覆盖窗口", created_count, monitor_count);
    Ok(created_count)
}

/// 关闭指定的覆盖窗口
///
/// # 参数
///
/// - `app`: Tauri 应用句柄
/// - `monitor_id`: 目标显示器 ID
#[tauri::command]
pub async fn close_overlay_window(app: tauri::AppHandle, monitor_id: u32) -> HuGeResult<()> {
    let window_label = format!("overlay-{}", monitor_id);
    info!("关闭覆盖窗口: {}", window_label);

    if let Some(window) = app.get_webview_window(&window_label) {
        window
            .close()
            .map_err(|e| HuGeError::WindowError(format!("关闭窗口失败: {}", e)))?;

        // 从记录中移除
        if let Ok(mut windows) = OVERLAY_WINDOWS.lock() {
            windows.retain(|w| w != &window_label);
        }

        info!("覆盖窗口 {} 已关闭", window_label);
    } else {
        debug!("覆盖窗口 {} 不存在，无需关闭", window_label);
    }

    // 如果所有窗口都关闭了，重置预加载标志
    if let Ok(windows) = OVERLAY_WINDOWS.lock() {
        if windows.is_empty() {
            OVERLAYS_PRELOADED.store(false, Ordering::SeqCst);
        }
    }

    Ok(())
}

/// 关闭所有覆盖窗口
///
/// 关闭所有截图选区覆盖窗口，通常在截图完成或取消时调用。
/// 注意：推荐使用 `hide_overlay_windows` 而不是此函数，以保持预加载状态。
///
/// # 参数
///
/// - `app`: Tauri 应用句柄
///
/// # 返回
///
/// 成功返回 `Ok(())`，失败返回错误信息
#[tauri::command]
pub async fn close_all_overlays(app: tauri::AppHandle) -> HuGeResult<()> {
    info!("关闭所有覆盖窗口...");

    // 获取所有已记录的覆盖窗口
    let window_labels: Vec<String> = if let Ok(windows) = OVERLAY_WINDOWS.lock() {
        windows.clone()
    } else {
        Vec::new()
    };

    let mut closed_count = 0;
    let mut errors = Vec::new();

    for label in &window_labels {
        if let Some(window) = app.get_webview_window(label) {
            match window.close() {
                Ok(()) => {
                    closed_count += 1;
                    debug!("覆盖窗口 {} 已关闭", label);
                }
                Err(e) => {
                    error!("关闭覆盖窗口 {} 失败: {}", label, e);
                    errors.push(format!("{}: {}", label, e));
                }
            }
        }
    }

    // 清空记录并重置预加载标志
    if let Ok(mut windows) = OVERLAY_WINDOWS.lock() {
        windows.clear();
    }
    OVERLAYS_PRELOADED.store(false, Ordering::SeqCst);

    if !errors.is_empty() {
        warn!("部分覆盖窗口关闭失败: {:?}", errors);
    }

    info!("已关闭 {} 个覆盖窗口", closed_count);
    Ok(())
}

/// 获取所有覆盖窗口的标签
///
/// 用于调试和状态查询
#[tauri::command]
pub async fn get_overlay_windows() -> HuGeResult<Vec<String>> {
    let windows = OVERLAY_WINDOWS
        .lock()
        .map_err(|e| HuGeError::WindowError(format!("获取窗口列表失败: {}", e)))?;
    Ok(windows.clone())
}

/// 设置覆盖窗口是否忽略鼠标事件
///
/// 当设置为 true 时，鼠标事件会穿透窗口传递给下层窗口。
/// 这在某些场景下有用，比如只显示选区预览而不需要交互。
///
/// # 参数
///
/// - `app`: Tauri 应用句柄
/// - `monitor_id`: 目标显示器 ID
/// - `ignore`: 是否忽略鼠标事件
#[tauri::command]
pub async fn set_overlay_ignore_cursor(
    app: tauri::AppHandle,
    monitor_id: u32,
    ignore: bool,
) -> HuGeResult<()> {
    let window_label = format!("overlay-{}", monitor_id);
    debug!(
        "设置覆盖窗口 {} 忽略鼠标事件: {}",
        window_label, ignore
    );

    let window = app
        .get_webview_window(&window_label)
        .ok_or_else(|| HuGeError::WindowError(format!("覆盖窗口 {} 不存在", window_label)))?;

    window
        .set_ignore_cursor_events(ignore)
        .map_err(|e| HuGeError::WindowError(format!("设置忽略鼠标事件失败: {}", e)))?;

    Ok(())
}

/// 检查覆盖窗口是否已预加载
#[tauri::command]
pub async fn is_overlay_preloaded() -> bool {
    OVERLAYS_PRELOADED.load(Ordering::SeqCst)
}

/// 前端通知后端 overlay 已就绪
///
/// 前端在 DOM 加载完成并准备好接收事件后调用此命令。
/// 后端记录就绪状态，用于判断是否可以安全显示窗口。
#[tauri::command]
pub async fn overlay_ready(app: tauri::AppHandle, monitor_id: u32) -> HuGeResult<()> {
    let window_label = format!("overlay-{}", monitor_id);
    info!("前端 {} 已就绪", window_label);

    // 记录就绪状态
    if let Ok(mut ready_set) = OVERLAY_READY.lock() {
        ready_set.insert(window_label.clone());
    }

    // 再次确保焦点（前端就绪后的双保险）
    if let Some(window) = app.get_webview_window(&window_label) {
        // 如果窗口可见，再次强制焦点
        if window.is_visible().unwrap_or(false) {
            #[cfg(windows)]
            {
                if let Ok(hwnd) = window.hwnd() {
                    let hwnd = HWND(hwnd.0);
                    force_foreground_window(hwnd);
                }
            }

            if let Err(e) = window.set_focus() {
                warn!("overlay_ready 设置焦点失败: {}", e);
            }

            // 再次执行 WebView 焦点脚本
            if let Err(e) = window.eval("window.focus(); document.body.focus(); if(document.querySelector('.overlay-mask')) document.querySelector('.overlay-mask').focus();") {
                warn!("overlay_ready 执行焦点脚本失败: {}", e);
            }
        }
    }

    Ok(())
}

/// 强制恢复 overlay 窗口焦点
///
/// 当 overlay 在截图会话中丢失焦点时，前端调用此命令
/// 使用 Windows API 强制将窗口设为前台并置顶。
#[tauri::command]
pub async fn overlay_force_focus(app: tauri::AppHandle) -> HuGeResult<()> {
    // 查找当前活跃的 overlay 窗口
    for window in app.webview_windows().values() {
        let label = window.label();
        if label.starts_with("overlay-") {
            if let Ok(true) = window.is_visible() {
                debug!("强制恢复 overlay 焦点: {}", label);

                // 使用 Windows API 强制前台焦点
                #[cfg(windows)]
                {
                    if let Ok(tauri_hwnd) = window.hwnd() {
                        // Tauri 的 HWND (windows 0.61) 和项目的 HWND (windows 0.58)
                        // 类型不同，通过原始指针转换
                        let raw_hwnd = tauri_hwnd.0;
                        let hwnd = HWND(raw_hwnd);
                        force_foreground_window(hwnd);
                    }
                }

                // Tauri 层面也设置焦点
                let _ = window.set_focus();

                // WebView 焦点
                let _ = window.eval("window.focus();");

                break;
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_overlay_config_default() {
        let config = OverlayConfig::default();
        assert_eq!(config.label_prefix, "overlay");
        assert!(config.skip_taskbar);
        assert!(!config.resizable);
    }

    #[test]
    fn test_window_label_format() {
        let label = format!("overlay-{}", 0);
        assert_eq!(label, "overlay-0");

        let label = format!("overlay-{}", 1);
        assert_eq!(label, "overlay-1");
    }
}
