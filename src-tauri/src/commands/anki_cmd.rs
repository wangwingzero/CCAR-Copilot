//! Anki 原生命令（直接 HTTP 调用 AnkiConnect，无需 Python Sidecar）
//!
//! 解决了之前通过 Sidecar 调用 AnkiConnect 导致的连接不稳定问题。
//! 所有 AnkiConnect 操作现在直接通过 Rust HTTP 客户端完成。

use crate::error::{HuGeError, HuGeResult};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

// ============================================
// AnkiConnect 配置
// ============================================

/// AnkiConnect 默认地址
const ANKI_CONNECT_URL: &str = "http://127.0.0.1:8765";
/// AnkiConnect API 版本
const ANKI_CONNECT_VERSION: u32 = 6;
/// HTTP 请求超时（秒）
const REQUEST_TIMEOUT_SECS: u64 = 10;

// ============================================
// AnkiConnect 通信类型
// ============================================

/// AnkiConnect 请求
#[derive(Debug, Serialize)]
struct AnkiRequest {
    action: String,
    version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<serde_json::Value>,
}

/// AnkiConnect 响应
#[derive(Debug, Deserialize)]
struct AnkiResponse {
    result: Option<serde_json::Value>,
    error: Option<String>,
}

// ============================================
// 内部辅助函数
// ============================================

/// 调用 AnkiConnect API
async fn invoke_anki(action: &str, params: Option<serde_json::Value>) -> HuGeResult<serde_json::Value> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(REQUEST_TIMEOUT_SECS))
        .build()
        .map_err(|e| HuGeError::Unknown(format!("HTTP 客户端创建失败: {}", e)))?;

    let request = AnkiRequest {
        action: action.to_string(),
        version: ANKI_CONNECT_VERSION,
        params,
    };

    debug!("AnkiConnect 请求: action={}, params={:?}", action, request.params);

    let response = client
        .post(ANKI_CONNECT_URL)
        .json(&request)
        .send()
        .await
        .map_err(|e| {
            if e.is_connect() {
                HuGeError::Unknown(
                    "无法连接到 AnkiConnect，请确保 Anki 已启动并安装了 AnkiConnect 插件".to_string(),
                )
            } else if e.is_timeout() {
                HuGeError::TimeoutError("AnkiConnect 请求超时".to_string())
            } else {
                HuGeError::Unknown(format!("AnkiConnect 网络请求失败: {}", e))
            }
        })?;

    let anki_resp: AnkiResponse = response.json().await.map_err(|e| {
        HuGeError::Unknown(format!("AnkiConnect 响应解析失败: {}", e))
    })?;

    if let Some(error) = anki_resp.error {
        return Err(HuGeError::Unknown(format!("AnkiConnect 错误: {}", error)));
    }

    Ok(anki_resp.result.unwrap_or(serde_json::Value::Null))
}

// ============================================
// 词典查询（Free Dictionary API）
// ============================================

/// 词典查询结果
#[derive(Debug, Clone, Serialize)]
struct DictResult {
    phonetic: String,
    definition: String,
    audio_url: Option<String>,
}

/// Free Dictionary API 响应
#[derive(Debug, Deserialize)]
struct DictApiEntry {
    phonetics: Option<Vec<DictPhonetic>>,
    meanings: Option<Vec<DictMeaning>>,
}

#[derive(Debug, Deserialize)]
struct DictPhonetic {
    text: Option<String>,
    audio: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DictMeaning {
    #[serde(rename = "partOfSpeech")]
    part_of_speech: Option<String>,
    definitions: Option<Vec<DictDefinition>>,
}

#[derive(Debug, Deserialize)]
struct DictDefinition {
    definition: Option<String>,
}

/// 查询英文单词的音标和释义
async fn lookup_word(word: &str) -> Option<DictResult> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .ok()?;

    let url = format!("https://api.dictionaryapi.dev/api/v2/entries/en/{}", word);

    let resp = client.get(&url).send().await.ok()?;
    if !resp.status().is_success() {
        debug!("词典查询失败: {} -> {}", word, resp.status());
        return None;
    }

    let entries: Vec<DictApiEntry> = resp.json().await.ok()?;
    let entry = entries.into_iter().next()?;

