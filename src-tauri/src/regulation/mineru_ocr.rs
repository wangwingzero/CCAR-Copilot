//! MinerU 在线 OCR 客户端
//!
//! 只负责单 PDF 的上传、轮询和 `full.md` 提取。调用方负责决定何时使用在线 OCR
//! 以及失败后的本地 OCR 回退。

use std::io::{Cursor, Read};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, info};
use zip::ZipArchive;

const MINERU_API_BASE: &str = "https://mineru.net";
pub const MINERU_MAX_FILE_BYTES: u64 = 200 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct MineruOcrOptions {
    pub api_key: String,
    pub language: String,
    pub timeout_seconds: u64,
}

impl MineruOcrOptions {
    pub fn chinese(api_key: String) -> Self {
        Self { api_key, language: "ch".to_string(), timeout_seconds: 900 }
    }
}

#[derive(Debug, Serialize)]
struct CreateBatchPayload {
    enable_formula: bool,
    enable_table: bool,
    language: String,
    files: Vec<CreateBatchFile>,
}

#[derive(Debug, Serialize)]
struct CreateBatchFile {
    name: String,
    is_ocr: bool,
    data_id: String,
}

#[derive(Debug, Deserialize)]
struct CreateBatchResponse {
    #[serde(default)]
    code: Option<i64>,
    #[serde(default)]
    msg: Option<String>,
    #[serde(default)]
    data: Option<CreateBatchData>,
}

#[derive(Debug, Deserialize)]
struct CreateBatchData {
    #[serde(default)]
    batch_id: Option<String>,
    #[serde(default)]
    file_urls: Vec<Value>,
    #[serde(default)]
    files: Vec<Value>,
}

pub async fn ocr_pdf_to_markdown(
    pdf_path: &Path,
    options: &MineruOcrOptions,
) -> Result<String, String> {
    ocr_pdf_to_markdown_with_cancel(
        pdf_path,
        options,
        Arc::new(AtomicBool::new(false)),
    )
    .await
}

pub async fn ocr_pdf_to_markdown_with_cancel(
    pdf_path: &Path,
    options: &MineruOcrOptions,
    cancel_flag: Arc<AtomicBool>,
) -> Result<String, String> {
    if cancel_flag.load(Ordering::Relaxed) {
        return Err("OCR 已中止".to_string());
    }

    let metadata =
        tokio::fs::metadata(pdf_path).await.map_err(|e| format!("读取 PDF 信息失败: {}", e))?;
    if metadata.len() > MINERU_MAX_FILE_BYTES {
        return Err(format!(
            "PDF 超过 MinerU 在线解析限制: {:.1}MB > 200MB",
            metadata.len() as f64 / 1024.0 / 1024.0
        ));
    }

    let client = Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(options.timeout_seconds.max(120)))
        .build()
        .map_err(|e| format!("创建 MinerU HTTP 客户端失败: {}", e))?;

    let (upload_url, batch_id) = request_upload_url(&client, pdf_path, options).await?;
    if cancel_flag.load(Ordering::Relaxed) {
        return Err("OCR 已中止".to_string());
    }
    upload_pdf(&client, pdf_path, &upload_url).await?;
    if cancel_flag.load(Ordering::Relaxed) {
        return Err("OCR 已中止".to_string());
    }
    let result_url = poll_result_url(&client, &batch_id, options, cancel_flag.clone()).await?;
    if cancel_flag.load(Ordering::Relaxed) {
        return Err("OCR 已中止".to_string());
    }
    download_full_markdown(&client, &result_url).await
}

