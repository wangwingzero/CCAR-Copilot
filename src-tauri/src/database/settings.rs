//! 设置持久化
//!
//! 管理应用配置的存储和读取。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::RwLock;
use tauri::{AppHandle, Manager, Runtime};
use tracing::{debug, error, info, warn};

use crate::error::{HuGeError, HuGeResult};

/// 配置文件名
const CONFIG_FILE_NAME: &str = "config.json";

/// 全局配置缓存
static CONFIG_CACHE: RwLock<Option<AppConfig>> = RwLock::new(None);

/// 应用配置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    /// 通用设置
    #[serde(default)]
    pub general: GeneralConfig,
    /// 通知设置
    #[serde(default)]
    pub notification: NotificationConfig,
    /// 更新设置
    #[serde(default)]
    pub update: UpdateConfig,
    /// 高级设置
    #[serde(default)]
    pub advanced: AdvancedConfig,
}

/// 通用设置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneralConfig {
    /// 界面语言
    pub language: String,
    /// 主题：light, dark, system
    pub theme: String,
    /// 开机自启动
    pub auto_start: bool,
    /// 关闭时最小化到托盘
    pub minimize_to_tray: bool,
    /// 是否启用自动更新
    #[serde(default = "default_auto_update_enabled")]
    pub auto_update_enabled: Option<bool>,
    /// 检查更新间隔（小时）
    #[serde(default)]
    pub update_check_interval_hours: Option<u32>,
    /// 是否在启动时检查更新
    #[serde(default = "default_check_update_on_startup")]
    pub check_update_on_startup: Option<bool>,
    /// 是否自动下载更新
    #[serde(default = "default_auto_download_update")]
    pub auto_download_update: Option<bool>,
    /// 是否自动安装更新
    #[serde(default)]
    pub auto_install_update: Option<bool>,
}

fn default_auto_update_enabled() -> Option<bool> {
    Some(true)
}

fn default_check_update_on_startup() -> Option<bool> {
    Some(true)
}

fn default_auto_download_update() -> Option<bool> {
    Some(true)
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            language: "zh-CN".to_string(),
            theme: "system".to_string(),
            auto_start: false,
            minimize_to_tray: true,
            auto_update_enabled: Some(true),
            update_check_interval_hours: Some(24),
            check_update_on_startup: Some(true),
            auto_download_update: Some(true),
            auto_install_update: Some(false),
        }
    }
}

fn default_true() -> bool {
    true
}

/// 通知设置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationConfig {
    /// 启动时显示通知
    #[serde(default = "default_true")]
    pub startup: bool,
    /// 软件更新时显示通知
    #[serde(default = "default_true")]
    pub software_update: bool,
}

impl Default for NotificationConfig {
    fn default() -> Self {
        Self { startup: true, software_update: true }
    }
}

/// 更新设置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateConfig {
    /// 自动检查更新
    #[serde(default = "default_true")]
    pub auto_check: bool,
    /// 检查间隔（小时，范围 1-168）
    #[serde(default = "default_check_interval")]
    pub check_interval_hours: u32,
    /// 使用代理检查更新
    #[serde(default)]
    pub use_proxy: bool,
    /// 代理 URL
    #[serde(default = "default_proxy_url")]
    pub proxy_url: String,
    /// 上次检查时间（ISO 8601 格式）
    #[serde(default)]
    pub last_check_time: String,
    /// 跳过的版本号
    #[serde(default)]
    pub skip_version: String,
}

fn default_check_interval() -> u32 {
    24
}

fn default_proxy_url() -> String {
    "https://ghproxy.net/".to_string()
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            auto_check: true,
            check_interval_hours: 24,
            use_proxy: false,
            proxy_url: "https://ghproxy.net/".to_string(),
            last_check_time: String::new(),
            skip_version: String::new(),
        }
    }
}

