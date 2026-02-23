//! Sidecar 相关 Tauri 命令
//!
//! 封装 Python Sidecar 服务调用，暴露给前端调用。
//!
//! 支持的服务：
//! - OCR: 文字识别
//! - 翻译: 多语言翻译
//! - Anki: 制卡服务
//! - 录屏: 屏幕录制

use crate::error::HuGeResult;
use crate::sidecar::SidecarManager;
use std::sync::Arc;
use tauri::State;
use tokio::sync::Mutex;

/// Sidecar 管理器状态
pub struct SidecarState {
    pub manager: Arc<Mutex<Option<SidecarManager>>>,
}

impl SidecarState {
    pub fn new() -> Self {
        Self {
            manager: Arc::new(Mutex::new(None)),
        }
    }
}

impl Default for SidecarState {
    fn default() -> Self {
        Self::new()
    }
}

/// OCR 识别结果
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OcrResult {
    /// 识别的文本
    pub text: String,
    /// 文本区域列表
    pub boxes: Vec<OcrBox>,
    /// 处理耗时（秒）
    pub elapse: f64,
}

/// OCR 文本区域
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OcrBox {
    /// 文本内容
    pub text: String,
    /// 置信度 (0.0 - 1.0)
    pub confidence: f64,
    /// 边界框坐标
    pub box_coords: Vec<Vec<f64>>,
}

// 录屏相关类型和命令已迁移到 recording_cmd.rs（原生 Rust 实现）

/// 调用 OCR 服务（原生 Rust 实现）
///
/// 使用 PP-OCRv5 模型进行文字识别，无需 Python Sidecar。
///
/// # 参数
///
/// - `image_path`: 图像文件路径
///
/// # 返回
///
/// 返回 OCR 识别结果，包含识别的文本、文本区域边界框和处理耗时
///
/// # 示例
///
/// ```ignore
/// let result = call_ocr("/tmp/screenshot.png").await?;
/// println!("识别到的文字: {}", result.text);
/// for box in result.boxes {
///     println!("区域: {} (置信度: {})", box.text, box.confidence);
/// }
/// ```
#[tauri::command]
pub async fn call_ocr(image_path: String) -> HuGeResult<OcrResult> {
    use crate::ocr::OcrEngine;
    use tracing::info;

    info!("调用原生 OCR 服务: {}", image_path);

    // 获取 OCR 引擎单例
    let engine = OcrEngine::instance().map_err(|e| {
        crate::error::HuGeError::OcrError(format!("OCR 引擎初始化失败: {}", e))
    })?;

    // 执行 OCR
    let native_result = engine.recognize(&image_path).await.map_err(|e| {
        crate::error::HuGeError::OcrError(format!("OCR 识别失败: {}", e))
    })?;

    info!(
        "OCR 完成: {} 个文本区域, 耗时 {:.2}s",
        native_result.boxes.len(),
        native_result.elapse
    );

    // 转换为命令返回类型
    let boxes: Vec<OcrBox> = native_result
        .boxes
        .into_iter()
        .map(|b| OcrBox {
            text: b.text,
            confidence: b.confidence,
            box_coords: b.box_coords,
        })
        .collect();

    Ok(OcrResult {
        text: native_result.text,
        boxes,
        elapse: native_result.elapse,
    })
}

/// 翻译结果
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TranslationResult {
    /// 翻译后的文本
    pub translated_text: String,
    /// 检测到的源语言
    pub source_lang: String,
    /// 目标语言
    pub target_lang: String,
    /// 使用的翻译提供商
    pub provider: String,
    /// 是否来自缓存
    pub cached: bool,
}

