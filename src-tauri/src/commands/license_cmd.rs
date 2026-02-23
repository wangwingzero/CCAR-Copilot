//! 许可证相关的 Tauri 命令

use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};
use tokio::sync::Mutex;
use tracing::info;

use crate::license::{LicenseInfo, LicenseService};
use crate::supabase::SupabaseClient;
use crate::commands::auth_cmd::AuthState;

/// 许可证状态（全局共享）
pub struct LicenseState {
    pub service: Arc<Mutex<LicenseService>>,
}

impl LicenseState {
    /// 创建许可证状态
    pub fn new(data_dir: PathBuf) -> Result<Self, String> {
        let client = SupabaseClient::from_env()
            .map_err(|e| format!("创建 Supabase 客户端失败: {}", e))?;
        let service = LicenseService::new(client, data_dir);
        Ok(Self {
            service: Arc::new(Mutex::new(service)),
        })
    }
}

/// 初始化许可证状态
pub fn init_license_state(app: &AppHandle) -> Result<LicenseState, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取数据目录失败: {}", e))?;
    LicenseState::new(data_dir)
}

/// 许可证响应
#[derive(Debug, Serialize)]
pub struct LicenseResponse {
    pub plan: String,
    pub is_vip: bool,
    pub is_valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grace_period_end: Option<String>,
}

impl From<&LicenseInfo> for LicenseResponse {
    fn from(info: &LicenseInfo) -> Self {
        Self {
            plan: info.plan.to_string(),
            is_vip: info.is_vip(),
            is_valid: info.is_valid,
            grace_period_end: info.grace_period_end.map(|t| t.to_rfc3339()),
        }
    }
}

/// 验证许可证
#[tauri::command]
pub async fn validate_license(
    auth_state: State<'_, AuthState>,
    license_state: State<'_, LicenseState>,
) -> Result<LicenseResponse, String> {
    // 获取当前用户信息
    let auth_service = auth_state.service.lock().await;

    let (user_id, access_token) = match (auth_service.current_user_id(), auth_service.access_token())
    {
        (Some(uid), Some(token)) => (uid.to_string(), token.to_string()),
        _ => {
            // 未登录用户返回免费许可证
            return Ok(LicenseResponse::from(&LicenseInfo::free()));
        }
    };
    drop(auth_service);

    // 验证许可证
    let mut license_service = license_state.service.lock().await;
    let license = license_service.validate(&user_id, &access_token).await;

    info!("许可证验证完成: plan={}, is_vip={}", license.plan, license.is_vip());
    Ok(LicenseResponse::from(&license))
}

/// 获取缓存的许可证（不触发网络请求）
#[tauri::command]
pub async fn get_cached_license(
    license_state: State<'_, LicenseState>,
) -> Result<Option<LicenseResponse>, String> {
    let service = license_state.service.lock().await;

    Ok(service.cached_license().map(LicenseResponse::from))
}

/// 清除许可证缓存
#[tauri::command]
pub async fn clear_license_cache(
    license_state: State<'_, LicenseState>,
) -> Result<(), String> {
    let mut service = license_state.service.lock().await;
    service.clear_cache().await;
    info!("许可证缓存已清除");
    Ok(())
}

/// 检查是否是 VIP 用户
#[tauri::command]
pub async fn is_vip_user(
    auth_state: State<'_, AuthState>,
    license_state: State<'_, LicenseState>,
) -> Result<bool, String> {
    let auth_service = auth_state.service.lock().await;

    let (user_id, access_token) = match (auth_service.current_user_id(), auth_service.access_token())
    {
        (Some(uid), Some(token)) => (uid.to_string(), token.to_string()),
        _ => return Ok(false),
    };
    drop(auth_service);

    let mut license_service = license_state.service.lock().await;
    let license = license_service.validate(&user_id, &access_token).await;

    Ok(license.is_vip())
}
