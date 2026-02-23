//! Python Sidecar 管理模块
//!
//! 本模块负责 Python Sidecar 进程的生命周期管理和通信。
//!
//! # 架构说明
//!
//! 使用 **Sidecar 架构**（独立进程 + IPC），而非 PyO3/maturin 嵌入式架构：
//!
//! | 架构 | 通信方式 | 优点 | 缺点 |
//! |------|----------|------|------|
//! | **Sidecar (本项目)** | 独立进程 + JSON | 隔离性好，崩溃不影响主程序 | 通信开销稍高 |
//! | PyO3/maturin | FFI 直接调用 | 性能更高 | 耦合紧密，panic 会崩溃主程序 |
//!
//! # 通信协议
//!
//! - 使用 stdin/stdout 进行 JSON 格式的请求/响应通信
//! - 每条消息以换行符结尾
//! - Python 端必须使用 `flush=True` 刷新缓冲区
//!
//! # 子模块
//!
//! - `manager`: Sidecar 进程管理器，负责启动、通信、崩溃重启
//! - `protocol`: 通信协议定义，包括请求和响应格式
//!
//! # 使用示例
//!
//! ```ignore
//! use hugescreenshot_tauri_lib::sidecar::{SidecarManager, SidecarState};
//!
//! let manager = SidecarManager::new(app_handle);
//! manager.start().await?;
//!
//! let result = manager.call("ocr", "recognize", json!({
//!     "image_path": "/tmp/screenshot.png"
//! })).await?;
//!
//! manager.stop().await?;
//! ```

pub mod manager;
pub mod protocol;

// 重新导出常用类型
pub use manager::{SidecarManager, SidecarState};
pub use protocol::{SidecarRequest, SidecarResponse};
