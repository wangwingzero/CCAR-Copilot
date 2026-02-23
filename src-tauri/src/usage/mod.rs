//! 使用量追踪服务模块
//!
//! 追踪用户的功能使用次数：
//! - 本地 SQLite 存储
//! - 午夜自动重置
//! - 云端同步

mod tracker;

pub use tracker::{UsageTracker, UsageStats, Feature as UsageFeature};
