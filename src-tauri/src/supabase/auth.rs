//! Supabase 认证服务
//!
//! 封装 Supabase Auth API，提供：
//! - 邮箱+密码登录/注册
//! - 密码重置
//! - 会话管理
//! - Token 刷新

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs;
use tracing::{debug, info, warn};

use super::client::{SupabaseClient, SupabaseError};

/// 认证用户信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthUser {
    /// 用户 ID
    pub id: String,
    /// 邮箱
    pub email: Option<String>,
    /// 创建时间
    pub created_at: Option<String>,
    /// 最后登录时间
    pub last_sign_in_at: Option<String>,
    /// 用户元数据
    #[serde(default)]
    pub user_metadata: serde_json::Value,
}

/// 认证会话
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthSession {
    /// 访问令牌
    pub access_token: String,
    /// 刷新令牌
    pub refresh_token: String,
    /// 令牌类型
    pub token_type: String,
    /// 过期时间（秒）
    pub expires_in: i64,
    /// 过期时间戳（Unix 时间戳）
    #[serde(default)]
    pub expires_at: Option<i64>,
    /// 用户信息
    pub user: AuthUser,
}

/// 登录请求
#[derive(Debug, Serialize)]
struct SignInRequest {
    email: String,
    password: String,
}

/// 注册请求
#[derive(Debug, Serialize)]
struct SignUpRequest {
    email: String,
    password: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
}

/// 刷新令牌请求
#[derive(Debug, Serialize)]
struct RefreshTokenRequest {
    refresh_token: String,
}

/// 密码重置请求
#[derive(Debug, Serialize)]
struct ResetPasswordRequest {
    email: String,
}

/// 认证服务
#[derive(Debug)]
pub struct AuthService {
    /// Supabase 客户端
    client: SupabaseClient,
    /// 当前会话
    session: Option<AuthSession>,
    /// 会话文件路径
    session_path: PathBuf,
}

impl AuthService {
    /// 创建认证服务
    pub fn new(client: SupabaseClient, data_dir: PathBuf) -> Self {
        let session_path = data_dir.join("session.json");
        Self {
            client,
            session: None,
            session_path,
        }
    }

    /// 获取当前会话
    pub fn current_session(&self) -> Option<&AuthSession> {
        self.session.as_ref()
    }

    /// 获取当前用户
    pub fn current_user(&self) -> Option<&AuthUser> {
        self.session.as_ref().map(|s| &s.user)
    }

    /// 获取当前用户 ID
    pub fn current_user_id(&self) -> Option<&str> {
        self.session.as_ref().map(|s| s.user.id.as_str())
    }

    /// 获取访问令牌
    pub fn access_token(&self) -> Option<&str> {
        self.session.as_ref().map(|s| s.access_token.as_str())
    }

    /// 检查是否已登录
    pub fn is_authenticated(&self) -> bool {
        self.session.is_some()
    }

    /// 邮箱+密码登录
    pub async fn sign_in_with_password(
        &mut self,
        email: &str,
        password: &str,
    ) -> Result<AuthSession, SupabaseError> {
        info!("正在登录: {}", email);

        let url = format!("{}/token?grant_type=password", self.client.config().auth_url());
        let request = SignInRequest {
            email: email.to_string(),
            password: password.to_string(),
        };

        let session: AuthSession = self.client.post(&url, &request, None).await?;

        // 计算过期时间戳
        let session = self.add_expires_at(session);

        // 保存会话
        self.session = Some(session.clone());
        self.save_session().await?;

        info!("登录成功: user_id={}", session.user.id);
        Ok(session)
    }

    /// 邮箱+密码注册
    pub async fn sign_up(
        &mut self,
        email: &str,
        password: &str,
        user_data: Option<serde_json::Value>,
    ) -> Result<AuthSession, SupabaseError> {
        info!("正在注册: {}", email);

        let url = format!("{}/signup", self.client.config().auth_url());
        let request = SignUpRequest {
            email: email.to_string(),
            password: password.to_string(),
            data: user_data,
        };

        let session: AuthSession = self.client.post(&url, &request, None).await?;
        let session = self.add_expires_at(session);

        // 保存会话
        self.session = Some(session.clone());
        self.save_session().await?;

        info!("注册成功: user_id={}", session.user.id);
        Ok(session)
    }

    /// 发送密码重置邮件
    pub async fn reset_password(&self, email: &str) -> Result<(), SupabaseError> {
        info!("发送密码重置邮件: {}", email);

        let url = format!("{}/recover", self.client.config().auth_url());
        let request = ResetPasswordRequest {
            email: email.to_string(),
        };

        let _: serde_json::Value = self.client.post(&url, &request, None).await?;

        info!("密码重置邮件已发送");
        Ok(())
    }

