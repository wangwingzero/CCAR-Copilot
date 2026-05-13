//! 自动更新命令模块
//!
//! 本模块提供自动更新相关的 Tauri 命令,使用 `tauri-plugin-updater` 与
//! `ccar-update.031986.xyz` 上托管的 `latest.json` 交互。
//!
//! # 功能
//!
//! - 检查更新(支持用户代理前缀)
//! - 下载并安装更新,实时推送进度事件
//! - 查询/保存前端使用的自动更新配置
//! - 获取当前版本、重启应用
//!
//! # 事件
//!
//! 下载过程中向 webview 发送以下事件:
//!
//! - `update://download-started`  `{ totalSize: number | null }`
//! - `update://download-progress` `{ downloaded: number, total: number | null }`
//! - `update://download-finished` `{}`
//!
//! # Requirements
//!
//! - 19.1: 启动时和定期检查更新
//! - 19.2: 更新可用时通知用户并显示发布说明
//! - 19.3: 后台下载和安装更新
//! - 19.4: 更新失败时返回错误
//! - 19.6: 用户可以在设置中禁用自动更新

use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;
use tracing::{info, warn};

static UPDATE_DOWNLOAD_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

const UPDATE_MANIFEST_ENDPOINTS: [&str; 2] =
    ["https://ccar-update.031986.xyz/latest.json", "https://ccar-dl.hudawang.cn/latest.json"];
const WINDOWS_UPDATE_TARGET: &str = "windows-x86_64";

struct DownloadGuard<'a> {
    flag: &'a AtomicBool,
}

impl Drop for DownloadGuard<'_> {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::Release);
    }
}

fn try_acquire_download_guard(flag: &AtomicBool) -> Option<DownloadGuard<'_>> {
    flag.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
        .ok()
        .map(|_| DownloadGuard { flag })
}

/// 更新信息
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInfo {
    /// 新版本号
    pub version: String,
    /// 发布说明
    pub notes: Option<String>,
    /// 发布日期
    pub date: Option<String>,
    /// 下载大小（字节）
    pub download_size: Option<u64>,
    /// 安装包下载地址
    pub download_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UpdateManifest {
    platforms: HashMap<String, UpdateManifestPlatform>,
}

#[derive(Debug, Deserialize)]
struct UpdateManifestPlatform {
    url: String,
}

/// 更新状态
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status")]
pub enum UpdateStatus {
    /// 空闲状态
    Idle,
    /// 正在检查更新
    Checking,
    /// 有可用更新
    Available { info: UpdateInfo },
    /// 正在下载
    Downloading { progress: f64 },
    /// 下载完成，准备安装
    Ready { info: UpdateInfo },
    /// 正在安装
    Installing,
    /// 更新完成，需要重启
    PendingRestart,
    /// 没有可用更新
    UpToDate,
    /// 更新失败
    Error { message: String },
}

/// 自动更新配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateConfig {
    /// 是否启用自动更新
    pub auto_update_enabled: bool,
    /// 检查更新间隔（小时）
    pub check_interval_hours: u32,
    /// 是否在启动时检查更新
    pub check_on_startup: bool,
    /// 是否自动下载更新
    pub auto_download: bool,
    /// 是否自动安装更新
    pub auto_install: bool,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            auto_update_enabled: true,
            check_interval_hours: 24,
            check_on_startup: true,
            auto_download: true,
            auto_install: false, // 默认不自动安装，让用户确认
        }
    }
}

