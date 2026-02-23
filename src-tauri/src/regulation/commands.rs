//! Tauri 命令接口
//!
//! 提供前端调用的规章索引相关命令。

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tracing::{debug, info, warn};

use super::index::RegulationIndex;
use super::schema::RegulationDocument;
use super::search::{sort_results, SortOrder};
use crate::database::regulation as regulation_db;

/// 规章索引状态（Tauri 管理）
pub struct RegulationIndexState {
    pub index: Mutex<Option<RegulationIndex>>,
}

impl Default for RegulationIndexState {
    fn default() -> Self {
        Self {
            index: Mutex::new(None),
        }
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

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取应用数据目录失败: {}", e))?;
    Ok(app_data_dir.join("regulations"))
}

fn sanitize_filename(input: &str) -> String {
    let mut out = input
        .chars()
        .map(|c| {
            if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') {
                '_'
            } else {
                c
            }
        })
        .collect::<String>();

    out = out.trim().to_string();
    if out.is_empty() {
        "document".to_string()
    } else {
        out.chars().take(180).collect::<String>()
    }
}

fn normalize_extension(original_name: Option<&str>, default_ext: &str) -> String {
    original_name
        .and_then(|n| Path::new(n).extension())
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_ext.to_string())
}

fn resolve_storage_path(
    source_path: &Path,
    sha256: &str,
    mode: LocalCopyMode,
    target_dir: &Path,
) -> Result<PathBuf, String> {
    if mode == LocalCopyMode::RegisterOnly {
        return Ok(source_path.to_path_buf());
    }

    std::fs::create_dir_all(target_dir)
        .map_err(|e| format!("创建目标目录失败: {}", e))?;

    let ext = source_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("pdf")
        .to_lowercase();
    let target_path = target_dir.join(format!("{}.{}", &sha256[..16], ext));

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
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取应用数据目录失败: {}", e))?;

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
                        "SELECT title, doc_number, doc_type, url, file_path, ocr_status \
                         FROM regulation_files WHERE indexed = 1 OR ocr_status = 'done'"
                    ).map_err(|e| format!("查询失败: {}", e))?;

                    let files: Vec<(String, String, String, String, String, String)> = stmt.query_map([], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, String>(5)?,
                        ))
                    }).map_err(|e| format!("查询失败: {}", e))?
                    .filter_map(|r| r.ok())
                    .collect();

                    let file_count = files.len();
                    if file_count > 0 {
                        info!("从数据库恢复 {} 个文件到索引", file_count);

                        for (title, doc_number, doc_type, url, file_path, _ocr_status) in &files {
                            let doc = RegulationDocument {
                                title: title.clone(),
                                doc_number: doc_number.clone(),
                                validity: String::new(), // 从数据库中没有存有效性
                                doc_type: doc_type.clone(),
                                office_unit: String::new(),
                                sign_date: String::new(),
                                publish_date: String::new(),
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

    let index = state_guard
        .as_ref()
        .ok_or("索引未初始化，请先调用 regulation_index_init")?;

    // 执行搜索
    let validity = if request.validity == "all" {
        None
    } else {
        Some(request.validity.as_str())
    };

    let doc_type = if request.doc_type == "all" {
        None
    } else {
        Some(request.doc_type.as_str())
    };

    let mut results = index
        .search_with_filter(&request.query, validity, doc_type, request.limit)
        .map_err(|e| format!("搜索失败: {}", e))?;

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

    let index = state_guard
        .as_ref()
        .ok_or("索引未初始化")?;

    // 检查是否已存在
    if index.exists(&document.url) {
        debug!("文档已存在，跳过: {}", document.url);
        return Ok(false);
    }

    index
        .add_document(&document)
        .map_err(|e| format!("添加文档失败: {}", e))?;

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

    let index = state_guard
        .as_ref()
        .ok_or("索引未初始化")?;

    // 过滤已存在的文档
    let new_docs: Vec<_> = documents
        .into_iter()
        .filter(|doc| !index.exists(&doc.url))
        .collect();

    if new_docs.is_empty() {
        return Ok(0);
    }

    let count = index
        .add_documents(&new_docs)
        .map_err(|e| format!("批量添加失败: {}", e))?;

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
        None => Ok(IndexStats {
            doc_count: 0,
            index_path: String::new(),
            initialized: false,
        }),
    }
}

/// 清空索引
#[tauri::command]
pub async fn regulation_index_clear(
    state: State<'_, RegulationIndexState>,
) -> Result<(), String> {
    info!("清空规章索引");

    let state_guard = state.index.lock().map_err(|e| format!("锁定状态失败: {}", e))?;

    let index = state_guard
        .as_ref()
        .ok_or("索引未初始化")?;

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

    let index = state_guard
        .as_ref()
        .ok_or("索引未初始化")?;

    Ok(index.exists(&url))
}

// ============================================================================
// 批量下载相关命令
// ============================================================================

use crate::database::regulation::SyncStatus;
use super::crawler::{RegulationCrawler, DownloadConfig, DownloadItem};
use super::sync::BatchProgress;

/// 批量下载状态（Tauri 管理）
pub struct BatchDownloadState {
    /// 下载进度
    pub progress: Mutex<BatchProgress>,
    /// 是否正在下载
    pub is_downloading: Mutex<bool>,
}

impl Default for BatchDownloadState {
    fn default() -> Self {
        Self {
            progress: Mutex::new(BatchProgress::default()),
            is_downloading: Mutex::new(false),
        }
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

fn extract_attachment_url(html: &str, detail_url: &str, exts: &[&str]) -> Option<String> {
    let ext_pattern = exts
        .iter()
        .map(|s| regex::escape(s))
        .collect::<Vec<_>>()
        .join("|");
    let pattern = format!(r#"href\s*=\s*"([^"]+({}))""#, ext_pattern);
    let re = regex::Regex::new(&pattern).ok()?;
    let href = re.captures(html)?.get(1)?.as_str();
    build_absolute_url(detail_url, href)
}

fn extract_text_from_html_body(html: &str) -> String {
    let doc = scraper::Html::parse_document(html);
    let selector = scraper::Selector::parse("body").expect("body selector should be valid");
    if let Some(body) = doc.select(&selector).next() {
        body.text()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        String::new()
    }
}

/// 单文件下载（详情页附件解析 + 回退）
#[tauri::command]
pub async fn regulation_download_single<R: tauri::Runtime>(
    app: AppHandle<R>,
    request: SingleDownloadRequest,
    index_state: State<'_, RegulationIndexState>,
) -> Result<SingleDownloadResponse, String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取应用数据目录失败: {}", e))?;
    let db_path = app_data_dir.join("history.db");
    let conn = rusqlite::Connection::open(&db_path).map_err(|e| format!("打开数据库失败: {}", e))?;
    regulation_db::init_regulation_schema(&conn).map_err(|e| format!("初始化规章表失败: {}", e))?;

    if let Ok(Some(existing)) = regulation_db::get_file_by_url(&conn, &request.document.url) {
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
        app_data_dir.join("regulations")
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

    let mut download_url = if !doc.pdf_url.trim().is_empty() {
        Some(doc.pdf_url.trim().to_string())
    } else if doc.url.to_lowercase().ends_with(".pdf") {
        Some(doc.url.clone())
    } else {
        None
    };

    if download_url.is_none() && request.prefer_attachment {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .map_err(|e| format!("创建 HTTP 客户端失败: {}", e))?;
        let resp = client
            .get(&doc.url)
            .send()
            .await
            .map_err(|e| format!("访问详情页失败: {}", e))?;
        let html = resp.text().await.map_err(|e| format!("读取详情页失败: {}", e))?;

        download_url = super::crawler::extract_pdf_url(&doc.url, &html)
            .or_else(|| extract_attachment_url(&html, &doc.url, &[".pdf"]));

        if download_url.is_none() {
            download_url = extract_attachment_url(&html, &doc.url, &[".docx", ".doc"]);
        }

        // 若仍未找到可下载附件，则回退为正文 TXT
        if download_url.is_none() {
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

    if let Some(url) = download_url {
        let ext = normalize_extension(Some(&url), "pdf");
        let result = crawler
            .download_file(&url, Some(&format!("{}.{}", sanitize_filename(&doc.title), ext)))
            .await
            .map_err(|e| format!("下载失败: {}", e))?;

        resolved_pdf_url = result.pdf_url.clone();
        sha256 = result.sha256.clone();
        file_size = result.file_size as i64;
        file_path = result.file_path.to_string_lossy().to_string();
        file_type = ext;

        let extraction = text_extractor::extract_text_from_pdf(Path::new(&file_path))
            .map_err(|e| format!("文本提取失败: {}", e))?;
        content = extraction.text;
        needs_ocr = extraction.needs_ocr;
    }

    if file_path.is_empty() {
        return Ok(SingleDownloadResponse {
            success: false,
            file_path: String::new(),
            file_type: String::new(),
            error: Some("未找到可下载的附件，且正文提取失败".to_string()),
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
        url: doc.url.clone(),
        pdf_url: resolved_pdf_url,
        sha256: sha256.clone(),
        file_path: file_path.clone(),
        file_size,
        ocr_status,
        ..Default::default()
    };

    let file_id = regulation_db::insert_file(&conn, &db_file).map_err(|e| format!("写入数据库失败: {}", e))?;

    if !needs_ocr && !content.is_empty() {
        let index_doc = RegulationDocument {
            title: doc.title,
            doc_number: doc.doc_number,
            validity: String::new(),
            doc_type: if db_file.doc_type.is_empty() {
                "regulation".to_string()
            } else {
                db_file.doc_type.clone()
            },
            office_unit: String::new(),
            sign_date: String::new(),
            publish_date: String::new(),
            url: db_file.url,
            file_path: file_path.clone(),
            content,
        };

        let state_guard = index_state
            .index
            .lock()
            .map_err(|e| format!("锁定索引状态失败: {}", e))?;
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

    Ok(SingleDownloadResponse {
        success: true,
        file_path,
        file_type,
        error: None,
    })
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
        let is_downloading = download_state.is_downloading.lock()
            .map_err(|e| format!("锁定状态失败: {}", e))?;
        if *is_downloading {
            return Err("已有下载任务正在进行".to_string());
        }
    }

    // 设置下载中状态
    {
        let mut is_downloading = download_state.is_downloading.lock()
            .map_err(|e| format!("锁定状态失败: {}", e))?;
        *is_downloading = true;
    }

    // 获取保存目录
    let save_dir = if let Some(dir) = request.save_dir {
        std::path::PathBuf::from(dir)
    } else {
        let app_data_dir = app
            .path()
            .app_data_dir()
            .map_err(|e| format!("获取应用数据目录失败: {}", e))?;
        app_data_dir.join("regulations")
    };

    // 创建下载器
    let config = DownloadConfig {
        save_dir: save_dir.clone(),
        max_concurrent: 2,
        delay_ms: 3000,
        ..Default::default()
    };

    let crawler = RegulationCrawler::new(config)
        .map_err(|e| format!("创建下载器失败: {}", e))?;

    // 转换下载项
    let items: Vec<DownloadItem> = request.items
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
    let results = crawler.batch_download(items, |progress| {
        // 更新进度状态
        if let Ok(mut p) = download_state.progress.lock() {
            *p = progress.clone();
        }

        // 发送进度事件到前端
        if let Err(e) = app.emit("regulation:download-progress", progress) {
            debug!("发送下载进度事件失败: {}", e);
        }
    }).await;

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
        let mut is_downloading = download_state.is_downloading.lock()
            .map_err(|e| format!("锁定状态失败: {}", e))?;
        *is_downloading = false;
    }

    info!(
        "批量下载完成: 成功 {}, 跳过 {}, 失败 {}",
        success, skipped, failed
    );

    Ok(BatchDownloadResponse {
        success,
        skipped,
        failed,
        failed_urls,
    })
}

/// 获取下载进度
#[tauri::command]
pub async fn regulation_get_download_progress(
    state: State<'_, BatchDownloadState>,
) -> Result<BatchProgress, String> {
    let progress = state.progress.lock()
        .map_err(|e| format!("锁定状态失败: {}", e))?;
    Ok(progress.clone())
}

/// 获取同步状态（文件统计）
#[tauri::command]
pub async fn regulation_get_sync_status<R: tauri::Runtime>(
    app: AppHandle<R>,
) -> Result<SyncStatus, String> {
    // 获取数据库路径
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取应用数据目录失败: {}", e))?;

    let db_path = app_data_dir.join("history.db");

    // 打开数据库连接
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("打开数据库失败: {}", e))?;

    // 初始化规章表（如果不存在）
    regulation_db::init_regulation_schema(&conn)
        .map_err(|e| format!("初始化规章表失败: {}", e))?;

    // 获取统计信息
    regulation_db::get_sync_status(&conn)
        .map_err(|e| format!("获取同步状态失败: {}", e))
}

// ============================================================================
// PDF 文本提取 + 索引命令
// ============================================================================

use super::text_extractor;

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
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取应用数据目录失败: {}", e))?;
    let db_path = app_data_dir.join("history.db");

    // 打开数据库连接
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("打开数据库失败: {}", e))?;

    regulation_db::init_regulation_schema(&conn)
        .map_err(|e| format!("初始化规章表失败: {}", e))?;

    // 获取待处理文件
    let pending_files = regulation_db::get_pending_ocr_files(&conn, batch_size)
        .map_err(|e| format!("获取待处理文件失败: {}", e))?;

    if pending_files.is_empty() {
        info!("没有待处理的文件");
        return Ok(ProcessFilesResponse {
            processed: 0,
            indexed: 0,
            needs_ocr: 0,
            failed: 0,
        });
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
                    validity: "有效".to_string(),
                    doc_type: file.doc_type.clone(),
                    office_unit: String::new(),
                    sign_date: String::new(),
                    publish_date: String::new(),
                    url: file.url.clone(),
                    file_path: file.file_path.clone(),
                    content: extraction.text,
                };

                // 写入 Tantivy 索引
                let index_result = {
                    let state_guard = index_state.index.lock()
                        .map_err(|e| format!("锁定索引状态失败: {}", e))?;
                    if let Some(index) = state_guard.as_ref() {
                        if !index.exists(&doc.url) {
                            index.add_document(&doc)
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
                        let _ = regulation_db::update_ocr_status(
                            &conn, file.id, "done", 100, 0, None,
                        );
                        let _ = regulation_db::mark_indexed(&conn, file.id);
                        indexed += 1;
                        info!("文件已索引: {}", file.title);
                    }
                    Err(e) => {
                        let _ = regulation_db::update_ocr_status(
                            &conn, file.id, "failed", 0, 0, Some(&e),
                        );
                        failed += 1;
                        warn!("索引写入失败: {} - {}", file.title, e);
                    }
                }
            }
            Ok(_extraction) => {
                // 文本不足，标记需要 OCR
                let _ = regulation_db::update_ocr_status(
                    &conn, file.id, "pending", 0, 0,
                    Some("文本不足，需要 OCR"),
                );
                needs_ocr += 1;
                info!("文件需要 OCR: {}", file.title);
            }
            Err(e) => {
                let _ = regulation_db::update_ocr_status(
                    &conn, file.id, "failed", 0, 0, Some(&e),
                );
                failed += 1;
                warn!("文本提取失败: {} - {}", file.title, e);
            }
        }

        // 发送进度事件
        if let Err(e) = app.emit("regulation:process-progress", serde_json::json!({
            "current": file.title,
            "indexed": indexed,
            "needs_ocr": needs_ocr,
            "failed": failed,
        })) {
            debug!("发送处理进度事件失败: {}", e);
        }
    }

    let processed = pending_files.len();
    info!(
        "文件处理完成: 处理 {}, 索引 {}, 需OCR {}, 失败 {}",
        processed, indexed, needs_ocr, failed
    );

    Ok(ProcessFilesResponse {
        processed,
        indexed,
        needs_ocr,
        failed,
    })
}

// ============================================================================
// 本地目录扫描命令
// ============================================================================

use super::sync::calculate_file_hash;
use std::collections::HashSet;

/// 扫描进度事件
#[derive(Debug, Clone, Serialize)]
pub struct ScanProgress {
    /// 已扫描文件数
    pub scanned: usize,
    /// 发现的 PDF 文件总数
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
    /// 发现的 PDF 文件总数
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
    /// 非 PDF 文件数（跳过）
    pub skipped_non_pdf: usize,
    /// OCR 成功索引数
    pub ocr_success: usize,
    /// OCR 失败数
    pub ocr_failed: usize,
}

/// 从文件名解析规章元数据
fn parse_filename_metadata(filename: &str) -> (String, String, String) {
    // 移除扩展名
    let name = filename
        .rsplit_once('.')
        .map(|(n, _)| n)
        .unwrap_or(filename);

    // 尝试提取文号（常见格式）
    let doc_number_patterns = [
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
    ];

    let mut doc_number = String::new();
    for pattern in &doc_number_patterns {
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
    } else {
        "normative".to_string()
    };

    // 标题：使用清理后的文件名
    let title = name
        .replace('_', " ")
        .trim()
        .to_string();

    (title, doc_number, doc_type)
}

/// 递归收集目录下的所有 PDF 文件
fn collect_pdf_files(dir: &std::path::Path, recursive: bool) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();

    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            warn!("无法读取目录 {:?}: {}", dir, e);
            return files;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && recursive {
            files.extend(collect_pdf_files(&path, true));
        } else if path.is_file() {
            if let Some(ext) = path.extension() {
                let ext_lower = ext.to_string_lossy().to_lowercase();
                if ext_lower == "pdf" {
                    files.push(path);
                }
            }
        }
    }

    files
}

