//! 功能门控系统
//!
//! 整合许可证验证和使用量追踪，提供统一的功能访问控制。
//!
//! # 功能等级
//!
//! - 免费用户：基础功能 + 每日使用限制
//! - VIP 用户：全部功能 + 无使用限制
//!
//! # 使用示例
//!
//! ```ignore
//! let gate = FeatureGate::new(license, usage);
//! let access = gate.check_access(Feature::Translation).await?;
//! if access.allowed {
//!     // 执行功能
//! } else {
//!     // 显示限制提示
//! }
//! ```

mod gate;

pub use gate::{FeatureGate, FeatureAccess, FeatureTier};