    // 提取音标
    let phonetic = entry.phonetics.as_ref()
        .and_then(|ps| ps.iter().find(|p| p.text.is_some()))
        .and_then(|p| p.text.clone())
        .unwrap_or_default();

    // 提取发音 URL
    let audio_url = entry.phonetics.as_ref()
        .and_then(|ps| ps.iter().find(|p| {
            p.audio.as_ref().is_some_and(|a| !a.is_empty())
        }))
        .and_then(|p| p.audio.clone());

    // 提取释义（取前 3 个含义）
    let mut defs = Vec::new();
    if let Some(meanings) = &entry.meanings {
        for meaning in meanings.iter().take(3) {
            let pos = meaning.part_of_speech.as_deref().unwrap_or("");
            if let Some(definitions) = &meaning.definitions {
                if let Some(first_def) = definitions.first() {
                    if let Some(d) = &first_def.definition {
                        defs.push(format!("{}: {}", pos, d));
                    }
                }
            }
        }
    }

    let definition = defs.join("\n");

    Some(DictResult {
        phonetic,
        definition,
        audio_url,
    })
}

/// 英译中（使用 MyMemory API）
async fn translate_to_chinese(text: &str) -> Option<String> {
    if text.is_empty() { return None; }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .ok()?;

    let lang_pair = "en|zh-CN";
    let resp = client
        .get("https://api.mymemory.translated.net/get")
        .query(&[("q", text), ("langpair", lang_pair)])
        .send()
        .await
        .ok()?;

    #[derive(Deserialize)]
    struct MmResp { #[serde(rename = "responseData")] data: MmData }
    #[derive(Deserialize)]
    struct MmData { #[serde(rename = "translatedText")] text: String }

    let mm: MmResp = resp.json().await.ok()?;
    if mm.data.text.contains("MYMEMORY WARNING") { return None; }
    Some(mm.data.text)
}

/// 搜索单词配图（Langeek → Unsplash → Pixabay → Bing → 360，与 Python 版本一致）
async fn search_word_image(word: &str) -> Option<String> {
    // 使用顺序调用避免复杂的类型系统

    // 1. Langeek（专业词汇配图）
    if let Some(r) = search_image_langeek(word).await { info!("[配图] {} 来源: Langeek", word); return Some(r); }
    // 2. Unsplash（需要 API Key，从配置读取）
    if let Some(r) = search_image_unsplash(word).await { info!("[配图] {} 来源: Unsplash", word); return Some(r); }
    // 3. Pixabay（需要 API Key，从配置读取）
    if let Some(r) = search_image_pixabay(word).await { info!("[配图] {} 来源: Pixabay", word); return Some(r); }
    // 4. Bing 图片搜索
    if let Some(r) = search_image_bing(word).await { info!("[配图] {} 来源: Bing", word); return Some(r); }
    // 5. 360 图片搜索
    if let Some(r) = search_image_360(word).await { info!("[配图] {} 来源: 360", word); return Some(r); }

    warn!("[配图] {} 所有图片源均失败", word);
    None
}

/// 从配置文件读取 API Key
fn read_api_keys() -> (Vec<String>, Vec<String>) {
    let config_path = dirs::config_dir()
        .unwrap_or_default()
        .join("com.wangh.hugescreenshot")
        .join("config.json");

    if let Ok(content) = std::fs::read_to_string(&config_path) {
        if let Ok(config) = serde_json::from_str::<serde_json::Value>(&content) {
            let anki = config.get("anki").unwrap_or(&serde_json::Value::Null);
            let unsplash = anki.get("unsplash_keys")
                .and_then(|v| v.as_str())
                .map(|s| s.split(',').filter(|k| !k.trim().is_empty()).map(|k| k.trim().to_string()).collect())
                .unwrap_or_default();
            let pixabay = anki.get("pixabay_key")
                .and_then(|v| v.as_str())
                .map(|s| s.split(',').filter(|k| !k.trim().is_empty()).map(|k| k.trim().to_string()).collect())
                .unwrap_or_default();
            return (unsplash, pixabay);
        }
    }
    (vec![], vec![])
}

/// Unsplash 图片搜索（需要 API Key）
async fn search_image_unsplash(word: &str) -> Option<String> {
    let (keys, _) = read_api_keys();
    let key = keys.first()?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .ok()?;

    let resp = client
        .get("https://api.unsplash.com/search/photos")
        .query(&[("query", word), ("per_page", "3"), ("orientation", "squarish")])
        .header("Authorization", format!("Client-ID {}", key))
        .send()
        .await
        .ok()?;

    #[derive(Deserialize)]
    struct UnsplashResp { results: Vec<UnsplashPhoto> }
    #[derive(Deserialize)]
    struct UnsplashPhoto { urls: UnsplashUrls }
    #[derive(Deserialize)]
    struct UnsplashUrls { small: Option<String>, regular: Option<String> }

    let data: UnsplashResp = resp.json().await.ok()?;
    for photo in data.results {
        let url = photo.urls.small.or(photo.urls.regular)?;
        if let Some(r) = download_and_store_image(word, &url, "unsplash").await {
            return Some(r);
        }
    }
    None
}

/// Pixabay 图片搜索（需要 API Key）
async fn search_image_pixabay(word: &str) -> Option<String> {
    let (_, keys) = read_api_keys();
    let key = keys.first()?;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .ok()?;

    let resp = client
        .get("https://pixabay.com/api/")
        .query(&[("key", key.as_str()), ("q", word), ("lang", "en"), ("image_type", "photo"), ("per_page", "5"), ("safesearch", "true")])
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
        .ok()?;

    #[derive(Deserialize)]
    struct PixabayResp { hits: Vec<PixabayHit> }
    #[derive(Deserialize)]
    struct PixabayHit { #[serde(rename = "webformatURL")] url: Option<String> }

    let data: PixabayResp = resp.json().await.ok()?;
    for hit in data.hits {
        if let Some(url) = hit.url {
            if let Some(r) = download_and_store_image(word, &url, "pixabay").await {
                return Some(r);
            }
        }
    }
    None
}

/// 360 图片搜索（国内可用，无需 API Key）
async fn search_image_360(word: &str) -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .ok()?;

    for query in &[format!("{} meaning", word), word.to_string()] {
        let resp = client
            .get("https://image.so.com/j")
            .query(&[("q", query.as_str()), ("src", "srp"), ("sn", "0"), ("pn", "5")])
            .header("User-Agent", "Mozilla/5.0")
            .send()
            .await
            .ok()?;

        #[derive(Deserialize)]
        struct SoResp { list: Option<Vec<SoItem>> }
        #[derive(Deserialize)]
        struct SoItem { img: Option<String>, thumb: Option<String> }

        if let Ok(data) = resp.json::<SoResp>().await {
            for item in data.list.unwrap_or_default().into_iter().take(3) {
                if let Some(url) = item.img.or(item.thumb) {
                    if let Some(r) = download_and_store_image(word, &url, "360").await {
                        return Some(r);
                    }
                }
            }
        }
    }
    None
}

/// Langeek 词汇图片（与 Python 版本一致）
async fn search_image_langeek(word: &str) -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .ok()?;

    let resp = client
        .get("https://api.langeek.co/v1/cs/en/word/")
        .query(&[("term", word), ("filter", ",inCategory,photo,withExamples")])
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .send()
        .await
        .ok()?;

    let data: Vec<serde_json::Value> = resp.json().await.ok()?;
    let item = data.into_iter().next()?;

    // 提取图片 URL（与 Python 版本 _extract_langeek_photo 逻辑一致）
    let photo_url = extract_langeek_photo(&item)?;
    if photo_url.is_empty() { return None; }

    download_and_store_image(word, &photo_url, "langeek").await
}

/// 从 Langeek 响应中提取图片 URL
fn extract_langeek_photo(item: &serde_json::Value) -> Option<String> {
    // 检查 translation.wordPhoto
    fn from_translation(tr: &serde_json::Value) -> Option<String> {
        let word_photo = tr.get("wordPhoto")?;
        for key in &["photoOriginal", "photoHD", "photoLarge", "photo"] {
            if let Some(url) = word_photo.get(key).and_then(|v| v.as_str()) {
                if !url.is_empty() { return Some(url.to_string()); }
            }
        }
        None
    }

    // 先检查 translation
    if let Some(tr) = item.get("translation") {
        if let Some(url) = from_translation(tr) {
            return Some(url);
        }
    }

    // 再检查 translations
    if let Some(translations) = item.get("translations").and_then(|v| v.as_object()) {
        for pos_list in translations.values() {
            if let Some(arr) = pos_list.as_array() {
                for tr in arr {
                    if let Some(url) = from_translation(tr) {
                        return Some(url);
                    }
                }
            }
        }
    }

    None
}

/// Bing 图片搜索
async fn search_image_bing(word: &str) -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?;

    let queries = [
        format!("{} meaning illustration", word),
        format!("{} definition picture", word),
    ];
    let re = regex::Regex::new(r#"murl&quot;:&quot;(https?://[^&]+?)&quot;"#).ok()?;

    for query in &queries {
        let resp = client
            .get("https://www.bing.com/images/search")
            .query(&[("q", query.as_str()), ("form", "HDRSC2"), ("first", "1")])
            .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
            .send()
            .await
            .ok()?;

        let html = resp.text().await.ok()?;

        // 提取图片 URL（与 Python 版本相同的正则）
        for cap in re.captures_iter(&html).take(3) {
            let url = &cap[1];
            // 跳过 SVG 和太小的图片
            if url.contains(".svg") || url.contains("favicon") {
                continue;
            }

            if let Some(result) = download_and_store_image(word, url, "bing").await {
                return Some(result);
            }
        }
    }

    None
}

/// 下载图片并上传到 Anki
async fn download_and_store_image(word: &str, url: &str, source: &str) -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?;

    let img_resp = client
        .get(url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .send()
        .await
        .ok()?;

    if !img_resp.status().is_success() { return None; }

    let data = img_resp.bytes().await.ok()?;
    if data.len() < 5000 { return None; } // 太小，可能不是有效图片

    let temp_dir = std::env::temp_dir().join("hugescreenshot");
    let _ = std::fs::create_dir_all(&temp_dir);
    let ext = if url.contains(".png") { "png" } else { "jpg" };
    let filename = format!("img_{}_{}_{}.{}", source, word, chrono::Utc::now().format("%H%M%S"), ext);
    let temp_path = temp_dir.join(&filename);

    std::fs::write(&temp_path, &data).ok()?;

    let params = serde_json::json!({
        "filename": &filename,
        "path": temp_path.to_string_lossy(),
    });

    invoke_anki("storeMediaFile", Some(params)).await.ok()?;
    info!("单词配图已上传: {} (来源: {})", filename, source);
    Some(format!("<img src=\"{}\">", filename))
}

/// 下载有道发音并上传到 Anki（与 Python 版本一致）
async fn download_youdao_audio(word: &str) -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?;

    // 有道发音 URL（type=2 美式发音）
    let url = format!(
        "http://dict.youdao.com/dictvoice?audio={}&type=2",
        urlencoding::encode(word)
    );

    let resp = client
        .get(&url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() { return None; }

    let audio_data = resp.bytes().await.ok()?;
    if audio_data.len() < 1000 { return None; } // 太小，可能是错误响应

    let temp_dir = std::env::temp_dir().join("hugescreenshot");
    let _ = std::fs::create_dir_all(&temp_dir);
    let filename = format!("youdao_{}.mp3", word);
    let temp_path = temp_dir.join(&filename);
    std::fs::write(&temp_path, &audio_data).ok()?;

    // 上传到 Anki
    let params = serde_json::json!({
        "filename": &filename,
        "path": temp_path.to_string_lossy(),
    });

    invoke_anki("storeMediaFile", Some(params)).await.ok()?;
    info!("有道发音已下载: {}", filename);
    Some(format!("[sound:{}]", filename))
}

/// 从有道 API 获取音标
async fn lookup_youdao_phonetic(word: &str) -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .ok()?;

    let url = format!(
        "https://dict.youdao.com/fsearch?q={}",
        urlencoding::encode(word)
    );

    let resp = client.get(&url).header("User-Agent", "Mozilla/5.0").send().await.ok()?;
    let xml = resp.text().await.ok()?;

    // 提取音标 <phonetic-symbol>...</phonetic-symbol>
    let re = regex::Regex::new(r"<phonetic-symbol><!\[CDATA\[(.*?)\]\]></phonetic-symbol>").ok()?;
    if let Some(cap) = re.captures(&xml) {
        let symbol = cap[1].trim().to_string();
        if !symbol.is_empty() {
            return Some(format!("/{}/", symbol));
        }
    }
    None
}

