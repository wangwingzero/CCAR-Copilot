//! 文件搜索模块
//!
//! 提供全盘文件名索引和快速搜索功能。
//! 使用 walkdir 遍历所有磁盘，rayon 并行搜索。

pub mod indexer;
pub mod types;

pub use indexer::FileIndexer;
pub use types::{MatchMode, SortField, SortOrder};