async fn request_upload_url(
    client: &Client,
    pdf_path: &Path,
    options: &MineruOcrOptions,
) -> Result<(String, String), String> {
    let file_name =
        pdf_path.file_name().and_then(|name| name.to_str()).unwrap_or("document.pdf").to_string();
    let data_id =
        pdf_path.file_stem().and_then(|name| name.to_str()).unwrap_or("document").to_string();

    let payload = CreateBatchPayload {
        enable_formula: true,
        enable_table: true,
        language: options.language.clone(),
        files: vec![CreateBatchFile { name: file_name, is_ocr: true, data_id }],
    };

    let response = client
        .post(format!("{}/api/v4/file-urls/batch", MINERU_API_BASE))
        .bearer_auth(options.api_key.trim())
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("请求 MinerU 上传地址失败: {}", e))?;

    let status = response.status();
    let body = response.text().await.map_err(|e| format!("读取 MinerU 响应失败: {}", e))?;
    if !status.is_success() {
        return Err(format!("MinerU 上传地址请求失败: HTTP {} {}", status, truncate(&body)));
    }

    let parsed: CreateBatchResponse =
        serde_json::from_str(&body).map_err(|e| format!("解析 MinerU 上传响应失败: {}", e))?;
    if let Some(code) = parsed.code {
        if code != 0 && parsed.data.is_none() {
            return Err(format!(
                "MinerU 上传地址请求失败: code={} {}",
                code,
                parsed.msg.unwrap_or_default()
            ));
        }
    }

    let data = parsed.data.ok_or_else(|| format!("MinerU 响应缺少 data: {}", truncate(&body)))?;
    let batch_id = data
        .batch_id
        .filter(|id| !id.is_empty())
        .ok_or_else(|| format!("MinerU 响应缺少 batch_id: {}", truncate(&body)))?;
    let upload_url = data
        .file_urls
        .iter()
        .chain(data.files.iter())
        .find_map(extract_upload_url)
        .ok_or_else(|| format!("MinerU 响应缺少 upload_url: {}", truncate(&body)))?;

    Ok((upload_url, batch_id))
}

fn extract_upload_url(value: &Value) -> Option<String> {
    match value {
        Value::String(url) if !url.is_empty() => Some(url.clone()),
        Value::Object(map) => ["url", "presigned_url", "upload_url"]
            .iter()
            .find_map(|key| map.get(*key)?.as_str().map(|url| url.to_string()))
            .filter(|url| !url.is_empty()),
        _ => None,
    }
}

async fn upload_pdf(client: &Client, pdf_path: &Path, upload_url: &str) -> Result<(), String> {
    let bytes =
        tokio::fs::read(pdf_path).await.map_err(|e| format!("读取待上传 PDF 失败: {}", e))?;
    let response = client
        .put(upload_url)
        .body(bytes)
        .send()
        .await
        .map_err(|e| format!("上传 PDF 到 MinerU 失败: {}", e))?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("上传 PDF 到 MinerU 失败: HTTP {} {}", status, truncate(&body)));
    }
    Ok(())
}

async fn poll_result_url(
    client: &Client,
    batch_id: &str,
    options: &MineruOcrOptions,
    cancel_flag: Arc<AtomicBool>,
) -> Result<String, String> {
    let deadline = Instant::now() + Duration::from_secs(options.timeout_seconds.max(60));
    let mut wait_seconds = 2;

    while Instant::now() < deadline {
        if cancel_flag.load(Ordering::Relaxed) {
            return Err("OCR 已中止".to_string());
        }

        let response = client
            .get(format!("{}/api/v4/extract-results/batch/{}", MINERU_API_BASE, batch_id))
            .bearer_auth(options.api_key.trim())
            .send()
            .await;

        match response {
            Ok(response) => {
                let status = response.status();
                let body = response.text().await.unwrap_or_default();
                if status.is_success() {
                    let value: Value = serde_json::from_str(&body)
                        .map_err(|e| format!("解析 MinerU 解析结果失败: {}", e))?;
                    if let Some(url) = find_result_url(&value) {
                        info!("MinerU 在线 OCR 完成: batch_id={}", batch_id);
                        return Ok(url);
                    }
                    if let Some(message) = find_failure_message(&value) {
                        return Err(format!("MinerU 解析失败: {}", message));
                    }
                    debug!(
                        "MinerU 仍在处理或排队中: batch_id={}, state={}",
                        batch_id,
                        summarize_states(&value)
                    );
                } else {
                    debug!("MinerU 轮询失败: HTTP {} {}", status, truncate(&body));
                }
            }
            Err(e) => {
                debug!("MinerU 轮询请求失败: {}", e);
            }
        }

        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(wait_seconds)) => {}
            _ = async {
                while !cancel_flag.load(Ordering::Relaxed) {
                    tokio::time::sleep(Duration::from_millis(150)).await;
                }
            } => return Err("OCR 已中止".to_string()),
        }
        wait_seconds = (wait_seconds * 2).min(30);
    }

    Err(format!("MinerU 解析超时: batch_id={}", batch_id))
}

