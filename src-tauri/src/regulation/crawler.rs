//! 规章批量下载模块
//!
//! 从 CAAC 官网批量下载规章 PDF 文件。
//!
//! 特性：
//! - 并发控制：限制同时下载数量
//! - 限速：请求间隔避免触发反爬
//! - 去重：SHA256 哈希去重
//! - 进度回调：实时通知下载进度

use reqwest::Client;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, Semaphore};
use tracing::{debug, info};

use super::filename::{build_pretty_filename, dedupe_filename};
use super::sync::{calculate_bytes_hash, BatchProgress, DownloadResult};
use crate::error::{HuGeError, HuGeResult};

/// 下载配置
#[derive(Debug, Clone)]
pub struct DownloadConfig {
    /// 保存目录
    pub save_dir: PathBuf,
    /// 最大并发数
    pub max_concurrent: usize,
    /// 请求间隔（毫秒）
    pub delay_ms: u64,
    /// 请求超时（秒）
    pub timeout_secs: u64,
    /// User-Agent
    pub user_agent: String,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            save_dir: PathBuf::from("regulations"),
            max_concurrent: 2,
            delay_ms: 3000, // 3 秒间隔
            timeout_secs: 60,
            user_agent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36".to_string(),
        }
    }
}

/// 规章下载器
pub struct RegulationCrawler {
    client: Client,
    config: DownloadConfig,
    semaphore: Arc<Semaphore>,
}

impl RegulationCrawler {
    /// 创建下载器
    pub fn new(config: DownloadConfig) -> HuGeResult<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .user_agent(&config.user_agent)
            .build()
            .map_err(|e| HuGeError::Internal(format!("创建 HTTP 客户端失败: {}", e)))?;

        let semaphore = Arc::new(Semaphore::new(config.max_concurrent));

        Ok(Self { client, semaphore, config })
    }

    /// 确保保存目录存在
    pub fn ensure_save_dir(&self) -> HuGeResult<()> {
        if !self.config.save_dir.exists() {
            std::fs::create_dir_all(&self.config.save_dir)
                .map_err(|e| HuGeError::Internal(format!("创建保存目录失败: {}", e)))?;
            info!("创建规章保存目录: {:?}", self.config.save_dir);
        }
        Ok(())
    }

    /// 下载单个文件
    ///
    /// # 参数
    /// - `url`: 下载 URL
    /// - `doc_number`: 规章文号（可选，用于生成可读文件名）
    /// - `title`: 规章标题（可选，用于生成可读文件名）
    /// - `original_name`: 原始文件名（仅用于推断扩展名）
    ///
    /// # 文件命名规则
    /// 优先使用 [`build_pretty_filename`] 生成 `{doc_number}_{title}.pdf`，
    /// 重名冲突由 [`dedupe_filename`] 自动追加 `__<sha前6位>` 后缀。
    /// 元数据全部缺失时回退到 `<sha前16位>.pdf`（与历史行为一致）。
    pub async fn download_file(
        &self,
        url: &str,
        doc_number: Option<&str>,
        title: Option<&str>,
        original_name: Option<&str>,
    ) -> HuGeResult<DownloadResult> {
        // 获取信号量许可
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|e| HuGeError::Internal(format!("获取下载许可失败: {}", e)))?;

        debug!("开始下载: {}", url);

        // 发送请求
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| HuGeError::Internal(format!("下载请求失败: {}", e)))?;

        if !response.status().is_success() {
            return Err(HuGeError::Internal(format!("下载失败，状态码: {}", response.status())));
        }

        // 获取文件内容
        let bytes = response
            .bytes()
            .await
            .map_err(|e| HuGeError::Internal(format!("读取响应内容失败: {}", e)))?;

        let file_size = bytes.len() as u64;

        // 计算 SHA256
        let sha256 = calculate_bytes_hash(&bytes);

        // 确定保存路径：优先用规则化文件名，旁路通道是 sha256 短缀
        let ext = original_name
            .and_then(|n| Path::new(n).extension())
            .and_then(|e| e.to_str())
            .map(str::to_lowercase)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "pdf".to_string());

        let desired_name = build_pretty_filename(doc_number, title, &sha256, &ext);

        // 确保目录存在再做 dedupe 检查
        std::fs::create_dir_all(&self.config.save_dir).map_err(|e| {
            HuGeError::Internal(format!(
                "创建保存目录失败 {:?}: {}",
                self.config.save_dir, e
            ))
        })?;

        let file_path = dedupe_filename(&self.config.save_dir, &desired_name, &sha256);

        // 如果文件已存在（dedupe 之前的同名匹配走 dedupe 逻辑，这里是再次防御），跳过写入
        if file_path.exists() {
            info!("文件已存在，跳过写入: {:?}", file_path);
            return Ok(DownloadResult {
                url: url.to_string(),
                pdf_url: Some(url.to_string()),
                sha256,
                file_path,
                file_size,
                is_new: false,
                skip_reason: Some("文件已存在".to_string()),
            });
        }

        // 写入文件（使用临时文件 + 重命名，保证原子性）。
        // 临时文件名仍用 sha256 前 16 位避免与正式文件命名冲突。
        let temp_path = self.config.save_dir.join(format!("{}.tmp", &sha256[..16]));

        {
            let mut file = std::fs::File::create(&temp_path)
                .map_err(|e| HuGeError::Internal(format!("创建临时文件失败: {}", e)))?;

            file.write_all(&bytes)
                .map_err(|e| HuGeError::Internal(format!("写入文件失败: {}", e)))?;
        }

        // 重命名为正式文件
        std::fs::rename(&temp_path, &file_path)
            .map_err(|e| HuGeError::Internal(format!("重命名文件失败: {}", e)))?;

        info!("下载完成: {} -> {:?} ({} bytes)", url, file_path, file_size);

        // 请求间隔
        if self.config.delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(self.config.delay_ms)).await;
        }

        Ok(DownloadResult {
            url: url.to_string(),
            pdf_url: Some(url.to_string()),
            sha256,
            file_path,
            file_size,
            is_new: true,
            skip_reason: None,
        })
    }

    /// 批量下载
    ///
    /// # 参数
    /// - `items`: 下载项列表，每项包含 (url, title, doc_number, doc_type)
    /// - `progress_callback`: 进度回调函数
    ///
    /// # 返回
    /// 下载结果列表
    pub async fn batch_download<F>(
        &self,
        items: Vec<DownloadItem>,
        mut progress_callback: F,
    ) -> Vec<Result<DownloadResult, String>>
    where
        F: FnMut(&BatchProgress),
    {
        self.ensure_save_dir().ok();

        let total = items.len();
        let progress = Arc::new(Mutex::new(BatchProgress { total, ..Default::default() }));

        let mut results = Vec::with_capacity(total);

        for (index, item) in items.into_iter().enumerate() {
            // 更新进度
            {
                let mut p = progress.lock().await;
                p.current_url = Some(item.url.clone());
                progress_callback(&p);
            }

            // 下载
            let result = self
                .download_file(
                    &item.url,
                    item.doc_number.as_deref(),
                    Some(item.title.as_str()),
                    item.original_name.as_deref(),
                )
                .await;

            // 更新进度
            {
                let mut p = progress.lock().await;
                p.completed = index + 1;
                match &result {
                    Ok(r) if r.is_new => p.success += 1,
                    Ok(_) => p.skipped += 1,
                    Err(_) => p.failed += 1,
                }
                p.current_url = None;
                progress_callback(&p);
            }

            results.push(result.map_err(|e| e.to_string()));
        }

        results
    }

    /// 获取配置
    pub fn config(&self) -> &DownloadConfig {
        &self.config
    }
}