/// 扫描本地目录，将 PDF 文件入库 + 入索引 + 自动 OCR
///
/// # 参数
/// - `dir_path`: 要扫描的目录路径
/// - `recursive`: 是否递归扫描子目录
/// - `auto_ocr`: 是否自动对扫描版 PDF 执行 OCR（默认 true）
///
/// # 流程
/// 1. 递归遍历目录，收集所有 PDF 文件
/// 2. 对每个文件计算 SHA256
/// 3. 数据库去重检查（同哈希 = 同文件，跳过）
/// 4. 从文件名智能解析文号、类型等元数据
/// 5. 用 pdfium-render 提取文本
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

    // Phase 1: 发现所有 PDF 文件
    if let Err(e) = app.emit("regulation:scan-progress", ScanProgress {
        scanned: 0,
        total_found: 0,
        new_files: 0,
        duplicates: 0,
        indexed: 0,
        needs_ocr: 0,
        failed: 0,
        current_file: Some("正在扫描目录...".to_string()),
        phase: "discovering".to_string(),
        ocr_processed: None,
        ocr_total: None,
    }) {
        debug!("发送扫描进度事件失败: {}", e);
    }

    let pdf_files = collect_pdf_files(dir, recursive);
    let total_found = pdf_files.len();
    info!("发现 {} 个 PDF 文件", total_found);

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

    // 获取数据库连接
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取应用数据目录失败: {}", e))?;
    let db_path = app_data_dir.join("history.db");
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("打开数据库失败: {}", e))?;

    regulation_db::init_regulation_schema(&conn)
        .map_err(|e| format!("初始化规章表失败: {}", e))?;

    // 加载已有哈希用于快速去重
    let existing_hashes: HashSet<String> = {
        let mut stmt = conn.prepare(
            "SELECT sha256 FROM regulation_files"
        ).map_err(|e| format!("查询已有哈希失败: {}", e))?;
        
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| format!("读取哈希失败: {}", e))?
            .filter_map(|r| r.ok())
            .collect();
        rows
    };
    info!("已有 {} 个文件哈希用于去重", existing_hashes.len());

    // 也加载已有文件路径用于去重
    let existing_paths: HashSet<String> = {
        let mut stmt = conn.prepare(
            "SELECT file_path FROM regulation_files"
        ).map_err(|e| format!("查询已有路径失败: {}", e))?;
        
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| format!("读取路径失败: {}", e))?
            .filter_map(|r| r.ok())
            .collect();
        rows
    };

    // Phase 2: 处理每个 PDF 文件
    let mut new_files = 0;
    let mut duplicates = 0;
    let mut indexed = 0;
    let mut needs_ocr = 0;
    let mut failed = 0;
    let mut batch_docs = Vec::new();
    let batch_commit_size = 20; // 每 20 个文件批量提交一次索引

    for (i, pdf_path) in pdf_files.iter().enumerate() {
        let filename = pdf_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        let file_path_str = pdf_path.to_string_lossy().to_string();

        // 发送进度
        if i % 5 == 0 || i == total_found - 1 {
            if let Err(e) = app.emit("regulation:scan-progress", ScanProgress {
                scanned: i + 1,
                total_found,
                new_files,
                duplicates,
                indexed,
                needs_ocr,
                failed,
                current_file: Some(filename.clone()),
                phase: "processing".to_string(),
                ocr_processed: None,
                ocr_total: None,
            }) {
                debug!("发送扫描进度事件失败: {}", e);
            }
        }

        // register_only 模式才按路径判重
        if copy_mode == LocalCopyMode::RegisterOnly && existing_paths.contains(&file_path_str) {
            duplicates += 1;
            debug!("文件路径已存在，跳过: {}", filename);
            continue;
        }

        // 计算 SHA256
        let sha256 = match calculate_file_hash(pdf_path) {
            Ok(hash) => hash,
            Err(e) => {
                failed += 1;
                warn!("计算哈希失败: {} - {}", filename, e);
                continue;
            }
        };

        // 检查哈希去重
        if existing_hashes.contains(&sha256) {
            duplicates += 1;
            debug!("文件哈希已存在（重复文件），跳过: {}", filename);
            continue;
        }

        // 计算导入后的存储路径
        let stored_path = match resolve_storage_path(pdf_path, &sha256, copy_mode, &target_dir) {
            Ok(path) => path,
            Err(e) => {
                failed += 1;
                warn!("复制文件失败: {} - {}", filename, e);
                continue;
            }
        };
        let stored_path_str = stored_path.to_string_lossy().to_string();

        // 从文件名解析元数据
        let (title, doc_number, doc_type) = parse_filename_metadata(&filename);

        // 获取文件大小
        let file_size = std::fs::metadata(&stored_path)
            .map(|m| m.len() as i64)
            .unwrap_or(0);

        // 提取 PDF 文本
        let (content, text_status, text_needs_ocr) =
            match text_extractor::extract_text_from_pdf(&stored_path) {
            Ok(extraction) if !extraction.needs_ocr => {
                (extraction.text, "done", false)
            }
            Ok(extraction) => {
                (extraction.text, "pending", true)
            }
            Err(e) => {
                debug!("文本提取失败: {} - {}", filename, e);
                (String::new(), "pending", true)
            }
        };

        // 使用 file:// URL 作为唯一标识（本地文件）
        let file_url = format!("file:///{}", stored_path_str.replace('\\', "/"));

        // 插入数据库
        let db_file = regulation_db::RegulationFile {
            title: title.clone(),
            doc_number: doc_number.clone(),
            doc_type: doc_type.clone(),
            url: file_url.clone(),
            pdf_url: None,
            sha256: sha256.clone(),
            file_path: stored_path_str.clone(),
            file_size,
            ocr_status: text_status.to_string(),
            ..Default::default()
        };

        match regulation_db::insert_file(&conn, &db_file) {
            Ok(file_id) => {
                new_files += 1;

                if !text_needs_ocr && !content.is_empty() {
                    // 文本充足，准备写入 Tantivy 索引
                    let reg_doc = super::schema::RegulationDocument {
                        title,
                        doc_number,
                        validity: String::new(), // 本地文件暂无有效性信息
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

        // 批量提交 Tantivy 索引
        if batch_docs.len() >= batch_commit_size {
            commit_batch_to_index(&batch_docs, &index_state, &conn)?;
            batch_docs.clear();
        }
    }

    // 提交剩余的文档
    if !batch_docs.is_empty() {
        commit_batch_to_index(&batch_docs, &index_state, &conn)?;
    }

    info!(
        "扫描完成: 发现 {}, 新增 {}, 重复 {}, 索引 {}, 需OCR {}, 失败 {}",
        total_found, new_files, duplicates, indexed, needs_ocr, failed
    );

    // Phase 3: 自动 OCR 处理（如果有需要 OCR 的文件且启用了 auto_ocr）
    let mut ocr_success = 0;
    let mut ocr_failed = 0;

    if auto_ocr && needs_ocr > 0 {
        info!("开始自动 OCR 处理 {} 个扫描版文件", needs_ocr);

        // 发送 OCR 阶段进度
        if let Err(e) = app.emit("regulation:scan-progress", ScanProgress {
            scanned: total_found,
            total_found,
            new_files,
            duplicates,
            indexed,
            needs_ocr,
            failed,
            current_file: Some("准备 OCR 处理...".to_string()),
            phase: "ocr".to_string(),
            ocr_processed: Some(0),
            ocr_total: Some(needs_ocr),
        }) {
            debug!("发送 OCR 阶段进度事件失败: {}", e);
        }

        // 获取所有待 OCR 文件
        let pending_files = regulation_db::get_pending_ocr_files(&conn, needs_ocr)
            .map_err(|e| format!("获取待 OCR 文件失败: {}", e))?;

        for (i, file) in pending_files.iter().enumerate() {
            // 更新状态为 processing
            let _ = regulation_db::update_ocr_status(&conn, file.id, "processing", 0, 0, None);

            // 发送 OCR 进度
            if let Err(e) = app.emit("regulation:scan-progress", ScanProgress {
                scanned: total_found,
                total_found,
                new_files,
                duplicates,
                indexed,
                needs_ocr,
                failed,
                current_file: Some(file.title.clone()),
                phase: "ocr".to_string(),
                ocr_processed: Some(i),
                ocr_total: Some(pending_files.len()),
            }) {
                debug!("发送 OCR 进度事件失败: {}", e);
            }

            // Step 1: 先重试 pdfium 文本提取
            let pdf_path = std::path::Path::new(&file.file_path);
            let extraction = super::text_extractor::extract_text_from_pdf(pdf_path);

            match extraction {
                Ok(result) if !result.needs_ocr => {
                    // pdfium 提取成功
                    match write_to_index(&index_state, &conn, file, result.text) {
                        Ok(()) => {
                            ocr_success += 1;
                            info!("pdfium 文本提取重试成功: {}", file.title);
                        }
                        Err(e) => {
                            let _ = regulation_db::update_ocr_status(
                                &conn, file.id, "failed", 0, 0, Some(&e),
                            );
                            ocr_failed += 1;
                        }
                    }
                }
                _ => {
                    // Step 2: 使用 Rust 原生 PDF OCR
                    info!("使用 OCR 处理: {}", file.title);

                    let app_clone = app.clone();
                    let file_title = file.title.clone();

                    match super::pdf_ocr::ocr_pdf(
                        &file.file_path,
                        50,
                        Some(&|current_page, total_pages| {
                            if let Err(e) = app_clone.emit("regulation:ocr-progress", serde_json::json!({
                                "current": file_title,
                                "current_page": current_page,
                                "total_pages": total_pages,
                            })) {
                                tracing::debug!("发送 OCR 页面进度事件失败: {}", e);
                            }
                        }),
                    ) {
                        Ok(ocr_result) if ocr_result.success && !ocr_result.text.is_empty() => {
                            if ocr_result.page_count > 0 {
                                let _ = regulation_db::update_page_count(
                                    &conn, file.id, ocr_result.page_count as i32,
                                );
                            }
                            match write_to_index(&index_state, &conn, file, ocr_result.text) {
                                Ok(()) => {
                                    ocr_success += 1;
                                    info!(
                                        "OCR 成功: {} ({}页, OCR {}页, {:.2}s)",
                                        file.title, ocr_result.page_count,
                                        ocr_result.ocr_pages, ocr_result.elapsed
                                    );
                                }
                                Err(e) => {
                                    let _ = regulation_db::update_ocr_status(
                                        &conn, file.id, "failed", 0, 0, Some(&e),
                                    );
                                    ocr_failed += 1;
                                }
                            }
                        }
                        Ok(ocr_result) => {
                            let error_msg = if ocr_result.error.is_empty() {
                                "OCR 未能提取到文本".to_string()
                            } else {
                                ocr_result.error
                            };
                            let _ = regulation_db::update_ocr_status(
                                &conn, file.id, "failed", 0, 0, Some(&error_msg),
                            );
                            ocr_failed += 1;
                            warn!("OCR 无文本: {} - {}", file.title, error_msg);
                        }
                        Err(e) => {
                            let error_msg = format!("OCR 失败: {}", e);
                            let _ = regulation_db::update_ocr_status(
                                &conn, file.id, "failed", 0, 0, Some(&error_msg),
                            );
                            ocr_failed += 1;
                            warn!("OCR 失败: {} - {}", file.title, e);
                        }
                    }
                }
            }
        }

        info!(
            "自动 OCR 完成: 成功 {}, 失败 {}",
            ocr_success, ocr_failed
        );
    }

    // 发送最终进度
    if let Err(e) = app.emit("regulation:scan-progress", ScanProgress {
        scanned: total_found,
        total_found,
        new_files,
        duplicates,
        indexed,
        needs_ocr,
        failed,
        current_file: None,
        phase: "done".to_string(),
        ocr_processed: if needs_ocr > 0 { Some(ocr_success + ocr_failed) } else { None },
        ocr_total: if needs_ocr > 0 { Some(needs_ocr) } else { None },
    }) {
        debug!("发送扫描完成事件失败: {}", e);
    }

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
}

/// 在线文档（前端传入）
#[derive(Debug, Deserialize)]
pub struct OnlineRegulation {
    pub title: String,
    pub url: String,
    pub validity: String,
    pub doc_number: String,
    pub doc_type: String,
    pub publish_date: Option<String>,
    pub office_unit: Option<String>,
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
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取应用数据目录失败: {}", e))?;
    let db_path = app_data_dir.join("history.db");
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("打开数据库失败: {}", e))?;

    regulation_db::init_regulation_schema(&conn)
        .map_err(|e| format!("初始化规章表失败: {}", e))?;

    // 加载本地已有的 URL 和有效性信息
    let local_data: std::collections::HashMap<String, (String, String)> = {
        let mut stmt = conn.prepare(
            "SELECT url, title, doc_number FROM regulation_files"
        ).map_err(|e| format!("查询本地数据失败: {}", e))?;

        let rows: Vec<(String, String, String)> = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?.unwrap_or_default(),
            ))
        })
        .map_err(|e| format!("读取本地数据失败: {}", e))?
        .filter_map(|r| r.ok())
        .collect();

        rows.into_iter()
            .map(|(url, title, doc_number)| (url, (title, doc_number)))
            .collect()
    };

    info!("本地已有 {} 条记录", local_data.len());

    let online_total = online_docs.len();
    let mut matched = 0;
    let mut new_regulations = Vec::new();
    let changed_regulations = Vec::new();
    for online_doc in &online_docs {
        if local_data.contains_key(&online_doc.url) {
            matched += 1;
            // 可以进一步对比有效性变化等
            // 目前先标记为匹配
        } else {
            // 也尝试通过文号匹配
            let found_by_doc_number = if !online_doc.doc_number.is_empty() {
                local_data.values().any(|(_, dn)| dn == &online_doc.doc_number)
            } else {
                false
            };

            if found_by_doc_number {
                matched += 1;
            } else {
                new_regulations.push(RegulationDiff {
                    title: online_doc.title.clone(),
                    doc_number: online_doc.doc_number.clone(),
                    online_validity: online_doc.validity.clone(),
                    local_validity: None,
                    change_type: "new".to_string(),
                    url: online_doc.url.clone(),
                    doc_type: online_doc.doc_type.clone(),
                    publish_date: online_doc.publish_date.clone().unwrap_or_default(),
                });
            }
        }
    }

    // 计算仅本地有的数量
    let online_urls: HashSet<&str> = online_docs.iter().map(|d| d.url.as_str()).collect();
    let local_only = local_data.keys()
        .filter(|url| {
            // 忽略 file:// 开头的本地扫描文件
            !url.starts_with("file://") && !online_urls.contains(url.as_str())
        })
        .count();

    info!(
        "同步对比完成: 在线 {}, 匹配 {}, 新增 {}, 变化 {}, 仅本地 {}",
        online_total, matched, new_regulations.len(), changed_regulations.len(), local_only
    );

    Ok(SyncCompareResponse {
        online_total,
        matched,
        new_regulations,
        changed_regulations,
        local_only,
    })
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
) -> Result<OcrProcessResponse, String> {
    let batch_size = batch_size.unwrap_or(5);
    info!("开始 OCR 处理待提取文件（Rust 原生），批次大小: {}", batch_size);

    // 获取数据库路径
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取应用数据目录失败: {}", e))?;
    let db_path = app_data_dir.join("history.db");

    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("打开数据库失败: {}", e))?;

    regulation_db::init_regulation_schema(&conn)
        .map_err(|e| format!("初始化规章表失败: {}", e))?;

    // 获取需要 OCR 的文件
    let pending_files = regulation_db::get_pending_ocr_files(&conn, batch_size)
        .map_err(|e| format!("获取待 OCR 文件失败: {}", e))?;

    if pending_files.is_empty() {
        info!("没有待 OCR 的文件");
        return Ok(OcrProcessResponse {
            processed: 0,
            ocr_success: 0,
            ocr_failed: 0,
            skipped: 0,
        });
    }

    info!("找到 {} 个待 OCR 文件", pending_files.len());

    let mut ocr_success = 0;
    let mut ocr_failed = 0;
    let skipped = 0;

    for file in &pending_files {
        // 更新状态为 processing
        let _ = regulation_db::update_ocr_status(&conn, file.id, "processing", 0, 0, None);

        // Step 1: 先重试 pdfium 文本提取（有些文件第一次可能因为锁或其他原因失败）
        let pdf_path = std::path::Path::new(&file.file_path);
        let extraction = text_extractor::extract_text_from_pdf(pdf_path);

        match extraction {
            Ok(result) if !result.needs_ocr => {
                // pdfium 提取成功，直接写入索引
                let content = result.text;
                match write_to_index(&index_state, &conn, file, content) {
                    Ok(()) => {
                        ocr_success += 1;
                        info!("pdfium 文本提取重试成功并索引: {}", file.title);
                    }
                    Err(e) => {
                        let _ = regulation_db::update_ocr_status(
                            &conn, file.id, "failed", 0, 0, Some(&e),
                        );
                        ocr_failed += 1;
                    }
                }
            }
            _ => {
                // Step 2: pdfium 文本提取不足，使用 Rust 原生 PDF OCR
                info!("使用 Rust PDF OCR 处理: {}", file.title);

                let app_clone = app.clone();
                let file_title = file.title.clone();

                match super::pdf_ocr::ocr_pdf(
                    &file.file_path,
                    50,
                    Some(&|current_page, total_pages| {
                        if let Err(e) = app_clone.emit("regulation:ocr-progress", serde_json::json!({
                            "current": file_title,
                            "current_page": current_page,
                            "total_pages": total_pages,
                        })) {
                            tracing::debug!("发送 OCR 页面进度事件失败: {}", e);
                        }
                    }),
                ) {
                    Ok(ocr_result) if ocr_result.success && !ocr_result.text.is_empty() => {
                        // OCR 成功，更新页数
                        if ocr_result.page_count > 0 {
                            let _ = regulation_db::update_page_count(
                                &conn, file.id, ocr_result.page_count as i32,
                            );
                        }

                        // 写入索引
                        match write_to_index(&index_state, &conn, file, ocr_result.text) {
                            Ok(()) => {
                                ocr_success += 1;
                                info!(
                                    "Rust OCR 成功并索引: {} ({}页, OCR {}页, {:.2}s)",
                                    file.title, ocr_result.page_count,
                                    ocr_result.ocr_pages, ocr_result.elapsed
                                );
                            }
                            Err(e) => {
                                let _ = regulation_db::update_ocr_status(
                                    &conn, file.id, "failed", 0, 0, Some(&e),
                                );
                                ocr_failed += 1;
                            }
                        }
                    }
                    Ok(ocr_result) => {
                        // OCR 完成但没有文本
                        let error_msg = if ocr_result.error.is_empty() {
                            "OCR 未能提取到文本".to_string()
                        } else {
                            ocr_result.error
                        };
                        let _ = regulation_db::update_ocr_status(
                            &conn, file.id, "failed", 0, 0, Some(&error_msg),
                        );
                        ocr_failed += 1;
                        warn!("OCR 无文本: {} - {}", file.title, error_msg);
                    }
                    Err(e) => {
                        let error_msg = format!("Rust OCR 失败: {}", e);
                        let _ = regulation_db::update_ocr_status(
                            &conn, file.id, "failed", 0, 0, Some(&error_msg),
                        );
                        ocr_failed += 1;
                        warn!("OCR 失败: {} - {}", file.title, e);
                    }
                }
            }
        }

        // 发送进度事件
        if let Err(e) = app.emit("regulation:ocr-progress", serde_json::json!({
            "current": file.title,
            "ocr_success": ocr_success,
            "ocr_failed": ocr_failed,
            "skipped": skipped,
        })) {
            debug!("发送 OCR 进度事件失败: {}", e);
        }
    }

    let processed = pending_files.len();
    info!(
        "OCR 处理完成: 处理 {}, 成功 {}, 失败 {}, 跳过 {}",
        processed, ocr_success, ocr_failed, skipped
    );

    Ok(OcrProcessResponse {
        processed,
        ocr_success,
        ocr_failed,
        skipped,
    })
}

