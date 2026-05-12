//! 规章 PDF 文件名规则化工具
//!
//! 集中管理「规则化文件名」生成逻辑，让以下三处使用同一套规则：
//!
//! - 官网同步下载 (`crawler::download_file`)
//! - 本地扫描复制 (`commands::resolve_storage_path`)
//! - 一键对齐磁盘文件名 (`commands::regulation_realign_pdf_filenames`)
//!
//! ## 命名规则
//!
//! 优先级（从高到低）：
//! 1. `{doc_number}_{title}.pdf` — 文号 + 标题，最易读
//! 2. `{title}.pdf` — 仅标题
//! 3. `{doc_number}.pdf` — 仅文号
//! 4. `{sha256前16位}.pdf` — 兜底（前面三项全为空时）
//!
//! 文件名 stem 部分通过 [`sanitize_filename`] 去除 Windows 非法字符，
//! 并截断到 180 个字符以避免触发 Windows 的 MAX_PATH 限制。
//!
//! 重名冲突时使用 [`dedupe_filename`] 在 stem 末尾追加 `__<sha256前6位>`。

use std::path::{Path, PathBuf};

/// Windows / 跨平台的文件名长度上限（按字符数）。
///
/// 取 180 是经验值：Windows MAX_PATH 是 260，留出 80 字符给目录前缀。
const MAX_STEM_CHARS: usize = 180;

/// 替换文件名中的非法字符，并截断到 [`MAX_STEM_CHARS`] 个字符。
///
/// 替换的非法字符：`< > : " / \ | ? *` 以及控制字符。
/// 输入为空 / 全是非法字符 / 全是空白时返回 `"document"` 作为安全兜底。
pub fn sanitize_filename(input: &str) -> String {
    let mut out: String = input
        .chars()
        .map(|c| {
            if matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
                || c.is_control()
            {
                '_'
            } else {
                c
            }
        })
        .collect();

    // 去除前后空白和点号（Windows 文件名末尾点号会被自动去掉，提前规范化避免歧义）
    out = out.trim().trim_end_matches('.').trim().to_string();
    if out.is_empty() {
        return "document".to_string();
    }

    // 按字符数截断
    out.chars().take(MAX_STEM_CHARS).collect::<String>()
}

/// 根据规章元数据生成"规则化"PDF 文件名。
///
/// # 参数
///
/// - `doc_number`: 文号（例：`IB-FS-MED-016`），可选
/// - `title`: 标题（例：`民用航空人员安全用药指南（第一版）`），可选
/// - `sha256`: 文件 SHA256 全 64 字符，作为最后兜底
/// - `ext`: 扩展名（不带点，例：`pdf`）
///
/// # 返回
///
/// 形如 `"IB-FS-MED-016_民用航空人员安全用药指南.pdf"` 的文件名。
pub fn build_pretty_filename(
    doc_number: Option<&str>,
    title: Option<&str>,
    sha256: &str,
    ext: &str,
) -> String {
    let doc_number = doc_number.map(str::trim).filter(|s| !s.is_empty());
    let title = title.map(str::trim).filter(|s| !s.is_empty());

    let stem = match (doc_number, title) {
        (Some(num), Some(t)) => format!("{}_{}", num, t),
        (Some(num), None) => num.to_string(),
        (None, Some(t)) => t.to_string(),
        (None, None) => sha256.chars().take(16).collect::<String>(),
    };

    let stem = sanitize_filename(&stem);
    let ext = if ext.is_empty() { "pdf" } else { ext };
    format!("{}.{}", stem, ext)
}