/// 根据当前配置构造一个 `Updater` 实例,在 `use_proxy = true` 且 `proxy_url`
/// 可解析时套上 HTTP 代理;否则使用默认的系统 TLS 直连。
///
/// 失败时返回字符串错误,便于直接返回给前端。
fn build_updater(app: &AppHandle) -> Result<tauri_plugin_updater::Updater, String> {
    use crate::database::settings::{get_config_path, load_config};

    let config_path = get_config_path(app).map_err(|e| e.to_string())?;
    let app_config = load_config(&config_path).map_err(|e| e.to_string())?;

    let mut builder = app.updater_builder();
    if app_config.update.use_proxy && !app_config.update.proxy_url.trim().is_empty() {
        match app_config.update.proxy_url.parse::<url::Url>() {
            Ok(url) => {
                info!("updater 使用代理: {}", url);
                builder = builder.proxy(url);
            }
            Err(e) => {
                warn!(
                    "代理 URL 无法解析,已忽略代理直接走默认通道: {} (err={})",
                    app_config.update.proxy_url, e
                );
            }
        }
    }

    builder.build().map_err(|e| format!("构造 updater 失败: {}", e))
}

fn normalize_installer_url(raw: &str) -> Result<String, String> {
    let candidate = raw.trim().replace(' ', "%20");
    let parsed =
        url::Url::parse(&candidate).map_err(|e| format!("安装包 URL 无效: {} ({})", raw, e))?;

    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!("安装包 URL 协议不支持: {}", raw));
    }

    if !parsed.path().to_ascii_lowercase().ends_with(".exe") {
        return Err(format!("安装包 URL 不是 exe 文件: {}", raw));
    }

    Ok(parsed.to_string())
}

fn extract_windows_download_url_from_manifest(manifest: &str) -> Result<String, String> {
    let manifest: UpdateManifest =
        serde_json::from_str(manifest).map_err(|e| format!("解析更新清单失败: {}", e))?;

    if let Some(platform) = manifest.platforms.get(WINDOWS_UPDATE_TARGET) {
        return normalize_installer_url(&platform.url);
    }

    manifest
        .platforms
        .values()
        .find_map(|platform| normalize_installer_url(&platform.url).ok())
        .ok_or_else(|| "更新清单中没有 Windows exe 安装包地址".to_string())
}

fn build_manifest_http_client(app: &AppHandle) -> Result<reqwest::Client, String> {
    use crate::database::settings::{get_config_path, load_config};

    let config_path = get_config_path(app).map_err(|e| e.to_string())?;
    let app_config = load_config(&config_path).map_err(|e| e.to_string())?;

    let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(20));
    if app_config.update.use_proxy && !app_config.update.proxy_url.trim().is_empty() {
        let proxy = reqwest::Proxy::all(app_config.update.proxy_url.trim())
            .map_err(|e| format!("代理 URL 无法解析: {}", e))?;
        builder = builder.proxy(proxy);
    }

    builder.build().map_err(|e| format!("构造更新清单 HTTP 客户端失败: {}", e))
}

/// 检查更新
///
/// # Requirements
/// - 19.1: 检查更新
/// - 19.2: 返回更新信息和发布说明
#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> Result<UpdateStatus, String> {
    info!("检查更新...");

    let updater = build_updater(&app)?;
    match updater.check().await {
        Ok(Some(update)) => {
            info!("发现新版本 {} (当前 {})", update.version, update.current_version);
            Ok(UpdateStatus::Available {
                info: UpdateInfo {
                    version: update.version.clone(),
                    notes: update.body.clone(),
                    date: update.date.map(|d| d.to_string()),
                    download_size: None,
                    download_url: Some(update.download_url.to_string()),
                },
            })
        }
        Ok(None) => {
            info!("已是最新版本");
            Ok(UpdateStatus::UpToDate)
        }
        Err(e) => {
            warn!("检查更新失败: {}", e);
            Err(format!("检查更新失败: {}", e))
        }
    }
}

