//! Tauri 命令接口
//!
//! 提供前端调用的规章索引相关命令。

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tracing::{debug, info, warn};
use walkdir::WalkDir;

use super::crawler::{DownloadConfig, DownloadItem, RegulationCrawler};
use super::filename::{build_pretty_filename, dedupe_filename, sanitize_filename};
use super::index::RegulationIndex;
use super::online_search::{
    CaacOnlineSearcher, OnlineDocument, OnlineSearchRequest, OnlineSearchResponse,
};
use super::schema::RegulationDocument;
use super::search::{generate_snippets, sort_results, SortOrder};
use super::sync::{calculate_file_hash, BatchProgress};
use super::text_extractor;
use crate::database::regulation as regulation_db;
use crate::database::regulation::SyncStatus;
use crate::database::settings::get_cached_config;

/// 规章索引状态（Tauri 管理）
pub struct RegulationIndexState {
    pub index: Mutex<Option<RegulationIndex>>,
}

impl Default for RegulationIndexState {
    fn default() -> Self {
        Self { index: Mutex::new(None) }
    }
}

/// 搜索请求参数
#[derive(Debug, Deserialize)]
pub struct SearchRequest {
    /// 搜索关键词
    pub query: String,
    /// 有效性筛选：all, valid, invalid
    #[serde(default = "default_all")]
    pub validity: String,
    /// 文档类型：all, regulation, normative
    #[serde(default = "default_all")]
    pub doc_type: String,
    /// 起始发布日期（YYYY-MM-DD，空字符串表示不限）
    #[serde(default)]
    pub start_date: String,
    /// 截止发布日期（YYYY-MM-DD，空字符串表示不限）
    #[serde(default)]
    pub end_date: String,
    /// 限制本地搜索结果必须位于这些扫描目录内
    #[serde(default)]
    pub scan_folders: Vec<String>,
    /// 返回数量限制
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// 排序方式：relevance, date_desc, date_asc, title_asc
    #[serde(default = "default_sort")]
    pub sort: String,
}

fn default_all() -> String {
    "all".to_string()
}

fn default_limit() -> usize {
    100
}

fn default_sort() -> String {
    "relevance".to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalCopyMode {
    RegisterOnly,
    CopyThenRegister,
}

const INVALID_VALIDITY_LABELS: &[&str] = &["废止", "失效", "历史版本"];

impl LocalCopyMode {
    fn from_optional(value: Option<&str>) -> Self {
        match value.unwrap_or("copy_then_register") {
            "register_only" => Self::RegisterOnly,
            _ => Self::CopyThenRegister,
        }
    }
}

fn resolve_target_dir<R: tauri::Runtime>(
    app: &AppHandle<R>,
    target_dir: Option<&str>,
) -> Result<PathBuf, String> {
    if let Some(dir) = target_dir {
        return Ok(PathBuf::from(dir));
    }

    // 优先使用用户配置的局方文件保存目录
    if let Some(config) = get_cached_config() {
        let custom_path = &config.advanced.regulation_storage_path;
        if !custom_path.is_empty() {
            return Ok(PathBuf::from(custom_path));
        }
    }

    let app_data_dir =
        app.path().app_data_dir().map_err(|e| format!("获取应用数据目录失败: {}", e))?;
    Ok(app_data_dir.join("regulations"))
}

const REGULATION_CATEGORY_DIRS: &[(&str, &str)] =
    &[("regulation", "CCAR规章"), ("normative", "规范性文件"), ("standard", "标准规范")];

fn category_subdir_for_doc_type(doc_type: &str) -> &'static str {
    REGULATION_CATEGORY_DIRS
        .iter()
        .find(|(key, _)| *key == doc_type)
        .map(|(_, subdir)| *subdir)
        .unwrap_or("CCAR规章")
}

fn resolve_category_dir(root: &Path, doc_type: &str) -> PathBuf {
    root.join(category_subdir_for_doc_type(doc_type))
}

fn ensure_regulation_category_dirs(root: &Path) -> Result<Vec<PathBuf>, String> {
    std::fs::create_dir_all(root)
        .map_err(|e| format!("创建局方文件保存目录失败 {}: {}", root.display(), e))?;

    let mut dirs = Vec::with_capacity(REGULATION_CATEGORY_DIRS.len());
    for (_, subdir) in REGULATION_CATEGORY_DIRS {
        let dir = root.join(subdir);
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("创建分类目录失败 {}: {}", dir.display(), e))?;
        dirs.push(dir);
    }

    Ok(dirs)
}

fn normalize_extension(original_name: Option<&str>, default_ext: &str) -> String {
    original_name
        .and_then(|n| Path::new(n).extension())
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_ext.to_string())
}

/// 解析目标存储路径。
///
/// `RegisterOnly` 模式直接返回原路径（不复制）。
/// `CopyThenRegister` 模式按 [`build_pretty_filename`] 规则生成「文号_标题.ext」
/// 风格的文件名，并通过 [`dedupe_filename`] 解决重名冲突。
///
/// 旧版本一律使用 `<sha256前16字符>.pdf`，会让用户在文件管理器里看到
/// `00d305aa24de6e30.pdf` 这种不可读名字；新版规则保留源扩展名并与官网下载保持一致。
fn resolve_storage_path(
    source_path: &Path,
    sha256: &str,
    doc_number: Option<&str>,
    title: Option<&str>,
    mode: LocalCopyMode,
    target_dir: &Path,
) -> Result<PathBuf, String> {
    if mode == LocalCopyMode::RegisterOnly {
        return Ok(source_path.to_path_buf());
    }

    std::fs::create_dir_all(target_dir).map_err(|e| format!("创建目标目录失败: {}", e))?;

    let ext = source_path.extension().and_then(|e| e.to_str()).unwrap_or("pdf").to_lowercase();
    let desired_name = build_pretty_filename(doc_number, title, sha256, &ext);
    let target_path = dedupe_filename(target_dir, &desired_name, sha256);

    if target_path.exists() {
        return Ok(target_path);
    }

    std::fs::copy(source_path, &target_path)
        .map_err(|e| format!("复制文件失败 {:?} -> {:?}: {}", source_path, target_path, e))?;

    Ok(target_path)
}

/// 搜索响应
#[derive(Debug, Serialize)]
pub struct SearchResponse {
    /// 搜索结果
    pub documents: Vec<RegulationDocument>,
    /// 结果总数
    pub total: usize,
    /// 搜索耗时（毫秒）
    pub elapsed_ms: u64,
    /// 正文摘要（与 documents 等长，在线结果为 null）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippets: Option<Vec<Option<String>>>,
}

/// 索引统计信息
#[derive(Debug, Serialize)]
pub struct IndexStats {
    /// 文档总数
    pub doc_count: u64,
    /// 索引路径
    pub index_path: String,
    /// 是否已初始化
    pub initialized: bool,
}

/// 初始化规章索引
///
/// 如果 tantivy 索引为空但 SQLite 数据库有文件记录，
/// 自动从数据库重建索引（只索引元数据，不重新 OCR）。
#[tauri::command]
pub async fn regulation_index_init<R: tauri::Runtime>(
    app: AppHandle<R>,
    state: State<'_, RegulationIndexState>,
) -> Result<IndexStats, String> {
    info!("初始化规章索引");

    // 获取索引存储路径
    let app_data_dir =
        app.path().app_data_dir().map_err(|e| format!("获取应用数据目录失败: {}", e))?;

    let index_path = app_data_dir.join("regulation_index");

    // 创建索引
    let index = RegulationIndex::open_or_create(index_path.clone())
        .map_err(|e| format!("创建索引失败: {}", e))?;

    let mut doc_count = index.doc_count();

    // 如果索引为空，尝试从 SQLite 数据库自动重建
    if doc_count == 0 {
        info!("tantivy 索引为空，尝试从数据库自动重建...");

        // 获取数据库连接
        let db_path = app_data_dir.join("history.db");
        if db_path.exists() {
            match rusqlite::Connection::open(&db_path) {
                Ok(conn) => {
                    // 初始化表（确保表存在）
                    let _ = regulation_db::init_regulation_schema(&conn);

                    // 查询所有已标记为 indexed 的文件
                    let mut stmt = conn.prepare(
                        "SELECT title, doc_number, doc_type, validity, office_unit, sign_date, \
                                publish_date, url, file_path, ocr_status \
                         FROM regulation_files WHERE indexed = 1 OR ocr_status = 'done'"
                    ).map_err(|e| format!("查询失败: {}", e))?;

                    let files: Vec<(
                        String,
                        String,
                        String,
                        String,
                        String,
                        String,
                        String,
                        String,
                        String,
                        String,
                    )> = stmt
                        .query_map([], |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                                row.get::<_, String>(2)?,
                                row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                                row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                                row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                                row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                                row.get::<_, String>(7)?,
                                row.get::<_, String>(8)?,
                                row.get::<_, String>(9)?,
                            ))
                        })
                        .map_err(|e| format!("查询失败: {}", e))?
                        .filter_map(|r| r.ok())
                        .collect();

                    let file_count = files.len();
                    if file_count > 0 {
                        info!("从数据库恢复 {} 个文件到索引", file_count);

                        for (
                            title,
                            doc_number,
                            doc_type,
                            validity,
                            office_unit,
                            sign_date,
                            publish_date,
                            url,
                            file_path,
                            _ocr_status,
                        ) in &files
                        {
                            let doc = RegulationDocument {
                                title: title.clone(),
                                doc_number: doc_number.clone(),
                                validity: validity.clone(),
                                doc_type: doc_type.clone(),
                                office_unit: office_unit.clone(),
                                sign_date: sign_date.clone(),
                                publish_date: publish_date.clone(),
                                url: url.clone(),
                                file_path: file_path.clone(),
                                content: String::new(), // 内容需要重新提取
                            };

                            if let Err(e) = index.add_document(&doc) {
                                warn!("添加文档到索引失败: {} - {}", title, e);
                            }
                        }

                        // 提交索引
                        if let Err(e) = index.commit() {
                            warn!("提交索引失败: {}", e);
                        } else {
                            info!("自动重建索引完成，已索引 {} 个文档", file_count);
                        }

                        doc_count = index.doc_count();
                    }
                }
                Err(e) => {
                    warn!("打开数据库失败，跳过自动重建: {}", e);
                }
            }
        }
    }

    // 存储到状态
    let mut state_guard = state.index.lock().map_err(|e| format!("锁定状态失败: {}", e))?;
    *state_guard = Some(index);

    info!("规章索引初始化完成，文档数: {}", doc_count);

    Ok(IndexStats {
        doc_count,
        index_path: index_path.to_string_lossy().to_string(),
        initialized: true,
    })
}

/// 本地搜索规章
#[tauri::command]
pub async fn regulation_local_search(
    request: SearchRequest,
    state: State<'_, RegulationIndexState>,
) -> Result<SearchResponse, String> {
    let start = std::time::Instant::now();

    debug!("本地搜索: {:?}", request);

    let state_guard = state.index.lock().map_err(|e| format!("锁定状态失败: {}", e))?;

    let index = state_guard.as_ref().ok_or("索引未初始化，请先调用 regulation_index_init")?;

    // 执行搜索
    let validity = if request.validity == "all" { None } else { Some(request.validity.as_str()) };

    let doc_type = if request.doc_type == "all" { None } else { Some(request.doc_type.as_str()) };

    let has_date_filter = !request.start_date.is_empty() || !request.end_date.is_empty();
    let scan_folder_filter = normalize_scan_folder_filters(&request.scan_folders);
    let has_scan_folder_filter = !scan_folder_filter.is_empty();
    let needs_validity_post_filter = matches!(request.validity.as_str(), "valid" | "invalid");
    let needs_post_filter =
        needs_validity_post_filter || has_date_filter || has_scan_folder_filter;

    let mut results = if needs_post_filter {
        let expanded_limit = if has_scan_folder_filter {
            usize::try_from(index.doc_count()).unwrap_or(usize::MAX).max(request.limit)
        } else {
            request.limit.saturating_mul(5).max(request.limit)
        };
        // validity 走后过滤时，索引层不传 validity；否则保留索引层 validity 过滤
        let index_validity = if needs_validity_post_filter { None } else { validity };
        let mut candidates = index
            .search_with_filter(&request.query, index_validity, doc_type, expanded_limit)
            .map_err(|e| format!("搜索失败: {}", e))?;
        normalize_document_validities(&mut candidates);

        // 有效性后过滤（"valid"/"invalid" 含推断逻辑，索引层无法准确表达）
        if needs_validity_post_filter {
            let want_invalid = request.validity == "invalid";
            candidates.retain(|doc| is_effectively_invalid(doc) == want_invalid);
        }

        // 日期范围后过滤（YYYY-MM-DD 字典序 == 日期序，无 publish_date 的文档被排除）
        if has_date_filter {
            let start = request.start_date.as_str();
            let end = request.end_date.as_str();
            candidates.retain(|doc| {
                let pd = doc.publish_date.as_str();
                if pd.is_empty() {
                    return false;
                }
                let after_start = start.is_empty() || pd >= start;
                let before_end = end.is_empty() || pd <= end;
                after_start && before_end
            });
        }

        if has_scan_folder_filter {
            candidates.retain(|doc| {
                is_document_in_normalized_scan_folders(doc, &scan_folder_filter)
            });
        }

        candidates.truncate(request.limit);
        candidates
    } else {
        let mut docs = index
            .search_with_filter(&request.query, validity, doc_type, request.limit)
            .map_err(|e| format!("搜索失败: {}", e))?;
        normalize_document_validities(&mut docs);
        docs
    };

    // 排序
    let sort_order = match request.sort.as_str() {
        "date_desc" => SortOrder::DateDesc,
        "date_asc" => SortOrder::DateAsc,
        "title_asc" => SortOrder::TitleAsc,
        _ => SortOrder::Relevance,
    };
    sort_results(&mut results, sort_order);

    let elapsed = start.elapsed();
    let total = results.len();

    // 生成正文摘要（420 字符限制）
    //
    // 240 字符在长标题 / 长句 PDF 里仍然经常只能看到半句上下文，
    // 这里再放宽到 420 字符，前端同步提高展示高度，让搜索结果能带出更完整的段落。
    let snippets = generate_snippets(&results, &request.query, 420);

    // 清空 documents 中的 content 字段（不把完整正文发给前端）
    for doc in &mut results {
        doc.content.clear();
    }

    info!(
        "本地搜索 '{}' 完成，返回 {} 条结果，耗时 {}ms",
        request.query,
        total,
        elapsed.as_millis()
    );

    Ok(SearchResponse {
        documents: results,
        total,
        elapsed_ms: elapsed.as_millis() as u64,
        snippets: Some(snippets),
    })
}

/// 添加文档到索引
#[tauri::command]
pub async fn regulation_index_add(
    document: RegulationDocument,
    state: State<'_, RegulationIndexState>,
) -> Result<bool, String> {
    debug!("添加文档到索引: {}", document.title);

    let state_guard = state.index.lock().map_err(|e| format!("锁定状态失败: {}", e))?;

    let index = state_guard.as_ref().ok_or("索引未初始化")?;

    // 检查是否已存在
    if index.exists(&document.url) {
        debug!("文档已存在，跳过: {}", document.url);
        return Ok(false);
    }

    index.add_document(&document).map_err(|e| format!("添加文档失败: {}", e))?;

    index.commit().map_err(|e| format!("提交索引失败: {}", e))?;

    info!("文档已添加到索引: {}", document.title);
    Ok(true)
}

/// 批量添加文档到索引
#[tauri::command]
pub async fn regulation_index_add_batch(
    documents: Vec<RegulationDocument>,
    state: State<'_, RegulationIndexState>,
) -> Result<usize, String> {
    info!("批量添加 {} 个文档到索引", documents.len());

    let state_guard = state.index.lock().map_err(|e| format!("锁定状态失败: {}", e))?;

    let index = state_guard.as_ref().ok_or("索引未初始化")?;

    // 过滤已存在的文档
    let new_docs: Vec<_> = documents.into_iter().filter(|doc| !index.exists(&doc.url)).collect();

    if new_docs.is_empty() {
        return Ok(0);
    }

    let count = index.add_documents(&new_docs).map_err(|e| format!("批量添加失败: {}", e))?;

    index.commit().map_err(|e| format!("提交索引失败: {}", e))?;

    info!("批量添加完成，新增 {} 个文档", count);
    Ok(count)
}

/// 获取索引统计信息
#[tauri::command]
pub async fn regulation_index_stats(
    state: State<'_, RegulationIndexState>,
) -> Result<IndexStats, String> {
    let state_guard = state.index.lock().map_err(|e| format!("锁定状态失败: {}", e))?;

    match state_guard.as_ref() {
        Some(index) => Ok(IndexStats {
            doc_count: index.doc_count(),
            index_path: index.index_path().to_string_lossy().to_string(),
            initialized: true,
        }),
        None => Ok(IndexStats { doc_count: 0, index_path: String::new(), initialized: false }),
    }
}

/// 清空索引
#[tauri::command]
pub async fn regulation_index_clear(state: State<'_, RegulationIndexState>) -> Result<(), String> {
    info!("清空规章索引");

    let state_guard = state.index.lock().map_err(|e| format!("锁定状态失败: {}", e))?;

    let index = state_guard.as_ref().ok_or("索引未初始化")?;

    index.clear().map_err(|e| format!("清空索引失败: {}", e))?;

    info!("规章索引已清空");
    Ok(())
}

/// 检查文档是否已索引
#[tauri::command]
pub async fn regulation_index_exists(
    url: String,
    state: State<'_, RegulationIndexState>,
) -> Result<bool, String> {
    let state_guard = state.index.lock().map_err(|e| format!("锁定状态失败: {}", e))?;

    let index = state_guard.as_ref().ok_or("索引未初始化")?;

    Ok(index.exists(&url))
}

// ============================================================================
// 批量下载相关命令
// ============================================================================

/// 批量下载状态（Tauri 管理）
pub struct BatchDownloadState {
    /// 下载进度
    pub progress: Mutex<BatchProgress>,
    /// 是否正在下载
    pub is_downloading: Mutex<bool>,
}

impl Default for BatchDownloadState {
    fn default() -> Self {
        Self { progress: Mutex::new(BatchProgress::default()), is_downloading: Mutex::new(false) }
    }
}

/// 批量下载请求
#[derive(Debug, Deserialize)]
pub struct BatchDownloadRequest {
    /// 下载项列表
    pub items: Vec<DownloadItemRequest>,
    /// 保存目录（可选，默认使用应用数据目录）
    pub save_dir: Option<String>,
}

/// 下载项请求
#[derive(Debug, Deserialize)]
pub struct DownloadItemRequest {
    /// 下载 URL
    pub url: String,
    /// 规章标题
    pub title: String,
    /// 文号
    pub doc_number: Option<String>,
    /// 文档类型
    pub doc_type: String,
    /// 来源 URL（用于去重）
    pub source_url: String,
}

/// 批量下载响应
#[derive(Debug, Serialize)]
pub struct BatchDownloadResponse {
    /// 成功数
    pub success: usize,
    /// 跳过数（已存在）
    pub skipped: usize,
    /// 失败数
    pub failed: usize,
    /// 失败的 URL 列表
    pub failed_urls: Vec<String>,
}

/// 单文件下载文档
#[derive(Debug, Deserialize, Clone)]
pub struct SingleDownloadDocument {
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub pdf_url: String,
    #[serde(default)]
    pub doc_number: String,
    #[serde(default)]
    pub doc_type: String,
    #[serde(default)]
    pub validity: String,
    #[serde(default)]
    pub office_unit: String,
    #[serde(default)]
    pub sign_date: String,
    #[serde(default)]
    pub publish_date: String,
}

/// 单文件下载请求
#[derive(Debug, Deserialize)]
pub struct SingleDownloadRequest {
    pub document: SingleDownloadDocument,
    #[serde(alias = "saveDir")]
    pub save_dir: Option<String>,
    #[serde(default = "default_true", alias = "preferAttachment")]
    pub prefer_attachment: bool,
}

fn default_true() -> bool {
    true
}

/// 单文件下载响应
#[derive(Debug, Serialize)]
pub struct SingleDownloadResponse {
    pub success: bool,
    pub file_path: String,
    pub file_type: String,
    pub error: Option<String>,
}

fn build_absolute_url(base_url: &str, href: &str) -> Option<String> {
    if href.starts_with("http://") || href.starts_with("https://") {
        return Some(href.to_string());
    }

    let base = url::Url::parse(base_url).ok()?;
    base.join(href).ok().map(|u| u.to_string())
}

const LEGACY_STATIC_HOST: &str = "https://ccar.hudawang.cn/";
const STATIC_HOST: &str = "https://flighttoolbox.hudawang.cn/";

fn normalize_static_mirror_url(url: &str) -> String {
    let trimmed = url.trim();
    if let Some(path) = trimmed.strip_prefix(LEGACY_STATIC_HOST) {
        format!("{}{}", STATIC_HOST, path)
    } else {
        trimmed.to_string()
    }
}

