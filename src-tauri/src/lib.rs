//! CCAR Copilot Tauri 后端
//!
//! 本库提供 CCAR Copilot 的 Rust 后端功能，包括：
//! - 规章全文搜索：Tantivy + jieba 中文分词
//! - OCR：OpenVINO PP-OCRv4（用于图片型 PDF 文本提取）
//! - 数据库：规章数据和设置持久化
//!
//! # 模块结构
//!
//! ```text
//! ccar_copilot_lib
//! ├── commands/       # Tauri 命令
//! ├── crash_report/   # 崩溃报告
//! ├── database/       # 数据持久化
//! │   ├── regulation  # 规章数据库
//! │   └── settings    # 设置
//! ├── error           # 错误类型
//! ├── logging/        # 日志系统
//! ├── ocr/            # OCR 功能
//! ├── regulation/     # 规章索引与搜索
//! └── tray/           # 系统托盘
//! ```

// 模块声明
pub mod commands;
pub mod crash_report;
pub mod database;
pub mod error;
pub mod logging;
pub mod ocr;
pub mod regulation;
pub mod tray;

// 重新导出常用类型
pub use error::{HuGeError, HuGeResult};

use tauri::Manager;
use tracing::{error, info, warn};
use tracing_appender::non_blocking::WorkerGuard;

use crate::crash_report::{setup_crash_handler, CrashReportConfig};
use crate::database::settings::{init_config, get_config_path, save_config, get_cached_config, AppConfig};
use crate::logging::{setup_logging_with_config, cleanup_old_logs, LogConfig};
use crate::regulation::RegulationIndexState;