/// 调用翻译服务
///
/// # 参数
///
/// - `text`: 要翻译的文本
/// - `target_lang`: 目标语言代码（如 "zh"、"en"、"ja"）
/// - `provider`: 翻译提供商（可选，默认 "google"，支持 "google"、"deepl"、"baidu"）
///
/// # 返回
///
/// 返回翻译结果，包含翻译后的文本、源语言、目标语言等信息
///
/// # 示例
///
/// ```ignore
/// let result = call_translate("Hello, world!", "zh", None).await?;
/// println!("翻译结果: {}", result.translated_text);
/// ```
#[tauri::command]
pub async fn call_translate(
    state: State<'_, SidecarState>,
    text: String,
    target_lang: String,
    provider: Option<String>,
) -> HuGeResult<TranslationResult> {
    use tracing::{debug, info};

    info!(
        "调用翻译服务: {} 字符 -> {} ({})",
        text.len(),
        target_lang,
        provider.as_deref().unwrap_or("google")
    );

    let manager_guard = state.manager.lock().await;
    let manager = manager_guard
        .as_ref()
        .ok_or_else(|| crate::error::HuGeError::SidecarError("Sidecar 未初始化".to_string()))?;

    // 构建请求参数
    let mut params = serde_json::json!({
        "text": text,
        "target_lang": target_lang,
    });

    if let Some(ref p) = provider {
        params["provider"] = serde_json::json!(p);
    }

    // 调用 Python 翻译服务
    let result = manager.call("translation", "translate", params).await?;

    debug!("翻译服务返回: {:?}", result);

    // 解析结果
    let translated_text = result
        .get("translated_text")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let source_lang = result
        .get("source_lang")
        .and_then(|v| v.as_str())
        .unwrap_or("auto")
        .to_string();

    let target_lang_result = result
        .get("target_lang")
        .and_then(|v| v.as_str())
        .unwrap_or(&target_lang)
        .to_string();

    let provider_result = result
        .get("provider")
        .and_then(|v| v.as_str())
        .unwrap_or("google")
        .to_string();

    let cached = result
        .get("cached")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    info!(
        "翻译完成: {} -> {} ({}, cached={})",
        source_lang, target_lang_result, provider_result, cached
    );

    Ok(TranslationResult {
        translated_text,
        source_lang,
        target_lang: target_lang_result,
        provider: provider_result,
        cached,
    })
}

/// Anki 制卡结果
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AnkiResult {
    /// 是否成功
    pub success: bool,
    /// 创建的卡片 ID（成功时）
    pub card_id: Option<i64>,
    /// 错误信息（失败时）
    pub error: Option<String>,
}

/// 调用 Anki 制卡服务
///
/// # 参数
///
/// - `front`: 卡片正面内容
/// - `back`: 卡片背面内容
/// - `deck`: 目标牌组名称
/// - `image_path`: 图片路径（可选，会自动上传到 Anki 媒体文件夹）
///
/// # 返回
///
/// 返回创建的卡片 ID
///
/// # 示例
///
/// ```ignore
/// let result = call_anki("Hello", "你好", "默认", Some("/tmp/screenshot.png")).await?;
/// if result.success {
///     println!("卡片已创建，ID: {:?}", result.card_id);
/// }
/// ```
///
/// # 注意事项
///
/// - 需要 Anki 已启动并安装了 AnkiConnect 插件
/// - 牌组必须已存在
#[tauri::command]
pub async fn call_anki(
    state: State<'_, SidecarState>,
    front: String,
    back: String,
    deck: String,
    image_path: Option<String>,
) -> HuGeResult<AnkiResult> {
    use tracing::{debug, info, warn};

    info!("调用 Anki 服务: 牌组={}, 正面长度={}", deck, front.len());

    let manager_guard = state.manager.lock().await;
    let manager = manager_guard
        .as_ref()
        .ok_or_else(|| crate::error::HuGeError::SidecarError("Sidecar 未初始化".to_string()))?;

    // 构建请求参数
    let mut params = serde_json::json!({
        "deck_name": deck,
        "model_name": "Basic",  // 使用基础模板
        "fields": {
            "Front": front,
            "Back": back,
        },
        "tags": ["虎哥截图"],
    });

    // 如果有图片，添加媒体文件
    if let Some(ref path) = image_path {
        // 生成唯一文件名避免冲突
        let unique_filename = format!(
            "huge_{}_{}.png",
            chrono::Utc::now().format("%Y%m%d_%H%M%S"),
            &uuid::Uuid::new_v4().to_string()[..8]
        );

        params["media_files"] = serde_json::json!({
            "Back": {
                "filename": unique_filename,
                "path": path,
            }
        });

        debug!("Anki 卡片包含图片: {} -> {}", path, unique_filename);
    }

    // 调用 Python Anki 服务
    let result = manager.call("anki", "add_card", params).await?;

    debug!("Anki 服务返回: {:?}", result);

    // 解析结果
    let success = result
        .get("success")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let card_id = result
        .get("card_id")
        .and_then(|v| v.as_i64());

    let error = result
        .get("error")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    if success {
        info!("Anki 卡片创建成功: card_id={:?}", card_id);
    } else {
        warn!("Anki 卡片创建失败: {:?}", error);
    }

    Ok(AnkiResult {
        success,
        card_id,
        error,
    })
}

