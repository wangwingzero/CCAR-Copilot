//! 功能门控核心实现

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::license::LicenseService;
use crate::usage::{UsageTracker, UsageFeature};

/// 功能等级
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureTier {
    /// 免费功能
    Free,
    /// VIP 专属功能
    Vip,
    /// 内测功能（仅开发者）
    Beta,
}

/// 功能访问结果
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureAccess {
    /// 是否允许访问
    pub allowed: bool,
    /// 功能名称
    pub feature: String,
    /// 功能等级
    pub tier: FeatureTier,
    /// 是否是 VIP 用户
    pub is_vip: bool,
    /// 使用次数（如果有限制）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_count: Option<u32>,
    /// 使用限制（如果有限制）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage_limit: Option<u32>,
    /// 限制原因（如果被拒绝）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deny_reason: Option<String>,
    /// 升级提示
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upgrade_hint: Option<String>,
}

/// 功能定义
#[derive(Debug, Clone)]
pub struct FeatureDefinition {
    /// 功能名称
    pub name: String,
    /// 功能等级
    pub tier: FeatureTier,
    /// 是否需要追踪使用量
    pub track_usage: bool,
    /// 对应的使用量功能（如果需要追踪）
    pub usage_feature: Option<UsageFeature>,
}

/// 功能门控系统
pub struct FeatureGate {
    /// 许可证服务
    license: Arc<Mutex<LicenseService>>,
    /// 使用量追踪器
    usage: Arc<UsageTracker>,
    /// 功能定义表
    features: Vec<FeatureDefinition>,
}

impl FeatureGate {
    /// 创建功能门控系统
    pub fn new(license: Arc<Mutex<LicenseService>>, usage: Arc<UsageTracker>) -> Self {
        // 定义所有功能
        let features = vec![
            // 免费功能
            FeatureDefinition {
                name: "screenshot".to_string(),
                tier: FeatureTier::Free,
                track_usage: false,
                usage_feature: None,
            },
            FeatureDefinition {
                name: "annotation".to_string(),
                tier: FeatureTier::Free,
                track_usage: false,
                usage_feature: None,
            },
            FeatureDefinition {
                name: "pin_window".to_string(),
                tier: FeatureTier::Free,
                track_usage: false,
                usage_feature: None,
            },
            // 有使用量限制的功能
            FeatureDefinition {
                name: "translation".to_string(),
                tier: FeatureTier::Free,
                track_usage: true,
                usage_feature: Some(UsageFeature::Translation),
            },
            FeatureDefinition {
                name: "web_to_markdown".to_string(),
                tier: FeatureTier::Free,
                track_usage: true,
                usage_feature: Some(UsageFeature::WebToMarkdown),
            },
            FeatureDefinition {
                name: "ocr".to_string(),
                tier: FeatureTier::Free,
                track_usage: true,
                usage_feature: Some(UsageFeature::Ocr),
            },
            // VIP 专属功能
            FeatureDefinition {
                name: "screen_recording".to_string(),
                tier: FeatureTier::Vip,
                track_usage: true,
                usage_feature: Some(UsageFeature::ScreenRecording),
            },
            FeatureDefinition {
                name: "batch_ocr".to_string(),
                tier: FeatureTier::Vip,
                track_usage: false,
                usage_feature: None,
            },
            FeatureDefinition {
                name: "anki_export".to_string(),
                tier: FeatureTier::Vip,
                track_usage: false,
                usage_feature: None,
            },
            FeatureDefinition {
                name: "advanced_annotation".to_string(),
                tier: FeatureTier::Vip,
                track_usage: false,
                usage_feature: None,
            },
        ];

        Self {
            license,
            usage,
            features,
        }
    }

