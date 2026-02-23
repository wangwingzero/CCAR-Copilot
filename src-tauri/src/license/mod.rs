//! 许可证验证服务模块
//!
//! 提供用户订阅许可证的验证功能：
//! - 24 小时缓存
//! - 7 天离线宽限期
//! - 本地持久化

mod service;

pub use service::{LicenseService, LicenseInfo, Plan};
