//! PDF 文本提取模块
//!
//! 从已下载的 PDF 文件中提取文本，用于 Tantivy 索引。
//!
//! 提取策略：
//! 1. 先用 pdfium-render 直接提取文本（适用于文本型 PDF）
//! 2. 若文本不足（< 100 字符），标记为需要 OCR

use std::path::Path;
use tracing::{debug, info, warn};

/// 最小有效文本长度（低于此值认为提取失败，需要 OCR）
const MIN_TEXT_LENGTH: usize = 100;

/// 乱码字符比例阈值（超过此值认为提取质量差，需要 OCR）
const MAX_GARBLED_RATIO: f64 = 0.1;

/// 检测文本是否包含大量乱码/替换字符
///
/// 检测 Unicode Private Use Area 字符和替换字符，
/// 这些通常是 PDF 提取失败的标志。
fn has_excessive_garbled_chars(text: &str) -> bool {
    if text.is_empty() {
        return false;
    }
    let total = text.chars().count();
    let garbled = text
        .chars()
        .filter(|c| {
            let cp = *c as u32;
            // Unicode Private Use Area + Replacement Character
            (0xE000..=0xF8FF).contains(&cp)       // BMP PUA
            || (0xF0000..=0xFFFFD).contains(&cp) // Supplementary PUA-A
            || (0x100000..=0x10FFFD).contains(&cp) // Supplementary PUA-B
            || cp == 0xFFFD // Replacement Character
        })
        .count();
    let ratio = garbled as f64 / total as f64;
    ratio > MAX_GARBLED_RATIO
}

/// 文本提取结果
#[derive(Debug)]
pub struct ExtractionResult {
    /// 提取的文本内容
    pub text: String,
    /// 是否需要 OCR（pdf-extract 文本不足）
    pub needs_ocr: bool,
}

/// 用 pdfium-render 提取 PDF 文本
fn pdfium_extract_text(pdf_path: &Path) -> Result<String, String> {
    let pdfium =
        super::pdf_ocr::create_pdfium().map_err(|e| format!("创建 pdfium 实例失败: {}", e))?;

    let document =
        pdfium.load_pdf_from_file(pdf_path, None).map_err(|e| format!("加载 PDF 失败: {}", e))?;

    let mut all_text = String::new();
    for page in document.pages().iter() {
        let page_text = page.text().map_err(|e| format!("提取页面文本失败: {}", e))?;
        all_text.push_str(&page_text.all());
        all_text.push('\n');
    }

    Ok(all_text)
}

/// 从 PDF 文件提取文本
///
/// 首先尝试 pdfium，如果文本不足则标记需要 OCR。
///
/// # 参数
/// - `pdf_path`: PDF 文件路径
///
/// # 返回
/// - `Ok(ExtractionResult)`: 提取结果
/// - `Err(String)`: 提取失败
pub fn extract_text_from_pdf(pdf_path: &Path) -> Result<ExtractionResult, String> {
    info!("提取 PDF 文本: {:?}", pdf_path);

    // 检查文件存在
    if !pdf_path.exists() {
        return Err(format!("文件不存在: {:?}", pdf_path));
    }

    // 尝试 pdfium 文本提取
    match pdfium_extract_text(pdf_path) {
        Ok(text) => {
            let text_len = text.chars().count();
            debug!("pdfium 提取 {} 字符", text_len);

            if text_len >= MIN_TEXT_LENGTH && !has_excessive_garbled_chars(&text) {
                info!("PDF 文本提取成功: {} 字符", text_len);
                Ok(ExtractionResult { text, needs_ocr: false })
            } else {
                let reason = if text_len < MIN_TEXT_LENGTH {
                    format!("文本不足 ({} < {}字符)", text_len, MIN_TEXT_LENGTH)
                } else {
                    "检测到大量乱码字符".to_string()
                };
                warn!("PDF 文本质量不足 ({})，标记需要 OCR: {:?}", reason, pdf_path);
                Ok(ExtractionResult { text, needs_ocr: true })
            }
        }
        Err(e) => {
            warn!("pdfium 文本提取失败，标记需要 OCR: {}", e);
            Ok(ExtractionResult { text: String::new(), needs_ocr: true })
        }
    }
}