/// 重试失败的 OCR 文件
///
/// 将所有 failed 状态的文件重置为 pending，然后执行 OCR 处理。
#[tauri::command]
pub async fn regulation_retry_failed_ocr<R: tauri::Runtime>(
    app: AppHandle<R>,
    index_state: State<'_, RegulationIndexState>,
) -> Result<OcrProcessResponse, String> {
    info!("开始重试失败的 OCR 文件");

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取应用数据目录失败: {}", e))?;
    let db_path = app_data_dir.join("history.db");
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("打开数据库失败: {}", e))?;

    regulation_db::init_regulation_schema(&conn)
        .map_err(|e| format!("初始化规章表失败: {}", e))?;

    // 重置 failed → pending
    let reset_count = regulation_db::reset_failed_ocr_files(&conn)
        .map_err(|e| format!("重置失败文件状态失败: {}", e))?;

    if reset_count == 0 {
        info!("没有失败的 OCR 文件需要重试");
        return Ok(OcrProcessResponse {
            processed: 0,
            ocr_success: 0,
            ocr_failed: 0,
            skipped: 0,
        });
    }

    info!("已重置 {} 个失败文件，开始重新 OCR", reset_count);

    // 调用已有的 OCR 处理逻辑
    regulation_ocr_pending(app, Some(reset_count), index_state).await
}

