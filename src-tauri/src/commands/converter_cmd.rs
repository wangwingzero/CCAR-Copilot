//! 文件转换 Tauri 命令
//!
//! 提供纯 Rust 实现的文件转 Markdown 功能

use serde::{Deserialize, Serialize};
use tracing::{error, info};

use crate::converter::{ConversionResult, ConverterError, FileConverter, FileFormat};

/// 转换结果（前端友好格式）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionResponse {
    /// 是否成功
    pub success: bool,
    /// Markdown 内容
    pub markdown: String,
    /// 文档标题
    pub title: Option<String>,
    /// 原始文件路径
    pub source_path: String,
    /// 转换耗时（毫秒）
    pub elapsed_ms: u64,
    /// 原始文件大小（字节）
    pub file_size: u64,
    /// 错误信息（如果失败）
    pub error: Option<String>,
}

impl From<ConversionResult> for ConversionResponse {
    fn from(result: ConversionResult) -> Self {
        Self {
            success: true,
            markdown: result.markdown,
            title: result.title,
            source_path: result.source_path,
            elapsed_ms: result.elapsed_ms,
            file_size: result.file_size,
            error: None,
        }
    }
}

impl From<ConverterError> for ConversionResponse {
    fn from(err: ConverterError) -> Self {
        Self {
            success: false,
            markdown: String::new(),
            title: None,
            source_path: String::new(),
            elapsed_ms: 0,
            file_size: 0,
            error: Some(err.to_string()),
        }
    }
}

/// 文件格式信息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileFormatInfo {
    /// 格式名称
    pub format: String,
    /// 是否支持
    pub supported: bool,
    /// 文件扩展名
    pub extension: String,
}

/// 将文件转换为 Markdown
///
/// # Arguments
/// * `file_path` - 文件路径
///
/// # Returns
/// 转换结果，包含 Markdown 内容和元数据
#[tauri::command]
pub async fn convert_file_to_markdown(file_path: String) -> ConversionResponse {
    info!("收到文件转换请求: {}", file_path);

    let converter = FileConverter::new();

    match converter.convert_to_markdown(&file_path).await {
        Ok(result) => {
            info!(
                "文件转换成功: {}, 耗时: {}ms",
                file_path, result.elapsed_ms
            );
            result.into()
        }
        Err(e) => {
            error!("文件转换失败: {}, 错误: {}", file_path, e);
            e.into()
        }
    }
}

/// 批量转换文件为 Markdown
///
/// # Arguments
/// * `file_paths` - 文件路径列表
///
/// # Returns
/// 转换结果列表
#[tauri::command]
pub async fn convert_files_to_markdown(file_paths: Vec<String>) -> Vec<ConversionResponse> {
    info!("收到批量文件转换请求: {} 个文件", file_paths.len());

    let converter = FileConverter::new();
    let mut results = Vec::with_capacity(file_paths.len());

    for path in file_paths {
        let response = match converter.convert_to_markdown(&path).await {
            Ok(result) => {
                info!("文件转换成功: {}", path);
                result.into()
            }
            Err(e) => {
                error!("文件转换失败: {}, 错误: {}", path, e);
                ConversionResponse {
                    success: false,
                    markdown: String::new(),
                    title: None,
                    source_path: path,
                    elapsed_ms: 0,
                    file_size: 0,
                    error: Some(e.to_string()),
                }
            }
        };
        results.push(response);
    }

    info!("批量转换完成: {} 个文件", results.len());
    results
}

/// 检测文件格式
///
/// # Arguments
/// * `file_path` - 文件路径
///
/// # Returns
/// 文件格式信息
#[tauri::command]
pub fn detect_file_format(file_path: String) -> FileFormatInfo {
    let path = std::path::Path::new(&file_path);
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_string();

    let format = FileFormat::from_extension(&extension);

    FileFormatInfo {
        format: format!("{:?}", format),
        supported: format.is_supported(),
        extension,
    }
}