fn is_download_file_url(url: &str) -> bool {
    let lower = url.split(['?', '#']).next().unwrap_or("").to_lowercase();
    lower.ends_with(".pdf")
        || lower.ends_with(".doc")
        || lower.ends_with(".docx")
        || lower.ends_with(".txt")
}

fn push_unique_download_candidate(candidates: &mut Vec<String>, url: String) {
    let normalized = normalize_static_mirror_url(&url);
    if !normalized.is_empty() && !candidates.iter().any(|candidate| candidate == &normalized) {
        candidates.push(normalized);
    }
}

fn build_single_download_candidates(
    doc: &SingleDownloadDocument,
    official_attachment_url: Option<String>,
    prefer_attachment: bool,
) -> Vec<String> {
    let mut candidates = Vec::new();

    if prefer_attachment {
        if is_download_file_url(&doc.url) {
            push_unique_download_candidate(&mut candidates, doc.url.clone());
        }
        if let Some(url) = official_attachment_url {
            push_unique_download_candidate(&mut candidates, url);
        }
    } else if is_download_file_url(&doc.url) {
        push_unique_download_candidate(&mut candidates, doc.url.clone());
    }

    push_unique_download_candidate(&mut candidates, doc.pdf_url.clone());
    candidates
}

fn extract_attachment_url(html: &str, detail_url: &str, exts: &[&str]) -> Option<String> {
    let ext_pattern = exts.iter().map(|s| regex::escape(s)).collect::<Vec<_>>().join("|");
    let pattern = format!(r#"href\s*=\s*"([^"]+({}))""#, ext_pattern);
    let re = regex::Regex::new(&pattern).ok()?;
    let href = re.captures(html)?.get(1)?.as_str();
    build_absolute_url(detail_url, href)
}

fn extract_text_from_html_body(html: &str) -> String {
    use std::sync::LazyLock;
    static BODY_SELECTOR: LazyLock<scraper::Selector> =
        LazyLock::new(|| scraper::Selector::parse("body").expect("body selector should be valid"));

    let doc = scraper::Html::parse_document(html);
    if let Some(body) = doc.select(&BODY_SELECTOR).next() {
        body.text().map(str::trim).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("\n")
    } else {
        String::new()
    }
}

#[cfg(test)]
mod single_download_candidate_tests {
    use super::*;

    #[test]
    fn prefers_official_attachment_before_static_mirror() {
        let doc = SingleDownloadDocument {
            title: "高原运输机场建设指南".to_string(),
            url: "https://www.caac.gov.cn/XXGK/XXGK/BZGF/HYBZ/202604/t20260403_229496.html"
                .to_string(),
            pdf_url: "https://ccar.hudawang.cn/specification/MH_T%205092-2026.pdf".to_string(),
            doc_number: "MH/T 5092-2026".to_string(),
            doc_type: "standard".to_string(),
            validity: "有效".to_string(),
            office_unit: "机场司".to_string(),
            sign_date: String::new(),
            publish_date: "2026-04-03".to_string(),
        };

        let candidates = build_single_download_candidates(
            &doc,
            Some("https://www.caac.gov.cn/XXGK/XXGK/BZGF/HYBZ/P020260403.pdf".to_string()),
            true,
        );

        assert_eq!(
            candidates,
            vec![
                "https://www.caac.gov.cn/XXGK/XXGK/BZGF/HYBZ/P020260403.pdf".to_string(),
                "https://flighttoolbox.hudawang.cn/specification/MH_T%205092-2026.pdf".to_string(),
            ]
        );
    }
}

/// 单文件下载（详情页附件解析 + 回退）
#[tauri::command]
pub async fn regulation_download_single<R: tauri::Runtime>(
    app: AppHandle<R>,
    request: SingleDownloadRequest,
    index_state: State<'_, RegulationIndexState>,
) -> Result<SingleDownloadResponse, String> {
    let app_data_dir =
        app.path().app_data_dir().map_err(|e| format!("获取应用数据目录失败: {}", e))?;
    let db_path = app_data_dir.join("history.db");
    let conn =
        rusqlite::Connection::open(&db_path).map_err(|e| format!("打开数据库失败: {}", e))?;
    regulation_db::init_regulation_schema(&conn).map_err(|e| format!("初始化规章表失败: {}", e))?;

    if let Ok(Some(existing)) = regulation_db::get_file_by_url(&conn, &request.document.url) {
        let doc_type = if request.document.doc_type.is_empty() {
            existing.doc_type.as_str()
        } else {
            request.document.doc_type.as_str()
        };
        let _ = regulation_db::update_official_metadata(
            &conn,
            &request.document.url,
            &request.document.title,
            &request.document.doc_number,
            doc_type,
            &request.document.validity,
            &request.document.office_unit,
            &request.document.sign_date,
            &request.document.publish_date,
        );

        return Ok(SingleDownloadResponse {
            success: true,
            file_path: existing.file_path,
            file_type: "cached".to_string(),
            error: None,
        });
    }

    let save_dir = if let Some(dir) = request.save_dir.clone() {
        PathBuf::from(dir)
    } else {
        let root = resolve_target_dir(&app, None)?;
        ensure_regulation_category_dirs(&root)?;
        resolve_category_dir(&root, &request.document.doc_type)
    };
    std::fs::create_dir_all(&save_dir).map_err(|e| format!("创建下载目录失败: {}", e))?;

    let doc = request.document;
    let mut file_path = String::new();
    let mut file_type = String::new();
    let mut sha256 = String::new();
    let mut file_size = 0_i64;
    let mut content = String::new();
    let mut needs_ocr = true;
    let mut resolved_pdf_url: Option<String> = None;

    let crawler = RegulationCrawler::new(DownloadConfig {
        save_dir: save_dir.clone(),
        max_concurrent: 1,
        delay_ms: 0,
        ..Default::default()
    })
    .map_err(|e| format!("创建下载器失败: {}", e))?;

    let mut detail_html: Option<String> = None;
    let mut official_attachment_url: Option<String> = None;

    if request.prefer_attachment && !is_download_file_url(&doc.url) {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

        match client.get(&doc.url).send().await {
            Ok(resp) => match resp.text().await {
                Ok(html) => {
                    official_attachment_url = super::crawler::extract_pdf_url(&doc.url, &html)
                        .or_else(|| extract_attachment_url(&html, &doc.url, &[".pdf"]))
                        .or_else(|| extract_attachment_url(&html, &doc.url, &[".docx", ".doc"]));
                    detail_html = Some(html);
                }
                Err(e) => warn!("读取局方详情页失败，将尝试服务器镜像: {}", e),
            },
            Err(e) => warn!("访问局方详情页失败，将尝试服务器镜像: {}", e),
        }
    }

    let download_candidates =
        build_single_download_candidates(&doc, official_attachment_url, request.prefer_attachment);
    let mut download_errors = Vec::new();

    for url in download_candidates {
        let ext = normalize_extension(Some(&url), "pdf");
        match crawler
            .download_file(
                &url,
                if doc.doc_number.is_empty() { None } else { Some(doc.doc_number.as_str()) },
                if doc.title.is_empty() { None } else { Some(doc.title.as_str()) },
                Some(&format!("placeholder.{}", ext)),
            )
            .await
        {
            Ok(result) => {
                resolved_pdf_url = result.pdf_url.clone();
                sha256 = result.sha256.clone();
                file_size = result.file_size as i64;
                file_path = result.file_path.to_string_lossy().to_string();
                file_type = ext;

                let extraction = text_extractor::extract_text_from_pdf(Path::new(&file_path))
                    .map_err(|e| format!("文本提取失败: {}", e))?;
                content = extraction.text;
                needs_ocr = extraction.needs_ocr;
                break;
            }
            Err(e) => {
                warn!("下载候选失败，将尝试下一个: {} ({})", url, e);
                download_errors.push(format!("{}: {}", url, e));
            }
        }
    }

    // 若仍未找到可下载附件，则回退为正文 TXT
    if file_path.is_empty() {
        if let Some(html) = detail_html {
            let text = extract_text_from_html_body(&html);
            if !text.trim().is_empty() {
                let filename = format!("{}.txt", sanitize_filename(&doc.title));
                let txt_path = save_dir.join(filename);
                std::fs::write(&txt_path, text.as_bytes())
                    .map_err(|e| format!("保存正文文本失败: {}", e))?;
                let bytes = text.into_bytes();
                sha256 = super::sync::calculate_bytes_hash(&bytes);
                file_size = bytes.len() as i64;
                file_path = txt_path.to_string_lossy().to_string();
                file_type = "txt".to_string();
                content = String::from_utf8_lossy(&bytes).to_string();
                needs_ocr = false;
            }
        }
    }

    if file_path.is_empty() {
        let error = if download_errors.is_empty() {
            "未找到可下载的附件，且正文提取失败".to_string()
        } else {
            format!("局方附件和服务器镜像均下载失败: {}", download_errors.join("; "))
        };
        return Ok(SingleDownloadResponse {
            success: false,
            file_path: String::new(),
            file_type: String::new(),
            error: Some(error),
        });
    }

    let ocr_status = if needs_ocr { "pending" } else { "done" }.to_string();
    let db_file = regulation_db::RegulationFile {
        title: doc.title.clone(),
        doc_number: doc.doc_number.clone(),
        doc_type: if doc.doc_type.is_empty() {
            "regulation".to_string()
        } else {
            doc.doc_type.clone()
        },
        validity: doc.validity.clone(),
        office_unit: doc.office_unit.clone(),
        sign_date: doc.sign_date.clone(),
        publish_date: doc.publish_date.clone(),
        url: doc.url.clone(),
        pdf_url: resolved_pdf_url,
        sha256: sha256.clone(),
        file_path: file_path.clone(),
        file_size,
        ocr_status,
        ..Default::default()
    };

    let file_id = regulation_db::insert_file(&conn, &db_file)
        .map_err(|e| format!("写入数据库失败: {}", e))?;

    if !needs_ocr && !content.is_empty() {
        let index_doc = RegulationDocument {
            title: doc.title,
            doc_number: doc.doc_number,
            validity: doc.validity,
            doc_type: if db_file.doc_type.is_empty() {
                "regulation".to_string()
            } else {
                db_file.doc_type.clone()
            },
            office_unit: doc.office_unit,
            sign_date: doc.sign_date,
            publish_date: doc.publish_date,
            url: db_file.url,
            file_path: file_path.clone(),
            content,
        };

        let state_guard =
            index_state.index.lock().map_err(|e| format!("锁定索引状态失败: {}", e))?;
        if let Some(index) = state_guard.as_ref() {
            if !index.exists(&index_doc.url) {
                index
                    .add_document(&index_doc)
                    .and_then(|_| index.commit())
                    .map_err(|e| format!("写入索引失败: {}", e))?;
            }
            let _ = regulation_db::mark_indexed(&conn, file_id);
        }
    }

    Ok(SingleDownloadResponse { success: true, file_path, file_type, error: None })
}

/// 批量下载规章
#[tauri::command]
pub async fn regulation_batch_download<R: tauri::Runtime>(
    app: AppHandle<R>,
    request: BatchDownloadRequest,
    download_state: State<'_, BatchDownloadState>,
) -> Result<BatchDownloadResponse, String> {
    info!("开始批量下载 {} 个规章", request.items.len());

    // 检查是否已在下载
    {
        let is_downloading =
            download_state.is_downloading.lock().map_err(|e| format!("锁定状态失败: {}", e))?;
        if *is_downloading {
            return Err("已有下载任务正在进行".to_string());
        }
    }

    // 设置下载中状态
    {
        let mut is_downloading =
            download_state.is_downloading.lock().map_err(|e| format!("锁定状态失败: {}", e))?;
        *is_downloading = true;
    }

    // 获取保存目录
    let save_dir = if let Some(dir) = request.save_dir {
        std::path::PathBuf::from(dir)
    } else {
        resolve_target_dir(&app, None)?
    };

    // 创建下载器
    let config = DownloadConfig {
        save_dir: save_dir.clone(),
        max_concurrent: 2,
        delay_ms: 3000,
        ..Default::default()
    };

    let crawler = RegulationCrawler::new(config).map_err(|e| format!("创建下载器失败: {}", e))?;

    // 转换下载项
    let items: Vec<DownloadItem> = request
        .items
        .into_iter()
        .map(|item| DownloadItem {
            url: item.url.clone(),
            title: item.title,
            doc_number: item.doc_number,
            doc_type: item.doc_type,
            original_name: None,
            source_url: item.source_url,
        })
        .collect();

    // 执行批量下载
    let results = crawler
        .batch_download(items, |progress| {
            // 更新进度状态
            if let Ok(mut p) = download_state.progress.lock() {
                *p = progress.clone();
            }

            // 发送进度事件到前端
            if let Err(e) = app.emit("regulation:download-progress", progress) {
                debug!("发送下载进度事件失败: {}", e);
            }
        })
        .await;

    // 统计结果
    let mut success = 0;
    let mut skipped = 0;
    let mut failed = 0;
    let mut failed_urls = Vec::new();

    for result in results {
        match result {
            Ok(r) if r.is_new => success += 1,
            Ok(_) => skipped += 1,
            Err(e) => {
                failed += 1;
                failed_urls.push(e);
            }
        }
    }

    // 重置下载状态
    {
        let mut is_downloading =
            download_state.is_downloading.lock().map_err(|e| format!("锁定状态失败: {}", e))?;
        *is_downloading = false;
    }

    info!("批量下载完成: 成功 {}, 跳过 {}, 失败 {}", success, skipped, failed);

    Ok(BatchDownloadResponse { success, skipped, failed, failed_urls })
}

/// 获取下载进度
#[tauri::command]
pub async fn regulation_get_download_progress(
    state: State<'_, BatchDownloadState>,
) -> Result<BatchProgress, String> {
    let progress = state.progress.lock().map_err(|e| format!("锁定状态失败: {}", e))?;
    Ok(progress.clone())
}

/// 获取同步状态（文件统计）
#[tauri::command]
pub async fn regulation_get_sync_status<R: tauri::Runtime>(
    app: AppHandle<R>,
) -> Result<SyncStatus, String> {
    // 获取数据库路径
    let app_data_dir =
        app.path().app_data_dir().map_err(|e| format!("获取应用数据目录失败: {}", e))?;

    let db_path = app_data_dir.join("history.db");

    // 打开数据库连接
    let conn =
        rusqlite::Connection::open(&db_path).map_err(|e| format!("打开数据库失败: {}", e))?;

    // 初始化规章表（如果不存在）
    regulation_db::init_regulation_schema(&conn).map_err(|e| format!("初始化规章表失败: {}", e))?;

    // 获取统计信息
    regulation_db::get_sync_status(&conn).map_err(|e| format!("获取同步状态失败: {}", e))
}

// ============================================================================
// PDF 文本提取 + 索引命令
// ============================================================================

/// 处理结果
#[derive(Debug, Serialize)]
pub struct ProcessFilesResponse {
    /// 处理总数
    pub processed: usize,
    /// 成功提取文本并索引
    pub indexed: usize,
    /// 需要 OCR（文本不足）
    pub needs_ocr: usize,
    /// 失败数
    pub failed: usize,
}

/// 处理待提取文件（提取 PDF 文本 + 写入 Tantivy 索引）
///
/// 从数据库获取 pending 状态的文件，尝试 pdfium 提取文本，
/// 成功后写入 Tantivy 索引。
#[tauri::command]
pub async fn regulation_process_pending<R: tauri::Runtime>(
    app: AppHandle<R>,
    batch_size: Option<usize>,
    index_state: State<'_, RegulationIndexState>,
) -> Result<ProcessFilesResponse, String> {
    let batch_size = batch_size.unwrap_or(10);
    info!("开始处理待提取文件，批次大小: {}", batch_size);

    // 获取数据库路径
    let app_data_dir =
        app.path().app_data_dir().map_err(|e| format!("获取应用数据目录失败: {}", e))?;
    let db_path = app_data_dir.join("history.db");

    // 打开数据库连接
    let conn =
        rusqlite::Connection::open(&db_path).map_err(|e| format!("打开数据库失败: {}", e))?;

    regulation_db::init_regulation_schema(&conn).map_err(|e| format!("初始化规章表失败: {}", e))?;

    // 获取待处理文件
    let pending_files = regulation_db::get_pending_ocr_files(&conn, batch_size)
        .map_err(|e| format!("获取待处理文件失败: {}", e))?;

    if pending_files.is_empty() {
        info!("没有待处理的文件");
        return Ok(ProcessFilesResponse { processed: 0, indexed: 0, needs_ocr: 0, failed: 0 });
    }

    info!("找到 {} 个待处理文件", pending_files.len());

    let mut indexed = 0;
    let mut needs_ocr = 0;
    let mut failed = 0;

    for file in &pending_files {
        // 更新状态为 processing
        let _ = regulation_db::update_ocr_status(&conn, file.id, "processing", 0, 0, None);

        // 提取文本
        let pdf_path = std::path::Path::new(&file.file_path);
        let result = text_extractor::extract_text_from_pdf(pdf_path);

        match result {
            Ok(extraction) if !extraction.needs_ocr => {
                // 文本充足，写入索引
                let doc = super::schema::RegulationDocument {
                    title: file.title.clone(),
                    doc_number: file.doc_number.clone(),
                    validity: file.validity.clone(),
                    doc_type: file.doc_type.clone(),
                    office_unit: file.office_unit.clone(),
                    sign_date: file.sign_date.clone(),
                    publish_date: file.publish_date.clone(),
                    url: file.url.clone(),
                    file_path: file.file_path.clone(),
                    content: extraction.text,
                };

                // 写入 Tantivy 索引
                let index_result = {
                    let state_guard =
                        index_state.index.lock().map_err(|e| format!("锁定索引状态失败: {}", e))?;
                    if let Some(index) = state_guard.as_ref() {
                        if !index.exists(&doc.url) {
                            index
                                .add_document(&doc)
                                .and_then(|_| index.commit())
                                .map_err(|e| format!("写入索引失败: {}", e))
                        } else {
                            Ok(())
                        }
                    } else {
                        Err("索引未初始化".to_string())
                    }
                };

                match index_result {
                    Ok(()) => {
                        // 更新数据库状态
                        let _ =
                            regulation_db::update_ocr_status(&conn, file.id, "done", 100, 0, None);
                        let _ = regulation_db::update_ocr_engine(&conn, file.id, "pdfium");
                        let _ = regulation_db::mark_indexed(&conn, file.id);
                        indexed += 1;
                        info!("文件已索引: {}", file.title);
                    }
                    Err(e) => {
                        let _ = regulation_db::update_ocr_status(
                            &conn,
                            file.id,
                            "failed",
                            0,
                            0,
                            Some(&e),
                        );
                        failed += 1;
                        warn!("索引写入失败: {} - {}", file.title, e);
                    }
                }
            }
            Ok(_extraction) => {
                // 文本不足，标记需要 OCR
                let _ = regulation_db::update_ocr_status(
                    &conn,
                    file.id,
                    "pending",
                    0,
                    0,
                    Some("文本不足，需要 OCR"),
                );
                needs_ocr += 1;
                info!("文件需要 OCR: {}", file.title);
            }
            Err(e) => {
                let _ = regulation_db::update_ocr_status(&conn, file.id, "failed", 0, 0, Some(&e));
                failed += 1;
                warn!("文本提取失败: {} - {}", file.title, e);
            }
        }

        // 发送进度事件
        if let Err(e) = app.emit(
            "regulation:process-progress",
            serde_json::json!({
                "current": file.title,
                "indexed": indexed,
                "needs_ocr": needs_ocr,
                "failed": failed,
            }),
        ) {
            debug!("发送处理进度事件失败: {}", e);
        }
    }

    let processed = pending_files.len();
    info!(
        "文件处理完成: 处理 {}, 索引 {}, 需OCR {}, 失败 {}",
        processed, indexed, needs_ocr, failed
    );

    Ok(ProcessFilesResponse { processed, indexed, needs_ocr, failed })
}

// ============================================================================
// 共享扫描/OCR 辅助函数
// ============================================================================

/// 发送扫描进度事件（减少样板代码）
fn emit_scan_progress<R: tauri::Runtime>(
    app: &AppHandle<R>,
    scanned: usize,
    total_found: usize,
    new_files: usize,
    duplicates: usize,
    indexed: usize,
    needs_ocr: usize,
    failed: usize,
    current_file: Option<String>,
    phase: &str,
    ocr_processed: Option<usize>,
    ocr_total: Option<usize>,
) {
    if let Err(e) = app.emit(
        "regulation:scan-progress",
        ScanProgress {
            scanned,
            total_found,
            new_files,
            duplicates,
            indexed,
            needs_ocr,
            failed,
            current_file,
            phase: phase.to_string(),
            ocr_processed,
            ocr_total,
        },
    ) {
        debug!("发送扫描进度事件失败: {}", e);
    }
}

/// 打开数据库并加载去重数据（哈希 + 路径）
fn open_regulation_db<R: tauri::Runtime>(
    app: &AppHandle<R>,
) -> Result<rusqlite::Connection, String> {
    let app_data_dir =
        app.path().app_data_dir().map_err(|e| format!("获取应用数据目录失败: {}", e))?;
    let db_path = app_data_dir.join("history.db");
    let conn =
        rusqlite::Connection::open(&db_path).map_err(|e| format!("打开数据库失败: {}", e))?;

    regulation_db::init_regulation_schema(&conn).map_err(|e| format!("初始化规章表失败: {}", e))?;
    Ok(conn)
}

fn open_db_and_load_dedup_data<R: tauri::Runtime>(
    app: &AppHandle<R>,
) -> Result<(rusqlite::Connection, HashSet<String>, HashSet<String>), String> {
    let conn = open_regulation_db(app)?;

    let existing_hashes: HashSet<String> = {
        let mut stmt = conn
            .prepare("SELECT sha256 FROM regulation_files")
            .map_err(|e| format!("查询已有哈希失败: {}", e))?;
        let rows: HashSet<String> = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| format!("读取哈希失败: {}", e))?
            .filter_map(|r| r.ok())
            .collect();
        rows
    };

    let existing_paths: HashSet<String> = {
        let mut stmt = conn
            .prepare("SELECT file_path FROM regulation_files")
            .map_err(|e| format!("查询已有路径失败: {}", e))?;
        let rows: HashSet<String> = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| format!("读取路径失败: {}", e))?
            .filter_map(|r| r.ok())
            .collect();
        rows
    };

    info!("已有 {} 个哈希 + {} 个路径用于去重", existing_hashes.len(), existing_paths.len());
    Ok((conn, existing_hashes, existing_paths))
}

