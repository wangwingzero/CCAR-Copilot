//! 设备管理相关的 Tauri 命令

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;
use tracing::info;

use crate::commands::auth_cmd::AuthState;
use crate::device::{get_device_info, DeviceInfo as RustDeviceInfo};
use crate::supabase::SupabaseClient;

/// 设备信息（用于前端显示）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    /// 设备唯一 ID
    pub device_id: String,
    /// 设备名称
    pub device_name: String,
    /// 操作系统版本
    pub os_version: String,
    /// 是否是当前设备
    pub is_current: bool,
    /// 绑定时间
    pub bound_at: String,
    /// 最后活跃时间
    pub last_active_at: String,
}

impl From<RustDeviceInfo> for DeviceInfo {
    fn from(info: RustDeviceInfo) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            device_id: info.machine_id,
            device_name: info.device_name,
            os_version: info.os_version,
            is_current: true,
            bound_at: now.clone(),
            last_active_at: now,
        }
    }
}

/// 设备绑定记录（从数据库返回）
#[derive(Debug, Clone, Deserialize)]
struct DeviceBinding {
    /// 数据库主键 ID
    #[allow(dead_code)]
    id: Option<String>,
    /// 机器 ID (对应 devices 表的 machine_id)
    machine_id: String,
    device_name: Option<String>,
    os_version: Option<String>,
    created_at: Option<String>,
    last_active_at: Option<String>,
}

