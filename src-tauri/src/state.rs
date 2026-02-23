//! 应用全局状态管理
//!
//! 存储需要跨窗口/命令共享的状态

use tokio::sync::Mutex;
use crate::commands::window_cmd::OcrResultPayload;

/// Anki 卡片初始化数据
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnkiCardInitData {
    pub image_path: Option<String>,
    pub ocr_text: Option<String>,
    pub highlight_words: Option<Vec<String>>,
}

/// 应用全局状态
pub struct AppState {
    /// 待处理的 OCR 结果
    /// 用于 OCR 结果窗口创建后获取数据
    pub pending_ocr_result: Mutex<Option<OcrResultPayload>>,
    /// 待处理的 Anki 卡片初始化数据
    /// 前端 mounted 后主动拉取，避免事件时序问题
    pub pending_anki_init: Mutex<Option<AnkiCardInitData>>,
}

impl AppState {
    /// 创建新的应用状态
    pub fn new() -> Self {
        Self {
            pending_ocr_result: Mutex::new(None),
            pending_anki_init: Mutex::new(None),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}