/// 检查 Sidecar 状态
///
/// # 返回
///
/// 返回 Sidecar 是否正在运行
#[tauri::command]
pub async fn check_sidecar_status(
    state: State<'_, SidecarState>,
) -> HuGeResult<bool> {
    let manager_guard = state.manager.lock().await;
    if let Some(ref manager) = *manager_guard {
        Ok(manager.is_running().await)
    } else {
        Ok(false)
    }
}

/// 启动 Sidecar
#[tauri::command]
pub async fn start_sidecar(
    app_handle: tauri::AppHandle,
    state: State<'_, SidecarState>,
) -> HuGeResult<()> {
    use tracing::info;

    info!("启动 Sidecar...");

    let mut manager_guard = state.manager.lock().await;

    // 如果已有 manager 且正在运行，直接返回
    if let Some(ref manager) = *manager_guard {
        if manager.is_running().await {
            info!("Sidecar 已在运行");
            return Ok(());
        }
    }

    // 创建新的 manager 并启动
    let manager = SidecarManager::new(app_handle);
    manager.start().await?;
    *manager_guard = Some(manager);

    info!("Sidecar 启动成功");
    Ok(())
}

/// 停止 Sidecar
#[tauri::command]
pub async fn stop_sidecar(
    state: State<'_, SidecarState>,
) -> HuGeResult<()> {
    use tracing::info;

    info!("停止 Sidecar...");

    let mut manager_guard = state.manager.lock().await;

    if let Some(ref manager) = *manager_guard {
        manager.stop().await?;
    }

    *manager_guard = None;

    info!("Sidecar 已停止");
    Ok(())
}

/// 重启 Sidecar
#[tauri::command]
pub async fn restart_sidecar(
    app_handle: tauri::AppHandle,
    state: State<'_, SidecarState>,
) -> HuGeResult<()> {
    use tracing::info;

    info!("重启 Sidecar...");

    // 先停止
    {
        let manager_guard = state.manager.lock().await;
        if let Some(ref manager) = *manager_guard {
            let _ = manager.stop().await;
        }
    }

    // 再启动
    let mut manager_guard = state.manager.lock().await;
    let manager = SidecarManager::new(app_handle);
    manager.start().await?;
    *manager_guard = Some(manager);

    info!("Sidecar 重启成功");
    Ok(())
}

/// 通用 Sidecar 调用命令
///
/// 允许前端调用任意 Sidecar 服务
///
/// # 参数
///
/// - `service`: 服务名称（如 "ocr", "translate", "document"）
/// - `method`: 方法名称
/// - `params`: 方法参数
///
/// # 返回
///
/// 返回服务调用结果
#[tauri::command]
pub async fn call_sidecar(
    state: State<'_, SidecarState>,
    service: String,
    method: String,
    params: serde_json::Value,
) -> HuGeResult<serde_json::Value> {
    use tracing::{debug, info};

    info!("通用 Sidecar 调用: {}.{}", service, method);
    debug!("参数: {:?}", params);

    let manager_guard = state.manager.lock().await;
    let manager = manager_guard
        .as_ref()
        .ok_or_else(|| crate::error::HuGeError::SidecarError("Sidecar 未初始化".to_string()))?;

    let result = manager.call(&service, &method, params).await?;

    debug!("Sidecar 返回: {:?}", result);

    Ok(result)
}

// ============================================
// 直接翻译（不依赖 Sidecar）
// ============================================
//
// 参考 Python 版本的 EnhancedTranslationService，
// 使用免费翻译 API（MyMemory）直接翻译，无需 Python Sidecar。
// 支持智能语言检测：中文→英语，英语/其他→中文。

/// 直接翻译结果（不依赖 Sidecar，字段与前端 TranslateResult 类型对齐）
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirectTranslationResult {
    /// 翻译后的文本
    pub translated_text: String,
    /// 检测到的源语言
    pub source_lang: String,
    /// 目标语言
    pub target_lang: String,
    /// 使用的翻译提供商
    pub provider: String,
}

/// MyMemory API 响应结构
#[derive(Debug, serde::Deserialize)]
struct MyMemoryResponse {
    #[serde(rename = "responseData")]
    response_data: MyMemoryResponseData,
    #[serde(rename = "responseStatus")]
    response_status: Option<u32>,
}

