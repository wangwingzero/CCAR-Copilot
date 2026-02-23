//! 单实例锁模块
//!
//! 使用 Windows Mutex 防止应用重复启动。
//! 检测到已有实例时，激活已有窗口。
//!
//! # 注意
//! 此模块为独立模块，暂不集成到 main.rs（Phase 2 集成）

mod lock;

pub use lock::{SingleInstanceLock, SingleInstanceError};
