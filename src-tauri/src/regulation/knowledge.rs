//! AI 知识库导出与服务器同步。
//!
//! 生成面向 OpenClaw/服务端检索的 SQLite 快照：
//! - `documents`: 文件元数据
//! - `chunks`: 分块正文
//! - `chunks_fts`: FTS5 全文索引，优先使用 trigram tokenizer 以支持中文子串检索

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use chrono::Utc;
use reqwest::multipart::{Form, Part};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Manager, Runtime, State};
use tracing::{info, warn};

use super::schema::RegulationDocument;
use super::RegulationIndex;
use super::RegulationIndexState;
use crate::database::regulation;
use crate::database::regulation::RegulationFile;

const KNOWLEDGE_SCHEMA_VERSION: u32 = 1;
const DEFAULT_CHUNK_CHARS: usize = 1800;
const DEFAULT_CHUNK_OVERLAP: usize = 160;
const DEFAULT_REMOTE_DIR: &str = "/www/wwwroot/ccar-knowledge-data";
const DEFAULT_SERVER_HOST: &str = "154.9.27.44";
const DEFAULT_SERVER_PORT: u16 = 7668;
const DEFAULT_SERVER_USER: &str = "root";
const DEFAULT_KNOWLEDGE_API_URL: &str = "https://ccar-api.hudawang.cn";

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeExportRequest {
    pub output_dir: Option<String>,
    pub chunk_chars: Option<usize>,
    pub chunk_overlap: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeExportManifest {
    pub schema_version: u32,
    pub export_version: String,
    pub exported_at: String,
    pub documents_total: usize,
    pub documents_with_content: usize,
    pub chunks_total: usize,
    pub db_filename: String,
    pub db_sha256: String,
    pub db_bytes: u64,
    pub fts_tokenizer: String,
    pub source_history_db_sha256: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeExportResponse {
    pub output_dir: String,
    pub release_dir: String,
    pub db_path: String,
    pub manifest_path: String,
    pub manifest: KnowledgeExportManifest,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeServerSyncRequest {
    pub export: Option<KnowledgeExportRequest>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub key_path: Option<String>,
    pub remote_dir: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeServerSyncResponse {
    pub export: KnowledgeExportResponse,
    pub remote_dir: String,
    pub remote_current_dir: String,
    pub host: String,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeApiSyncRequest {
    pub export: Option<KnowledgeExportRequest>,
    pub api_url: Option<String>,
    pub api_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct KnowledgeApiUploadResponse {
    ok: bool,
    version: Option<String>,
    release_dir: Option<String>,
    current_dir: Option<String>,
    chunks_total: Option<usize>,
    db_sha256: Option<String>,
    actual_chunks: Option<usize>,
    actual_sha256: Option<String>,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KnowledgeApiSyncResponse {
    pub export: KnowledgeExportResponse,
    pub api_url: String,
    pub version: String,
    pub release_dir: String,
    pub current_dir: String,
    pub chunks_total: usize,
    pub db_sha256: String,
}

#[derive(Debug, Clone)]
struct IndexedContent {
    content: String,
}

fn default_key_path() -> String {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".ssh")
        .join("154.9.27.44_id_ed25519")
        .to_string_lossy()
        .to_string()
}

fn app_data_dir<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|e| format!("获取应用数据目录失败: {}", e))
}

fn query_regulation_files(conn: &Connection) -> Result<Vec<RegulationFile>, String> {
    let mut stmt = conn
        .prepare(
            r#"
            SELECT id, title, doc_number, doc_type, validity, office_unit, sign_date, publish_date,
                   url, pdf_url, sha256, file_path,
                   file_size, page_count, ocr_status, ocr_progress, ocr_current_page,
                   ocr_error, indexed, indexed_at, created_at, updated_at,
                   COALESCE(ocr_engine, 'unknown')
            FROM regulation_files
            ORDER BY id ASC
            "#,
        )
        .map_err(|e| format!("准备查询规章文件失败: {}", e))?;

    let files = stmt
        .query_map([], |row| {
            Ok(RegulationFile {
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
                indexed: row.get::<_, i64>(18)? != 0,
                indexed_at: row.get(19)?,
                created_at: row.get(20)?,
                updated_at: row.get(21)?,
                ocr_engine: row
                    .get::<_, Option<String>>(22)?
                    .unwrap_or_else(|| "unknown".to_string()),
            })
        })
        .map_err(|e| format!("读取规章文件失败: {}", e))?
        .filter_map(|row| row.ok())
        .collect();

    Ok(files)
}

fn load_indexed_documents<R: Runtime>(
    app: &AppHandle<R>,
    index_state: &State<'_, RegulationIndexState>,
) -> Result<Vec<RegulationDocument>, String> {
    if let Ok(guard) = index_state.index.lock() {
        if let Some(index) = guard.as_ref() {
            return index.all_documents().map_err(|e| format!("导出本地全文索引失败: {}", e));
        }
    }

    let index_path = app_data_dir(app)?.join("regulation_index");
    let index = RegulationIndex::open_or_create(index_path)
        .map_err(|e| format!("打开规章全文索引失败: {}", e))?;
    index.all_documents().map_err(|e| format!("导出本地全文索引失败: {}", e))
}

fn build_content_maps(
    documents: Vec<RegulationDocument>,
) -> (HashMap<String, IndexedContent>, HashMap<String, IndexedContent>) {
    let mut by_url = HashMap::new();
    let mut by_file_path = HashMap::new();

    for doc in documents {
        if doc.content.trim().is_empty() {
            continue;
        }
        let indexed = IndexedContent { content: doc.content };
        if !doc.url.is_empty() {
            by_url.insert(doc.url, indexed.clone());
        }
        if !doc.file_path.is_empty() {
            by_file_path.insert(doc.file_path, indexed);
        }
    }

    (by_url, by_file_path)
}

fn sha256_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path)
        .map_err(|e| format!("读取文件计算 SHA256 失败 {}: {}", path.display(), e))?;
    Ok(sha256_bytes(&bytes))
}

fn chunk_text(text: &str, chunk_chars: usize, overlap_chars: usize) -> Vec<String> {
    let normalized =
        text.lines().map(str::trim).filter(|line| !line.is_empty()).collect::<Vec<_>>().join("\n");
    let chars: Vec<char> = normalized.chars().collect();
    if chars.is_empty() {
        return Vec::new();
    }

    let chunk_chars = chunk_chars.clamp(600, 6000);
    let overlap_chars = overlap_chars.min(chunk_chars / 3);
    let step = chunk_chars.saturating_sub(overlap_chars).max(1);

    let mut chunks = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let end = (start + chunk_chars).min(chars.len());
        let chunk: String = chars[start..end].iter().collect();
        if !chunk.trim().is_empty() {
            chunks.push(chunk);
        }
        if end == chars.len() {
            break;
        }
        start += step;
    }

    chunks
}

fn create_schema(conn: &Connection) -> Result<String, String> {
    conn.execute_batch(
        r#"
        PRAGMA journal_mode = DELETE;
        PRAGMA synchronous = NORMAL;
        PRAGMA foreign_keys = ON;

        CREATE TABLE metadata (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE documents (
            id INTEGER PRIMARY KEY,
            source_file_id INTEGER NOT NULL,
            title TEXT NOT NULL,
            doc_number TEXT NOT NULL,
            doc_type TEXT NOT NULL,
            validity TEXT NOT NULL,
            office_unit TEXT NOT NULL,
            sign_date TEXT NOT NULL,
            publish_date TEXT NOT NULL,
            url TEXT NOT NULL,
            pdf_url TEXT,
            sha256 TEXT NOT NULL,
            file_path TEXT NOT NULL,
            file_size INTEGER NOT NULL,
            page_count INTEGER NOT NULL,
            ocr_status TEXT NOT NULL,
            indexed INTEGER NOT NULL,
            indexed_at TEXT,
            updated_at TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            chunk_count INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE chunks (
            id INTEGER PRIMARY KEY,
            document_id INTEGER NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
            source_file_id INTEGER NOT NULL,
            chunk_index INTEGER NOT NULL,
            text TEXT NOT NULL,
            char_count INTEGER NOT NULL,
            token_estimate INTEGER NOT NULL,
            content_hash TEXT NOT NULL,
            UNIQUE(document_id, chunk_index)
        );

        CREATE INDEX idx_documents_doc_type ON documents(doc_type);
        CREATE INDEX idx_documents_doc_number ON documents(doc_number);
        CREATE INDEX idx_documents_validity ON documents(validity);
        CREATE INDEX idx_chunks_document_id ON chunks(document_id);
        "#,
    )
    .map_err(|e| format!("创建知识库表失败: {}", e))?;

    match conn.execute_batch(
        r#"
        CREATE VIRTUAL TABLE chunks_fts USING fts5(
            title,
            doc_number,
            doc_type,
            text,
            document_id UNINDEXED,
            chunk_index UNINDEXED,
            tokenize='trigram'
        );
        "#,
    ) {
        Ok(_) => Ok("trigram".to_string()),
        Err(e) => {
            warn!("SQLite FTS5 trigram 不可用，回退到 unicode61: {}", e);
            conn.execute_batch(
                r#"
                CREATE VIRTUAL TABLE chunks_fts USING fts5(
                    title,
                    doc_number,
                    doc_type,
                    text,
                    document_id UNINDEXED,
                    chunk_index UNINDEXED,
                    tokenize='unicode61'
                );
                "#,
            )
            .map_err(|e| format!("创建 FTS5 表失败: {}", e))?;
            Ok("unicode61".to_string())
        }
    }
}

fn insert_metadata(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT OR REPLACE INTO metadata (key, value) VALUES (?1, ?2)",
        params![key, value],
    )
    .map_err(|e| format!("写入知识库元数据失败: {}", e))?;
    Ok(())
}

fn build_knowledge_db(
    db_path: &Path,
    files: &[RegulationFile],
    by_url: &HashMap<String, IndexedContent>,
    by_file_path: &HashMap<String, IndexedContent>,
    chunk_chars: usize,
    chunk_overlap: usize,
    export_version: &str,
    exported_at: &str,
) -> Result<(usize, usize, String), String> {
    let mut conn = Connection::open(db_path).map_err(|e| format!("创建知识库数据库失败: {}", e))?;
    let fts_tokenizer = create_schema(&conn)?;

    insert_metadata(&conn, "schema_version", &KNOWLEDGE_SCHEMA_VERSION.to_string())?;
    insert_metadata(&conn, "export_version", export_version)?;
    insert_metadata(&conn, "exported_at", exported_at)?;
    insert_metadata(&conn, "fts_tokenizer", &fts_tokenizer)?;

    let tx = conn.transaction().map_err(|e| format!("创建知识库事务失败: {}", e))?;
    let mut documents_with_content = 0usize;
    let mut chunks_total = 0usize;

    for file in files {
        let indexed_content = by_url.get(&file.url).or_else(|| by_file_path.get(&file.file_path));
        let content = indexed_content.map(|item| item.content.as_str()).unwrap_or("");
        let content_hash =
            if content.is_empty() { String::new() } else { sha256_bytes(content.as_bytes()) };

        tx.execute(
            r#"
            INSERT INTO documents (
                source_file_id, title, doc_number, doc_type, validity, office_unit,
                sign_date, publish_date, url, pdf_url, sha256, file_path, file_size,
                page_count, ocr_status, indexed, indexed_at, updated_at, content_hash, chunk_count
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, 0)
            "#,
            params![
                file.id,
                file.title,
                file.doc_number,
                file.doc_type,
                file.validity,
                file.office_unit,
                file.sign_date,
                file.publish_date,
                file.url,
                file.pdf_url,
                file.sha256,
                file.file_path,
                file.file_size,
                file.page_count,
                file.ocr_status,
                if file.indexed { 1 } else { 0 },
                file.indexed_at,
                file.updated_at,
                content_hash,
            ],
        )
        .map_err(|e| format!("写入 documents 失败: {}", e))?;
        let document_id = tx.last_insert_rowid();

        let chunks = chunk_text(content, chunk_chars, chunk_overlap);
        if !chunks.is_empty() {
            documents_with_content += 1;
        }

        for (chunk_index, chunk) in chunks.iter().enumerate() {
            let char_count = chunk.chars().count() as i64;
            let token_estimate = ((char_count as f64) / 1.8).ceil() as i64;
            let chunk_hash = sha256_bytes(chunk.as_bytes());
            tx.execute(
                r#"
                INSERT INTO chunks (
                    document_id, source_file_id, chunk_index, text, char_count, token_estimate, content_hash
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                "#,
                params![
                    document_id,
                    file.id,
                    chunk_index as i64,
                    chunk,
                    char_count,
                    token_estimate,
                    chunk_hash,
                ],
            )
            .map_err(|e| format!("写入 chunks 失败: {}", e))?;

            let chunk_id = tx.last_insert_rowid();
            tx.execute(
                r#"
                INSERT INTO chunks_fts(rowid, title, doc_number, doc_type, text, document_id, chunk_index)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                "#,
                params![
                    chunk_id,
                    file.title,
                    file.doc_number,
                    file.doc_type,
                    chunk,
                    document_id,
                    chunk_index as i64,
                ],
            )
            .map_err(|e| format!("写入 chunks_fts 失败: {}", e))?;
        }

        let chunk_count = chunks.len() as i64;
        chunks_total += chunks.len();
        tx.execute(
            "UPDATE documents SET chunk_count = ?1 WHERE id = ?2",
            params![chunk_count, document_id],
        )
        .map_err(|e| format!("更新 chunk_count 失败: {}", e))?;
    }

    tx.commit().map_err(|e| format!("提交知识库事务失败: {}", e))?;
    conn.execute_batch("PRAGMA optimize;").map_err(|e| format!("优化知识库失败: {}", e))?;

    Ok((documents_with_content, chunks_total, fts_tokenizer))
}

fn write_manifest(path: &Path, manifest: &KnowledgeExportManifest) -> Result<(), String> {
    let json = serde_json::to_string_pretty(manifest)
        .map_err(|e| format!("序列化知识库 manifest 失败: {}", e))?;
    fs::write(path, json).map_err(|e| format!("写入知识库 manifest 失败 {}: {}", path.display(), e))
}

fn export_knowledge_snapshot(
    app_data: &Path,
    history_db: &Path,
    files: Vec<RegulationFile>,
    indexed_docs: Vec<RegulationDocument>,
    request: KnowledgeExportRequest,
) -> Result<KnowledgeExportResponse, String> {
    let (by_url, by_file_path) = build_content_maps(indexed_docs);

    let output_root =
        request.output_dir.map(PathBuf::from).unwrap_or_else(|| app_data.join("knowledge_exports"));
    fs::create_dir_all(&output_root)
        .map_err(|e| format!("创建知识库导出目录失败 {}: {}", output_root.display(), e))?;

    let export_version = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
    let exported_at = Utc::now().to_rfc3339();
    let release_dir = output_root.join(&export_version);
    fs::create_dir_all(&release_dir)
        .map_err(|e| format!("创建知识库版本目录失败 {}: {}", release_dir.display(), e))?;

    let db_path = release_dir.join("regulation_knowledge.db");
    if db_path.exists() {
        fs::remove_file(&db_path).map_err(|e| format!("删除旧知识库失败: {}", e))?;
    }

    let chunk_chars = request.chunk_chars.unwrap_or(DEFAULT_CHUNK_CHARS);
    let chunk_overlap = request.chunk_overlap.unwrap_or(DEFAULT_CHUNK_OVERLAP);
    let (documents_with_content, chunks_total, fts_tokenizer) = build_knowledge_db(
        &db_path,
        &files,
        &by_url,
        &by_file_path,
        chunk_chars,
        chunk_overlap,
        &export_version,
        &exported_at,
    )?;

    let db_sha256 = sha256_file(&db_path)?;
    let db_bytes =
        fs::metadata(&db_path).map_err(|e| format!("读取知识库文件大小失败: {}", e))?.len();
    let source_history_db_sha256 = sha256_file(history_db)?;

    let manifest = KnowledgeExportManifest {
        schema_version: KNOWLEDGE_SCHEMA_VERSION,
        export_version: export_version.clone(),
        exported_at,
        documents_total: files.len(),
        documents_with_content,
        chunks_total,
        db_filename: "regulation_knowledge.db".to_string(),
        db_sha256,
        db_bytes,
        fts_tokenizer,
        source_history_db_sha256,
    };

    let manifest_path = release_dir.join("manifest.json");
    write_manifest(&manifest_path, &manifest)?;
    write_manifest(&output_root.join("latest.json"), &manifest)?;

    info!(
        "AI 知识库导出完成: docs={}, with_content={}, chunks={}, size={} bytes",
        manifest.documents_total,
        manifest.documents_with_content,
        manifest.chunks_total,
        manifest.db_bytes
    );

    Ok(KnowledgeExportResponse {
        output_dir: output_root.to_string_lossy().to_string(),
        release_dir: release_dir.to_string_lossy().to_string(),
        db_path: db_path.to_string_lossy().to_string(),
        manifest_path: manifest_path.to_string_lossy().to_string(),
        manifest,
    })
}

pub fn export_knowledge_from_app_data(
    app_data: PathBuf,
    request: KnowledgeExportRequest,
) -> Result<KnowledgeExportResponse, String> {
    let history_db = app_data.join("history.db");
    let source_conn =
        Connection::open(&history_db).map_err(|e| format!("打开本地 history.db 失败: {}", e))?;
    regulation::init_regulation_schema(&source_conn)
        .map_err(|e| format!("初始化规章表失败: {}", e))?;

    let files = query_regulation_files(&source_conn)?;
    let index_path = app_data.join("regulation_index");
    let index = RegulationIndex::open_or_create(index_path)
        .map_err(|e| format!("打开规章全文索引失败: {}", e))?;
    let indexed_docs = index.all_documents().map_err(|e| format!("导出本地全文索引失败: {}", e))?;

    export_knowledge_snapshot(&app_data, &history_db, files, indexed_docs, request)
}

fn export_knowledge<R: Runtime>(
    app: AppHandle<R>,
    index_state: State<'_, RegulationIndexState>,
    request: KnowledgeExportRequest,
) -> Result<KnowledgeExportResponse, String> {
    let app_data = app_data_dir(&app)?;
    let history_db = app_data.join("history.db");
    let source_conn =
        Connection::open(&history_db).map_err(|e| format!("打开本地 history.db 失败: {}", e))?;
    regulation::init_regulation_schema(&source_conn)
        .map_err(|e| format!("初始化规章表失败: {}", e))?;

    let files = query_regulation_files(&source_conn)?;
    let indexed_docs = load_indexed_documents(&app, &index_state)?;
    export_knowledge_snapshot(&app_data, &history_db, files, indexed_docs, request)
}

#[tauri::command]
pub async fn regulation_knowledge_export<R: Runtime>(
    app: AppHandle<R>,
    index_state: State<'_, RegulationIndexState>,
    request: Option<KnowledgeExportRequest>,
) -> Result<KnowledgeExportResponse, String> {
    export_knowledge(app, index_state, request.unwrap_or_default())
}

fn normalize_api_url(api_url: &str) -> String {
    api_url.trim().trim_end_matches('/').to_string()
}

async fn file_part(path: &str, file_name: &str, mime: &str) -> Result<Part, String> {
    Part::file(path)
        .await
        .map_err(|e| format!("读取上传文件失败 {}: {}", path, e))?
        .file_name(file_name.to_string())
        .mime_str(mime)
        .map_err(|e| format!("设置上传文件类型失败: {}", e))
}

#[tauri::command]
pub async fn regulation_knowledge_sync_api<R: Runtime>(
    app: AppHandle<R>,
    index_state: State<'_, RegulationIndexState>,
    request: Option<KnowledgeApiSyncRequest>,
) -> Result<KnowledgeApiSyncResponse, String> {
    let request = request.unwrap_or_default();
    let api_url =
        normalize_api_url(request.api_url.as_deref().unwrap_or(DEFAULT_KNOWLEDGE_API_URL));
    let api_token = request.api_token.unwrap_or_default();
    if api_token.trim().is_empty() {
        return Err("AI 知识库 API Token 不能为空".to_string());
    }

    let export = export_knowledge(app, index_state, request.export.unwrap_or_default())?;
    let upload_url = format!("{}/api/knowledge/upload", api_url);

    let db_part =
        file_part(&export.db_path, "regulation_knowledge.db", "application/octet-stream").await?;
    let manifest_part =
        file_part(&export.manifest_path, "manifest.json", "application/json").await?;
    let form = Form::new().part("db", db_part).part("manifest", manifest_part);

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(1800))
        .build()
        .map_err(|e| format!("创建 API 客户端失败: {}", e))?;

    let response = client
        .post(&upload_url)
        .bearer_auth(api_token.trim())
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("上传 AI 知识库失败: {}", e))?;

    let status = response.status();
    let body = response.text().await.map_err(|e| format!("读取 API 响应失败: {}", e))?;
    if !status.is_success() {
        return Err(format!("AI 知识库 API 返回 {}: {}", status, body));
    }

    let upload: KnowledgeApiUploadResponse = serde_json::from_str(&body)
        .map_err(|e| format!("解析 API 响应失败: {}; body={}", e, body))?;
    if !upload.ok {
        return Err(upload.error.unwrap_or_else(|| "AI 知识库 API 同步失败".to_string()));
    }

    let chunks_total = upload.actual_chunks.or(upload.chunks_total).unwrap_or_default();
    if chunks_total != export.manifest.chunks_total {
        return Err(format!(
            "服务器知识库校验失败: 本地 chunks={}, 服务器 chunks={}",
            export.manifest.chunks_total, chunks_total
        ));
    }

    if let Some(actual_sha256) = upload.actual_sha256.as_deref().or(upload.db_sha256.as_deref()) {
        if actual_sha256 != export.manifest.db_sha256 {
            return Err(format!(
                "服务器知识库 sha256 校验失败: 本地 {}, 服务器 {}",
                export.manifest.db_sha256, actual_sha256
            ));
        }
    }

    Ok(KnowledgeApiSyncResponse {
        export,
        api_url,
        version: upload.version.unwrap_or_default(),
        release_dir: upload.release_dir.unwrap_or_default(),
        current_dir: upload.current_dir.unwrap_or_default(),
        chunks_total,
        db_sha256: upload.actual_sha256.or(upload.db_sha256).unwrap_or_default(),
    })
}

fn command_output_to_string(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).trim().to_string()
}