/// 高级设置
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdvancedConfig {
    /// 是否启用代理
    #[serde(default)]
    pub proxy_enabled: bool,
    /// 代理类型：http 或 socks5
    #[serde(default = "default_proxy_type")]
    pub proxy_type: String,
    /// 代理主机
    #[serde(default)]
    pub proxy_host: String,
    /// 代理端口
    #[serde(default = "default_proxy_port")]
    pub proxy_port: u16,
    /// 是否启用调试日志
    #[serde(default)]
    pub debug_logging: bool,
    /// 调试日志路径
    #[serde(default)]
    pub debug_log_path: String,
    /// 便携模式
    #[serde(default)]
    pub portable_mode: bool,
    /// 局方文件保存目录（空字符串表示使用默认 AppData 目录）
    #[serde(default)]
    pub regulation_storage_path: String,
    /// 是否每天自动同步局方官网文件
    #[serde(default)]
    pub regulation_auto_sync_enabled: bool,
    /// 自动同步是否仅在 Wi-Fi 连接时执行
    #[serde(default = "default_true")]
    pub regulation_auto_sync_wifi_only: bool,
    /// 是否启用 MinerU 在线 OCR
    #[serde(default)]
    pub mineru_ocr_enabled: bool,
    /// 是否优先使用 MinerU 在线 OCR，失败后回退本地 OCR
    #[serde(default)]
    pub mineru_ocr_prefer_online: bool,
    /// MinerU API Key
    #[serde(default)]
    pub mineru_api_key: String,
    /// 是否启用 AI 知识库服务器同步
    #[serde(default = "default_true")]
    pub knowledge_server_sync_enabled: bool,
    /// 局方本地更新完成后是否自动刷新并同步 AI 知识库
    #[serde(default = "default_true")]
    pub knowledge_auto_sync_after_regulation_update: bool,
    /// AI 知识库同步方式：api 或 ssh
    #[serde(default = "default_knowledge_sync_mode")]
    pub knowledge_sync_mode: String,
    /// AI 知识库 API 地址
    #[serde(default = "default_knowledge_api_url")]
    pub knowledge_api_url: String,
    /// AI 知识库 API Token
    #[serde(default)]
    pub knowledge_api_token: String,
    /// AI 知识库服务器地址
    #[serde(default = "default_knowledge_server_host")]
    pub knowledge_server_host: String,
    /// AI 知识库 SSH 端口
    #[serde(default = "default_knowledge_server_port")]
    pub knowledge_server_port: u16,
    /// AI 知识库 SSH 用户
    #[serde(default = "default_knowledge_server_user")]
    pub knowledge_server_user: String,
    /// AI 知识库 SSH 私钥路径
    #[serde(default = "default_knowledge_server_key_path")]
    pub knowledge_server_key_path: String,
    /// AI 知识库服务器发布目录
    #[serde(default = "default_knowledge_server_remote_dir")]
    pub knowledge_server_remote_dir: String,
}

fn default_proxy_type() -> String {
    "http".to_string()
}

fn default_proxy_port() -> u16 {
    8080
}

fn default_knowledge_server_host() -> String {
    "154.9.27.44".to_string()
}

fn default_knowledge_sync_mode() -> String {
    "api".to_string()
}

fn default_knowledge_api_url() -> String {
    "https://ccar-api.hudawang.cn".to_string()
}

fn default_knowledge_server_port() -> u16 {
    7668
}

fn default_knowledge_server_user() -> String {
    "root".to_string()
}

fn default_knowledge_server_key_path() -> String {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".ssh")
        .join("154.9.27.44_id_ed25519")
        .to_string_lossy()
        .to_string()
}

fn default_knowledge_server_remote_dir() -> String {
    "/www/wwwroot/ccar-knowledge-data".to_string()
}

impl Default for AdvancedConfig {
    fn default() -> Self {
        Self {
            proxy_enabled: false,
            proxy_type: "http".to_string(),
            proxy_host: String::new(),
            proxy_port: 8080,
            debug_logging: false,
            debug_log_path: String::new(),
            portable_mode: false,
            regulation_storage_path: String::new(),
            regulation_auto_sync_enabled: false,
            regulation_auto_sync_wifi_only: true,
            mineru_ocr_enabled: false,
            mineru_ocr_prefer_online: false,
            mineru_api_key: String::new(),
            knowledge_server_sync_enabled: true,
            knowledge_auto_sync_after_regulation_update: true,
            knowledge_sync_mode: default_knowledge_sync_mode(),
            knowledge_api_url: default_knowledge_api_url(),
            knowledge_api_token: String::new(),
            knowledge_server_host: default_knowledge_server_host(),
            knowledge_server_port: default_knowledge_server_port(),
            knowledge_server_user: default_knowledge_server_user(),
            knowledge_server_key_path: default_knowledge_server_key_path(),
            knowledge_server_remote_dir: default_knowledge_server_remote_dir(),
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            notification: NotificationConfig::default(),
            update: UpdateConfig::default(),
            advanced: AdvancedConfig::default(),
        }
    }
}

