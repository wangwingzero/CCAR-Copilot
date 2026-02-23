//! 数据库模块
//!
//! 本模块负责数据持久化，包括：
//! - 历史记录存储（SQLite）
//! - 应用配置存储
//! - 规章文件存储（用于全文搜索）
//!
//! # 子模块
//!
//! - `history`: 历史记录数据库
//! - `settings`: 设置持久化
//! - `regulation`: 规章文件数据库

pub mod history;
pub mod regulation;
pub mod settings;

// 重新导出常用类型
pub use history::{
    HistoryDatabase, HistoryStats, PoolStatus, ScreenshotRecord, ScreenshotRecordUpdate,
    SearchParams, SearchResult,
};
pub use regulation::{
    RegulationFile, SyncStatus, OcrProgress,
    init_regulation_schema, file_exists_by_hash, url_exists, get_file_by_url,
    insert_file, update_ocr_status, mark_indexed, get_pending_ocr_files,
    get_sync_status, get_unindexed_files, update_page_count, reset_failed_ocr_files,
};
pub use settings::{
    AppConfig, get_cached_config, get_config_path, init_config, load_config,
    load_hotkey_config, save_config, save_hotkey_config, update_cached_config,
};
