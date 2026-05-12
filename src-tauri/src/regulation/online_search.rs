//! CAAC 官网在线搜索模块
//!
//! 直接使用 Rust (reqwest + scraper) 替代 Python Sidecar 实现在线规章搜索。
//! 数据来源：中国民用航空局官网 (https://www.caac.gov.cn)

use regex::Regex;
use reqwest::Client;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
use std::time::Duration;
use tracing::{debug, info, warn};
use urlencoding::encode;

use crate::error::{HuGeError, HuGeResult};

// CAAC 官网配置
const BASE_URL: &str = "https://www.caac.gov.cn";
const WAS5_SEARCH_URL: &str = "https://www.caac.gov.cn/was5/web/search";
const STATIC_DATA_BASE_URL: &str = "https://flighttoolbox.hudawang.cn/data/v1";

// 频道 ID
const REGULATION_CHANNEL: &str = "269689"; // 民航规章频道
const NORMATIVE_CHANNEL: &str = "238066"; // 规范性文件频道
const STANDARD_CHANNEL: &str = "269689"; // 标准规范频道（与民航规章共用频道，不同 fl 分类）

// 分类 ID (fl 参数)
const REGULATION_FL: &str = "13"; // 民航规章分类
const NORMATIVE_FL: &str = "14"; // 规范性文件分类
const STANDARD_FL: &str = "15"; // 标准规范分类

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

// 共享的日期正则（parse_regulation_page 和 parse_normative_page 均使用）
static DATE_FROM_URL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"/t(\d{4})(\d{2})(\d{2})_").expect("date_from_url regex pattern invalid")
});
static DATE_NORMALIZE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(\d{4})年(\d{1,2})月(\d{1,2})日").expect("date_normalize regex pattern invalid")
});

/// 在线搜索请求
#[derive(Debug, Deserialize)]
pub struct OnlineSearchRequest {
    pub keyword: String,
    #[serde(default = "default_all")]
    pub doc_type: String,
    #[serde(default = "default_all")]
    pub validity: String,
    #[serde(default)]
    pub start_date: String,
    #[serde(default)]
    pub end_date: String,
}

fn default_all() -> String {
    "all".to_string()
}

/// 在线搜索结果文档
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnlineDocument {
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub validity: String,
    #[serde(default)]
    pub doc_number: String,
    #[serde(default)]
    pub office_unit: String,
    #[serde(default)]
    pub doc_type: String,
    #[serde(default)]
    pub publish_date: String,
    #[serde(default)]
    pub sign_date: String,
    #[serde(default)]
    pub pdf_url: String,
}

/// 在线搜索响应
#[derive(Debug, Serialize)]
pub struct OnlineSearchResponse {
    pub documents: Vec<OnlineDocument>,
    pub total: usize,
    pub elapsed_ms: u64,
}

/// CAAC 在线搜索器
pub struct CaacOnlineSearcher {
    client: Client,
}

impl CaacOnlineSearcher {
    /// 创建新的搜索器
    pub fn new() -> HuGeResult<Self> {
        let client = Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(30))
            .default_headers({
                let mut headers = reqwest::header::HeaderMap::new();
                headers.insert(
                    "Accept",
                    "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8"
                        .parse()
                        .expect("Invalid Accept header value"),
                );
                headers.insert(
                    "Accept-Language",
                    "zh-CN,zh;q=0.9,en;q=0.8"
                        .parse()
                        .expect("Invalid Accept-Language header value"),
                );
                headers.insert(
                    "Referer",
                    "https://www.caac.gov.cn/XXGK/XXGK/"
                        .parse()
                        .expect("Invalid Referer header value"),
                );
                headers
            })
            .build()
            .map_err(|e| HuGeError::Internal(format!("创建 HTTP 客户端失败: {}", e)))?;