/// 通用音频下载并上传到 Anki
async fn download_generic_audio(word: &str, audio_url: &str) -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?;

    let resp = client.get(audio_url).send().await.ok()?;
    if !resp.status().is_success() { return None; }

    let audio_data = resp.bytes().await.ok()?;
    if audio_data.len() < 1000 { return None; }

    let temp_dir = std::env::temp_dir().join("hugescreenshot");
    let _ = std::fs::create_dir_all(&temp_dir);
    let filename = format!("audio_{}.mp3", word);
    let temp_path = temp_dir.join(&filename);
    std::fs::write(&temp_path, &audio_data).ok()?;

    let params = serde_json::json!({
        "filename": &filename,
        "path": temp_path.to_string_lossy(),
    });

    invoke_anki("storeMediaFile", Some(params)).await.ok()?;
    Some(format!("[sound:{}]", filename))
}

/// 从有道 API 获取释义（与 Python 版本一致）
async fn lookup_youdao_definition(word: &str) -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .ok()?;

    let url = format!(
        "https://dict.youdao.com/fsearch?q={}",
        urlencoding::encode(word)
    );

    let resp = client
        .get(&url)
        .header("User-Agent", "Mozilla/5.0")
        .send()
        .await
        .ok()?;

    let xml = resp.text().await.ok()?;

    // 提取释义（<content>...</content> 标签）
    let mut definitions = Vec::new();
    for cap in regex::Regex::new(r"<content><!\[CDATA\[(.*?)\]\]></content>").ok()?.captures_iter(&xml) {
        let def = cap[1].trim().to_string();
        if !def.is_empty() {
            definitions.push(def);
        }
    }

    if definitions.is_empty() { return None; }
    Some(definitions.join("<br>"))
}

