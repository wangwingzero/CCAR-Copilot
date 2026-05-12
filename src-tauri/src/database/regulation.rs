//! 规章文件数据库
//!
//! 存储已下载的规章文件元数据，包括：
//! - 文件路径和 SHA256 哈希
//! - OCR 状态和进度
//! - 索引状态
//!
//! 与 Tantivy 索引配合使用，实现规章全文搜索。

use rusqlite::params;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::error::{HuGeError, HuGeResult};

/// 规章文件记录
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegulationFile {
    /// 记录 ID
    pub id: i64,
    /// 规章标题
    pub title: String,
    /// 文号
    pub doc_number: String,
    /// 文档类型：regulation（规章）、normative（规范性文件）
    pub doc_type: String,
    /// 有效性：有效、失效、废止
    pub validity: String,
    /// 发布单位
    pub office_unit: String,
    /// 签发日期
    pub sign_date: String,
    /// 发布日期
    pub publish_date: String,
    /// 原始 URL（去重键）
    pub url: String,
    /// PDF 下载 URL
    pub pdf_url: Option<String>,
    /// 文件 SHA256 哈希
    pub sha256: String,
    /// 本地文件路径
    pub file_path: String,
    /// 文件大小（字节）
    pub file_size: i64,
    /// PDF 页数
    pub page_count: i32,
    /// OCR 状态：pending/processing/done/failed
    pub ocr_status: String,
    /// OCR 进度（0-100）
    pub ocr_progress: i32,
    /// OCR 处理的当前页
    pub ocr_current_page: i32,
    /// OCR 错误信息
    pub ocr_error: Option<String>,
    /// OCR 引擎标识：pdfium / pp_ocrv4 / mineru / unknown
    pub ocr_engine: String,
    /// 是否已入索引
    pub indexed: bool,
    /// 入索引时间
    pub indexed_at: Option<String>,
    /// 创建时间
    pub created_at: String,
    /// 更新时间
    pub updated_at: String,
}

