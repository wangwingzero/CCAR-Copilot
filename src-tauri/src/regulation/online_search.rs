//! CAAC 官网在线搜索模块
//!
//! 直接使用 Rust (reqwest + scraper) 替代 Python Sidecar 实现在线规章搜索。
//! 数据来源：中国民用航空局官网 (https://www.caac.gov.cn)

use regex::Regex;
use reqwest::Client;
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, info, warn};
use urlencoding::encode;

use crate::error::{HuGeError, HuGeResult};

// CAAC 官网配置
const BASE_URL: &str = "https://www.caac.gov.cn";
const WAS5_SEARCH_URL: &str = "https://www.caac.gov.cn/was5/web/search";

// 频道 ID
const REGULATION_CHANNEL: &str = "269689"; // 民航规章频道
const NORMATIVE_CHANNEL: &str = "238066";  // 规范性文件频道
const STANDARD_CHANNEL: &str = "269689";   // 标准规范频道（与民航规章共用频道，不同 fl 分类）

// 分类 ID (fl 参数)
const REGULATION_FL: &str = "13";  // 民航规章分类
const NORMATIVE_FL: &str = "14";   // 规范性文件分类
const STANDARD_FL: &str = "15";    // 标准规范分类

const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";

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
    pub title: String,
    pub url: String,
    pub validity: String,
    pub doc_number: String,
    pub office_unit: String,
    pub doc_type: String,
    pub publish_date: String,
    pub sign_date: String,
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
                headers.insert("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8".parse().expect("Invalid Accept header value"));
                headers.insert("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8".parse().expect("Invalid Accept-Language header value"));
                headers.insert("Referer", "https://www.caac.gov.cn/XXGK/XXGK/".parse().expect("Invalid Referer header value"));
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
            match self.search_channel(&request.keyword, REGULATION_CHANNEL, REGULATION_FL, &date_params, "regulation").await {
                Ok(regs) => {
                    info!("CCAR 规章搜索完成，找到 {} 条", regs.len());
                    documents.extend(regs);
                }
                Err(e) => warn!("CCAR 规章搜索失败: {}", e),
            }
        }

        // 搜索规范性文件
        if request.doc_type == "all" || request.doc_type == "normative" {
            match self.search_channel(&request.keyword, NORMATIVE_CHANNEL, NORMATIVE_FL, &date_params, "normative").await {
                Ok(norms) => {
                    info!("规范性文件搜索完成，找到 {} 条", norms.len());
                    documents.extend(norms);
                }
                Err(e) => warn!("规范性文件搜索失败: {}", e),
            }
        }

        // 搜索标准规范
        if request.doc_type == "all" || request.doc_type == "standard" {
            match self.search_channel(&request.keyword, STANDARD_CHANNEL, STANDARD_FL, &date_params, "standard").await {
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

        Ok(OnlineSearchResponse {
            documents,
            total,
            elapsed_ms,
        })
    }

    /// 全量分页爬取规章列表（无关键词）
    pub async fn fetch_all(&self, doc_type: &str, max_pages: usize) -> HuGeResult<OnlineSearchResponse> {
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

        Ok(OnlineSearchResponse {
            documents,
            total,
            elapsed_ms,
        })
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
                WAS5_SEARCH_URL, channel_id, encode(keyword), fl, date_params
            )
        };

        info!("搜索 URL: {}", search_url);

        let response = self.client.get(&search_url)
            .send()
            .await
            .map_err(|e| HuGeError::Internal(format!("HTTP 请求失败: {}", e)))?;

        if !response.status().is_success() {
            return Err(HuGeError::Internal(format!("HTTP 状态码: {}", response.status())));
        }

        let html_content = response.text().await
            .map_err(|e| HuGeError::Internal(format!("读取响应失败: {}", e)))?;

        if html_content.is_empty() {
            return Ok(Vec::new());
        }

        // 根据文档类型选择解析方式
        if doc_type == "regulation" {
            parse_regulation_page(&html_content)
        } else {
            parse_normative_page(&html_content)
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

            let response = self.client.get(&search_url).send().await.map_err(|e| {
                HuGeError::Internal(format!("HTTP 请求失败: {}", e))
            })?;

            if !response.status().is_success() {
                break;
            }

            let html_content = response.text().await.map_err(|e| {
                HuGeError::Internal(format!("读取响应失败: {}", e))
            })?;

            if html_content.is_empty() {
                break;
            }

            let page_docs = if doc_type == "regulation" {
                parse_regulation_page(&html_content)?
            } else {
                parse_normative_page(&html_content)?
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

/// 解析 CCAR 规章搜索结果页面
fn parse_regulation_page(html_content: &str) -> HuGeResult<Vec<OnlineDocument>> {
    let mut documents = Vec::new();
    let document = Html::parse_document(html_content);

    // 查找搜索结果表格
    let table_selector = Selector::parse("table.t_table").unwrap();
    let table = document.select(&table_selector).next();

    let table = match table {
        Some(t) => t,
        None => {
            // 尝试找任意 table
            let any_table = Selector::parse("table").unwrap();
            match document.select(&any_table).next() {
                Some(t) => t,
                None => return Ok(documents),
            }
        }
    };

    // 获取表格行
    let tr_selector = Selector::parse("tr").unwrap();
    let td_selector = Selector::parse("td").unwrap();
    let a_selector = Selector::parse("a[href]").unwrap();
    let title_cell_selector = Selector::parse("td.t_l").unwrap();
    let detail_selector = Selector::parse("div.t_l_content li").unwrap();

    let date_from_url_re = Regex::new(r"/t(\d{4})(\d{2})(\d{2})_")
        .expect("date_from_url regex pattern invalid");
    let date_normalize_re = Regex::new(r"(\d{4})年(\d{1,2})月(\d{1,2})日")
        .expect("date_normalize regex pattern invalid");

    for row in table.select(&tr_selector) {
        let cells: Vec<_> = row.select(&td_selector).collect();
        if cells.len() < 4 {
            continue;
        }

        // 查找标题单元格
        let title_cell = row.select(&title_cell_selector).next()
            .or_else(|| cells.get(1).copied());

        let title_cell = match title_cell {
            Some(c) => c,
            None => continue,
        };

        // 查找链接
        let link = match title_cell.select(&a_selector).next() {
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
        let doc_number = cells.get(2)
            .map(|c| c.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        // 有效性
        let mut validity = cells.get(3)
            .map(|c| c.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        // 发布日期（从 URL 提取）
        let mut publish_date = extract_date_from_url(&full_url, &date_from_url_re);

        // 发布单位（从详情 div 提取）
        let mut office_unit = String::new();
        for li in title_cell.select(&detail_selector) {
            let li_text = li.text().collect::<String>().trim().to_string();
            if li_text.contains("办文单位：") {
                office_unit = li_text.replace("办文单位：", "").trim().to_string();
            } else if li_text.contains("发文日期") {
                let date_text = li_text
                    .replace("发文日期：", "")
                    .replace("发文日期:", "")
                    .trim()
                    .to_string();
                if let Some(normalized) = normalize_date(&date_text, &date_normalize_re) {
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

/// 解析规范性文件搜索结果页面
fn parse_normative_page(html_content: &str) -> HuGeResult<Vec<OnlineDocument>> {
    let mut documents = Vec::new();
    let document = Html::parse_document(html_content);

    // 查找搜索结果表格
    let table_selector = Selector::parse("table.t_table").unwrap();
    let table = document.select(&table_selector).next();

    let table = match table {
        Some(t) => t,
        None => {
            let any_table = Selector::parse("table").unwrap();
            match document.select(&any_table).next() {
                Some(t) => t,
                None => return Ok(documents),
            }
        }
    };

    let tr_selector = Selector::parse("tr").unwrap();
    let td_selector = Selector::parse("td").unwrap();
    let a_selector = Selector::parse("a[href]").unwrap();
    let title_cell_selector = Selector::parse("td.tdMC").unwrap();
    let doc_number_selector = Selector::parse("td.strFL").unwrap();
    let validity_selector = Selector::parse("td.strGF").unwrap();
    let date_selector = Selector::parse("td.tdRQ").unwrap();
    let unit_selector = Selector::parse("div.t_l_content li.t_l_content_left").unwrap();

    let date_normalize_re = Regex::new(r"(\d{4})年(\d{1,2})月(\d{1,2})日")
        .expect("date_normalize regex pattern invalid");

    for row in table.select(&tr_selector) {
        let cells: Vec<_> = row.select(&td_selector).collect();
        if cells.len() < 4 {
            continue;
        }

        // 标题单元格
        let title_cell = row.select(&title_cell_selector).next()
            .or_else(|| cells.get(1).copied());

        let title_cell = match title_cell {
            Some(c) => c,
            None => continue,
        };

        // 链接
        let link = match title_cell.select(&a_selector).next() {
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
        let doc_number = row.select(&doc_number_selector).next()
            .map(|c| c.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        // 有效性
        let validity = row.select(&validity_selector).next()
            .map(|c| c.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        // 日期
        let date_cells: Vec<_> = row.select(&date_selector).collect();
        let sign_date = date_cells.first()
            .and_then(|c| normalize_date(c.text().collect::<String>().trim(), &date_normalize_re))
            .unwrap_or_default();
        let publish_date = date_cells.get(1)
            .and_then(|c| normalize_date(c.text().collect::<String>().trim(), &date_normalize_re))
            .unwrap_or_default();

        // 发布单位
        let office_unit = title_cell.select(&unit_selector).next()
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
            doc_type: "normative".to_string(),
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