        Ok(Self { client })
    }

    /// 搜索规章（CCAR + 规范性文件）
    pub async fn search(&self, request: &OnlineSearchRequest) -> HuGeResult<OnlineSearchResponse> {
        let start = std::time::Instant::now();
        let mut documents = Vec::new();

        // 构建日期参数
        let mut date_params = String::new();
        if !request.start_date.is_empty() {
            date_params.push_str(&format!("&fwrq1={}", request.start_date));
        }
        if !request.end_date.is_empty() {
            date_params.push_str(&format!("&fwrq2={}", request.end_date));
        }

        // 搜索 CCAR 规章
        if request.doc_type == "all" || request.doc_type == "regulation" {
            match self
                .search_channel(
                    &request.keyword,
                    REGULATION_CHANNEL,
                    REGULATION_FL,
                    &date_params,
                    "regulation",
                )
                .await
            {
                Ok(regs) => {
                    info!("CCAR 规章搜索完成，找到 {} 条", regs.len());
                    documents.extend(regs);
                }
                Err(e) => warn!("CCAR 规章搜索失败: {}", e),
            }
        }

        // 搜索规范性文件
        if request.doc_type == "all" || request.doc_type == "normative" {
            match self
                .search_channel(
                    &request.keyword,
                    NORMATIVE_CHANNEL,
                    NORMATIVE_FL,
                    &date_params,
                    "normative",
                )
                .await
            {
                Ok(norms) => {
                    info!("规范性文件搜索完成，找到 {} 条", norms.len());
                    documents.extend(norms);
                }
                Err(e) => warn!("规范性文件搜索失败: {}", e),
            }
        }

        // 搜索标准规范
        if request.doc_type == "all" || request.doc_type == "standard" {
            match self
                .search_channel(
                    &request.keyword,
                    STANDARD_CHANNEL,
                    STANDARD_FL,
                    &date_params,
                    "standard",
                )
                .await
            {
                Ok(stds) => {
                    info!("标准规范搜索完成，找到 {} 条", stds.len());
                    documents.extend(stds);
                }
                Err(e) => warn!("标准规范搜索失败: {}", e),
            }
        }

        // 根据有效性筛选
        if request.validity == "valid" {
            documents.retain(|d| d.validity == "有效");
        } else if request.validity == "invalid" {
            documents.retain(|d| d.validity == "失效" || d.validity == "废止");
        }

        let total = documents.len();
        let elapsed_ms = start.elapsed().as_millis() as u64;

        info!("在线搜索完成: {} 条结果，耗时 {}ms", total, elapsed_ms);

        Ok(OnlineSearchResponse { documents, total, elapsed_ms })
    }

    /// 全量分页爬取规章列表（无关键词）
    pub async fn fetch_all(
        &self,
        doc_type: &str,
        max_pages: usize,
    ) -> HuGeResult<OnlineSearchResponse> {
        let start = std::time::Instant::now();
        let mut documents = Vec::new();
        let capped_pages = max_pages.max(1);

        if doc_type == "all" || doc_type == "regulation" {
            let regs = self
                .fetch_channel_pages(REGULATION_CHANNEL, REGULATION_FL, "regulation", capped_pages)
                .await?;
            documents.extend(regs);
        }

        if doc_type == "all" || doc_type == "normative" {
            let norms = self
                .fetch_channel_pages(NORMATIVE_CHANNEL, NORMATIVE_FL, "normative", capped_pages)
                .await?;
            documents.extend(norms);
        }

        if doc_type == "all" || doc_type == "standard" {
            let stds = self
                .fetch_channel_pages(STANDARD_CHANNEL, STANDARD_FL, "standard", capped_pages)
                .await?;
            documents.extend(stds);
        }

        let total = documents.len();
        let elapsed_ms = start.elapsed().as_millis() as u64;
        info!("在线全量爬取完成: {} 条结果，耗时 {}ms", total, elapsed_ms);

        Ok(OnlineSearchResponse { documents, total, elapsed_ms })
    }

    /// 从虎哥服务器静态 JSON 获取全量局方清单。
    ///
    /// 该数据由服务器每日同步 CAAC 后生成，包含已解析的 `pdf_url`、有效性、
    /// 发布单位和日期。用于“对比更新”时比实时分页爬局方官网更快、更稳。
    pub async fn fetch_all_static(&self, doc_type: &str) -> HuGeResult<OnlineSearchResponse> {
        let start = std::time::Instant::now();
        let manifest_url = format!("{}/manifest.json", STATIC_DATA_BASE_URL);
        let manifest: serde_json::Value = self
            .client
            .get(&manifest_url)
            .send()
            .await
            .map_err(|e| HuGeError::Internal(format!("读取静态清单失败: {}", e)))?
            .error_for_status()
            .map_err(|e| HuGeError::Internal(format!("静态清单 HTTP 错误: {}", e)))?
            .json()
            .await
            .map_err(|e| HuGeError::Internal(format!("解析静态清单失败: {}", e)))?;

        let last_updated =
            manifest.get("lastUpdated").and_then(|v| v.as_str()).unwrap_or("unknown");

        let mut documents = Vec::new();
        if doc_type == "all" || doc_type == "regulation" {
            documents.extend(self.fetch_static_file("regulation.json", "regulation").await?);
        }
        if doc_type == "all" || doc_type == "normative" {
            documents.extend(self.fetch_static_file("normative.json", "normative").await?);
        }
        if doc_type == "all" || doc_type == "standard" {
            documents.extend(self.fetch_static_file("specification.json", "standard").await?);
        }

        documents.retain(|doc| !doc.title.trim().is_empty() && !doc.url.trim().is_empty());
        let total = documents.len();
        let elapsed_ms = start.elapsed().as_millis() as u64;
        info!(
            "静态 JSON 全量读取完成: {} 条, lastUpdated={}, 耗时 {}ms",
            total, last_updated, elapsed_ms
        );

        Ok(OnlineSearchResponse { documents, total, elapsed_ms })
    }

    async fn fetch_static_file(
        &self,
        filename: &str,
        fallback_doc_type: &str,
    ) -> HuGeResult<Vec<OnlineDocument>> {
        let url = format!("{}/{}", STATIC_DATA_BASE_URL, filename);
        let mut documents: Vec<OnlineDocument> = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| HuGeError::Internal(format!("读取静态数据失败 {}: {}", filename, e)))?
            .error_for_status()
            .map_err(|e| HuGeError::Internal(format!("静态数据 HTTP 错误 {}: {}", filename, e)))?
            .json()
            .await
            .map_err(|e| HuGeError::Internal(format!("解析静态数据失败 {}: {}", filename, e)))?;

        for doc in &mut documents {
            normalize_static_document(doc, fallback_doc_type);
        }

        Ok(documents)
    }

    /// 搜索单个频道
    async fn search_channel(
        &self,
        keyword: &str,
        channel_id: &str,
        fl: &str,
        date_params: &str,
        doc_type: &str,
    ) -> HuGeResult<Vec<OnlineDocument>> {
        let search_url = if keyword.is_empty() {
            format!(
                "{}?channelid={}&perpage=100&orderby=-fabuDate&fl={}{}",
                WAS5_SEARCH_URL, channel_id, fl, date_params
            )
        } else {
            format!(
                "{}?channelid={}&sw={}&perpage=100&orderby=-fabuDate&fl={}{}",
                WAS5_SEARCH_URL,
                channel_id,
                encode(keyword),
                fl,
                date_params
            )
        };

        info!("搜索 URL: {}", search_url);

        let response = self
            .client
            .get(&search_url)
            .send()
            .await
            .map_err(|e| HuGeError::Internal(format!("HTTP 请求失败: {}", e)))?;

        if !response.status().is_success() {
            return Err(HuGeError::Internal(format!("HTTP 状态码: {}", response.status())));
        }

        let html_content = response
            .text()
            .await
            .map_err(|e| HuGeError::Internal(format!("读取响应失败: {}", e)))?;

        if html_content.is_empty() {
            return Ok(Vec::new());
        }

        // 根据文档类型选择解析方式
        if doc_type == "regulation" {
            parse_regulation_page(&html_content)
        } else {
            parse_normative_page(&html_content, doc_type)
        }
    }

    async fn fetch_channel_pages(
        &self,
        channel_id: &str,
        fl: &str,
        doc_type: &str,
        max_pages: usize,
    ) -> HuGeResult<Vec<OnlineDocument>> {
        let mut all_docs = Vec::new();
        let per_page = 100;

        for page in 1..=max_pages {
            let search_url = format!(
                "{}?channelid={}&perpage={}&page={}&orderby=-fabuDate&fl={}",
                WAS5_SEARCH_URL, channel_id, per_page, page, fl
            );
            info!("全量爬取 {} 第 {} 页: {}", doc_type, page, search_url);

            let response = self
                .client
                .get(&search_url)
                .send()
                .await
                .map_err(|e| HuGeError::Internal(format!("HTTP 请求失败: {}", e)))?;

            if !response.status().is_success() {
                break;
            }

            let html_content = response
                .text()
                .await
                .map_err(|e| HuGeError::Internal(format!("读取响应失败: {}", e)))?;

            if html_content.is_empty() {
                break;
            }

            let page_docs = if doc_type == "regulation" {
                parse_regulation_page(&html_content)?
            } else {
                parse_normative_page(&html_content, doc_type)?
            };

            if page_docs.is_empty() {
                break;
            }

            let page_count = page_docs.len();
            all_docs.extend(page_docs);

            if page_count < per_page {
                break;
            }
        }

        Ok(all_docs)
    }
}