/// 在指定目录中为 `desired_name` 解决重名冲突。
///
/// 算法：
/// 1. 如果目标路径不存在，直接使用 `desired_name`
/// 2. 否则在 stem 末尾追加 `__<sha256前6位>` 再尝试
/// 3. 仍冲突则继续追加 `_2` `_3` …
///
/// 返回最终选定的完整路径。
pub fn dedupe_filename(target_dir: &Path, desired_name: &str, sha256: &str) -> PathBuf {
    let target_path = target_dir.join(desired_name);
    if !target_path.exists() {
        return target_path;
    }

    let path = Path::new(desired_name);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("document");
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("pdf");

    let sha_short: String = sha256.chars().take(6).collect();
    let with_sha = format!("{}__{}.{}", stem, sha_short, ext);
    let with_sha_path = target_dir.join(&with_sha);
    if !with_sha_path.exists() {
        return with_sha_path;
    }

    // 极端情况：连 sha 短缀都撞了（不同文件 sha 前 6 位重复 + 同元数据），
    // 加数字后缀。
    let stem_with_sha = format!("{}__{}", stem, sha_short);
    for n in 2..1000 {
        let candidate = format!("{}_{}.{}", stem_with_sha, n, ext);
        let candidate_path = target_dir.join(&candidate);
        if !candidate_path.exists() {
            return candidate_path;
        }
    }

    // 仍然冲突就强制覆盖原路径（理论上几乎不可能到这里）
    target_dir.join(desired_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn sanitize_replaces_illegal_chars() {
        assert_eq!(sanitize_filename(r#"a<b>c:d"e/f\g|h?i*j"#), "a_b_c_d_e_f_g_h_i_j");
    }

    #[test]
    fn sanitize_trims_whitespace_and_trailing_dot() {
        assert_eq!(sanitize_filename("  hello  "), "hello");
        assert_eq!(sanitize_filename("file."), "file");
        assert_eq!(sanitize_filename("file...."), "file");
    }

    #[test]
    fn sanitize_returns_document_for_blank_input() {
        assert_eq!(sanitize_filename(""), "document");
        assert_eq!(sanitize_filename("   "), "document");
    }

    #[test]
    fn sanitize_truncates_long_names_by_chars_not_bytes() {
        // 200 个中文字符；如果按字节截断会切到 UTF-8 中间
        let long = "中".repeat(200);
        let out = sanitize_filename(&long);
        assert_eq!(out.chars().count(), MAX_STEM_CHARS);
        // 验证不破坏 UTF-8
        assert!(out.chars().all(|c| c == '中'));
    }

    #[test]
    fn build_pretty_uses_doc_number_and_title() {
        let name = build_pretty_filename(
            Some("IB-FS-MED-016"),
            Some("民用航空人员安全用药指南（第一版）"),
            "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
            "pdf",
        );
        assert_eq!(name, "IB-FS-MED-016_民用航空人员安全用药指南（第一版）.pdf");
    }

    #[test]
    fn build_pretty_falls_back_to_title_only() {
        let name = build_pretty_filename(
            None,
            Some("民用航空运输承运人运行合格审定规则"),
            "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
            "pdf",
        );
        assert_eq!(name, "民用航空运输承运人运行合格审定规则.pdf");
    }

    #[test]
    fn build_pretty_falls_back_to_doc_number_only() {
        let name = build_pretty_filename(
            Some("CCAR-121"),
            None,
            "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
            "pdf",
        );
        assert_eq!(name, "CCAR-121.pdf");
    }

    #[test]
    fn build_pretty_falls_back_to_sha256_when_metadata_missing() {
        let name = build_pretty_filename(
            None,
            None,
            "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789",
            "pdf",
        );
        assert_eq!(name, "abcdef0123456789.pdf");
    }

    #[test]
    fn build_pretty_treats_empty_strings_as_missing() {
        let name =
            build_pretty_filename(Some(""), Some("   "), "0123456789abcdef".repeat(4).as_str(), "pdf");
        assert_eq!(name, "0123456789abcdef.pdf");
    }

    #[test]
    fn build_pretty_strips_illegal_chars_from_title() {
        let name = build_pretty_filename(
            Some("CCAR-121"),
            Some(r#"标题 / 含 \ 非法 ? 字符 *"#),
            "abcd",
            "pdf",
        );
        assert_eq!(name, "CCAR-121_标题 _ 含 _ 非法 _ 字符 _.pdf");
    }

    #[test]
    fn build_pretty_uses_default_ext_when_blank() {
        let name = build_pretty_filename(Some("CCAR-121"), None, "abcd", "");
        assert_eq!(name, "CCAR-121.pdf");
    }

    #[test]
    fn dedupe_returns_desired_when_no_conflict() {
        let dir = tempdir().unwrap();
        let path = dedupe_filename(dir.path(), "test.pdf", "abcdef123456");
        assert_eq!(path.file_name().unwrap().to_str().unwrap(), "test.pdf");
    }

    #[test]
    fn dedupe_appends_sha_short_on_conflict() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("test.pdf"), b"existing").unwrap();

        let path = dedupe_filename(dir.path(), "test.pdf", "abcdef123456789");
        assert_eq!(path.file_name().unwrap().to_str().unwrap(), "test__abcdef.pdf");
    }

    #[test]
    fn dedupe_appends_numeric_suffix_when_sha_short_also_conflicts() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("test.pdf"), b"a").unwrap();
        fs::write(dir.path().join("test__abcdef.pdf"), b"b").unwrap();

        let path = dedupe_filename(dir.path(), "test.pdf", "abcdef999");
        assert_eq!(path.file_name().unwrap().to_str().unwrap(), "test__abcdef_2.pdf");
    }
}