/// 获取支持的文件格式列表
#[tauri::command]
pub fn get_supported_formats() -> Vec<FileFormatInfo> {
    vec![
        FileFormatInfo {
            format: "PDF".to_string(),
            supported: true,
            extension: "pdf".to_string(),
        },
        FileFormatInfo {
            format: "DOCX".to_string(),
            supported: true,
            extension: "docx".to_string(),
        },
        FileFormatInfo {
            format: "TXT".to_string(),
            supported: true,
            extension: "txt".to_string(),
        },
        FileFormatInfo {
            format: "Markdown".to_string(),
            supported: true,
            extension: "md".to_string(),
        },
        FileFormatInfo {
            format: "HTML".to_string(),
            supported: true,
            extension: "html".to_string(),
        },
        FileFormatInfo {
            format: "HTM".to_string(),
            supported: true,
            extension: "htm".to_string(),
        },
        FileFormatInfo {
            format: "DOC".to_string(),
            supported: false,
            extension: "doc".to_string(),
        },
        FileFormatInfo {
            format: "RTF".to_string(),
            supported: false,
            extension: "rtf".to_string(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_file_format() {
        let info = detect_file_format("test.pdf".to_string());
        assert_eq!(info.format, "Pdf");
        assert!(info.supported);

        let info = detect_file_format("test.docx".to_string());
        assert_eq!(info.format, "Docx");
        assert!(info.supported);

        let info = detect_file_format("test.xyz".to_string());
        assert_eq!(info.format, "Unknown");
        assert!(!info.supported);
    }

    #[test]
    fn test_get_supported_formats() {
        let formats = get_supported_formats();
        assert!(!formats.is_empty());

        let pdf = formats.iter().find(|f| f.extension == "pdf");
        assert!(pdf.is_some());
        assert!(pdf.unwrap().supported);
    }
}

// ============================================
// Word 公文格式化命令（纯 Rust 实现）
// TODO: 因 docx-rs API 变更暂时禁用，需要单独修复
// ============================================

/*
use crate::converter::word_formatter::{FormatResult, WordFormatter};

/// Word 格式化结果（前端友好格式）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WordFormatResponse {
    /// 是否成功
    pub success: bool,
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
    /// 错误信息（如果失败）
    pub error: Option<String>,
}

impl From<FormatResult> for WordFormatResponse {
    fn from(result: FormatResult) -> Self {
        Self {
            success: true,
            input_path: result.input_path,
            output_path: result.output_path,
            issues: result.issues,
            compliant: result.compliant,
            elapsed_ms: result.elapsed_ms,
            error: None,
        }
    }
}

/// 格式化 Word 文档（纯 Rust，GB/T 9704-2012 标准）
///
/// # Arguments
/// * `input_path` - 输入文件路径（.docx）
/// * `output_path` - 输出文件路径（可选）
///
/// # Returns
/// 格式化结果
///
/// # 注意
/// 此命令仅支持 .docx 文件，无法操作打开的 Word 文档。
/// 如需操作打开的文档，请使用 Python Sidecar 的 document 服务。
#[tauri::command]
pub fn format_word_document(
    input_path: String,
    output_path: Option<String>,
) -> WordFormatResponse {
    info!("收到 Word 格式化请求: {}", input_path);

    let formatter = WordFormatter::new();

    match formatter.format(&input_path, output_path.as_deref()) {
        Ok(result) => {
            info!(
                "Word 格式化成功: {} -> {}, 耗时: {}ms",
                input_path, result.output_path, result.elapsed_ms
            );
            result.into()
        }
        Err(e) => {
            error!("Word 格式化失败: {}, 错误: {}", input_path, e);
            WordFormatResponse {
                success: false,
                input_path,
                output_path: String::new(),
                issues: vec![],
                compliant: false,
                elapsed_ms: 0,
                error: Some(e.to_string()),
            }
        }
    }
}

/// 批量格式化 Word 文档
///
/// # Arguments
/// * `input_paths` - 输入文件路径列表
/// * `output_dir` - 输出目录（可选）
#[tauri::command]
pub fn format_word_documents_batch(
    input_paths: Vec<String>,
    output_dir: Option<String>,
) -> Vec<WordFormatResponse> {
    info!("收到批量 Word 格式化请求: {} 个文件", input_paths.len());

    let formatter = WordFormatter::new();
    let mut results = Vec::with_capacity(input_paths.len());

    for input_path in input_paths {
        // 计算输出路径
        let output_path = output_dir.as_ref().map(|dir| {
            let filename = std::path::Path::new(&input_path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("document");
            format!("{}/{}_formatted.docx", dir, filename)
        });

        let response = match formatter.format(&input_path, output_path.as_deref()) {
            Ok(result) => {
                info!("Word 格式化成功: {}", input_path);
                result.into()
            }
            Err(e) => {
                error!("Word 格式化失败: {}, 错误: {}", input_path, e);
                WordFormatResponse {
                    success: false,
                    input_path,
                    output_path: String::new(),
                    issues: vec![],
                    compliant: false,
                    elapsed_ms: 0,
                    error: Some(e.to_string()),
                }
            }
        };
        results.push(response);
    }

    info!("批量格式化完成: {} 个文件", results.len());
    results
}
*/