impl Default for RegulationFile {
    fn default() -> Self {
        Self {
            id: 0,
            title: String::new(),
            doc_number: String::new(),
            doc_type: String::new(),
            validity: String::new(),
            office_unit: String::new(),
            sign_date: String::new(),
            publish_date: String::new(),
            url: String::new(),
            pdf_url: None,
            sha256: String::new(),
            file_path: String::new(),
            file_size: 0,
            page_count: 0,
            ocr_status: "pending".to_string(),
            ocr_progress: 0,
            ocr_current_page: 0,
            ocr_error: None,
            ocr_engine: "unknown".to_string(),
            indexed: false,
            indexed_at: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }
}

/// 同步状态统计
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SyncStatus {
    /// 总文件数
    pub total_files: u32,
    /// 待 OCR 数量
    pub pending_ocr: u32,
    /// OCR 处理中数量
    pub processing_ocr: u32,
    /// OCR 完成数量
    pub done_ocr: u32,
    /// OCR 失败数量
    pub failed_ocr: u32,
    /// 已索引数量
    pub indexed: u32,
}

/// OCR 进度信息
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OcrProgress {
    /// 是否正在处理
    pub is_processing: bool,
    /// 当前处理的文件 ID
    pub current_file_id: Option<i64>,
    /// 当前文件标题
    pub current_file_title: Option<String>,
    /// 当前页码
    pub current_page: i32,
    /// 总页数
    pub total_pages: i32,
    /// 队列中等待的文件数
    pub queue_size: u32,
}

/// 初始化规章文件表
///
/// 在现有数据库中创建 regulation_files 表
pub fn init_regulation_schema(conn: &rusqlite::Connection) -> HuGeResult<()> {
    info!("初始化规章文件数据库表");

    conn.execute_batch(
        r#"
        -- 规章文件元数据表
        CREATE TABLE IF NOT EXISTS regulation_files (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            title TEXT NOT NULL,
            doc_number TEXT,
            doc_type TEXT NOT NULL DEFAULT 'regulation',
            validity TEXT DEFAULT '',
            office_unit TEXT DEFAULT '',
            sign_date TEXT DEFAULT '',
            publish_date TEXT DEFAULT '',
            url TEXT UNIQUE NOT NULL,
            pdf_url TEXT,
            sha256 TEXT NOT NULL,
            file_path TEXT NOT NULL,
            file_size INTEGER DEFAULT 0,
            page_count INTEGER DEFAULT 0,
            ocr_status TEXT DEFAULT 'pending',
            ocr_progress INTEGER DEFAULT 0,
            ocr_current_page INTEGER DEFAULT 0,
            ocr_error TEXT,
            indexed INTEGER DEFAULT 0,
            indexed_at DATETIME,
            created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
            updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
        );

        -- 索引：用于快速查找
        CREATE INDEX IF NOT EXISTS idx_regulation_files_sha256
            ON regulation_files(sha256);
        CREATE INDEX IF NOT EXISTS idx_regulation_files_url
            ON regulation_files(url);
        CREATE INDEX IF NOT EXISTS idx_regulation_files_ocr_status
            ON regulation_files(ocr_status);
        CREATE INDEX IF NOT EXISTS idx_regulation_files_indexed
            ON regulation_files(indexed);
        "#,
    )?;

    // 检查并添加新列（数据库迁移）
    migrate_regulation_schema(conn)?;

    info!("规章文件数据库表初始化完成");
    Ok(())
}

/// 数据库 Schema 迁移
fn migrate_regulation_schema(conn: &rusqlite::Connection) -> HuGeResult<()> {
    // 获取现有列
    let mut stmt = conn.prepare("PRAGMA table_info(regulation_files)")?;
    let columns: Vec<String> =
        stmt.query_map([], |row| row.get::<_, String>(1))?.filter_map(|r| r.ok()).collect();

    // 添加缺失的列
    let migrations = [
        (
            "ocr_current_page",
            "ALTER TABLE regulation_files ADD COLUMN ocr_current_page INTEGER DEFAULT 0",
        ),
        ("ocr_error", "ALTER TABLE regulation_files ADD COLUMN ocr_error TEXT"),
        ("validity", "ALTER TABLE regulation_files ADD COLUMN validity TEXT DEFAULT ''"),
        ("office_unit", "ALTER TABLE regulation_files ADD COLUMN office_unit TEXT DEFAULT ''"),
        ("sign_date", "ALTER TABLE regulation_files ADD COLUMN sign_date TEXT DEFAULT ''"),
        ("publish_date", "ALTER TABLE regulation_files ADD COLUMN publish_date TEXT DEFAULT ''"),
        (
            "ocr_engine",
            "ALTER TABLE regulation_files ADD COLUMN ocr_engine TEXT DEFAULT 'unknown'",
        ),
    ];

    for (column, sql) in migrations {
        if !columns.contains(&column.to_string()) {
            info!("迁移规章数据库: 添加列 {}", column);
            if let Err(e) = conn.execute(sql, []) {
                warn!("迁移列 {} 失败（可能已存在）: {}", column, e);
            }
        }
    }

    // 回填 ocr_engine（幂等：只处理 unknown / NULL 的）
    backfill_ocr_engine(conn)?;

    Ok(())
}

/// 回填 ocr_engine 字段（幂等）
///
/// 推断规则：
/// - `page_count > 0` → `pp_ocrv4`（本地 OCR 成功时 `update_page_count` 会写入页数，`pdfium` 路径不写）
/// - `page_count = 0 AND ocr_status = 'done' AND indexed_at < '2026-04-30'` → `pdfium`
///   （MinerU 模块是 2026-04-30 才加的，此前的 done 一定是 pdfium 直接提取）
/// - 其余（pending / failed / 2026-04-30 之后的 done）保持 `unknown`
fn backfill_ocr_engine(conn: &rusqlite::Connection) -> HuGeResult<()> {
    let unknown_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM regulation_files WHERE ocr_engine = 'unknown' OR ocr_engine IS NULL",
        [],
        |row| row.get(0),
    )?;

    if unknown_count == 0 {
        return Ok(());
    }