/// 下载项
#[derive(Debug, Clone)]
pub struct DownloadItem {
    /// 下载 URL
    pub url: String,
    /// 规章标题
    pub title: String,
    /// 文号
    pub doc_number: Option<String>,
    /// 文档类型
    pub doc_type: String,
    /// 原始文件名（用于确定扩展名）
    pub original_name: Option<String>,
    /// 来源 URL（用于去重）
    pub source_url: String,
}

/// 从规章列表页解析 PDF 下载链接
#[allow(dead_code)]
pub fn extract_pdf_url(detail_url: &str, html: &str) -> Option<String> {
    // CAAC 官网的 PDF 链接通常在 <a href="...pdf"> 标签中
    use regex::Regex;
    use std::sync::LazyLock;

    static PDF_REGEX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"href="([^"]+\.pdf)""#).expect("PDF_REGEX pattern invalid"));

    PDF_REGEX.captures(html).and_then(|cap| {
        let pdf_path = cap.get(1).unwrap().as_str();
        // 处理相对路径
        if pdf_path.starts_with("http") {
            Some(pdf_path.to_string())
        } else if pdf_path.starts_with('/') {
            // 绝对路径
            let base_url = url::Url::parse(detail_url).ok()?;
            Some(format!("{}://{}{}", base_url.scheme(), base_url.host_str()?, pdf_path))
        } else {
            // 相对路径
            let base_url = url::Url::parse(detail_url).ok()?;
            base_url.join(pdf_path).ok().map(|u| u.to_string())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_download_config_default() {
        let config = DownloadConfig::default();
        assert_eq!(config.max_concurrent, 2);
        assert_eq!(config.delay_ms, 3000);
    }

    #[test]
    fn test_extract_pdf_url() {
        let html = r#"<a href="/P020230101123456.pdf">下载</a>"#;
        let url = extract_pdf_url("https://www.caac.gov.cn/test/", html);
        assert_eq!(url, Some("https://www.caac.gov.cn/P020230101123456.pdf".to_string()));
    }
}