/// 下载并安装更新
///
/// 下载过程中会向前端推送 `update://download-*` 事件,安装完成后返回
/// `PendingRestart`,由用户点击确认后调用 `restart_app` 重启完成升级。
///
/// # Requirements
/// - 19.3: 后台下载和安装更新
#[tauri::command]
pub async fn download_and_install_update(app: AppHandle) -> Result<UpdateStatus, String> {
    info!("下载并安装更新...");

    let _download_guard = try_acquire_download_guard(&UPDATE_DOWNLOAD_IN_PROGRESS)
        .ok_or_else(|| "已有更新下载任务正在进行".to_string())?;

    let updater = build_updater(&app)?;
    let update = updater
        .check()
        .await
        .map_err(|e| format!("检查更新失败: {}", e))?
        .ok_or_else(|| "没有可用更新".to_string())?;

    let app_for_progress = app.clone();
    let app_for_finish = app.clone();
    let mut emitted_start = false;
    let mut downloaded: u64 = 0;

    update
        .download_and_install(
            move |chunk_length, content_length| {
                downloaded = downloaded.saturating_add(chunk_length as u64);
                if !emitted_start {
                    emitted_start = true;
                    let _ = app_for_progress.emit(
                        "update://download-started",
                        serde_json::json!({ "totalSize": content_length }),
                    );
                }
                let _ = app_for_progress.emit(
                    "update://download-progress",
                    serde_json::json!({
                        "downloaded": downloaded,
                        "total": content_length,
                    }),
                );
            },
            move || {
                info!("更新包下载完成,开始安装...");
                let _ = app_for_finish.emit("update://download-finished", ());
            },
        )
        .await
        .map_err(|e| {
            warn!("下载或安装更新失败: {}", e);
            format!("下载或安装更新失败: {}", e)
        })?;

    info!("更新安装完成,等待重启");
    Ok(UpdateStatus::PendingRestart)
}

/// 重启应用以完成更新
///
/// # Requirements
/// - 19.3: 完成更新安装
#[tauri::command]
pub async fn restart_app() -> Result<(), String> {
    info!("重启应用以完成更新...");

    // 使用 tauri 的退出功能，应用将在退出后由系统重新启动（如果配置了自动重启）
    // 或者用户可以手动重新启动应用
    tauri::async_runtime::spawn(async {
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        std::process::exit(0);
    });

    Ok(())
}

/// 获取当前版本
#[tauri::command]
pub fn get_current_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// 获取更新配置
///
/// # Requirements
/// - 19.6: 用户可以在设置中禁用自动更新
#[tauri::command]
pub async fn get_update_config(app: AppHandle) -> Result<UpdateConfig, String> {
    use crate::database::settings::{get_config_path, load_config};

    let config_path = get_config_path(&app).map_err(|e| e.to_string())?;
    let app_config = load_config(&config_path).map_err(|e| e.to_string())?;

    // 从 AppConfig 中提取更新配置
    Ok(UpdateConfig {
        auto_update_enabled: app_config.general.auto_update_enabled.unwrap_or(true),
        check_interval_hours: app_config.general.update_check_interval_hours.unwrap_or(24),
        check_on_startup: app_config.general.check_update_on_startup.unwrap_or(true),
        auto_download: app_config.general.auto_download_update.unwrap_or(true),
        auto_install: app_config.general.auto_install_update.unwrap_or(false),
    })
}

/// 设置更新配置
///
/// # Requirements
/// - 19.6: 用户可以在设置中禁用自动更新
#[tauri::command]
pub async fn set_update_config(app: AppHandle, config: UpdateConfig) -> Result<(), String> {
    use crate::database::settings::{get_config_path, load_config, save_config};

    let config_path = get_config_path(&app).map_err(|e| e.to_string())?;
    let mut app_config = load_config(&config_path).map_err(|e| e.to_string())?;

    // 更新 AppConfig 中的更新配置
    app_config.general.auto_update_enabled = Some(config.auto_update_enabled);
    app_config.general.update_check_interval_hours = Some(config.check_interval_hours);
    app_config.general.check_update_on_startup = Some(config.check_on_startup);
    app_config.general.auto_download_update = Some(config.auto_download);
    app_config.general.auto_install_update = Some(config.auto_install);

    save_config(&config_path, &app_config).map_err(|e| e.to_string())?;

    info!("更新配置已保存: {:?}", config);
    Ok(())
}

