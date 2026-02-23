//! 认证相关的 Tauri 命令
//!
//! 提供用户认证功能给前端调用

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};
use tokio::sync::Mutex;
use tracing::{error, info};

use crate::database::settings::AdvancedConfig;
use crate::supabase::{AuthService, AuthUser, ProxyConfig, SupabaseClient, SupabaseConfig, SupabaseError};

/// 认证状态（全局共享）
pub struct AuthState {
    pub service: Arc<Mutex<AuthService>>,
}

impl AuthState {
    /// 创建认证状态
    pub fn new(data_dir: PathBuf) -> Result<Self, String> {
        Self::new_with_proxy(data_dir, None)
    }

    /// 创建带代理配置的认证状态
    pub fn new_with_proxy(data_dir: PathBuf, advanced_config: Option<&AdvancedConfig>) -> Result<Self, String> {
        let supabase_config = SupabaseConfig::from_env()
            .map_err(|e| format!("创建 Supabase 配置失败: {}", e))?;

        // 转换代理配置
        let proxy_config = advanced_config.map(|cfg| ProxyConfig {
            enabled: cfg.proxy_enabled,
            proxy_type: cfg.proxy_type.clone(),
            host: cfg.proxy_host.clone(),
            port: cfg.proxy_port,
        });

        let client = SupabaseClient::new_with_proxy(supabase_config, proxy_config)
            .map_err(|e| format!("创建 Supabase 客户端失败: {}", e))?;

        let service = AuthService::new(client, data_dir);
        Ok(Self {
            service: Arc::new(Mutex::new(service)),
        })
    }
}

/// 认证响应
#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<AuthUserInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 用户信息（简化版）
#[derive(Debug, Serialize)]
pub struct AuthUserInfo {
    pub id: String,
    pub email: Option<String>,
}

impl From<&AuthUser> for AuthUserInfo {
    fn from(user: &AuthUser) -> Self {
        Self {
            id: user.id.clone(),
            email: user.email.clone(),
        }
    }
}

/// 初始化认证状态
pub fn init_auth_state(app: &AppHandle) -> Result<AuthState, String> {
    use crate::database::settings::{get_config_path, load_config};

    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取数据目录失败: {}", e))?;

    // 尝试加载配置以获取代理设置
    let advanced_config = get_config_path(app)
        .ok()
        .and_then(|path| load_config(&path).ok())
        .map(|cfg| cfg.advanced);

    AuthState::new_with_proxy(data_dir, advanced_config.as_ref())
}

/// 获取当前用户信息
#[tauri::command]
pub async fn get_current_user(
    state: State<'_, AuthState>,
) -> Result<AuthResponse, String> {
    let service = state.service.lock().await;

    if let Some(user) = service.current_user() {
        Ok(AuthResponse {
            success: true,
            user: Some(AuthUserInfo::from(user)),
            error: None,
        })
    } else {
        Ok(AuthResponse {
            success: false,
            user: None,
            error: Some("未登录".to_string()),
        })
    }
}

/// 检查是否已登录
#[tauri::command]
pub async fn is_authenticated(
    state: State<'_, AuthState>,
) -> Result<bool, String> {
    let service = state.service.lock().await;
    Ok(service.is_authenticated())
}

/// 加载保存的会话
#[tauri::command]
pub async fn load_saved_session(
    state: State<'_, AuthState>,
) -> Result<AuthResponse, String> {
    let mut service = state.service.lock().await;

    match service.load_session().await {
        Ok(Some(session)) => {
            info!("已加载保存的会话: user_id={}", session.user.id);
            Ok(AuthResponse {
                success: true,
                user: Some(AuthUserInfo::from(&session.user)),
                error: None,
            })
        }
        Ok(None) => {
            Ok(AuthResponse {
                success: false,
                user: None,
                error: None,
            })
        }
        Err(e) => {
            error!("加载会话失败: {}", e);
            Ok(AuthResponse {
                success: false,
                user: None,
                error: Some(e.to_string()),
            })
        }
    }
}

/// 登录请求参数
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