/// 对单个文件执行 OCR 处理（先尝试 pdfium 文本提取，不足时使用 PP-OCRv4）
///
/// 返回 `true` 表示成功（文本已写入索引），`false` 表示失败。
fn ocr_single_file<R: tauri::Runtime>(
    app: &AppHandle<R>,
    conn: &rusqlite::Connection,
    index_state: &State<'_, RegulationIndexState>,
    file: &regulation_db::RegulationFile,
    cancel_flag: Option<Arc<AtomicBool>>,
) -> bool {
    if cancel_flag.as_ref().is_some_and(cancel_requested) {
        return false;
    }

    // 更新状态为 processing
    let _ = regulation_db::update_ocr_status(conn, file.id, "processing", 0, 0, None);

    // Step 1: 先尝试 pdfium 文本提取
    let pdf_path = std::path::Path::new(&file.file_path);
    let extraction = text_extractor::extract_text_from_pdf(pdf_path);

    match extraction {
        Ok(result) if !result.needs_ocr => {
            match write_to_index(index_state, conn, file, result.text, "pdfium") {
                Ok(()) => {
                    info!("pdfium 文本提取成功: {}", file.title);
                    return true;
                }
                Err(e) => {
                    let _ =
                        regulation_db::update_ocr_status(conn, file.id, "failed", 0, 0, Some(&e));
                    return false;
                }
            }
        }
        _ => {}
    }

    // Step 2: 使用 Rust 原生 PDF OCR（pdfium + PP-OCRv4）
    info!("使用 OCR 处理: {}", file.title);
    let app_clone = app.clone();
    let file_title = file.title.clone();

    match super::pdf_ocr::ocr_pdf_with_cancel(
        &file.file_path,
        50,
        cancel_flag,
        Some(&|current_page, total_pages| {
            if let Err(e) = app_clone.emit(
                "regulation:ocr-progress",
                serde_json::json!({
                    "current": file_title,
                    "current_page": current_page,
                    "total_pages": total_pages,
                }),
            ) {
                tracing::debug!("发送 OCR 页面进度事件失败: {}", e);
            }
        }),
    ) {
        Ok(ocr_result) if ocr_result.success && !ocr_result.text.is_empty() => {
            if ocr_result.page_count > 0 {
                let _ =
                    regulation_db::update_page_count(conn, file.id, ocr_result.page_count as i32);
            }
            match write_to_index(index_state, conn, file, ocr_result.text, "pp_ocrv4") {
                Ok(()) => {
                    info!(
                        "OCR 成功: {} ({}页, OCR {}页, {:.2}s)",
                        file.title, ocr_result.page_count, ocr_result.ocr_pages, ocr_result.elapsed
                    );
                    true
                }
                Err(e) => {
                    let _ =
                        regulation_db::update_ocr_status(conn, file.id, "failed", 0, 0, Some(&e));
                    false
                }
            }
        }
        Ok(ocr_result) => {
            let error_msg = if ocr_result.error.is_empty() {
                "OCR 未能提取到文本".to_string()
            } else {
                ocr_result.error
            };
            let _ =
                regulation_db::update_ocr_status(conn, file.id, "failed", 0, 0, Some(&error_msg));
            warn!("OCR 无文本: {} - {}", file.title, error_msg);
            false
        }
        Err(e) => {
            if matches!(e, super::pdf_ocr::PdfOcrError::Cancelled) {
                let _ = regulation_db::update_ocr_status(
                    conn,
                    file.id,
                    "pending",
                    0,
                    0,
                    Some("OCR 已中止，等待下次处理"),
                );
                info!("OCR 已中止: {}", file.title);
                return false;
            }
            let _ = regulation_db::update_ocr_status(
                conn,
                file.id,
                "failed",
                0,
                0,
                Some(&format!("OCR 失败: {}", e)),
            );
            warn!("OCR 失败: {} - {}", file.title, e);
            false
        }
    }
}

fn load_mineru_ocr_options() -> Option<super::mineru_ocr::MineruOcrOptions> {
    let config = get_cached_config()?;
    let advanced = config.advanced;
    if !advanced.mineru_ocr_enabled || !advanced.mineru_ocr_prefer_online {
        return None;
    }

    let api_key = advanced.mineru_api_key.trim();
    if api_key.is_empty() {
        return None;
    }

    Some(super::mineru_ocr::MineruOcrOptions::chinese(api_key.to_string()))
}

async fn ocr_single_file_with_online_fallback<R: tauri::Runtime>(
    app: &AppHandle<R>,
    index_state: &State<'_, RegulationIndexState>,
    file: &regulation_db::RegulationFile,
    mineru_options: Option<&super::mineru_ocr::MineruOcrOptions>,
    cancel_flag: Arc<AtomicBool>,
) -> bool {
    if cancel_requested(&cancel_flag) {
        return false;
    }

    if mineru_options.is_none() {
        return match open_regulation_db(app) {
            Ok(conn) => ocr_single_file(app, &conn, index_state, file, Some(cancel_flag)),
            Err(e) => {
                warn!("打开数据库失败，无法执行本地 OCR: {} - {}", file.title, e);
                false
            }
        };
    }

    let pdf_path = Path::new(&file.file_path);

    match text_extractor::extract_text_from_pdf(pdf_path) {
        Ok(result) if !result.needs_ocr => {
            match open_regulation_db(app)
                .and_then(|conn| write_to_index(index_state, &conn, file, result.text, "pdfium"))
            {
                Ok(()) => {
                    info!("pdfium 文本提取成功: {}", file.title);
                    return true;
                }
                Err(e) => {
                    if let Ok(conn) = open_regulation_db(app) {
                        let _ = regulation_db::update_ocr_status(
                            &conn,
                            file.id,
                            "failed",
                            0,
                            0,
                            Some(&e),
                        );
                    }
                    return false;
                }
            }
        }
        Ok(_) | Err(_) => {}
    }

    if cancel_requested(&cancel_flag) {
        return false;
    }

    if let Some(options) = mineru_options {
        if let Ok(conn) = open_regulation_db(app) {
            let _ = regulation_db::update_ocr_status(&conn, file.id, "processing", 0, 0, None);
        }
        let _ = app.emit(
            "regulation:ocr-progress",
            serde_json::json!({
                "current": format!("MinerU 在线 OCR: {}", file.title),
                "validity": file.validity,
            }),
        );

        info!("使用 MinerU 在线 OCR 处理: {}", file.title);
        match super::mineru_ocr::ocr_pdf_to_markdown_with_cancel(
            pdf_path,
            options,
            cancel_flag.clone(),
        )
        .await
        {
            Ok(text) if !text.trim().is_empty() => {
                match open_regulation_db(app)
                    .and_then(|conn| write_to_index(index_state, &conn, file, text, "mineru"))
                {
                    Ok(()) => {
                        info!("MinerU 在线 OCR 成功: {}", file.title);
                        return true;
                    }
                    Err(e) => {
                        warn!("MinerU OCR 结果写入索引失败，回退本地 OCR: {} - {}", file.title, e);
                    }
                }
            }
            Ok(_) => {
                warn!("MinerU 在线 OCR 返回空文本，回退本地 OCR: {}", file.title);
            }
            Err(e) => {
                if cancel_requested(&cancel_flag) {
                    if let Ok(conn) = open_regulation_db(app) {
                        let _ = regulation_db::update_ocr_status(
                            &conn,
                            file.id,
                            "pending",
                            0,
                            0,
                            Some("OCR 已中止，等待下次处理"),
                        );
                    }
                    info!("MinerU 在线 OCR 已中止: {}", file.title);
                    return false;
                }
                warn!("MinerU 在线 OCR 失败，回退本地 OCR: {} - {}", file.title, e);
            }
        }
    }

    if cancel_requested(&cancel_flag) {
        return false;
    }

    match open_regulation_db(app) {
        Ok(conn) => ocr_single_file(app, &conn, index_state, file, Some(cancel_flag)),
        Err(e) => {
            warn!("打开数据库失败，无法执行本地 OCR: {} - {}", file.title, e);
            false
        }
    }
}

/// 处理本地法规文件列表：去重、文本提取、入库、入索引
///
/// 返回 `(new_files, duplicates, indexed, needs_ocr, failed)`
fn process_local_file_batch<R: tauri::Runtime>(
    app: &AppHandle<R>,
    source_files: &[PathBuf],
    conn: &rusqlite::Connection,
    index_state: &State<'_, RegulationIndexState>,
    existing_hashes: &HashSet<String>,
    existing_paths: &HashSet<String>,
    copy_mode: LocalCopyMode,
    target_dir: &Path,
) -> Result<(usize, usize, usize, usize, usize), String> {
    let total_found = source_files.len();
    let mut new_files = 0;
    let mut duplicates = 0;
    let mut indexed = 0;
    let mut needs_ocr = 0;
    let mut failed = 0;
    let mut batch_docs = Vec::new();
    let batch_commit_size = 20;

    for (i, source_path) in source_files.iter().enumerate() {
        let filename = source_path.file_name().unwrap_or_default().to_string_lossy().to_string();
        let file_path_str = source_path.to_string_lossy().to_string();
        let is_pdf = is_pdf_path(source_path);
        let is_txt = is_txt_path(source_path);

        // 防御性扩展名校验：仅允许 PDF / TXT，避免历史上把 .doc/.docx 等
        // 误录入 regulation_files 后续被 OCR / 清理流程误处理。
        if !is_supported_local_scan_path(source_path) {
            warn!("跳过不支持的本地法规文件: {}", file_path_str);
            continue;
        }

        // 发送进度（每 5 个或最后一个）
        if i % 5 == 0 || i == total_found - 1 {
            if let Err(e) = app.emit(
                "regulation:scan-progress",
                ScanProgress {
                    scanned: i + 1,
                    total_found,
                    new_files,
                    duplicates,
                    indexed,
                    needs_ocr,
                    failed,
                    current_file: Some(file_path_str.clone()),
                    phase: "processing".to_string(),
                    ocr_processed: None,
                    ocr_total: None,
                },
            ) {
                debug!("发送扫描进度事件失败: {}", e);
            }
        }

        // RegisterOnly 模式按路径去重
        if copy_mode == LocalCopyMode::RegisterOnly && existing_paths.contains(&file_path_str) {
            duplicates += 1;
            continue;
        }

        // 计算 SHA256
        let sha256 = match calculate_file_hash(source_path) {
            Ok(hash) => hash,
            Err(e) => {
                failed += 1;
                warn!("计算哈希失败: {} - {}", filename, e);
                continue;
            }
        };

        // 哈希去重
        if existing_hashes.contains(&sha256) {
            duplicates += 1;
            continue;
        }

        // 先从文件名解析元数据，便于生成规则化的存储文件名
        let (title, doc_number, doc_type) = parse_filename_metadata(&filename);
        let validity = infer_filename_validity(&filename);

        // 计算存储路径（CopyThenRegister 模式使用 doc_number_title 规则命名）
        let stored_path = match resolve_storage_path(
            source_path,
            &sha256,
            Some(doc_number.as_str()),
            Some(title.as_str()),
            copy_mode,
            target_dir,
        ) {
            Ok(path) => path,
            Err(e) => {
                failed += 1;
                warn!("解析存储路径失败: {} - {}", filename, e);
                continue;
            }
        };
        let stored_path_str = stored_path.to_string_lossy().to_string();

        // 获取文件大小
        let file_size = std::fs::metadata(&stored_path).map(|m| m.len() as i64).unwrap_or(0);

        let (content, text_status, text_needs_ocr, initial_engine) = if is_txt {
            match load_local_text_content(&stored_path) {
                Ok(content) => (content, "done", false, "plain_text"),
                Err(e) => {
                    failed += 1;
                    warn!("读取文本文件失败: {} - {}", filename, e);
                    continue;
                }
            }
        } else if is_pdf {
            match text_extractor::extract_text_from_pdf(&stored_path) {
                Ok(extraction) if !extraction.needs_ocr => (extraction.text, "done", false, "pdfium"),
                Ok(extraction) => (extraction.text, "pending", true, "unknown"),
                Err(e) => {
                    debug!("文本提取失败: {} - {}", filename, e);
                    (String::new(), "pending", true, "unknown")
                }
            }
        } else {
            failed += 1;
            warn!("无法识别本地法规文件类型: {}", file_path_str);
            continue;
        };

        let file_url = format!("file:///{}", stored_path_str.replace('\\', "/"));

        // 插入数据库
        // PDF 直接提取成功时记录为 pdfium，TXT 直接记为 plain_text，
        // 其余待 OCR 的文件保持 unknown，后续 OCR 成功时由 write_to_index 更新。
        let db_file = regulation_db::RegulationFile {
            title: title.clone(),
            doc_number: doc_number.clone(),
            doc_type: doc_type.clone(),
            validity: validity.clone(),
            url: file_url.clone(),
            pdf_url: None,
            sha256: sha256.clone(),
            file_path: stored_path_str.clone(),
            file_size,
            ocr_status: text_status.to_string(),
            ocr_engine: initial_engine.to_string(),
            ..Default::default()
        };

        match regulation_db::insert_file(conn, &db_file) {
            Ok(file_id) => {
                new_files += 1;
                if !text_needs_ocr {
                    let reg_doc = super::schema::RegulationDocument {
                        title,
                        doc_number,
                        validity,
                        doc_type,
                        office_unit: String::new(),
                        sign_date: String::new(),
                        publish_date: String::new(),
                        url: file_url,
                        file_path: stored_path_str,
                        content,
                    };
                    batch_docs.push((file_id, reg_doc));
                    indexed += 1;
                } else {
                    needs_ocr += 1;
                }
            }
            Err(e) => {
                failed += 1;
                warn!("插入数据库失败: {} - {}", filename, e);
            }
        }

        // 批量提交索引
        if batch_docs.len() >= batch_commit_size {
            commit_batch_to_index(&batch_docs, index_state, conn)?;
            batch_docs.clear();
        }
    }

    // 提交剩余文档
    if !batch_docs.is_empty() {
        commit_batch_to_index(&batch_docs, index_state, conn)?;
    }

    Ok((new_files, duplicates, indexed, needs_ocr, failed))
}

/// 自动 OCR 处理待 OCR 文件，发送扫描进度事件
///
/// 返回 `(ocr_success, ocr_failed)`
fn run_auto_ocr<R: tauri::Runtime>(
    app: &AppHandle<R>,
    conn: &rusqlite::Connection,
    index_state: &State<'_, RegulationIndexState>,
    needs_ocr_count: usize,
    total_found: usize,
    new_files: usize,
    duplicates: usize,
    indexed: usize,
    failed: usize,
) -> Result<(usize, usize), String> {
    info!("开始自动 OCR 处理 {} 个扫描版文件", needs_ocr_count);

    // 发送 OCR 阶段进度
    if let Err(e) = app.emit(
        "regulation:scan-progress",
        ScanProgress {
            scanned: total_found,
            total_found,
            new_files,
            duplicates,
            indexed,
            needs_ocr: needs_ocr_count,
            failed,
            current_file: Some("准备 OCR 处理...".to_string()),
            phase: "ocr".to_string(),
            ocr_processed: Some(0),
            ocr_total: Some(needs_ocr_count),
        },
    ) {
        debug!("发送 OCR 阶段进度事件失败: {}", e);
    }

    let pending_files = regulation_db::get_pending_ocr_files(conn, needs_ocr_count)
        .map_err(|e| format!("获取待 OCR 文件失败: {}", e))?;

    let mut ocr_success = 0;
    let mut ocr_failed = 0;

    for (i, file) in pending_files.iter().enumerate() {
        // 发送 OCR 进度
        if let Err(e) = app.emit(
            "regulation:scan-progress",
            ScanProgress {
                scanned: total_found,
                total_found,
                new_files,
                duplicates,
                indexed,
                needs_ocr: needs_ocr_count,
                failed,
                current_file: Some(file.title.clone()),
                phase: "ocr".to_string(),
                ocr_processed: Some(i),
                ocr_total: Some(pending_files.len()),
            },
        ) {
            debug!("发送 OCR 进度事件失败: {}", e);
        }

        if ocr_single_file(app, conn, index_state, file, None) {
            ocr_success += 1;
        } else {
            ocr_failed += 1;
        }
    }

    info!("自动 OCR 完成: 成功 {}, 失败 {}", ocr_success, ocr_failed);
    Ok((ocr_success, ocr_failed))
}

// ============================================================================
// 本地目录扫描命令
// ============================================================================

/// 扫描进度事件
#[derive(Debug, Clone, Serialize)]
pub struct ScanProgress {
    /// 已扫描文件数
    pub scanned: usize,
    /// 发现的受支持文件总数（PDF / TXT）
    pub total_found: usize,
    /// 新文件数（非重复）
    pub new_files: usize,
    /// 重复文件数（SHA256 一致）
    pub duplicates: usize,
    /// 已索引数
    pub indexed: usize,
    /// 需要 OCR 数
    pub needs_ocr: usize,
    /// 失败数
    pub failed: usize,
    /// 当前正在处理的文件名
    pub current_file: Option<String>,
    /// 当前阶段：discovering / processing / ocr / done
    pub phase: String,
    /// OCR 已处理数（ocr 阶段使用）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ocr_processed: Option<usize>,
    /// OCR 总数（ocr 阶段使用）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ocr_total: Option<usize>,
}

/// 扫描结果
#[derive(Debug, Serialize)]
pub struct ScanResponse {
    /// 发现的受支持文件总数（PDF / TXT）
    pub total_found: usize,
    /// 新文件数
    pub new_files: usize,
    /// 重复文件数
    pub duplicates: usize,
    /// 已索引数（文本提取直接索引）
    pub indexed: usize,
    /// 需要 OCR 数
    pub needs_ocr: usize,
    /// 失败数
    pub failed: usize,
    /// 不支持的文件数（保留兼容字段名）
    pub skipped_non_pdf: usize,
    /// OCR 成功索引数
    pub ocr_success: usize,
    /// OCR 失败数
    pub ocr_failed: usize,
}

/// 从文件名解析规章元数据
fn parse_filename_metadata(filename: &str) -> (String, String, String) {
    use std::sync::LazyLock;

    // 移除扩展名
    let name = filename.rsplit_once('.').map(|(n, _)| n).unwrap_or(filename);

    // 缓存编译后的正则表达式，避免每次调用重复编译
    static DOC_NUMBER_PATTERNS: LazyLock<Vec<regex::Regex>> = LazyLock::new(|| {
        vec![
            // AC-xxx-xxx 格式（咨询通告）
            regex::Regex::new(r"(AC-\d+[-\w]*(?:R\d+)?)").expect("AC regex pattern invalid"),
            // CCAR-xxx 格式（民航规章）
            regex::Regex::new(r"(CCAR-\d+[-\w]*)").expect("CCAR regex pattern invalid"),
            // IB-xxx 格式（信息通告）
            regex::Regex::new(r"(IB-[\w-]+)").expect("IB regex pattern invalid"),
            // MD-xxx 格式（管理文件）
            regex::Regex::new(r"(MD-[\w-]+)").expect("MD regex pattern invalid"),
            // AP-xxx 格式（管理程序）
            regex::Regex::new(r"(AP-\d+[-\w]*)").expect("AP regex pattern invalid"),
            // OSB-xxx 格式
            regex::Regex::new(r"(OSB-[\w-]+)").expect("OSB regex pattern invalid"),
            // MHT/MH 格式（民航行业标准）
            regex::Regex::new(r"(MH[T]?\d+[-\w]*)").expect("MH/MHT regex pattern invalid"),
        ]
    });

    let mut doc_number = String::new();
    for pattern in DOC_NUMBER_PATTERNS.iter() {
        if let Some(caps) = pattern.captures(name) {
            doc_number = caps.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
            break;
        }
    }

    // 推断文档类型
    let doc_type = if name.contains("CCAR-") || name.contains("规章") || name.contains("规则") {
        "regulation".to_string()
    } else if name.contains("AC-") || name.contains("咨询通告") {
        "advisory_circular".to_string()
    } else if name.contains("IB-") || name.contains("信息通告") {
        "information_bulletin".to_string()
    } else if name.contains("MD-") || name.contains("管理文件") {
        "management_document".to_string()
    } else if name.contains("AP-") || name.contains("管理程序") {
        "administrative_procedure".to_string()
    } else if name.contains("MHT")
        || name.contains("MH/T")
        || name.contains("MH ")
        || name.contains("标准")
    {
        "standard".to_string()
    } else {
        "normative".to_string()
    };

    // 标题：使用清理后的文件名
    let title = name.replace('_', " ").trim().to_string();

    (title, doc_number, doc_type)
}