/// 获取最新版 Windows 安装包下载地址,供用户手动打开浏览器更新。
#[tauri::command]
pub async fn get_latest_update_download_url(app: AppHandle) -> Result<String, String> {
    info!("获取最新版安装包地址...");

    let client = build_manifest_http_client(&app)?;
    let mut last_error: Option<String> = None;

    for endpoint in UPDATE_MANIFEST_ENDPOINTS {
        let result = async {
            let response = client
                .get(endpoint)
                .send()
                .await
                .map_err(|e| format!("请求更新清单失败: {}", e))?;

            if !response.status().is_success() {
                return Err(format!("更新清单返回 HTTP {}", response.status()));
            }

            let body = response.text().await.map_err(|e| format!("读取更新清单失败: {}", e))?;

            extract_windows_download_url_from_manifest(&body)
        }
        .await;

        match result {
            Ok(url) => return Ok(url),
            Err(e) => {
                warn!("从 {} 获取安装包地址失败: {}", endpoint, e);
                last_error = Some(format!("{}: {}", endpoint, e));
            }
        }
    }

    Err(format!(
        "获取最新版安装包地址失败: {}",
        last_error.unwrap_or_else(|| "所有更新通道均不可用".to_string())
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_config_default() {
        let config = UpdateConfig::default();
        assert!(config.auto_update_enabled);
        assert_eq!(config.check_interval_hours, 24);
        assert!(config.check_on_startup);
        assert!(config.auto_download);
        assert!(!config.auto_install);
    }

    #[test]
    fn test_get_current_version() {
        let version = get_current_version();
        assert!(!version.is_empty());
    }

    #[test]
    fn test_update_status_serialization() {
        let status = UpdateStatus::Available {
            info: UpdateInfo {
                version: "1.0.0".to_string(),
                notes: Some("Bug fixes".to_string()),
                date: Some("2024-01-01".to_string()),
                download_size: Some(1024),
                download_url: Some(
                    "https://ccar-update.031986.xyz/downloads/CCAR%20Copilot_1.0.0_x64-setup.exe"
                        .to_string(),
                ),
            },
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("Available"));
        assert!(json.contains("1.0.0"));
        assert!(json.contains("setup.exe"));
    }

    #[test]
    fn test_extract_windows_download_url_from_manifest() {
        let manifest = r#"{
            "version": "1.0.0",
            "notes": "Bug fixes",
            "pub_date": "2026-05-13T00:00:00Z",
            "platforms": {
                "windows-x86_64": {
                    "signature": "abc",
                    "url": "https://ccar-update.031986.xyz/downloads/CCAR%20Copilot_1.0.0_x64-setup.exe"
                }
            }
        }"#;

        let url = extract_windows_download_url_from_manifest(manifest).unwrap();

        assert_eq!(
            url,
            "https://ccar-update.031986.xyz/downloads/CCAR%20Copilot_1.0.0_x64-setup.exe"
        );
    }

    #[test]
    fn test_extract_windows_download_url_normalizes_spaces() {
        let manifest = r#"{
            "version": "1.0.0",
            "platforms": {
                "windows-x86_64": {
                    "signature": "abc",
                    "url": "https://ccar-update.031986.xyz/downloads/CCAR Copilot_1.0.0_x64-setup.exe"
                }
            }
        }"#;

        let url = extract_windows_download_url_from_manifest(manifest).unwrap();

        assert_eq!(
            url,
            "https://ccar-update.031986.xyz/downloads/CCAR%20Copilot_1.0.0_x64-setup.exe"
        );
    }

    #[test]
    fn test_download_gate_rejects_concurrent_downloads_until_guard_drops() {
        let flag = std::sync::atomic::AtomicBool::new(false);
        let first = try_acquire_download_guard(&flag).expect("first download should acquire gate");

        assert!(try_acquire_download_guard(&flag).is_none());

        drop(first);

        assert!(try_acquire_download_guard(&flag).is_some());
    }
}