fn run_command(program: &str, args: &[String]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|e| format!("执行 {} 失败: {}", program, e))?;

    if output.status.success() {
        Ok(command_output_to_string(&output.stdout))
    } else {
        let stderr = command_output_to_string(&output.stderr);
        let stdout = command_output_to_string(&output.stdout);
        Err(format!("{} 退出码 {:?}: {} {}", program, output.status.code(), stderr, stdout))
    }
}

fn remote_shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn resolve_sync_request(
    request: &KnowledgeServerSyncRequest,
) -> (String, u16, String, String, String) {
    (
        request.host.clone().unwrap_or_else(|| DEFAULT_SERVER_HOST.to_string()),
        request.port.unwrap_or(DEFAULT_SERVER_PORT),
        request.user.clone().unwrap_or_else(|| DEFAULT_SERVER_USER.to_string()),
        request.key_path.clone().unwrap_or_else(default_key_path),
        request.remote_dir.clone().unwrap_or_else(|| DEFAULT_REMOTE_DIR.to_string()),
    )
}

fn remote_db_chunk_count(
    host: &str,
    port: u16,
    user: &str,
    key_path: &str,
    db_path: &str,
) -> Result<i64, String> {
    let script = format!(
        "python3 - <<'PY'\nimport sqlite3\nconn=sqlite3.connect({:?})\nprint(conn.execute('select count(*) from chunks').fetchone()[0])\nPY",
        db_path
    );
    let output = run_command(
        "ssh",
        &[
            "-i".to_string(),
            key_path.to_string(),
            "-p".to_string(),
            port.to_string(),
            "-o".to_string(),
            "BatchMode=yes".to_string(),
            "-o".to_string(),
            "StrictHostKeyChecking=accept-new".to_string(),
            format!("{}@{}", user, host),
            script,
        ],
    )?;
    output.parse::<i64>().map_err(|e| format!("解析远端知识库验证结果失败: {}", e))
}