fn infer_filename_validity(filename: &str) -> String {
    let name = filename.rsplit_once('.').map(|(n, _)| n).unwrap_or(filename);

    infer_text_validity(name).unwrap_or("有效").to_string()
}

fn infer_text_validity(text: &str) -> Option<&'static str> {
    INVALID_VALIDITY_LABELS.iter().copied().find(|label| text.contains(label))
}

fn infer_document_validity(doc: &RegulationDocument) -> Option<&'static str> {
    infer_text_validity(&doc.title)
        .or_else(|| infer_text_validity(&doc.doc_number))
        .or_else(|| infer_text_validity(&doc.file_path))
        .or_else(|| infer_text_validity(&doc.url))
}

fn normalize_document_validities(docs: &mut [RegulationDocument]) {
    for doc in docs {
        if let Some(inferred) = infer_document_validity(doc) {
            doc.validity = inferred.to_string();
        }
    }
}

fn is_effectively_invalid(doc: &RegulationDocument) -> bool {
    INVALID_VALIDITY_LABELS.contains(&doc.validity.trim()) || infer_document_validity(doc).is_some()
}

/// 长任务取消状态（Tauri 管理）。
///
/// Tauri command 本身不能从前端直接 abort；前端点击“中止”时设置这些标记，
/// 后端长循环在安全边界检查并尽快退出。
pub struct RegulationTaskCancelState {
    pub ocr: Arc<AtomicBool>,
    pub sync_compare: Arc<AtomicBool>,
    pub full_sync: Arc<AtomicBool>,
}

impl Default for RegulationTaskCancelState {
    fn default() -> Self {
        Self {
            ocr: Arc::new(AtomicBool::new(false)),
            sync_compare: Arc::new(AtomicBool::new(false)),
            full_sync: Arc::new(AtomicBool::new(false)),
        }
    }
}

fn cancel_requested(flag: &Arc<AtomicBool>) -> bool {
    flag.load(Ordering::Relaxed)
}

fn reset_cancel_flag(flag: &Arc<AtomicBool>) {
    flag.store(false, Ordering::Relaxed);
}

async fn wait_for_cancel(flag: Arc<AtomicBool>) {
    while !cancel_requested(&flag) {
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    }
}

fn normalize_scan_filter_path(path: &str) -> String {
    let mut normalized = path.trim().replace('/', "\\");

    while normalized.len() > 3 && normalized.ends_with('\\') {
        normalized.pop();
    }

    normalized.to_lowercase()
}

fn normalize_scan_folder_filters(scan_folders: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut normalized = Vec::new();

    for folder in scan_folders {
        let path = normalize_scan_filter_path(folder);
        if path.is_empty() || !seen.insert(path.clone()) {
            continue;
        }
        normalized.push(path);
    }

    normalized
}

fn is_document_in_normalized_scan_folders(
    doc: &RegulationDocument,
    normalized_scan_folders: &[String],
) -> bool {
    if normalized_scan_folders.is_empty() {
        return true;
    }

    let file_path = normalize_scan_filter_path(&doc.file_path);
    if file_path.is_empty() {
        return false;
    }

    normalized_scan_folders.iter().any(|folder| {
        if file_path == *folder {
            return true;
        }

        if folder.ends_with('\\') {
            file_path.starts_with(folder)
        } else {
            file_path
                .strip_prefix(folder.as_str())
                .is_some_and(|rest| rest.starts_with('\\'))
        }
    })
}

#[cfg(test)]
fn is_document_in_scan_folders(doc: &RegulationDocument, scan_folders: &[String]) -> bool {
    let normalized = normalize_scan_folder_filters(scan_folders);
    is_document_in_normalized_scan_folders(doc, &normalized)
}

#[cfg(test)]
mod validity_tests {
    use super::*;

    fn doc(title: &str, validity: &str) -> RegulationDocument {
        RegulationDocument {
            title: title.to_string(),
            doc_number: String::new(),
            validity: validity.to_string(),
            doc_type: "regulation".to_string(),
            office_unit: String::new(),
            sign_date: String::new(),
            publish_date: String::new(),
            url: String::new(),
            file_path: String::new(),
            content: String::new(),
        }
    }

    #[test]
    fn title_marker_overrides_blank_validity() {
        let mut docs = vec![doc("废止!CCAR-121-R7 交通运输部决定", "")];

        normalize_document_validities(&mut docs);

        assert_eq!(docs[0].validity, "废止");
        assert!(is_effectively_invalid(&docs[0]));
    }

    #[test]
    fn valid_document_without_invalid_marker_stays_valid() {
        let doc = doc("CCAR-121-R7 大型飞机公共航空运输承运人运行合格审定规则", "有效");

        assert!(!is_effectively_invalid(&doc));
    }

    #[test]
    fn scan_folder_filter_matches_only_inside_selected_roots() {
        let mut doc = doc("CCAR-121", "有效");
        doc.file_path = r"D:\Regs\CCAR-121.pdf".to_string();

        assert!(is_document_in_scan_folders(&doc, &[r"D:\Regs".to_string()]));
        assert!(is_document_in_scan_folders(&doc, &[r"D:/Regs/".to_string()]));
        assert!(!is_document_in_scan_folders(&doc, &[r"D:\Regs-old".to_string()]));
    }

    #[test]
    fn scan_folder_filter_ignores_empty_filter_list() {
        let mut doc = doc("CCAR-121", "有效");
        doc.file_path = r"D:\Regs\CCAR-121.pdf".to_string();

        assert!(is_document_in_scan_folders(&doc, &[]));
        assert!(is_document_in_scan_folders(&doc, &[" ".to_string()]));
    }
}

/// 应跳过的 Windows 系统/保护目录（小写匹配）
const SKIP_DIRS: &[&str] = &[
    "$recycle.bin",
    "system volume information",
    "$windows.~bt",
    "$windows.~ws",
    "windows",
    "windows.old",
    "recovery",
    "perflogs",
    "config.msi",
    "appdata",
    "application data",
    "local settings",
    "temporary internet files",
    "inetcache",
    "recent",
    "sendto",
    "printhood",
    "templates",
    "nethood",
    "cookies",
    ".git",
    "node_modules",
    "__pycache__",
];

/// 判断给定路径的扩展名是否为 PDF（大小写不敏感）。
///
/// 该函数用于 PDF 专属流程；本地扫描的完整支持范围见
/// [`is_supported_local_scan_path`]。
pub(crate) fn is_pdf_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("pdf"))
        .unwrap_or(false)
}

fn is_txt_path(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("txt"))
        .unwrap_or(false)
}

fn is_supported_local_scan_path(path: &std::path::Path) -> bool {
    is_pdf_path(path) || is_txt_path(path)
}

fn is_supported_realign_filename_path(path: &std::path::Path) -> bool {
    is_supported_local_scan_path(path)
}

fn is_invalid_regulation_file_record(path: &std::path::Path) -> bool {
    path.as_os_str().is_empty() || !is_supported_local_scan_path(path) || !path.exists()
}

fn load_local_text_content(path: &std::path::Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("读取文本文件失败: {}", e))?;
    let mut content = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8_lossy(&bytes[3..]).into_owned()
    } else if bytes.starts_with(&[0xFF, 0xFE]) {
        let (decoded, _, _) = encoding_rs::UTF_16LE.decode(&bytes[2..]);
        decoded.into_owned()
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        let (decoded, _, _) = encoding_rs::UTF_16BE.decode(&bytes[2..]);
        decoded.into_owned()
    } else {
        match String::from_utf8(bytes) {
            Ok(text) => text,
            Err(err) => {
                let bytes = err.into_bytes();
                let (decoded, _, _) = encoding_rs::GBK.decode(&bytes);
                decoded.into_owned()
            }
        }
    };
    if content.starts_with('\u{feff}') {
        content = content.trim_start_matches('\u{feff}').to_string();
    }
    Ok(content)
}

/// 递归收集目录下的所有受支持文件（PDF / TXT）
///
/// 使用 walkdir 遍历，自动跳过系统保护目录和符号链接，权限错误静默处理。
fn collect_supported_local_files(
    dir: &std::path::Path,
    recursive: bool,
) -> Vec<std::path::PathBuf> {
    let max_depth = if recursive { usize::MAX } else { 1 };

    WalkDir::new(dir)
        .follow_links(false)       // 不跟踪符号链接 / Junction Points
        .min_depth(1)              // 跳过根目录本身
        .max_depth(max_depth)
        .into_iter()
        .filter_entry(|e| {
            // 跳过系统保护目录（整个子树剪枝）
            if e.file_type().is_dir() {
                let name_lower = e.file_name().to_string_lossy().to_lowercase();
                !SKIP_DIRS.contains(&name_lower.as_str())
            } else {
                true
            }
        })
        .filter_map(|entry_result| match entry_result {
            Ok(entry) => {
                let path = entry.into_path();
                if path.is_file() && is_supported_local_scan_path(&path) {
                    return Some(path);
                }
                None
            }
            Err(_e) => {
                // 权限错误等静默跳过
                debug!("跳过无法访问的路径: {}", _e);
                None
            }
        })
        .collect()
}

/// 扫描本地目录，将本地法规文件入库 + 入索引 + 自动 OCR
///
/// # 参数
/// - `dir_path`: 要扫描的目录路径
/// - `recursive`: 是否递归扫描子目录
/// - `auto_ocr`: 是否自动对扫描版 PDF 执行 OCR（默认 true）
///
/// # 流程
/// 1. 递归遍历目录，收集所有受支持文件（PDF / TXT）
/// 2. 对每个文件计算 SHA256
/// 3. 数据库去重检查（同哈希 = 同文件，跳过）
/// 4. 从文件名智能解析文号、类型等元数据
/// 5. 对 PDF 用 pdfium-render 提取文本；TXT 直接读取正文
/// 6. 写入 SQLite (regulation_files) + Tantivy 索引
/// 7. 对文本不足的扫描版 PDF 自动执行 OCR（PP-OCRv4）
/// 8. 实时通过 Tauri event "regulation:scan-progress" 报告进度
#[tauri::command]
pub async fn regulation_scan_local_dir<R: tauri::Runtime>(
    app: AppHandle<R>,
    dir_path: String,
    recursive: Option<bool>,
    auto_ocr: Option<bool>,
    local_copy_mode: Option<String>,
    target_dir: Option<String>,
    index_state: State<'_, RegulationIndexState>,
) -> Result<ScanResponse, String> {
    let recursive = recursive.unwrap_or(true);
    let auto_ocr = auto_ocr.unwrap_or(true);
    let copy_mode = LocalCopyMode::from_optional(local_copy_mode.as_deref());
    let target_dir = resolve_target_dir(&app, target_dir.as_deref())?;
    info!(
        "开始扫描本地目录: {}, 递归: {}, 自动OCR: {}, 导入模式: {:?}, 目标目录: {:?}",
        dir_path, recursive, auto_ocr, copy_mode, target_dir
    );

    let dir = std::path::Path::new(&dir_path);
    if !dir.exists() || !dir.is_dir() {
        return Err(format!("目录不存在或不是目录: {}", dir_path));
    }

    // Phase 1: 发现所有受支持文件
    emit_scan_progress(
        &app,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        Some("正在扫描目录...".to_string()),
        "discovering",
        None,
        None,
    );

    let source_files = collect_supported_local_files(dir, recursive);
    let total_found = source_files.len();
    info!("发现 {} 个受支持文件", total_found);

    if total_found == 0 {
        return Ok(ScanResponse {
            total_found: 0,
            new_files: 0,
            duplicates: 0,
            indexed: 0,
            needs_ocr: 0,
            failed: 0,
            skipped_non_pdf: 0,
            ocr_success: 0,
            ocr_failed: 0,
        });
    }

    // 获取数据库连接 + 加载去重数据
    let (conn, existing_hashes, existing_paths) = open_db_and_load_dedup_data(&app)?;

    // Phase 2: 处理本地文件
    let (new_files, duplicates, indexed, needs_ocr, failed) = process_local_file_batch(
        &app,
        &source_files,
        &conn,
        &index_state,
        &existing_hashes,
        &existing_paths,
        copy_mode,
        &target_dir,
    )?;

    info!(
        "扫描完成: 发现 {}, 新增 {}, 重复 {}, 索引 {}, 需OCR {}, 失败 {}",
        total_found, new_files, duplicates, indexed, needs_ocr, failed
    );

    // Phase 3: 自动 OCR
    let (ocr_success, ocr_failed) = if auto_ocr && needs_ocr > 0 {
        run_auto_ocr(
            &app,
            &conn,
            &index_state,
            needs_ocr,
            total_found,
            new_files,
            duplicates,
            indexed,
            failed,
        )?
    } else {
        (0, 0)
    };

    // 发送最终进度
    emit_scan_progress(
        &app,
        total_found,
        total_found,
        new_files,
        duplicates,
        indexed,
        needs_ocr,
        failed,
        None,
        "done",
        if needs_ocr > 0 { Some(ocr_success + ocr_failed) } else { None },
        if needs_ocr > 0 { Some(needs_ocr) } else { None },
    );

    info!(
        "全部完成: 发现 {}, 新增 {}, 索引 {}, OCR成功 {}, OCR失败 {}, 跳过 {}",
        total_found, new_files, indexed, ocr_success, ocr_failed, duplicates
    );

    Ok(ScanResponse {
        total_found,
        new_files,
        duplicates,
        indexed,
        needs_ocr,
        failed,
        skipped_non_pdf: 0,
        ocr_success,
        ocr_failed,
    })
}

// ============================================================================
// 同步对比命令
// ============================================================================

/// 枚举 Windows 所有可用盘符
fn enumerate_drives() -> Vec<PathBuf> {
    (b'A'..=b'Z')
        .map(|c| PathBuf::from(format!("{}:\\", c as char)))
        .filter(|p| p.exists() && p.is_dir())
        .collect()
}

/// 扫描全盘所有受支持文件，入库 + 入索引 + 自动 OCR
///
/// 遍历 Windows 所有盘符，对每个盘递归收集受支持文件，
/// 复用共享的去重/提取/索引/OCR 流程。
///
/// # 参数
/// - `auto_ocr`: 是否自动对扫描版 PDF 执行 OCR（默认 true）
#[tauri::command]
pub async fn regulation_scan_all_drives<R: tauri::Runtime>(
    app: AppHandle<R>,
    auto_ocr: Option<bool>,
    index_state: State<'_, RegulationIndexState>,
) -> Result<ScanResponse, String> {
    let auto_ocr = auto_ocr.unwrap_or(true);
    let drives = enumerate_drives();
    info!("全盘扫描: 发现 {} 个盘符: {:?}", drives.len(), drives);

    if drives.is_empty() {
        return Err("未发现任何可用盘符".to_string());
    }

    // Phase 1: 从所有盘符收集受支持文件
    emit_scan_progress(
        &app,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        Some("正在扫描全盘...".to_string()),
        "discovering",
        None,
        None,
    );

    let mut all_source_files = Vec::new();
    for drive in &drives {
        info!("扫描盘符: {}", drive.display());
        emit_scan_progress(
            &app,
            0,
            all_source_files.len(),
            0,
            0,
            0,
            0,
            0,
            Some(format!("正在扫描 {} ...", drive.display())),
            "discovering",
            None,
            None,
        );
        let files = collect_supported_local_files(drive, true);
        info!("盘符 {} 发现 {} 个受支持文件", drive.display(), files.len());
        all_source_files.extend(files);
    }

    let total_found = all_source_files.len();
    info!("全盘共发现 {} 个受支持文件", total_found);

    if total_found == 0 {
        return Ok(ScanResponse {
            total_found: 0,
            new_files: 0,
            duplicates: 0,
            indexed: 0,
            needs_ocr: 0,
            failed: 0,
            skipped_non_pdf: 0,
            ocr_success: 0,
            ocr_failed: 0,
        });
    }

    // 获取数据库连接 + 加载去重数据
    let (conn, existing_hashes, existing_paths) = open_db_and_load_dedup_data(&app)?;

    // Phase 2: 处理受支持文件（全盘扫描使用 RegisterOnly）
    let copy_mode = LocalCopyMode::RegisterOnly;
    let target_dir = resolve_target_dir(&app, None)?;
    let (new_files, duplicates, indexed, needs_ocr, failed) = process_local_file_batch(
        &app,
        &all_source_files,
        &conn,
        &index_state,
        &existing_hashes,
        &existing_paths,
        copy_mode,
        &target_dir,
    )?;

    info!(
        "全盘扫描处理完成: 发现 {}, 新增 {}, 重复 {}, 索引 {}, 需OCR {}, 失败 {}",
        total_found, new_files, duplicates, indexed, needs_ocr, failed
    );

    // Phase 3: 自动 OCR
    let (ocr_success, ocr_failed) = if auto_ocr && needs_ocr > 0 {
        run_auto_ocr(
            &app,
            &conn,
            &index_state,
            needs_ocr,
            total_found,
            new_files,
            duplicates,
            indexed,
            failed,
        )?
    } else {
        (0, 0)
    };

    // 发送最终进度
    emit_scan_progress(
        &app,
        total_found,
        total_found,
        new_files,
        duplicates,
        indexed,
        needs_ocr,
        failed,
        None,
        "done",
        if needs_ocr > 0 { Some(ocr_success + ocr_failed) } else { None },
        if needs_ocr > 0 { Some(needs_ocr) } else { None },
    );

    info!(
        "全盘扫描全部完成: 发现 {}, 新增 {}, 索引 {}, OCR成功 {}, OCR失败 {}, 跳过 {}",
        total_found, new_files, indexed, ocr_success, ocr_failed, duplicates
    );

    Ok(ScanResponse {
        total_found,
        new_files,
        duplicates,
        indexed,
        needs_ocr,
        failed,
        skipped_non_pdf: 0,
        ocr_success,
        ocr_failed,
    })
}

// ============================================================================
// 全盘自动发现命令（轻量级，启动时自动调用）
// ============================================================================

/// 全盘发现响应
#[derive(Debug, Serialize)]
pub struct DiscoverLocalResponse {
    /// 新增文件数
    pub new_added: usize,
    /// 发现文件总数
    pub total_found: usize,
    /// 重复跳过数
    pub duplicates: usize,
}

/// 获取当前局方文件保存目录（供前端显示用）
#[tauri::command]
pub async fn regulation_get_storage_path<R: tauri::Runtime>(
    app: AppHandle<R>,
) -> Result<String, String> {
    let path = resolve_target_dir(&app, None)?;
    Ok(path.to_string_lossy().to_string())
}

#[derive(Debug, Serialize)]
pub struct RegulationStorageDirsResponse {
    pub root: String,
    pub directories: Vec<String>,
}

/// 创建局方文件保存目录及三个分类子目录
#[tauri::command]
pub async fn regulation_prepare_storage_dirs<R: tauri::Runtime>(
    app: AppHandle<R>,
) -> Result<RegulationStorageDirsResponse, String> {
    let root = resolve_target_dir(&app, None)?;
    let directories = ensure_regulation_category_dirs(&root)?;

    Ok(RegulationStorageDirsResponse {
        root: root.to_string_lossy().to_string(),
        directories: directories.into_iter().map(|dir| dir.to_string_lossy().to_string()).collect(),
    })
}

/// 判断当前是否连接 Wi-Fi（Windows 使用 netsh，其他平台宽松返回 true）
#[tauri::command]
pub async fn regulation_is_wifi_connected() -> Result<bool, String> {
    #[cfg(windows)]
    {
        let output = std::process::Command::new("netsh")
            .args(["wlan", "show", "interfaces"])
            .output()
            .map_err(|e| format!("检测 Wi-Fi 状态失败: {}", e))?;

        if !output.status.success() {
            return Ok(false);
        }

        let stdout = String::from_utf8_lossy(&output.stdout).to_lowercase();
        Ok(stdout.contains("state") && stdout.contains("connected")
            || stdout.contains("状态") && stdout.contains("已连接"))
    }

    #[cfg(not(windows))]
    {
        Ok(true)
    }
}

