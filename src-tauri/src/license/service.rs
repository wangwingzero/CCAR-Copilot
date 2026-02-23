//! 许可证验证服务实现
//!
//! 提供 VIP 订阅状态的验证和缓存

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs;
use tracing::{debug, info, warn};

use crate::supabase::{DatabaseService, SupabaseClient, SupabaseError};

/// 订阅计划类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum Plan {
    /// 免费用户
    #[default]
    Free,
    /// 终身 VIP
    LifetimeVip,
}


impl std::fmt::Display for Plan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Plan::Free => write!(f, "free"),
            Plan::LifetimeVip => write!(f, "lifetime_vip"),
        }
    }
}

impl From<&str> for Plan {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "lifetime_vip" | "lifetimevip" | "vip" => Plan::LifetimeVip,
            _ => Plan::Free,
        }
    }
}

/// 许可证信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseInfo {
    /// 订阅计划
    pub plan: Plan,
    /// 是否有效
    pub is_valid: bool,
    /// 缓存时间
    pub cached_at: DateTime<Utc>,
    /// 宽限期结束时间（离线时使用）
    pub grace_period_end: Option<DateTime<Utc>>,
    /// 用户 ID
    pub user_id: Option<String>,
}

impl Default for LicenseInfo {
    fn default() -> Self {
        Self {
            plan: Plan::Free,
            is_valid: true,
            cached_at: Utc::now(),
            grace_period_end: None,
            user_id: None,
        }
    }
}

impl LicenseInfo {
    /// 创建免费用户许可证
    pub fn free() -> Self {
        Self::default()
    }

    /// 创建 VIP 许可证
    pub fn vip(user_id: &str) -> Self {
        let now = Utc::now();
        Self {
            plan: Plan::LifetimeVip,
            is_valid: true,
            cached_at: now,
            grace_period_end: Some(now + Duration::days(7)),
            user_id: Some(user_id.to_string()),
        }
    }

    /// 检查是否是 VIP
    pub fn is_vip(&self) -> bool {
        self.plan == Plan::LifetimeVip && self.is_valid
    }
}

/// 许可证验证服务
pub struct LicenseService {
    /// Supabase 数据库服务
    db: DatabaseService,
    /// 缓存的许可证信息
    cache: Option<LicenseInfo>,
    /// 缓存 TTL（24 小时）
    cache_ttl: Duration,
    /// 离线宽限期（7 天）
    grace_period: Duration,
    /// 缓存文件路径
    cache_path: PathBuf,
}

impl LicenseService {
    /// 创建许可证服务
    pub fn new(client: SupabaseClient, data_dir: PathBuf) -> Self {
        let db = DatabaseService::new(client);
        let cache_path = data_dir.join("license_cache.json");

        Self {
            db,
            cache: None,
            cache_ttl: Duration::hours(24),
            grace_period: Duration::days(7),
            cache_path,
        }
    }

    /// 验证用户许可证
    ///
    /// 验证流程：
    /// 1. 检查本地缓存是否有效
    /// 2. 如果缓存无效，从服务器获取
    /// 3. 如果网络失败，检查宽限期
    pub async fn validate(&mut self, user_id: &str, access_token: &str) -> LicenseInfo {
        // 1. 检查缓存
        if let Some(ref cached) = self.cache {
            if cached.user_id.as_deref() == Some(user_id) {
                let cache_age = Utc::now() - cached.cached_at;

                // VIP 用户：缓存 24 小时有效
                if cached.plan == Plan::LifetimeVip && cache_age < self.cache_ttl {
                    debug!("使用缓存的 VIP 许可证");
                    return cached.clone();
                }

                // 免费用户：缓存 1 小时有效
                if cached.plan == Plan::Free && cache_age < Duration::hours(1) {
                    debug!("使用缓存的免费用户许可证");
                    return cached.clone();
                }
            }
        }

        // 2. 从服务器获取
        match self.fetch_license(user_id, access_token).await {
            Ok(license) => {
                info!("许可证验证成功: plan={}", license.plan);
                self.cache = Some(license.clone());
                let _ = self.save_cache().await;
                license
            }
            Err(e) => {
                warn!("许可证验证失败: {}", e);

                // 3. 网络失败时检查宽限期
                if let Some(ref cached) = self.cache {
                    if cached.plan == Plan::LifetimeVip {
                        if let Some(grace_end) = cached.grace_period_end {
                            if Utc::now() < grace_end {
                                info!("使用宽限期内的 VIP 许可证");
                                return cached.clone();
                            }
                        }
                    }
                }

                // 尝试从本地缓存文件恢复
                if let Ok(Some(cached)) = self.load_cache().await {
                    if cached.user_id.as_deref() == Some(user_id)
                        && cached.plan == Plan::LifetimeVip {
                            if let Some(grace_end) = cached.grace_period_end {
                                if Utc::now() < grace_end {
                                    info!("从本地恢复宽限期内的 VIP 许可证");
                                    self.cache = Some(cached.clone());
                                    return cached;
                                }
                            }
                        }
                }

                // 默认返回免费用户
                LicenseInfo::free()
            }
        }
    }

