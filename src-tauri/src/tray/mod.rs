//! 系统托盘模块
//!
//! 提供系统托盘功能，包括：
//! - 托盘图标显示
//! - 右键菜单
//! - 最小化到托盘
//! - 双击显示主窗口

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use tauri::{
    image::Image,
    menu::{Menu, MenuBuilder, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, Runtime, Wry,
};
use tracing::{error, info, warn};

/// 托盘状态枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TrayState {
    /// 空闲状态
    Idle = 0,
    /// 处理中（OCR 等）
    Processing = 2,
}

impl From<u8> for TrayState {
    fn from(value: u8) -> Self {
        match value {
            2 => TrayState::Processing,
            _ => TrayState::Idle,
        }
    }
}

/// 托盘状态管理器
pub struct TrayStateManager {
    state: AtomicU8,
}

impl Default for TrayStateManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TrayStateManager {
    pub fn new() -> Self {
        Self { state: AtomicU8::new(TrayState::Idle as u8) }
    }

    pub fn get_state(&self) -> TrayState {
        TrayState::from(self.state.load(Ordering::SeqCst))
    }

    pub fn set_state(&self, state: TrayState) {
        self.state.store(state as u8, Ordering::SeqCst);
    }
}

/// 托盘图标 ID
pub const TRAY_ID: &str = "main-tray";

/// 菜单项 ID
pub mod menu_ids {
    pub const SHOW_WINDOW: &str = "show_window";
    pub const SETTINGS: &str = "settings";
    pub const CHECK_UPDATE: &str = "check_update";
    pub const EXIT: &str = "exit";
}

/// 创建托盘菜单
fn create_tray_menu<R: Runtime>(app: &AppHandle<R>) -> Result<Menu<R>, String> {
    let show_window_item =
        MenuItem::with_id(app, menu_ids::SHOW_WINDOW, "显示主窗口", true, None::<&str>)
            .map_err(|e| format!("创建显示窗口菜单项失败: {}", e))?;

    let settings_item = MenuItem::with_id(app, menu_ids::SETTINGS, "设置", true, None::<&str>)
        .map_err(|e| format!("创建设置菜单项失败: {}", e))?;

    // v0.1.6 新增: 一键检查更新。点击后 Rust emit `tray-check-update` 事件,
    // 前端 `useUpdate.ts` 设置了监听器会静默触发一次检查;若发现新版本
    // 会弹 toast 提示用户进设置面板下载。
    let check_update_item =
        MenuItem::with_id(app, menu_ids::CHECK_UPDATE, "检查更新", true, None::<&str>)
            .map_err(|e| format!("创建检查更新菜单项失败: {}", e))?;

    let exit_item = MenuItem::with_id(app, menu_ids::EXIT, "退出", true, None::<&str>)
        .map_err(|e| format!("创建退出菜单项失败: {}", e))?;

    let separator =
        PredefinedMenuItem::separator(app).map_err(|e| format!("创建分隔符失败: {}", e))?;

    MenuBuilder::new(app)
        .item(&show_window_item)
        .item(&settings_item)
        .item(&check_update_item)
        .item(&separator)
        .item(&exit_item)
        .build()
        .map_err(|e| format!("构建托盘菜单失败: {}", e))
}

/// 处理托盘菜单事件
fn handle_menu_event(app: &AppHandle<Wry>, event_id: &str) {
    info!("托盘菜单点击: {}", event_id);

    match event_id {
        menu_ids::SHOW_WINDOW => {
            show_main_window(app);
        }
        menu_ids::SETTINGS => {
            info!("用户请求打开设置");
            show_main_window(app);
            let _ = app.emit("open-settings", ());
        }
        menu_ids::CHECK_UPDATE => {
            info!("用户从托盘请求检查更新");
            // 不强制打开主窗口也不跳设置面板，静默触发一次 check；
            // useUpdate 的 watch(status) 会在发现新版本时弹 toast，
            // 但 toast 只在 webview 可见时才能看到，所以顺便把窗口拉出来。
            show_main_window(app);
            let _ = app.emit("tray-check-update", ());
        }
        menu_ids::EXIT => {
            info!("用户请求退出应用");
            app.exit(0);
        }
        _ => {
            warn!("未知的菜单项: {}", event_id);
        }
    }
}