// ============================================
// Tauri 命令
// ============================================

/// 检查 AnkiConnect 连接状态（原生 Rust 实现，不依赖 Sidecar）
#[derive(Debug, Serialize)]
pub struct AnkiConnectionStatus {
    pub connected: bool,
    pub version: Option<u32>,
    pub error: Option<String>,
}

#[tauri::command]
pub async fn check_anki_connection() -> HuGeResult<AnkiConnectionStatus> {
    info!("检查 AnkiConnect 连接状态（原生）...");

    match invoke_anki("version", None).await {
        Ok(result) => {
            let version = result.as_u64().map(|v| v as u32);
            info!("AnkiConnect 已连接，版本: {:?}", version);
            Ok(AnkiConnectionStatus {
                connected: true,
                version,
                error: None,
            })
        }
        Err(e) => {
            warn!("AnkiConnect 连接失败: {}", e);
            Ok(AnkiConnectionStatus {
                connected: false,
                version: None,
                error: Some(e.to_string()),
            })
        }
    }
}

/// 获取 Anki 牌组列表（原生 Rust 实现）
#[tauri::command]
pub async fn get_anki_decks() -> HuGeResult<Vec<String>> {
    info!("获取 Anki 牌组列表（原生）...");

    let result = invoke_anki("deckNames", None).await?;

    let decks: Vec<String> = serde_json::from_value(result).map_err(|e| {
        HuGeError::Unknown(format!("解析牌组列表失败: {}", e))
    })?;

    info!("获取到 {} 个牌组", decks.len());
    Ok(decks)
}