/// 自动发现本地法规文件（轻量级，无 OCR）
///
/// 仅扫描当前局方保存目录下的三类局方法规目录，
/// 注册到数据库并索引到 Tantivy。
/// 设计为启动时自动调用，跳过 OCR 以保证速度。
#[tauri::command]
pub async fn regulation_discover_local<R: tauri::Runtime>(
    app: AppHandle<R>,
    local_copy_mode: Option<String>,
    target_dir: Option<String>,
    index_state: State<'_, RegulationIndexState>,
) -> Result<DiscoverLocalResponse, String> {
    let copy_mode = local_copy_mode
        .as_deref()
        .map(|mode| LocalCopyMode::from_optional(Some(mode)))
        .unwrap_or(LocalCopyMode::RegisterOnly);
    let target_dir = resolve_target_dir(&app, target_dir.as_deref())?;

    info!("开始扫描局方法规目录（无 OCR）");

    // Phase 1: 扫描用户选择的局方根目录，包含三类标准目录和 OSB 等补充目录。
    ensure_regulation_category_dirs(&target_dir)?;
    if !target_dir.exists() || !target_dir.is_dir() {
        info!("法规目录不存在，跳过: {}", target_dir.display());
        return Ok(DiscoverLocalResponse { new_added: 0, total_found: 0, duplicates: 0 });
    }
    let all_source_files = collect_supported_local_files(&target_dir, true);
    info!("目录 {} 发现 {} 个受支持文件", target_dir.display(), all_source_files.len());

    let total_found = all_source_files.len();
    info!("局方法规目录共发现 {} 个受支持文件", total_found);

    if total_found == 0 {
        return Ok(DiscoverLocalResponse { new_added: 0, total_found: 0, duplicates: 0 });
    }

    // Phase 2: 注册到数据库并索引（不做 OCR）
    let (conn, existing_hashes, existing_paths) = open_db_and_load_dedup_data(&app)?;

    let (new_files, duplicates, _indexed, _needs_ocr, _failed) = process_local_file_batch(
        &app,
        &all_source_files,
        &conn,
        &index_state,
        &existing_hashes,
        &existing_paths,
        copy_mode,
        &target_dir,
    )?;

    info!("局方法规扫描完成: 发现 {}, 新增 {}, 重复 {}", total_found, new_files, duplicates);

    Ok(DiscoverLocalResponse { new_added: new_files, total_found, duplicates })
}

// ============================================================================
// 同步对比命令
// ============================================================================

/// 同步对比结果
#[derive(Debug, Serialize)]
pub struct SyncCompareResponse {
    /// 在线总数
    pub online_total: usize,
    /// 本地已有数（匹配的）
    pub matched: usize,
    /// 新增文件（在线有，本地无）
    pub new_regulations: Vec<RegulationDiff>,
    /// 状态变化（有效性发生变化）
    pub changed_regulations: Vec<RegulationDiff>,
    /// 仅本地有（在线已找不到）
    pub local_only: usize,
}

/// 规章变化项
#[derive(Debug, Serialize)]
pub struct RegulationDiff {
    /// 规章标题
    pub title: String,
    /// 文号
    pub doc_number: String,
    /// 在线有效性
    pub online_validity: String,
    /// 本地有效性（如果存在）
    pub local_validity: Option<String>,
    /// 变化类型：new / validity_changed
    pub change_type: String,
    /// 在线 URL
    pub url: String,
    /// 文档类型
    pub doc_type: String,
    /// 发布日期
    pub publish_date: String,
    /// 签发日期
    pub sign_date: String,
    /// 发布单位
    pub office_unit: String,
    /// PDF 下载 URL（优先使用服务器已解析好的附件链接）
    pub pdf_url: String,
}

/// 在线文档（前端传入）
#[derive(Debug, Clone, Deserialize)]
pub struct OnlineRegulation {
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub pdf_url: Option<String>,
    pub validity: String,
    pub doc_number: String,
    pub doc_type: String,
    pub publish_date: Option<String>,
    pub sign_date: Option<String>,
    pub office_unit: Option<String>,
}

#[derive(Debug, Clone)]
struct LocalRegulationMeta {
    url: String,
    title: String,
    doc_number: String,
    validity: String,
}

fn diff_from_online(
    online_doc: &OnlineRegulation,
    change_type: &str,
    local_validity: Option<String>,
) -> RegulationDiff {
    RegulationDiff {
        title: online_doc.title.clone(),
        doc_number: online_doc.doc_number.clone(),
        online_validity: online_doc.validity.clone(),
        local_validity,
        change_type: change_type.to_string(),
        url: online_doc.url.clone(),
        doc_type: online_doc.doc_type.clone(),
        publish_date: online_doc.publish_date.clone().unwrap_or_default(),
        sign_date: online_doc.sign_date.clone().unwrap_or_default(),
        office_unit: online_doc.office_unit.clone().unwrap_or_default(),
        pdf_url: online_doc.pdf_url.clone().unwrap_or_default(),
    }
}

fn load_local_regulation_meta(
    conn: &rusqlite::Connection,
) -> Result<HashMap<String, LocalRegulationMeta>, String> {
    let mut stmt = conn
        .prepare("SELECT url, title, doc_number, validity FROM regulation_files")
        .map_err(|e| format!("查询本地数据失败: {}", e))?;

    let rows: Vec<(String, LocalRegulationMeta)> = stmt
        .query_map([], |row| {
            let url = row.get::<_, String>(0)?;
            Ok((
                url.clone(),
                LocalRegulationMeta {
                    url,
                    title: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    doc_number: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    validity: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                },
            ))
        })
        .map_err(|e| format!("读取本地数据失败: {}", e))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(rows.into_iter().collect())
}

fn normalize_regulation_match_key(value: &str) -> String {
    static LEADING_DOC_CODE_RE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| {
            regex::Regex::new(r"(?i)^\s*(ccar|ac|ap|ib|md|ctso|mh/t)[-_\s/0-9a-z.]*")
                .expect("leading document code regex should be valid")
        });

    let without_code = LEADING_DOC_CODE_RE.replace(value.trim(), "");
    compact_regulation_match_key(&without_code)
}

fn compact_regulation_match_key(value: &str) -> String {
    value
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_alphanumeric() || ('\u{4e00}'..='\u{9fff}').contains(c))
        .collect()
}

struct LocalRegulationMatchIndex<'a> {
    local_data: &'a HashMap<String, LocalRegulationMeta>,
    by_doc_number: HashMap<String, &'a LocalRegulationMeta>,
    by_title: HashMap<String, &'a LocalRegulationMeta>,
}

impl<'a> LocalRegulationMatchIndex<'a> {
    fn new(local_data: &'a HashMap<String, LocalRegulationMeta>) -> Self {
        let mut by_doc_number = HashMap::new();
        let mut by_title = HashMap::new();

        for meta in local_data.values() {
            let doc_number_key = compact_regulation_match_key(&meta.doc_number);
            if !doc_number_key.is_empty() {
                by_doc_number.entry(doc_number_key).or_insert(meta);
            }

            let title_key = normalize_regulation_match_key(&meta.title);
            if !title_key.is_empty() {
                by_title.entry(title_key).or_insert(meta);
            }
        }

        Self { local_data, by_doc_number, by_title }
    }

    fn find(&self, online_doc: &OnlineRegulation) -> Option<&'a LocalRegulationMeta> {
        if let Some(meta) = self.local_data.get(&online_doc.url) {
            return Some(meta);
        }

        let doc_number_key = compact_regulation_match_key(&online_doc.doc_number);
        if !doc_number_key.is_empty() {
            if let Some(meta) = self.by_doc_number.get(&doc_number_key) {
                return Some(*meta);
            }
        }

        let title_key = normalize_regulation_match_key(&online_doc.title);
        if !title_key.is_empty() {
            if let Some(meta) = self.by_title.get(&title_key) {
                return Some(*meta);
            }
        }

        if title_key.chars().count() >= 8 {
            self.by_title
                .iter()
                .find(|(local_key, _)| {
                    local_key.chars().count() >= 8
                        && (local_key.contains(title_key.as_str())
                            || title_key.contains(local_key.as_str()))
                })
                .map(|(_, meta)| *meta)
        } else {
            None
        }
    }
}

fn compare_online_with_local(
    local_data: &HashMap<String, LocalRegulationMeta>,
    online_docs: &[OnlineRegulation],
) -> SyncCompareResponse {
    let online_total = online_docs.len();
    let mut matched = 0;
    let mut new_regulations = Vec::new();
    let mut changed_regulations = Vec::new();
    let mut matched_local_urls: HashSet<String> = HashSet::new();
    let match_index = LocalRegulationMatchIndex::new(local_data);

    for online_doc in online_docs {
        if let Some(meta) = match_index.find(online_doc) {
            matched += 1;
            matched_local_urls.insert(meta.url.clone());
            if !online_doc.validity.is_empty()
                && !meta.validity.is_empty()
                && online_doc.validity != meta.validity
            {
                changed_regulations.push(diff_from_online(
                    online_doc,
                    "validity_changed",
                    Some(meta.validity.clone()),
                ));
            }
        } else {
            new_regulations.push(diff_from_online(online_doc, "new", None));
        }
    }

    let online_urls: HashSet<&str> = online_docs.iter().map(|d| d.url.as_str()).collect();
    let local_only = local_data
        .keys()
        .filter(|url| {
            !url.starts_with("file://")
                && !url.starts_with("local://")
                && !online_urls.contains(url.as_str())
                && !matched_local_urls.contains(url.as_str())
        })
        .count();

    SyncCompareResponse { online_total, matched, new_regulations, changed_regulations, local_only }
}

/// 同步对比：将在线规章列表与本地数据库对比
///
/// # 参数
/// - `online_docs`: 在线爬取的规章列表（从 Python sidecar 获取）
///
/// # 返回
/// 对比结果，包含新增、变化、仅本地有的规章
#[tauri::command]
pub async fn regulation_sync_compare<R: tauri::Runtime>(
    app: AppHandle<R>,
    online_docs: Vec<OnlineRegulation>,
) -> Result<SyncCompareResponse, String> {
    info!("开始同步对比，在线文档数: {}", online_docs.len());

    // 获取数据库连接
    let app_data_dir =
        app.path().app_data_dir().map_err(|e| format!("获取应用数据目录失败: {}", e))?;
    let db_path = app_data_dir.join("history.db");
    let conn =
        rusqlite::Connection::open(&db_path).map_err(|e| format!("打开数据库失败: {}", e))?;

    regulation_db::init_regulation_schema(&conn).map_err(|e| format!("初始化规章表失败: {}", e))?;

    let local_data = load_local_regulation_meta(&conn)?;
    info!("本地已有 {} 条记录", local_data.len());

    let result = compare_online_with_local(&local_data, &online_docs);
    let match_index = LocalRegulationMatchIndex::new(&local_data);

    let mut metadata_updates = 0usize;
    for doc in &online_docs {
        if let Some(local_meta) = match_index.find(doc) {
            metadata_updates += regulation_db::update_official_metadata(
                &conn,
                &local_meta.url,
                &doc.title,
                &doc.doc_number,
                &doc.doc_type,
                &doc.validity,
                doc.office_unit.as_deref().unwrap_or_default(),
                doc.sign_date.as_deref().unwrap_or_default(),
                doc.publish_date.as_deref().unwrap_or_default(),
            )
            .unwrap_or(0);
        }
    }

    info!(
        "同步对比完成: 在线 {}, 匹配 {}, 新增 {}, 变化 {}, 仅本地 {}, 元数据更新 {}",
        result.online_total,
        result.matched,
        result.new_regulations.len(),
        result.changed_regulations.len(),
        result.local_only,
        metadata_updates
    );

    Ok(result)
}

// ============================================================================
// OCR 处理命令
// ============================================================================

/// OCR 处理结果
#[derive(Debug, Serialize)]
pub struct OcrProcessResponse {
    /// 处理总数
    pub processed: usize,
    /// 成功 OCR 并索引
    pub ocr_success: usize,
    /// OCR 失败
    pub ocr_failed: usize,
    /// 跳过（无需 OCR）
    pub skipped: usize,
}

/// 处理需要 OCR 的规章文件（纯 Rust 实现）
///
/// 从数据库获取 pending 状态（需要 OCR）的文件，
/// 使用 Rust 原生 PDF OCR（pdfium + PP-OCRv4）处理。
/// 不再依赖 Python sidecar。
///
/// # 参数
/// - `batch_size`: 每批处理的文件数
#[tauri::command]
pub async fn regulation_ocr_pending<R: tauri::Runtime>(
    app: AppHandle<R>,
    batch_size: Option<usize>,
    index_state: State<'_, RegulationIndexState>,
    cancel_state: State<'_, RegulationTaskCancelState>,
) -> Result<OcrProcessResponse, String> {
    let batch_size = batch_size.unwrap_or(5);
    reset_cancel_flag(&cancel_state.ocr);
    info!("开始 OCR 处理待提取文件（Rust 原生），批次大小: {}", batch_size);

    // 获取需要 OCR 的文件
    let pending_files = {
        let (conn, _, _) = open_db_and_load_dedup_data(&app)?;
        let _ = regulation_db::reset_stale_processing_ocr_files(&conn, 30);
        regulation_db::get_pending_ocr_files(&conn, batch_size)
            .map_err(|e| format!("获取待 OCR 文件失败: {}", e))?
    };

    if pending_files.is_empty() {
        info!("没有待 OCR 的文件");
        return Ok(OcrProcessResponse { processed: 0, ocr_success: 0, ocr_failed: 0, skipped: 0 });
    }

    info!("找到 {} 个待 OCR 文件", pending_files.len());

    let mut ocr_success = 0;
    let mut ocr_failed = 0;
    let skipped = 0;
    let mineru_options = load_mineru_ocr_options();
    if mineru_options.is_some() {
        info!("待 OCR 队列启用 MinerU 在线优先模式");
    }

    for file in &pending_files {
        if cancel_requested(&cancel_state.ocr) {
            info!("OCR 队列收到中止请求，停止处理剩余文件");
            break;
        }

        let success = ocr_single_file_with_online_fallback(
            &app,
            &index_state,
            file,
            mineru_options.as_ref(),
            cancel_state.ocr.clone(),
        )
        .await;

        if cancel_requested(&cancel_state.ocr) {
            info!("OCR 当前文件处理中止: {}", file.title);
            break;
        }

        if success {
            ocr_success += 1;
        } else {
            ocr_failed += 1;
        }

        // 发送进度事件
        if let Err(e) = app.emit(
            "regulation:ocr-progress",
            serde_json::json!({
                "current": file.title,
                "validity": file.validity,
                "ocr_success": ocr_success,
                "ocr_failed": ocr_failed,
                "skipped": skipped,
            }),
        ) {
            debug!("发送 OCR 进度事件失败: {}", e);
        }
    }

    if cancel_requested(&cancel_state.ocr) {
        return Err("OCR 已中止".to_string());
    }

    let processed = ocr_success + ocr_failed + skipped;
    info!(
        "OCR 处理完成: 处理 {}, 成功 {}, 失败 {}, 跳过 {}",
        processed, ocr_success, ocr_failed, skipped
    );

    Ok(OcrProcessResponse { processed, ocr_success, ocr_failed, skipped })
}

/// 请求中止当前 OCR 队列。
#[tauri::command]
pub async fn regulation_cancel_ocr(
    cancel_state: State<'_, RegulationTaskCancelState>,
) -> Result<bool, String> {
    cancel_state.ocr.store(true, Ordering::Relaxed);
    Ok(true)
}

/// 重试失败的 OCR 文件
///
/// 将所有 failed 状态的文件重置为 pending，然后执行 OCR 处理。
#[tauri::command]
pub async fn regulation_retry_failed_ocr<R: tauri::Runtime>(
    app: AppHandle<R>,
    index_state: State<'_, RegulationIndexState>,
    cancel_state: State<'_, RegulationTaskCancelState>,
) -> Result<OcrProcessResponse, String> {
    info!("开始重试失败的 OCR 文件");

    let (conn, _, _) = open_db_and_load_dedup_data(&app)?;

    // 重置 failed → pending
    let reset_count = regulation_db::reset_failed_ocr_files(&conn)
        .map_err(|e| format!("重置失败文件状态失败: {}", e))?;

    if reset_count == 0 {
        info!("没有失败的 OCR 文件需要重试");
        return Ok(OcrProcessResponse { processed: 0, ocr_success: 0, ocr_failed: 0, skipped: 0 });
    }

    info!("已重置 {} 个失败文件，开始重新 OCR", reset_count);
    drop(conn);

    // 调用已有的 OCR 处理逻辑
    regulation_ocr_pending(app, Some(reset_count), index_state, cancel_state).await
}

/// 重置候选记录的响应
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequeueOcrResponse {
    pub candidate_count: usize,
    pub deleted_from_index: usize,
    pub reset_to_pending: usize,
    pub sample_titles: Vec<String>,
}

/// 按 OCR 引擎分布统计响应（供前端对话框预计数量）
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrEngineStats {
    pub pdfium: i64,
    pub pp_ocrv4: i64,
    pub mineru: i64,
    pub unknown: i64,
    /// 扫描件聚合：pp_ocrv4 + unknown，作为重做对话框的推荐默认范围
    /// （pdfium 是文本型 PDF，重做无质量提升，故不计入）
    pub scan_only: i64,
    pub non_mineru: i64,
    pub total_done: i64,
}

/// 统计各 OCR 引擎的 done 记录数量（供前端展示）
#[tauri::command]
pub async fn regulation_ocr_engine_stats<R: tauri::Runtime>(
    app: AppHandle<R>,
) -> Result<OcrEngineStats, String> {
    let conn = open_regulation_db(&app)?;

    let count_for = |engine: &str| -> Result<i64, String> {
        conn.query_row(
            "SELECT COUNT(*) FROM regulation_files \
             WHERE ocr_status = 'done' AND ocr_engine = ?1",
            rusqlite::params![engine],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|e| format!("统计 {} 失败: {}", engine, e))
    };

    let pdfium = count_for("pdfium")?;
    let pp_ocrv4 = count_for("pp_ocrv4")?;
    let mineru = count_for("mineru")?;
    let unknown = count_for("unknown")?;
    let total_done: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM regulation_files WHERE ocr_status = 'done'",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("统计 total_done 失败: {}", e))?;

    let non_mineru = total_done - mineru;
    let scan_only = pp_ocrv4 + unknown;

    Ok(OcrEngineStats {
        pdfium,
        pp_ocrv4,
        mineru,
        unknown,
        scan_only,
        non_mineru,
        total_done,
    })
}

/// 重置筛选范围：按 OCR 引擎
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequeueOcrFilter {
    /// 指定引擎重做：支持 `scan_only` / `pp_ocrv4` / `pdfium` / `unknown` / `non_mineru` / `all_done`
    pub scope: String,
}

