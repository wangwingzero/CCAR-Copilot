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

use super::schema::{RegulationDocument, RegulationFields};
use crate::error::HuGeError;

/// 索引内存预算（64MB）
const INDEX_MEMORY_BUDGET: usize = 64 * 1024 * 1024;

/// 索引 Schema 版本号
/// v1: content 字段仅 TEXT（不存储）
/// v2: content 字段 TEXT | STORED（支持搜索摘要）
/// v3: url 字段 STRING | STORED（支持按 url 做 TermQuery / delete_term，修复一键对齐文件名）
const INDEX_SCHEMA_VERSION: u32 = 3;

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
        std::fs::create_dir_all(&index_path)
            .map_err(|e| HuGeError::Internal(format!("创建索引目录失败: {}", e)))?;

        // 检查 Schema 版本：能保留 content 的版本走数据迁移，其它情况删除重建
        let version_file = index_path.join(".schema_version");
        let current_version = Self::read_schema_version(&version_file);
        if current_version != INDEX_SCHEMA_VERSION && index_path.join("meta.json").exists() {
            // v2→v3：content 已 STORED，可整体抢救出来回写新 schema，无需用户重做 OCR
            if current_version == 2 {
                info!("索引 Schema v2→v3 迁移：开始抢救已索引文档");
                Self::migrate_v2_to_v3(&index_path)?;
                info!("索引 Schema v2→v3 迁移完成");
            } else {
                info!(
                    "索引 Schema 版本不匹配 (当前: {}, 需要: {})，删除旧索引以重建",
                    current_version, INDEX_SCHEMA_VERSION
                );
                Self::delete_index_files(&index_path)?;
            }
        }

        // 构建 Schema
        let (schema, fields) = RegulationFields::build_schema();

        // 打开或创建索引
        let index = if index_path.join("meta.json").exists() {
            info!("打开已有索引");
            Index::open_in_dir(&index_path)
                .map_err(|e| HuGeError::Internal(format!("打开索引失败: {}", e)))?
        } else {
            info!("创建新索引");
            Index::create_in_dir(&index_path, schema.clone())
                .map_err(|e| HuGeError::Internal(format!("创建索引失败: {}", e)))?
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
        use tantivy::tokenizer::{LowerCaser, RemoveLongFilter, TextAnalyzer};

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

        let writer = self
            .writer
            .write()
            .map_err(|e| HuGeError::Internal(format!("获取 Writer 锁失败: {}", e)))?;

        writer
            .add_document(tantivy_doc)
            .map_err(|e| HuGeError::Internal(format!("添加文档失败: {}", e)))?;

        debug!("文档已添加到索引: {}", doc.title);
        Ok(())
    }

    /// 按 URL 覆盖写入文档。
    ///
    /// 扫描阶段会先写入只有标题/文件名的元数据文档；OCR 完成后需要用带正文的
    /// 文档替换它，否则后续正文搜索永远搜不到 OCR 结果。
    pub fn upsert_document(&self, doc: &RegulationDocument) -> Result<(), HuGeError> {
        use tantivy::Term;

        let tantivy_doc = doc.to_tantivy_doc(&self.fields);
        let term = Term::from_field_text(self.fields.url, &doc.url);

        let writer = self
            .writer
            .write()
            .map_err(|e| HuGeError::Internal(format!("获取 Writer 锁失败: {}", e)))?;

        writer.delete_term(term);
        writer
            .add_document(tantivy_doc)
            .map_err(|e| HuGeError::Internal(format!("更新文档失败: {}", e)))?;

        debug!("文档已覆盖写入索引: {}", doc.title);
        Ok(())
    }

    /// 批量添加文档
    pub fn add_documents(&self, docs: &[RegulationDocument]) -> Result<usize, HuGeError> {
        let writer = self
            .writer
            .write()
            .map_err(|e| HuGeError::Internal(format!("获取 Writer 锁失败: {}", e)))?;

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
        let mut writer = self
            .writer
            .write()
            .map_err(|e| HuGeError::Internal(format!("获取 Writer 锁失败: {}", e)))?;

        writer.commit().map_err(|e| HuGeError::Internal(format!("提交索引失败: {}", e)))?;
        drop(writer);

        // OnCommitWithDelay 是异步刷新；这里显式 reload 让调用方在 commit 后立即可见结果
        self.reader
            .reload()
            .map_err(|e| HuGeError::Internal(format!("刷新索引读取器失败: {}", e)))?;

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
    pub fn search(
        &self,
        query_str: &str,
        limit: usize,
    ) -> Result<Vec<RegulationDocument>, HuGeError> {
        if query_str.trim().is_empty() {
            return Ok(Vec::new());
        }

        let searcher = self.reader.searcher();

        // 只在 TEXT 类型字段中搜索（标题、发布单位、正文）
        // doc_number 是 STRING 类型，用于精确匹配，不参与全文搜索
        let mut query_parser = QueryParser::for_index(
            &self.index,
            vec![self.fields.title, self.fields.office_unit, self.fields.content],
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

        let text_query = query_parser
            .parse_query(query_str)
            .map_err(|e| HuGeError::Internal(format!("解析查询失败: {}", e)))?;

        // 收集过滤条件作为 MUST 子句
        let mut clauses: Vec<(tantivy::query::Occur, Box<dyn tantivy::query::Query>)> = Vec::new();
        clauses.push((tantivy::query::Occur::Must, text_query));

        if let Some(v) = validity {
            let term_values: Vec<&str> = match v {
                "valid" => vec!["有效"],
                "invalid" => vec!["失效", "废止", "历史版本"],
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
                let term_values: Vec<&str> = match t {
                    "normative" => vec![
                        "normative",
                        "advisory_circular",
                        "information_bulletin",
                        "management_document",
                        "administrative_procedure",
                    ],
                    _ => vec![t],
                };

                if term_values.len() == 1 {
                    let term = Term::from_field_text(self.fields.doc_type, term_values[0]);
                    let tq = TermQuery::new(term, IndexRecordOption::Basic);
                    clauses.push((tantivy::query::Occur::Must, Box::new(tq)));
                } else {
                    let sub: Vec<(tantivy::query::Occur, Box<dyn tantivy::query::Query>)> =
                        term_values
                            .iter()
                            .map(|val| {
                                let term = Term::from_field_text(self.fields.doc_type, val);
                                let tq: Box<dyn tantivy::query::Query> =
                                    Box::new(TermQuery::new(term, IndexRecordOption::Basic));
                                (tantivy::query::Occur::Should, tq)
                            })
                            .collect();
                    clauses.push((tantivy::query::Occur::Must, Box::new(BooleanQuery::new(sub))));
                }
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

        debug!(
            "搜索 '{}' (filter: validity={:?}, doc_type={:?}) 返回 {} 条结果",
            query_str,
            validity,
            doc_type,
            results.len()
        );
        Ok(results)
    }

    /// 获取索引中的文档数量
    pub fn doc_count(&self) -> u64 {
        let searcher = self.reader.searcher();
        searcher.num_docs()
    }

    /// 导出索引中已存储的全部文档。
    ///
    /// 用于构建 AI 知识库快照。content 字段在 schema v2 起为 STORED，
    /// 因此这里可以直接取回正文，不需要重新读取 PDF。
    pub fn all_documents(&self) -> Result<Vec<RegulationDocument>, HuGeError> {
        use tantivy::query::AllQuery;

        let searcher = self.reader.searcher();
        let limit = searcher.num_docs() as usize;
        if limit == 0 {
            return Ok(Vec::new());
        }

        let top_docs = searcher
            .search(&AllQuery, &TopDocs::with_limit(limit))
            .map_err(|e| HuGeError::Internal(format!("导出索引文档失败: {}", e)))?;

        let mut documents = Vec::with_capacity(top_docs.len());
        for (_score, doc_address) in top_docs {
            if let Ok(doc) = searcher.doc::<TantivyDocument>(doc_address) {
                documents.push(RegulationDocument::from_tantivy_doc(&doc, &self.fields));
            }
        }

        Ok(documents)
    }

    /// 删除所有文档（重建索引）
    pub fn clear(&self) -> Result<(), HuGeError> {
        let mut writer = self
            .writer
            .write()
            .map_err(|e| HuGeError::Internal(format!("获取 Writer 锁失败: {}", e)))?;

        writer
            .delete_all_documents()
            .map_err(|e| HuGeError::Internal(format!("删除文档失败: {}", e)))?;

        writer.commit().map_err(|e| HuGeError::Internal(format!("提交失败: {}", e)))?;

        info!("索引已清空");
        Ok(())
    }

    /// 按 URL 批量删除文档（用于强制重建某些文件的 OCR 索引）
    ///
    /// 注意：调用方负责确保 url 与索引中的 STRING 字段精确匹配。
    /// 删除后会调用 `commit()`，使变更立即可见。
    pub fn delete_by_urls(&self, urls: &[String]) -> Result<usize, HuGeError> {
        if urls.is_empty() {
            return Ok(0);
        }
        let mut writer = self
            .writer
            .write()
            .map_err(|e| HuGeError::Internal(format!("获取 Writer 锁失败: {}", e)))?;

        for url in urls {
            let term = tantivy::Term::from_field_text(self.fields.url, url);
            writer.delete_term(term);
        }
        writer.commit().map_err(|e| HuGeError::Internal(format!("提交失败: {}", e)))?;
        info!("从索引删除 {} 个文档", urls.len());
        Ok(urls.len())
    }

    /// 按 URL 批量更新文档的 `file_path` 字段。
    ///
    /// Tantivy 不支持原地更新单字段，必须 `delete_term` + 重新 `add_document`。
    /// 本方法在内部完成"读取整个文档 → 修改 file_path → 重新写入"的循环，
    /// 一次 commit，避免反复刷新读者带来的开销。
    ///
    /// `updates` 为 `(url, new_file_path)` 列表。返回成功更新的文档数。
    /// 找不到对应 url 的条目会被静默跳过。
    pub fn update_file_paths_by_url(
        &self,
        updates: &[(String, String)],
    ) -> Result<usize, HuGeError> {
        if updates.is_empty() {
            return Ok(0);
        }

        use tantivy::query::TermQuery;
        use tantivy::schema::IndexRecordOption;
        use tantivy::TantivyDocument;
        use tantivy::Term;

        let searcher = self.reader.searcher();
        let mut writer = self
            .writer
            .write()
            .map_err(|e| HuGeError::Internal(format!("获取 Writer 锁失败: {}", e)))?;

        let mut updated = 0usize;

        for (url, new_file_path) in updates {
            let term = Term::from_field_text(self.fields.url, url);
            let query = TermQuery::new(term.clone(), IndexRecordOption::Basic);
            let top_docs = searcher
                .search(&query, &TopDocs::with_limit(1))
                .map_err(|e| HuGeError::Internal(format!("查询索引失败: {}", e)))?;

            let Some((_, addr)) = top_docs.first() else {
                debug!("索引中未找到 url={}, 跳过", url);
                continue;
            };

            let tdoc: TantivyDocument = searcher
                .doc(*addr)
                .map_err(|e| HuGeError::Internal(format!("读取索引文档失败: {}", e)))?;
            let mut reg_doc = RegulationDocument::from_tantivy_doc(&tdoc, &self.fields);
            reg_doc.file_path = new_file_path.clone();

            writer.delete_term(term);
            writer
                .add_document(reg_doc.to_tantivy_doc(&self.fields))
                .map_err(|e| HuGeError::Internal(format!("更新索引文档失败: {}", e)))?;

            updated += 1;
        }

        writer.commit().map_err(|e| HuGeError::Internal(format!("提交索引失败: {}", e)))?;
        drop(writer);
        self.reader
            .reload()
            .map_err(|e| HuGeError::Internal(format!("刷新索引读取器失败: {}", e)))?;

        info!("索引已批量更新 {} 个文档的 file_path", updated);
        Ok(updated)
    }

    /// 按文号精确搜索
    ///
    /// 文号是 STRING 类型字段，需要精确匹配（如 "CCAR-121-R7"）
    pub fn search_by_doc_number(
        &self,
        doc_number: &str,
    ) -> Result<Option<RegulationDocument>, HuGeError> {
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

    /// v2 → v3 索引迁移：保留 content，把 url 字段从「仅 STORED」升级到「STRING | STORED」。
    ///
    /// v3 之前 `url` 字段没建倒排索引，导致 `delete_by_urls` / `update_file_paths_by_url`
    /// 这类基于 url 的 `TermQuery` 操作直接报 `Schema error: 'Field "url" is not indexed.'`
    /// （症状是「一键对齐文件名」对未对齐文件失败）。
    ///
    /// v2 起 content 字段是 STORED，所以可以从旧索引读出全部文档（用 `AllQuery`，不依赖
    /// url 字段索引），删旧 segment 后用新 schema 重新写一遍，正文不丢，**不需要用户重做 OCR**。
    ///
    /// 字段在 [`RegulationFields::build_schema`] 中的注册顺序在 v2/v3 之间完全一致，
    /// 因此用新 schema 构建的 `RegulationFields` 上的 Field 编号能正确读取旧 segment。
    fn migrate_v2_to_v3(index_path: &std::path::Path) -> Result<(), HuGeError> {
        use tantivy::query::AllQuery;

        // 1) 用旧 segment 自带的 schema（从 meta.json 加载）打开旧索引
        let old_index = Index::open_in_dir(index_path)
            .map_err(|e| HuGeError::Internal(format!("打开旧索引失败: {}", e)))?;
        let old_reader = old_index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()
            .map_err(|e| HuGeError::Internal(format!("创建旧索引 Reader 失败: {}", e)))?;

        // 2) 用新 schema 的 fields 解码旧 segment：字段顺序与 v2 完全一致，Field 编号匹配
        let (_, fields) = RegulationFields::build_schema();
        let searcher = old_reader.searcher();
        let total = searcher.num_docs() as usize;

        let documents: Vec<RegulationDocument> = if total == 0 {
            Vec::new()
        } else {
            let top_docs = searcher
                .search(&AllQuery, &TopDocs::with_limit(total))
                .map_err(|e| HuGeError::Internal(format!("导出旧索引文档失败: {}", e)))?;
            let mut out = Vec::with_capacity(top_docs.len());
            for (_score, addr) in top_docs {
                if let Ok(doc) = searcher.doc::<TantivyDocument>(addr) {
                    out.push(RegulationDocument::from_tantivy_doc(&doc, &fields));
                }
            }
            out
        };
        info!("v2→v3 迁移：从旧索引导出 {} 篇文档", documents.len());

        // 3) 释放旧索引句柄再删文件（Windows 下不释放无法删除）
        drop(searcher);
        drop(old_reader);
        drop(old_index);
        Self::delete_index_files(index_path)?;

        // 4) 用新 schema 重建索引并把全部文档写回
        let (schema, fields) = RegulationFields::build_schema();
        let new_index = Index::create_in_dir(index_path, schema)
            .map_err(|e| HuGeError::Internal(format!("创建新索引失败: {}", e)))?;
        // 迁移阶段也要注册 jieba：title/office_unit/content 是 TEXT 字段，写入时会分词，
        // 没 jieba 就退化成 SimpleTokenizer，中文搜索会失效到下次进程重启
        Self::register_chinese_tokenizer(&new_index)?;
        {
            let mut writer = new_index
                .writer(INDEX_MEMORY_BUDGET)
                .map_err(|e| HuGeError::Internal(format!("创建迁移 Writer 失败: {}", e)))?;
            for doc in &documents {
                writer
                    .add_document(doc.to_tantivy_doc(&fields))
                    .map_err(|e| HuGeError::Internal(format!("回写迁移文档失败: {}", e)))?;
            }
            writer.commit().map_err(|e| HuGeError::Internal(format!("提交迁移失败: {}", e)))?;
        }
        info!("v2→v3 迁移：已回写 {} 篇文档到新 schema", documents.len());
        Ok(())
    }

    /// 删除索引目录下的所有文件（保留目录本身）
    fn delete_index_files(index_path: &std::path::Path) -> Result<(), HuGeError> {
        let entries = std::fs::read_dir(index_path)
            .map_err(|e| HuGeError::Internal(format!("读取索引目录失败: {}", e)))?;

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

        Self { tokens, index: 0, offset: 0, token: tantivy::tokenizer::Token::default() }
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

    #[test]
    fn upsert_replaces_metadata_only_document_with_content() {
        let temp_dir = TempDir::new().unwrap();
        let index = RegulationIndex::open_or_create(temp_dir.path().to_path_buf()).unwrap();

        let metadata_doc = RegulationDocument {
            title: "飞行检查员工作手册".to_string(),
            doc_number: "AP-TEST".to_string(),
            validity: "有效".to_string(),
            doc_type: "normative".to_string(),
            office_unit: String::new(),
            sign_date: String::new(),
            publish_date: String::new(),
            url: "file:///D:/docs/checker.pdf".to_string(),
            file_path: "D:\\docs\\checker.pdf".to_string(),
            content: String::new(),
        };

        index.add_document(&metadata_doc).unwrap();
        index.commit().unwrap();

        let filename_results = index.search("检查员", 10).unwrap();
        assert_eq!(filename_results.len(), 1);
        assert_eq!(filename_results[0].url, metadata_doc.url);
        assert!(filename_results[0].content.is_empty());

        let mut content_doc = metadata_doc;
        content_doc.content = "检查员应当按照手册完成监督检查".to_string();
        index.upsert_document(&content_doc).unwrap();
        index.commit().unwrap();

        let results = index.search("监督检查", 10).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, content_doc.url);
        assert!(results[0].content.contains("监督检查"));
    }
}