/// 获取 Anki 模板列表（原生 Rust 实现）
#[tauri::command]
pub async fn get_anki_models() -> HuGeResult<Vec<String>> {
    info!("获取 Anki 模板列表（原生）...");

    let result = invoke_anki("modelNames", None).await?;

    let models: Vec<String> = serde_json::from_value(result).map_err(|e| {
        HuGeError::Unknown(format!("解析模板列表失败: {}", e))
    })?;

    info!("获取到 {} 个模板", models.len());
    Ok(models)
}

/// 从文本中提取英文单词（原生 Rust 实现，不依赖 Sidecar）
#[tauri::command]
pub async fn extract_english_words_native(text: String) -> HuGeResult<Vec<String>> {
    debug!("提取英文单词（原生），文本长度: {}", text.len());

    // 常见停用词
    let stop_words: std::collections::HashSet<&str> = [
        "a", "an", "the", "is", "are", "was", "were", "be", "been", "being",
        "have", "has", "had", "do", "does", "did", "will", "would", "shall",
        "should", "may", "might", "must", "can", "could", "of", "in", "to",
        "for", "with", "on", "at", "from", "by", "as", "or", "and", "but",
        "if", "not", "no", "so", "it", "its", "he", "she", "we", "they",
        "me", "him", "her", "us", "them", "my", "his", "our", "your", "their",
        "this", "that", "these", "those", "am", "about", "up", "out", "all",
        "just", "also", "than", "more", "very", "too", "how", "what", "when",
        "where", "who", "which", "why", "then", "here", "there", "each",
    ].into_iter().collect();

    let mut words: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    // 使用正则提取英文单词（2个字母以上）
    for word in text.split(|c: char| !c.is_ascii_alphabetic()) {
        let lower = word.to_lowercase();
        if lower.len() >= 2
            && !stop_words.contains(lower.as_str())
            && !seen.contains(&lower)
            && lower.chars().all(|c| c.is_ascii_lowercase())
        {
            seen.insert(lower.clone());
            words.push(lower);
        }
    }

    info!("提取到 {} 个英文单词", words.len());
    Ok(words)
}

