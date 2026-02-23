//! 支付服务模块
//!
//! 集成虎皮椒支付平台，提供：
//! - 订单创建
//! - 支付二维码生成
//! - 订单状态查询

mod xunhupay;

pub use xunhupay::{PaymentService, PaymentOrder, PaymentStatus, PaymentMethod};
