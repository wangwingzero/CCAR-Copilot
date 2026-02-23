//! 虎皮椒支付服务实现
//!
//! 官方文档: https://www.xunhupay.com/doc/api/pay.html

use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use tracing::{debug, error, info};

/// 虎皮椒 API 配置
const XUNHUPAY_API_URL: &str = "https://api.xunhupay.com/payment/do.html";
const XUNHUPAY_QUERY_URL: &str = "https://api.xunhupay.com/payment/query.html";

/// 支付方式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaymentMethod {
    /// 支付宝
    Alipay,
    /// 微信支付
    Wechat,
}

impl std::fmt::Display for PaymentMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PaymentMethod::Alipay => write!(f, "alipay"),
            PaymentMethod::Wechat => write!(f, "wechat"),
        }
    }
}

/// 支付状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PaymentStatus {
    /// 待支付
    Pending,
    /// 已支付
    Paid,
    /// 已取消
    Cancelled,
    /// 已退款
    Refunded,
    /// 失败
    Failed,
}

impl From<&str> for PaymentStatus {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "ots" | "pending" => PaymentStatus::Pending,
            "complete" | "paid" | "success" => PaymentStatus::Paid,
            "cancelled" | "cancel" => PaymentStatus::Cancelled,
            "refunded" | "refund" => PaymentStatus::Refunded,
            _ => PaymentStatus::Failed,
        }
    }
}

/// 支付订单
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentOrder {
    /// 商户订单号
    pub order_no: String,
    /// 订单金额（元）
    pub amount: f64,
    /// 支付方式
    pub payment_method: PaymentMethod,
    /// 订单状态
    pub status: PaymentStatus,
    /// 支付链接/二维码内容
    pub pay_url: Option<String>,
    /// 二维码图片 URL
    pub qr_code_url: Option<String>,
    /// 虎皮椒交易号
    pub trade_no: Option<String>,
    /// 创建时间
    pub created_at: DateTime<Utc>,
    /// 支付时间
    pub paid_at: Option<DateTime<Utc>>,
}

/// 虎皮椒 API 响应
#[derive(Debug, Deserialize)]
struct XunhuPayResponse {
    /// 状态码（1 成功）
    errcode: i32,
    /// 错误信息
    #[serde(default)]
    errmsg: String,
    /// 支付链接
    #[serde(default)]
    url: Option<String>,
    /// 二维码链接
    #[serde(default)]
    url_qrcode: Option<String>,
    /// 订单号
    #[serde(default)]
    order_id: Option<String>,
    /// 交易状态
    #[serde(default)]
    status: Option<String>,
    /// 支付时间
    #[serde(default)]
    pay_time: Option<String>,
}

/// 支付服务
pub struct PaymentService {
    /// HTTP 客户端
    client: Client,
    /// 应用 ID
    app_id: String,
    /// 应用密钥
    app_secret: String,
    /// 回调通知 URL
    notify_url: String,
}

impl PaymentService {
    /// 创建支付服务
    pub fn new(app_id: String, app_secret: String, notify_url: String) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap();

