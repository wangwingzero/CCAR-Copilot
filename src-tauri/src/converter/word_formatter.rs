//! Word 公文格式化模块
//!
//! 纯 Rust 实现的 GB/T 9704-2012 公文格式化功能。
//!
//! # 功能
//! - 设置页边距（上 37mm, 下 35mm, 左 28mm, 右 26mm）
//! - 设置正文字体（3号仿宋，16pt）
//! - 设置标题字体（2号小标宋，22pt）
//! - 设置行间距（28磅固定值）
//! - 设置首行缩进（2字符）
//!
//! # 限制
//! - 仅支持 .docx 文件（不支持 .doc）
//! - 无法操作打开的 Word 文档（需要 COM 自动化）
//! - 复杂格式可能丢失

use std::fs::File;
use std::path::Path;
use std::time::Instant;
use tracing::{debug, info, warn};

use super::ConverterError;

/// GB/T 9704-2012 公文格式标准常量
pub struct GBT9704;

impl GBT9704 {
    // 纸张尺寸 (A4) - twips (1 twip = 1/1440 inch)
    pub const PAGE_WIDTH_TWIPS: i32 = 11906;   // 210mm
    pub const PAGE_HEIGHT_TWIPS: i32 = 16838;  // 297mm

    // 页边距 (twips)
    pub const MARGIN_TOP_TWIPS: i32 = 2098;    // 37mm
    pub const MARGIN_BOTTOM_TWIPS: i32 = 1984; // 35mm
    pub const MARGIN_LEFT_TWIPS: i32 = 1587;   // 28mm
    pub const MARGIN_RIGHT_TWIPS: i32 = 1474;  // 26mm

    // 字体大小 (half-points, 1pt = 2 half-points)
    pub const TITLE_FONT_SIZE: usize = 44;     // 22pt (2号字)
    pub const BODY_FONT_SIZE: usize = 32;      // 16pt (3号字)

    // 行间距 (twips)
    pub const LINE_SPACING_TWIPS: i32 = 560;   // 28pt

    // 首行缩进 (twips) = 2字符 * 16pt
    pub const FIRST_LINE_INDENT_TWIPS: i32 = 640; // 32pt

    // 字体名称
    pub const TITLE_FONT: &'static str = "黑体";
    pub const BODY_FONT: &'static str = "仿宋";
}

/// 格式化结果
#[derive(Debug, Clone)]
pub struct FormatResult {
    /// 输入文件路径
    pub input_path: String,
    /// 输出文件路径
    pub output_path: String,
    /// 发现的问题列表
    pub issues: Vec<String>,
    /// 是否完全符合标准
    pub compliant: bool,
    /// 耗时（毫秒）
    pub elapsed_ms: u64,
}

/// Word 公文格式化器
pub struct WordFormatter {
    /// 是否格式化标题
    format_title: bool,
    /// 是否格式化正文
    format_body: bool,
    /// 是否设置页边距
    format_margins: bool,
}

impl Default for WordFormatter {
    fn default() -> Self {
        Self::new()
    }
}

impl WordFormatter {
    /// 创建新的格式化器
    pub fn new() -> Self {
        Self {
            format_title: true,
            format_body: true,
            format_margins: true,
        }
    }

    /// 设置是否格式化标题
    pub fn with_title_format(mut self, enable: bool) -> Self {
        self.format_title = enable;
        self
    }

    /// 设置是否格式化正文
    pub fn with_body_format(mut self, enable: bool) -> Self {
        self.format_body = enable;
        self
    }

    /// 设置是否格式化页边距
    pub fn with_margin_format(mut self, enable: bool) -> Self {
        self.format_margins = enable;
        self
    }

    /// 格式化 Word 文档
    ///
    /// # Arguments
    /// * `input_path` - 输入文件路径
    /// * `output_path` - 输出文件路径（可选，默认在原文件名后加 _formatted）
    pub fn format<P: AsRef<Path>>(
        &self,
        input_path: P,
        output_path: Option<P>,
    ) -> Result<FormatResult, ConverterError> {
        let input_path = input_path.as_ref();
        let start = Instant::now();
        let mut issues = Vec::new();

        // 验证输入文件
        if !input_path.exists() {
            return Err(ConverterError::FileNotFound(input_path.display().to_string()));
        }

        let ext = input_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        if ext != "docx" {
            return Err(ConverterError::UnsupportedFormat(
                "仅支持 .docx 格式，.doc 文件请使用 Word 另存为 .docx".to_string(),
            ));
        }

        // 计算输出路径
        let output = match output_path {
            Some(p) => p.as_ref().to_path_buf(),
            None => {
                let stem = input_path.file_stem().and_then(|s| s.to_str()).unwrap_or("document");
                let parent = input_path.parent().unwrap_or(Path::new("."));
                parent.join(format!("{}_formatted.docx", stem))
            }
        };

        info!("开始格式化文档: {:?} -> {:?}", input_path, output);

        // 读取文档
        let bytes = std::fs::read(input_path)?;
        let docx = docx_rs::read_docx(&bytes)
            .map_err(|e| ConverterError::DocxError(format!("读取文档失败: {:?}", e)))?;

        // 创建新文档并应用格式
        let formatted_docx = self.apply_format(docx, &mut issues)?;

        // 保存文档
        let file = File::create(&output)?;
        formatted_docx
            .build()
            .pack(file)
            .map_err(|e| ConverterError::DocxError(format!("保存文档失败: {:?}", e)))?;

        let elapsed = start.elapsed();
        let elapsed_ms = elapsed.as_millis() as u64;

        info!(
            "文档格式化完成: {:?}, 耗时: {}ms, 问题: {}",
            output,
            elapsed_ms,
            issues.len()
        );

        Ok(FormatResult {
            input_path: input_path.display().to_string(),
            output_path: output.display().to_string(),
            compliant: issues.is_empty(),
            issues,
            elapsed_ms,
        })
    }