/// 找出指定范围的 OCR 记录，从 Tantivy 索引删除并重置 `ocr_status = 'pending'`，
/// 让后续 OCR 队列（优先 MinerU）重做。
///
/// **scope 选项**（前端在对话框里选择）：
/// - `scan_only`：推荐。重做扫描件部分（pp_ocrv4 + unknown），不动 pdfium
/// - `pp_ocrv4`：仅重做本地 PP-OCRv4 做的记录（最保守，历史上效果较差）
/// - `pdfium`：仅重做 pdfium 文本直接提取的记录（一般无质量提升，慎用）
/// - `unknown`：仅重做 engine 为 unknown 的记录
/// - `non_mineru`：重做所有不是 MinerU 做的（含 pdfium，慎用）
/// - `all_done`：全部 done 都重做（极慎用）
///
/// 此命令只重置状态，不立即触发 OCR；用户需要在 OCR 队列页面手动启动处理。
#[tauri::command]
pub async fn regulation_requeue_ocr_by_engine<R: tauri::Runtime>(
    app: AppHandle<R>,
    index_state: State<'_, RegulationIndexState>,
    filter: RequeueOcrFilter,
) -> Result<RequeueOcrResponse, String> {
    info!("[RequeueOCR] 按范围查找待重做记录: scope={}", filter.scope);

    let conn = open_regulation_db(&app)?;

    // 根据 scope 构造 WHERE 条件
    let where_clause = match filter.scope.as_str() {
        "scan_only" => {
            "ocr_status = 'done' AND ocr_engine IN ('pp_ocrv4', 'unknown')".to_string()
        }
        "pp_ocrv4" => "ocr_status = 'done' AND ocr_engine = 'pp_ocrv4'".to_string(),
        "pdfium" => "ocr_status = 'done' AND ocr_engine = 'pdfium'".to_string(),
        "unknown" => "ocr_status = 'done' AND ocr_engine = 'unknown'".to_string(),
        "non_mineru" => {
            "ocr_status = 'done' AND ocr_engine != 'mineru'".to_string()
        }
        "all_done" => "ocr_status = 'done'".to_string(),
        other => return Err(format!("不支持的 scope: {}", other)),
    };

    // 1. 找候选记录
    let sql = format!(
        "SELECT id, url, title FROM regulation_files WHERE {} ORDER BY id ASC",
        where_clause
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| format!("准备查询语句失败: {}", e))?;

    let rows: Vec<(i64, String, String)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?.unwrap_or_default(),
            ))
        })
        .map_err(|e| format!("查询候选失败: {}", e))?
        .filter_map(|r| r.ok())
        .collect();
    drop(stmt);

    let candidate_count = rows.len();
    if candidate_count == 0 {
        info!("[RequeueOCR] scope={} 没有匹配记录", filter.scope);
        return Ok(RequeueOcrResponse {
            candidate_count: 0,
            deleted_from_index: 0,
            reset_to_pending: 0,
            sample_titles: Vec::new(),
        });
    }

    let urls: Vec<String> = rows.iter().map(|(_, url, _)| url.clone()).collect();
    let sample_titles: Vec<String> = rows.iter().take(8).map(|(_, _, t)| t.clone()).collect();
    let ids: Vec<i64> = rows.iter().map(|(id, _, _)| *id).collect();

    info!(
        "[RequeueOCR] scope={} 找到 {} 条候选，前几条标题: {:?}",
        filter.scope, candidate_count, sample_titles
    );

    // 2. 从 Tantivy 索引删除
    let deleted_from_index = {
        let guard = index_state.index.lock().map_err(|e| format!("锁定索引状态失败: {}", e))?;
        match guard.as_ref() {
            Some(index) => index
                .delete_by_urls(&urls)
                .map_err(|e| format!("从索引删除失败: {}", e))?,
            None => {
                warn!("[RequeueOCR] 索引未初始化，跳过 Tantivy 删除");
                0
            }
        }
    };

    // 3. 重置数据库状态：done → pending，indexed=0，清掉进度
    //    注意：这里不清除 ocr_engine，保留为原 engine，便于下次重做后 UPDATE
    let placeholders: String =
        std::iter::repeat("?").take(ids.len()).collect::<Vec<_>>().join(",");
    let update_sql = format!(
        "UPDATE regulation_files \
         SET ocr_status = 'pending', indexed = 0, indexed_at = NULL, \
             ocr_progress = 0, ocr_current_page = 0, \
             ocr_error = '已重置以使用 MinerU 重做', \
             updated_at = CURRENT_TIMESTAMP \
         WHERE id IN ({})",
        placeholders
    );
    let params_vec: Vec<&dyn rusqlite::ToSql> =
        ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();
    let reset_to_pending = conn
        .execute(&update_sql, params_vec.as_slice())
        .map_err(|e| format!("重置数据库状态失败: {}", e))?;

    info!(
        "[RequeueOCR] 完成: scope={}, 候选 {}, 索引删除 {}, DB 重置 {}",
        filter.scope, candidate_count, deleted_from_index, reset_to_pending
    );

    Ok(RequeueOcrResponse {
        candidate_count,
        deleted_from_index,
        reset_to_pending,
        sample_titles,
    })
}

/// 旧版兼容命令：等价于 `regulation_requeue_ocr_by_engine(scope = 'pp_ocrv4')`
/// 保留以避免前端冷启动调用失败；新代码请使用 `regulation_requeue_ocr_by_engine`
#[tauri::command]
pub async fn regulation_requeue_local_ocr_for_mineru<R: tauri::Runtime>(
    app: AppHandle<R>,
    index_state: State<'_, RegulationIndexState>,
) -> Result<RequeueOcrResponse, String> {
    regulation_requeue_ocr_by_engine(
        app,
        index_state,
        RequeueOcrFilter { scope: "pp_ocrv4".to_string() },
    )
    .await
}

/// 清理无效记录响应
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupInvalidResponse {
    /// 候选总数（满足"不支持的后缀"或"物理文件不存在"任一条件的记录数）
    pub candidate_count: usize,
    /// 后缀不是受支持类型（PDF / TXT）的记录数
    pub non_pdf_count: usize,
    /// 文件物理不存在的记录数
    pub missing_file_count: usize,
    /// 从 Tantivy 索引删除的文档数
    pub deleted_from_index: usize,
    /// 从数据库实际删除的记录数
    pub deleted_from_db: usize,
    /// 前若干个被删除记录的标题（最多 8 个，便于用户确认）
    pub sample_titles: Vec<String>,
}

/// 清理无效的规章记录：
/// - 文件路径后缀不是受支持类型（PDF / TXT）
/// - 文件物理不存在（磁盘上已被删除）
///
/// 这些记录会一直造成 OCR 失败计数（pdfium FormatError、文件不存在），
/// 即便点击"重试失败 OCR"也不可能成功。本命令把它们彻底从数据库 + Tantivy 索引移除。
///
/// **不会删除磁盘上的实际文件**，只清理数据库 / 索引里的残留记录。
#[tauri::command]
pub async fn regulation_cleanup_invalid_files<R: tauri::Runtime>(
    app: AppHandle<R>,
    index_state: State<'_, RegulationIndexState>,
) -> Result<CleanupInvalidResponse, String> {
    info!("[CleanupInvalid] 开始清理无效规章记录");

    let conn = open_regulation_db(&app)?;

    // 1. 全表扫描，按是否支持的文件类型 + 物理文件是否存在两个维度判定
    let mut stmt = conn
        .prepare("SELECT id, url, title, file_path FROM regulation_files ORDER BY id ASC")
        .map_err(|e| format!("准备查询语句失败: {}", e))?;

    let rows: Vec<(i64, String, String, String)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                row.get::<_, Option<String>>(3)?.unwrap_or_default(),
            ))
        })
        .map_err(|e| format!("查询规章文件失败: {}", e))?
        .filter_map(|r| r.ok())
        .collect();
    drop(stmt);

    let mut invalid_ids: Vec<i64> = Vec::new();
    let mut invalid_urls: Vec<String> = Vec::new();
    let mut sample_titles: Vec<String> = Vec::new();
    let mut non_pdf_count = 0usize;
    let mut missing_file_count = 0usize;

    for (id, url, title, file_path) in &rows {
        let path = std::path::Path::new(file_path);
        let is_supported = is_supported_local_scan_path(path);
        let exists = !file_path.is_empty() && path.exists();

        if !is_invalid_regulation_file_record(path) {
            continue;
        }

        if !is_supported {
            non_pdf_count += 1;
        }
        if !exists {
            missing_file_count += 1;
        }

        invalid_ids.push(*id);
        invalid_urls.push(url.clone());
        if sample_titles.len() < 8 {
            sample_titles.push(title.clone());
        }
    }

    let candidate_count = invalid_ids.len();
    info!(
        "[CleanupInvalid] 候选 {} 条 (不支持类型: {}, 文件不存在: {})，前几条: {:?}",
        candidate_count, non_pdf_count, missing_file_count, sample_titles
    );

    if candidate_count == 0 {
        return Ok(CleanupInvalidResponse {
            candidate_count: 0,
            non_pdf_count: 0,
            missing_file_count: 0,
            deleted_from_index: 0,
            deleted_from_db: 0,
            sample_titles: Vec::new(),
        });
    }

    // 2. 从 Tantivy 索引删除（按 url）
    let deleted_from_index = {
        let guard = index_state.index.lock().map_err(|e| format!("锁定索引状态失败: {}", e))?;
        match guard.as_ref() {
            Some(index) => index
                .delete_by_urls(&invalid_urls)
                .map_err(|e| format!("从索引删除失败: {}", e))?,
            None => {
                warn!("[CleanupInvalid] 索引未初始化，跳过 Tantivy 删除");
                0
            }
        }
    };

    // 3. 从数据库删除
    let deleted_from_db = regulation_db::delete_files_by_ids(&conn, &invalid_ids)
        .map_err(|e| format!("数据库删除失败: {}", e))?;

    info!(
        "[CleanupInvalid] 完成: 候选 {}, 索引删除 {}, DB 删除 {}",
        candidate_count, deleted_from_index, deleted_from_db
    );

    Ok(CleanupInvalidResponse {
        candidate_count,
        non_pdf_count,
        missing_file_count,
        deleted_from_index,
        deleted_from_db,
        sample_titles,
    })
}

/// 一键对齐 PDF/TXT 文件名响应
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RealignFilenamesResponse {
    /// 扫描的 DB 记录总数
    pub total_scanned: usize,
    /// 因 file_path 为空 / 文件不存在 / 非 PDF/TXT 而跳过的数量
    pub skipped_invalid: usize,
    /// 已经符合规则、无需重命名的数量
    pub already_aligned: usize,
    /// 实际重命名成功的数量
    pub renamed: usize,
    /// 重命名失败的数量
    pub failed: usize,
    /// Tantivy 索引中同步更新的 file_path 数量
    pub index_updated: usize,
    /// 重命名样本（最多 5 个 `<旧文件名> -> <新文件名>`）
    pub samples: Vec<String>,
    /// 重命名失败的错误样本（最多 5 个 `<title>: <reason>`）
    pub failure_samples: Vec<String>,
}

/// 一键对齐磁盘上的 PDF/TXT 文件名为「文号_标题.ext」格式。
///
/// 历史下载 / 复制流程使用 `<sha256前16字符>.pdf` 作为文件名，导致用户在文件管理器
/// 里看到一堆不可读的哈希。本命令按现行命名规则（[`build_pretty_filename`]）批量
/// 重命名磁盘文件，并同步更新数据库 file_path 字段以及 Tantivy 索引。
///
/// 对每条 DB 记录：
/// 1. 跳过 file_path 为空 / 物理文件不存在 / 非 PDF 的记录
/// 2. 算出 desired 文件名，与现有 basename 比较
/// 3. 如果一致，记入 `already_aligned`
/// 4. 如果不一致，按 [`dedupe_filename`] 解决冲突后 rename，更新 DB 与索引
///
/// 不会移动文件到其它目录，仅在原目录内 rename。
#[tauri::command]
pub async fn regulation_realign_pdf_filenames<R: tauri::Runtime>(
    app: AppHandle<R>,
    index_state: State<'_, RegulationIndexState>,
) -> Result<RealignFilenamesResponse, String> {
    info!("[RealignFilenames] 开始一键对齐 PDF 文件名");

    let conn = open_regulation_db(&app)?;

    // 拉出全部记录
    let mut stmt = conn
        .prepare(
            "SELECT id, url, title, doc_number, sha256, file_path \
             FROM regulation_files ORDER BY id ASC",
        )
        .map_err(|e| format!("准备查询语句失败: {}", e))?;

    let rows: Vec<(i64, String, String, String, String, String)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?.unwrap_or_default(),
            ))
        })
        .map_err(|e| format!("查询规章文件失败: {}", e))?
        .filter_map(|r| r.ok())
        .collect();
    drop(stmt);

    let total_scanned = rows.len();
    let mut skipped_invalid = 0usize;
    let mut already_aligned = 0usize;
    let mut renamed = 0usize;
    let mut failed = 0usize;
    let mut samples: Vec<String> = Vec::new();
    let mut failure_samples: Vec<String> = Vec::new();
    // (id, new_file_path_string) — 用于 DB 更新
    let mut db_updates: Vec<(i64, String)> = Vec::new();
    // (url, new_file_path_string) — 用于 Tantivy 更新
    let mut index_updates: Vec<(String, String)> = Vec::new();

    for (id, url, title, doc_number, sha256, file_path) in &rows {
        if file_path.is_empty() {
            skipped_invalid += 1;
            continue;
        }
        let path = std::path::Path::new(file_path);
        if !path.exists() {
            skipped_invalid += 1;
            continue;
        }
        if !is_supported_realign_filename_path(path) {
            skipped_invalid += 1;
            continue;
        }

        let parent = match path.parent() {
            Some(p) => p,
            None => {
                skipped_invalid += 1;
                continue;
            }
        };
        let current_basename = match path.file_name().and_then(|s| s.to_str()) {
            Some(name) => name.to_string(),
            None => {
                skipped_invalid += 1;
                continue;
            }
        };

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_lowercase)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "pdf".to_string());

        let doc_number_opt =
            if doc_number.trim().is_empty() { None } else { Some(doc_number.as_str()) };
        let title_opt = if title.trim().is_empty() { None } else { Some(title.as_str()) };

        let desired_name = build_pretty_filename(doc_number_opt, title_opt, sha256, &ext);

        if current_basename == desired_name {
            already_aligned += 1;
            continue;
        }

        // 解决重名（dedupe 内部会查 parent 是否存在同名文件）
        let new_path = dedupe_filename(parent, &desired_name, sha256);

        // dedupe 可能把目标解析成与当前同路径（极端情况：当前文件就是已 dedupe 后名字）
        if new_path == path {
            already_aligned += 1;
            continue;
        }

        match std::fs::rename(path, &new_path) {
            Ok(()) => {
                let new_path_str = new_path.to_string_lossy().to_string();
                if samples.len() < 5 {
                    let new_basename = new_path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("?")
                        .to_string();
                    samples.push(format!("{} -> {}", current_basename, new_basename));
                }
                db_updates.push((*id, new_path_str.clone()));
                index_updates.push((url.clone(), new_path_str));
                renamed += 1;
            }
            Err(e) => {
                failed += 1;
                if failure_samples.len() < 5 {
                    failure_samples.push(format!("{}: {}", title, e));
                }
                warn!("[RealignFilenames] 重命名失败 id={} {} - {}", id, file_path, e);
            }
        }
    }

    info!(
        "[RealignFilenames] 扫描 {} 条, 重命名 {}, 已对齐 {}, 跳过 {}, 失败 {}",
        total_scanned, renamed, already_aligned, skipped_invalid, failed
    );

    // 批量更新 DB.file_path
    if !db_updates.is_empty() {
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| format!("开启事务失败: {}", e))?;
        {
            let mut update_stmt = tx
                .prepare(
                    "UPDATE regulation_files SET file_path = ?1, updated_at = CURRENT_TIMESTAMP \
                     WHERE id = ?2",
                )
                .map_err(|e| format!("准备更新语句失败: {}", e))?;
            for (id, new_path) in &db_updates {
                update_stmt
                    .execute(rusqlite::params![new_path, id])
                    .map_err(|e| format!("更新 file_path 失败 id={}: {}", id, e))?;
            }
        }
        tx.commit().map_err(|e| format!("提交事务失败: {}", e))?;
    }

    // 批量更新 Tantivy 索引中的 file_path
    let index_updated = if index_updates.is_empty() {
        0
    } else {
        let guard = index_state
            .index
            .lock()
            .map_err(|e| format!("锁定索引状态失败: {}", e))?;
        match guard.as_ref() {
            Some(index) => index
                .update_file_paths_by_url(&index_updates)
                .map_err(|e| format!("更新 Tantivy 索引失败: {}", e))?,
            None => {
                warn!("[RealignFilenames] 索引未初始化，跳过 Tantivy 更新");
                0
            }
        }
    };

    Ok(RealignFilenamesResponse {
        total_scanned,
        skipped_invalid,
        already_aligned,
        renamed,
        failed,
        index_updated,
        samples,
        failure_samples,
    })
}

/// 辅助函数：将文本写入 Tantivy 索引并更新数据库状态
///
/// `engine` 应传具体的 OCR 引擎标识：`pdfium` / `pp_ocrv4` / `mineru`，
/// 用于记录每条记录的文本来源，便于后续按 engine 重做。
fn write_to_index(
    index_state: &State<'_, RegulationIndexState>,
    conn: &rusqlite::Connection,
    file: &regulation_db::RegulationFile,
    content: String,
    engine: &str,
) -> Result<(), String> {
    let doc = super::schema::RegulationDocument {
        title: file.title.clone(),
        doc_number: file.doc_number.clone(),
        validity: file.validity.clone(),
        doc_type: file.doc_type.clone(),
        office_unit: file.office_unit.clone(),
        sign_date: file.sign_date.clone(),
        publish_date: file.publish_date.clone(),
        url: file.url.clone(),
        file_path: file.file_path.clone(),
        content,
    };

    let state_guard = index_state.index.lock().map_err(|e| format!("锁定索引状态失败: {}", e))?;

    if let Some(index) = state_guard.as_ref() {
        if !index.exists(&doc.url) {
            index
                .add_document(&doc)
                .and_then(|_| index.commit())
                .map_err(|e| format!("写入索引失败: {}", e))?;
        }
    } else {
        return Err("索引未初始化".to_string());
    }

    let _ = regulation_db::update_ocr_status(conn, file.id, "done", 100, 0, None);
    let _ = regulation_db::update_ocr_engine(conn, file.id, engine);
    let _ = regulation_db::mark_indexed(conn, file.id);
    Ok(())
}

/// 通过 Python sidecar OCR 处理单个 PDF 文件
///
/// 前端调用此命令时，先调用 sidecar 的 ocr_pdf 方法获取文本，
/// 然后将文本传入此命令写入索引。
///
/// `engine` 参数标记本次 OCR 来源（用于 ocr_engine 列）：
/// - `None` / 缺省：按 sidecar 默认走 `pp_ocrv4`
/// - 显式传 `mineru` / `pdfium` / `pp_ocrv4` / `unknown`：以传入值为准
#[tauri::command]
pub async fn regulation_ocr_update<R: tauri::Runtime>(
    app: AppHandle<R>,
    file_id: i64,
    ocr_text: String,
    page_count: i32,
    engine: Option<String>,
    index_state: State<'_, RegulationIndexState>,
) -> Result<bool, String> {
    info!("更新 OCR 结果: file_id={}, 文本长度={}", file_id, ocr_text.len());

    let app_data_dir =
        app.path().app_data_dir().map_err(|e| format!("获取应用数据目录失败: {}", e))?;
    let db_path = app_data_dir.join("history.db");
    let conn =
        rusqlite::Connection::open(&db_path).map_err(|e| format!("打开数据库失败: {}", e))?;

    regulation_db::init_regulation_schema(&conn).map_err(|e| format!("初始化规章表失败: {}", e))?;

    // 获取文件信息
    let file = {
        let mut stmt = conn
            .prepare(
                "SELECT title, doc_number, doc_type, validity, office_unit, sign_date, \
                    publish_date, url, file_path FROM regulation_files WHERE id = ?1",
            )
            .map_err(|e| format!("查询文件失败: {}", e))?;

        let result = stmt.query_row(rusqlite::params![file_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
            ))
        });

        match result {
            Ok(f) => f,
            Err(_) => return Err(format!("文件不存在: id={}", file_id)),
        }
    };

    let (
        title,
        doc_number,
        doc_type,
        validity,
        office_unit,
        sign_date,
        publish_date,
        url,
        file_path,
    ) = file;

    // 更新页数
    if page_count > 0 {
        let _ = regulation_db::update_page_count(&conn, file_id, page_count);
    }

    if ocr_text.is_empty() {
        let _ = regulation_db::update_ocr_status(
            &conn,
            file_id,
            "failed",
            0,
            0,
            Some("OCR 未能提取到文本"),
        );
        return Ok(false);
    }

    // 构建文档
    let doc = super::schema::RegulationDocument {
        title,
        doc_number,
        validity,
        doc_type,
        office_unit,
        sign_date,
        publish_date,
        url,
        file_path,
        content: ocr_text,
    };

    // 写入 Tantivy 索引
    let state_guard = index_state.index.lock().map_err(|e| format!("锁定索引状态失败: {}", e))?;

    if let Some(index) = state_guard.as_ref() {
        if !index.exists(&doc.url) {
            index
                .add_document(&doc)
                .and_then(|_| index.commit())
                .map_err(|e| format!("写入索引失败: {}", e))?;
        }
    } else {
        return Err("索引未初始化".to_string());
    }

    // 更新数据库状态
    let engine_label = engine.as_deref().unwrap_or("pp_ocrv4");
    let _ = regulation_db::update_ocr_status(&conn, file_id, "done", 100, 0, None);
    let _ = regulation_db::update_ocr_engine(&conn, file_id, engine_label);
    let _ = regulation_db::mark_indexed(&conn, file_id);

    info!("OCR 结果已写入索引: file_id={}, engine={}", file_id, engine_label);
    Ok(true)
}

/// 获取需要 OCR 引擎处理的文件列表
#[tauri::command]
pub async fn regulation_get_ocr_queue<R: tauri::Runtime>(
    app: AppHandle<R>,
    limit: Option<usize>,
) -> Result<Vec<regulation_db::RegulationFile>, String> {
    let limit = limit.unwrap_or(20);

    let app_data_dir =
        app.path().app_data_dir().map_err(|e| format!("获取应用数据目录失败: {}", e))?;
    let db_path = app_data_dir.join("history.db");
    let conn =
        rusqlite::Connection::open(&db_path).map_err(|e| format!("打开数据库失败: {}", e))?;

    regulation_db::init_regulation_schema(&conn).map_err(|e| format!("初始化规章表失败: {}", e))?;

    // 获取需要 OCR 的文件（pending 或 needs_ocr_engine 状态）
    let mut stmt = conn
        .prepare(
            r#"
        SELECT id, title, doc_number, doc_type, validity, office_unit, sign_date, publish_date,
               url, pdf_url, sha256, file_path,
               file_size, page_count, ocr_status, ocr_progress, ocr_current_page,
               ocr_error, indexed, indexed_at, created_at, updated_at,
               COALESCE(ocr_engine, 'unknown')
        FROM regulation_files
        WHERE ocr_status IN ('pending', 'needs_ocr_engine')
        ORDER BY created_at ASC
        LIMIT ?1
        "#,
        )
        .map_err(|e| format!("查询失败: {}", e))?;

    let files: Vec<regulation_db::RegulationFile> = stmt
        .query_map(rusqlite::params![limit as i64], |row| {
            Ok(regulation_db::RegulationFile {
                id: row.get(0)?,
                title: row.get(1)?,
                doc_number: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                doc_type: row.get(3)?,
                validity: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                office_unit: row.get::<_, Option<String>>(5)?.unwrap_or_default(),
                sign_date: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
                publish_date: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
                url: row.get(8)?,
                pdf_url: row.get(9)?,
                sha256: row.get(10)?,
                file_path: row.get(11)?,
                file_size: row.get(12)?,
                page_count: row.get(13)?,
                ocr_status: row.get(14)?,
                ocr_progress: row.get(15)?,
                ocr_current_page: row.get(16)?,
                ocr_error: row.get(17)?,
                indexed: row.get::<_, i32>(18)? != 0,
                indexed_at: row.get(19)?,
                created_at: row.get(20)?,
                updated_at: row.get(21)?,
                ocr_engine: row
                    .get::<_, Option<String>>(22)?
                    .unwrap_or_else(|| "unknown".to_string()),
            })
        })
        .map_err(|e| format!("读取文件列表失败: {}", e))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(files)
}

