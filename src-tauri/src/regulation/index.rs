//! 规章索引核心实现
//!
//! 使用 Tantivy 构建本地全文索引，支持中文分词。

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::Schema;
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument};
use tracing::{debug, info, warn};

use crate::error::HuGeError;
use super::schema::{RegulationDocument, RegulationFields};

/// 索引内存预算（64MB）
const INDEX_MEMORY_BUDGET: usize = 64 * 1024 * 1024;

/// 索引 Schema 版本号
/// v1: content 字段仅 TEXT（不存储）
/// v2: content 字段 TEXT | STORED（支持搜索摘要）
const INDEX_SCHEMA_VERSION: u32 = 2;

/// 规章索引管理器
pub struct RegulationIndex {
    index: Index,
    #[allow(dead_code)]
    schema: Schema,
    fields: RegulationFields,
    reader: IndexReader,
    writer: Arc<RwLock<IndexWriter>>,
    index_path: PathBuf,
}

impl RegulationIndex {
    /// 打开或创建索引
    ///
    /// # Arguments
    /// * `index_path` - 索引存储目录
    ///
    /// # Returns
    /// * `Ok(RegulationIndex)` - 索引管理器实例
    /// * `Err(HuGeError)` - 初始化失败
    pub fn open_or_create(index_path: PathBuf) -> Result<Self, HuGeError> {
        info!("初始化规章索引: {:?}", index_path);

        // 确保目录存在
        std::fs::create_dir_all(&index_path).map_err(|e| {
            HuGeError::Internal(format!("创建索引目录失败: {}", e))
        })?;

        // 检查 Schema 版本，不匹配时删除旧索引
        let version_file = index_path.join(".schema_version");
        let current_version = Self::read_schema_version(&version_file);
        if current_version != INDEX_SCHEMA_VERSION && index_path.join("meta.json").exists() {
            info!(
                "索引 Schema 版本不匹配 (当前: {}, 需要: {})，删除旧索引以重建",
                current_version, INDEX_SCHEMA_VERSION
            );
            Self::delete_index_files(&index_path)?;
        }

        // 构建 Schema
        let (schema, fields) = RegulationFields::build_schema();

        // 打开或创建索引
        let index = if index_path.join("meta.json").exists() {
            info!("打开已有索引");
            Index::open_in_dir(&index_path).map_err(|e| {
                HuGeError::Internal(format!("打开索引失败: {}", e))
            })?
        } else {
            info!("创建新索引");
            Index::create_in_dir(&index_path, schema.clone()).map_err(|e| {
                HuGeError::Internal(format!("创建索引失败: {}", e))
            })?
        };

        // 注册中文分词器
        Self::register_chinese_tokenizer(&index)?;

        // 创建 Reader（用于搜索）
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .map_err(|e| HuGeError::Internal(format!("创建 IndexReader 失败: {}", e)))?;

        // 创建 Writer（用于索引）
        let writer = index
            .writer(INDEX_MEMORY_BUDGET)
            .map_err(|e| HuGeError::Internal(format!("创建 IndexWriter 失败: {}", e)))?;

        // 写入当前 Schema 版本号
        Self::write_schema_version(&version_file, INDEX_SCHEMA_VERSION);

        info!("规章索引初始化完成");

        Ok(Self {
            index,
            schema,
            fields,
            reader,
            writer: Arc::new(RwLock::new(writer)),
            index_path,
        })
    }

    /// 注册中文分词器
    fn register_chinese_tokenizer(index: &Index) -> Result<(), HuGeError> {
        use tantivy::tokenizer::{LowerCaser, TextAnalyzer, RemoveLongFilter};

        // 使用 jieba 分词器
        let chinese_tokenizer = TextAnalyzer::builder(JiebaTokenizer)
            .filter(RemoveLongFilter::limit(40))
            .filter(LowerCaser)
            .build();

        // 关键：注册为 "default"，覆盖 tantivy 自带的 SimpleTokenizer
        // 这样所有使用 TEXT 预设的字段（title, office_unit, content）都会自动使用 jieba 分词
        index.tokenizers().register("default", chinese_tokenizer);

        debug!("中文分词器注册完成（已替换默认分词器）");
        Ok(())
    }

    /// 添加文档到索引
    pub fn add_document(&self, doc: &RegulationDocument) -> Result<(), HuGeError> {
        let tantivy_doc = doc.to_tantivy_doc(&self.fields);

        let writer = self.writer.write().map_err(|e| {
            HuGeError::Internal(format!("获取 Writer 锁失败: {}", e))
        })?;

        writer.add_document(tantivy_doc).map_err(|e| {
            HuGeError::Internal(format!("添加文档失败: {}", e))
        })?;

        debug!("文档已添加到索引: {}", doc.title);
        Ok(())
    }

