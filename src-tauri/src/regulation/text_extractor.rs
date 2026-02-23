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

/// 文本提取结果
#[derive(Debug)]
pub struct ExtractionResult {
    /// 提取的文本内容
    pub text: String,
    /// 提取方式：pdfium / ocr
    #[allow(dead_code)]
    pub method: String,
    /// 是否需要 OCR（pdf-extract 文本不足）
    pub needs_ocr: bool,
    /// 页数（如果可获取）
    #[allow(dead_code)]
    pub page_count: i32,
}

/// 从 PDF 文件提取文本
///
/// 首先尝试 pdf-extract，如果文本不足则标记需要 OCR。
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
    match crate::converter::pdf::extract_text(pdf_path) {
        Ok(text) => {
            let text_len = text.chars().count();
            debug!("pdfium 提取 {} 字符", text_len);

            if text_len >= MIN_TEXT_LENGTH {
                // 文本充足，直接使用
                info!("PDF 文本提取成功: {} 字符", text_len);
                Ok(ExtractionResult {
                    text,
                    method: "pdfium".to_string(),
                    needs_ocr: false,
                    page_count: 0,
                })
            } else {
                // 文本不足，标记需要 OCR
                warn!(
                    "PDF 文本不足 ({} < {}字符)，标记需要 OCR: {:?}",
                    text_len, MIN_TEXT_LENGTH, pdf_path
                );
                Ok(ExtractionResult {
                    text,
                    method: "pdfium".to_string(),
                    needs_ocr: true,
                    page_count: 0,
                })
            }
        }
        Err(e) => {
            // pdfium 文本提取失败，标记需要 OCR
            warn!("pdfium 文本提取失败，标记需要 OCR: {}", e);
            Ok(ExtractionResult {
                text: String::new(),
                method: "failed".to_string(),
                needs_ocr: true,
                page_count: 0,
            })
        }
    }
}
