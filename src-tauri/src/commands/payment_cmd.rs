//! 支付相关的 Tauri 命令

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;
use tracing::{error, info};

use crate::payment::{PaymentMethod, PaymentOrder, PaymentService, PaymentStatus};

/// 支付状态（全局共享）
pub struct PaymentState {
    pub service: Arc<Mutex<PaymentService>>,
}

impl PaymentState {
    pub fn new() -> Result<Self, String> {
        let service = PaymentService::from_env()?;
        Ok(Self {
            service: Arc::new(Mutex::new(service)),
        })
    }
}

/// 初始化支付状态
pub fn init_payment_state() -> Result<PaymentState, String> {
    PaymentState::new()
}

/// 创建订单请求
#[derive(Debug, Deserialize)]
pub struct CreateOrderRequest {
    /// 订单金额（元）
    pub amount: f64,
    /// 支付方式: "alipay" | "wechat"
    pub payment_method: String,
    /// 商品名称
    #[serde(default = "default_title")]
    pub title: String,
}

fn default_title() -> String {
    "虎哥截图 终身VIP".to_string()
}

/// 订单响应
#[derive(Debug, Serialize)]
pub struct OrderResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<OrderInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 订单信息（简化版）
#[derive(Debug, Serialize)]
pub struct OrderInfo {
    pub order_no: String,
    pub amount: f64,
    pub payment_method: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pay_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qr_code_url: Option<String>,
    pub created_at: String,
}

impl From<PaymentOrder> for OrderInfo {
    fn from(order: PaymentOrder) -> Self {
        Self {
            order_no: order.order_no,
            amount: order.amount,
            payment_method: order.payment_method.to_string(),
            status: format!("{:?}", order.status).to_lowercase(),
            pay_url: order.pay_url,
            qr_code_url: order.qr_code_url,
            created_at: order.created_at.to_rfc3339(),
        }
    }
}

/// 创建支付订单
#[tauri::command]
pub async fn create_payment_order(
    request: CreateOrderRequest,
    state: State<'_, PaymentState>,
) -> Result<OrderResponse, String> {
    let method = match request.payment_method.to_lowercase().as_str() {
        "alipay" => PaymentMethod::Alipay,
        "wechat" => PaymentMethod::Wechat,
        _ => {
            return Ok(OrderResponse {
                success: false,
                order: None,
                error: Some("不支持的支付方式".to_string()),
            });
        }
    };

    let service = state.service.lock().await;

    match service.create_order(request.amount, method, &request.title).await {
        Ok(order) => {
            info!("订单创建成功: {}", order.order_no);
            Ok(OrderResponse {
                success: true,
                order: Some(OrderInfo::from(order)),
                error: None,
            })
        }
        Err(e) => {
            error!("订单创建失败: {}", e);
            Ok(OrderResponse {
                success: false,
                order: None,
                error: Some(e),
            })
        }
    }
}

/// 查询订单状态
#[tauri::command]
pub async fn query_payment_order(
    order_no: String,
    state: State<'_, PaymentState>,
) -> Result<OrderResponse, String> {
    let service = state.service.lock().await;

    match service.query_order(&order_no).await {
        Ok(order) => {
            Ok(OrderResponse {
                success: true,
                order: Some(OrderInfo::from(order)),
                error: None,
            })
        }
        Err(e) => {
            error!("查询订单失败: {}", e);
            Ok(OrderResponse {
                success: false,
                order: None,
                error: Some(e),
            })
        }
    }
}

/// 订单状态响应
#[derive(Debug, Serialize)]
pub struct OrderStatusResponse {
    pub is_paid: bool,
    pub status: String,
}

/// 检查订单是否已支付（用于轮询）
#[tauri::command]
pub async fn check_payment_status(
    order_no: String,
    state: State<'_, PaymentState>,
) -> Result<OrderStatusResponse, String> {
    let service = state.service.lock().await;

    match service.query_order(&order_no).await {
        Ok(order) => {
            let is_paid = order.status == PaymentStatus::Paid;
            Ok(OrderStatusResponse {
                is_paid,
                status: format!("{:?}", order.status).to_lowercase(),
            })
        }
        Err(_e) => {
            Ok(OrderStatusResponse {
                is_paid: false,
                status: "error".to_string(),
            })
        }
    }
}