/// 设备列表响应
#[derive(Debug, Serialize)]
pub struct DeviceListResponse {
    /// 是否成功
    pub success: bool,
    /// 设备列表
    #[serde(skip_serializing_if = "Option::is_none")]
    pub devices: Option<Vec<DeviceInfo>>,
    /// 最大设备数
    pub max_devices: u32,
    /// 错误信息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 解绑设备响应
#[derive(Debug, Serialize)]
pub struct UnbindDeviceResponse {
    /// 是否成功
    pub success: bool,
    /// 错误信息
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 设备状态（全局共享）
pub struct DeviceState {
    pub client: Arc<Mutex<SupabaseClient>>,
    pub current_device: Arc<Mutex<Option<RustDeviceInfo>>>,
}

impl DeviceState {
    /// 创建设备状态
    pub fn new() -> Result<Self, String> {
        let client = SupabaseClient::from_env()
            .map_err(|e| format!("创建 Supabase 客户端失败: {}", e))?;

        // 获取当前设备信息
        let device_info = get_device_info();

        Ok(Self {
            client: Arc::new(Mutex::new(client)),
            current_device: Arc::new(Mutex::new(Some(device_info))),
        })
    }
}

/// 初始化设备状态
pub fn init_device_state() -> Result<DeviceState, String> {
    DeviceState::new()
}

/// 获取当前设备信息
#[tauri::command]
pub async fn get_current_device(
    device_state: State<'_, DeviceState>,
) -> Result<DeviceInfo, String> {
    let current = device_state.current_device.lock().await;

    match current.as_ref() {
        Some(info) => Ok(DeviceInfo::from(info.clone())),
        None => {
            // 如果没有缓存，重新获取
            drop(current);
            let info = get_device_info();
            let mut current = device_state.current_device.lock().await;
            *current = Some(info.clone());
            Ok(DeviceInfo::from(info))
        }
    }
}

/// 获取绑定的设备列表
#[tauri::command]
pub async fn get_bound_devices(
    auth_state: State<'_, AuthState>,
    device_state: State<'_, DeviceState>,
) -> Result<DeviceListResponse, String> {
    // 获取当前用户信息
    let auth_service = auth_state.service.lock().await;

    let (user_id, access_token) = match (auth_service.current_user_id(), auth_service.access_token())
    {
        (Some(uid), Some(token)) => (uid.to_string(), token.to_string()),
        _ => {
            return Ok(DeviceListResponse {
                success: false,
                devices: None,
                max_devices: 3,
                error: Some("请先登录".to_string()),
            });
        }
    };
    drop(auth_service);

    // 获取当前设备 ID
    let current_device_id = {
        let current = device_state.current_device.lock().await;
        current.as_ref().map(|d| d.machine_id.clone()).unwrap_or_default()
    };

    // 从 Supabase 获取设备列表（使用 devices 表）
    let client = device_state.client.lock().await;
    let base_url = client.config().url.clone();
    let url = format!(
        "{}/rest/v1/devices?user_id=eq.{}&select=*",
        base_url, user_id
    );

    match client.get::<Vec<DeviceBinding>>(&url, Some(&access_token)).await {
        Ok(bindings) => {
            let devices: Vec<DeviceInfo> = bindings
                .into_iter()
                .map(|b| DeviceInfo {
                    device_id: b.machine_id.clone(),
                    device_name: b.device_name.unwrap_or_else(|| "Unknown".to_string()),
                    os_version: b.os_version.unwrap_or_default(),
                    is_current: b.machine_id == current_device_id,
                    bound_at: b.created_at.unwrap_or_default(),
                    last_active_at: b.last_active_at.unwrap_or_default(),
                })
                .collect();

            info!("获取设备列表成功: {} 台设备", devices.len());

            Ok(DeviceListResponse {
                success: true,
                devices: Some(devices),
                max_devices: 3,
                error: None,
            })
        }
        Err(e) => Ok(DeviceListResponse {
            success: false,
            devices: None,
            max_devices: 3,
            error: Some(format!("获取设备列表失败: {}", e)),
        }),
    }
}

/// 绑定当前设备
#[tauri::command]
pub async fn bind_current_device(
    auth_state: State<'_, AuthState>,
    device_state: State<'_, DeviceState>,
) -> Result<UnbindDeviceResponse, String> {
    // 获取当前用户信息
    let auth_service = auth_state.service.lock().await;

    let (user_id, access_token) = match (auth_service.current_user_id(), auth_service.access_token())
    {
        (Some(uid), Some(token)) => (uid.to_string(), token.to_string()),
        _ => {
            return Ok(UnbindDeviceResponse {
                success: false,
                error: Some("请先登录".to_string()),
            });
        }
    };
    drop(auth_service);

    // 获取当前设备信息
    let device_info = {
        let current = device_state.current_device.lock().await;
        current.clone().unwrap_or_else(get_device_info)
    };

    // 插入设备记录（使用 devices 表）
    let client = device_state.client.lock().await;
    let base_url = client.config().url.clone();
    let url = format!("{}/rest/v1/devices", base_url);

    let body = serde_json::json!({
        "user_id": user_id,
        "machine_id": device_info.machine_id,
        "device_name": device_info.device_name,
        "os_version": device_info.os_version,
        "last_active_at": chrono::Utc::now().to_rfc3339(),
    });

    match client.post::<serde_json::Value, _>(&url, &body, Some(&access_token)).await {
        Ok(_) => {
            info!("设备绑定成功: {}", device_info.machine_id);
            Ok(UnbindDeviceResponse {
                success: true,
                error: None,
            })
        }
        Err(e) => {
            // 如果是冲突错误（设备已绑定），也视为成功
            let err_str = e.to_string();
            if err_str.contains("409") || err_str.contains("duplicate") || err_str.contains("already exists") {
                info!("设备已绑定: {}", device_info.machine_id);
                Ok(UnbindDeviceResponse {
                    success: true,
                    error: None,
                })
            } else {
                Ok(UnbindDeviceResponse {
                    success: false,
                    error: Some(format!("绑定失败: {}", e)),
                })
            }
        }
    }
}

/// 解绑设备
#[tauri::command]
pub async fn unbind_device(
    device_id: String,
    auth_state: State<'_, AuthState>,
    device_state: State<'_, DeviceState>,
) -> Result<UnbindDeviceResponse, String> {
    // 获取当前用户信息
    let auth_service = auth_state.service.lock().await;

    let (user_id, access_token) = match (auth_service.current_user_id(), auth_service.access_token())
    {
        (Some(uid), Some(token)) => (uid.to_string(), token.to_string()),
        _ => {
            return Ok(UnbindDeviceResponse {
                success: false,
                error: Some("请先登录".to_string()),
            });
        }
    };
    drop(auth_service);

    // 检查是否尝试解绑当前设备
    let current_device_id = {
        let current = device_state.current_device.lock().await;
        current.as_ref().map(|d| d.machine_id.clone()).unwrap_or_default()
    };

    if device_id == current_device_id {
        return Ok(UnbindDeviceResponse {
            success: false,
            error: Some("无法解绑当前设备".to_string()),
        });
    }

    // 删除设备记录（使用 devices 表）
    let client = device_state.client.lock().await;
    let base_url = client.config().url.clone();
    let url = format!(
        "{}/rest/v1/devices?user_id=eq.{}&machine_id=eq.{}",
        base_url, user_id, device_id
    );

    match client.delete::<serde_json::Value>(&url, Some(&access_token)).await {
        Ok(_) => {
            info!("设备解绑成功: {}", device_id);
            Ok(UnbindDeviceResponse {
                success: true,
                error: None,
            })
        }
        Err(e) => Ok(UnbindDeviceResponse {
            success: false,
            error: Some(format!("解绑失败: {}", e)),
        }),
    }
}

/// 更新设备最后活跃时间
#[tauri::command]
pub async fn update_device_activity(
    auth_state: State<'_, AuthState>,
    device_state: State<'_, DeviceState>,
) -> Result<(), String> {
    // 获取当前用户信息
    let auth_service = auth_state.service.lock().await;

    let (user_id, access_token) = match (auth_service.current_user_id(), auth_service.access_token())
    {
        (Some(uid), Some(token)) => (uid.to_string(), token.to_string()),
        _ => return Ok(()), // 未登录则静默返回
    };
    drop(auth_service);

    // 获取当前设备 ID
    let device_id = {
        let current = device_state.current_device.lock().await;
        current.as_ref().map(|d| d.machine_id.clone()).unwrap_or_default()
    };

    if device_id.is_empty() {
        return Ok(());
    }

    // 更新最后活跃时间（使用 devices 表）
    let client = device_state.client.lock().await;
    let base_url = client.config().url.clone();
    let url = format!(
        "{}/rest/v1/devices?user_id=eq.{}&machine_id=eq.{}",
        base_url, user_id, device_id
    );

    let body = serde_json::json!({
        "last_active_at": chrono::Utc::now().to_rfc3339(),
    });

    // 忽略更新结果
    let _ = client.patch::<serde_json::Value, _>(&url, &body, Some(&access_token)).await;

    Ok(())
}