    info!("[backfill_ocr_engine] 开始回填 {} 条 unknown 记录", unknown_count);

    // 1. page_count > 0 → pp_ocrv4（本地 OCR）
    let pp_ocrv4 = conn.execute(
        "UPDATE regulation_files SET ocr_engine = 'pp_ocrv4' \
         WHERE (ocr_engine = 'unknown' OR ocr_engine IS NULL) \
           AND page_count > 0",
        [],
    )?;

    // 2. page_count = 0 AND done AND indexed_at < 2026-04-30 → pdfium
    let pdfium = conn.execute(
        "UPDATE regulation_files SET ocr_engine = 'pdfium' \
         WHERE (ocr_engine = 'unknown' OR ocr_engine IS NULL) \
           AND page_count = 0 \
           AND ocr_status = 'done' \
           AND indexed_at IS NOT NULL \
           AND indexed_at < '2026-04-30'",
        [],
    )?;

    let remaining = unknown_count - (pp_ocrv4 as i64) - (pdfium as i64);
    info!(
        "[backfill_ocr_engine] 回填完成: pp_ocrv4={}, pdfium={}, 剩余 unknown={}",
        pp_ocrv4, pdfium, remaining
    );

    Ok(())
}

/// 检查文件是否已存在（通过 SHA256）
pub fn file_exists_by_hash(conn: &rusqlite::Connection, sha256: &str) -> HuGeResult<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM regulation_files WHERE sha256 = ?1",
        params![sha256],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// 检查 URL 是否已下载
pub fn url_exists(conn: &rusqlite::Connection, url: &str) -> HuGeResult<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM regulation_files WHERE url = ?1",
        params![url],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

/// 通过 URL 获取文件记录
pub fn get_file_by_url(
    conn: &rusqlite::Connection,
    url: &str,
) -> HuGeResult<Option<RegulationFile>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, title, doc_number, doc_type, validity, office_unit, sign_date, publish_date,
               url, pdf_url, sha256, file_path,
               file_size, page_count, ocr_status, ocr_progress, ocr_current_page,
               ocr_error, indexed, indexed_at, created_at, updated_at, ocr_engine
        FROM regulation_files
        WHERE url = ?1
        "#,
    )?;

    let result = stmt.query_row(params![url], |row| {
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
            indexed: row.get::<_, i32>(18)? != 0,
            indexed_at: row.get(19)?,
            created_at: row.get(20)?,
            updated_at: row.get(21)?,
            ocr_engine: row.get::<_, Option<String>>(22)?.unwrap_or_else(|| "unknown".to_string()),
        })
    });

    match result {
        Ok(file) => Ok(Some(file)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(HuGeError::Database(format!("查询规章文件失败: {}", e))),
    }
}

/// 插入新的规章文件记录
pub fn insert_file(conn: &rusqlite::Connection, file: &RegulationFile) -> HuGeResult<i64> {
    conn.execute(
        r#"
        INSERT INTO regulation_files
            (title, doc_number, doc_type, validity, office_unit, sign_date, publish_date,
             url, pdf_url, sha256, file_path,
             file_size, page_count, ocr_status, ocr_progress, ocr_engine)
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
        "#,
        params![
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
            file.ocr_progress,
            file.ocr_engine,
        ],
    )?;

    let id = conn.last_insert_rowid();
    debug!("插入规章文件记录: id={}, title={}", id, file.title);
    Ok(id)
}

/// 更新官网元数据（用于同步有效性、发布日期等变化）
pub fn update_official_metadata(
    conn: &rusqlite::Connection,
    url: &str,
    title: &str,
    doc_number: &str,
    doc_type: &str,
    validity: &str,
    office_unit: &str,
    sign_date: &str,
    publish_date: &str,
) -> HuGeResult<usize> {
    let changed = conn.execute(
        r#"
        UPDATE regulation_files
        SET title = ?1, doc_number = ?2, doc_type = ?3, validity = ?4,
            office_unit = ?5, sign_date = ?6, publish_date = ?7,
            updated_at = CURRENT_TIMESTAMP
        WHERE url = ?8
        "#,
        params![title, doc_number, doc_type, validity, office_unit, sign_date, publish_date, url,],
    )?;

    Ok(changed)
}

