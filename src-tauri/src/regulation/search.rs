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
/// 在文本中用 `<mark>` 标签包裹匹配的关键词
#[allow(dead_code)]
pub fn highlight_keywords(text: &str, keywords: &[&str]) -> String {
    let mut result = text.to_string();

    for keyword in keywords {
        if keyword.is_empty() {
            continue;
        }

        // 简单的大小写不敏感替换
        let lower_text = result.to_lowercase();
        let lower_keyword = keyword.to_lowercase();

        let mut new_result = String::with_capacity(result.len() + 20);
        let mut last_end = 0;

        for (start, _) in lower_text.match_indices(&lower_keyword) {
            new_result.push_str(&result[last_end..start]);
            new_result.push_str("<mark>");
            new_result.push_str(&result[start..start + keyword.len()]);
            new_result.push_str("</mark>");
            last_end = start + keyword.len();
        }

        new_result.push_str(&result[last_end..]);
        result = new_result;
    }

    result
}

/// 提取搜索关键词
///
/// 从查询字符串中提取关键词列表
#[allow(dead_code)]
pub fn extract_keywords(query: &str) -> Vec<&str> {
    query
        .split_whitespace()
        .filter(|s| !s.is_empty() && s.len() > 1)
        .collect()
}

/// 计算文本摘要
///
/// 提取包含关键词的文本片段作为摘要
#[allow(dead_code)]
pub fn extract_snippet(content: &str, keywords: &[&str], max_length: usize) -> String {
    if content.is_empty() {
        return String::new();
    }

    let max_chars = max_length.max(1);
    let lower_content = content.to_lowercase();

    // 查找第一个关键词出现的位置
    let mut best_byte_pos = 0usize;
    for keyword in keywords {
        if let Some(pos) = lower_content.find(&keyword.to_lowercase()) {
            best_byte_pos = pos;
            break;
        }
    }

    let best_char_pos = lower_content[..best_byte_pos].chars().count();
    let total_chars = content.chars().count();
    let start_char = best_char_pos.saturating_sub(max_chars / 4);
    let end_char = (start_char + max_chars).min(total_chars);

    // 按字符边界提取摘要，避免 UTF-8 多字节字符被错误截断
    let mut snippet: String = content
        .chars()
        .skip(start_char)
        .take(end_char - start_char)
        .collect();

    // 添加省略号
    if start_char > 0 {
        snippet = format!("...{}", snippet);
    }
    if end_char < total_chars {
        snippet.push_str("...");
    }

    snippet
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
}