        Self {
            client,
            app_id,
            app_secret,
            notify_url,
        }
    }

    /// 从环境变量创建
    pub fn from_env() -> Result<Self, String> {
        let app_id = std::env::var("XUNHUPAY_APP_ID")
            .unwrap_or_else(|_| "202501122317301".to_string()); // 默认测试 ID
        let app_secret = std::env::var("XUNHUPAY_APP_SECRET")
            .unwrap_or_else(|_| "9c1b9679d85d4a89dc84aa15f4e2dc15".to_string()); // 默认测试密钥
        let notify_url = std::env::var("XUNHUPAY_NOTIFY_URL")
            .unwrap_or_else(|_| {
                "https://trkjgcjblqfqypimddwx.supabase.co/functions/v1/xunhu-webhook".to_string()
            });

        Ok(Self::new(app_id, app_secret, notify_url))
    }

    /// 创建支付订单
    ///
    /// # Arguments
    /// * `amount` - 订单金额（元）
    /// * `method` - 支付方式
    /// * `title` - 商品名称
    ///
    /// # Returns
    /// 返回包含支付链接和二维码的订单信息
    pub async fn create_order(
        &self,
        amount: f64,
        method: PaymentMethod,
        title: &str,
    ) -> Result<PaymentOrder, String> {
        // 生成订单号
        let order_no = self.generate_order_no();

        // 构建请求参数
        let mut params = BTreeMap::new();
        params.insert("version".to_string(), "1.1".to_string());
        params.insert("appid".to_string(), self.app_id.clone());
        params.insert("trade_order_id".to_string(), order_no.clone());
        params.insert("total_fee".to_string(), format!("{:.2}", amount));
        params.insert("title".to_string(), title.to_string());
        params.insert("time".to_string(), Utc::now().timestamp().to_string());
        params.insert("notify_url".to_string(), self.notify_url.clone());
        params.insert("nonce_str".to_string(), self.generate_nonce());

        // 支付方式
        let type_str = match method {
            PaymentMethod::Alipay => "alipay",
            PaymentMethod::Wechat => "wechat",
        };
        params.insert("type".to_string(), type_str.to_string());

        // 计算签名
        let sign = self.sign_request(&params);
        params.insert("hash".to_string(), sign);

        info!("创建支付订单: order_no={}, amount={}, method={}", order_no, amount, type_str);

        // 发送请求
        let response = self
            .client
            .post(XUNHUPAY_API_URL)
            .form(&params)
            .send()
            .await
            .map_err(|e| format!("请求支付接口失败: {}", e))?;

        let resp: XunhuPayResponse = response
            .json()
            .await
            .map_err(|e| format!("解析响应失败: {}", e))?;

        if resp.errcode != 0 {
            error!("创建订单失败: {}", resp.errmsg);
            return Err(format!("创建订单失败: {}", resp.errmsg));
        }

        info!("订单创建成功: order_no={}", order_no);

        Ok(PaymentOrder {
            order_no,
            amount,
            payment_method: method,
            status: PaymentStatus::Pending,
            pay_url: resp.url,
            qr_code_url: resp.url_qrcode,
            trade_no: resp.order_id,
            created_at: Utc::now(),
            paid_at: None,
        })
    }

    /// 查询订单状态
    pub async fn query_order(&self, order_no: &str) -> Result<PaymentOrder, String> {
        let mut params = BTreeMap::new();
        params.insert("appid".to_string(), self.app_id.clone());
        params.insert("out_trade_order".to_string(), order_no.to_string());
        params.insert("time".to_string(), Utc::now().timestamp().to_string());
        params.insert("nonce_str".to_string(), self.generate_nonce());

        let sign = self.sign_request(&params);
        params.insert("hash".to_string(), sign);

        debug!("查询订单状态: order_no={}", order_no);

        let response = self
            .client
            .post(XUNHUPAY_QUERY_URL)
            .form(&params)
            .send()
            .await
            .map_err(|e| format!("查询订单失败: {}", e))?;

        let resp: XunhuPayResponse = response
            .json()
            .await
            .map_err(|e| format!("解析响应失败: {}", e))?;

        if resp.errcode != 0 {
            return Err(format!("查询订单失败: {}", resp.errmsg));
        }

        let status = resp
            .status
            .as_deref()
            .map(PaymentStatus::from)
            .unwrap_or(PaymentStatus::Pending);

        let paid_at = if status == PaymentStatus::Paid {
            resp.pay_time
                .as_ref()
                .and_then(|t| chrono::NaiveDateTime::parse_from_str(t, "%Y-%m-%d %H:%M:%S").ok())
                .map(|dt| DateTime::from_naive_utc_and_offset(dt, Utc))
        } else {
            None
        };

        Ok(PaymentOrder {
            order_no: order_no.to_string(),
            amount: 0.0, // 查询接口不返回金额
            payment_method: PaymentMethod::Alipay, // 查询接口不返回支付方式
            status,
            pay_url: None,
            qr_code_url: None,
            trade_no: resp.order_id,
            created_at: Utc::now(),
            paid_at,
        })
    }

    /// 生成订单号
    ///
    /// 格式: HG + 时间戳 + 4位随机数
    fn generate_order_no(&self) -> String {
        let timestamp = Utc::now().format("%Y%m%d%H%M%S");
        let random: u32 = rand::random::<u32>() % 10000;
        format!("HG{}{:04}", timestamp, random)
    }

    /// 生成随机字符串
    fn generate_nonce(&self) -> String {
        const CHARSET: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let mut rng = rand::thread_rng();
        (0..16)
            .map(|_| {
                let idx = rand::Rng::gen_range(&mut rng, 0..CHARSET.len());
                CHARSET[idx] as char
            })
            .collect()
    }

    /// 计算请求签名
    ///
    /// 签名规则：
    /// 1. 按 key 字母序排序
    /// 2. 拼接成 key=value&key2=value2 格式
    /// 3. 末尾追加 app_secret
    /// 4. MD5 加密
    fn sign_request(&self, params: &BTreeMap<String, String>) -> String {
        let mut pairs: Vec<String> = params
            .iter()
            .filter(|(k, v)| !v.is_empty() && *k != "hash")
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        pairs.sort();

        let sign_str = format!("{}{}", pairs.join("&"), self.app_secret);

        let digest = md5::compute(sign_str.as_bytes());
        format!("{:x}", digest)
    }

    /// 验证回调签名
    pub fn verify_callback(&self, params: &BTreeMap<String, String>) -> bool {
        let provided_hash = match params.get("hash") {
            Some(h) => h.clone(),
            None => return false,
        };

        let calculated_hash = self.sign_request(params);
        provided_hash == calculated_hash
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_payment_status_from_str() {
        assert_eq!(PaymentStatus::from("OTS"), PaymentStatus::Pending);
        assert_eq!(PaymentStatus::from("complete"), PaymentStatus::Paid);
        assert_eq!(PaymentStatus::from("paid"), PaymentStatus::Paid);
        assert_eq!(PaymentStatus::from("unknown"), PaymentStatus::Failed);
    }

    #[test]
    fn test_generate_order_no() {
        let service = PaymentService::from_env().unwrap();
        let order1 = service.generate_order_no();

        // 订单号应该以 HG 开头
        assert!(order1.starts_with("HG"));
        // 长度: HG(2) + 时间戳(14) + 随机数(4) = 20
        assert_eq!(order1.len(), 20);
    }

    #[test]
    fn test_sign_request() {
        let service = PaymentService::new(
            "test_app_id".to_string(),
            "test_secret".to_string(),
            "https://example.com/notify".to_string(),
        );

        let mut params = BTreeMap::new();
        params.insert("appid".to_string(), "test_app_id".to_string());
        params.insert("amount".to_string(), "100.00".to_string());

        let sign = service.sign_request(&params);

        // 签名应该是 32 位十六进制字符串
        assert_eq!(sign.len(), 32);
        assert!(sign.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