/// 更新 OCR 状态
pub fn update_ocr_status(
    conn: &rusqlite::Connection,
    file_id: i64,
    status: &str,
    progress: i32,
    current_page: i32,
    error: Option<&str>,
) -> HuGeResult<()> {
    conn.execute(
        r#"
        UPDATE regulation_files
        SET ocr_status = ?1, ocr_progress = ?2, ocr_current_page = ?3,
            ocr_error = ?4, updated_at = CURRENT_TIMESTAMP
        WHERE id = ?5
        "#,
        params![status, progress, current_page, error, file_id],
    )?;

    debug!("更新 OCR 状态: id={}, status={}, progress={}%", file_id, status, progress);
    Ok(())
}

/// 标记文件已入索引
pub fn mark_indexed(conn: &rusqlite::Connection, file_id: i64) -> HuGeResult<()> {
    conn.execute(
        r#"
        UPDATE regulation_files
        SET indexed = 1, indexed_at = CURRENT_TIMESTAMP, updated_at = CURRENT_TIMESTAMP
        WHERE id = ?1
        "#,
        params![file_id],
    )?;

    debug!("标记文件已入索引: id={}", file_id);
    Ok(())
}

/// 获取待 OCR 的文件列表
pub fn get_pending_ocr_files(
    conn: &rusqlite::Connection,
    limit: usize,
) -> HuGeResult<Vec<RegulationFile>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, title, doc_number, doc_type, validity, office_unit, sign_date, publish_date,
               url, pdf_url, sha256, file_path,
               file_size, page_count, ocr_status, ocr_progress, ocr_current_page,
               ocr_error, indexed, indexed_at, created_at, updated_at, ocr_engine
        FROM regulation_files
        WHERE ocr_status = 'pending'
        ORDER BY created_at ASC
        LIMIT ?1
        "#,
    )?;

    let files = stmt
        .query_map(params![limit as i64], |row| {
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
                indexed: row.get::<_, i32>(18)? != 0,
                indexed_at: row.get(19)?,
                created_at: row.get(20)?,
                updated_at: row.get(21)?,
                ocr_engine: row.get::<_, Option<String>>(22)?.unwrap_or_else(|| "unknown".to_string()),
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(files)
}

/// 重置失败的 OCR 文件状态为 pending（用于重试）
///
/// 返回重置的文件数量
pub fn reset_failed_ocr_files(conn: &rusqlite::Connection) -> HuGeResult<usize> {
    let count = conn.execute(
        r#"
        UPDATE regulation_files
        SET ocr_status = 'pending', ocr_error = NULL, ocr_progress = 0,
            ocr_current_page = 0, updated_at = CURRENT_TIMESTAMP
        WHERE ocr_status = 'failed'
        "#,
        [],
    )?;

    tracing::info!("重置 {} 个失败的 OCR 文件为 pending 状态", count);
    Ok(count)
}

/// 将长时间停留在 processing 的 OCR 任务重置为 pending。
///
/// 这通常发生在应用被强制关闭、系统休眠或网络 OCR 等待过程中进程退出。
pub fn reset_stale_processing_ocr_files(
    conn: &rusqlite::Connection,
    stale_minutes: i64,
) -> HuGeResult<usize> {
    let stale_window = format!("-{} minutes", stale_minutes.max(1));
    let count = conn.execute(
        r#"
        UPDATE regulation_files
        SET ocr_status = 'pending', ocr_error = NULL, ocr_progress = 0,
            ocr_current_page = 0, updated_at = CURRENT_TIMESTAMP
        WHERE ocr_status = 'processing'
          AND updated_at < datetime('now', ?1)
        "#,
        params![stale_window],
    )?;

    if count > 0 {
        tracing::info!("重置 {} 个超时 processing OCR 任务为 pending", count);
    }
    Ok(count)
}

/// 获取同步状态统计
pub fn get_sync_status(conn: &rusqlite::Connection) -> HuGeResult<SyncStatus> {
    let total: u32 =
        conn.query_row("SELECT COUNT(*) FROM regulation_files", [], |row| row.get(0))?;

    let pending: u32 = conn.query_row(
        "SELECT COUNT(*) FROM regulation_files WHERE ocr_status = 'pending'",
        [],
        |row| row.get(0),
    )?;

    let processing: u32 = conn.query_row(
        "SELECT COUNT(*) FROM regulation_files WHERE ocr_status = 'processing'",
        [],
        |row| row.get(0),
    )?;

    let done: u32 = conn.query_row(
        "SELECT COUNT(*) FROM regulation_files WHERE ocr_status = 'done'",
        [],
        |row| row.get(0),
    )?;

    let failed: u32 = conn.query_row(
        "SELECT COUNT(*) FROM regulation_files WHERE ocr_status = 'failed'",
        [],
        |row| row.get(0),
    )?;

    let indexed: u32 =
        conn.query_row("SELECT COUNT(*) FROM regulation_files WHERE indexed = 1", [], |row| {
            row.get(0)
        })?;

    Ok(SyncStatus {
        total_files: total,
        pending_ocr: pending,
        processing_ocr: processing,
        done_ocr: done,
        failed_ocr: failed,
        indexed,
    })
}

/// 获取所有已完成 OCR 但未入索引的文件
pub fn get_unindexed_files(
    conn: &rusqlite::Connection,
    limit: usize,
) -> HuGeResult<Vec<RegulationFile>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, title, doc_number, doc_type, validity, office_unit, sign_date, publish_date,
               url, pdf_url, sha256, file_path,
               file_size, page_count, ocr_status, ocr_progress, ocr_current_page,
               ocr_error, indexed, indexed_at, created_at, updated_at, ocr_engine
        FROM regulation_files
        WHERE ocr_status = 'done' AND indexed = 0
        ORDER BY created_at ASC
        LIMIT ?1
        "#,
    )?;

    let files = stmt
        .query_map(params![limit as i64], |row| {
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
                indexed: row.get::<_, i32>(18)? != 0,
                indexed_at: row.get(19)?,
                created_at: row.get(20)?,
                updated_at: row.get(21)?,
                ocr_engine: row.get::<_, Option<String>>(22)?.unwrap_or_else(|| "unknown".to_string()),
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(files)
}

/// 更新文件页数
pub fn update_page_count(
    conn: &rusqlite::Connection,
    file_id: i64,
    page_count: i32,
) -> HuGeResult<()> {
    conn.execute(
        r#"
        UPDATE regulation_files
        SET page_count = ?1, updated_at = CURRENT_TIMESTAMP
        WHERE id = ?2
        "#,
        params![page_count, file_id],
    )?;

    debug!("更新文件页数: id={}, page_count={}", file_id, page_count);
    Ok(())
}

/// 更新 OCR 引擎标识
///
/// 在 OCR 路径成功完成时调用，记录实际使用的引擎。
/// 支持的值：`pdfium` / `pp_ocrv4` / `mineru` / `unknown`。
pub fn update_ocr_engine(
    conn: &rusqlite::Connection,
    file_id: i64,
    engine: &str,
) -> HuGeResult<()> {
    conn.execute(
        r#"
        UPDATE regulation_files
        SET ocr_engine = ?1, updated_at = CURRENT_TIMESTAMP
        WHERE id = ?2
        "#,
        params![engine, file_id],
    )?;
    debug!("更新 OCR 引擎: id={}, engine={}", file_id, engine);
    Ok(())
}

/// 按 id 列表批量删除规章文件记录。
///
/// 返回实际删除的行数。
/// 调用方负责在删除数据库行之前先把对应记录从 Tantivy 索引移除。
pub fn delete_files_by_ids(
    conn: &rusqlite::Connection,
    ids: &[i64],
) -> HuGeResult<usize> {
    if ids.is_empty() {
        return Ok(0);
    }

    let placeholders: String =
        std::iter::repeat("?").take(ids.len()).collect::<Vec<_>>().join(",");
    let sql = format!("DELETE FROM regulation_files WHERE id IN ({})", placeholders);

    let params_vec: Vec<&dyn rusqlite::ToSql> =
        ids.iter().map(|id| id as &dyn rusqlite::ToSql).collect();

    let count = conn.execute(&sql, params_vec.as_slice())?;
    info!("批量删除规章文件记录: 请求 {} 条，实际删除 {} 条", ids.len(), count);
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_regulation_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn test_insert_and_query() {
        let conn = setup_test_db();

        let file = RegulationFile {
            title: "测试规章".to_string(),
            doc_number: "CCAR-121".to_string(),
            doc_type: "regulation".to_string(),
            url: "https://example.com/test.pdf".to_string(),
            sha256: "abc123".to_string(),
            file_path: "/tmp/test.pdf".to_string(),
            ..Default::default()
        };

        let id = insert_file(&conn, &file).unwrap();
        assert!(id > 0);

        let found = get_file_by_url(&conn, &file.url).unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().title, "测试规章");
    }

    #[test]
    fn test_url_exists() {
        let conn = setup_test_db();

        let file = RegulationFile {
            title: "测试".to_string(),
            url: "https://example.com/test.pdf".to_string(),
            sha256: "abc123".to_string(),
            file_path: "/tmp/test.pdf".to_string(),
            ..Default::default()
        };

        assert!(!url_exists(&conn, &file.url).unwrap());

        insert_file(&conn, &file).unwrap();

        assert!(url_exists(&conn, &file.url).unwrap());
    }

    #[test]
    fn test_sync_status() {
        let conn = setup_test_db();

        let status = get_sync_status(&conn).unwrap();
        assert_eq!(status.total_files, 0);

        let file = RegulationFile {
            title: "测试".to_string(),
            url: "https://example.com/test.pdf".to_string(),
            sha256: "abc123".to_string(),
            file_path: "/tmp/test.pdf".to_string(),
            ocr_status: "pending".to_string(),
            ..Default::default()
        };

        insert_file(&conn, &file).unwrap();

        let status = get_sync_status(&conn).unwrap();
        assert_eq!(status.total_files, 1);
        assert_eq!(status.pending_ocr, 1);
    }

    #[test]
    fn test_delete_files_by_ids_empty_input_is_noop() {
        let conn = setup_test_db();
        let count = delete_files_by_ids(&conn, &[]).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_delete_files_by_ids_removes_only_matching_records() {
        let conn = setup_test_db();

        let mut ids = Vec::new();
        for i in 0..3 {
            let file = RegulationFile {
                title: format!("file{}", i),
                url: format!("https://example.com/{}.pdf", i),
                sha256: format!("hash{}", i),
                file_path: format!("/tmp/{}.pdf", i),
                ..Default::default()
            };
            ids.push(insert_file(&conn, &file).unwrap());
        }

        // 删除前 2 条，保留第 3 条
        let to_delete = vec![ids[0], ids[1]];
        let deleted = delete_files_by_ids(&conn, &to_delete).unwrap();
        assert_eq!(deleted, 2);

        // 验证只有第 3 条还在
        let remaining = get_sync_status(&conn).unwrap();
        assert_eq!(remaining.total_files, 1);

        // 重复删除已删除的 id 返回 0（幂等）
        let deleted_again = delete_files_by_ids(&conn, &to_delete).unwrap();
        assert_eq!(deleted_again, 0);
    }

    #[test]
    fn test_delete_files_by_ids_ignores_nonexistent_ids() {
        let conn = setup_test_db();

        let file = RegulationFile {
            title: "exists".to_string(),
            url: "https://example.com/x.pdf".to_string(),
            sha256: "h".to_string(),
            file_path: "/tmp/x.pdf".to_string(),
            ..Default::default()
        };
        let real_id = insert_file(&conn, &file).unwrap();

        // 混合：1 个真实存在的 id + 2 个不存在的
        let mix = vec![real_id, 999_999, 888_888];
        let deleted = delete_files_by_ids(&conn, &mix).unwrap();
        assert_eq!(deleted, 1);
        assert_eq!(get_sync_status(&conn).unwrap().total_files, 0);
    }
}
