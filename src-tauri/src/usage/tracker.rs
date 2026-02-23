//! 使用量追踪服务实现

use chrono::Local;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;
use tracing::{debug, info};

use crate::supabase::{DatabaseService, SupabaseClient};

/// 受追踪的功能
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Feature {
    /// 翻译
    Translation,
    /// 网页转 Markdown
    WebToMarkdown,
    /// OCR（可选追踪）
    Ocr,
    /// 录屏
    ScreenRecording,
}

impl std::fmt::Display for Feature {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Feature::Translation => write!(f, "translation"),
            Feature::WebToMarkdown => write!(f, "web_to_markdown"),
            Feature::Ocr => write!(f, "ocr"),
            Feature::ScreenRecording => write!(f, "screen_recording"),
        }
    }
}

impl From<&str> for Feature {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "translation" => Feature::Translation,
            "web_to_markdown" => Feature::WebToMarkdown,
            "ocr" => Feature::Ocr,
            "screen_recording" => Feature::ScreenRecording,
            _ => Feature::Translation, // 默认
        }
    }
}

/// 使用统计
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageStats {
    /// 功能
    pub feature: String,
    /// 今日使用次数
    pub today_count: u32,
    /// 每日限制（0 = 无限制）
    pub daily_limit: u32,
    /// 是否达到限制
    pub is_limited: bool,
    /// 重置时间（明日零点）
    pub reset_at: String,
}

/// 使用量追踪器
pub struct UsageTracker {
    /// SQLite 连接
    conn: Mutex<Connection>,
    /// Supabase 客户端（用于云端同步）
    db: Option<DatabaseService>,
}

