//! Tauri 命令模块
//!
//! 本模块定义所有暴露给前端的 Tauri 命令。
//! 命令按功能分类到不同的子模块中。
//!
//! # 子模块
//!
//! - `config_cmd`: 配置相关命令
//! - `file_cmd`: 文件操作相关命令
//! - `tray_cmd`: 托盘相关命令
//! - `update_cmd`: 自动更新相关命令

pub mod config_cmd;
pub mod file_cmd;
pub mod file_search_cmd;
pub mod tray_cmd;
pub mod update_cmd;

// 重新导出所有命令，方便在 lib.rs 中注册
pub use config_cmd::*;
pub use file_cmd::*;
pub use tray_cmd::*;
pub use update_cmd::*;
