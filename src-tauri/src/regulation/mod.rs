//! 规章查询模块
//!
//! 提供 CAAC 规章的本地全文索引和搜索功能。
//!
//! ## 功能
//! - 本地 Tantivy 索引：毫秒级搜索已下载的规章
//! - 中文分词：使用 jieba-rs 进行中文分词
//! - 增量索引：下载新规章时自动添加到索引
//! - 批量下载：从在线搜索结果批量下载并索引
//! - 文件去重：SHA256 哈希去重
//!
//! ## 架构
//! ```text
//! ┌─────────────────────────────────────────┐
//! │           RegulationIndex               │
//! ├─────────────────────────────────────────┤
//! │  • index: Tantivy Index                 │
//! │  • reader: IndexReader (搜索用)          │
//! │  • writer: IndexWriter (索引用)          │
//! └─────────────────────────────────────────┘
//!                    ↑
//! ┌─────────────────────────────────────────┐
//! │           RegulationSync                │
//! ├─────────────────────────────────────────┤
//! │  • 文件去重 (SHA256)                     │
//! │  • 下载状态跟踪                          │
//! │  • OCR 队列管理                         │
//! └─────────────────────────────────────────┘
//!                    ↑
//! ┌─────────────────────────────────────────┐
//! │          RegulationCrawler              │
//! ├─────────────────────────────────────────┤
//! │  • 批量下载                              │
//! │  • 限速控制                              │
//! │  • 进度回调                              │
//! └─────────────────────────────────────────┘
//! ```

mod index;
mod schema;
mod search;
mod sync;
mod crawler;
mod text_extractor;
pub mod pdf_ocr;
mod commands;
pub mod online_search;

pub use index::RegulationIndex;
pub use schema::RegulationDocument;
pub use sync::{RegulationSync, DownloadResult, BatchProgress, calculate_file_hash, calculate_bytes_hash};
pub use crawler::{RegulationCrawler, DownloadConfig, DownloadItem};
pub use online_search::{CaacOnlineSearcher, OnlineSearchRequest, OnlineSearchResponse, OnlineDocument};
// 使用通配符导出所有命令（包括 Tauri 宏生成的 __cmd__ 函数）
pub use commands::*;