/// 加载配置
pub fn load_config<P: AsRef<Path>>(config_path: P) -> HuGeResult<AppConfig> {
    let path = config_path.as_ref();
    debug!("加载配置文件: {:?}", path);

    if !path.exists() {
        info!("配置文件不存在，使用默认配置: {:?}", path);
        let default_config = AppConfig::default();

        if let Ok(mut cache) = CONFIG_CACHE.write() {
            *cache = Some(default_config.clone());
        }

        return Ok(default_config);
    }

    let content = fs::read_to_string(path).map_err(|e| {
        error!("读取配置文件失败: {:?}, 错误: {}", path, e);
        HuGeError::ConfigError(format!("读取配置文件失败: {}", e))
    })?;

    let config: AppConfig = serde_json::from_str(&content).map_err(|e| {
        error!("解析配置文件失败: {:?}, 错误: {}", path, e);
        warn!("配置文件格式错误，将使用默认配置");
        HuGeError::ConfigError(format!("解析配置文件失败: {}", e))
    })?;

    info!("配置文件加载成功: {:?}", path);
    debug!("加载的配置: {:?}", config);

    if let Ok(mut cache) = CONFIG_CACHE.write() {
        *cache = Some(config.clone());
    }

    Ok(config)
}

/// 保存配置
pub fn save_config<P: AsRef<Path>>(config_path: P, config: &AppConfig) -> HuGeResult<()> {
    let path = config_path.as_ref();
    debug!("保存配置文件: {:?}", path);

    if let Some(parent) = path.parent() {
        if !parent.exists() {
            info!("创建配置目录: {:?}", parent);
            fs::create_dir_all(parent).map_err(|e| {
                error!("创建配置目录失败: {:?}, 错误: {}", parent, e);
                HuGeError::ConfigError(format!("创建配置目录失败: {}", e))
            })?;
        }
    }

    let content = serde_json::to_string_pretty(config).map_err(|e| {
        error!("序列化配置失败: {}", e);
        HuGeError::ConfigError(format!("序列化配置失败: {}", e))
    })?;

    fs::write(path, &content).map_err(|e| {
        error!("写入配置文件失败: {:?}, 错误: {}", path, e);
        HuGeError::ConfigError(format!("写入配置文件失败: {}", e))
    })?;

    info!("配置文件保存成功: {:?}", path);

    if let Ok(mut cache) = CONFIG_CACHE.write() {
        *cache = Some(config.clone());
    }

    Ok(())
}

/// 获取配置文件路径
pub fn get_config_path<R: Runtime>(app: &AppHandle<R>) -> HuGeResult<PathBuf> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| {
        error!("获取应用数据目录失败: {}", e);
        HuGeError::ConfigError(format!("获取应用数据目录失败: {}", e))
    })?;

    let config_path = app_data_dir.join(CONFIG_FILE_NAME);
    debug!("配置文件路径: {:?}", config_path);

    Ok(config_path)
}

/// 获取缓存的配置
pub fn get_cached_config() -> Option<AppConfig> {
    CONFIG_CACHE.read().ok().and_then(|cache| cache.clone())
}

/// 更新缓存的配置
pub fn update_cached_config(config: AppConfig) {
    if let Ok(mut cache) = CONFIG_CACHE.write() {
        *cache = Some(config);
    }
}

