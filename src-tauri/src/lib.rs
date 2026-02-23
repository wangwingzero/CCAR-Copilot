//! 虎哥截图 Tauri 后端
//!
//! 本库提供虎哥截图的 Rust 后端功能，包括：
//! - 截图引擎：屏幕捕获和窗口检测
//! - 热键管理：全局热键注册和处理
//! - 窗口管理：覆盖窗口和钉图窗口
//! - Sidecar 管理：Python Sidecar 进程通信
//! - 数据库：历史记录和设置持久化
//! - OCR：后台 OCR 缓存管理
//!
//! # 模块结构
//!
//! ```text
//! hugescreenshot_tauri_lib
//! ├── screenshot/     # 截图引擎
//! │   ├── capture     # 屏幕捕获
//! │   ├── image_hash  # 图片哈希计算
//! │   └── window_detect # 窗口检测
//! ├── hotkey/         # 全局热键
//! │   └── manager     # 热键管理器
//! ├── window/         # 窗口管理
//! │   ├── overlay     # 覆盖窗口
//! │   └── pin         # 钉图窗口
//! ├── sidecar/        # Python Sidecar
//! │   ├── manager     # 进程管理
//! │   └── protocol    # 通信协议
//! ├── ocr/            # OCR 功能
//! │   └── background_cache  # 后台 OCR 缓存
//! ├── commands/       # Tauri 命令
//! ├── database/       # 数据持久化
//! │   ├── history     # 历史记录
//! │   └── settings    # 设置
//! └── error           # 错误类型
//! ```

// 模块声明
pub mod clipboard; // 剪贴板监听器
pub mod commands;
pub mod converter; // 文件转 Markdown（纯 Rust 实现）
pub mod crash_report; // 崩溃报告系统
pub mod database;
pub mod device; // Phase 1: 设备指纹（暂不集成命令）
pub mod error;
pub mod feature_gate; // 功能门控系统
pub mod file_search; // 文件搜索客户端（与索引服务通信）
pub mod hotkey;
pub mod license; // 许可证验证服务
pub mod logging; // 日志系统
pub mod mouse_highlight; // 鼠标高亮效果
pub mod ocr; // OCR 功能（后台缓存）
pub mod payment; // 虎皮椒支付服务
pub mod regulation; // 规章本地索引（Tantivy 全文搜索）
pub mod recording; // 录屏引擎（DXGI + FFmpeg）
pub mod screenshot;
pub mod sidecar;
pub mod state; // 应用全局状态
pub mod single_instance; // Phase 1: 单实例锁（暂不集成）
pub mod supabase; // Supabase API 客户端
pub mod tray; // 系统托盘
pub mod usage; // 使用量追踪服务
pub mod window;

// 重新导出常用类型
pub use error::{HuGeError, HuGeResult};

use tauri::{Emitter, Manager};
use tracing::{error, info, warn};
use tracing_appender::non_blocking::WorkerGuard;

use crate::crash_report::{setup_crash_handler, CrashReportConfig};
use crate::database::settings::{init_config, get_config_path, save_config, get_cached_config, AppConfig};
use crate::hotkey::{setup_hotkeys, HotkeyConfig};
use crate::logging::{setup_logging_with_config, cleanup_old_logs, LogConfig};
use crate::commands::auth_cmd::init_auth_state;
use crate::commands::device_cmd::init_device_state;
use crate::commands::feature_gate_cmd::init_feature_gate_state;
use crate::commands::history_cmd::HistoryState;
use crate::commands::license_cmd::init_license_state;
use crate::commands::payment_cmd::init_payment_state;
use crate::commands::usage_cmd::init_usage_state;
use crate::commands::mouse_highlight_cmd::MouseHighlightState;
// init_file_search_state 的缓存加载已移到后台线程，此处仅需 FileSearchState::new()
use crate::regulation::RegulationIndexState;

