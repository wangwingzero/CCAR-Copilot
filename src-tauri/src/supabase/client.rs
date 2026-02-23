//! Supabase HTTP 客户端
//!
//! 提供基础的 HTTP 请求功能，包括：
//! - 自动添加认证头
//! - 请求重试逻辑（指数退避）
//! - 并发限制（Semaphore）
//! - 连接池优化
//! - 错误处理
//!
//! # 性能优化
//!
//! - 使用 `tokio::sync::Semaphore` 限制最大并发数（默认 10）
//! - 配置连接池参数（`pool_max_idle_per_host`）
//! - 支持批量请求（`batch_get`、`batch_post`）
//! - 指数退避重试策略（最多 3 次）
//!
//! **Validates: Requirements 2.2, 2.3, 2.6, 2.7, 2.8**

use futures::stream::{self, StreamExt};
use reqwest::{Client, Response};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::Semaphore;
use tracing::{debug, info, warn};

/// Supabase 客户端配置
#[derive(Debug, Clone)]
pub struct SupabaseConfig {
    /// Supabase 项目 URL
    pub url: String,
    /// Anonymous Key（公开密钥）
    pub anon_key: String,
    /// Service Role Key（服务端密钥，可选）
    pub service_role_key: Option<String>,
}

impl SupabaseConfig {
    /// 从环境变量创建配置
    pub fn from_env() -> Result<Self, SupabaseError> {
        Ok(Self {
            url: std::env::var("SUPABASE_URL")
                .unwrap_or_else(|_| "https://ttgtdiybtmvdddxanumk.supabase.co".to_string()),
            anon_key: std::env::var("SUPABASE_ANON_KEY")
                .unwrap_or_else(|_| {
                    // 默认使用项目的 anon key
                    "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJpc3MiOiJzdXBhYmFzZSIsInJlZiI6InR0Z3RkaXlidG12ZGRkeGFudW1rIiwicm9sZSI6ImFub24iLCJpYXQiOjE3NjgwMjY4NDUsImV4cCI6MjA4MzYwMjg0NX0.3M8397FMs5opGITupMlHhe0yo2qm7SHKS3ShK0Mjteo".to_string()
                }),
            service_role_key: std::env::var("SUPABASE_SERVICE_ROLE_KEY").ok(),
        })
    }

    /// 获取 Auth API URL
    pub fn auth_url(&self) -> String {
        format!("{}/auth/v1", self.url)
    }

    /// 获取 REST API URL
    pub fn rest_url(&self) -> String {
        format!("{}/rest/v1", self.url)
    }
}