#[derive(Debug, serde::Deserialize)]
struct MyMemoryResponseData {
    #[serde(rename = "translatedText")]
    translated_text: String,
}

/// 直接翻译文本（不依赖 Sidecar）
///
/// 使用免费翻译 API（MyMemory）进行翻译，无需 Python Sidecar。
/// 支持智能语言检测：
/// - 文本包含中文 → 翻译为英语
/// - 文本不含中文 → 翻译为中文
///
/// # 参数
///
/// - `text`: 要翻译的文本
/// - `target_lang`: 目标语言代码（可选，不提供时自动检测）
///
/// # 返回
///
/// 返回翻译结果，字段名使用 camelCase 以匹配前端 TranslateResult 类型
#[tauri::command]
pub async fn translate_text_direct(
    text: String,
    target_lang: Option<String>,
) -> HuGeResult<DirectTranslationResult> {
    use tracing::info;

    let text = text.trim().to_string();
    if text.is_empty() {
        return Err(crate::error::HuGeError::Unknown(
            "没有可翻译的文字".to_string(),
        ));
    }

    // 智能语言检测（参考 Python 版本的 _do_smart_translate）
    let has_chinese = text.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));

    let (source, target) = match target_lang {
        Some(ref lang) if !lang.is_empty() => {
            if has_chinese {
                ("zh-CN".to_string(), lang.clone())
            } else {
                ("en".to_string(), lang.clone())
            }
        }
        _ => {
            // 智能检测：中文→英语，非中文→中文
            if has_chinese {
                ("zh-CN".to_string(), "en".to_string())
            } else {
                ("en".to_string(), "zh-CN".to_string())
            }
        }
    };

    info!(
        "直接翻译: {} 字符, {} -> {}",
        text.len(),
        source,
        target
    );

    // 对长文本进行分段翻译（MyMemory 限制 500 字节/请求）
    let translated = if text.len() > 400 {
        translate_chunked(&text, &source, &target).await?
    } else {
        call_mymemory_api(&text, &source, &target).await?
    };

    info!("翻译完成: {} -> {} 字符", text.len(), translated.len());

    Ok(DirectTranslationResult {
        translated_text: translated,
        source_lang: source,
        target_lang: target,
        provider: "MyMemory".to_string(),
    })
}

/// 调用 MyMemory 翻译 API
///
/// API 文档: https://mymemory.translated.net/doc/spec.php
/// 免费额度: 5000 字符/天（匿名），50000 字符/天（提供邮箱）
async fn call_mymemory_api(text: &str, source: &str, target: &str) -> HuGeResult<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| {
            crate::error::HuGeError::Unknown(format!("HTTP 客户端创建失败: {}", e))
        })?;

    let lang_pair = format!("{}|{}", source, target);

    let resp = client
        .get("https://api.mymemory.translated.net/get")
        .query(&[("q", text), ("langpair", &lang_pair)])
        .send()
        .await
        .map_err(|e| {
            crate::error::HuGeError::Unknown(format!("翻译请求失败: {}", e))
        })?;

    if !resp.status().is_success() {
        return Err(crate::error::HuGeError::Unknown(format!(
            "翻译服务返回错误状态: {}",
            resp.status()
        )));
    }

    let api_resp: MyMemoryResponse = resp.json().await.map_err(|e| {
        crate::error::HuGeError::Unknown(format!("翻译响应解析失败: {}", e))
    })?;

    // 检查 API 状态
    if let Some(status) = api_resp.response_status {
        if status != 200 {
            return Err(crate::error::HuGeError::Unknown(format!(
                "翻译服务返回错误: status={}",
                status
            )));
        }
    }

    let translated = api_resp.response_data.translated_text;

    // 检查是否返回了警告信息（超出额度时 MyMemory 会返回警告文本而非翻译结果）
    if translated.contains("MYMEMORY WARNING") {
        return Err(crate::error::HuGeError::Unknown(
            "翻译服务今日免费额度已用完，请明天再试".to_string(),
        ));
    }

    if translated.is_empty() {
        return Err(crate::error::HuGeError::Unknown(
            "翻译服务返回空结果".to_string(),
        ));
    }

    Ok(translated)
}