/// 批量提交文档到 Tantivy 索引
fn commit_batch_to_index(
    docs: &[(i64, super::schema::RegulationDocument)],
    index_state: &State<'_, RegulationIndexState>,
    conn: &rusqlite::Connection,
) -> Result<(), String> {
    let state_guard = index_state.index.lock().map_err(|e| format!("锁定索引状态失败: {}", e))?;

    if let Some(index) = state_guard.as_ref() {
        for (file_id, doc) in docs {
            if !index.exists(&doc.url) {
                if let Err(e) = index.add_document(doc) {
                    warn!("添加文档到索引失败: {} - {}", doc.title, e);
                    continue;
                }
            }
            // 标记已索引（此路径为 pdfium 直接提取成功）
            let _ = regulation_db::update_ocr_status(conn, *file_id, "done", 100, 0, None);
            let _ = regulation_db::update_ocr_engine(conn, *file_id, "pdfium");
            let _ = regulation_db::mark_indexed(conn, *file_id);
        }

        // 批量提交
        if let Err(e) = index.commit() {
            warn!("提交索引失败: {}", e);
        } else {
            info!("批量提交 {} 个文档到索引", docs.len());
        }
    } else {
        warn!("索引未初始化，跳过索引写入");
    }

    Ok(())
}

// ============================================================================
// 在线搜索命令（Rust 原生实现，替代 Python Sidecar）
// ============================================================================

/// 在线搜索规章（Rust 原生，不依赖 Sidecar）
///
/// 直接使用 reqwest + scraper 访问 CAAC 官网搜索页面，
/// 解析 HTML 表格返回结果。
#[tauri::command]
pub async fn regulation_online_search(
    keyword: String,
    doc_type: Option<String>,
    validity: Option<String>,
    start_date: Option<String>,
    end_date: Option<String>,
) -> Result<OnlineSearchResponse, String> {
    info!("Rust 原生在线搜索: keyword={}, doc_type={:?}", keyword, doc_type);

    let request = OnlineSearchRequest {
        keyword,
        doc_type: doc_type.unwrap_or_else(|| "all".to_string()),
        validity: validity.unwrap_or_else(|| "all".to_string()),
        start_date: start_date.unwrap_or_default(),
        end_date: end_date.unwrap_or_default(),
    };

    let searcher = CaacOnlineSearcher::new().map_err(|e| format!("创建搜索器失败: {}", e))?;

    searcher.search(&request).await.map_err(|e| format!("在线搜索失败: {}", e))
}

/// 全量在线爬取（Rust 原生分页）
#[tauri::command]
pub async fn regulation_fetch_all_online(
    doc_type: Option<String>,
    max_pages: Option<usize>,
) -> Result<OnlineSearchResponse, String> {
    let doc_type = doc_type.unwrap_or_else(|| "all".to_string());
    let max_pages = max_pages.unwrap_or(20);

    let searcher = CaacOnlineSearcher::new().map_err(|e| format!("创建搜索器失败: {}", e))?;

    match searcher.fetch_all_static(&doc_type).await {
        Ok(response) => Ok(response),
        Err(e) => {
            warn!("静态 JSON 源读取失败，回退到局方官网实时爬取: {}", e);
            searcher
                .fetch_all(&doc_type, max_pages)
                .await
                .map_err(|e| format!("全量爬取失败: {}", e))
        }
    }
}

/// 同步对比（在线抓取 + 本地对比）
#[tauri::command]
pub async fn regulation_sync_compare_online<R: tauri::Runtime>(
    app: AppHandle<R>,
    doc_type: Option<String>,
    max_pages: Option<usize>,
    cancel_state: State<'_, RegulationTaskCancelState>,
) -> Result<SyncCompareResponse, String> {
    let doc_type = doc_type.unwrap_or_else(|| "all".to_string());
    let max_pages = max_pages.unwrap_or(20);
    reset_cancel_flag(&cancel_state.sync_compare);
    let cancel_flag = cancel_state.sync_compare.clone();

    let searcher = CaacOnlineSearcher::new().map_err(|e| format!("创建搜索器失败: {}", e))?;
    let fetch_future = async {
        match searcher.fetch_all_static(&doc_type).await {
            Ok(response) => Ok(response),
            Err(e) => {
                warn!("静态 JSON 源读取失败，回退到局方官网实时爬取: {}", e);
                searcher
                    .fetch_all(&doc_type, max_pages)
                    .await
                    .map_err(|e| format!("在线抓取失败: {}", e))
            }
        }
    };

    let online = tokio::select! {
        result = fetch_future => result?,
        _ = wait_for_cancel(cancel_flag.clone()) => return Err("同步已中止".to_string()),
    };

    if cancel_requested(&cancel_flag) {
        return Err("同步已中止".to_string());
    }

    let online_docs: Vec<OnlineRegulation> = online
        .documents
        .into_iter()
        .map(|doc: OnlineDocument| OnlineRegulation {
            title: doc.title,
            url: doc.url,
            pdf_url: if doc.pdf_url.is_empty() { None } else { Some(doc.pdf_url) },
            validity: doc.validity,
            doc_number: doc.doc_number,
            doc_type: doc.doc_type,
            publish_date: if doc.publish_date.is_empty() { None } else { Some(doc.publish_date) },
            sign_date: if doc.sign_date.is_empty() { None } else { Some(doc.sign_date) },
            office_unit: if doc.office_unit.is_empty() { None } else { Some(doc.office_unit) },
        })
        .collect();

    regulation_sync_compare(app, online_docs).await
}

/// 请求中止“同步对比官网”。
#[tauri::command]
pub async fn regulation_cancel_sync_compare(
    cancel_state: State<'_, RegulationTaskCancelState>,
) -> Result<bool, String> {
    cancel_state.sync_compare.store(true, Ordering::Relaxed);
    Ok(true)
}

#[derive(Debug, Deserialize)]
pub struct LegacyImportRequest {
    /// 旧应用数据目录（包含 history.db / regulations / regulation_index）
    pub legacy_data_dir: String,
    /// 是否复制旧文件到新应用受管目录，默认 true
    pub copy_files: Option<bool>,
    /// 是否复制旧索引目录，默认 false
    pub copy_index: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct LegacyImportResponse {
    pub total_found: usize,
    pub imported: usize,
    pub skipped: usize,
    pub failed: usize,
    pub copied_files: usize,
    pub copied_index_files: usize,
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<usize, String> {
    if !src.exists() || !src.is_dir() {
        return Ok(0);
    }
    std::fs::create_dir_all(dst).map_err(|e| format!("创建目录失败 {:?}: {}", dst, e))?;

    let mut copied = 0usize;
    for entry in std::fs::read_dir(src).map_err(|e| format!("读取目录失败 {:?}: {}", src, e))?
    {
        let entry = entry.map_err(|e| format!("读取目录项失败: {}", e))?;
        let path = entry.path();
        let target = dst.join(entry.file_name());

        if path.is_dir() {
            copied += copy_dir_recursive(&path, &target)?;
        } else {
            std::fs::copy(&path, &target)
                .map_err(|e| format!("复制文件失败 {:?} -> {:?}: {}", path, target, e))?;
            copied += 1;
        }
    }
    Ok(copied)
}

/// 导入旧应用规章数据（history.db + 本地文件 + 索引目录）
#[tauri::command]
pub async fn regulation_import_legacy_data<R: tauri::Runtime>(
    app: AppHandle<R>,
    request: LegacyImportRequest,
) -> Result<LegacyImportResponse, String> {
    let copy_files = request.copy_files.unwrap_or(true);
    let copy_index = request.copy_index.unwrap_or(false);
    let legacy_dir = PathBuf::from(request.legacy_data_dir);
    if !legacy_dir.exists() || !legacy_dir.is_dir() {
        return Err(format!("旧数据目录不存在: {}", legacy_dir.display()));
    }

    let legacy_db_path = legacy_dir.join("history.db");
    if !legacy_db_path.exists() {
        return Err(format!("旧数据库不存在: {}", legacy_db_path.to_string_lossy()));
    }

    let app_data_dir =
        app.path().app_data_dir().map_err(|e| format!("获取应用数据目录失败: {}", e))?;
    std::fs::create_dir_all(&app_data_dir).map_err(|e| format!("创建应用数据目录失败: {}", e))?;

    let new_db_path = app_data_dir.join("history.db");
    let legacy_conn = rusqlite::Connection::open(&legacy_db_path)
        .map_err(|e| format!("打开旧数据库失败: {}", e))?;
    let new_conn =
        rusqlite::Connection::open(&new_db_path).map_err(|e| format!("打开新数据库失败: {}", e))?;

    regulation_db::init_regulation_schema(&legacy_conn)
        .map_err(|e| format!("旧数据库 schema 校验失败: {}", e))?;
    regulation_db::init_regulation_schema(&new_conn)
        .map_err(|e| format!("新数据库 schema 校验失败: {}", e))?;

    let mut stmt = legacy_conn
        .prepare(
            r#"
            SELECT title, doc_number, doc_type, url, pdf_url, sha256, file_path,
                   file_size, page_count, ocr_status, ocr_progress, ocr_current_page,
                   ocr_error, indexed
            FROM regulation_files
            "#,
        )
        .map_err(|e| format!("读取旧数据库失败: {}", e))?;

    let rows = stmt
        .query_map([], |row| {
            Ok(regulation_db::RegulationFile {
                id: 0,
                title: row.get(0)?,
                doc_number: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                doc_type: row
                    .get::<_, Option<String>>(2)?
                    .unwrap_or_else(|| "regulation".to_string()),
                validity: String::new(),
                office_unit: String::new(),
                sign_date: String::new(),
                publish_date: String::new(),
                url: row.get(3)?,
                pdf_url: row.get(4)?,
                sha256: row.get(5)?,
                file_path: row.get(6)?,
                file_size: row.get::<_, Option<i64>>(7)?.unwrap_or(0),
                page_count: row.get::<_, Option<i32>>(8)?.unwrap_or(0),
                ocr_status: row
                    .get::<_, Option<String>>(9)?
                    .unwrap_or_else(|| "pending".to_string()),
                ocr_progress: row.get::<_, Option<i32>>(10)?.unwrap_or(0),
                ocr_current_page: row.get::<_, Option<i32>>(11)?.unwrap_or(0),
                ocr_error: row.get(12)?,
                indexed: row.get::<_, Option<i32>>(13)?.unwrap_or(0) != 0,
                indexed_at: None,
                created_at: String::new(),
                updated_at: String::new(),
                ocr_engine: "unknown".to_string(),
            })
        })
        .map_err(|e| format!("查询旧规章记录失败: {}", e))?;

    let mut total_found = 0usize;
    let mut imported = 0usize;
    let mut skipped = 0usize;
    let mut failed = 0usize;
    let mut copied_files = 0usize;

    let managed_target = resolve_target_dir(&app, None)?;
    if copy_files {
        std::fs::create_dir_all(&managed_target).map_err(|e| format!("创建受管目录失败: {}", e))?;
    }

    for row in rows {
        let mut file = match row {
            Ok(v) => v,
            Err(e) => {
                warn!("读取旧记录失败: {}", e);
                failed += 1;
                continue;
            }
        };
        total_found += 1;

        if regulation_db::file_exists_by_hash(&new_conn, &file.sha256).unwrap_or(false)
            || regulation_db::url_exists(&new_conn, &file.url).unwrap_or(false)
        {
            skipped += 1;
            continue;
        }

        if copy_files {
            let source_path = PathBuf::from(&file.file_path);
            if source_path.exists() {
                match resolve_storage_path(
                    &source_path,
                    &file.sha256,
                    Some(file.doc_number.as_str()),
                    Some(file.title.as_str()),
                    LocalCopyMode::CopyThenRegister,
                    &managed_target,
                ) {
                    Ok(target_path) => {
                        if target_path != source_path {
                            copied_files += 1;
                        }
                        file.file_path = target_path.to_string_lossy().to_string();
                        if file.url.starts_with("file:///") {
                            file.url = format!("file:///{}", file.file_path.replace('\\', "/"));
                        } else if file.url.starts_with("local://") {
                            file.url = format!("local://{}", file.file_path);
                        }
                    }
                    Err(e) => {
                        warn!("复制旧文件失败: {} - {}", file.title, e);
                        failed += 1;
                        continue;
                    }
                }
            }
        }

        match regulation_db::insert_file(&new_conn, &file) {
            Ok(new_id) => {
                if file.indexed {
                    let _ = regulation_db::mark_indexed(&new_conn, new_id);
                }
                imported += 1;
            }
            Err(e) => {
                warn!("导入记录失败: {} - {}", file.title, e);
                failed += 1;
            }
        }
    }

    let mut copied_index_files = 0usize;
    if copy_index {
        let src_index = legacy_dir.join("regulation_index");
        let dst_index = app_data_dir.join("regulation_index");
        copied_index_files = copy_dir_recursive(&src_index, &dst_index)?;
    }

    Ok(LegacyImportResponse {
        total_found,
        imported,
        skipped,
        failed,
        copied_files,
        copied_index_files,
    })
}

/// 服务器同步状态检查响应
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerSyncCheckResponse {
    /// 服务器 manifest 的 lastUpdated（ISO 8601）
    pub server_last_updated: String,
    /// 服务器总条目数
    pub server_total_count: usize,
    /// 本地上次同步时记录的服务器 lastUpdated（None = 从未同步）
    pub local_synced_server_last_updated: Option<String>,
    /// 本地最后一次同步发生时间
    pub local_synced_at: Option<String>,
    /// 服务器是否有更新（即服务器 > 本地记录）
    pub has_update: bool,
    /// 本地归档根目录
    pub local_root: String,
    /// 上次同步统计（如果有）
    pub last_sync_stats: Option<serde_json::Value>,
}

const SERVER_MANIFEST_URL: &str = "https://flighttoolbox.hudawang.cn/data/v1/manifest.json";

/// 检查服务器规章镜像是否有更新
///
/// 拉取服务器 manifest，对比本地 `.server_sync_state.json` 中记录的 serverLastUpdated。
/// 应用启动时调用，提示用户是否需要执行 `align_full.py + sync` 同步。
#[tauri::command]
pub async fn regulation_check_server_manifest<R: tauri::Runtime>(
    app: AppHandle<R>,
) -> Result<ServerSyncCheckResponse, String> {
    // 1. 拉取服务器 manifest
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

    let manifest: serde_json::Value = client
        .get(SERVER_MANIFEST_URL)
        .send()
        .await
        .map_err(|e| format!("拉取服务器 manifest 失败: {}", e))?
        .error_for_status()
        .map_err(|e| format!("服务器返回错误: {}", e))?
        .json()
        .await
        .map_err(|e| format!("解析 manifest JSON 失败: {}", e))?;

    let server_last_updated = manifest
        .get("lastUpdated")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let server_total_count = manifest
        .get("totalCount")
        .and_then(|v| v.as_u64())
        .unwrap_or(0) as usize;

    // 2. 解析本地归档根目录（优先用户配置，否则 AppData/regulations）
    let local_root = if let Some(config) = get_cached_config() {
        let custom_path = &config.advanced.regulation_storage_path;
        if !custom_path.is_empty() {
            PathBuf::from(custom_path)
        } else {
            app.path()
                .app_data_dir()
                .map_err(|e| format!("获取应用数据目录失败: {}", e))?
                .join("regulations")
        }
    } else {
        app.path()
            .app_data_dir()
            .map_err(|e| format!("获取应用数据目录失败: {}", e))?
            .join("regulations")
    };

    // 3. 读本地状态文件
    let state_path = local_root.join(".server_sync_state.json");
    let (local_synced_server_last_updated, local_synced_at, last_sync_stats) =
        if state_path.exists() {
            match std::fs::read_to_string(&state_path) {
                Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
                    Ok(state) => (
                        state
                            .get("serverLastUpdated")
                            .and_then(|v| v.as_str())
                            .map(String::from),
                        state.get("syncedAt").and_then(|v| v.as_str()).map(String::from),
                        state.get("syncStats").cloned(),
                    ),
                    Err(e) => {
                        warn!("解析 .server_sync_state.json 失败: {}", e);
                        (None, None, None)
                    }
                },
                Err(e) => {
                    warn!("读取 .server_sync_state.json 失败: {}", e);
                    (None, None, None)
                }
            }
        } else {
            (None, None, None)
        };

    // 4. 对比（ISO 8601 字符串字典序 == 时间序）
    let has_update = match &local_synced_server_last_updated {
        Some(local) => server_last_updated.as_str() > local.as_str(),
        None => true, // 从未同步过
    };

    info!(
        "服务器同步检查: server={}, local={:?}, has_update={}",
        server_last_updated, local_synced_server_last_updated, has_update
    );

    Ok(ServerSyncCheckResponse {
        server_last_updated,
        server_total_count,
        local_synced_server_last_updated,
        local_synced_at,
        has_update,
        local_root: local_root.to_string_lossy().to_string(),
        last_sync_stats,
    })
}

// ============================================================================
// 完整同步（Phase 2）：应用内一键从服务器对齐
// ============================================================================

/// 完整同步响应
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FullSyncResponse {
    pub caac_total: usize,
    pub matched: usize,
    pub meta_updated: usize,
    pub obsolete_marked: usize,
    pub downloaded: usize,
    pub download_failed: usize,
    pub download_skipped_no_url: usize,
    pub archive_renamed: usize,
    pub archive_copied: usize,
    pub archive_missing_source: usize,
    pub server_last_updated: String,
    pub synced_at: String,
}

/// 完整同步进度事件 payload
#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct FullSyncProgress {
    stage: String,
    current: usize,
    total: usize,
    message: String,
}

fn emit_full_sync_progress<R: tauri::Runtime>(
    app: &AppHandle<R>,
    stage: &str,
    current: usize,
    total: usize,
    message: &str,
) {
    let _ = app.emit(
        "regulation:full-sync-progress",
        FullSyncProgress {
            stage: stage.to_string(),
            current,
            total,
            message: message.to_string(),
        },
    );
}

const FULL_SYNC_INVALID_VALIDITY_LABELS: &[&str] = &["失效", "废止", "历史版本"];

/// 从 title 开头的标记前缀推断 validity，返回 (清理后的 title, 推断的 validity)
fn infer_title_validity_prefix(title: &str) -> (String, Option<&'static str>) {
    use std::sync::LazyLock;
    static RE: LazyLock<regex::Regex> = LazyLock::new(|| {
        regex::Regex::new(r"^[\[【\(（]?\s*(失效|废止|历史版本)\s*[\]】\)）!！:：\-\s]+")
            .expect("invalid title prefix regex should be valid")
    });
    let trimmed = title.trim();
    let caps = match RE.captures(trimmed) {
        Some(c) => c,
        None => return (trimmed.to_string(), None),
    };
    let label: Option<&'static str> = caps.get(1).and_then(|m| match m.as_str() {
        "失效" => Some("失效"),
        "废止" => Some("废止"),
        "历史版本" => Some("历史版本"),
        _ => None,
    });
    let cleaned = RE.replace(trimmed, "").trim().to_string();
    let final_title = if cleaned.is_empty() { trimmed.to_string() } else { cleaned };
    (final_title, label)
}

/// CAAC 静态镜像里的 doc_type 是中文，映射为数据库使用的英文
fn map_cn_doc_type_to_en(cn: &str) -> &'static str {
    match cn {
        "CCAR规章" | "regulation" => "regulation",
        "标准规范" | "standard" => "standard",
        "规范性文件" | "normative" => "normative",
        "advisory_circular" => "advisory_circular",
        "administrative_procedure" => "administrative_procedure",
        "information_bulletin" => "information_bulletin",
        "management_document" => "management_document",
        _ => "normative",
    }
}

/// 归档同步统计
struct ArchiveSyncStats {
    renamed: usize,
    copied: usize,
    missing_source: usize,
}

