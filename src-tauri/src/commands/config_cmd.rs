//! 配置命令模块
//!
//! 提供应用配置的加载、保存、导入、导出功能。
//! 以及 Windows 自动启动设置。

use std::fs;
use std::path::Path;
use tauri::AppHandle;
use tracing::{debug, error, info, warn};

use crate::database::settings::{
    get_config_path, load_config as db_load_config, save_config as db_save_config, AppConfig,
};
use crate::error::{HuGeError, HuGeResult};

/// 加载应用配置
#[tauri::command]
pub async fn load_config(app: AppHandle) -> HuGeResult<AppConfig> {
    info!("加载应用配置...");

    let config_path = get_config_path(&app)?;
    let config = db_load_config(&config_path)?;

    debug!("配置加载成功: {:?}", config);
    Ok(config)
}

/// 保存应用配置
#[tauri::command]
pub async fn save_config(app: AppHandle, config: AppConfig) -> HuGeResult<()> {
    info!("保存应用配置...");

    let config_path = get_config_path(&app)?;
    db_save_config(&config_path, &config)?;

    info!("配置保存成功");
    Ok(())
}

/// 导出配置到指定文件
#[tauri::command]
pub async fn export_config(file_path: String, config: AppConfig) -> HuGeResult<()> {
    info!("导出配置到: {}", file_path);

    let path = Path::new(&file_path);

    if let Some(parent) = path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent).map_err(|e| {
                error!("创建导出目录失败: {}", e);
                HuGeError::ConfigError(format!("创建导出目录失败: {}", e))
            })?;
        }
    }

    let content = serde_json::to_string_pretty(&config).map_err(|e| {
        error!("序列化配置失败: {}", e);
        HuGeError::ConfigError(format!("序列化配置失败: {}", e))
    })?;

    fs::write(path, &content).map_err(|e| {
        error!("写入导出文件失败: {}", e);
        HuGeError::ConfigError(format!("写入导出文件失败: {}", e))
    })?;

    info!("配置导出成功");
    Ok(())
}

/// 从文件导入配置
#[tauri::command]
pub async fn import_config(file_path: String) -> HuGeResult<AppConfig> {
    info!("从文件导入配置: {}", file_path);

    let path = Path::new(&file_path);

    if !path.exists() {
        error!("导入文件不存在: {}", file_path);
        return Err(HuGeError::ConfigError(format!(
            "导入文件不存在: {}",
            file_path
        )));
    }

    let content = fs::read_to_string(path).map_err(|e| {
        error!("读取导入文件失败: {}", e);
        HuGeError::ConfigError(format!("读取导入文件失败: {}", e))
    })?;

    let config: AppConfig = serde_json::from_str(&content).map_err(|e| {
        error!("解析导入文件失败: {}", e);
        HuGeError::ConfigError(format!("配置文件格式错误: {}", e))
    })?;

    info!("配置导入成功");
    Ok(config)
}

/// 设置 Windows 自动启动
#[tauri::command]
pub async fn set_auto_start(_app: AppHandle, enabled: bool) -> HuGeResult<()> {
    info!("设置自动启动: {}", enabled);

    #[cfg(windows)]
    {
        use winreg::enums::*;
        use winreg::RegKey;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let run_key = hkcu
            .open_subkey_with_flags(
                r"Software\Microsoft\Windows\CurrentVersion\Run",
                KEY_READ | KEY_WRITE,
            )
            .map_err(|e| {
                error!("打开注册表失败: {}", e);
                HuGeError::ConfigError(format!("打开注册表失败: {}", e))
            })?;

        let app_name = "CCARCopilot";

        if enabled {
            let exe_path = std::env::current_exe().map_err(|e| {
                error!("获取可执行文件路径失败: {}", e);
                HuGeError::ConfigError(format!("获取可执行文件路径失败: {}", e))
            })?;

            let exe_path_str = exe_path.to_string_lossy().to_string();

            let value = if exe_path_str.contains(' ') {
                format!("\"{}\" --minimized", exe_path_str)
            } else {
                format!("{} --minimized", exe_path_str)
            };

            run_key.set_value(app_name, &value).map_err(|e| {
                error!("设置注册表值失败: {}", e);
                HuGeError::ConfigError(format!("设置自动启动失败: {}", e))
            })?;

            info!("自动启动已启用（静默模式）: {}", value);
        } else {
            match run_key.delete_value(app_name) {
                Ok(_) => info!("自动启动已禁用"),
                Err(e) => {
                    if e.kind() != std::io::ErrorKind::NotFound {
                        warn!("删除注册表值失败（可能不存在）: {}", e);
                    }
                }
            }
        }

        Ok(())
    }

    #[cfg(not(windows))]
    {
        warn!("自动启动功能仅支持 Windows 平台");
        Err(HuGeError::ConfigError(
            "自动启动功能仅支持 Windows 平台".to_string(),
        ))
    }
}

/// 检查自动启动状态
#[tauri::command]
pub async fn check_auto_start() -> HuGeResult<bool> {
    debug!("检查自动启动状态...");

    #[cfg(windows)]
    {
        use winreg::enums::*;
        use winreg::RegKey;

        let hkcu = RegKey::predef(HKEY_CURRENT_USER);

        match hkcu.open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Run") {
            Ok(run_key) => {
                let app_name = "CCARCopilot";
                match run_key.get_value::<String, _>(app_name) {
                    Ok(value) => {
                        debug!("自动启动已启用: {}", value);
                        Ok(true)
                    }
                    Err(_) => {
                        debug!("自动启动未启用");
                        Ok(false)
                    }
                }
            }
            Err(e) => {
                warn!("打开注册表失败: {}", e);
                Ok(false)
            }
        }
    }

    #[cfg(not(windows))]
    {
        warn!("自动启动功能仅支持 Windows 平台");
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_export_import_config() {
        let temp_dir = tempdir().unwrap();
        let export_path = temp_dir.path().join("exported_config.json");

        let mut config = AppConfig::default();
        config.general.language = "en-US".to_string();

        let content = serde_json::to_string_pretty(&config).unwrap();
        fs::write(&export_path, &content).unwrap();

        let imported_content = fs::read_to_string(&export_path).unwrap();
        let imported_config: AppConfig = serde_json::from_str(&imported_content).unwrap();

        assert_eq!(imported_config.general.language, "en-US");
    }

    #[cfg(windows)]
    #[test]
    fn test_auto_start_registry_path() {
        let path = r"Software\Microsoft\Windows\CurrentVersion\Run";
        assert!(path.contains("Run"));
    }
}