/// 运行 Tauri 应用
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 日志 guard 必须保持存活直到应用退出
    // 使用 Option 以便在无法初始化日志时仍能运行
    let _log_guard: Option<WorkerGuard>;

    // 尝试初始化日志系统
    // 日志统一存放到项目根目录的"日志/Rust版本日志"文件夹，方便管理
    let log_dir = std::path::PathBuf::from(r"D:\screenshot\日志\Rust版本日志");

    let log_config = LogConfig {
        log_dir: log_dir.clone(),
        // 开发模式使用 DEBUG 级别以便调试，生产模式使用 INFO
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
            
            // 清理旧日志文件
            match cleanup_old_logs(&log_dir, logging::LOG_RETENTION_DAYS) {
                Ok(count) if count > 0 => {
                    info!("清理了 {} 个过期日志文件", count);
                }
                Ok(_) => {}
                Err(e) => {
                    warn!("清理旧日志文件失败: {}", e);
                }
            }

            // 设置崩溃处理器
            // Requirement 20.4: 显示用户友好的错误对话框
            // Requirement 20.6: 生成包含系统信息的崩溃报告
            let crash_config = CrashReportConfig {
                report_dir: log_dir.clone(),
                app_name: "虎哥截图".to_string(),
                app_version: env!("CARGO_PKG_VERSION").to_string(),
                show_dialog: true,
            };
            setup_crash_handler(crash_config);
        }
        Err(e) => {
            // 日志初始化失败时，使用简单的控制台输出
            eprintln!("警告: 日志系统初始化失败: {}", e);
            _log_guard = None;
            
            // 回退到简单的控制台日志
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::from_default_env()
                        .add_directive(tracing::Level::INFO.into()),
                )
                .init();

            // 即使日志失败，也设置崩溃处理器
            let crash_config = CrashReportConfig {
                report_dir: log_dir.clone(),
                app_name: "虎哥截图".to_string(),
                app_version: env!("CARGO_PKG_VERSION").to_string(),
                show_dialog: true,
            };
            setup_crash_handler(crash_config);
        }
    }

    info!("虎哥截图启动中...");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_process::init())
        .setup(|app| {
            info!("Tauri 应用初始化中...");

            // 检查是否通过开机自启动以 --minimized 模式启动
            let start_minimized = std::env::args().any(|arg| arg == "--minimized");
            if start_minimized {
                info!("检测到 --minimized 参数，将静默启动到系统托盘");
            }

            // 立即设置主窗口图标（使用高清虎头图标）
            if let Some(window) = app.get_webview_window("main") {
                let icon_data = include_bytes!("../icons/window-icon.png");
                if let Ok(icon) = tauri::image::Image::from_bytes(icon_data) {
                    if let Err(e) = window.set_icon(icon) {
                        warn!("设置窗口图标失败: {}", e);
                    } else {
                        info!("窗口图标设置成功");
                    }
                }

                // 开机自启动时隐藏主窗口（静默启动到托盘）
                if start_minimized {
                    if let Err(e) = window.hide() {
                        warn!("隐藏主窗口失败: {}", e);
                    } else {
                        info!("主窗口已隐藏（静默启动模式）");
                    }
                }
            }

            // 初始化配置系统并加载配置
            #[cfg(desktop)]
            let hotkey_config = {
                match init_config(app.handle()) {
                    Ok(config) => {
                        info!("配置加载成功");
                        config.hotkeys
                    }
                    Err(e) => {
                        warn!("配置加载失败，使用默认配置: {}", e);
                        // 尝试创建默认配置文件
                        if let Ok(config_path) = get_config_path(app.handle()) {
                            let default_config = AppConfig::default();
                            if let Err(save_err) = save_config(&config_path, &default_config) {
                                warn!("保存默认配置失败: {}", save_err);
                            }
                        }
                        HotkeyConfig::default()
                    }
                }
            };

            // 根据配置设置主窗口主题（覆盖 tauri.conf.json 的默认值）
            #[cfg(desktop)]
            if let Some(window) = app.get_webview_window("main") {
                let theme = match get_cached_config() {
                    Some(config) => match config.general.theme.as_str() {
                        "light" => Some(tauri::Theme::Light),
                        "dark" => Some(tauri::Theme::Dark),
                        _ => None, // "system" 跟随系统
                    },
                    None => None,
                };
                if let Err(e) = window.set_theme(theme) {
                    warn!("设置主窗口主题失败: {}", e);
                }
            }

            // 注册全局热键
            #[cfg(desktop)]
            {
                if let Err(e) = setup_hotkeys(app.handle(), hotkey_config) {
                    error!("热键注册失败: {}", e);
                    // 热键注册失败不应该阻止应用启动
                    // 发送通知到前端让用户知道
                    warn!("应用将继续运行，但部分热键可能不可用");
                }
            }

            // 初始化应用全局状态（轻量级，立即执行）
            #[cfg(desktop)]
            {
                let app_state = state::AppState::new();
                app.manage(app_state);
                info!("应用全局状态初始化成功");
            }

            // 初始化历史记录状态（轻量级，立即执行）
            #[cfg(desktop)]
            {
                let history_state = HistoryState::default();
                app.manage(history_state);
                info!("历史记录状态初始化成功");
            }

            // 初始化剪贴板监听器
            #[cfg(desktop)]
            {
                let clipboard_watcher = clipboard::ClipboardWatcher::new();
                clipboard_watcher.start(app.handle().clone());
                app.manage(clipboard_watcher);
                info!("剪贴板监听器已启动");
            }

            // 初始化鼠标高亮状态（轻量级，立即执行）
            #[cfg(desktop)]
            {
                let mouse_highlight_state = MouseHighlightState::new();
                app.manage(mouse_highlight_state);
                info!("鼠标高亮服务初始化成功");
            }

            // 初始化文件搜索状态（轻量级 - 仅创建空状态，缓存加载延迟到后台线程）
            #[cfg(desktop)]
            {
                let file_search_state = commands::file_search_cmd::FileSearchState::new();
                app.manage(file_search_state);
                info!("文件搜索状态已注册（缓存将在后台加载）");
            }

            // 初始化规章索引状态（轻量级，立即执行）
            #[cfg(desktop)]
            {
                let regulation_index_state = RegulationIndexState::default();
                app.manage(regulation_index_state);

                // 初始化批量下载状态
                let batch_download_state = regulation::BatchDownloadState::default();
                app.manage(batch_download_state);

                info!("规章索引状态初始化成功");
            }

            // 初始化录屏引擎状态
            {
                let recording_state = commands::recording_cmd::RecordingEngineState::new();
                app.manage(recording_state);
                info!("录屏引擎状态初始化成功");
            }

            // 初始化系统托盘（必须在主线程）
            #[cfg(desktop)]
            {
                if let Err(e) = tray::setup_tray(app.handle()) {
                    error!("系统托盘初始化失败: {}", e);
                    // 托盘初始化失败不应该阻止应用启动
                    warn!("应用将继续运行，但托盘功能不可用");
                } else {
                    info!("系统托盘初始化成功");
                }
            }

            // 预加载覆盖窗口（性能优化：避免热键触发时的延迟）
            // 在后台线程中执行，不阻塞主窗口显示
            #[cfg(desktop)]
            {
                let preload_app_handle = app.handle().clone();
                std::thread::spawn(move || {
                    // 短暂延迟确保主窗口初始化完成，然后尽快预加载 overlay
                    std::thread::sleep(std::time::Duration::from_millis(200));
                    match window::overlay::preload_overlay_windows(&preload_app_handle) {
                        Ok(count) => {
                            info!("覆盖窗口预加载完成，预加载了 {} 个窗口", count);
                        }
                        Err(e) => {
                            warn!("覆盖窗口预加载失败: {}，将在首次使用时创建", e);
                        }
                    }
                });
            }

            // 将非关键服务的初始化移到后台线程（避免阻塞主线程导致窗口"未响应"）
            let app_handle = app.handle().clone();
            std::thread::spawn(move || {
                let start_time = std::time::Instant::now();

                // 文件搜索缓存加载（可能耗时较长，必须在后台执行）
                if let Some(file_search_state) = app_handle.try_state::<commands::file_search_cmd::FileSearchState>() {
                    let cache_loaded = file_search_state.indexer.load_cache();
                    if cache_loaded {
                        info!("文件搜索缓存加载成功（后台, {:.1}s）", start_time.elapsed().as_secs_f64());
                    } else {
                        info!("无缓存可用，启动全盘扫描...（后台）");
                    }
                    file_search_state.indexer.start_background_scan();
                }

                // 初始化认证状态
                match init_auth_state(&app_handle) {
                    Ok(auth_state) => {
                        app_handle.manage(auth_state);
                        info!("认证服务初始化成功（后台）");
                    }
                    Err(e) => {
                        error!("认证服务初始化失败: {}", e);
                    }
                }

                // 初始化许可证状态
                let license_state_opt = match init_license_state(&app_handle) {
                    Ok(license_state) => {
                        info!("许可证服务初始化成功（后台）");
                        Some(license_state)
                    }
                    Err(e) => {
                        error!("许可证服务初始化失败: {}", e);
                        None
                    }
                };

                // 初始化支付服务
                match init_payment_state() {
                    Ok(payment_state) => {
                        app_handle.manage(payment_state);
                        info!("支付服务初始化成功（后台）");
                    }
                    Err(e) => {
                        error!("支付服务初始化失败: {}", e);
                    }
                }

                // 初始化使用量追踪服务
                let usage_state_opt = match init_usage_state(&app_handle) {
                    Ok(usage_state) => {
                        info!("使用量追踪服务初始化成功（后台）");
                        Some(usage_state)
                    }
                    Err(e) => {
                        error!("使用量追踪服务初始化失败: {}", e);
                        None
                    }
                };

                // 初始化功能门控服务
                if let (Some(ref license_state), Some(ref usage_state)) = (&license_state_opt, &usage_state_opt) {
                    let feature_gate_state = init_feature_gate_state(license_state, usage_state);
                    app_handle.manage(feature_gate_state);
                    info!("功能门控服务初始化成功（后台）");
                } else {
                    warn!("功能门控服务初始化跳过（依赖服务未就绪）");
                }

                // 注册许可证和使用量状态到 Tauri
                if let Some(license_state) = license_state_opt {
                    app_handle.manage(license_state);
                }
                if let Some(usage_state) = usage_state_opt {
                    app_handle.manage(usage_state);
                }

                // 初始化设备管理状态
                match init_device_state() {
                    Ok(device_state) => {
                        app_handle.manage(device_state);
                        info!("设备管理服务初始化成功（后台）");
                    }
                    Err(e) => {
                        error!("设备管理服务初始化失败: {}", e);
                    }
                }

                // 通知前端后台初始化完成
                let elapsed = start_time.elapsed().as_secs_f64();
                info!("后台服务初始化完成（耗时 {:.2}s）", elapsed);
                let _ = app_handle.emit("app:ready", serde_json::json!({
                    "elapsed_ms": (elapsed * 1000.0) as u64,
                }));
            });

            // 历史记录数据库初始化（异步任务）
            // 在应用启动时初始化数据库，而不是在工作台窗口打开时
            #[cfg(desktop)]
            {
                let history_app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    // 稍微延迟，确保状态已注册
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

                    // 获取数据目录并初始化数据库
                    match history_app_handle.path().app_data_dir() {
                        Ok(data_dir) => {
                            // 确保目录存在
                            if !data_dir.exists() {
                                if let Err(e) = std::fs::create_dir_all(&data_dir) {
                                    error!("创建数据目录失败: {}", e);
                                    return;
                                }
                            }
                            let db_path = data_dir.join("history.db");
                            if let Some(state) = history_app_handle.try_state::<HistoryState>() {
                                match state.init(&db_path.to_string_lossy()).await {
                                    Ok(()) => {
                                        info!("历史记录数据库启动初始化成功: {:?}", db_path);
                                        
                                        // 启动后台 OCR 缓存服务
                                        let db_clone = state.db.clone();
                                        tauri::async_runtime::spawn(async move {
                                            // 延迟 2 秒后预热 WGC D3D11 设备，消除首次截图的 1.3s 延迟
                                            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                                            #[cfg(windows)]
                                            {
                                                // 在 blocking 线程中预热 D3D11 设备（需要 COM 环境）
                                                let _ = tokio::task::spawn_blocking(|| {
                                                    screenshot::pre_warm_d3d_devices();
                                                }).await;
                                            }

                                            // 延迟 3 秒启动 OCR 模型预热，避免影响应用启动
                                            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                                            
                                            // 预热 OCR 模型，避免首次使用时的冷启动延迟
                                            info!("开始预热 OCR 模型...");
                                            if let Err(e) = ocr::engine::OcrEngine::warmup().await {
                                                warn!("OCR 模型预热失败（不影响正常使用）: {}", e);
                                            }
                                            
                                            // 再延迟 2 秒启动后台缓存服务
                                            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                                            
                                            let ocr_cache = ocr::background_cache::BackgroundOcrCache::new();
                                            info!("后台 OCR 缓存服务启动中...");
                                            if let Err(e) = ocr_cache.start(db_clone).await {
                                                error!("后台 OCR 缓存服务启动失败: {}", e);
                                            }
                                        });
                                    }
                                    Err(e) => error!("历史记录数据库启动初始化失败: {}", e),
                                }
                            } else {
                                warn!("HistoryState 未注册，跳过数据库初始化");
                            }
                        }
                        Err(e) => {
                            error!("获取应用数据目录失败: {}", e);
                        }
                    }
                });
            }

            info!("Tauri 应用初始化完成");
            Ok(())
        })
        .on_window_event(|window, event| {
            // 处理窗口关闭事件
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // 如果是主窗口关闭，最小化到托盘而不是退出
                if window.label() == "main" {
                    info!("主窗口关闭请求，最小化到托盘");
                    // 阻止默认关闭行为
                    api.prevent_close();
                    // 隐藏窗口到托盘
                    tray::hide_to_tray(window.app_handle());
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            // 截图命令
            screenshot::capture::capture_screen,
            screenshot::capture::capture_all_monitors,
            screenshot::capture::get_screen_info,
            screenshot::window_detect::detect_window_at,
            screenshot::window_detect::get_all_windows,
            // 静态快照命令
            screenshot::snapshot::capture_static_snapshot,
            screenshot::snapshot::cleanup_snapshot,
            // 窗口命令
            window::get_monitors,
            window::overlay::create_overlay_window,
            window::overlay::create_all_overlay_windows,
            window::overlay::show_overlay_windows,
            window::overlay::hide_overlay_windows,
            window::overlay::is_overlay_preloaded,
            window::overlay::overlay_ready,
            window::overlay::overlay_force_focus,
            window::overlay::close_overlay_window,
            window::overlay::close_all_overlays,
            window::overlay::get_overlay_windows,
            window::overlay::set_overlay_ignore_cursor,
            window::pin::create_pin_window,
            window::pin::set_pin_opacity,
            window::pin::close_pin_window,
            window::pin::close_all_pin_windows,
            window::pin::get_pin_windows,
            window::pin::get_pin_window_init,
            // 热键命令
            commands::hotkey_cmd::get_hotkey_config,
            commands::hotkey_cmd::set_hotkey_config,
            commands::hotkey_cmd::check_hotkey_available,
            commands::hotkey_cmd::update_single_hotkey,
            // 截图扩展命令
            commands::screenshot_cmd::capture_region,
            commands::screenshot_cmd::capture_window,
            commands::screenshot_cmd::capture_screen_for_overlay,
            commands::screenshot_cmd::auto_save_screenshot,
            commands::screenshot_cmd::get_screenshot_save_config,
            commands::screenshot_cmd::save_screenshot_with_history,
            commands::screenshot_cmd::save_screenshot_with_history_from_file,
            commands::screenshot_cmd::save_temp_image,
            commands::screenshot_cmd::crop_and_save_temp,
            // 窗口扩展命令
            commands::window_cmd::start_capture_mode,
            commands::window_cmd::exit_capture_mode,
            commands::window_cmd::open_workbench_window,
            commands::window_cmd::open_anki_card_window,
            commands::window_cmd::open_ocr_result_window,
            commands::window_cmd::get_pending_ocr_result,
            commands::window_cmd::open_ocr_panel_no_focus,
            commands::window_cmd::close_ocr_result_window,
            commands::window_cmd::get_pending_anki_init,
            // Anki 原生命令（直接 HTTP 调用 AnkiConnect，不依赖 Sidecar）
            commands::anki_cmd::check_anki_connection,
            commands::anki_cmd::get_anki_decks,
            commands::anki_cmd::get_anki_models,
            commands::anki_cmd::extract_english_words_native,
            commands::anki_cmd::import_words_to_anki,
            commands::anki_cmd::ensure_anki_model,
            // 录屏命令（原生 Rust 实现）
            commands::recording_cmd::start_recording,
            commands::recording_cmd::stop_recording,
            commands::recording_cmd::pause_recording,
            commands::recording_cmd::resume_recording,
            commands::recording_cmd::get_recording_status,
            commands::recording_cmd::open_recording_control,
            commands::recording_cmd::close_recording_control,
            commands::recording_cmd::open_recording_preview,
            commands::recording_cmd::close_recording_preview,
            commands::recording_cmd::set_overlay_recording_mode,
            // 认证命令
            commands::auth_cmd::get_current_user,
            commands::auth_cmd::is_authenticated,
            commands::auth_cmd::load_saved_session,
            commands::auth_cmd::sign_in_with_password,
            commands::auth_cmd::sign_up,
            commands::auth_cmd::reset_password,
            commands::auth_cmd::sign_out,
            commands::auth_cmd::refresh_session,
            // 许可证命令
            commands::license_cmd::validate_license,
            commands::license_cmd::get_cached_license,
            commands::license_cmd::clear_license_cache,
            commands::license_cmd::is_vip_user,
            // 支付命令
            commands::payment_cmd::create_payment_order,
            commands::payment_cmd::query_payment_order,
            commands::payment_cmd::check_payment_status,
            // 使用量追踪命令
            commands::usage_cmd::get_usage_stats,
            commands::usage_cmd::check_usage,
            commands::usage_cmd::record_usage,
            commands::usage_cmd::sync_usage_to_cloud,
            // 功能门控命令
            commands::feature_gate_cmd::check_feature_access,
            commands::feature_gate_cmd::use_feature,
            commands::feature_gate_cmd::get_all_features,
            commands::feature_gate_cmd::check_features_batch,
            // 历史记录命令
            commands::history_cmd::init_history_database,
            commands::history_cmd::search_history,
            commands::history_cmd::add_history_item,
            commands::history_cmd::update_history_item,
            commands::history_cmd::delete_history_item,
            commands::history_cmd::delete_history_items,
            commands::history_cmd::toggle_pin_history_item,
            commands::history_cmd::clear_unpinned_history,
            commands::history_cmd::get_history_stats,
            commands::history_cmd::export_history_items,
            // 设备管理命令
            commands::device_cmd::get_current_device,
            commands::device_cmd::get_bound_devices,
            commands::device_cmd::bind_current_device,
            commands::device_cmd::unbind_device,
            commands::device_cmd::update_device_activity,
            // 托盘命令
            commands::tray_cmd::set_tray_state,
            commands::tray_cmd::show_main_window,
            commands::tray_cmd::hide_to_tray,
            commands::tray_cmd::get_tray_state,
            // 配置命令
            commands::config_cmd::load_config,
            commands::config_cmd::save_config,
            commands::config_cmd::export_config,
            commands::config_cmd::import_config,
            commands::config_cmd::set_auto_start,
            commands::config_cmd::check_auto_start,
            commands::config_cmd::update_hotkey,
            // 自动更新命令
            commands::update_cmd::check_for_update,
            commands::update_cmd::download_and_install_update,
            commands::update_cmd::restart_app,
            commands::update_cmd::get_current_version,
            commands::update_cmd::get_update_config,
            commands::update_cmd::set_update_config,
            // 崩溃报告命令
            crash_report::show_error_dialog,
            crash_report::get_crash_report_dir,
            // 自动更新命令
            commands::update_cmd::check_for_update,
            commands::update_cmd::download_and_install_update,
            commands::update_cmd::restart_app,
            commands::update_cmd::get_current_version,
            commands::update_cmd::get_update_config,
            commands::update_cmd::set_update_config,
            // 鼠标高亮命令
            commands::mouse_highlight_cmd::start_mouse_highlight,
            commands::mouse_highlight_cmd::stop_mouse_highlight,
            commands::mouse_highlight_cmd::get_mouse_highlight_status,
            commands::mouse_highlight_cmd::get_mouse_position,
            commands::mouse_highlight_cmd::get_mouse_highlight_config,
            commands::mouse_highlight_cmd::set_mouse_highlight_config,
            // 剪贴板命令（使用 arboard 绕过 Windows PATH_TOO_LONG 问题）
            commands::clipboard_cmd::copy_image_to_clipboard,
            commands::clipboard_cmd::copy_png_to_clipboard,
            commands::clipboard_cmd::copy_file_to_clipboard,
            commands::clipboard_cmd::pause_clipboard_watcher,
            commands::clipboard_cmd::resume_clipboard_watcher,
            // 定时关机命令
            commands::shutdown_cmd::schedule_shutdown,
            commands::shutdown_cmd::cancel_scheduled_shutdown,
            // 文件搜索命令
            commands::file_search_cmd::file_search,
            commands::file_search_cmd::get_search_service_status,
            commands::file_search_cmd::start_search_service,
            commands::file_search_cmd::rebuild_search_index,
            commands::file_search_cmd::update_search_config,
            commands::file_search_cmd::check_windows_service_status,
            commands::file_search_cmd::search_from_ocr,
            commands::file_search_cmd::get_available_drives,
            commands::file_search_cmd::is_file_search_service_installed,
            commands::file_search_cmd::install_file_search_service,
            commands::file_search_cmd::uninstall_file_search_service,
            commands::file_search_cmd::get_index_files_path,
            // 文件操作命令
            commands::file_cmd::save_text_file,
            commands::file_cmd::read_text_file,
            commands::file_cmd::file_exists,
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
            // 规章同步对比命令
            regulation::regulation_sync_compare,
            regulation::regulation_sync_compare_online,
            // 规章在线搜索命令（Rust 原生，替代 Python Sidecar）
            regulation::regulation_online_search,
            regulation::regulation_fetch_all_online,
            regulation::regulation_download_single,
            // 全盘自动发现规章 PDF
            regulation::regulation_discover_local,
            regulation::regulation_import_legacy_data,
            // 规章 OCR 处理命令
            regulation::regulation_ocr_pending,
            regulation::regulation_ocr_update,
            regulation::regulation_get_ocr_queue,
            regulation::regulation_retry_failed_ocr,
            // 文件转 Markdown 命令（纯 Rust 实现）
            commands::converter_cmd::convert_file_to_markdown,
            commands::converter_cmd::convert_files_to_markdown,
            commands::converter_cmd::detect_file_format,
            commands::converter_cmd::get_supported_formats,
        ])
        .run(tauri::generate_context!())
        .expect("运行 Tauri 应用时出错");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_types_exist() {
        // 验证错误类型可以正常创建
        let err = HuGeError::CaptureError("测试".to_string());
        assert!(err.to_string().contains("截图捕获失败"));
    }
}