/// 辅助函数：将文本写入 Tantivy 索引并更新数据库状态
fn write_to_index(
    index_state: &State<'_, RegulationIndexState>,
    conn: &rusqlite::Connection,
    file: &regulation_db::RegulationFile,
    content: String,
) -> Result<(), String> {
    let doc = super::schema::RegulationDocument {
        title: file.title.clone(),
        doc_number: file.doc_number.clone(),
        validity: String::new(),
        doc_type: file.doc_type.clone(),
        office_unit: String::new(),
        sign_date: String::new(),
        publish_date: String::new(),
        url: file.url.clone(),
        file_path: file.file_path.clone(),
        content,
    };

    let state_guard = index_state.index.lock()
        .map_err(|e| format!("锁定索引状态失败: {}", e))?;

    if let Some(index) = state_guard.as_ref() {
        if !index.exists(&doc.url) {
            index.add_document(&doc)
                .and_then(|_| index.commit())
                .map_err(|e| format!("写入索引失败: {}", e))?;
        }
    } else {
        return Err("索引未初始化".to_string());
    }

    let _ = regulation_db::update_ocr_status(conn, file.id, "done", 100, 0, None);
    let _ = regulation_db::mark_indexed(conn, file.id);
    Ok(())
}

/// 通过 Python sidecar OCR 处理单个 PDF 文件
///
/// 前端调用此命令时，先调用 sidecar 的 ocr_pdf 方法获取文本，
/// 然后将文本传入此命令写入索引。
#[tauri::command]
pub async fn regulation_ocr_update<R: tauri::Runtime>(
    app: AppHandle<R>,
    file_id: i64,
    ocr_text: String,
    page_count: i32,
    index_state: State<'_, RegulationIndexState>,
) -> Result<bool, String> {
    info!("更新 OCR 结果: file_id={}, 文本长度={}", file_id, ocr_text.len());

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取应用数据目录失败: {}", e))?;
    let db_path = app_data_dir.join("history.db");
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("打开数据库失败: {}", e))?;

    regulation_db::init_regulation_schema(&conn)
        .map_err(|e| format!("初始化规章表失败: {}", e))?;

    // 获取文件信息
    let file = {
        let mut stmt = conn.prepare(
            "SELECT title, doc_number, doc_type, url, file_path FROM regulation_files WHERE id = ?1"
        ).map_err(|e| format!("查询文件失败: {}", e))?;

        let result = stmt.query_row(rusqlite::params![file_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        });

        match result {
            Ok(f) => f,
            Err(_) => return Err(format!("文件不存在: id={}", file_id)),
        }
    };

    let (title, doc_number, doc_type, url, file_path) = file;

    // 更新页数
    if page_count > 0 {
        let _ = regulation_db::update_page_count(&conn, file_id, page_count);
    }

    if ocr_text.is_empty() {
        let _ = regulation_db::update_ocr_status(
            &conn, file_id, "failed", 0, 0, Some("OCR 未能提取到文本"),
        );
        return Ok(false);
    }

    // 构建文档
    let doc = super::schema::RegulationDocument {
        title,
        doc_number,
        validity: String::new(),
        doc_type,
        office_unit: String::new(),
        sign_date: String::new(),
        publish_date: String::new(),
        url,
        file_path,
        content: ocr_text,
    };

    // 写入 Tantivy 索引
    let state_guard = index_state.index.lock()
        .map_err(|e| format!("锁定索引状态失败: {}", e))?;

    if let Some(index) = state_guard.as_ref() {
        if !index.exists(&doc.url) {
            index.add_document(&doc)
                .and_then(|_| index.commit())
                .map_err(|e| format!("写入索引失败: {}", e))?;
        }
    } else {
        return Err("索引未初始化".to_string());
    }

    // 更新数据库状态
    let _ = regulation_db::update_ocr_status(&conn, file_id, "done", 100, 0, None);
    let _ = regulation_db::mark_indexed(&conn, file_id);

    info!("OCR 结果已写入索引: file_id={}", file_id);
    Ok(true)
}