/// 邮箱+密码登录
#[tauri::command]
pub async fn sign_in_with_password(
    request: LoginRequest,
    state: State<'_, AuthState>,
) -> Result<AuthResponse, String> {
    let mut service = state.service.lock().await;

    match service.sign_in_with_password(&request.email, &request.password).await {
        Ok(session) => {
            info!("登录成功: user_id={}", session.user.id);
            Ok(AuthResponse {
                success: true,
                user: Some(AuthUserInfo::from(&session.user)),
                error: None,
            })
        }
        Err(e) => {
            error!("登录失败: {}", e);
            Ok(AuthResponse {
                success: false,
                user: None,
                error: Some(format_auth_error(&e)),
            })
        }
    }
}

/// 注册请求参数
#[derive(Debug, Deserialize)]
pub struct SignUpRequest {
    pub email: String,
    pub password: String,
}

/// 邮箱+密码注册
#[tauri::command]
pub async fn sign_up(
    request: SignUpRequest,
    state: State<'_, AuthState>,
) -> Result<AuthResponse, String> {
    let mut service = state.service.lock().await;

    match service.sign_up(&request.email, &request.password, None).await {
        Ok(session) => {
            info!("注册成功: user_id={}", session.user.id);
            Ok(AuthResponse {
                success: true,
                user: Some(AuthUserInfo::from(&session.user)),
                error: None,
            })
        }
        Err(e) => {
            error!("注册失败: {}", e);
            Ok(AuthResponse {
                success: false,
                user: None,
                error: Some(format_auth_error(&e)),
            })
        }
    }
}

/// 发送密码重置邮件
#[tauri::command]
pub async fn reset_password(
    email: String,
    state: State<'_, AuthState>,
) -> Result<AuthResponse, String> {
    let service = state.service.lock().await;

    match service.reset_password(&email).await {
        Ok(_) => {
            info!("密码重置邮件已发送: {}", email);
            Ok(AuthResponse {
                success: true,
                user: None,
                error: None,
            })
        }
        Err(e) => {
            error!("发送密码重置邮件失败: {}", e);
            Ok(AuthResponse {
                success: false,
                user: None,
                error: Some(format_auth_error(&e)),
            })
        }
    }
}

/// 登出
#[tauri::command]
pub async fn sign_out(
    state: State<'_, AuthState>,
) -> Result<AuthResponse, String> {
    let mut service = state.service.lock().await;

    match service.sign_out().await {
        Ok(_) => {
            info!("已登出");
            Ok(AuthResponse {
                success: true,
                user: None,
                error: None,
            })
        }
        Err(e) => {
            error!("登出失败: {}", e);
            Ok(AuthResponse {
                success: false,
                user: None,
                error: Some(e.to_string()),
            })
        }
    }
}

/// 刷新会话
#[tauri::command]
pub async fn refresh_session(
    state: State<'_, AuthState>,
) -> Result<AuthResponse, String> {
    let mut service = state.service.lock().await;

    match service.refresh_session().await {
        Ok(session) => {
            Ok(AuthResponse {
                success: true,
                user: Some(AuthUserInfo::from(&session.user)),
                error: None,
            })
        }
        Err(e) => {
            error!("刷新会话失败: {}", e);
            Ok(AuthResponse {
                success: false,
                user: None,
                error: Some(format_auth_error(&e)),
            })
        }
    }
}

/// 格式化认证错误信息
fn format_auth_error(error: &SupabaseError) -> String {
    match error {
        SupabaseError::ApiError { message, error_code, .. } => {
            // 转换常见错误为友好消息
            if let Some(code) = error_code {
                match code.as_str() {
                    "invalid_credentials" => return "邮箱或密码错误".to_string(),
                    "email_not_confirmed" => return "请先验证邮箱".to_string(),
                    "user_already_exists" => return "该邮箱已注册".to_string(),
                    "weak_password" => return "密码强度不足".to_string(),
                    "invalid_email" => return "邮箱格式不正确".to_string(),
                    _ => {}
                }
            }

            // 检查消息内容
            let msg = message.to_lowercase();
            if msg.contains("invalid login credentials") {
                return "邮箱或密码错误".to_string();
            }
            if msg.contains("email not confirmed") {
                return "请先验证邮箱".to_string();
            }
            if msg.contains("user already registered") {
                return "该邮箱已注册".to_string();
            }

            message.clone()
        }
        SupabaseError::Timeout => "网络超时，请重试".to_string(),
        SupabaseError::HttpError(_) => "网络连接失败".to_string(),
        _ => error.to_string(),
    }
}
