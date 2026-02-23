//! Feature Gate 相关的 Tauri 命令

use serde::Serialize;
use std::sync::Arc;
use tauri::State;

use crate::feature_gate::{FeatureGate, FeatureAccess};
use crate::commands::license_cmd::LicenseState;
use crate::commands::usage_cmd::UsageState;

/// Feature Gate 状态（全局共享）
pub struct FeatureGateState {
    pub gate: Arc<FeatureGate>,
}

/// 初始化 Feature Gate 状态
pub fn init_feature_gate_state(
    license_state: &LicenseState,
    usage_state: &UsageState,
) -> FeatureGateState {
    let gate = FeatureGate::new(
        license_state.service.clone(),
        usage_state.tracker.clone(),
    );
    FeatureGateState {
        gate: Arc::new(gate),
    }
}

/// 功能列表响应
#[derive(Debug, Serialize)]
pub struct FeatureListResponse {
    pub features: Vec<FeatureAccess>,
    pub vip_features: Vec<String>,
}

/// 检查功能访问权限
#[tauri::command]
pub fn check_feature_access(
    feature: String,
    state: State<'_, FeatureGateState>,
) -> FeatureAccess {
    state.gate.check_access(&feature)
}

/// 使用功能（检查权限并增加使用计数）
#[tauri::command]
pub fn use_feature(
    feature: String,
    state: State<'_, FeatureGateState>,
) -> FeatureAccess {
    state.gate.use_feature(&feature)
}

/// 获取所有功能状态
#[tauri::command]
pub fn get_all_features(
    state: State<'_, FeatureGateState>,
) -> FeatureListResponse {
    FeatureListResponse {
        features: state.gate.get_all_features(),
        vip_features: state.gate.get_vip_features(),
    }
}

/// 批量检查功能访问权限
#[tauri::command]
pub fn check_features_batch(
    features: Vec<String>,
    state: State<'_, FeatureGateState>,
) -> Vec<FeatureAccess> {
    features
        .iter()
        .map(|f| state.gate.check_access(f))
        .collect()
}