/// 分段翻译长文本
///
/// 将文本按段落分割为不超过 400 字符的块，逐段翻译后拼接。
async fn translate_chunked(text: &str, source: &str, target: &str) -> HuGeResult<String> {
    let chunks = split_text_into_chunks(text, 400);
    let mut results = Vec::with_capacity(chunks.len());

    for chunk in &chunks {
        let translated = call_mymemory_api(chunk, source, target).await?;
        results.push(translated);
    }

    Ok(results.join(""))
}

/// 将文本按段落/换行分割为不超过指定长度的块
fn split_text_into_chunks(text: &str, max_len: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();

    for line in text.lines() {
        // 如果加上当前行会超过限制，先保存已有内容
        if !current.is_empty() && current.len() + line.len() + 1 > max_len {
            chunks.push(current.clone());
            current.clear();
        }

        // 如果单行本身就超过限制，按字符分割
        if line.len() > max_len {
            if !current.is_empty() {
                chunks.push(current.clone());
                current.clear();
            }
            let mut remaining = line;
            while remaining.len() > max_len {
                // 在字符边界处分割
                let split_at = remaining
                    .char_indices()
                    .take_while(|(i, _)| *i <= max_len)
                    .last()
                    .map(|(i, c)| i + c.len_utf8())
                    .unwrap_or(max_len);
                chunks.push(remaining[..split_at].to_string());
                remaining = &remaining[split_at..];
            }
            if !remaining.is_empty() {
                current = remaining.to_string();
            }
        } else {
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
        }
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

// 录屏命令已迁移到 recording_cmd.rs（原生 Rust 实现）

// ============================================
// 打开文档检测（不依赖 Sidecar，纯 Rust 实现）
// ============================================
//
// 使用 Win32 API EnumWindows 枚举窗口标题来检测打开的 Word/WPS 文档。
// 此方法不依赖 COM 或 Sidecar，不受管理员/普通用户权限隔离影响。

/// 打开的文档信息
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenDocumentInfo {
    /// 文档名称
    pub name: String,
    /// 完整路径（可能为空，窗口枚举无法获取完整路径）
    pub full_path: String,
    /// 应用类型 ("word" 或 "wps")
    pub app_type: String,
}

/// 获取打开的文档列表结果
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenDocumentsResult {
    pub success: bool,
    pub documents: Vec<OpenDocumentInfo>,
    pub available: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// 获取打开的 Word/WPS 文档列表（纯 Rust 实现，不依赖 Sidecar）
///
/// 使用 Win32 API 枚举窗口标题，从 WPS (wps.exe) 和 Word (WINWORD.EXE) 的
/// 窗口标题中提取文档名称。不受管理员/普通用户权限隔离影响。
#[tauri::command]
pub async fn get_open_documents_native() -> HuGeResult<OpenDocumentsResult> {
    use tracing::info;
    info!("纯 Rust 实现: 获取打开的 Word/WPS 文档列表");

    #[cfg(windows)]
    {
        get_open_documents_impl()
    }

    #[cfg(not(windows))]
    {
        Ok(OpenDocumentsResult {
            success: false,
            documents: vec![],
            available: false,
            error: Some("仅支持 Windows 平台".to_string()),
        })
    }
}

#[cfg(windows)]
fn get_open_documents_impl() -> HuGeResult<OpenDocumentsResult> {
    use std::collections::HashSet;
    use tracing::{debug, info};
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM, TRUE};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
        IsWindowVisible, GetClassNameW,
    };

    // Step 1: 获取 WPS 和 Word 进程 PID
    let mut word_pids: HashSet<u32> = HashSet::new();
    let mut wps_pids: HashSet<u32> = HashSet::new();

    // 使用 CreateToolhelp32Snapshot 枚举进程
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
            .map_err(|e| crate::error::HuGeError::WindowError(format!("CreateToolhelp32Snapshot 失败: {}", e)))?;

        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };

        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let exe_name: String = entry.szExeFile.iter()
                    .take_while(|&&c| c != 0)
                    .map(|&c| c as u8 as char)
                    .collect();
                let exe_lower = exe_name.to_lowercase();

                if exe_lower == "winword.exe" {
                    word_pids.insert(entry.th32ProcessID);
                } else if exe_lower == "wps.exe" {
                    wps_pids.insert(entry.th32ProcessID);
                }

                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }

        let _ = windows::Win32::Foundation::CloseHandle(snapshot);
    }

    if word_pids.is_empty() && wps_pids.is_empty() {
        info!("未检测到 Word 或 WPS 进程");
        return Ok(OpenDocumentsResult {
            success: true,
            documents: vec![],
            available: true,
            error: None,
        });
    }

    debug!("检测到进程 - Word PIDs: {:?}, WPS PIDs: {:?}", word_pids, wps_pids);

    let target_pids: HashSet<u32> = word_pids.union(&wps_pids).cloned().collect();

    // Step 2: 枚举窗口，提取文档信息
    struct WindowData {
        title: String,
        app_type: String,
    }

    let mut found_windows: Vec<WindowData> = Vec::new();
    let found_ptr = &mut found_windows as *mut Vec<WindowData>;

    // 在回调外部准备所需数据
    struct CallbackData {
        windows: *mut Vec<WindowData>,
        target_pids: HashSet<u32>,
        word_pids: HashSet<u32>,
    }

    let mut callback_data = CallbackData {
        windows: found_ptr,
        target_pids,
        word_pids: word_pids.clone(),
    };
    let data_ptr = &mut callback_data as *mut CallbackData;

    unsafe extern "system" fn enum_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let data = &mut *(lparam.0 as *mut CallbackData);

        if !IsWindowVisible(hwnd).as_bool() {
            return TRUE;
        }

        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));

        if !data.target_pids.contains(&pid) {
            return TRUE;
        }

        let length = GetWindowTextLengthW(hwnd);
        if length <= 0 {
            return TRUE;
        }

        let mut buf = vec![0u16; (length + 1) as usize];
        let actual_len = GetWindowTextW(hwnd, &mut buf);
        if actual_len <= 0 {
            return TRUE;
        }
        let title = String::from_utf16_lossy(&buf[..actual_len as usize]);

        // 获取窗口类名
        let mut class_buf = [0u16; 256];
        let class_len = GetClassNameW(hwnd, &mut class_buf);
        let class_name = if class_len > 0 {
            String::from_utf16_lossy(&class_buf[..class_len as usize])
        } else {
            String::new()
        };

        let app_type = if data.word_pids.contains(&pid) {
            "word"
        } else {
            "wps"
        };

        // 过滤：只保留主文档窗口
        let is_doc_window = if app_type == "word" {
            class_name.contains("OpusApp")
        } else {
            // WPS: Qt 窗口且标题包含文档后缀
            (class_name.contains("Qt") && class_name.contains("QWindow"))
                && (title.to_lowercase().contains(".doc")
                    || title.to_lowercase().contains(".docx")
                    || title.to_lowercase().contains(".wps"))
        };

        if is_doc_window {
            let windows = &mut *data.windows;
            windows.push(WindowData {
                title,
                app_type: app_type.to_string(),
            });
        }

        TRUE
    }

    unsafe {
        let _ = EnumWindows(Some(enum_callback), LPARAM(data_ptr as isize));
    }

    // Step 3: 从窗口标题提取文档名
    let mut documents: Vec<OpenDocumentInfo> = Vec::new();
    let mut seen_names: HashSet<String> = HashSet::new();

    for win in &found_windows {
        if let Some(doc_name) = extract_doc_name_from_title(&win.title) {
            if !seen_names.contains(&doc_name) {
                seen_names.insert(doc_name.clone());
                documents.push(OpenDocumentInfo {
                    name: doc_name,
                    full_path: String::new(),
                    app_type: win.app_type.clone(),
                });
            }
        }
    }

    info!("纯 Rust 实现: 找到 {} 个打开的文档", documents.len());
    for doc in &documents {
        info!("  [{}] {}", doc.app_type, doc.name);
    }

    Ok(OpenDocumentsResult {
        success: true,
        documents,
        available: true,
        error: None,
    })
}

/// 从窗口标题提取文档名
///
/// 支持的标题格式：
/// - Word: "文档名.docx - Word" 或 "文档名.docx  -  兼容模式 - Word"
/// - WPS:  "文档名.docx - WPS Office"
#[cfg(windows)]
fn extract_doc_name_from_title(title: &str) -> Option<String> {
    let extensions = [".docx", ".doc", ".docm", ".dotx", ".dotm", ".dot", ".wps", ".wpt"];

    let title_lower = title.to_lowercase();
    for ext in &extensions {
        if let Some(idx) = title_lower.rfind(ext) {
            let doc_name = title[..idx + ext.len()].trim().to_string();
            if !doc_name.is_empty() {
                return Some(doc_name);
            }
        }
    }

    None
}