    /// 从服务器获取许可证
    async fn fetch_license(
        &self,
        user_id: &str,
        access_token: &str,
    ) -> Result<LicenseInfo, SupabaseError> {
        #[derive(Debug, Deserialize)]
        struct SubscriptionRow {
            plan: String,
            status: String,
        }

        let result: Option<SubscriptionRow> = self
            .db
            .from("subscriptions")
            .select("plan,status")
            .eq("user_id", user_id)
            .single()
            .execute_single(Some(access_token))
            .await?;

        match result {
            Some(sub) if sub.status == "active" => {
                let plan = Plan::from(sub.plan.as_str());
                let now = Utc::now();

                Ok(LicenseInfo {
                    plan,
                    is_valid: true,
                    cached_at: now,
                    grace_period_end: if plan == Plan::LifetimeVip {
                        Some(now + self.grace_period)
                    } else {
                        None
                    },
                    user_id: Some(user_id.to_string()),
                })
            }
            _ => Ok(LicenseInfo::free()),
        }
    }

    /// 获取缓存的许可证（不触发验证）
    pub fn cached_license(&self) -> Option<&LicenseInfo> {
        self.cache.as_ref()
    }

    /// 清除缓存
    pub async fn clear_cache(&mut self) {
        self.cache = None;
        if self.cache_path.exists() {
            let _ = fs::remove_file(&self.cache_path).await;
        }
    }

    /// 保存缓存到文件
    async fn save_cache(&self) -> Result<(), std::io::Error> {
        if let Some(ref cache) = self.cache {
            if let Some(parent) = self.cache_path.parent() {
                fs::create_dir_all(parent).await?;
            }
            let content = serde_json::to_string_pretty(cache)
                .map_err(std::io::Error::other)?;
            fs::write(&self.cache_path, content).await?;
        }
        Ok(())
    }

    /// 从文件加载缓存
    async fn load_cache(&self) -> Result<Option<LicenseInfo>, std::io::Error> {
        if !self.cache_path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&self.cache_path).await?;
        let cache: LicenseInfo = serde_json::from_str(&content)
            .map_err(std::io::Error::other)?;
        Ok(Some(cache))
    }

    /// 初始化时加载缓存
    pub async fn init(&mut self) {
        if let Ok(Some(cache)) = self.load_cache().await {
            self.cache = Some(cache);
            debug!("从本地加载许可证缓存");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plan_from_str() {
        assert_eq!(Plan::from("free"), Plan::Free);
        assert_eq!(Plan::from("lifetime_vip"), Plan::LifetimeVip);
        assert_eq!(Plan::from("vip"), Plan::LifetimeVip);
        assert_eq!(Plan::from("unknown"), Plan::Free);
    }

    #[test]
    fn test_license_info_default() {
        let info = LicenseInfo::default();
        assert_eq!(info.plan, Plan::Free);
        assert!(info.is_valid);
        assert!(!info.is_vip());
    }

    #[test]
    fn test_license_info_vip() {
        let info = LicenseInfo::vip("user123");
        assert_eq!(info.plan, Plan::LifetimeVip);
        assert!(info.is_valid);
        assert!(info.is_vip());
        assert!(info.grace_period_end.is_some());
    }
}
