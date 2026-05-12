//! 文件搜索相关的 Tauri 命令
//!
//! 使用内置文件索引器提供全盘文件名快速搜索。

use serde::Serialize;
use std::sync::Arc;
use tauri::State;
use tracing::{debug, info};

use crate::file_search::indexer::{FileIndexer, IndexerStatus};

// =============================================================================
// State Management
// =============================================================================

/// 文件搜索状态（全局共享）
pub struct FileSearchState {
    /// 内置文件索引器
    pub indexer: Arc<FileIndexer>,
}

impl FileSearchState {
    /// 创建新的文件搜索状态
    pub fn new() -> Self {
        Self { indexer: Arc::new(FileIndexer::new()) }
    }
}

impl Default for FileSearchState {
    fn default() -> Self {
        Self::new()
    }
}

/// 初始化文件搜索状态
///
/// 在应用启动时调用，先加载磁盘缓存，然后启动后台扫描更新索引。
pub fn init_file_search_state() -> FileSearchState {
    info!("初始化文件搜索状态（内置索引器模式）");
    let state = FileSearchState::new();

    // 先尝试加载磁盘缓存
    let cache_loaded = state.indexer.load_cache();
    if cache_loaded {
        info!("已从缓存加载文件索引，后台将继续更新...");
    } else {
        info!("无缓存可用，启动全盘扫描...");
    }

    // 无论是否有缓存，都启动后台扫描以更新索引
    state.indexer.start_background_scan();

    state
}

// =============================================================================
// Response Types
// =============================================================================

/// 搜索结果（序列化给前端）
#[derive(Debug, Clone, Serialize)]
pub struct FileSearchResultItem {
    /// 文件名
    pub name: String,
    /// 完整路径
    pub path: String,
    /// 文件大小（字节）
    pub size: u64,
    /// 修改时间（Unix 时间戳秒）
    pub modified_secs: i64,
    /// 是否为目录
    pub is_directory: bool,
    /// 匹配得分
    pub score: i64,
    /// 匹配位置（用于高亮）
    pub match_indices: Vec<(usize, usize)>,
}

/// 搜索响应
#[derive(Debug, Clone, Serialize)]
pub struct FileSearchResponse {
    /// 搜索结果列表
    pub results: Vec<FileSearchResultItem>,
    /// 匹配总数
    pub total_count: u64,
    /// 搜索耗时（毫秒）
    pub search_time_ms: u64,
}

/// 索引状态响应
#[derive(Debug, Clone, Serialize)]
pub struct FileSearchStatusResponse {
    /// 状态: "idle" / "scanning" / "ready" / "error"
    pub status: String,
    /// 已索引文件数
    pub indexed_files: u64,
    /// 已扫描文件数（扫描中时）
    pub scanned_files: u64,
    /// 扫描耗时（毫秒，就绪时）
    pub scan_time_ms: u64,
    /// 是否正在扫描
    pub is_scanning: bool,
    /// 错误消息
    pub error: Option<String>,
}

// =============================================================================
// Tauri Commands
// =============================================================================

/// 文件搜索命令
///
/// 搜索文件名匹配关键词的文件。
#[tauri::command]
pub async fn file_search(
    state: State<'_, FileSearchState>,
    keyword: String,
    match_mode: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
) -> Result<FileSearchResponse, String> {
    let mode = match_mode.unwrap_or_else(|| "fuzzy".to_string());
    let limit = limit.unwrap_or(100);
    let offset = offset.unwrap_or(0);

    debug!(
        "文件搜索请求: keyword='{}', mode='{}', limit={}, offset={}",
        keyword, mode, limit, offset
    );

    let results = state.indexer.search(&keyword, &mode, limit, offset);

    let items: Vec<FileSearchResultItem> = results
        .hits
        .into_iter()
        .map(|hit| FileSearchResultItem {
            name: hit.name,
            path: hit.path,
            size: hit.size,
            modified_secs: hit.modified_secs,
            is_directory: hit.is_directory,
            score: hit.score,
            match_indices: hit.match_indices,
        })
        .collect();

    Ok(FileSearchResponse {
        results: items,
        total_count: results.total_count,
        search_time_ms: results.search_time_ms,
    })
}

/// 获取文件搜索索引状态
#[tauri::command]
pub async fn get_file_search_status(
    state: State<'_, FileSearchState>,
) -> Result<FileSearchStatusResponse, String> {
    let indexer_status = state.indexer.get_status();
    let is_scanning = state.indexer.is_scanning();
    let indexed_files = state.indexer.indexed_count();

    let response = match indexer_status {
        IndexerStatus::Idle => FileSearchStatusResponse {
            status: "idle".to_string(),
            indexed_files,
            scanned_files: 0,
            scan_time_ms: 0,
            is_scanning,
            error: None,
        },
        IndexerStatus::Scanning { scanned_files } => FileSearchStatusResponse {
            status: "scanning".to_string(),
            indexed_files,
            scanned_files,
            scan_time_ms: 0,
            is_scanning: true,
            error: None,
        },
        IndexerStatus::Ready { total_files, scan_time_ms } => FileSearchStatusResponse {
            status: "ready".to_string(),
            indexed_files: total_files,
            scanned_files: total_files,
            scan_time_ms,
            is_scanning,
            error: None,
        },
        IndexerStatus::Error(msg) => FileSearchStatusResponse {
            status: "error".to_string(),
            indexed_files,
            scanned_files: 0,
            scan_time_ms: 0,
            is_scanning,
            error: Some(msg),
        },
    };

    Ok(response)
}

/// 重建文件搜索索引
#[tauri::command]
pub async fn rebuild_file_search_index(
    state: State<'_, FileSearchState>,
) -> Result<String, String> {
    info!("收到重建文件索引请求");
    state.indexer.start_background_scan();
    Ok("索引重建已启动".to_string())
}