async fn download_and_save_pdf(
    client: &reqwest::Client,
    url: &str,
    target: &Path,
) -> Result<u64, String> {
    let parent = target.parent().ok_or_else(|| "目标路径无父目录".to_string())?;
    std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {}", e))?;

    let tmp = parent.join(format!(
        ".{}.tmp",
        target.file_name().and_then(|n| n.to_str()).unwrap_or("download")
    ));

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("HTTP 请求失败: {}", e))?
        .error_for_status()
        .map_err(|e| format!("HTTP 错误: {}", e))?;
    let bytes = response.bytes().await.map_err(|e| format!("读取响应内容失败: {}", e))?;
    let size = bytes.len() as u64;
    std::fs::write(&tmp, &bytes).map_err(|e| format!("写入临时文件失败: {}", e))?;
    std::fs::rename(&tmp, target).map_err(|e| format!("重命名失败: {}", e))?;
    Ok(size)
}

fn insert_new_regulation_from_online(
    conn: &rusqlite::Connection,
    doc: &OnlineRegulation,
    doc_type_en: &str,
    target: &Path,
    file_size: i64,
) -> Result<(), String> {
    let sha256 =
        calculate_file_hash(target).map_err(|e| format!("计算文件 hash 失败: {}", e))?;

    let file = regulation_db::RegulationFile {
        id: 0,
        title: doc.title.clone(),
        doc_number: doc.doc_number.clone(),
        doc_type: doc_type_en.to_string(),
        validity: doc.validity.clone(),
        office_unit: doc.office_unit.clone().unwrap_or_default(),
        sign_date: doc.sign_date.clone().unwrap_or_default(),
        publish_date: doc.publish_date.clone().unwrap_or_default(),
        url: doc.url.clone(),
        pdf_url: doc.pdf_url.clone(),
        sha256,
        file_path: target.to_string_lossy().to_string(),
        file_size,
        page_count: 0,
        ocr_status: "pending".to_string(),
        ocr_progress: 0,
        ocr_current_page: 0,
        ocr_error: None,
        indexed: false,
        indexed_at: None,
        created_at: String::new(),
        updated_at: String::new(),
        ocr_engine: "unknown".to_string(),
    };

    regulation_db::insert_file(conn, &file).map_err(|e| format!("INSERT 数据库失败: {}", e))?;
    Ok(())
}

/// 把数据库里所有记录按 desired 路径（分类 + [失效] 前缀 + title.pdf）对齐到归档目录。
///
/// 行为：
/// - source == target 跳过
/// - target 已存在 → 跳过（不覆盖）
/// - source 在归档根目录内 → `rename`（文件名更新为当前期望）
/// - source 在 hash 仓库 → `copy`
/// - 源文件不存在 → 计数 missing_source
fn sync_archive_to_desired(
    conn: &rusqlite::Connection,
    local_root: &Path,
) -> Result<ArchiveSyncStats, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, title, doc_type, validity, file_path FROM regulation_files",
        )
        .map_err(|e| format!("准备语句失败: {}", e))?;

    let rows: Vec<(i64, String, String, String, String)> = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                row.get::<_, Option<String>>(4)?.unwrap_or_default(),
            ))
        })
        .map_err(|e| format!("执行查询失败: {}", e))?
        .filter_map(|r| r.ok())
        .collect();
    drop(stmt);

    let mut renamed = 0usize;
    let mut copied = 0usize;
    let mut missing_source = 0usize;

    for (id, title, doc_type, validity, file_path) in rows {
        if file_path.is_empty() {
            continue;
        }
        let source = PathBuf::from(&file_path);
        if !source.exists() {
            missing_source += 1;
            continue;
        }

        let category = category_subdir_for_doc_type(&doc_type);
        let category_dir = local_root.join(category);
        if let Err(e) = std::fs::create_dir_all(&category_dir) {
            warn!("创建分类目录失败: {}", e);
            continue;
        }

        let base = sanitize_filename(&title);
        let validity_trim = validity.trim();
        let basename = if FULL_SYNC_INVALID_VALIDITY_LABELS.contains(&validity_trim) {
            format!("[{}] {}", validity_trim, base)
        } else {
            base
        };
        let target = category_dir.join(format!("{}.pdf", basename));

        if source == target {
            continue;
        }
        if target.exists() {
            continue;
        }

        let source_in_archive = source.starts_with(local_root);
        let op_result: Result<&str, std::io::Error> = if source_in_archive {
            std::fs::rename(&source, &target).map(|_| "rename")
        } else {
            std::fs::copy(&source, &target).map(|_| "copy")
        };

        match op_result {
            Ok("rename") => renamed += 1,
            Ok("copy") => copied += 1,
            Ok(_) => {}
            Err(e) => {
                warn!("归档同步失败 id={}: {}", id, e);
                continue;
            }
        }

        if let Err(e) = conn.execute(
            "UPDATE regulation_files SET file_path = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2",
            rusqlite::params![target.to_string_lossy().to_string(), id],
        ) {
            warn!("更新 file_path 失败 id={}: {}", id, e);
        }
    }

    Ok(ArchiveSyncStats { renamed, copied, missing_source })
}

/// 完整同步：应用内一键从服务器对齐
///
/// 流程：
/// 1. 拉服务器静态镜像全量（regulation + normative + specification）
/// 2. 多级 fallback 匹配本地数据库（url → doc_number → title → title 子串）
/// 3. UPDATE 匹配记录的 CAAC 元数据
/// 4. 对 DB-only 未标 validity 的记录，按 title 前缀推断 validity，未推断的标为"失效"
/// 5. 下载 CAAC 有但本地没有的 PDF 到归档对应分类目录，INSERT 数据库
/// 6. 把数据库所有记录的文件对齐到归档目录（含 [失效] 前缀重命名）
/// 7. 写 `<local_root>/.server_sync_state.json`
///
/// 全程通过 `regulation:full-sync-progress` 事件推送进度给前端。
#[tauri::command]
pub async fn regulation_full_sync_from_server<R: tauri::Runtime>(
    app: AppHandle<R>,
    cancel_state: State<'_, RegulationTaskCancelState>,
) -> Result<FullSyncResponse, String> {
    info!("[FullSync] 开始完整同步从服务器...");
    reset_cancel_flag(&cancel_state.full_sync);
    let cancel_flag = cancel_state.full_sync.clone();

    // ==================== Stage 1: 拉服务器全量 ====================
    emit_full_sync_progress(&app, "fetching", 0, 0, "正在拉取服务器镜像...");

    let searcher = CaacOnlineSearcher::new().map_err(|e| format!("创建搜索器失败: {}", e))?;
    let online_response = tokio::select! {
        result = searcher.fetch_all_static("all") => {
            result.map_err(|e| format!("拉服务器镜像失败: {}", e))?
        }
        _ = wait_for_cancel(cancel_flag.clone()) => return Err("同步已中止".to_string()),
    };

    if cancel_requested(&cancel_flag) {
        return Err("同步已中止".to_string());
    }

    let online_docs: Vec<OnlineRegulation> = online_response
        .documents
        .into_iter()
        .map(|doc: OnlineDocument| OnlineRegulation {
            title: doc.title,
            url: doc.url,
            pdf_url: if doc.pdf_url.is_empty() { None } else { Some(doc.pdf_url) },
            validity: doc.validity,
            doc_number: doc.doc_number,
            doc_type: doc.doc_type,
            publish_date: if doc.publish_date.is_empty() {
                None
            } else {
                Some(doc.publish_date)
            },
            sign_date: if doc.sign_date.is_empty() { None } else { Some(doc.sign_date) },
            office_unit: if doc.office_unit.is_empty() {
                None
            } else {
                Some(doc.office_unit)
            },
        })
        .collect();

    let caac_total = online_docs.len();
    emit_full_sync_progress(
        &app,
        "fetching",
        caac_total,
        caac_total,
        &format!("服务器 {} 条记录", caac_total),
    );

    // 拉 manifest 拿 lastUpdated（用于写 .server_sync_state.json）
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;

    let manifest: serde_json::Value = tokio::select! {
        result = async {
            client
                .get(SERVER_MANIFEST_URL)
                .send()
                .await
                .map_err(|e| format!("拉 manifest 失败: {}", e))?
                .error_for_status()
                .map_err(|e| format!("manifest HTTP 错误: {}", e))?
                .json()
                .await
                .map_err(|e| format!("解析 manifest 失败: {}", e))
        } => result?,
        _ = wait_for_cancel(cancel_flag.clone()) => return Err("同步已中止".to_string()),
    };
    let server_last_updated = manifest
        .get("lastUpdated")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let server_total_count =
        manifest.get("totalCount").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

    // ==================== Stage 2: 元数据对齐 ====================
    emit_full_sync_progress(&app, "metadata", 0, caac_total, "正在对齐元数据...");

    let conn = open_regulation_db(&app)?;
    let local_data = load_local_regulation_meta(&conn)?;
    let match_index = LocalRegulationMatchIndex::new(&local_data);

    let mut matched_urls: HashSet<String> = HashSet::new();
    let mut matched = 0usize;
    let mut meta_updated = 0usize;

    for (i, doc) in online_docs.iter().enumerate() {
        if cancel_requested(&cancel_flag) {
            return Err("同步已中止".to_string());
        }

        if i % 200 == 0 {
            emit_full_sync_progress(
                &app,
                "metadata",
                i,
                caac_total,
                &format!("对齐元数据 {}/{}", i, caac_total),
            );
        }
        if let Some(local_meta) = match_index.find(doc) {
            matched += 1;
            matched_urls.insert(local_meta.url.clone());
            let changed = regulation_db::update_official_metadata(
                &conn,
                &local_meta.url,
                &doc.title,
                &doc.doc_number,
                &doc.doc_type,
                &doc.validity,
                doc.office_unit.as_deref().unwrap_or_default(),
                doc.sign_date.as_deref().unwrap_or_default(),
                doc.publish_date.as_deref().unwrap_or_default(),
            )
            .unwrap_or(0);
            if changed > 0 {
                meta_updated += 1;
            }
        }
    }

    // 标记未匹配 + 未标 validity 的记录（按 title 前缀推断）
    let mut obsolete_marked = 0usize;
    for (url, meta) in &local_data {
        if cancel_requested(&cancel_flag) {
            return Err("同步已中止".to_string());
        }

        if matched_urls.contains(url) {
            continue;
        }
        if !meta.validity.trim().is_empty() {
            continue;
        }
        let (cleaned_title, inferred_label) = infer_title_validity_prefix(&meta.title);
        let new_validity = inferred_label.unwrap_or("失效");
        let sql_result = if cleaned_title != meta.title {
            conn.execute(
                "UPDATE regulation_files SET title = ?1, validity = ?2, updated_at = CURRENT_TIMESTAMP WHERE url = ?3",
                rusqlite::params![cleaned_title, new_validity, url],
            )
        } else {
            conn.execute(
                "UPDATE regulation_files SET validity = ?1, updated_at = CURRENT_TIMESTAMP WHERE url = ?2",
                rusqlite::params![new_validity, url],
            )
        };
        if let Err(e) = sql_result {
            warn!("更新失效状态失败 url={}: {}", url, e);
            continue;
        }
        obsolete_marked += 1;
    }
    emit_full_sync_progress(
        &app,
        "metadata",
        caac_total,
        caac_total,
        &format!("元数据对齐：更新 {}，标失效 {}", meta_updated, obsolete_marked),
    );

    // ==================== Stage 3: 下载缺失文件 ====================
    let missing_docs: Vec<&OnlineRegulation> =
        online_docs.iter().filter(|d| match_index.find(d).is_none()).collect();
    let missing_total = missing_docs.len();

    let local_root = resolve_target_dir(&app, None)?;
    std::fs::create_dir_all(&local_root).map_err(|e| format!("创建归档根目录失败: {}", e))?;

    let mut downloaded = 0usize;
    let mut download_failed = 0usize;
    let mut download_skipped_no_url = 0usize;

    for (i, doc) in missing_docs.iter().enumerate() {
        if cancel_requested(&cancel_flag) {
            return Err("同步已中止".to_string());
        }

        let preview: String = doc.title.chars().take(30).collect();
        emit_full_sync_progress(
            &app,
            "download",
            i,
            missing_total,
            &format!("下载 {}/{}: {}", i + 1, missing_total, preview),
        );

        let pdf_url = doc.pdf_url.as_deref().unwrap_or("").trim().to_string();
        if pdf_url.is_empty() {
            download_skipped_no_url += 1;
            continue;
        }

        let doc_type_en = map_cn_doc_type_to_en(&doc.doc_type);
        let category = category_subdir_for_doc_type(doc_type_en);
        let category_dir = local_root.join(category);

        let base = sanitize_filename(&doc.title);
        let validity_trim = doc.validity.trim();
        let filename = if FULL_SYNC_INVALID_VALIDITY_LABELS.contains(&validity_trim) {
            format!("[{}] {}.pdf", validity_trim, base)
        } else {
            format!("{}.pdf", base)
        };
        let target = category_dir.join(&filename);

        let file_size: i64 = if target.exists() {
            target.metadata().map(|m| m.len() as i64).unwrap_or(0)
        } else {
            match download_and_save_pdf(&client, &pdf_url, &target).await {
                Ok(size) => size as i64,
                Err(e) => {
                    if cancel_requested(&cancel_flag) {
                        return Err("同步已中止".to_string());
                    }
                    warn!("下载失败 {}: {}", doc.title, e);
                    download_failed += 1;
                    continue;
                }
            }
        };

        if let Err(e) = insert_new_regulation_from_online(&conn, doc, doc_type_en, &target, file_size)
        {
            warn!("写数据库失败 {}: {}", doc.title, e);
            download_failed += 1;
            continue;
        }
        downloaded += 1;
    }
    emit_full_sync_progress(
        &app,
        "download",
        missing_total,
        missing_total,
        &format!("下载：{} 成功，{} 失败，{} 跳过(无URL)", downloaded, download_failed, download_skipped_no_url),
    );

    // ==================== Stage 4: 归档同步 ====================
    if cancel_requested(&cancel_flag) {
        return Err("同步已中止".to_string());
    }

    emit_full_sync_progress(&app, "archive", 0, 0, "同步归档目录...");
    let archive_stats = sync_archive_to_desired(&conn, &local_root)?;
    emit_full_sync_progress(
        &app,
        "archive",
        1,
        1,
        &format!(
            "归档：重命名 {}，复制 {}，源缺失 {}",
            archive_stats.renamed, archive_stats.copied, archive_stats.missing_source
        ),
    );

    // ==================== Stage 5: 写状态文件 ====================
    let synced_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let state_path = local_root.join(".server_sync_state.json");
    let state = serde_json::json!({
        "schemaVersion": 1,
        "serverLastUpdated": server_last_updated,
        "serverTotalCount": server_total_count,
        "syncedAt": synced_at,
        "syncStats": {
            "source": "app-full-sync",
            "caacTotal": caac_total,
            "matched": matched,
            "metaUpdated": meta_updated,
            "obsoleteMarked": obsolete_marked,
            "downloaded": downloaded,
            "downloadFailed": download_failed,
            "downloadSkippedNoUrl": download_skipped_no_url,
            "archiveRenamed": archive_stats.renamed,
            "archiveCopied": archive_stats.copied,
            "archiveMissingSource": archive_stats.missing_source,
        }
    });
    std::fs::write(
        &state_path,
        serde_json::to_string_pretty(&state).unwrap_or_default(),
    )
    .map_err(|e| format!("写 .server_sync_state.json 失败: {}", e))?;

    info!(
        "[FullSync] 完成: 匹配 {}, 更新 {}, 失效标记 {}, 下载 {}, 失败 {}, 归档 rename {} / copy {}",
        matched,
        meta_updated,
        obsolete_marked,
        downloaded,
        download_failed,
        archive_stats.renamed,
        archive_stats.copied
    );

    emit_full_sync_progress(&app, "done", caac_total, caac_total, "同步完成");

    Ok(FullSyncResponse {
        caac_total,
        matched,
        meta_updated,
        obsolete_marked,
        downloaded,
        download_failed,
        download_skipped_no_url,
        archive_renamed: archive_stats.renamed,
        archive_copied: archive_stats.copied,
        archive_missing_source: archive_stats.missing_source,
        server_last_updated,
        synced_at,
    })
}

/// 请求中止“应用内完整同步”。
#[tauri::command]
pub async fn regulation_cancel_full_sync(
    cancel_state: State<'_, RegulationTaskCancelState>,
) -> Result<bool, String> {
    cancel_state.full_sync.store(true, Ordering::Relaxed);
    Ok(true)
}

#[cfg(test)]
mod is_pdf_path_tests {
    use super::is_pdf_path;
    use super::{
        is_invalid_regulation_file_record, is_supported_local_scan_path, load_local_text_content,
        is_supported_realign_filename_path,
    };
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn accepts_lowercase_pdf() {
        assert!(is_pdf_path(&PathBuf::from(r"D:\docs\foo.pdf")));
        assert!(is_pdf_path(&PathBuf::from("/tmp/foo.pdf")));
    }

    #[test]
    fn accepts_uppercase_or_mixed_case_pdf() {
        assert!(is_pdf_path(&PathBuf::from(r"D:\docs\FOO.PDF")));
        assert!(is_pdf_path(&PathBuf::from(r"D:\docs\Foo.Pdf")));
    }

    #[test]
    fn rejects_non_pdf_extensions() {
        assert!(!is_pdf_path(&PathBuf::from(r"D:\docs\foo.txt")));
        assert!(!is_pdf_path(&PathBuf::from(r"D:\docs\foo.doc")));
        assert!(!is_pdf_path(&PathBuf::from(r"D:\docs\foo.docx")));
        assert!(!is_pdf_path(&PathBuf::from(r"D:\docs\foo.PDFX")));
    }

    #[test]
    fn rejects_paths_without_extension() {
        assert!(!is_pdf_path(&PathBuf::from(r"D:\docs\foo")));
        assert!(!is_pdf_path(&PathBuf::from(r"D:\docs\")));
        assert!(!is_pdf_path(&PathBuf::from("")));
    }

    #[test]
    fn rejects_pdf_substring_in_filename_without_real_extension() {
        // 文件名里有 "pdf" 字串但扩展名不是 .pdf 应被拒绝
        assert!(!is_pdf_path(&PathBuf::from(r"D:\docs\my-pdf-notes.txt")));
        assert!(!is_pdf_path(&PathBuf::from(r"D:\pdf-archive\readme.md")));
    }

    #[test]
    fn rejects_chinese_dot_in_filename_without_real_extension() {
        // 实际数据里出现过的形态：标题里有句号或括号
        assert!(!is_pdf_path(&PathBuf::from(r"D:\docs\民航法规.txt")));
        assert!(is_pdf_path(&PathBuf::from(r"D:\docs\民航法规.pdf")));
    }

    #[test]
    fn accepts_txt_as_supported_local_scan_source() {
        assert!(is_supported_local_scan_path(&PathBuf::from(r"D:\docs\规章摘要.txt")));
        assert!(is_supported_local_scan_path(&PathBuf::from(r"D:\docs\规章摘要.TXT")));
        assert!(is_supported_local_scan_path(&PathBuf::from(r"D:\docs\规章摘要.pdf")));
        assert!(!is_supported_local_scan_path(&PathBuf::from(r"D:\docs\规章摘要.docx")));
    }

    #[test]
    fn accepts_txt_and_pdf_as_realign_filename_targets() {
        assert!(is_supported_realign_filename_path(&PathBuf::from(r"D:\docs\规章摘要.txt")));
        assert!(is_supported_realign_filename_path(&PathBuf::from(r"D:\docs\规章摘要.TXT")));
        assert!(is_supported_realign_filename_path(&PathBuf::from(r"D:\docs\规章摘要.pdf")));
        assert!(!is_supported_realign_filename_path(&PathBuf::from(r"D:\docs\规章摘要.docx")));
    }

    #[test]
    fn loads_local_text_content_for_txt_files() {
        let temp_dir = TempDir::new().unwrap();
        let txt_path = temp_dir.path().join("regulation.txt");
        std::fs::write(&txt_path, "检查员 岗位职责\n局方要求").unwrap();

        let content = load_local_text_content(&txt_path).unwrap();

        assert!(content.contains("检查员"));
        assert!(content.contains("局方要求"));
    }

    #[test]
    fn loads_gbk_encoded_local_text_content() {
        let temp_dir = TempDir::new().unwrap();
        let txt_path = temp_dir.path().join("regulation-gbk.txt");
        let (gbk_bytes, _, _) = encoding_rs::GBK.encode("检查员 岗位职责\n局方要求");
        std::fs::write(&txt_path, gbk_bytes.as_ref()).unwrap();

        let content = load_local_text_content(&txt_path).unwrap();

        assert!(content.contains("检查员"));
        assert!(content.contains("局方要求"));
    }

    #[test]
    fn cleanup_keeps_existing_txt_records() {
        let temp_dir = TempDir::new().unwrap();
        let txt_path = temp_dir.path().join("regulation.txt");
        std::fs::write(&txt_path, "检查员 岗位职责\n局方要求").unwrap();

        assert!(!is_invalid_regulation_file_record(&txt_path));
    }
}
