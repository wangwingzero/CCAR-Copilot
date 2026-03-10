//! 规章文档 Schema 定义
//!
//! 定义 Tantivy 索引的字段结构和文档数据模型。

use serde::{Deserialize, Serialize};
use tantivy::schema::{Schema, STORED, STRING, TEXT, Field};

/// 规章文档数据模型
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegulationDocument {
    /// 文档标题
    pub title: String,
    /// 文号（如 CCAR-121-R7）
    pub doc_number: String,
    /// 有效性：有效、失效、废止
    pub validity: String,
    /// 文档类型：regulation（规章）、normative（规范性文件）
    pub doc_type: String,
    /// 发布单位
    pub office_unit: String,
    /// 签发日期（YYYY-MM-DD）
    pub sign_date: String,
    /// 发布日期（YYYY-MM-DD）
    pub publish_date: String,
    /// 原始 URL
    pub url: String,
    /// 本地文件路径
    pub file_path: String,
    /// PDF 正文内容（用于全文搜索）
    #[serde(default)]
    pub content: String,
}

/// Tantivy Schema 字段引用
#[derive(Clone)]
pub struct RegulationFields {
    pub title: Field,
    pub doc_number: Field,
    pub validity: Field,
    pub doc_type: Field,
    pub office_unit: Field,
    pub publish_date: Field,
    pub url: Field,
    pub file_path: Field,
    pub content: Field,
}

impl RegulationFields {
    /// 构建 Tantivy Schema
    pub fn build_schema() -> (Schema, Self) {
        let mut schema_builder = Schema::builder();

        // 标题：全文索引 + 存储（用于显示）
        let title = schema_builder.add_text_field("title", TEXT | STORED);

        // 文号：精确匹配 + 存储
        let doc_number = schema_builder.add_text_field("doc_number", STRING | STORED);

        // 有效性：精确匹配 + 存储（用于筛选）
        let validity = schema_builder.add_text_field("validity", STRING | STORED);

        // 文档类型：精确匹配 + 存储
        let doc_type = schema_builder.add_text_field("doc_type", STRING | STORED);

        // 发布单位：全文索引 + 存储
        let office_unit = schema_builder.add_text_field("office_unit", TEXT | STORED);

        // 发布日期：存储（用于排序和显示）
        let publish_date = schema_builder.add_text_field("publish_date", STRING | STORED);

        // 原始 URL：仅存储
        let url = schema_builder.add_text_field("url", STORED);

        // 本地文件路径：仅存储
        let file_path = schema_builder.add_text_field("file_path", STORED);

        // PDF 正文：全文索引 + 存储（用于生成搜索摘要）
        let content = schema_builder.add_text_field("content", TEXT | STORED);

        let schema = schema_builder.build();
        let fields = Self {
            title,
            doc_number,
            validity,
            doc_type,
            office_unit,
            publish_date,
            url,
            file_path,
            content,
        };

        (schema, fields)
    }
}

impl RegulationDocument {
    /// 从 Tantivy 文档转换
    pub fn from_tantivy_doc(doc: &tantivy::TantivyDocument, fields: &RegulationFields) -> Self {
        use tantivy::schema::Value;
        
        let get_text = |field: Field| -> String {
            doc.get_first(field)
                .and_then(|v| Value::as_str(&v))
                .unwrap_or("")
                .to_string()
        };

        Self {
            title: get_text(fields.title),
            doc_number: get_text(fields.doc_number),
            validity: get_text(fields.validity),
            doc_type: get_text(fields.doc_type),
            office_unit: get_text(fields.office_unit),
            sign_date: String::new(), // 不存储签发日期
            publish_date: get_text(fields.publish_date),
            url: get_text(fields.url),
            file_path: get_text(fields.file_path),
            content: get_text(fields.content), // 从索引中取回正文
        }
    }

    /// 转换为 Tantivy 文档
    pub fn to_tantivy_doc(&self, fields: &RegulationFields) -> tantivy::TantivyDocument {
        let mut doc = tantivy::TantivyDocument::new();

        doc.add_text(fields.title, &self.title);
        doc.add_text(fields.doc_number, &self.doc_number);
        doc.add_text(fields.validity, &self.validity);
        doc.add_text(fields.doc_type, &self.doc_type);
        doc.add_text(fields.office_unit, &self.office_unit);
        doc.add_text(fields.publish_date, &self.publish_date);
        doc.add_text(fields.url, &self.url);
        doc.add_text(fields.file_path, &self.file_path);
        doc.add_text(fields.content, &self.content);

        doc
    }
}
