//! 搜索辅助功能
//!
//! 提供搜索结果排序、高亮等辅助功能。

use super::schema::RegulationDocument;

/// 搜索结果排序方式
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortOrder {
    /// 按相关性排序（默认）
    Relevance,
    /// 按发布日期降序
    DateDesc,
    /// 按发布日期升序
    DateAsc,
    /// 按标题字母顺序
    TitleAsc,
}

/// 对搜索结果进行排序
pub fn sort_results(results: &mut [RegulationDocument], order: SortOrder) {
    match order {
        SortOrder::Relevance => {
            // Tantivy 已按相关性排序，无需处理
        }
        SortOrder::DateDesc => {
            results.sort_by(|a, b| b.publish_date.cmp(&a.publish_date));
        }
        SortOrder::DateAsc => {
            results.sort_by(|a, b| a.publish_date.cmp(&b.publish_date));
        }
        SortOrder::TitleAsc => {
            results.sort_by(|a, b| a.title.cmp(&b.title));
        }
    }
}

/// 高亮搜索关键词
///
/// 在文本中用 `<mark>` 标签包裹匹配的关键词。
/// 先对文本进行 HTML 转义，再插入标记标签，防止 XSS。
///
/// 注意：内部使用 `ascii_lower_preserving_bytes` 而不是 `String::to_lowercase()`，
/// 因为后者对部分 Unicode 字符（如 `İ`→`i\u{0307}`、`ẞ`→`ß`、`Σ`→`σ/ς`）
/// 会改变字节长度，导致 lower_text 与 result 字节错位，
/// `match_indices` 返回的 byte index 可能落在 result 中多字节字符（如中文 `飞`）
/// 的中间，引发 `byte index is not a char boundary` panic。
pub fn highlight_keywords(text: &str, keywords: &[&str]) -> String {
    // 先 HTML 转义原始文本，防止注入
    let mut result = html_escape(text);

    for keyword in keywords {
        if keyword.is_empty() {
            continue;
        }

        // 关键词也需要转义后再匹配
        let escaped_keyword = html_escape(keyword);
        let lower_text = ascii_lower_preserving_bytes(&result);
        let lower_keyword = ascii_lower_preserving_bytes(&escaped_keyword);

        let mut new_result = String::with_capacity(result.len() + 20);
        let mut last_end = 0;

        for (start, matched) in lower_text.match_indices(&lower_keyword) {
            new_result.push_str(&result[last_end..start]);
            new_result.push_str("<mark>");
            new_result.push_str(&result[start..start + matched.len()]);
            new_result.push_str("</mark>");
            last_end = start + matched.len();
        }

        new_result.push_str(&result[last_end..]);
        result = new_result;
    }

    result
}