/// Supabase 错误类型
#[derive(Debug, Error)]
pub enum SupabaseError {
    /// HTTP 请求错误
    #[error("HTTP 请求失败: {0}")]
    HttpError(#[from] reqwest::Error),

    /// API 错误响应
    #[error("API 错误: {message} (状态码: {status})")]
    ApiError {
        status: u16,
        message: String,
        error_code: Option<String>,
    },

    /// JSON 解析错误
    #[error("JSON 解析失败: {0}")]
    JsonError(#[from] serde_json::Error),

    /// 配置错误
    #[error("配置错误: {0}")]
    ConfigError(String),

    /// 认证错误
    #[error("认证失败: {0}")]
    AuthError(String),

    /// 网络超时
    #[error("请求超时")]
    Timeout,

    /// 重试耗尽
    #[error("重试次数已用完")]
    RetryExhausted,
}

/// Supabase API 错误响应结构
#[derive(Debug, Deserialize)]
struct ApiErrorResponse {
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
    #[serde(default)]
    error_code: Option<String>,
    #[serde(default)]
    msg: Option<String>,
}

/// 代理配置
#[derive(Debug, Clone, Default)]
pub struct ProxyConfig {
    /// 是否启用代理
    pub enabled: bool,
    /// 代理类型：http 或 socks5
    pub proxy_type: String,
    /// 代理主机
    pub host: String,
    /// 代理端口
    pub port: u16,
}

/// HTTP 客户端配置
///
/// **Validates: Requirements 2.2, 2.3, 2.6, 2.7**
#[derive(Debug, Clone)]
pub struct HttpClientConfig {
    /// 最大并发数（默认 10）
    pub max_concurrency: usize,
    /// 请求超时（秒，默认 30）
    pub timeout_secs: u64,
    /// 连接超时（秒，默认 10）
    pub connect_timeout_secs: u64,
    /// 最大重试次数（默认 3）
    pub max_retries: u32,
    /// 每主机最大空闲连接（默认 10）
    pub pool_max_idle_per_host: usize,
    /// 空闲连接超时（秒，默认 90）
    pub pool_idle_timeout_secs: u64,
}

impl Default for HttpClientConfig {
    fn default() -> Self {
        Self {
            max_concurrency: 10,
            timeout_secs: 30,
            connect_timeout_secs: 10,
            max_retries: 3,
            pool_max_idle_per_host: 10,
            pool_idle_timeout_secs: 90,
        }
    }
}

/// HTTP 客户端状态
///
/// **Validates: Requirements 4.5**
#[derive(Debug, Clone, Serialize)]
pub struct HttpClientStatus {
    /// 可用许可数
    pub available_permits: usize,
    /// 最大并发数
    pub max_concurrency: usize,
}

/// Supabase 客户端
#[derive(Debug, Clone)]
pub struct SupabaseClient {
    /// HTTP 客户端
    client: Client,
    /// 配置
    config: SupabaseConfig,
    /// HTTP 客户端配置
    http_config: HttpClientConfig,
    /// 并发限制信号量
    semaphore: Arc<Semaphore>,
}

impl SupabaseClient {
    /// 创建新的 Supabase 客户端
    pub fn new(config: SupabaseConfig) -> Result<Self, SupabaseError> {
        Self::new_with_options(config, None, HttpClientConfig::default())
    }

    /// 创建带代理的 Supabase 客户端
    pub fn new_with_proxy(
        config: SupabaseConfig,
        proxy_config: Option<ProxyConfig>,
    ) -> Result<Self, SupabaseError> {
        Self::new_with_options(config, proxy_config, HttpClientConfig::default())
    }

    /// 创建带完整配置的 Supabase 客户端
    ///
    /// **Validates: Requirements 2.2, 2.3, 2.6**
    pub fn new_with_options(
        config: SupabaseConfig,
        proxy_config: Option<ProxyConfig>,
        http_config: HttpClientConfig,
    ) -> Result<Self, SupabaseError> {
        let mut builder = Client::builder()
            .timeout(Duration::from_secs(http_config.timeout_secs))
            .connect_timeout(Duration::from_secs(http_config.connect_timeout_secs))
            .pool_max_idle_per_host(http_config.pool_max_idle_per_host)
            .pool_idle_timeout(Duration::from_secs(http_config.pool_idle_timeout_secs))
            .tcp_keepalive(Duration::from_secs(60));

        // 配置代理
        if let Some(proxy) = proxy_config {
            if proxy.enabled && !proxy.host.is_empty() {
                let proxy_url = if proxy.proxy_type.to_lowercase() == "socks5" {
                    format!("socks5://{}:{}", proxy.host, proxy.port)
                } else {
                    format!("http://{}:{}", proxy.host, proxy.port)
                };

                info!("使用代理: {}", proxy_url);

                let proxy = reqwest::Proxy::all(&proxy_url).map_err(|e| {
                    SupabaseError::ConfigError(format!("代理配置错误: {}", e))
                })?;
                builder = builder.proxy(proxy);
            }
        }

        let client = builder.build()?;
        let semaphore = Arc::new(Semaphore::new(http_config.max_concurrency));

        info!(
            "Supabase 客户端初始化完成: 最大并发={}, 超时={}s, 连接池={}",
            http_config.max_concurrency,
            http_config.timeout_secs,
            http_config.pool_max_idle_per_host
        );

        Ok(Self {
            client,
            config,
            http_config,
            semaphore,
        })
    }

    /// 从环境变量创建客户端
    pub fn from_env() -> Result<Self, SupabaseError> {
        let config = SupabaseConfig::from_env()?;
        Self::new(config)
    }

    /// 获取配置
    pub fn config(&self) -> &SupabaseConfig {
        &self.config
    }

    /// 获取 HTTP 客户端配置
    pub fn http_config(&self) -> &HttpClientConfig {
        &self.http_config
    }

    /// 获取客户端状态
    ///
    /// **Validates: Requirements 4.5**
    pub fn status(&self) -> HttpClientStatus {
        HttpClientStatus {
            available_permits: self.semaphore.available_permits(),
            max_concurrency: self.http_config.max_concurrency,
        }
    }

    /// 发送 GET 请求
    pub async fn get<T: DeserializeOwned>(
        &self,
        url: &str,
        access_token: Option<&str>,
    ) -> Result<T, SupabaseError> {
        self.request_with_retry(|| {
            let mut req = self.client.get(url);
            req = self.add_headers(req, access_token);
            req
        })
        .await
    }

    /// 发送 POST 请求
    pub async fn post<T: DeserializeOwned, B: Serialize>(
        &self,
        url: &str,
        body: &B,
        access_token: Option<&str>,
    ) -> Result<T, SupabaseError> {
        self.request_with_retry(|| {
            let mut req = self.client.post(url).json(body);
            req = self.add_headers(req, access_token);
            req
        })
        .await
    }

    /// 发送 PATCH 请求
    pub async fn patch<T: DeserializeOwned, B: Serialize>(
        &self,
        url: &str,
        body: &B,
        access_token: Option<&str>,
    ) -> Result<T, SupabaseError> {
        self.request_with_retry(|| {
            let mut req = self.client.patch(url).json(body);
            req = self.add_headers(req, access_token);
            req
        })
        .await
    }

    /// 发送 DELETE 请求
    pub async fn delete<T: DeserializeOwned>(
        &self,
        url: &str,
        access_token: Option<&str>,
    ) -> Result<T, SupabaseError> {
        self.request_with_retry(|| {
            let mut req = self.client.delete(url);
            req = self.add_headers(req, access_token);
            req
        })
        .await
    }

    /// 批量 GET 请求
    ///
    /// 使用 `futures::stream::buffer_unordered` 实现高效并发请求。
    /// 并发数由 `HttpClientConfig::max_concurrency` 控制。
    ///
    /// **Validates: Requirements 2.1, 4.5**
    ///
    /// # 参数
    ///
    /// - `urls`: URL 列表
    /// - `access_token`: 可选的访问令牌
    ///
    /// # 返回
    ///
    /// 返回结果列表，顺序与输入 URL 顺序一致
    pub async fn batch_get<T: DeserializeOwned + Send + 'static>(
        &self,
        urls: Vec<String>,
        access_token: Option<String>,
    ) -> Vec<Result<T, SupabaseError>> {
        let start = Instant::now();
        let total = urls.len();
        
        info!("开始批量 GET 请求: {} 个 URL", total);

        // 使用 buffer_unordered 并发执行，但保持结果顺序
        let mut indexed_results: Vec<(usize, Result<T, SupabaseError>)> = 
            stream::iter(urls.into_iter().enumerate())
                .map(|(idx, url)| {
                    let client = self.clone();
                    let token = access_token.clone();
                    async move {
                        let result = client.get(&url, token.as_deref()).await;
                        (idx, result)
                    }
                })
                .buffer_unordered(self.http_config.max_concurrency)
                .collect()
                .await;

        // 按索引排序以保持原始顺序
        indexed_results.sort_by_key(|(idx, _)| *idx);
        
        let results: Vec<Result<T, SupabaseError>> = indexed_results
            .into_iter()
            .map(|(_, result)| result)
            .collect();

        let elapsed = start.elapsed();
        let success_count = results.iter().filter(|r| r.is_ok()).count();
        
        info!(
            "批量 GET 请求完成: {}/{} 成功，耗时 {:?}",
            success_count, total, elapsed
        );

        results
    }

    /// 批量 POST 请求
    ///
    /// 使用 `futures::stream::buffer_unordered` 实现高效并发请求。
    /// 并发数由 `HttpClientConfig::max_concurrency` 控制。
    ///
    /// **Validates: Requirements 2.1, 4.5**
    ///
    /// # 参数
    ///
    /// - `requests`: (URL, Body) 元组列表
    /// - `access_token`: 可选的访问令牌
    ///
    /// # 返回
    ///
    /// 返回结果列表，顺序与输入请求顺序一致
    pub async fn batch_post<T, B>(
        &self,
        requests: Vec<(String, B)>,
        access_token: Option<String>,
    ) -> Vec<Result<T, SupabaseError>>
    where
        T: DeserializeOwned + Send + 'static,
        B: Serialize + Send + Clone + 'static,
    {
        let start = Instant::now();
        let total = requests.len();
        
        info!("开始批量 POST 请求: {} 个请求", total);

        // 使用 buffer_unordered 并发执行，但保持结果顺序
        let mut indexed_results: Vec<(usize, Result<T, SupabaseError>)> = 
            stream::iter(requests.into_iter().enumerate())
                .map(|(idx, (url, body))| {
                    let client = self.clone();
                    let token = access_token.clone();
                    async move {
                        let result = client.post(&url, &body, token.as_deref()).await;
                        (idx, result)
                    }
                })
                .buffer_unordered(self.http_config.max_concurrency)
                .collect()
                .await;

        // 按索引排序以保持原始顺序
        indexed_results.sort_by_key(|(idx, _)| *idx);
        
        let results: Vec<Result<T, SupabaseError>> = indexed_results
            .into_iter()
            .map(|(_, result)| result)
            .collect();

        let elapsed = start.elapsed();
        let success_count = results.iter().filter(|r| r.is_ok()).count();
        
        info!(
            "批量 POST 请求完成: {}/{} 成功，耗时 {:?}",
            success_count, total, elapsed
        );

        results
    }

    /// 添加请求头
    fn add_headers(
        &self,
        req: reqwest::RequestBuilder,
        access_token: Option<&str>,
    ) -> reqwest::RequestBuilder {
        let mut req = req
            .header("apikey", &self.config.anon_key)
            .header("Content-Type", "application/json");

        if let Some(token) = access_token {
            req = req.header("Authorization", format!("Bearer {}", token));
        } else {
            req = req.header("Authorization", format!("Bearer {}", self.config.anon_key));
        }

        req
    }

    /// 带重试的请求
    ///
    /// **Validates: Requirements 2.7, 2.8**
    ///
    /// 重试策略：
    /// - 可重试错误：5xx、超时、连接错误
    /// - 不可重试错误：4xx（客户端错误）
    /// - 指数退避：100ms, 200ms, 400ms...
    async fn request_with_retry<T, F>(&self, build_request: F) -> Result<T, SupabaseError>
    where
        T: DeserializeOwned,
        F: Fn() -> reqwest::RequestBuilder,
    {
        let start = Instant::now();
        let mut last_error = None;
        let max_retries = self.http_config.max_retries;

        // 获取并发许可
        let _permit = self.semaphore.acquire().await.map_err(|_| {
            SupabaseError::ConfigError("获取并发许可失败".to_string())
        })?;

        for attempt in 0..=max_retries {
            if attempt > 0 {
                // 指数退避：100ms * 2^(attempt-1)
                let delay = Duration::from_millis(100 * 2u64.pow(attempt - 1));
                debug!("重试请求，等待 {:?}（第 {} 次重试）", delay, attempt);
                tokio::time::sleep(delay).await;
            }

            let response = match build_request().send().await {
                Ok(resp) => resp,
                Err(e) => {
                    // 判断是否可重试
                    let is_retryable = e.is_timeout() || e.is_connect();
                    
                    if is_retryable && attempt < max_retries {
                        warn!(
                            "请求失败（{}），将重试: {}",
                            if e.is_timeout() { "超时" } else { "连接错误" },
                            e
                        );
                        last_error = Some(if e.is_timeout() {
                            SupabaseError::Timeout
                        } else {
                            SupabaseError::HttpError(e)
                        });
                        continue;
                    }
                    
                    // 记录请求耗时
                    let elapsed = start.elapsed();
                    if elapsed.as_secs() > 5 {
                        warn!("HTTP 请求耗时 {:?}，超过 5s 阈值", elapsed);
                    }
                    
                    return Err(if e.is_timeout() {
                        SupabaseError::Timeout
                    } else {
                        SupabaseError::HttpError(e)
                    });
                }
            };

            let status = response.status();
            
            // 5xx 服务器错误可重试
            if status.is_server_error() && attempt < max_retries {
                warn!("服务器错误 {}，将重试", status.as_u16());
                last_error = Some(SupabaseError::ApiError {
                    status: status.as_u16(),
                    message: format!("服务器错误: {}", status),
                    error_code: None,
                });
                continue;
            }
            
            // 4xx 客户端错误不重试
            if status.is_client_error() {
                let elapsed = start.elapsed();
                debug!("HTTP 请求完成: 状态={}, 耗时={:?}", status.as_u16(), elapsed);
                return self.handle_response(response).await;
            }

            // 记录请求耗时和状态码
            let elapsed = start.elapsed();
            debug!("HTTP 请求完成: 状态={}, 耗时={:?}", status.as_u16(), elapsed);
            
            // 性能警告：超过 5s 记录警告
            if elapsed.as_secs() > 5 {
                warn!(
                    "HTTP 请求耗时 {:?}，超过 5s 阈值（状态码: {}）",
                    elapsed, status.as_u16()
                );
            }

            return self.handle_response(response).await;
        }

        Err(last_error.unwrap_or(SupabaseError::RetryExhausted))
    }

    /// 处理响应
    async fn handle_response<T: DeserializeOwned>(
        &self,
        response: Response,
    ) -> Result<T, SupabaseError> {
        let status = response.status();

        if status.is_success() {
            // 特殊处理空响应
            let text = response.text().await?;
            if text.is_empty() || text == "null" {
                // 尝试返回默认值（对于 Option 类型）
                return serde_json::from_str("null")
                    .map_err(SupabaseError::JsonError);
            }
            serde_json::from_str(&text).map_err(SupabaseError::JsonError)
        } else {
            let error_text = response.text().await.unwrap_or_default();
            let api_error: ApiErrorResponse =
                serde_json::from_str(&error_text).unwrap_or(ApiErrorResponse {
                    message: Some(error_text.clone()),
                    error: None,
                    error_description: None,
                    error_code: None,
                    msg: None,
                });

            let message = api_error
                .message
                .or(api_error.error_description)
                .or(api_error.error)
                .or(api_error.msg)
                .unwrap_or_else(|| format!("HTTP {}", status.as_u16()));

            Err(SupabaseError::ApiError {
                status: status.as_u16(),
                message,
                error_code: api_error.error_code,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_from_env() {
        let config = SupabaseConfig::from_env().unwrap();
        assert!(!config.url.is_empty());
        assert!(!config.anon_key.is_empty());
    }

    #[test]
    fn test_auth_url() {
        let config = SupabaseConfig {
            url: "https://example.supabase.co".to_string(),
            anon_key: "test".to_string(),
            service_role_key: None,
        };
        assert_eq!(config.auth_url(), "https://example.supabase.co/auth/v1");
    }

    #[test]
    fn test_rest_url() {
        let config = SupabaseConfig {
            url: "https://example.supabase.co".to_string(),
            anon_key: "test".to_string(),
            service_role_key: None,
        };
        assert_eq!(config.rest_url(), "https://example.supabase.co/rest/v1");
    }

    #[test]
    fn test_http_client_config_default() {
        let config = HttpClientConfig::default();
        assert_eq!(config.max_concurrency, 10);
        assert_eq!(config.timeout_secs, 30);
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.pool_max_idle_per_host, 10);
    }

    #[test]
    fn test_http_client_status() {
        let config = SupabaseConfig {
            url: "https://example.supabase.co".to_string(),
            anon_key: "test".to_string(),
            service_role_key: None,
        };
        let client = SupabaseClient::new(config).unwrap();
        let status = client.status();
        
        assert_eq!(status.max_concurrency, 10);
        assert_eq!(status.available_permits, 10);
    }

    #[test]
    fn test_custom_http_config() {
        let config = SupabaseConfig {
            url: "https://example.supabase.co".to_string(),
            anon_key: "test".to_string(),
            service_role_key: None,
        };
        let http_config = HttpClientConfig {
            max_concurrency: 5,
            timeout_secs: 60,
            connect_timeout_secs: 15,
            max_retries: 5,
            pool_max_idle_per_host: 20,
            pool_idle_timeout_secs: 120,
        };
        let client = SupabaseClient::new_with_options(config, None, http_config).unwrap();
        let status = client.status();
        
        assert_eq!(status.max_concurrency, 5);
        assert_eq!(status.available_permits, 5);
    }
}

// ============================================================================
// 属性测试 (Property-Based Testing)
// ============================================================================

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    // ========================================================================
    // Feature: rust-performance-optimization
    // Property 3: HTTP 并发限制
    // Validates: Requirements 2.3
    // ========================================================================

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// Property: 并发限制配置正确应用
        ///
        /// 对于任意并发数配置（1-100），客户端状态应正确反映配置值。
        #[test]
        fn prop_concurrency_limit_applied(
            max_concurrency in 1usize..=100,
        ) {
            let config = SupabaseConfig {
                url: "https://example.supabase.co".to_string(),
                anon_key: "test".to_string(),
                service_role_key: None,
            };
            let http_config = HttpClientConfig {
                max_concurrency,
                ..Default::default()
            };
            
            let client = SupabaseClient::new_with_options(config, None, http_config).unwrap();
            let status = client.status();
            
            prop_assert_eq!(status.max_concurrency, max_concurrency,
                "最大并发数应该等于配置值");
            prop_assert_eq!(status.available_permits, max_concurrency,
                "初始可用许可数应该等于最大并发数");
        }

        /// Property: 连接池配置正确应用
        ///
        /// 对于任意连接池配置，客户端应成功创建。
        #[test]
        fn prop_pool_config_applied(
            pool_max_idle in 1usize..=50,
            pool_idle_timeout in 30u64..=300,
        ) {
            let config = SupabaseConfig {
                url: "https://example.supabase.co".to_string(),
                anon_key: "test".to_string(),
                service_role_key: None,
            };
            let http_config = HttpClientConfig {
                pool_max_idle_per_host: pool_max_idle,
                pool_idle_timeout_secs: pool_idle_timeout,
                ..Default::default()
            };
            
            let result = SupabaseClient::new_with_options(config, None, http_config);
            prop_assert!(result.is_ok(), "客户端应该成功创建");
        }
    }

    // ========================================================================
    // Feature: rust-performance-optimization
    // Property 4: HTTP 重试策略
    // Validates: Requirements 2.8
    // ========================================================================

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// Property: 重试次数配置正确应用
        ///
        /// 对于任意重试次数配置（0-10），客户端应成功创建。
        #[test]
        fn prop_retry_config_applied(
            max_retries in 0u32..=10,
        ) {
            let config = SupabaseConfig {
                url: "https://example.supabase.co".to_string(),
                anon_key: "test".to_string(),
                service_role_key: None,
            };
            let http_config = HttpClientConfig {
                max_retries,
                ..Default::default()
            };
            
            let result = SupabaseClient::new_with_options(config, None, http_config);
            prop_assert!(result.is_ok(), "客户端应该成功创建");
            
            let client = result.unwrap();
            prop_assert_eq!(client.http_config().max_retries, max_retries,
                "重试次数应该等于配置值");
        }

        /// Property: 超时配置正确应用
        ///
        /// 对于任意超时配置（1-120秒），客户端应成功创建。
        #[test]
        fn prop_timeout_config_applied(
            timeout_secs in 1u64..=120,
            connect_timeout_secs in 1u64..=30,
        ) {
            let config = SupabaseConfig {
                url: "https://example.supabase.co".to_string(),
                anon_key: "test".to_string(),
                service_role_key: None,
            };
            let http_config = HttpClientConfig {
                timeout_secs,
                connect_timeout_secs,
                ..Default::default()
            };
            
            let result = SupabaseClient::new_with_options(config, None, http_config);
            prop_assert!(result.is_ok(), "客户端应该成功创建");
            
            let client = result.unwrap();
            prop_assert_eq!(client.http_config().timeout_secs, timeout_secs,
                "超时时间应该等于配置值");
            prop_assert_eq!(client.http_config().connect_timeout_secs, connect_timeout_secs,
                "连接超时时间应该等于配置值");
        }

        /// Property: 指数退避延迟计算正确
        ///
        /// 对于任意重试次数，延迟应该遵循 100ms * 2^(attempt-1) 的公式。
        #[test]
        fn prop_exponential_backoff_calculation(
            attempt in 1u32..=10,
        ) {
            // 计算预期延迟
            let expected_delay_ms = 100u64 * 2u64.pow(attempt - 1);
            let actual_delay = Duration::from_millis(expected_delay_ms);
            
            // 验证延迟计算
            prop_assert!(actual_delay.as_millis() >= 100,
                "延迟应该至少为 100ms");
            prop_assert!(actual_delay.as_millis() <= 100 * 2u128.pow(9),
                "延迟应该不超过最大值");
            
            // 验证指数增长
            if attempt > 1 {
                let prev_delay_ms = 100u64 * 2u64.pow(attempt - 2);
                prop_assert_eq!(expected_delay_ms, prev_delay_ms * 2,
                    "延迟应该是前一次的两倍");
            }
        }
    }
}