    /// 检查用户是否是 VIP（同步方法，使用 try_lock）
    fn check_is_vip(&self) -> bool {
        match self.license.try_lock() {
            Ok(guard) => {
                guard.cached_license()
                    .map(|info| info.is_vip())
                    .unwrap_or(false)
            }
            Err(_) => {
                // 如果无法获取锁，默认返回 false（保守策略）
                tracing::warn!("无法获取许可证锁，默认非 VIP");
                false
            }
        }
    }

    /// 检查功能访问权限
    pub fn check_access(&self, feature_name: &str) -> FeatureAccess {
        // 查找功能定义
        let feature_def = self
            .features
            .iter()
            .find(|f| f.name == feature_name);

        let feature_def = match feature_def {
            Some(f) => f,
            None => {
                // 未知功能默认拒绝
                return FeatureAccess {
                    allowed: false,
                    feature: feature_name.to_string(),
                    tier: FeatureTier::Free,
                    is_vip: false,
                    usage_count: None,
                    usage_limit: None,
                    deny_reason: Some("未知功能".to_string()),
                    upgrade_hint: None,
                };
            }
        };

        // 检查许可证状态
        let is_vip = self.check_is_vip();

        // VIP 用户可以访问所有功能
        if is_vip {
            return FeatureAccess {
                allowed: true,
                feature: feature_name.to_string(),
                tier: feature_def.tier,
                is_vip: true,
                usage_count: None,
                usage_limit: None,
                deny_reason: None,
                upgrade_hint: None,
            };
        }

        // 非 VIP 用户检查功能等级
        match feature_def.tier {
            FeatureTier::Vip => {
                // VIP 专属功能，拒绝访问
                FeatureAccess {
                    allowed: false,
                    feature: feature_name.to_string(),
                    tier: feature_def.tier,
                    is_vip: false,
                    usage_count: None,
                    usage_limit: None,
                    deny_reason: Some("此功能需要 VIP 会员".to_string()),
                    upgrade_hint: Some("升级 VIP 解锁此功能".to_string()),
                }
            }
            FeatureTier::Beta => {
                // 内测功能，拒绝访问
                FeatureAccess {
                    allowed: false,
                    feature: feature_name.to_string(),
                    tier: feature_def.tier,
                    is_vip: false,
                    usage_count: None,
                    usage_limit: None,
                    deny_reason: Some("此功能正在内测中".to_string()),
                    upgrade_hint: None,
                }
            }
            FeatureTier::Free => {
                // 免费功能，检查使用量限制
                if feature_def.track_usage {
                    if let Some(usage_feature) = feature_def.usage_feature {
                        self.check_usage_limit(feature_name, feature_def.tier, usage_feature)
                    } else {
                        // 需要追踪但没有定义使用量功能，允许访问
                        FeatureAccess {
                            allowed: true,
                            feature: feature_name.to_string(),
                            tier: feature_def.tier,
                            is_vip: false,
                            usage_count: None,
                            usage_limit: None,
                            deny_reason: None,
                            upgrade_hint: None,
                        }
                    }
                } else {
                    // 不需要追踪使用量，允许访问
                    FeatureAccess {
                        allowed: true,
                        feature: feature_name.to_string(),
                        tier: feature_def.tier,
                        is_vip: false,
                        usage_count: None,
                        usage_limit: None,
                        deny_reason: None,
                        upgrade_hint: None,
                    }
                }
            }
        }
    }