/// 显示主窗口
pub fn show_main_window(app: &AppHandle<Wry>) {
    if let Some(window) = app.get_webview_window("main") {
        if let Ok(true) = window.is_minimized() {
            if let Err(e) = window.unminimize() {
                error!("恢复窗口失败: {}", e);
            }
        }

        if let Err(e) = window.show() {
            error!("显示窗口失败: {}", e);
        }

        if let Err(e) = window.set_focus() {
            error!("设置窗口焦点失败: {}", e);
        }

        info!("主窗口已显示");
    } else {
        warn!("找不到主窗口");
    }
}

/// 隐藏主窗口到托盘
pub fn hide_to_tray(app: &AppHandle<Wry>) {
    if let Some(window) = app.get_webview_window("main") {
        if let Err(e) = window.hide() {
            error!("隐藏窗口失败: {}", e);
        } else {
            info!("主窗口已隐藏到托盘");
        }
    }
}

/// 设置托盘图标状态
pub fn set_tray_state(app: &AppHandle<Wry>, state: TrayState) -> Result<(), String> {
    if let Some(state_manager) = app.try_state::<Arc<TrayStateManager>>() {
        state_manager.set_state(state);
    }

    let tray = app.tray_by_id(TRAY_ID).ok_or_else(|| "找不到托盘图标".to_string())?;

    let tooltip = match state {
        TrayState::Idle => "CCAR Copilot",
        TrayState::Processing => "CCAR Copilot - 处理中...",
    };

    tray.set_tooltip(Some(tooltip)).map_err(|e| format!("设置托盘提示失败: {}", e))?;

    info!("托盘状态已更新: {:?}", state);
    Ok(())
}

/// 初始化系统托盘
pub fn setup_tray(app: &AppHandle<Wry>) -> Result<(), String> {
    info!("初始化系统托盘...");

    let menu = create_tray_menu(app)?;

    let icon_data = include_bytes!("../../../resources/PNG/huge-ccar.png");
    let icon = Image::from_bytes(icon_data).map_err(|e| format!("无法加载托盘图标: {}", e))?;

    let _tray = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .menu(&menu)
        .tooltip("CCAR Copilot")
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            handle_menu_event(app, event.id.as_ref());
        })
        .on_tray_icon_event(|tray, event| {
            let app = tray.app_handle();

            match event {
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } => {
                    info!("托盘图标左键单击，显示主窗口");
                    show_main_window(app);
                }
                TrayIconEvent::DoubleClick { button: MouseButton::Left, .. } => {
                    info!("托盘图标双击");
                    show_main_window(app);
                }
                _ => {}
            }
        })
        .build(app)
        .map_err(|e| format!("创建托盘图标失败: {}", e))?;

    let state_manager = Arc::new(TrayStateManager::new());
    app.manage(state_manager);

    info!("系统托盘初始化完成");
    Ok(())
}

/// 处理窗口关闭事件 - 最小化到托盘而不是退出
pub fn handle_close_requested(app: &AppHandle<Wry>, label: &str) -> bool {
    if label == "main" {
        info!("主窗口关闭请求，最小化到托盘");
        hide_to_tray(app);
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tray_state_conversion() {
        assert_eq!(TrayState::from(0), TrayState::Idle);
        assert_eq!(TrayState::from(2), TrayState::Processing);
        assert_eq!(TrayState::from(255), TrayState::Idle);
    }

    #[test]
    fn test_tray_state_manager() {
        let manager = TrayStateManager::new();
        assert_eq!(manager.get_state(), TrayState::Idle);

        manager.set_state(TrayState::Processing);
        assert_eq!(manager.get_state(), TrayState::Processing);

        manager.set_state(TrayState::Idle);
        assert_eq!(manager.get_state(), TrayState::Idle);
    }
}