/// 仅对 ASCII 字符做小写化，非 ASCII 字符原样保留。
///
/// 与 `String::to_lowercase()` 不同，本函数保证：
/// 1. 输出与输入字节长度完全相等
/// 2. 字节位置一一对应（任何字节索引在两者中都指向同一个字符的同一字节）
///
/// 这是高亮 / 摘要等需要"按位置反向定位到原文"场景的关键不变量。
fn ascii_lower_preserving_bytes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii() {
            out.push(c.to_ascii_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// HTML 转义，防止 XSS 注入
fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

/// 提取搜索关键词
///
/// 从查询字符串中提取关键词列表
pub fn extract_keywords(query: &str) -> Vec<&str> {
    query.split_whitespace().filter(|s| !s.is_empty() && s.chars().count() > 1).collect()
}

/// 计算文本摘要
///
/// 提取包含关键词的文本片段作为摘要
pub fn extract_snippet(content: &str, keywords: &[&str], max_length: usize) -> String {
    if content.is_empty() {
        return String::new();
    }

    let max_chars = max_length.max(1);
    // 使用 ASCII-only 小写化：保证 lower_content 与 content 的字节布局完全一致，
    // 否则后续 best_char_pos 可能漂移（详见 ascii_lower_preserving_bytes 文档）。
    let lower_content = ascii_lower_preserving_bytes(content);

    // 查找第一个关键词出现的位置
    let mut best_byte_pos = 0usize;
    for keyword in keywords {
        let lower_keyword = ascii_lower_preserving_bytes(keyword);
        if let Some(pos) = lower_content.find(&lower_keyword) {
            best_byte_pos = pos;
            break;
        }
    }

    let best_char_pos = lower_content[..best_byte_pos].chars().count();
    let total_chars = content.chars().count();
    let start_char = best_char_pos.saturating_sub(max_chars / 4);
    let end_char = (start_char + max_chars).min(total_chars);

    // 按字符边界提取摘要，避免 UTF-8 多字节字符被错误截断
    let mut snippet: String =
        content.chars().skip(start_char).take(end_char - start_char).collect();

    // 添加省略号
    if start_char > 0 {
        snippet = format!("...{}", snippet);
    }
    if end_char < total_chars {
        snippet.push_str("...");
    }

    snippet
}

/// 为搜索结果批量生成摘要
///
/// 对每条结果提取正文摘要并高亮关键词。
/// 返回与 `results` 等长的 `Vec<Option<String>>`，
/// 正文为空时对应位置为 `None`。
pub fn generate_snippets(
    results: &[RegulationDocument],
    query: &str,
    max_len: usize,
) -> Vec<Option<String>> {
    let keywords = extract_keywords(query);
    results
        .iter()
        .map(|doc| {
            if doc.content.is_empty() {
                None
            } else {
                let snippet = extract_snippet(&doc.content, &keywords, max_len);
                if snippet.is_empty() {
                    None
                } else {
                    Some(highlight_keywords(&snippet, &keywords))
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_highlight_keywords() {
        let text = "大型飞机公共航空运输承运人运行合格审定规则";
        let result = highlight_keywords(text, &["飞机", "运输"]);
        assert!(result.contains("<mark>飞机</mark>"));
        assert!(result.contains("<mark>运输</mark>"));
    }

    #[test]
    fn test_highlight_keywords_xss_prevention() {
        let text = "<script>alert('xss')</script>飞机测试";
        let result = highlight_keywords(text, &["飞机"]);
        // 应该转义 HTML 标签
        assert!(!result.contains("<script>"));
        assert!(result.contains("&lt;script&gt;"));
        assert!(result.contains("<mark>飞机</mark>"));
    }

    #[test]
    fn test_extract_keywords() {
        let query = "大型 飞机 运输";
        let keywords = extract_keywords(query);
        assert_eq!(keywords, vec!["大型", "飞机", "运输"]);
    }

    #[test]
    fn test_extract_snippet() {
        let content = "本规则适用于大型飞机公共航空运输承运人的运行合格审定";
        let snippet = extract_snippet(content, &["飞机"], 20);
        assert!(snippet.contains("飞机"));
    }

    /// 回归测试：之前 `to_lowercase()` 会把土耳其大写 `İ`（2 字节）
    /// 展开为 `i\u{0307}`（3 字节），导致 lower_text 与 result 字节错位，
    /// 切片落到中文字符中间引发 panic：
    /// `byte index 102 is not a char boundary; it is inside '飞' (bytes 101..104)`
    #[test]
    fn test_highlight_keywords_with_unicode_case_changing_chars() {
        // 含土耳其大写 İ + 大量中文 + 用户搜索词
        let text = "İstanbul 是城市。飞行程序设计、塔台监视、机场蠕行仿真验证等工程实践都是民航适航专业重点。";
        let result = highlight_keywords(text, &["飞行"]);
        // 不应 panic，并且能成功高亮
        assert!(result.contains("<mark>飞行</mark>"), "result = {}", result);
    }

    #[test]
    fn test_highlight_keywords_with_eszett() {
        // 德语 ẞ (U+1E9E, 3 字节) lowercase 为 ß (U+00DF, 2 字节) - 字节长度变化
        let text = "STRAẞE 飞行测试";
        let result = highlight_keywords(text, &["飞行"]);
        assert!(result.contains("<mark>飞行</mark>"), "result = {}", result);
    }

    #[test]
    fn test_highlight_keywords_case_insensitive_ascii() {
        // 确认 ASCII 大小写不敏感仍工作
        let text = "Pilot Flight Manual";
        let result = highlight_keywords(text, &["pilot", "FLIGHT"]);
        assert!(result.contains("<mark>Pilot</mark>"), "result = {}", result);
        assert!(result.contains("<mark>Flight</mark>"), "result = {}", result);
    }

    #[test]
    fn test_extract_snippet_with_unicode_case_changing_chars() {
        // 同样的字符在 extract_snippet 里不会崩溃
        let content = "İstanbul 飞行程序设计、塔台监视、机场蠕行仿真验证等工程实践";
        let snippet = extract_snippet(content, &["飞行"], 30);
        assert!(snippet.contains("飞行"), "snippet = {}", snippet);
    }
}