impl UsageTracker {
    /// 创建使用量追踪器
    pub fn new(data_dir: PathBuf, client: Option<SupabaseClient>) -> Result<Self, String> {
        let db_path = data_dir.join("usage.db");

        // 确保目录存在
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("创建数据目录失败: {}", e))?;
        }

        let conn = Connection::open(&db_path)
            .map_err(|e| format!("打开数据库失败: {}", e))?;

        // 创建表
        conn.execute(
            "CREATE TABLE IF NOT EXISTS usage (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                feature TEXT NOT NULL,
                date TEXT NOT NULL,
                count INTEGER DEFAULT 0,
                updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
                UNIQUE(feature, date)
            )",
            [],
        )
        .map_err(|e| format!("创建表失败: {}", e))?;

        // 创建索引
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_usage_feature_date ON usage(feature, date)",
            [],
        )
        .map_err(|e| format!("创建索引失败: {}", e))?;

        let db = client.map(DatabaseService::new);

        Ok(Self {
            conn: Mutex::new(conn),
            db,
        })
    }

    /// 获取今日日期
    fn today() -> String {
        Local::now().format("%Y-%m-%d").to_string()
    }

    /// 获取明日零点时间
    fn tomorrow_reset_time() -> String {
        let tomorrow = Local::now().date_naive() + chrono::Duration::days(1);
        format!("{}T00:00:00+08:00", tomorrow)
    }

    /// 获取每日限制
    fn get_daily_limit(feature: Feature) -> u32 {
        match feature {
            Feature::Translation => 10,
            Feature::WebToMarkdown => 5,
            Feature::Ocr => 0, // 无限制
            Feature::ScreenRecording => 0, // VIP 专属
        }
    }

    /// 获取今日使用次数
    pub fn get_today_count(&self, feature: Feature) -> Result<u32, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let today = Self::today();

        let count: u32 = conn
            .query_row(
                "SELECT count FROM usage WHERE feature = ?1 AND date = ?2",
                params![feature.to_string(), today],
                |row| row.get(0),
            )
            .unwrap_or(0);

        Ok(count)
    }

    /// 增加使用次数
    pub fn increment(&self, feature: Feature) -> Result<u32, String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let today = Self::today();
        let feature_str = feature.to_string();

        // 尝试插入或更新
        conn.execute(
            "INSERT INTO usage (feature, date, count, updated_at)
             VALUES (?1, ?2, 1, datetime('now'))
             ON CONFLICT(feature, date) DO UPDATE SET
                 count = count + 1,
                 updated_at = datetime('now')",
            params![feature_str, today],
        )
        .map_err(|e| format!("更新使用量失败: {}", e))?;

        // 获取新的计数
        let count: u32 = conn
            .query_row(
                "SELECT count FROM usage WHERE feature = ?1 AND date = ?2",
                params![feature_str, today],
                |row| row.get(0),
            )
            .map_err(|e| format!("查询使用量失败: {}", e))?;

        debug!("使用量增加: feature={}, count={}", feature_str, count);
        Ok(count)
    }

    /// 获取使用统计
    pub fn get_stats(&self, feature: Feature) -> Result<UsageStats, String> {
        let count = self.get_today_count(feature)?;
        let limit = Self::get_daily_limit(feature);
        let is_limited = limit > 0 && count >= limit;

        Ok(UsageStats {
            feature: feature.to_string(),
            today_count: count,
            daily_limit: limit,
            is_limited,
            reset_at: Self::tomorrow_reset_time(),
        })
    }

    /// 检查是否可以使用功能
    pub fn can_use(&self, feature: Feature) -> Result<bool, String> {
        let count = self.get_today_count(feature)?;
        let limit = Self::get_daily_limit(feature);

        if limit == 0 {
            Ok(true) // 无限制
        } else {
            Ok(count < limit)
        }
    }

    /// 检查并增加使用次数
    ///
    /// 返回 (是否允许, 当前次数, 限制)
    pub fn check_and_increment(&self, feature: Feature) -> Result<(bool, u32, u32), String> {
        let limit = Self::get_daily_limit(feature);

        if limit == 0 {
            let count = self.increment(feature)?;
            return Ok((true, count, 0));
        }

        let current = self.get_today_count(feature)?;
        if current >= limit {
            return Ok((false, current, limit));
        }

        let new_count = self.increment(feature)?;
        Ok((true, new_count, limit))
    }

    /// 重置今日使用量（用于测试）
    #[cfg(test)]
    pub fn reset_today(&self, feature: Feature) -> Result<(), String> {
        let conn = self.conn.lock().map_err(|e| e.to_string())?;
        let today = Self::today();

        conn.execute(
            "DELETE FROM usage WHERE feature = ?1 AND date = ?2",
            params![feature.to_string(), today],
        )
        .map_err(|e| format!("重置失败: {}", e))?;

        Ok(())
    }

    /// 同步到云端
    pub async fn sync_to_cloud(&self, user_id: &str, access_token: &str) -> Result<(), String> {
        let db = match &self.db {
            Some(db) => db.clone(),
            None => return Ok(()), // 未配置云端同步
        };

        let today = Self::today();

        // 在单独的作用域中获取数据，完全收集后释放锁
        let records: Vec<(String, u32)> = {
            let conn = self.conn.lock().map_err(|e| e.to_string())?;

            let mut stmt = conn
                .prepare("SELECT feature, count FROM usage WHERE date = ?1")
                .map_err(|e| format!("准备语句失败: {}", e))?;

            let rows = stmt
                .query_map(params![&today], |row| Ok((row.get(0)?, row.get(1)?)))
                .map_err(|e| format!("查询失败: {}", e))?;

            // 在作用域内完成收集
            let mut result = Vec::new();
            for r in rows.flatten() {
                result.push(r);
            }
            result
        }; // conn 和 stmt 在这里被释放

        // 同步每条记录（此时不持有任何锁）
        for (feature, count) in records {
            #[derive(Serialize)]
            struct UsageRecord {
                user_id: String,
                date: String,
                feature: String,
                count: i32,
            }

            let record = UsageRecord {
                user_id: user_id.to_string(),
                date: today.clone(),
                feature,
                count: count as i32,
            };

            let _: Result<Vec<serde_json::Value>, _> = db
                .from("usage_stats")
                .upsert(&record, Some(access_token))
                .await;
        }

        info!("使用量已同步到云端");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_feature_display() {
        assert_eq!(Feature::Translation.to_string(), "translation");
        assert_eq!(Feature::WebToMarkdown.to_string(), "web_to_markdown");
    }

    #[test]
    fn test_feature_from_str() {
        assert_eq!(Feature::from("translation"), Feature::Translation);
        assert_eq!(Feature::from("web_to_markdown"), Feature::WebToMarkdown);
    }

    #[test]
    fn test_usage_tracker() {
        let temp_dir = tempdir().unwrap();
        let tracker = UsageTracker::new(temp_dir.path().to_path_buf(), None).unwrap();

        // 初始计数应该是 0
        let count = tracker.get_today_count(Feature::Translation).unwrap();
        assert_eq!(count, 0);

        // 增加计数
        let new_count = tracker.increment(Feature::Translation).unwrap();
        assert_eq!(new_count, 1);

        // 再次增加
        let new_count = tracker.increment(Feature::Translation).unwrap();
        assert_eq!(new_count, 2);
    }

    #[test]
    fn test_usage_stats() {
        let temp_dir = tempdir().unwrap();
        let tracker = UsageTracker::new(temp_dir.path().to_path_buf(), None).unwrap();

        let stats = tracker.get_stats(Feature::Translation).unwrap();
        assert_eq!(stats.today_count, 0);
        assert_eq!(stats.daily_limit, 10);
        assert!(!stats.is_limited);
    }

    #[test]
    fn test_check_and_increment() {
        let temp_dir = tempdir().unwrap();
        let tracker = UsageTracker::new(temp_dir.path().to_path_buf(), None).unwrap();

        // 前 10 次应该允许
        for i in 1..=10 {
            let (allowed, count, limit) = tracker
                .check_and_increment(Feature::Translation)
                .unwrap();
            assert!(allowed, "第 {} 次应该允许", i);
            assert_eq!(count, i);
            assert_eq!(limit, 10);
        }

        // 第 11 次应该不允许
        let (allowed, count, limit) = tracker
            .check_and_increment(Feature::Translation)
            .unwrap();
        assert!(!allowed);
        assert_eq!(count, 10);
        assert_eq!(limit, 10);
    }
}
