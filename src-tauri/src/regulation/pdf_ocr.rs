//! PDF OCR 模块（纯 Rust 实现）
//!
//! 使用 pdfium-render 渲染 PDF 页面为图片，然后调用 Rust 原生 OCR 引擎识别文字。
//! 替代原来通过 Python sidecar 的 OCR 方案。
//!
//! ## 架构
//!
//! ```text
//! PDF 文件
//!     ↓ pdfium-render (渲染)
//! DynamicImage (每页)
//!     ↓ OcrEngine::recognize_image (OCR)
//! 文字文本
//! ```
//!
//! ## 依赖
//!
//! - `pdfium-render`: PDF 渲染（Google PDFium 的 Rust 封装）
//! - `crate::ocr::OcrEngine`: 已有的 PP-OCRv4 + OpenVINO OCR 引擎

use image::DynamicImage;
use pdfium_render::prelude::*;
use std::path::Path;
use tracing::{debug, info, warn};

/// PDF OCR 结果
#[derive(Debug, Clone, serde::Serialize)]
pub struct PdfOcrResult {
    /// 是否成功
    pub success: bool,
    /// 提取的全部文本
    pub text: String,
    /// PDF 总页数
    pub page_count: usize,
    /// 实际 OCR 处理的页数
    pub ocr_pages: usize,
    /// 耗时（秒）
    pub elapsed: f64,
    /// 错误信息
    pub error: String,
}

/// PDF OCR 错误
#[derive(Debug, thiserror::Error)]
pub enum PdfOcrError {
    #[error("PDFium 库加载失败: {0}")]
    LibraryLoadError(String),

    #[error("PDF 文件打开失败: {0}")]
    PdfOpenError(String),

    #[error("PDF 页面渲染失败: {0}")]
    RenderError(String),

    #[error("OCR 引擎错误: {0}")]
    OcrError(String),

    #[error("文件不存在: {0}")]
    FileNotFound(String),
}

/// 获取 pdfium.dll 的搜索路径列表
///
/// 按优先级返回：
/// 1. 可执行文件同级目录的 pdfium/pdfium.dll
/// 2. 可执行文件同级目录的 pdfium.dll
/// 3. 开发模式 - src-tauri/pdfium/pdfium.dll
pub(crate) fn get_pdfium_search_paths() -> Vec<std::path::PathBuf> {
    let mut paths = Vec::new();

    // 获取可执行文件目录
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            // 优先级 1: exe_dir/pdfium/pdfium.dll
            paths.push(exe_dir.join("pdfium").join("pdfium.dll"));
            // 优先级 2: exe_dir/pdfium.dll
            paths.push(exe_dir.join("pdfium.dll"));
        }
    }

    // 优先级 3: 开发模式 - src-tauri/pdfium/pdfium.dll
    let dev_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("pdfium").join("pdfium.dll");
    paths.push(dev_path);

    paths
}

/// 创建 Pdfium 实例
///
/// 每次调用时创建新的绑定。由于 DLL 已被 OS 缓存，
/// 后续绑定的开销很小。
pub(crate) fn create_pdfium() -> Result<Pdfium, PdfOcrError> {
    // 搜索 pdfium.dll
    let search_paths = get_pdfium_search_paths();

    for path in &search_paths {
        if path.exists() {
            debug!("找到 pdfium.dll: {:?}", path);
            let parent_dir = path.parent().unwrap_or(Path::new("."));

            match Pdfium::bind_to_library(
                Pdfium::pdfium_platform_library_name_at_path(
                    parent_dir.to_str().unwrap_or(".")
                )
            ) {
                Ok(bindings) => {
                    info!("PDFium 库加载成功: {:?}", path);
                    return Ok(Pdfium::new(bindings));
                }
                Err(e) => {
                    warn!("从 {:?} 加载 PDFium 失败: {:?}", path, e);
                    continue;
                }
            }
        }
    }

    // 最后尝试系统 PATH
    debug!("尝试从系统 PATH 加载 PDFium...");
    match Pdfium::bind_to_system_library() {
        Ok(bindings) => {
            info!("PDFium 从系统 PATH 加载成功");
            Ok(Pdfium::new(bindings))
        }
        Err(e) => {
            let searched = search_paths
                .iter()
                .map(|p| format!("  - {:?}", p))
                .collect::<Vec<_>>()
                .join("\n");
            Err(PdfOcrError::LibraryLoadError(format!(
                "无法加载 pdfium.dll。已搜索:\n{}\n系统 PATH 也未找到。\n错误: {:?}",
                searched, e
            )))
        }
    }
}