/// 获取需要 OCR 引擎处理的文件列表
#[tauri::command]
pub async fn regulation_get_ocr_queue<R: tauri::Runtime>(
    app: AppHandle<R>,
    limit: Option<usize>,
) -> Result<Vec<regulation_db::RegulationFile>, String> {
    let limit = limit.unwrap_or(20);

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取应用数据目录失败: {}", e))?;
    let db_path = app_data_dir.join("history.db");
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("打开数据库失败: {}", e))?;

    regulation_db::init_regulation_schema(&conn)
        .map_err(|e| format!("初始化规章表失败: {}", e))?;

    // 获取需要 OCR 的文件（pending 或 needs_ocr_engine 状态）
    let mut stmt = conn.prepare(
        r#"
        SELECT id, title, doc_number, doc_type, url, pdf_url, sha256, file_path,
               file_size, page_count, ocr_status, ocr_progress, ocr_current_page,
               ocr_error, indexed, indexed_at, created_at, updated_at
        FROM regulation_files
        WHERE ocr_status IN ('pending', 'needs_ocr_engine')
        ORDER BY created_at ASC
        LIMIT ?1
        "#,
    ).map_err(|e| format!("查询失败: {}", e))?;

    let files: Vec<regulation_db::RegulationFile> = stmt
        .query_map(rusqlite::params![limit as i64], |row| {
            Ok(regulation_db::RegulationFile {
                id: row.get(0)?,
                title: row.get(1)?,
                doc_number: row.get::<_, Option<String>>(2)?.unwrap_or_default(),
                doc_type: row.get(3)?,
                url: row.get(4)?,
                pdf_url: row.get(5)?,
                sha256: row.get(6)?,
                file_path: row.get(7)?,
                file_size: row.get(8)?,
                page_count: row.get(9)?,
                ocr_status: row.get(10)?,
                ocr_progress: row.get(11)?,
                ocr_current_page: row.get(12)?,
                ocr_error: row.get(13)?,
                indexed: row.get::<_, i32>(14)? != 0,
                indexed_at: row.get(15)?,
                created_at: row.get(16)?,
                updated_at: row.get(17)?,
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
    let state_guard = index_state.index.lock()
        .map_err(|e| format!("锁定索引状态失败: {}", e))?;

    if let Some(index) = state_guard.as_ref() {
        for (file_id, doc) in docs {
            if !index.exists(&doc.url) {
                if let Err(e) = index.add_document(doc) {
                    warn!("添加文档到索引失败: {} - {}", doc.title, e);
                    continue;
                }
            }
            // 标记已索引
            let _ = regulation_db::update_ocr_status(conn, *file_id, "done", 100, 0, None);
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

use super::online_search::{CaacOnlineSearcher, OnlineDocument, OnlineSearchRequest, OnlineSearchResponse};

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

    let searcher = CaacOnlineSearcher::new()
        .map_err(|e| format!("创建搜索器失败: {}", e))?;

    searcher.search(&request).await
        .map_err(|e| format!("在线搜索失败: {}", e))
}

/// 全量在线爬取（Rust 原生分页）
#[tauri::command]
pub async fn regulation_fetch_all_online(
    doc_type: Option<String>,
    max_pages: Option<usize>,
) -> Result<OnlineSearchResponse, String> {
    let doc_type = doc_type.unwrap_or_else(|| "all".to_string());
    let max_pages = max_pages.unwrap_or(20);

    let searcher = CaacOnlineSearcher::new()
        .map_err(|e| format!("创建搜索器失败: {}", e))?;

    searcher
        .fetch_all(&doc_type, max_pages)
        .await
        .map_err(|e| format!("全量爬取失败: {}", e))
}

/// 同步对比（在线抓取 + 本地对比）
#[tauri::command]
pub async fn regulation_sync_compare_online<R: tauri::Runtime>(
    app: AppHandle<R>,
    doc_type: Option<String>,
    max_pages: Option<usize>,
) -> Result<SyncCompareResponse, String> {
    let doc_type = doc_type.unwrap_or_else(|| "all".to_string());
    let max_pages = max_pages.unwrap_or(20);

    let searcher = CaacOnlineSearcher::new()
        .map_err(|e| format!("创建搜索器失败: {}", e))?;
    let online = searcher
        .fetch_all(&doc_type, max_pages)
        .await
        .map_err(|e| format!("在线抓取失败: {}", e))?;

    let online_docs: Vec<OnlineRegulation> = online
        .documents
        .into_iter()
        .map(|doc: OnlineDocument| OnlineRegulation {
            title: doc.title,
            url: doc.url,
            validity: doc.validity,
            doc_number: doc.doc_number,
            doc_type: doc.doc_type,
            publish_date: if doc.publish_date.is_empty() {
                None
            } else {
                Some(doc.publish_date)
            },
            office_unit: if doc.office_unit.is_empty() {
                None
            } else {
                Some(doc.office_unit)
            },
        })
        .collect();

    regulation_sync_compare(app, online_docs).await
}

// ============================================================================
// 全盘自动发现规章 PDF 文件
// ============================================================================

use crate::commands::file_search_cmd::FileSearchState;

/// 民航规章文件名前缀模式
const REGULATION_PREFIXES: &[&str] = &[
    "ac-", "ccar-", "ccar ", "ib-", "ap-", "osb-", "wm-",
    "mh-", "mh/t", "mht", "mh_t", "aac-", "car-",
    "gb-", "gb/t", "gbt",
];

/// 判断文件名是否像民航规章文件
fn is_regulation_filename(name: &str) -> bool {
    let lower = name.to_lowercase();

    // 必须是 PDF
    if !lower.ends_with(".pdf") {
        return false;
    }

    // 按前缀匹配
    for prefix in REGULATION_PREFIXES {
        if lower.starts_with(prefix) || lower.contains(prefix) {
            return true;
        }
    }

    // 文号模式匹配（AC-XXX-XX-XXXX-XX 格式）
    let doc_number_patterns = [
        "ac-", "ccar-", "ib-fs-", "ap-", "osb-", "wm-fs-",
    ];
    for pattern in doc_number_patterns {
        if lower.contains(pattern) {
            return true;
        }
    }

    // 中文关键词匹配
    let cn_keywords = [
        "规章", "咨询通告", "规范性文件", "飞行标准", "适航",
        "运行规则", "飞行程序", "飞行校验", "运行合格", "航空承运人",
        "驾驶员", "机组资源", "全天候", "地面结冰", "飞行数据",
        "训练大纲", "运行最低标准", "航空器运行", "安全保卫",
        "危险品运输", "事件信息", "航线运输",
        "民用航空", "标准规范", "固定电报", "航空气象",
        "跑道表面", "飞行动态",
    ];
    for kw in cn_keywords {
        if name.contains(kw) {
            return true;
        }
    }

    false
}

/// 全盘发现规章 PDF 文件
///
/// 利用已有的文件搜索索引器（12M+ 文件），自动发现所有民航规章 PDF。
/// 按文件名模式匹配，无需手动选择目录。
#[tauri::command]
pub async fn regulation_discover_local(
    app: AppHandle,
    file_search_state: State<'_, FileSearchState>,
    local_copy_mode: Option<String>,
    target_dir: Option<String>,
    index_state: State<'_, RegulationIndexState>,
) -> Result<serde_json::Value, String> {
    let copy_mode = LocalCopyMode::from_optional(local_copy_mode.as_deref());
    let target_dir = resolve_target_dir(&app, target_dir.as_deref())?;
    info!("开始全盘发现规章 PDF 文件...");
    let start = std::time::Instant::now();

    let indexer = &file_search_state.indexer;

    // 搜索常见规章前缀 - 使用多种搜索词和更大的限制
    let search_terms = vec![
        "AC-", "CCAR-", "CCAR ", "IB-", "AP-", "OSB-", "WM-",
        "AC-91", "AC-121", "AC-61", "AC-141", "AC-396", "AC-398",
        "CCAR-121", "CCAR-91", "CCAR-61",
        "MH-", "MH/T", "MHT", "AAC-", "CAR-",
        "GB-", "GB/T", "GBT",
        "飞行程序", "运行规则", "咨询通告", "规范性文件",
        "飞行标准", "适航", "飞行校验", "运行合格",
        "驾驶员", "机组资源", "全天候", "地面结冰",
        "民用航空", "标准规范",
    ];

    let mut all_paths: Vec<(String, String)> = Vec::new(); // (name, path)
    let mut seen_paths = std::collections::HashSet::new();

    for term in &search_terms {
        // 增大限制到 2000，确保不遗漏
        let results = indexer.search(term, "fuzzy", 2000, 0);
        for hit in results.hits {
            if hit.is_directory {
                continue;
            }
            if !is_regulation_filename(&hit.name) {
                continue;
            }
            if seen_paths.insert(hit.path.clone()) {
                all_paths.push((hit.name.clone(), hit.path.clone()));
            }
        }
    }

    // 补充：直接搜索 .pdf 扩展名，捕获可能遗漏的文件
    let pdf_results = indexer.search(".pdf", "fuzzy", 5000, 0);
    for hit in pdf_results.hits {
        if hit.is_directory {
            continue;
        }
        if !is_regulation_filename(&hit.name) {
            continue;
        }
        if seen_paths.insert(hit.path.clone()) {
            all_paths.push((hit.name.clone(), hit.path.clone()));
        }
    }

    info!("全盘发现完成：找到 {} 个规章 PDF 文件", all_paths.len());

    // 获取数据库连接，准备导入
    let app_data_dir = app.path().app_data_dir()
        .map_err(|e| format!("获取应用数据目录失败: {}", e))?;
    let db_path = app_data_dir.join("history.db");
    let conn = rusqlite::Connection::open(&db_path)
        .map_err(|e| format!("打开数据库失败: {}", e))?;
    regulation_db::init_regulation_schema(&conn)
        .map_err(|e| format!("初始化规章表失败: {}", e))?;

    // 加载已有路径用于去重
    let existing_paths: std::collections::HashSet<String> = {
        let mut stmt = conn.prepare("SELECT file_path FROM regulation_files")
            .map_err(|e| format!("查询失败: {}", e))?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| format!("读取失败: {}", e))?;
        let result: std::collections::HashSet<String> = rows
            .filter_map(|r| r.ok())
            .collect();
        result
    };

    let mut new_count = 0;
    let mut skip_count = 0;
    let mut indexed_count = 0;

    // 获取索引引用
    let index_guard = index_state.index.lock()
        .map_err(|e| format!("锁定索引失败: {}", e))?;

    for (name, path) in &all_paths {
        // register_only 模式才按路径判重
        if copy_mode == LocalCopyMode::RegisterOnly && existing_paths.contains(path) {
            skip_count += 1;
            continue;
        }

        // 从文件名提取标题（去掉扩展名和常见前缀）
        let title = name
            .trim_end_matches(".pdf")
            .trim_end_matches(".PDF")
            .to_string();

        // 生成唯一 URL（用本地路径作为标识）
        let url = format!("local://{}", path);

        // 计算文件哈希
        let sha256 = match super::sync::calculate_file_hash(std::path::Path::new(path)) {
            Ok(hash) => hash,
            Err(_) => {
                continue; // 无法读取文件，跳过
            }
        };

        // 计算导入后的存储路径
        let source_path = Path::new(path);
        let stored_path = match resolve_storage_path(source_path, &sha256, copy_mode, &target_dir) {
            Ok(path) => path,
            Err(e) => {
                debug!("复制文件失败: {} - {}", name, e);
                continue;
            }
        };
        let stored_path_str = stored_path.to_string_lossy().to_string();

        // 插入数据库
        let file = regulation_db::RegulationFile {
            id: 0,
            title: title.clone(),
            doc_number: String::new(),
            doc_type: "local".to_string(),
            url: url.clone(),
            pdf_url: None,
            sha256,
            file_path: stored_path_str.clone(),
            file_size: std::fs::metadata(&stored_path).map(|m| m.len() as i64).unwrap_or(0),
            page_count: 0,
            ocr_status: "pending".to_string(),
            ocr_progress: 0,
            ocr_current_page: 0,
            ocr_error: None,
            indexed: false,
            indexed_at: None,
            created_at: String::new(),
            updated_at: String::new(),
        };

        match regulation_db::insert_file(&conn, &file) {
            Ok(_) => {
                new_count += 1;

                // 同时添加到 tantivy 索引（仅文件名）
                if let Some(ref index) = *index_guard {
                    let doc = RegulationDocument {
                        title: title.clone(),
                        doc_number: String::new(),
                        validity: String::new(),
                        doc_type: "local".to_string(),
                        office_unit: String::new(),
                        sign_date: String::new(),
                        publish_date: String::new(),
                        url: url.clone(),
                        file_path: stored_path_str.clone(),
                        content: String::new(),
                    };
                    if index.add_document(&doc).is_ok() {
                        indexed_count += 1;
                    }
                }
            }
            Err(e) => {
                debug!("插入文件记录失败: {} - {}", name, e);
            }
        }
    }

    // 提交索引
    if indexed_count > 0 {
        if let Some(ref index) = *index_guard {
            let _ = index.commit();
        }
    }

    let elapsed_ms = start.elapsed().as_millis() as u64;

    info!(
        "全盘发现完成: 发现 {}, 新增 {}, 已存在 {}, 已索引 {}, 耗时 {}ms",
        all_paths.len(), new_count, skip_count, indexed_count, elapsed_ms
    );

    Ok(serde_json::json!({
        "found": all_paths.len(),
        "newAdded": new_count,
        "skipped": skip_count,
        "indexed": indexed_count,
        "elapsedMs": elapsed_ms,
        "total_found": all_paths.len(),
        "new_added": new_count,
        "elapsed_ms": elapsed_ms
    }))
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
    for entry in std::fs::read_dir(src).map_err(|e| format!("读取目录失败 {:?}: {}", src, e))? {
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
        return Err(format!(
            "旧数据库不存在: {}",
            legacy_db_path.to_string_lossy()
        ));
    }

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("获取应用数据目录失败: {}", e))?;
    std::fs::create_dir_all(&app_data_dir).map_err(|e| format!("创建应用数据目录失败: {}", e))?;

    let new_db_path = app_data_dir.join("history.db");
    let legacy_conn =
        rusqlite::Connection::open(&legacy_db_path).map_err(|e| format!("打开旧数据库失败: {}", e))?;
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
                doc_type: row.get::<_, Option<String>>(2)?.unwrap_or_else(|| "regulation".to_string()),
                url: row.get(3)?,
                pdf_url: row.get(4)?,
                sha256: row.get(5)?,
                file_path: row.get(6)?,
                file_size: row.get::<_, Option<i64>>(7)?.unwrap_or(0),
                page_count: row.get::<_, Option<i32>>(8)?.unwrap_or(0),
                ocr_status: row.get::<_, Option<String>>(9)?.unwrap_or_else(|| "pending".to_string()),
                ocr_progress: row.get::<_, Option<i32>>(10)?.unwrap_or(0),
                ocr_current_page: row.get::<_, Option<i32>>(11)?.unwrap_or(0),
                ocr_error: row.get(12)?,
                indexed: row.get::<_, Option<i32>>(13)?.unwrap_or(0) != 0,
                indexed_at: None,
                created_at: String::new(),
                updated_at: String::new(),
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