#[tauri::command]
pub async fn regulation_knowledge_sync_server<R: Runtime>(
    app: AppHandle<R>,
    index_state: State<'_, RegulationIndexState>,
    request: Option<KnowledgeServerSyncRequest>,
) -> Result<KnowledgeServerSyncResponse, String> {
    let request = request.unwrap_or_default();
    let (host, port, user, key_path, remote_dir) = resolve_sync_request(&request);
    let export = export_knowledge(app, index_state, request.export.unwrap_or_default())?;
    let remote_release = format!("{}/releases/{}", remote_dir, export.manifest.export_version);
    let remote_incoming = format!("{}/incoming/{}", remote_dir, export.manifest.export_version);
    let remote_current = format!("{}/current", remote_dir);

    let mkdir_script = format!(
        "set -e; mkdir -p {} {}",
        remote_shell_quote(&remote_incoming),
        remote_shell_quote(&format!("{}/releases", remote_dir))
    );
    run_command(
        "ssh",
        &[
            "-i".to_string(),
            key_path.clone(),
            "-p".to_string(),
            port.to_string(),
            "-o".to_string(),
            "BatchMode=yes".to_string(),
            "-o".to_string(),
            "StrictHostKeyChecking=accept-new".to_string(),
            format!("{}@{}", user, host),
            mkdir_script,
        ],
    )?;

    run_command(
        "scp",
        &[
            "-i".to_string(),
            key_path.clone(),
            "-P".to_string(),
            port.to_string(),
            "-o".to_string(),
            "BatchMode=yes".to_string(),
            "-o".to_string(),
            "StrictHostKeyChecking=accept-new".to_string(),
            export.db_path.clone(),
            export.manifest_path.clone(),
            format!("{}@{}:{}/", user, host, remote_incoming),
        ],
    )?;

    let publish_script = format!(
        "set -e; rm -rf {release}; mkdir -p {release}; \
         mv {incoming}/regulation_knowledge.db {release}/regulation_knowledge.db; \
         mv {incoming}/manifest.json {release}/manifest.json; \
         ln -sfn {release} {current}; \
         cp {release}/manifest.json {remote_dir}/latest.json; \
         chmod 644 {release}/regulation_knowledge.db {release}/manifest.json {remote_dir}/latest.json",
        release = remote_shell_quote(&remote_release),
        incoming = remote_shell_quote(&remote_incoming),
        current = remote_shell_quote(&remote_current),
        remote_dir = remote_shell_quote(&remote_dir),
    );
    run_command(
        "ssh",
        &[
            "-i".to_string(),
            key_path.clone(),
            "-p".to_string(),
            port.to_string(),
            "-o".to_string(),
            "BatchMode=yes".to_string(),
            format!("{}@{}", user, host),
            publish_script,
        ],
    )?;

    let remote_db = format!("{}/regulation_knowledge.db", remote_current);
    let remote_chunks = remote_db_chunk_count(&host, port, &user, &key_path, &remote_db)?;
    if remote_chunks as usize != export.manifest.chunks_total {
        return Err(format!(
            "远端知识库校验失败: 本地 chunks={}, 远端 chunks={}",
            export.manifest.chunks_total, remote_chunks
        ));
    }

    Ok(KnowledgeServerSyncResponse {
        export,
        remote_dir: remote_dir.clone(),
        remote_current_dir: remote_current,
        host,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_text_overlap() {
        let text: String = (0..700).map(|index| char::from_u32(0x4e00 + index).unwrap()).collect();
        let chunks = chunk_text(&text, 600, 100);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].chars().count(), 600);
        assert_eq!(chunks[1].chars().count(), 200);
        assert_eq!(
            chunks[0].chars().skip(500).collect::<String>(),
            chunks[1].chars().take(100).collect::<String>()
        );
    }

    #[test]
    fn test_remote_shell_quote() {
        assert_eq!(remote_shell_quote("/tmp/a b"), "'/tmp/a b'");
        assert_eq!(remote_shell_quote("a'b"), "'a'\\''b'");
    }
}