/// 上传媒体文件到 Anki（使用文件路径，无需 base64）
async fn store_media_file(filename: &str, path: &str) -> HuGeResult<String> {
    // AnkiConnect 的 storeMediaFile 支持 path 参数，直接读取本地文件
    let params = serde_json::json!({
        "filename": filename,
        "path": path,
    });

    let result = invoke_anki("storeMediaFile", Some(params)).await?;
    Ok(result.as_str().unwrap_or(filename).to_string())
}

/// 确保「虎哥单词卡」模板存在（与 Python 版本完全一致）
async fn ensure_word_card_model() -> HuGeResult<()> {
    let model_name = "虎哥单词卡";

    // 检查模板是否已存在
    let models_result = invoke_anki("modelNames", None).await?;
    let models: Vec<String> = serde_json::from_value(models_result).unwrap_or_default();

    if models.iter().any(|m| m == model_name) {
        debug!("单词卡模板 '{}' 已存在", model_name);
        return Ok(());
    }

    info!("创建单词卡模板 '{}'...", model_name);

    // 与 Python 版本 templates.py 完全一致的字段和模板
    let params = serde_json::json!({
        "modelName": model_name,
        "inOrderFields": ["单词", "音标", "中文释义", "单词发音", "单词配图", "绘本原图"],
        "isCloze": false,
        "cardTemplates": [
            {
                "Name": "英译中",
                "Front": "<div id=\"danci\">\n<div style='font-family: Arial; font-size: 20px;'>{{绘本原图}}</div>\n<div style='font-family: Arial;color:green; font-size: 60px;'>{{单词}}</div>\n<div style='font-family: Arial; font-size: 40px;'>{{音标}}</div>\n<div style='font-family: Arial; font-size: 20px;'>{{单词发音}}</div>\n</div>",
                "Back": "<div id=\"danci\">\n<div style='font-family: Arial;color:green; font-size: 60px;'>{{中文释义}}</div>\n<div style='font-family: Arial; font-size: 40px;'>{{单词}}</div>\n<div style='font-family: Arial; font-size: 30px;'>{{音标}}</div>\n<div style='font-family: Arial; font-size: 20px;'>{{单词发音}}</div>\n<div style='font-family: Arial; font-size: 20px;'>{{单词配图}}</div>\n<div style='font-family: Arial; font-size: 20px;'>{{绘本原图}}</div>\n</div>"
            },
            {
                "Name": "中译英",
                "Front": "<div id=\"danci\">\n<div style='font-family: Arial; font-size: 20px;'>{{单词配图}}</div>\n<div style='font-family: Arial;color:green; font-size: 60px;'>{{中文释义}}</div>\n</div>",
                "Back": "<div id=\"danci\">\n<div style='font-family: Arial; font-size: 20px;'>{{绘本原图}}</div>\n<div style='font-family: Arial;color:green; font-size: 60px;'>{{单词}}</div>\n<div style='font-family: Arial; font-size: 30px;'>{{音标}}</div>\n<div style='font-family: Arial; font-size: 20px;'>{{单词发音}}</div>\n<div style='font-family: Arial; font-size: 20px;'>{{中文释义}}</div>\n<div style='font-family: Arial; font-size: 20px;'>{{单词配图}}</div>\n</div>"
            }
        ],
        "css": ".card {\n    font-family: arial;\n    font-size: 24px;\n    color: black;\n    background-color: white;\n}\n\n#danci, #yinbiao {\n    text-align: center;\n    font-family: serif;\n    font-size: 30px;\n}\n\n.back {\n    text-align: left;\n    line-height: 80%;\n}\n\n.back img {\n    width: 720px;\n}\n\n.example {\n    font-size: 20px;\n    text-align: left;\n    line-height: 95%;\n}"
    });

    invoke_anki("createModel", Some(params)).await?;
    info!("单词卡模板 '{}' 创建成功", model_name);
    Ok(())
}