    /// 应用格式到文档
    fn apply_format(
        &self,
        docx: docx_rs::Docx,
        issues: &mut Vec<String>,
    ) -> Result<docx_rs::Docx, ConverterError> {
        use docx_rs::*;

        // 创建新文档
        let mut new_docx = Docx::new();

        // 1. 设置页边距
        if self.format_margins {
            debug!("设置页边距...");
            new_docx = new_docx.page_margin(
                PageMargin::new()
                    .top(GBT9704::MARGIN_TOP_TWIPS)
                    .bottom(GBT9704::MARGIN_BOTTOM_TWIPS)
                    .left(GBT9704::MARGIN_LEFT_TWIPS)
                    .right(GBT9704::MARGIN_RIGHT_TWIPS),
            );
        }

        // 2. 处理段落
        let mut para_index = 0;
        for child in docx.document.children {
            match child {
                DocumentChild::Paragraph(para) => {
                    let text = extract_paragraph_text(&para);
                    let is_empty = text.trim().is_empty();

                    if is_empty {
                        // 保留空段落
                        new_docx = new_docx.add_paragraph(Paragraph::new());
                        continue;
                    }

                    // 判断是否为标题（第一个非空段落且较短）
                    let is_title = para_index == 0 && text.len() < 100;

                    let formatted_para = if is_title && self.format_title {
                        self.format_title_paragraph(&para, issues)
                    } else if self.format_body {
                        self.format_body_paragraph(&para, issues)
                    } else {
                        // 保持原样
                        *para
                    };

                    new_docx = new_docx.add_paragraph(formatted_para);
                    para_index += 1;
                }
                DocumentChild::Table(table) => {
                    // 保留表格
                    new_docx = new_docx.add_table(*table);
                }
                _ => {
                    // 其他元素暂不处理
                    warn!("跳过不支持的文档元素");
                }
            }
        }

        Ok(new_docx)
    }

    /// 格式化标题段落
    fn format_title_paragraph(
        &self,
        para: &docx_rs::Paragraph,
        _issues: &mut Vec<String>,
    ) -> docx_rs::Paragraph {
        use docx_rs::*;

        let text = extract_paragraph_text(para);
        debug!("格式化标题: {}...", &text[..text.len().min(30)]);

        // 创建新段落
        Paragraph::new()
            .align(AlignmentType::Center)
            .add_run(
                Run::new()
                    .add_text(&text)
                    .size(GBT9704::TITLE_FONT_SIZE)
                    .fonts(RunFonts::new().east_asia(GBT9704::TITLE_FONT)),
            )
    }

    /// 格式化正文段落
    fn format_body_paragraph(
        &self,
        para: &docx_rs::Paragraph,
        _issues: &mut Vec<String>,
    ) -> docx_rs::Paragraph {
        use docx_rs::*;

        let text = extract_paragraph_text(para);
        debug!("格式化正文: {}...", &text[..text.len().min(30)]);

        // 创建新段落
        Paragraph::new()
            .align(AlignmentType::Both)
            .indent(
                Some(0),                              // left
                None,                                 // special indent type
                Some(GBT9704::FIRST_LINE_INDENT_TWIPS), // first line
                None,                                 // right
            )
            .line_spacing(
                LineSpacing::new()
                    .line(GBT9704::LINE_SPACING_TWIPS)
                    .line_rule(LineSpacingType::Exact),
            )
            .add_run(
                Run::new()
                    .add_text(&text)
                    .size(GBT9704::BODY_FONT_SIZE)
                    .fonts(RunFonts::new().east_asia(GBT9704::BODY_FONT)),
            )
    }
}

/// 提取段落文本
fn extract_paragraph_text(para: &docx_rs::Paragraph) -> String {
    let mut text = String::new();

    for child in &para.children {
        if let docx_rs::ParagraphChild::Run(run) = child {
            for run_child in &run.children {
                match run_child {
                    docx_rs::RunChild::Text(t) => {
                        text.push_str(&t.text);
                    }
                    docx_rs::RunChild::Tab(_) => {
                        text.push('\t');
                    }
                    docx_rs::RunChild::Break(_) => {
                        text.push('\n');
                    }
                    _ => {}
                }
            }
        }
    }

    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gbt9704_constants() {
        // 验证常量值
        assert_eq!(GBT9704::TITLE_FONT_SIZE, 44); // 22pt
        assert_eq!(GBT9704::BODY_FONT_SIZE, 32);  // 16pt
        assert_eq!(GBT9704::TITLE_FONT, "黑体");
        assert_eq!(GBT9704::BODY_FONT, "仿宋");
    }

    #[test]
    fn test_word_formatter_builder() {
        let formatter = WordFormatter::new()
            .with_title_format(true)
            .with_body_format(true)
            .with_margin_format(false);

        assert!(formatter.format_title);
        assert!(formatter.format_body);
        assert!(!formatter.format_margins);
    }
}