/// 运行 Tauri 应用
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 日志 guard 必须保持存活直到应用退出
    let _log_guard: Option<WorkerGuard>;

    // 日志目录：使用临时目录作为回退，实际路径在 setup 中通过 app_data_dir 确定
    // 这里先用一个合理的默认路径初始化日志
    let log_dir = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("com.wangh.ccarcopilot")
        .join("logs");

    let log_config = LogConfig {
        log_dir: log_dir.clone(),
        level: if cfg!(debug_assertions) {
            tracing::Level::DEBUG
        } else {
            tracing::Level::INFO
        },
        retention_days: logging::LOG_RETENTION_DAYS,
        console_output: cfg!(debug_assertions),
    };

    match setup_logging_with_config(&log_config) {
        Ok(guard) => {
            _log_guard = Some(guard);
            info!("日志系统初始化成功，日志目录: {:?}", log_dir);

            match cleanup_old_logs(&log_dir, logging::LOG_RETENTION_DAYS) {
                Ok(count) if count > 0 => {
                    info!("清理了 {} 个过期日志文件", count);
                }
                Ok(_) => {}
                Err(e) => {
                    warn!("清理旧日志文件失败: {}", e);
                }
            }

            let crash_config = CrashReportConfig {
                report_dir: log_dir.clone(),
                app_name: "CCAR Copilot".to_string(),
                app_version: env!("CARGO_PKG_VERSION").to_string(),
                show_dialog: true,
            };
            setup_crash_handler(crash_config);
        }
        Err(e) => {
            eprintln!("警告: 日志系统初始化失败: {}", e);
            _log_guard = None;

            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::from_default_env()
                        .add_directive(tracing::Level::INFO.into()),
                )
                .init();

            let crash_config = CrashReportConfig {
                report_dir: log_dir.clone(),
                app_name: "CCAR Copilot".to_string(),
                app_version: env!("CARGO_PKG_VERSION").to_string(),
                show_dialog: true,
            };
            setup_crash_handler(crash_config);
        }
    }

    info!("CCAR Copilot 启动中...");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            info!("Tauri 应用初始化中...");

            // 检查是否通过开机自启动以 --minimized 模式启动
            let start_minimized = std::env::args().any(|arg| arg == "--minimized");
            if start_minimized {
                info!("检测到 --minimized 参数，将静默启动到系统托盘");
            }

            // 设置主窗口图标
            if let Some(window) = app.get_webview_window("main") {
                let icon_data = include_bytes!("../icons/window-icon.png");
                if let Ok(icon) = tauri::image::Image::from_bytes(icon_data) {
                    if let Err(e) = window.set_icon(icon) {
                        warn!("设置窗口图标失败: {}", e);
                    } else {
                        info!("窗口图标设置成功");
                    }
                }

                // 开机自启动时隐藏主窗口
                if start_minimized {
                    if let Err(e) = window.hide() {
                        warn!("隐藏主窗口失败: {}", e);
                    } else {
                        info!("主窗口已隐藏（静默启动模式）");
                    }
                }
            }

            // 初始化配置系统
            #[cfg(desktop)]
            {
                match init_config(app.handle()) {
                    Ok(_config) => {
                        info!("配置加载成功");
                    }
                    Err(e) => {
                        warn!("配置加载失败，使用默认配置: {}", e);
                        if let Ok(config_path) = get_config_path(app.handle()) {
                            let default_config = AppConfig::default();
                            if let Err(save_err) = save_config(&config_path, &default_config) {
                                warn!("保存默认配置失败: {}", save_err);
                            }
                        }
                    }
                }
            }

            // 根据配置设置主窗口主题
            #[cfg(desktop)]
            if let Some(window) = app.get_webview_window("main") {
                let theme = match get_cached_config() {
                    Some(config) => match config.general.theme.as_str() {
                        "light" => Some(tauri::Theme::Light),
                        "dark" => Some(tauri::Theme::Dark),
                        _ => None,
                    },
                    None => None,
                };
                if let Err(e) = window.set_theme(theme) {
                    warn!("设置主窗口主题失败: {}", e);
                }
            }

            // 初始化规章索引状态
            #[cfg(desktop)]
            {
                let regulation_index_state = RegulationIndexState::default();
                app.manage(regulation_index_state);

                let batch_download_state = regulation::BatchDownloadState::default();
                app.manage(batch_download_state);

                info!("规章索引状态初始化成功");
            }

            // 初始化系统托盘
            #[cfg(desktop)]
            {
                if let Err(e) = tray::setup_tray(app.handle()) {
                    error!("系统托盘初始化失败: {}", e);
                    warn!("应用将继续运行，但托盘功能不可用");
                } else {
                    info!("系统托盘初始化成功");
                }
            }

            // 后台预热 OCR 模型
            {
                tauri::async_runtime::spawn(async {
                    // 延迟 3 秒启动预热，避免影响应用启动
                    tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;

                    info!("开始预热 OCR 模型...");
                    if let Err(e) = ocr::engine::OcrEngine::warmup().await {
                        warn!("OCR 模型预热失败（不影响正常使用）: {}", e);
                    }
                });
            }

            info!("Tauri 应用初始化完成");
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    info!("主窗口关闭请求，最小化到托盘");
                    api.prevent_close();
                    tray::hide_to_tray(window.app_handle());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            // 配置命令
            commands::config_cmd::load_config,
            commands::config_cmd::save_config,
            commands::config_cmd::export_config,
            commands::config_cmd::import_config,
            commands::config_cmd::set_auto_start,
            commands::config_cmd::check_auto_start,
            // 自动更新命令
            commands::update_cmd::check_for_update,
            commands::update_cmd::download_and_install_update,
            commands::update_cmd::restart_app,
            commands::update_cmd::get_current_version,
            commands::update_cmd::get_update_config,
            commands::update_cmd::set_update_config,
            // 托盘命令
            commands::tray_cmd::set_tray_state,
            commands::tray_cmd::show_main_window,
            commands::tray_cmd::hide_to_tray,
            commands::tray_cmd::get_tray_state,
            // 文件操作命令
            commands::file_cmd::save_text_file,
            commands::file_cmd::read_text_file,
            commands::file_cmd::file_exists,
            // 崩溃报告命令
            crash_report::show_error_dialog,
            crash_report::get_crash_report_dir,
            // 规章索引命令
            regulation::regulation_index_init,
            regulation::regulation_local_search,
            regulation::regulation_index_add,
            regulation::regulation_index_add_batch,
            regulation::regulation_index_stats,
            regulation::regulation_index_clear,
            regulation::regulation_index_exists,
            // 规章批量下载命令
            regulation::regulation_batch_download,
            regulation::regulation_get_download_progress,
            regulation::regulation_get_sync_status,
            regulation::regulation_process_pending,
            // 规章本地扫描命令
            regulation::regulation_scan_local_dir,
            regulation::regulation_scan_all_drives,
            // 规章同步对比命令
            regulation::regulation_sync_compare,
            regulation::regulation_sync_compare_online,
            // 规章在线搜索命令
            regulation::regulation_online_search,
            regulation::regulation_fetch_all_online,
            regulation::regulation_download_single,
            // 规章旧数据迁移
            regulation::regulation_import_legacy_data,
            // 规章 OCR 处理命令
            regulation::regulation_ocr_pending,
            regulation::regulation_ocr_update,
            regulation::regulation_get_ocr_queue,
            regulation::regulation_retry_failed_ocr,
        ])
        .run(tauri::generate_context!())
        .expect("运行 Tauri 应用时出错");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_types_exist() {
        let err = HuGeError::Internal("测试".to_string());
        assert!(err.to_string().contains("内部错误"));
    }
}