    /// 检查使用量限制
    fn check_usage_limit(
        &self,
        feature_name: &str,
        tier: FeatureTier,
        usage_feature: UsageFeature,
    ) -> FeatureAccess {
        match self.usage.get_stats(usage_feature) {
            Ok(stats) => {
                if stats.is_limited {
                    FeatureAccess {
                        allowed: false,
                        feature: feature_name.to_string(),
                        tier,
                        is_vip: false,
                        usage_count: Some(stats.today_count),
                        usage_limit: Some(stats.daily_limit),
                        deny_reason: Some(format!(
                            "今日使用次数已达上限（{}/{}）",
                            stats.today_count, stats.daily_limit
                        )),
                        upgrade_hint: Some("升级 VIP 解锁无限使用".to_string()),
                    }
                } else {
                    FeatureAccess {
                        allowed: true,
                        feature: feature_name.to_string(),
                        tier,
                        is_vip: false,
                        usage_count: Some(stats.today_count),
                        usage_limit: if stats.daily_limit > 0 {
                            Some(stats.daily_limit)
                        } else {
                            None
                        },
                        deny_reason: None,
                        upgrade_hint: None,
                    }
                }
            }
            Err(e) => {
                // 使用量查询失败，允许访问（降级策略）
                tracing::warn!("使用量查询失败，降级允许访问: {}", e);
                FeatureAccess {
                    allowed: true,
                    feature: feature_name.to_string(),
                    tier,
                    is_vip: false,
                    usage_count: None,
                    usage_limit: None,
                    deny_reason: None,
                    upgrade_hint: None,
                }
            }
        }
    }

    /// 使用功能（增加使用计数）
    pub fn use_feature(&self, feature_name: &str) -> FeatureAccess {
        // 先检查访问权限
        let access = self.check_access(feature_name);
        if !access.allowed {
            return access;
        }

        // 查找功能定义
        let feature_def = self
            .features
            .iter()
            .find(|f| f.name == feature_name);

        let feature_def = match feature_def {
            Some(f) => f,
            None => return access,
        };

        // 如果需要追踪使用量，增加计数
        if feature_def.track_usage {
            if let Some(usage_feature) = feature_def.usage_feature {
                let is_vip = self.check_is_vip();

                match self.usage.check_and_increment(usage_feature) {
                    Ok((allowed, count, limit)) => {
                        return FeatureAccess {
                            allowed,
                            feature: feature_name.to_string(),
                            tier: feature_def.tier,
                            is_vip,
                            usage_count: Some(count),
                            usage_limit: if limit > 0 { Some(limit) } else { None },
                            deny_reason: if !allowed {
                                Some(format!(
                                    "今日使用次数已达上限（{}/{}）",
                                    count, limit
                                ))
                            } else {
                                None
                            },
                            upgrade_hint: if !allowed && !is_vip {
                                Some("升级 VIP 解锁无限使用".to_string())
                            } else {
                                None
                            },
                        };
                    }
                    Err(e) => {
                        tracing::warn!("使用量更新失败: {}", e);
                    }
                }
            }
        }

        access
    }

    /// 获取所有功能及其状态
    pub fn get_all_features(&self) -> Vec<FeatureAccess> {
        self.features
            .iter()
            .map(|f| self.check_access(&f.name))
            .collect()
    }

    /// 获取 VIP 专属功能列表
    pub fn get_vip_features(&self) -> Vec<String> {
        self.features
            .iter()
            .filter(|f| f.tier == FeatureTier::Vip)
            .map(|f| f.name.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 测试需要 mock LicenseService 和 UsageTracker
    // 这里只测试基本结构
    #[test]
    fn test_feature_tier_serialize() {
        let tier = FeatureTier::Vip;
        let json = serde_json::to_string(&tier).unwrap();
        assert_eq!(json, "\"vip\"");

        let tier = FeatureTier::Free;
        let json = serde_json::to_string(&tier).unwrap();
        assert_eq!(json, "\"free\"");
    }

    #[test]
    fn test_feature_access_serialize() {
        let access = FeatureAccess {
            allowed: true,
            feature: "screenshot".to_string(),
            tier: FeatureTier::Free,
            is_vip: false,
            usage_count: Some(5),
            usage_limit: Some(10),
            deny_reason: None,
            upgrade_hint: None,
        };

        let json = serde_json::to_string(&access).unwrap();
        assert!(json.contains("\"allowed\":true"));
        assert!(json.contains("\"feature\":\"screenshot\""));
        assert!(json.contains("\"usage_count\":5"));
    }
}
