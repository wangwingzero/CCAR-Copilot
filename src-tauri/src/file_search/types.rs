//! 文件搜索类型定义

use serde::{Deserialize, Serialize};

/// 搜索匹配模式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MatchMode {
    /// 精确匹配
    Exact,
    /// 通配符匹配
    Wildcard,
    /// 模糊匹配（容错）
    Fuzzy,
    /// 正则表达式
    Regex,
}

/// 搜索结果排序字段
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortField {
    /// 按相关性排序
    Relevance,
    /// 按文件名排序
    Name,
    /// 按路径排序
    Path,
    /// 按文件大小排序
    Size,
    /// 按修改时间排序
    Modified,
}

/// 排序方向
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortOrder {
    /// 升序
    Asc,
    /// 降序
    Desc,
}