    /// 批量添加文档
    pub fn add_documents(&self, docs: &[RegulationDocument]) -> Result<usize, HuGeError> {
        let writer = self.writer.write().map_err(|e| {
            HuGeError::Internal(format!("获取 Writer 锁失败: {}", e))
        })?;

        let mut count = 0;
        for doc in docs {
            let tantivy_doc = doc.to_tantivy_doc(&self.fields);
            if writer.add_document(tantivy_doc).is_ok() {
                count += 1;
            }
        }

        info!("批量添加 {} 个文档到索引", count);
        Ok(count)
    }

    /// 提交索引更改
    pub fn commit(&self) -> Result<(), HuGeError> {
        let mut writer = self.writer.write().map_err(|e| {
            HuGeError::Internal(format!("获取 Writer 锁失败: {}", e))
        })?;

        writer.commit().map_err(|e| {
            HuGeError::Internal(format!("提交索引失败: {}", e))
        })?;
        drop(writer);

        // OnCommitWithDelay 是异步刷新；这里显式 reload 让调用方在 commit 后立即可见结果
        self.reader.reload().map_err(|e| {
            HuGeError::Internal(format!("刷新索引读取器失败: {}", e))
        })?;

        info!("索引已提交");
        Ok(())
    }

    /// 搜索文档
    ///
    /// # Arguments
    /// * `query_str` - 搜索关键词
    /// * `limit` - 返回结果数量限制
    ///
    /// # Returns
    /// * `Ok(Vec<RegulationDocument>)` - 搜索结果
    pub fn search(&self, query_str: &str, limit: usize) -> Result<Vec<RegulationDocument>, HuGeError> {
        if query_str.trim().is_empty() {
            return Ok(Vec::new());
        }

        let searcher = self.reader.searcher();

        // 只在 TEXT 类型字段中搜索（标题、发布单位、正文）
        // doc_number 是 STRING 类型，用于精确匹配，不参与全文搜索
        let mut query_parser = QueryParser::for_index(
            &self.index,
            vec![
                self.fields.title,
                self.fields.office_unit,
                self.fields.content,
            ],
        );

        // 设置字段权重：标题 >> 发布单位 > 正文
        // 标题（文件名）匹配优先级最高，确保本地文件按文件名排序
        query_parser.set_field_boost(self.fields.title, 50.0);
        query_parser.set_field_boost(self.fields.office_unit, 5.0);
        query_parser.set_field_boost(self.fields.content, 0.5);

        let query = query_parser.parse_query(query_str).map_err(|e| {
            warn!("解析查询失败: {}, 使用模糊匹配", e);
            HuGeError::Internal(format!("解析查询失败: {}", e))
        })?;

        let top_docs = searcher
            .search(&query, &TopDocs::with_limit(limit))
            .map_err(|e| HuGeError::Internal(format!("搜索失败: {}", e)))?;

        let mut results = Vec::with_capacity(top_docs.len());
        for (_score, doc_address) in top_docs {
            if let Ok(doc) = searcher.doc::<TantivyDocument>(doc_address) {
                results.push(RegulationDocument::from_tantivy_doc(&doc, &self.fields));
            }
        }

        debug!("搜索 '{}' 返回 {} 条结果", query_str, results.len());
        Ok(results)
    }

