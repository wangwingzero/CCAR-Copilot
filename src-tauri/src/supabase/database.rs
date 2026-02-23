//! Supabase 数据库服务
//!
//! 封装 Supabase REST API (PostgREST)，提供：
//! - 表数据查询
//! - 数据插入/更新/删除
//! - 查询构建器

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tracing::debug;

use super::client::{SupabaseClient, SupabaseError};

/// 数据库服务
#[derive(Debug, Clone)]
pub struct DatabaseService {
    /// Supabase 客户端
    client: SupabaseClient,
}

impl DatabaseService {
    /// 创建数据库服务
    pub fn new(client: SupabaseClient) -> Self {
        Self { client }
    }

    /// 创建查询构建器
    pub fn from(&self, table: &str) -> QueryBuilder {
        QueryBuilder::new(self.client.clone(), table)
    }
}

/// 查询构建器
///
/// 提供链式 API 构建 PostgREST 查询
#[derive(Debug, Clone)]
pub struct QueryBuilder {
    client: SupabaseClient,
    table: String,
    select: Option<String>,
    filters: Vec<String>,
    order: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
    single: bool,
}

impl QueryBuilder {
    /// 创建新的查询构建器
    fn new(client: SupabaseClient, table: &str) -> Self {
        Self {
            client,
            table: table.to_string(),
            select: None,
            filters: Vec::new(),
            order: None,
            limit: None,
            offset: None,
            single: false,
        }
    }

    /// 选择列
    ///
    /// # Example
    /// ```ignore
    /// db.from("users").select("id,name,email")
    /// ```
    pub fn select(mut self, columns: &str) -> Self {
        self.select = Some(columns.to_string());
        self
    }

    /// 等于过滤
    ///
    /// # Example
    /// ```ignore
    /// db.from("users").eq("id", "123")
    /// ```
    pub fn eq(mut self, column: &str, value: &str) -> Self {
        self.filters
            .push(format!("{}=eq.{}", column, encode_value(value)));
        self
    }

    /// 不等于过滤
    pub fn neq(mut self, column: &str, value: &str) -> Self {
        self.filters
            .push(format!("{}=neq.{}", column, encode_value(value)));
        self
    }

    /// 大于过滤
    pub fn gt(mut self, column: &str, value: &str) -> Self {
        self.filters
            .push(format!("{}=gt.{}", column, encode_value(value)));
        self
    }

    /// 大于等于过滤
    pub fn gte(mut self, column: &str, value: &str) -> Self {
        self.filters
            .push(format!("{}=gte.{}", column, encode_value(value)));
        self
    }

    /// 小于过滤
    pub fn lt(mut self, column: &str, value: &str) -> Self {
        self.filters
            .push(format!("{}=lt.{}", column, encode_value(value)));
        self
    }

    /// 小于等于过滤
    pub fn lte(mut self, column: &str, value: &str) -> Self {
        self.filters
            .push(format!("{}=lte.{}", column, encode_value(value)));
        self
    }

    /// LIKE 过滤（区分大小写）
    pub fn like(mut self, column: &str, pattern: &str) -> Self {
        self.filters
            .push(format!("{}=like.{}", column, encode_value(pattern)));
        self
    }

    /// ILIKE 过滤（不区分大小写）
    pub fn ilike(mut self, column: &str, pattern: &str) -> Self {
        self.filters
            .push(format!("{}=ilike.{}", column, encode_value(pattern)));
        self
    }

    /// IN 过滤
    pub fn in_list(mut self, column: &str, values: &[&str]) -> Self {
        let values_str = values
            .iter()
            .map(|v| encode_value(v))
            .collect::<Vec<_>>()
            .join(",");
        self.filters
            .push(format!("{}=in.({})", column, values_str));
        self
    }

    /// IS NULL 过滤
    pub fn is_null(mut self, column: &str) -> Self {
        self.filters.push(format!("{}=is.null", column));
        self
    }

    /// IS NOT NULL 过滤
    pub fn is_not_null(mut self, column: &str) -> Self {
        self.filters.push(format!("{}=not.is.null", column));
        self
    }

    /// 排序
    ///
    /// # Example
    /// ```ignore
    /// db.from("users").order("created_at", false) // 降序
    /// ```
    pub fn order(mut self, column: &str, ascending: bool) -> Self {
        let direction = if ascending { "asc" } else { "desc" };
        self.order = Some(format!("{}={}.nullslast", column, direction));
        self
    }

    /// 限制结果数量
    pub fn limit(mut self, count: u32) -> Self {
        self.limit = Some(count);
        self
    }

    /// 跳过指定数量
    pub fn offset(mut self, count: u32) -> Self {
        self.offset = Some(count);
        self
    }

    /// 返回单条记录
    pub fn single(mut self) -> Self {
        self.single = true;
        self.limit = Some(1);
        self
    }

