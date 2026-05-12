//! 规章文件同步模块
//!
//! 负责文件下载的去重、状态跟踪和增量同步。

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use tracing::info;

use crate::database::regulation::{self, RegulationFile, SyncStatus};
use crate::error::{HuGeError, HuGeResult};

/// 计算文件的 SHA256 哈希
pub fn calculate_file_hash(path: &Path) -> HuGeResult<String> {
    let file = File::open(path).map_err(|e| HuGeError::Internal(format!("打开文件失败: {}", e)))?;

    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        let bytes_read = reader
            .read(&mut buffer)
            .map_err(|e| HuGeError::Internal(format!("读取文件失败: {}", e)))?;

        if bytes_read == 0 {
            break;
        }

        hasher.update(&buffer[..bytes_read]);
    }

    let hash = hasher.finalize();
    Ok(format!("{:x}", hash))
}

/// 计算字节数据的 SHA256 哈希
pub fn calculate_bytes_hash(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let hash = hasher.finalize();
    format!("{:x}", hash)
}

/// 规章同步管理器
pub struct RegulationSync {
    /// 规章文件保存目录
    save_dir: PathBuf,
}

impl RegulationSync {
    /// 创建同步管理器
    pub fn new(save_dir: PathBuf) -> Self {
        Self { save_dir }
    }

    /// 获取保存目录
    pub fn save_dir(&self) -> &Path {
        &self.save_dir
    }

    /// 确保保存目录存在
    pub fn ensure_save_dir(&self) -> HuGeResult<()> {
        if !self.save_dir.exists() {
            std::fs::create_dir_all(&self.save_dir)
                .map_err(|e| HuGeError::Internal(format!("创建保存目录失败: {}", e)))?;
            info!("创建规章保存目录: {:?}", self.save_dir);
        }
        Ok(())
    }

    /// 生成文件保存路径
    ///
    /// 使用 SHA256 前 16 位作为文件名，避免文件名冲突
    pub fn get_file_path(&self, sha256: &str, original_name: Option<&str>) -> PathBuf {
        let ext = original_name
            .and_then(|n| Path::new(n).extension())
            .and_then(|e| e.to_str())
            .unwrap_or("pdf");

        let filename = format!("{}.{}", &sha256[..16], ext);
        self.save_dir.join(filename)
    }

    /// 检查文件是否已存在（通过 SHA256）
    pub fn file_exists_by_hash(
        &self,
        conn: &rusqlite::Connection,
        sha256: &str,
    ) -> HuGeResult<bool> {
        regulation::file_exists_by_hash(conn, sha256)
    }

    /// 检查 URL 是否已下载
    pub fn url_exists(&self, conn: &rusqlite::Connection, url: &str) -> HuGeResult<bool> {
        regulation::url_exists(conn, url)
    }

    /// 通过 URL 获取已有记录
    pub fn get_file_by_url(
        &self,
        conn: &rusqlite::Connection,
        url: &str,
    ) -> HuGeResult<Option<RegulationFile>> {
        regulation::get_file_by_url(conn, url)
    }

    /// 记录新下载的文件
    pub fn record_download(
        &self,
        conn: &rusqlite::Connection,
        file: &RegulationFile,
    ) -> HuGeResult<i64> {
        regulation::insert_file(conn, file)
    }

    /// 获取同步状态
    pub fn get_status(&self, conn: &rusqlite::Connection) -> HuGeResult<SyncStatus> {
        regulation::get_sync_status(conn)
    }

    /// 获取待 OCR 的文件列表
    pub fn get_pending_ocr(
        &self,
        conn: &rusqlite::Connection,
        limit: usize,
    ) -> HuGeResult<Vec<RegulationFile>> {
        regulation::get_pending_ocr_files(conn, limit)
    }

    /// 更新 OCR 状态
    pub fn update_ocr_status(
        &self,
        conn: &rusqlite::Connection,
        file_id: i64,
        status: &str,
        progress: i32,
        current_page: i32,
        error: Option<&str>,
    ) -> HuGeResult<()> {
        regulation::update_ocr_status(conn, file_id, status, progress, current_page, error)
    }

    /// 标记文件已入索引
    pub fn mark_indexed(&self, conn: &rusqlite::Connection, file_id: i64) -> HuGeResult<()> {
        regulation::mark_indexed(conn, file_id)
    }

    /// 检查并跳过已存在的文件
    ///
    /// 返回：(是否跳过, 原因)
    pub fn should_skip_download(
        &self,
        conn: &rusqlite::Connection,
        url: &str,
        sha256: Option<&str>,
    ) -> HuGeResult<(bool, Option<String>)> {
        // 先检查 URL 是否已下载
        if let Some(existing) = self.get_file_by_url(conn, url)? {
            // 如果提供了 sha256，检查是否一致
            if let Some(new_hash) = sha256 {
                if existing.sha256 == new_hash {
                    return Ok((true, Some("文件已存在且哈希一致".to_string())));
                } else {
                    // 文件已更新，需要重新下载
                    return Ok((false, Some("文件已更新，需重新下载".to_string())));
                }
            }
            return Ok((true, Some("URL 已下载".to_string())));
        }

        // 如果有 sha256，检查是否有相同哈希的文件
        if let Some(hash) = sha256 {
            if self.file_exists_by_hash(conn, hash)? {
                return Ok((true, Some("相同哈希的文件已存在".to_string())));
            }
        }

        Ok((false, None))
    }
}

/// 下载结果
#[derive(Debug, Clone)]
pub struct DownloadResult {
    /// 原始 URL
    pub url: String,
    /// PDF 下载 URL
    pub pdf_url: Option<String>,
    /// 文件 SHA256 哈希
    pub sha256: String,
    /// 本地文件路径
    pub file_path: PathBuf,
    /// 文件大小（字节）
    pub file_size: u64,
    /// 是否为新文件（false 表示已存在，跳过下载）
    pub is_new: bool,
    /// 跳过原因（如果 is_new 为 false）
    pub skip_reason: Option<String>,
}

/// 批量下载进度
#[derive(Debug, Clone, Serialize, Default)]
pub struct BatchProgress {
    /// 总数
    pub total: usize,
    /// 已完成
    pub completed: usize,
    /// 成功数
    pub success: usize,
    /// 跳过数（已存在）
    pub skipped: usize,
    /// 失败数
    pub failed: usize,
    /// 当前正在处理的 URL
    pub current_url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_calculate_file_hash() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");

        let mut file = File::create(&file_path).unwrap();
        file.write_all(b"hello world").unwrap();

        let hash = calculate_file_hash(&file_path).unwrap();
        // SHA256 of "hello world"
        assert_eq!(hash, "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9");
    }

    #[test]
    fn test_calculate_bytes_hash() {
        let hash = calculate_bytes_hash(b"hello world");
        assert_eq!(hash, "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9");
    }

    #[test]
    fn test_get_file_path() {
        let sync = RegulationSync::new(PathBuf::from("/tmp/regulations"));
        let hash = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";

        let path = sync.get_file_path(hash, Some("test.pdf"));
        assert_eq!(path.file_name().unwrap().to_str().unwrap(), "b94d27b9934d3e08.pdf");

        let path = sync.get_file_path(hash, None);
        assert_eq!(path.file_name().unwrap().to_str().unwrap(), "b94d27b9934d3e08.pdf");
    }
}