/// 确保牌组存在（如果不存在则创建）
async fn ensure_deck(deck_name: &str) -> HuGeResult<()> {
    let params = serde_json::json!({
        "deck": deck_name,
    });

    invoke_anki("createDeck", Some(params)).await?;
    debug!("牌组 '{}' 已确保存在", deck_name);
    Ok(())
}

/// 导入结果
#[derive(Debug, Serialize)]
pub struct NativeImportResult {
    pub success_count: u32,
    pub total_count: u32,
    pub results: Vec<NativeImportWordResult>,
}

#[derive(Debug, Serialize)]
pub struct NativeImportWordResult {
    pub word: String,
    pub success: bool,
    pub status: String,
    pub note_id: Option<i64>,
}

/// 批量导入单词到 Anki（原生 Rust 实现）
///
/// 使用简单的 Front/Back 模式创建笔记，不需要外部词典 API。
#[tauri::command]
pub async fn import_words_to_anki(
    words: Vec<String>,
    deck_name: String,
    screenshot_path: Option<String>,
) -> HuGeResult<NativeImportResult> {
    info!(
        "批量导入 {} 个单词到牌组 '{}'（原生）",
        words.len(),
        deck_name
    );

    // 确保牌组和模板存在
    ensure_deck(&deck_name).await?;
    ensure_word_card_model().await?;

    // 如果有截图，先上传到 Anki 媒体文件夹
    let screenshot_tag = if let Some(ref path) = screenshot_path {
        let filename = format!(
            "huge_screenshot_{}.png",
            chrono::Utc::now().format("%Y%m%d_%H%M%S")
        );
        match store_media_file(&filename, path).await {
            Ok(stored_name) => {
                info!("截图已上传到 Anki: {}", stored_name);
                Some(format!("<img src=\"{}\">", stored_name))
            }
            Err(e) => {
                warn!("截图上传失败，继续导入: {}", e);
                None
            }
        }
    } else {
        None
    };

    let mut results = Vec::with_capacity(words.len());
    let mut success_count = 0u32;

    for (i, word) in words.iter().enumerate() {
        info!("处理单词 {}/{}: '{}'", i + 1, words.len(), word);

        // 1. 从有道获取音标（备用：Free Dictionary API）
        let youdao_data = lookup_youdao_phonetic(word).await;
        let phonetic = if let Some(ref yd) = youdao_data {
            yd.clone()
        } else {
            // 有道失败，用 Free Dictionary
            lookup_word(word).await.map(|d| d.phonetic).unwrap_or_default()
        };

        // 2. 获取中文释义（优先有道，备用 Free Dictionary + MyMemory 翻译）
        let cn_definition = if let Some(youdao_def) = lookup_youdao_definition(word).await {
            youdao_def
        } else {
            // 有道失败，用 Free Dictionary 英文释义 + 翻译
            let dict = lookup_word(word).await;
            let en_def = dict.map(|d| d.definition).unwrap_or_default();
            if !en_def.is_empty() {
                let first_def = en_def.lines().next().unwrap_or(&en_def);
                let def_text = first_def.split(": ").nth(1).unwrap_or(first_def);
                translate_to_chinese(def_text).await.unwrap_or(en_def)
            } else {
                String::new()
            }
        };

        // 3. 下载有道发音（美式，备用：Free Dictionary）
        let audio_field = if let Some(audio) = download_youdao_audio(word).await {
            audio
        } else {
            // 有道失败，尝试 Free Dictionary 的发音 URL
            if let Some(dict) = lookup_word(word).await {
                if let Some(url) = dict.audio_url {
                    download_generic_audio(word, &url).await.unwrap_or_default()
                } else { String::new() }
            } else { String::new() }
        };

        // 4. 搜索单词配图（Bing 图片搜索，无需 API Key）
        let image_field = search_word_image(word).await.unwrap_or_default();

        debug!("单词 '{}': 音标={}, 释义长度={}, 发音={}, 配图={}", word, phonetic, cn_definition.len(), !audio_field.is_empty(), !image_field.is_empty());

        // 5. 创建 Anki 笔记
        let screenshot_field = screenshot_tag.clone().unwrap_or_default();

        let params = serde_json::json!({
            "note": {
                "deckName": deck_name,
                "modelName": "虎哥单词卡",
                "fields": {
                    "单词": word,
                    "音标": phonetic,
                    "中文释义": cn_definition,
                    "单词发音": audio_field,
                    "单词配图": image_field,
                    "绘本原图": screenshot_field,
                },
                "tags": ["虎哥截图"],
                "options": {
                    "allowDuplicate": false,
                    "duplicateScope": "deck",
                },
            }
        });

        match invoke_anki("addNote", Some(params)).await {
            Ok(result) => {
                let note_id = result.as_i64();
                success_count += 1;
                info!("单词 '{}' 导入成功，note_id: {:?}", word, note_id);
                results.push(NativeImportWordResult {
                    word: word.clone(),
                    success: true,
                    status: "已导入".to_string(),
                    note_id,
                });
            }
            Err(e) => {
                let error_msg = e.to_string();
                // 检查是否是重复卡片（重复也算成功）
                if error_msg.contains("duplicate") {
                    info!("单词 '{}' 已存在，跳过", word);
                    success_count += 1;
                    results.push(NativeImportWordResult {
                        word: word.clone(),
                        success: true,
                        status: "已存在".to_string(),
                        note_id: None,
                    });
                } else {
                    warn!("单词 '{}' 导入失败: {}", word, error_msg);
                    results.push(NativeImportWordResult {
                        word: word.clone(),
                        success: false,
                        status: format!("失败: {}", error_msg),
                        note_id: None,
                    });
                }
            }
        }
    }

    info!(
        "批量导入完成：成功 {}/{} 个",
        success_count,
        words.len()
    );

    Ok(NativeImportResult {
        success_count,
        total_count: words.len() as u32,
        results,
    })
}

/// 确保 Anki 模板存在（暴露给前端调用）
#[tauri::command]
pub async fn ensure_anki_model() -> HuGeResult<()> {
    ensure_word_card_model().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_english_words() {
        let text = "Hello world, this is a Test. The quick brown FOX jumps over the lazy dog.";
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(extract_english_words_native(text.to_string())).unwrap();

        // 应该过滤掉停用词（the, is, a, over）
        assert!(result.contains(&"hello".to_string()));
        assert!(result.contains(&"world".to_string()));
        assert!(result.contains(&"test".to_string()));
        assert!(result.contains(&"quick".to_string()));
        assert!(result.contains(&"brown".to_string()));
        assert!(result.contains(&"fox".to_string()));
        assert!(result.contains(&"jumps".to_string()));
        assert!(result.contains(&"lazy".to_string()));
        assert!(result.contains(&"dog".to_string()));
        // 停用词不应出现
        assert!(!result.contains(&"the".to_string()));
        assert!(!result.contains(&"is".to_string()));
        assert!(!result.contains(&"a".to_string()));
    }
}