/// 对 PDF 文件执行 OCR
///
/// 1. 使用 pdfium 渲染每页为图片（约 200 DPI）
/// 2. 先尝试提取 PDF 原生文本
/// 3. 文本不足时调用 Rust OCR 引擎识别
///
/// # 参数
/// - `pdf_path`: PDF 文件路径
/// - `max_pages`: 最大处理页数
/// - `progress_callback`: 进度回调 (当前页, 总页数)
pub fn ocr_pdf(
    pdf_path: &str,
    max_pages: usize,
    progress_callback: Option<&dyn Fn(usize, usize)>,
) -> Result<PdfOcrResult, PdfOcrError> {
    let start = std::time::Instant::now();

    // 检查文件是否存在
    if !Path::new(pdf_path).exists() {
        return Err(PdfOcrError::FileNotFound(pdf_path.to_string()));
    }

    info!("开始 PDF OCR: {}, max_pages={}", pdf_path, max_pages);

    // 创建 PDFium 实例
    let pdfium = create_pdfium()?;

    // 打开 PDF
    let document = pdfium.load_pdf_from_file(pdf_path, None)
        .map_err(|e| PdfOcrError::PdfOpenError(format!("{}: {:?}", pdf_path, e)))?;

    let page_count = document.pages().len() as usize;
    let pages_to_process = page_count.min(max_pages);

    info!("PDF 打开成功: {} 页, 将处理 {} 页", page_count, pages_to_process);

    // 获取 OCR 引擎
    let ocr_engine = crate::ocr::OcrEngine::instance()
        .map_err(|e| PdfOcrError::OcrError(format!("OCR 引擎初始化失败: {}", e)))?;

    let mut all_texts: Vec<String> = Vec::new();
    let mut ocr_pages = 0;

    for i in 0..pages_to_process {
        if let Some(cb) = progress_callback {
            cb(i + 1, pages_to_process);
        }

        let page = document.pages().get(i as u16)
            .map_err(|e| PdfOcrError::RenderError(format!("获取第 {} 页失败: {:?}", i + 1, e)))?;

        // 1. 先尝试提取 PDF 原生文本
        let native_text = page.text()
            .map(|t| t.all())
            .unwrap_or_default();

        if native_text.len() > 50 {
            // 原生文本足够，直接使用
            debug!("第 {} 页: 原生文本 {} 字符", i + 1, native_text.len());
            all_texts.push(native_text);
            continue;
        }

        // 2. 原生文本不足，渲染为图片并 OCR
        debug!("第 {} 页: 原生文本不足 ({} 字符), 执行 OCR", i + 1, native_text.len());

        match render_page_to_image(&page) {
            Ok(dynamic_image) => {
                match ocr_engine.recognize_image(&dynamic_image) {
                    Ok(ocr_result) => {
                        let text_len = ocr_result.text.len();
                        let elapse = ocr_result.elapse;
                        if !ocr_result.text.is_empty() {
                            all_texts.push(ocr_result.text);
                            ocr_pages += 1;
                            debug!(
                                "第 {} 页 OCR 成功: {} 字符, {:.2}s",
                                i + 1, text_len, elapse
                            );
                        }
                    }
                    Err(e) => {
                        warn!("第 {} 页 OCR 失败: {}", i + 1, e);
                    }
                }
            }
            Err(e) => {
                warn!("第 {} 页渲染失败: {}", i + 1, e);
            }
        }
    }

    let full_text = all_texts.join("\n\n");
    let elapsed = start.elapsed().as_secs_f64();

    info!(
        "PDF OCR 完成: {}, 页数={}, OCR页数={}, 文本长度={}, 耗时={:.2}s",
        pdf_path, page_count, ocr_pages, full_text.len(), elapsed
    );

    Ok(PdfOcrResult {
        success: true,
        text: full_text,
        page_count,
        ocr_pages,
        elapsed,
        error: String::new(),
    })
}

/// 将 PDF 页面渲染为 DynamicImage
///
/// 使用约 200 DPI 渲染，平衡质量和速度。
fn render_page_to_image(page: &PdfPage) -> Result<DynamicImage, PdfOcrError> {
    // 渲染配置：约 200 DPI，白色背景
    let render_config = PdfRenderConfig::new()
        .set_target_width(2000) // 约 200 DPI for A4
        .set_maximum_height(3000)
        .rotate_if_landscape(PdfPageRenderRotation::None, false);

    // 渲染页面为位图
    let bitmap = page
        .render_with_config(&render_config)
        .map_err(|e| PdfOcrError::RenderError(format!("渲染失败: {:?}", e)))?;

    // 转换为 DynamicImage
    let image = bitmap
        .as_image()
        .as_rgba8()
        .ok_or_else(|| PdfOcrError::RenderError("转换为 RGBA8 失败".to_string()))?
        .clone();

    Ok(DynamicImage::ImageRgba8(image))
}

/// 检查 PDFium 是否可用
pub fn is_pdfium_available() -> bool {
    create_pdfium().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_pdfium_search_paths() {
        let paths = get_pdfium_search_paths();
        assert!(!paths.is_empty(), "搜索路径不应为空");
        for path in &paths {
            println!("搜索路径: {:?}", path);
        }
    }
}