    /// 按有效性和文档类型筛选搜索
    ///
    /// 使用 Tantivy `BooleanQuery` 将全文搜索与 TermQuery 过滤条件组合，
    /// 直接在索引层过滤，避免先取再筛的低效做法。
    pub fn search_with_filter(
        &self,
        query_str: &str,
        validity: Option<&str>,
        doc_type: Option<&str>,
        limit: usize,
    ) -> Result<Vec<RegulationDocument>, HuGeError> {
        use tantivy::query::{BooleanQuery, TermQuery};
        use tantivy::schema::IndexRecordOption;
        use tantivy::Term;

        if query_str.trim().is_empty() {
            return Ok(Vec::new());
        }

        let searcher = self.reader.searcher();

        // 构建全文搜索子查询
        let mut query_parser = QueryParser::for_index(
            &self.index,
            vec![self.fields.title, self.fields.office_unit, self.fields.content],
        );
        query_parser.set_field_boost(self.fields.title, 50.0);
        query_parser.set_field_boost(self.fields.office_unit, 5.0);
        query_parser.set_field_boost(self.fields.content, 0.5);

        let text_query = query_parser.parse_query(query_str).map_err(|e| {
            HuGeError::Internal(format!("解析查询失败: {}", e))
        })?;

        // 收集过滤条件作为 MUST 子句
        let mut clauses: Vec<(tantivy::query::Occur, Box<dyn tantivy::query::Query>)> = Vec::new();
        clauses.push((tantivy::query::Occur::Must, text_query));

        if let Some(v) = validity {
            let term_values: Vec<&str> = match v {
                "valid" => vec!["有效"],
                "invalid" => vec!["失效", "废止"],
                _ => vec![],
            };
            if term_values.len() == 1 {
                let term = Term::from_field_text(self.fields.validity, term_values[0]);
                let tq = TermQuery::new(term, IndexRecordOption::Basic);
                clauses.push((tantivy::query::Occur::Must, Box::new(tq)));
            } else if term_values.len() > 1 {
                // "失效" OR "废止" → 用 BooleanQuery(SHOULD) 包裹，再作为 MUST 子句
                let sub: Vec<(tantivy::query::Occur, Box<dyn tantivy::query::Query>)> = term_values
                    .iter()
                    .map(|val| {
                        let term = Term::from_field_text(self.fields.validity, val);
                        let tq: Box<dyn tantivy::query::Query> =
                            Box::new(TermQuery::new(term, IndexRecordOption::Basic));
                        (tantivy::query::Occur::Should, tq)
                    })
                    .collect();
                clauses.push((tantivy::query::Occur::Must, Box::new(BooleanQuery::new(sub))));
            }
        }

        if let Some(t) = doc_type {
            if t != "all" {
                let term = Term::from_field_text(self.fields.doc_type, t);
                let tq = TermQuery::new(term, IndexRecordOption::Basic);
                clauses.push((tantivy::query::Occur::Must, Box::new(tq)));
            }
        }

        let compound_query = BooleanQuery::new(clauses);

        let top_docs = searcher
            .search(&compound_query, &TopDocs::with_limit(limit))
            .map_err(|e| HuGeError::Internal(format!("搜索失败: {}", e)))?;

        let mut results = Vec::with_capacity(top_docs.len());
        for (_score, doc_address) in top_docs {
            if let Ok(doc) = searcher.doc::<TantivyDocument>(doc_address) {
                results.push(RegulationDocument::from_tantivy_doc(&doc, &self.fields));
            }
        }

        debug!("搜索 '{}' (filter: validity={:?}, doc_type={:?}) 返回 {} 条结果",
            query_str, validity, doc_type, results.len());
        Ok(results)
    }

    /// 获取索引中的文档数量
    pub fn doc_count(&self) -> u64 {
        let searcher = self.reader.searcher();
        searcher.num_docs()
    }

    /// 删除所有文档（重建索引）
    pub fn clear(&self) -> Result<(), HuGeError> {
        let mut writer = self.writer.write().map_err(|e| {
            HuGeError::Internal(format!("获取 Writer 锁失败: {}", e))
        })?;

        writer.delete_all_documents().map_err(|e| {
            HuGeError::Internal(format!("删除文档失败: {}", e))
        })?;

        writer.commit().map_err(|e| {
            HuGeError::Internal(format!("提交失败: {}", e))
        })?;

        info!("索引已清空");
        Ok(())
    }

    /// 按文号精确搜索
    ///
    /// 文号是 STRING 类型字段，需要精确匹配（如 "CCAR-121-R7"）
    pub fn search_by_doc_number(&self, doc_number: &str) -> Result<Option<RegulationDocument>, HuGeError> {
        use tantivy::query::TermQuery;
        use tantivy::schema::IndexRecordOption;
        use tantivy::Term;

        if doc_number.trim().is_empty() {
            return Ok(None);
        }

        let searcher = self.reader.searcher();
        let term = Term::from_field_text(self.fields.doc_number, doc_number);
        let query = TermQuery::new(term, IndexRecordOption::Basic);

        let top_docs = searcher
            .search(&query, &TopDocs::with_limit(1))
            .map_err(|e| HuGeError::Internal(format!("搜索失败: {}", e)))?;

        if let Some((_score, doc_address)) = top_docs.first() {
            if let Ok(doc) = searcher.doc::<TantivyDocument>(*doc_address) {
                return Ok(Some(RegulationDocument::from_tantivy_doc(&doc, &self.fields)));
            }
        }

        Ok(None)
    }

    /// 检查文档是否已存在（通过 URL 判断）
    pub fn exists(&self, url: &str) -> bool {
        use tantivy::query::TermQuery;
        use tantivy::schema::IndexRecordOption;
        use tantivy::Term;

        let searcher = self.reader.searcher();
        let term = Term::from_field_text(self.fields.url, url);
        let query = TermQuery::new(term, IndexRecordOption::Basic);

        searcher
            .search(&query, &TopDocs::with_limit(1))
            .map(|docs| !docs.is_empty())
            .unwrap_or(false)
    }

