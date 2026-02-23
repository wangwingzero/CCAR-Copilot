//! Sidecar 通信协议
//!
//! 定义 Rust 与 Python Sidecar 之间的通信协议。
//! 使用 stdin/stdout 进行 JSON 格式的请求/响应通信。

use serde::{Deserialize, Serialize};

/// Sidecar 请求
///
/// 发送给 Python Sidecar 的请求格式
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidecarRequest {
    /// 请求 ID（UUID），用于匹配响应
    pub id: String,
    /// 服务名称：ocr, translate, anki, web, record, document
    pub service: String,
    /// 方法名称
    pub method: String,
    /// 参数（JSON 对象）
    pub params: serde_json::Value,
}

/// Sidecar 响应
///
/// Python Sidecar 返回的响应格式
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SidecarResponse {
    /// 响应 ID，与请求 ID 匹配
    pub id: String,
    /// 是否成功
    pub success: bool,
    /// 成功时的结果
    pub result: Option<serde_json::Value>,
    /// 失败时的错误信息
    pub error: Option<String>,
}

impl SidecarRequest {
    /// 创建新的 Sidecar 请求
    ///
    /// # 参数
    ///
    /// - `service`: 服务名称
    /// - `method`: 方法名称
    /// - `params`: 参数
    ///
    /// # 示例
    ///
    /// ```
    /// use hugescreenshot_tauri_lib::sidecar::protocol::SidecarRequest;
    /// use serde_json::json;
    ///
    /// let request = SidecarRequest::new(
    ///     "ocr",
    ///     "recognize",
    ///     json!({ "image_path": "/tmp/screenshot.png" })
    /// );
    /// ```
    pub fn new(service: &str, method: &str, params: serde_json::Value) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            service: service.to_string(),
            method: method.to_string(),
            params,
        }
    }

    /// 将请求序列化为 JSON 行（用于 stdin 发送）
    pub fn to_json_line(&self) -> Result<String, serde_json::Error> {
        let mut json = serde_json::to_string(self)?;
        json.push('\n');
        Ok(json)
    }
}

impl SidecarResponse {
    /// 从 JSON 行解析响应
    pub fn from_json_line(line: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(line.trim())
    }

    /// 检查响应是否成功
    pub fn is_success(&self) -> bool {
        self.success && self.error.is_none()
    }

    /// 获取结果，如果失败则返回错误
    pub fn into_result(self) -> Result<serde_json::Value, String> {
        if self.success {
            self.result.ok_or_else(|| "响应成功但结果为空".to_string())
        } else {
            Err(self.error.unwrap_or_else(|| "未知错误".to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_request_new() {
        let request = SidecarRequest::new(
            "ocr",
            "recognize",
            json!({ "image_path": "/tmp/test.png" }),
        );
        assert_eq!(request.service, "ocr");
        assert_eq!(request.method, "recognize");
        assert!(!request.id.is_empty());
    }

    #[test]
    fn test_request_to_json_line() {
        let request = SidecarRequest::new("ocr", "recognize", json!({}));
        let line = request.to_json_line().unwrap();
        assert!(line.ends_with('\n'));
        assert!(line.contains("ocr"));
    }

    #[test]
    fn test_response_from_json_line() {
        let json = r#"{"id":"123","success":true,"result":{"text":"Hello"},"error":null}"#;
        let response = SidecarResponse::from_json_line(json).unwrap();
        assert_eq!(response.id, "123");
        assert!(response.success);
        assert!(response.is_success());
    }

    #[test]
    fn test_response_into_result_success() {
        let response = SidecarResponse {
            id: "123".to_string(),
            success: true,
            result: Some(json!({"text": "Hello"})),
            error: None,
        };
        let result = response.into_result().unwrap();
        assert_eq!(result["text"], "Hello");
    }

    #[test]
    fn test_response_into_result_error() {
        let response = SidecarResponse {
            id: "123".to_string(),
            success: false,
            result: None,
            error: Some("OCR 失败".to_string()),
        };
        let err = response.into_result().unwrap_err();
        assert_eq!(err, "OCR 失败");
    }
}