    /// 构建查询 URL
    fn build_url(&self) -> String {
        let mut url = format!("{}/{}", self.client.config().rest_url(), self.table);
        let mut params = Vec::new();

        if let Some(ref select) = self.select {
            params.push(format!("select={}", select));
        }

        params.extend(self.filters.clone());

        if let Some(ref order) = self.order {
            params.push(format!("order={}", order));
        }

        if let Some(limit) = self.limit {
            params.push(format!("limit={}", limit));
        }

        if let Some(offset) = self.offset {
            params.push(format!("offset={}", offset));
        }

        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }

        url
    }

    /// 执行查询
    pub async fn execute<T: DeserializeOwned>(
        self,
        access_token: Option<&str>,
    ) -> Result<Vec<T>, SupabaseError> {
        let url = self.build_url();
        debug!("执行查询: {}", url);
        self.client.get(&url, access_token).await
    }

    /// 执行查询，返回单条记录
    pub async fn execute_single<T: DeserializeOwned>(
        self,
        access_token: Option<&str>,
    ) -> Result<Option<T>, SupabaseError> {
        let url = self.build_url();
        debug!("执行单条查询: {}", url);

        let results: Vec<T> = self.client.get(&url, access_token).await?;
        Ok(results.into_iter().next())
    }

    /// 插入数据
    pub async fn insert<T: Serialize, R: DeserializeOwned>(
        &self,
        data: &T,
        access_token: Option<&str>,
    ) -> Result<R, SupabaseError> {
        let url = format!("{}/{}?select=*", self.client.config().rest_url(), self.table);
        debug!("插入数据: {}", url);
        self.client.post(&url, data, access_token).await
    }

    /// 更新数据（需要先设置过滤条件）
    pub async fn update<T: Serialize, R: DeserializeOwned>(
        &self,
        data: &T,
        access_token: Option<&str>,
    ) -> Result<R, SupabaseError> {
        let mut url = format!("{}/{}?select=*", self.client.config().rest_url(), self.table);

        // 添加过滤条件
        if !self.filters.is_empty() {
            url.push('&');
            url.push_str(&self.filters.join("&"));
        }

        debug!("更新数据: {}", url);
        self.client.patch(&url, data, access_token).await
    }

    /// Upsert 数据（插入或更新）
    pub async fn upsert<T: Serialize, R: DeserializeOwned>(
        &self,
        data: &T,
        access_token: Option<&str>,
    ) -> Result<R, SupabaseError> {
        let url = format!(
            "{}/{}?select=*&on_conflict=id",
            self.client.config().rest_url(),
            self.table
        );
        debug!("Upsert 数据: {}", url);
        self.client.post(&url, data, access_token).await
    }

    /// 删除数据（需要先设置过滤条件）
    pub async fn delete<R: DeserializeOwned>(
        &self,
        access_token: Option<&str>,
    ) -> Result<R, SupabaseError> {
        let mut url = format!("{}/{}?select=*", self.client.config().rest_url(), self.table);

        // 添加过滤条件
        if !self.filters.is_empty() {
            url.push('&');
            url.push_str(&self.filters.join("&"));
        }

        debug!("删除数据: {}", url);
        self.client.delete(&url, access_token).await
    }
}

/// URL 编码值
fn encode_value(value: &str) -> String {
    // 简单的 URL 编码
    value
        .replace('%', "%25")
        .replace(' ', "%20")
        .replace('+', "%2B")
        .replace('&', "%26")
        .replace('=', "%3D")
        .replace('#', "%23")
}

// ============= 数据模型 =============

/// 订阅信息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct Subscription {
    pub id: String,
    pub user_id: String,
    pub plan: String,
    pub status: String,
    pub purchased_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 设备信息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct Device {
    pub id: String,
    pub user_id: String,
    pub machine_id: String,
    pub device_name: Option<String>,
    pub os_version: Option<String>,
    pub app_version: Option<String>,
    pub is_active: bool,
    pub last_active_at: String,
    pub created_at: String,
}

/// 使用统计
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct UsageStat {
    pub id: String,
    pub user_id: String,
    pub date: String,
    pub feature: String,
    pub count: i32,
    pub created_at: String,
    pub updated_at: String,
}

/// 订单信息
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct Order {
    pub id: String,
    pub user_id: String,
    pub order_no: String,
    pub amount: f64,
    pub status: String,
    pub payment_method: Option<String>,
    pub trade_no: Option<String>,
    pub paid_at: Option<String>,
    pub created_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_client() -> SupabaseClient {
        let config = super::super::client::SupabaseConfig::from_env().unwrap();
        SupabaseClient::new(config).unwrap()
    }

    #[test]
    fn test_query_builder_url() {
        let client = create_test_client();
        let db = DatabaseService::new(client);

        let query = db
            .from("users")
            .select("id,name")
            .eq("status", "active")
            .order("created_at", false)
            .limit(10);

        let url = query.build_url();
        assert!(url.contains("/users?"));
        assert!(url.contains("select=id,name"));
        assert!(url.contains("status=eq.active"));
        assert!(url.contains("limit=10"));
    }

    #[test]
    fn test_encode_value() {
        assert_eq!(encode_value("hello world"), "hello%20world");
        assert_eq!(encode_value("a+b"), "a%2Bb");
        assert_eq!(encode_value("a&b=c"), "a%26b%3Dc");
    }
}