fn normalize_static_document(doc: &mut OnlineDocument, fallback_doc_type: &str) {
    doc.doc_type = normalize_doc_type(&doc.doc_type, fallback_doc_type);

    if let Some(date) = normalize_date(&doc.publish_date, &DATE_NORMALIZE_RE) {
        doc.publish_date = date;
    }
    if let Some(date) = normalize_date(&doc.sign_date, &DATE_NORMALIZE_RE) {
        doc.sign_date = date;
    }
}

fn normalize_doc_type(raw: &str, fallback_doc_type: &str) -> String {
    let value = raw.trim();
    if value.eq_ignore_ascii_case("regulation")
        || value.eq_ignore_ascii_case("normative")
        || value.eq_ignore_ascii_case("standard")
    {
        return value.to_ascii_lowercase();
    }

    if value.contains("标准") {
        "standard".to_string()
    } else if value.contains("规范") {
        "normative".to_string()
    } else if value.contains("规章") || value.contains("CCAR") {
        "regulation".to_string()
    } else {
        fallback_doc_type.to_string()
    }
}

/// 解析 CCAR 规章搜索结果页面
fn parse_regulation_page(html_content: &str) -> HuGeResult<Vec<OnlineDocument>> {
    use std::sync::LazyLock;

    // 缓存编译后的选择器和正则，避免每次调用重复编译
    static TABLE_SELECTOR: LazyLock<Selector> =
        LazyLock::new(|| Selector::parse("table.t_table").unwrap());
    static ANY_TABLE_SELECTOR: LazyLock<Selector> =
        LazyLock::new(|| Selector::parse("table").unwrap());
    static TR_SELECTOR: LazyLock<Selector> = LazyLock::new(|| Selector::parse("tr").unwrap());
    static TD_SELECTOR: LazyLock<Selector> = LazyLock::new(|| Selector::parse("td").unwrap());
    static A_SELECTOR: LazyLock<Selector> = LazyLock::new(|| Selector::parse("a[href]").unwrap());
    static TITLE_CELL_SELECTOR: LazyLock<Selector> =
        LazyLock::new(|| Selector::parse("td.t_l").unwrap());
    static DETAIL_SELECTOR: LazyLock<Selector> =
        LazyLock::new(|| Selector::parse("div.t_l_content li").unwrap());

    let mut documents = Vec::new();
    let document = Html::parse_document(html_content);

    // 查找搜索结果表格
    let table = document.select(&TABLE_SELECTOR).next();

    let table = match table {
        Some(t) => t,
        None => {
            // 尝试找任意 table
            match document.select(&ANY_TABLE_SELECTOR).next() {
                Some(t) => t,
                None => return Ok(documents),
            }
        }
    };

    // 获取表格行
    for row in table.select(&TR_SELECTOR) {
        let cells: Vec<_> = row.select(&TD_SELECTOR).collect();
        if cells.len() < 4 {
            continue;
        }

        // 查找标题单元格
        let title_cell = row.select(&TITLE_CELL_SELECTOR).next().or_else(|| cells.get(1).copied());

        let title_cell = match title_cell {
            Some(c) => c,
            None => continue,
        };

        // 查找链接
        let link = match title_cell.select(&A_SELECTOR).next() {
            Some(a) => a,
            None => continue,
        };

        let title = link.text().collect::<String>().trim().to_string();
        if title.is_empty() {
            continue;
        }

        let href = link.value().attr("href").unwrap_or("");
        let full_url = build_full_url(href);

        // 文号
        let doc_number = cells
            .get(2)
            .map(|c| c.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        // 有效性
        let mut validity = cells
            .get(3)
            .map(|c| c.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        // 发布日期（从 URL 提取）
        let mut publish_date = extract_date_from_url(&full_url, &DATE_FROM_URL_RE);

        // 发布单位（从详情 div 提取）
        let mut office_unit = String::new();
        for li in title_cell.select(&DETAIL_SELECTOR) {
            let li_text = li.text().collect::<String>().trim().to_string();
            if li_text.contains("办文单位：") {
                office_unit = li_text.replace("办文单位：", "").trim().to_string();
            } else if li_text.contains("发文日期") {
                let date_text =
                    li_text.replace("发文日期：", "").replace("发文日期:", "").trim().to_string();
                if let Some(normalized) = normalize_date(&date_text, &DATE_NORMALIZE_RE) {
                    publish_date = normalized;
                }
            } else if li_text.contains("有效性") && validity.is_empty() {
                validity = li_text
                    .replace("有效性：", "")
                    .replace("有效性:", "")
                    .replace("有 效 性：", "")
                    .replace("有 效 性:", "")
                    .trim()
                    .to_string();
            }
        }

        documents.push(OnlineDocument {
            title,
            url: full_url,
            validity,
            doc_number,
            office_unit,
            doc_type: "regulation".to_string(),
            publish_date,
            sign_date: String::new(),
            pdf_url: String::new(),
        });
    }

    debug!("解析规章页面完成，找到 {} 条结果", documents.len());
    Ok(documents)
}

/// 解析规范性文件/标准规范搜索结果页面
fn parse_normative_page(html_content: &str, doc_type: &str) -> HuGeResult<Vec<OnlineDocument>> {
    // 缓存 CSS 选择器和正则（避免每次调用重新编译）
    static NORM_TABLE_SELECTOR: LazyLock<Selector> =
        LazyLock::new(|| Selector::parse("table.t_table").unwrap());
    static NORM_ANY_TABLE: LazyLock<Selector> = LazyLock::new(|| Selector::parse("table").unwrap());
    static NORM_TR: LazyLock<Selector> = LazyLock::new(|| Selector::parse("tr").unwrap());
    static NORM_TD: LazyLock<Selector> = LazyLock::new(|| Selector::parse("td").unwrap());
    static NORM_A: LazyLock<Selector> = LazyLock::new(|| Selector::parse("a[href]").unwrap());
    static NORM_TITLE_CELL: LazyLock<Selector> =
        LazyLock::new(|| Selector::parse("td.tdMC").unwrap());
    static NORM_DOC_NUMBER: LazyLock<Selector> =
        LazyLock::new(|| Selector::parse("td.strFL").unwrap());
    static NORM_VALIDITY: LazyLock<Selector> =
        LazyLock::new(|| Selector::parse("td.strGF").unwrap());
    static NORM_DATE: LazyLock<Selector> = LazyLock::new(|| Selector::parse("td.tdRQ").unwrap());
    static NORM_UNIT: LazyLock<Selector> =
        LazyLock::new(|| Selector::parse("div.t_l_content li.t_l_content_left").unwrap());

    let mut documents = Vec::new();
    let document = Html::parse_document(html_content);

    // 查找搜索结果表格
    let table = document.select(&NORM_TABLE_SELECTOR).next();

    let table = match table {
        Some(t) => t,
        None => match document.select(&NORM_ANY_TABLE).next() {
            Some(t) => t,
            None => return Ok(documents),
        },
    };

    for row in table.select(&NORM_TR) {
        let cells: Vec<_> = row.select(&NORM_TD).collect();
        if cells.len() < 4 {
            continue;
        }

        // 标题单元格
        let title_cell = row.select(&NORM_TITLE_CELL).next().or_else(|| cells.get(1).copied());

        let title_cell = match title_cell {
            Some(c) => c,
            None => continue,
        };

        // 链接
        let link = match title_cell.select(&NORM_A).next() {
            Some(a) => a,
            None => continue,
        };

        let title = link.text().collect::<String>().trim().to_string();
        if title.is_empty() {
            continue;
        }

        let href = link.value().attr("href").unwrap_or("");
        let full_url = build_full_url(href);

        // 文号
        let doc_number = row
            .select(&NORM_DOC_NUMBER)
            .next()
            .map(|c| c.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        // 有效性
        let validity = row
            .select(&NORM_VALIDITY)
            .next()
            .map(|c| c.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        // 日期
        let date_cells: Vec<_> = row.select(&NORM_DATE).collect();
        let sign_date = date_cells
            .first()
            .and_then(|c| normalize_date(c.text().collect::<String>().trim(), &DATE_NORMALIZE_RE))
            .unwrap_or_default();
        let publish_date = date_cells
            .get(1)
            .and_then(|c| normalize_date(c.text().collect::<String>().trim(), &DATE_NORMALIZE_RE))
            .unwrap_or_default();

        // 发布单位
        let office_unit = title_cell
            .select(&NORM_UNIT)
            .next()
            .map(|c| {
                let text = c.text().collect::<String>().trim().to_string();
                text.replace("办文单位：", "").trim().to_string()
            })
            .unwrap_or_default();

        documents.push(OnlineDocument {
            title,
            url: full_url,
            validity,
            doc_number,
            office_unit,
            doc_type: doc_type.to_string(),
            publish_date,
            sign_date,
            pdf_url: String::new(),
        });
    }

    debug!("解析规范性文件页面完成，找到 {} 条结果", documents.len());
    Ok(documents)
}

/// 构建完整 URL
fn build_full_url(href: &str) -> String {
    if href.starts_with("http") {
        href.to_string()
    } else if href.starts_with('/') {
        format!("{}{}", BASE_URL, href)
    } else {
        format!("{}/{}", BASE_URL, href)
    }
}

/// 从 URL 提取日期
fn extract_date_from_url(url: &str, re: &Regex) -> String {
    if let Some(caps) = re.captures(url) {
        format!("{}-{}-{}", &caps[1], &caps[2], &caps[3])
    } else {
        String::new()
    }
}

/// YYYY-MM-DD 日期格式正则（静态编译，避免每次调用重新创建）
static DATE_ISO_RE: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
    Regex::new(r"^\d{4}-\d{2}-\d{2}$").expect("DATE_ISO_RE regex pattern invalid")
});

/// 标准化日期格式
fn normalize_date(date_str: &str, re: &Regex) -> Option<String> {
    if date_str.is_empty() {
        return None;
    }

    if let Some(caps) = re.captures(date_str) {
        let year = &caps[1];
        let month: u32 = caps[2].parse().unwrap_or(1);
        let day: u32 = caps[3].parse().unwrap_or(1);
        return Some(format!("{}-{:02}-{:02}", year, month, day));
    }

    // 已经是 YYYY-MM-DD 格式
    if DATE_ISO_RE.is_match(date_str) {
        return Some(date_str.to_string());
    }

    None
}