/// 初始化配置系统
pub fn init_config<R: Runtime>(app: &AppHandle<R>) -> HuGeResult<AppConfig> {
    info!("初始化配置系统...");

    let config_path = get_config_path(app)?;

    if let Some(parent) = config_path.parent() {
        if !parent.exists() {
            info!("创建配置目录: {:?}", parent);
            fs::create_dir_all(parent).map_err(|e| {
                error!("创建配置目录失败: {:?}, 错误: {}", parent, e);
                HuGeError::ConfigError(format!("创建配置目录失败: {}", e))
            })?;
        }
    }

    let config = load_config(&config_path)?;

    info!("配置系统初始化完成");
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_app_config_default() {
        let config = AppConfig::default();
        assert_eq!(config.general.language, "zh-CN");
        assert_eq!(config.general.theme, "system");
    }

    #[test]
    fn test_app_config_serialize() {
        let config = AppConfig::default();
        let json = serde_json::to_string_pretty(&config).unwrap();
        assert!(json.contains("zh-CN"));
    }

    #[test]
    fn test_load_config_file_not_exists() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("nonexistent.json");

        let config = load_config(&config_path).unwrap();
        assert_eq!(config.general.language, "zh-CN");
    }

    #[test]
    fn test_save_and_load_config() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("test_config.json");

        let mut config = AppConfig::default();
        config.general.theme = "dark".to_string();

        save_config(&config_path, &config).unwrap();

        assert!(config_path.exists());

        let loaded_config = load_config(&config_path).unwrap();
        assert_eq!(loaded_config.general.theme, "dark");
    }

    #[test]
    fn test_save_config_creates_directory() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("subdir").join("nested").join("config.json");

        assert!(!config_path.parent().unwrap().exists());

        let config = AppConfig::default();
        save_config(&config_path, &config).unwrap();

        assert!(config_path.exists());
        assert!(config_path.parent().unwrap().exists());
    }

    #[test]
    #[serial]
    fn test_config_cache() {
        if let Ok(mut cache) = CONFIG_CACHE.write() {
            *cache = None;
        }

        assert!(get_cached_config().is_none());

        let config = AppConfig::default();
        update_cached_config(config.clone());

        let cached = get_cached_config().unwrap();
        assert_eq!(cached.general.language, config.general.language);
    }

    #[test]
    #[serial]
    fn test_load_config_updates_cache() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("cache_test.json");

        if let Ok(mut cache) = CONFIG_CACHE.write() {
            *cache = None;
        }

        let mut config = AppConfig::default();
        config.general.theme = "dark".to_string();

        let content = serde_json::to_string_pretty(&config).unwrap();
        fs::write(&config_path, &content).unwrap();

        let loaded = load_config(&config_path).unwrap();
        assert_eq!(loaded.general.theme, "dark");

        let cached = get_cached_config().unwrap();
        assert_eq!(cached.general.theme, "dark");
    }

    #[test]
    fn test_notification_config_default() {
        let config = NotificationConfig::default();
        assert!(config.startup);
        assert!(config.software_update);
    }

    #[test]
    fn test_update_config_default() {
        let config = UpdateConfig::default();
        assert!(config.auto_check);
        assert_eq!(config.check_interval_hours, 24);
        assert!(!config.use_proxy);
        assert_eq!(config.proxy_url, "https://ghproxy.net/");
    }

    #[test]
    fn test_advanced_config_default() {
        let config = AdvancedConfig::default();
        assert!(!config.proxy_enabled);
        assert_eq!(config.proxy_type, "http");
        assert_eq!(config.proxy_port, 8080);
    }

    #[test]
    fn test_backward_compatibility_missing_new_sections() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("old_config.json");

        // 模拟旧版配置文件（只包含 general）
        let old_config_json = r##"{
            "general": {
                "language": "zh-CN",
                "theme": "dark",
                "autoStart": false,
                "minimizeToTray": true
            }
        }"##;

        fs::write(&config_path, old_config_json).unwrap();

        let loaded = load_config(&config_path).unwrap();

        assert_eq!(loaded.general.theme, "dark");
        assert!(loaded.notification.startup);
        assert!(loaded.update.auto_check);
        assert!(!loaded.advanced.proxy_enabled);
    }

    #[test]
    fn test_config_json_format() {
        let temp_dir = tempdir().unwrap();
        let config_path = temp_dir.path().join("format_test.json");

        let config = AppConfig::default();
        save_config(&config_path, &config).unwrap();

        let content = fs::read_to_string(&config_path).unwrap();

        assert!(content.contains('\n'));
        assert!(content.contains("  "));
        assert!(content.contains("\"general\""));
        assert!(content.contains("\"notification\""));
        assert!(content.contains("\"update\""));
        assert!(content.contains("\"advanced\""));
    }
}
