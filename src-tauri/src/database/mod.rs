//! 数据库模块
//!
//! 本模块负责数据持久化，包括：
//! - 应用配置存储
//! - 规章文件存储（用于全文搜索）
//!
//! # 子模块
//!
//! - `settings`: 设置持久化
//! - `regulation`: 规章文件数据库

pub mod regulation;
pub mod settings;

// 重新导出常用类型
pub use regulation::{
    delete_files_by_ids, file_exists_by_hash, get_file_by_url, get_pending_ocr_files,
    get_sync_status, get_unindexed_files, init_regulation_schema, insert_file, mark_indexed,
    reset_failed_ocr_files, update_ocr_status, update_page_count, url_exists, OcrProgress,
    RegulationFile, SyncStatus,
};
pub use settings::{
    get_cached_config, get_config_path, init_config, load_config, save_config,
    update_cached_config, AppConfig,
};
