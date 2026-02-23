//! 使用量追踪相关的 Tauri 命令

use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};
use tracing::info;

use crate::usage::{UsageTracker, UsageStats, UsageFeature};
use crate::supabase::SupabaseClient;
use crate::commands::auth_cmd::AuthState;

/// 使用量状态（全局共享）
///
/// 注意：`UsageTracker` 内部已有 `Mutex<Connection>` 管理数据库访问，
/// 所以外层直接使用 `Arc<UsageTracker>` 即可。
pub struct UsageState {
    pub tracker: Arc<UsageTracker>,
}

impl UsageState {
    pub fn new(data_dir: PathBuf) -> Result<Self, String> {
        let client = SupabaseClient::from_env().ok();
        let tracker = UsageTracker::new(data_dir, client)?;
        Ok(Self {
            tracker: Arc::new(tracker),
        })
    }
}

/// 初始化使用量状态
pub fn init_usage_state(app: &AppHandle) -> Result<UsageState, String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取数据目录失败: {}", e))?;
    UsageState::new(data_dir)
}

/// 使用量响应
#[derive(Debug, Serialize)]
pub struct UsageResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stats: Option<UsageStats>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 使用检查响应
#[derive(Debug, Serialize)]
pub struct UsageCheckResponse {
    pub allowed: bool,
    pub current_count: u32,
    pub daily_limit: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// 获取功能使用统计
#[tauri::command]
pub fn get_usage_stats(
    feature: String,
    state: State<'_, UsageState>,
) -> Result<UsageResponse, String> {
    let feature = UsageFeature::from(feature.as_str());

    match state.tracker.get_stats(feature) {
        Ok(stats) => Ok(UsageResponse {
            success: true,
            stats: Some(stats),
            error: None,
        }),
        Err(e) => Ok(UsageResponse {
            success: false,
            stats: None,
            error: Some(e),
        }),
    }
}

/// 检查是否可以使用功能
#[tauri::command]
pub fn check_usage(
    feature: String,
    state: State<'_, UsageState>,
) -> Result<UsageCheckResponse, String> {
    let feature = UsageFeature::from(feature.as_str());

    let stats = state.tracker.get_stats(feature).map_err(|e| e.to_string())?;

    let allowed = !stats.is_limited;
    let message = if !allowed {
        Some(format!(
            "今日{}次数已用完（{}/{}），请明日再试或升级 VIP",
            feature, stats.today_count, stats.daily_limit
        ))
    } else {
        None
    };

    Ok(UsageCheckResponse {
        allowed,
        current_count: stats.today_count,
        daily_limit: stats.daily_limit,
        message,
    })
}

/// 记录功能使用
#[tauri::command]
pub fn record_usage(
    feature: String,
    state: State<'_, UsageState>,
) -> Result<UsageCheckResponse, String> {
    let feature_enum = UsageFeature::from(feature.as_str());

    let (allowed, count, limit) = state.tracker
        .check_and_increment(feature_enum)
        .map_err(|e| e.to_string())?;

    let message = if !allowed {
        Some(format!(
            "今日{}次数已用完（{}/{}）",
            feature, count, limit
        ))
    } else {
        None
    };

    Ok(UsageCheckResponse {
        allowed,
        current_count: count,
        daily_limit: limit,
        message,
    })
}

/// 同步使用量到云端
#[tauri::command]
pub async fn sync_usage_to_cloud(
    auth_state: State<'_, AuthState>,
    usage_state: State<'_, UsageState>,
) -> Result<bool, String> {
    // 获取认证信息
    let auth_service = auth_state.service.lock().await;

    let (user_id, access_token) = match (auth_service.current_user_id(), auth_service.access_token())
    {
        (Some(uid), Some(token)) => (uid.to_string(), token.to_string()),
        _ => return Ok(false), // 未登录
    };
    drop(auth_service);

    // 克隆 tracker 的 Arc 以便在 async 块中使用
    let tracker = usage_state.tracker.clone();

    // 同步
    tracker
        .sync_to_cloud(&user_id, &access_token)
        .await
        .map_err(|e| e.to_string())?;

    info!("使用量同步完成");
    Ok(true)
}