fn find_result_url(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in ["full_zip_url", "zip_url", "result_url", "download_url"] {
                if let Some(url) = map.get(key).and_then(Value::as_str) {
                    if !url.is_empty() {
                        return Some(url.to_string());
                    }
                }
            }
            map.values().find_map(find_result_url)
        }
        Value::Array(items) => items.iter().find_map(find_result_url),
        _ => None,
    }
}

fn find_failure_message(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            let status = ["status", "state", "stage", "extract_state"]
                .iter()
                .find_map(|key| map.get(*key).and_then(Value::as_str))
                .unwrap_or_default()
                .to_ascii_lowercase();
            if matches!(status.as_str(), "failed" | "fail" | "error") {
                return Some(
                    ["err_msg", "error", "message", "msg"]
                        .iter()
                        .find_map(|key| map.get(*key).and_then(Value::as_str))
                        .unwrap_or("未知错误")
                        .to_string(),
                );
            }
            map.values().find_map(find_failure_message)
        }
        Value::Array(items) => items.iter().find_map(find_failure_message),
        _ => None,
    }
}

fn summarize_states(value: &Value) -> String {
    fn collect_states(value: &Value, states: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for key in ["status", "state", "stage", "extract_state"] {
                    if let Some(state) = map.get(key).and_then(Value::as_str) {
                        if !state.is_empty() {
                            states.push(state.to_string());
                        }
                    }
                }
                for child in map.values() {
                    collect_states(child, states);
                }
            }
            Value::Array(items) => {
                for child in items {
                    collect_states(child, states);
                }
            }
            _ => {}
        }
    }

    let mut states = Vec::new();
    collect_states(value, &mut states);
    states.dedup();
    if states.is_empty() {
        "unknown".to_string()
    } else {
        states.join(",")
    }
}

async fn download_full_markdown(client: &Client, result_url: &str) -> Result<String, String> {
    let response = client
        .get(result_url)
        .send()
        .await
        .map_err(|e| format!("下载 MinerU 结果包失败: {}", e))?;
    let status = response.status();
    let bytes = response.bytes().await.map_err(|e| format!("读取 MinerU 结果包失败: {}", e))?;
    if !status.is_success() {
        let body = String::from_utf8_lossy(&bytes);
        return Err(format!("下载 MinerU 结果包失败: HTTP {} {}", status, truncate(&body)));
    }

    let cursor = Cursor::new(bytes.to_vec());
    let mut archive =
        ZipArchive::new(cursor).map_err(|e| format!("打开 MinerU 结果 ZIP 失败: {}", e))?;

    let mut full_md_index = None;
    let mut fallback_md_index = None;
    for index in 0..archive.len() {
        let entry =
            archive.by_index(index).map_err(|e| format!("读取 MinerU ZIP 条目失败: {}", e))?;
        let name = entry.name().replace('\\', "/");
        if name.ends_with("full.md") {
            full_md_index = Some(index);
            break;
        }
        if fallback_md_index.is_none() && name.ends_with(".md") {
            fallback_md_index = Some(index);
        }
    }

    let markdown_index =
        full_md_index.or(fallback_md_index).ok_or("MinerU 结果包中没有 Markdown 文件")?;
    let mut markdown_file = archive
        .by_index(markdown_index)
        .map_err(|e| format!("打开 MinerU Markdown 失败: {}", e))?;
    let mut markdown = String::new();
    markdown_file
        .read_to_string(&mut markdown)
        .map_err(|e| format!("读取 MinerU Markdown 失败: {}", e))?;
    if markdown.trim().is_empty() {
        return Err("MinerU 返回的 Markdown 为空".to_string());
    }
    Ok(markdown)
}

fn truncate(value: &str) -> String {
    value.chars().take(300).collect()
}
