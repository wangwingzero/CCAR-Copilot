//! Tauri 命令模块
//!
//! 本模块定义所有暴露给前端的 Tauri 命令。
//! 命令按功能分类到不同的子模块中。
//!
//! # 子模块
//!
//! - `screenshot_cmd`: 截图相关命令
//! - `hotkey_cmd`: 热键相关命令
//! - `window_cmd`: 窗口相关命令
//! - `sidecar_cmd`: Sidecar 相关命令
//! - `auth_cmd`: 认证相关命令
//! - `license_cmd`: 许可证相关命令
//! - `payment_cmd`: 支付相关命令
//! - `usage_cmd`: 使用量追踪命令
//! - `feature_gate_cmd`: 功能门控命令
//! - `history_cmd`: 历史记录相关命令
//! - `device_cmd`: 设备管理相关命令
//! - `tray_cmd`: 托盘相关命令
//! - `config_cmd`: 配置相关命令
//! - `update_cmd`: 自动更新相关命令
//! - `mouse_highlight_cmd`: 鼠标高亮相关命令
//! - `clipboard_cmd`: 剪贴板相关命令
//! - `shutdown_cmd`: 定时关机相关命令
//! - `file_search_cmd`: 文件搜索相关命令
//! - `file_cmd`: 文件操作相关命令
//! - `converter_cmd`: 文件转 Markdown 命令（纯 Rust 实现）

pub mod anki_cmd; // 原生 Anki 命令（直接 HTTP 调用 AnkiConnect，无需 Sidecar）
pub mod auth_cmd;
pub mod clipboard_cmd;
pub mod converter_cmd;
pub mod file_cmd;
pub mod config_cmd;
pub mod device_cmd;
pub mod feature_gate_cmd;
pub mod file_search_cmd;
pub mod history_cmd;
pub mod hotkey_cmd;
pub mod license_cmd;
pub mod mouse_highlight_cmd;
pub mod payment_cmd;
pub mod recording_cmd; // 原生录屏命令（替代 sidecar 录屏）
pub mod screenshot_cmd;
pub mod shutdown_cmd;
pub mod sidecar_cmd;
pub mod tray_cmd;
pub mod update_cmd;
pub mod usage_cmd;
pub mod window_cmd;

// 重新导出所有命令，方便在 lib.rs 中注册
pub use anki_cmd::*;
pub use auth_cmd::*;
pub use clipboard_cmd::*;
pub use config_cmd::*;
pub use converter_cmd::*;
pub use device_cmd::*;
pub use feature_gate_cmd::*;
pub use file_cmd::*;
pub use file_search_cmd::*;
pub use history_cmd::*;
pub use hotkey_cmd::*;
pub use license_cmd::*;
pub use mouse_highlight_cmd::*;
pub use payment_cmd::*;
pub use screenshot_cmd::*;
pub use shutdown_cmd::*;
pub use sidecar_cmd::*;
pub use tray_cmd::*;
pub use update_cmd::*;
pub use usage_cmd::*;
pub use window_cmd::*;