    /// 刷新访问令牌
    pub async fn refresh_session(&mut self) -> Result<AuthSession, SupabaseError> {
        let refresh_token = self
            .session
            .as_ref()
            .map(|s| s.refresh_token.clone())
            .ok_or_else(|| SupabaseError::AuthError("未登录".to_string()))?;

        debug!("正在刷新令牌");

        let url = format!("{}/token?grant_type=refresh_token", self.client.config().auth_url());
        let request = RefreshTokenRequest { refresh_token };

        let session: AuthSession = self.client.post(&url, &request, None).await?;
        let session = self.add_expires_at(session);

        // 保存新会话
        self.session = Some(session.clone());
        self.save_session().await?;

        debug!("令牌刷新成功");
        Ok(session)
    }

    /// 登出
    pub async fn sign_out(&mut self) -> Result<(), SupabaseError> {
        if let Some(session) = &self.session {
            let url = format!("{}/logout", self.client.config().auth_url());

            // 尝试通知服务器登出（忽略错误）
            let _: Result<serde_json::Value, _> = self
                .client
                .post(&url, &serde_json::json!({}), Some(&session.access_token))
                .await;
        }

        // 清除本地会话
        self.session = None;
        self.delete_session().await?;

        info!("已登出");
        Ok(())
    }

    /// 从本地加载会话
    pub async fn load_session(&mut self) -> Result<Option<AuthSession>, SupabaseError> {
        if !self.session_path.exists() {
            debug!("会话文件不存在");
            return Ok(None);
        }

        let content = fs::read_to_string(&self.session_path)
            .await
            .map_err(|e| SupabaseError::ConfigError(format!("读取会话文件失败: {}", e)))?;

        let session: AuthSession = serde_json::from_str(&content)?;

        // 检查是否过期
        if let Some(expires_at) = session.expires_at {
            let now = chrono::Utc::now().timestamp();
            if now >= expires_at - 60 {
                // 提前 60 秒认为过期
                info!("会话已过期，尝试刷新");
                self.session = Some(session);
                return match self.refresh_session().await {
                    Ok(new_session) => Ok(Some(new_session)),
                    Err(e) => {
                        warn!("刷新令牌失败: {}", e);
                        self.session = None;
                        self.delete_session().await?;
                        Ok(None)
                    }
                };
            }
        }

        self.session = Some(session.clone());
        info!("从本地加载会话成功: user_id={}", session.user.id);
        Ok(Some(session))
    }

    /// 保存会话到本地
    async fn save_session(&self) -> Result<(), SupabaseError> {
        if let Some(session) = &self.session {
            // 确保目录存在
            if let Some(parent) = self.session_path.parent() {
                fs::create_dir_all(parent)
                    .await
                    .map_err(|e| SupabaseError::ConfigError(format!("创建目录失败: {}", e)))?;
            }

            let content = serde_json::to_string_pretty(session)?;
            fs::write(&self.session_path, content)
                .await
                .map_err(|e| SupabaseError::ConfigError(format!("保存会话失败: {}", e)))?;

            debug!("会话已保存到: {:?}", self.session_path);
        }
        Ok(())
    }

    /// 删除本地会话
    async fn delete_session(&self) -> Result<(), SupabaseError> {
        if self.session_path.exists() {
            fs::remove_file(&self.session_path)
                .await
                .map_err(|e| SupabaseError::ConfigError(format!("删除会话文件失败: {}", e)))?;
        }
        Ok(())
    }

    /// 添加过期时间戳
    fn add_expires_at(&self, mut session: AuthSession) -> AuthSession {
        if session.expires_at.is_none() {
            session.expires_at = Some(chrono::Utc::now().timestamp() + session.expires_in);
        }
        session
    }

    /// 检查令牌是否即将过期（5 分钟内）
    pub fn is_token_expiring_soon(&self) -> bool {
        if let Some(session) = &self.session {
            if let Some(expires_at) = session.expires_at {
                let now = chrono::Utc::now().timestamp();
                return now >= expires_at - 300; // 5 分钟
            }
        }
        false
    }

    /// 确保令牌有效（自动刷新）
    pub async fn ensure_valid_token(&mut self) -> Result<&str, SupabaseError> {
        if self.session.is_none() {
            return Err(SupabaseError::AuthError("未登录".to_string()));
        }

        if self.is_token_expiring_soon() {
            self.refresh_session().await?;
        }

        self.access_token()
            .ok_or_else(|| SupabaseError::AuthError("获取令牌失败".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn create_test_client() -> SupabaseClient {
        let config = super::super::client::SupabaseConfig::from_env().unwrap();
        SupabaseClient::new(config).unwrap()
    }

    #[test]
    fn test_auth_service_creation() {
        let client = create_test_client();
        let temp_dir = tempdir().unwrap();
        let service = AuthService::new(client, temp_dir.path().to_path_buf());
        assert!(!service.is_authenticated());
    }

    #[test]
    fn test_session_path() {
        let client = create_test_client();
        let temp_dir = tempdir().unwrap();
        let service = AuthService::new(client, temp_dir.path().to_path_buf());
        assert!(service.session_path.ends_with("session.json"));
    }
}