    /// 获取索引路径
    pub fn index_path(&self) -> &PathBuf {
        &self.index_path
    }

    /// 读取 Schema 版本号文件，不存在则返回 0
    fn read_schema_version(version_file: &std::path::Path) -> u32 {
        std::fs::read_to_string(version_file)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .unwrap_or(0)
    }

    /// 写入 Schema 版本号文件
    fn write_schema_version(version_file: &std::path::Path, version: u32) {
        if let Err(e) = std::fs::write(version_file, version.to_string()) {
            warn!("写入 Schema 版本文件失败: {}", e);
        }
    }

    /// 删除索引目录下的所有文件（保留目录本身）
    fn delete_index_files(index_path: &std::path::Path) -> Result<(), HuGeError> {
        let entries = std::fs::read_dir(index_path).map_err(|e| {
            HuGeError::Internal(format!("读取索引目录失败: {}", e))
        })?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Err(e) = std::fs::remove_file(&path) {
                    warn!("删除索引文件失败 {:?}: {}", path, e);
                }
            }
        }

        info!("旧索引文件已删除");
        Ok(())
    }
}

/// Jieba 中文分词器
#[derive(Clone)]
struct JiebaTokenizer;

/// 全局 Jieba 实例（懒加载单例）
fn get_jieba() -> &'static jieba_rs::Jieba {
    use std::sync::OnceLock;
    static JIEBA: OnceLock<jieba_rs::Jieba> = OnceLock::new();
    JIEBA.get_or_init(|| {
        debug!("初始化 Jieba 分词器");
        jieba_rs::Jieba::new()
    })
}

impl tantivy::tokenizer::Tokenizer for JiebaTokenizer {
    type TokenStream<'a> = JiebaTokenStream<'a>;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> Self::TokenStream<'a> {
        JiebaTokenStream::new(text)
    }
}

/// Jieba 分词流
struct JiebaTokenStream<'a> {
    tokens: Vec<&'a str>,
    index: usize,
    offset: usize,
    token: tantivy::tokenizer::Token,
}

impl<'a> JiebaTokenStream<'a> {
    fn new(text: &'a str) -> Self {
        // 使用全局 jieba 实例分词
        let tokens: Vec<&str> = get_jieba()
            .cut(text, true) // true = HMM 模式
            .into_iter()
            .filter(|s| !s.trim().is_empty())
            .collect();

        Self {
            tokens,
            index: 0,
            offset: 0,
            token: tantivy::tokenizer::Token::default(),
        }
    }
}

impl<'a> tantivy::tokenizer::TokenStream for JiebaTokenStream<'a> {
    fn advance(&mut self) -> bool {
        if self.index >= self.tokens.len() {
            return false;
        }

        let text = self.tokens[self.index];
        self.token.text.clear();
        self.token.text.push_str(text);
        self.token.offset_from = self.offset;
        self.token.offset_to = self.offset + text.len();
        self.token.position = self.index;

        self.offset = self.token.offset_to;
        self.index += 1;

        true
    }

    fn token(&self) -> &tantivy::tokenizer::Token {
        &self.token
    }

    fn token_mut(&mut self) -> &mut tantivy::tokenizer::Token {
        &mut self.token
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_create_index() {
        let temp_dir = TempDir::new().unwrap();
        let index = RegulationIndex::open_or_create(temp_dir.path().to_path_buf());
        assert!(index.is_ok());
    }

    #[test]
    fn test_add_and_search() {
        let temp_dir = TempDir::new().unwrap();
        let index = RegulationIndex::open_or_create(temp_dir.path().to_path_buf()).unwrap();

        let doc = RegulationDocument {
            title: "大型飞机公共航空运输承运人运行合格审定规则".to_string(),
            doc_number: "CCAR-121-R7".to_string(),
            validity: "有效".to_string(),
            doc_type: "regulation".to_string(),
            office_unit: "中国民用航空局".to_string(),
            sign_date: "2024-01-01".to_string(),
            publish_date: "2024-01-15".to_string(),
            url: "https://example.com/doc1".to_string(),
            file_path: "/path/to/doc.pdf".to_string(),
            content: "本规则适用于大型飞机公共航空运输承运人".to_string(),
        };

        index.add_document(&doc).unwrap();
        index.commit().unwrap();

        let results = index.search("大型飞机", 10).unwrap();
        assert!(!results.is_empty());
        assert_eq!(results[0].doc_number, "CCAR-121-R7");
    }
}
