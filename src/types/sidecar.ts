/**
 * Sidecar 通信协议类型定义
 * 对应 Rust: src-tauri/src/sidecar/
 * 对应 Python: python/huge_sidecar/
 */

/** Sidecar 服务名称 */
export type SidecarService =
  | 'ocr'
  | 'translate'
  | 'anki'
  | 'web'
  | 'record'
  | 'document'
  | 'regulation'
  | 'converter'

/** Sidecar 请求 */
export interface SidecarRequest {
  /** 请求 ID (UUID) */
  id: string
  /** 服务名称 */
  service: SidecarService
  /** 方法名称 */
  method: string
  /** 参数 */
  params: Record<string, unknown>
}

/** Sidecar 响应 */
export interface SidecarResponse<T = unknown> {
  /** 请求 ID (与请求匹配) */
  id: string
  /** 是否成功 */
  success: boolean
  /** 成功时的结果 */
  result?: T
  /** 失败时的错误信息 */
  error?: string
}

// ============================================
// OCR 服务
// ============================================

/** OCR 识别请求参数 */
export interface OcrRecognizeParams {
  /** 图像文件路径 */
  imagePath: string
  /** 语言 (可选) */
  language?: string
}

/** OCR 文本框 */
export interface OcrTextBox {
  /** 识别的文字 */
  text: string
  /** 置信度 (0-1) */
  confidence: number
  /** 边界框坐标 [[x1,y1], [x2,y2], [x3,y3], [x4,y4]] */
  box: [number, number][]
}

/** OCR 识别结果 */
export interface OcrResult {
  /** 完整文本 */
  text: string
  /** 文本框列表 */
  boxes: OcrTextBox[]
  /** 耗时 (秒) */
  elapse: number
}

// ============================================
// 翻译服务
// ============================================

/** 翻译提供商 */
export type TranslateProvider = 'google' | 'deepl' | 'baidu'

/** 翻译请求参数 */
export interface TranslateParams {
  /** 待翻译文本 */
  text: string
  /** 目标语言 */
  targetLang: string
  /** 源语言 (可选，自动检测) */
  sourceLang?: string
  /** 提供商 */
  provider?: TranslateProvider
}

/** 翻译结果 */
export interface TranslateResult {
  /** 翻译后的文本 */
  translatedText: string
  /** 检测到的源语言 */
  sourceLang: string
  /** 目标语言 */
  targetLang: string
  /** 使用的提供商 */
  provider: TranslateProvider
}

// ============================================
// Anki 服务
// ============================================

/** Anki 卡片字段 */
export interface AnkiCardFields {
  [fieldName: string]: string
}

/** Anki 制卡请求参数 */
export interface AnkiAddCardParams {
  /** 牌组名称 */
  deck: string
  /** 笔记类型 */
  noteType: string
  /** 字段内容 */
  fields: AnkiCardFields
  /** 标签 */
  tags?: string[]
  /** 附件图片路径 */
  imagePath?: string
  /** 附件音频路径 */
  audioPath?: string
}

/** Anki 制卡结果 */
export interface AnkiAddCardResult {
  /** 卡片 ID */
  cardId: number
  /** 是否为重复卡片 */
  duplicate: boolean
}

// ============================================
// 网页爬取服务
// ============================================

/** 网页爬取请求参数 */
export interface WebScrapeParams {
  /** 目标 URL */
  url: string
  /** 是否下载图片 */
  downloadImages?: boolean
  /** 输出目录 */
  outputDir?: string
}

/** 网页爬取结果 */
export interface WebScrapeResult {
  /** Markdown 内容 */
  markdown: string
  /** 标题 */
  title: string
  /** 保存的图片路径列表 */
  images: string[]
  /** 输出文件路径 */
  outputPath?: string
}

// ============================================
// 公文格式化服务
// ============================================

/** 公文格式化请求参数 */
export interface DocumentFormatParams {
  /** 输入文件路径 */
  inputPath: string
  /** 输出文件路径 (可选) */
  outputPath?: string
}

/** 公文格式化结果 */
export interface DocumentFormatResult {
  /** 输出文件路径 */
  outputPath: string
  /** 修复的问题列表 */
  issues: string[]
  /** 是否完全符合标准 */
  compliant: boolean
}

// ============================================
// 录屏服务
// ============================================

/** 录屏请求参数 */
export interface RecordStartParams {
  /** 录制区域 (null 表示全屏) */
  region?: {
    x: number
    y: number
    width: number
    height: number
  }
  /** 帧率 */
  fps?: number
  /** 是否录制系统音频 */
  systemAudio?: boolean
  /** 是否录制麦克风 */
  micAudio?: boolean
  /** 输出路径 */
  outputPath: string
}

/** 录屏状态 */
export type RecordingState = 'idle' | 'recording' | 'paused' | 'encoding'

/** 录屏结果 */
export interface RecordResult {
  /** 输出文件路径 */
  outputPath: string
  /** 时长 (秒) */
  duration: number
  /** 文件大小 (字节) */
  fileSize: number
}

// ============================================
// 规章查询服务
// ============================================

/** 规章文档类型 */
export type RegulationDocType = 'all' | 'regulation' | 'normative' | 'standard'

/** 规章有效性 */
export type RegulationValidity = 'all' | 'valid' | 'invalid'

/** 规章文档 */
export interface RegulationDocument {
  /** 文档标题 */
  title: string
  /** 详情页 URL */
  url: string
  /** 有效性状态 ("有效", "失效", "废止") */
  validity: string
  /** 文号 */
  doc_number: string
  /** 发布单位 */
  office_unit: string
  /** 文档类型 ("regulation" 规章, "normative" 规范性文件) */
  doc_type: string
  /** 签发日期 */
  sign_date?: string
  /** 发布日期 */
  publish_date?: string
  /** 字号 */
  file_number?: string
  /** PDF 附件链接 */
  pdf_url?: string
  /** 本地文件路径 */
  file_path?: string
}

/** 规章搜索参数 */
export interface RegulationSearchParams {
  /** 搜索关键词 */
  keyword?: string
  /** 文档类型 */
  doc_type?: RegulationDocType
  /** 有效性 */
  validity?: RegulationValidity
  /** 起始日期 (YYYY-MM-DD) */
  start_date?: string
  /** 结束日期 (YYYY-MM-DD) */
  end_date?: string
}

/** 规章搜索结果 */
export interface RegulationSearchResult {
  /** 文档列表 */
  documents: RegulationDocument[]
  /** 总数 */
  total: number
}

/** 规章下载参数 */
export interface RegulationDownloadParams {
  /** 文档对象 */
  document: RegulationDocument
  /** 保存路径 (可选) */
  save_path?: string
}

/** 规章下载结果 */
export interface RegulationDownloadResult {
  /** 是否成功 */
  success: boolean
  /** 文件路径 */
  file_path: string
  /** 文件类型 (pdf, doc, docx, txt) */
  file_type?: string
  /** 错误信息 */
  error?: string
}

// ============================================
// 文件转换服务
// ============================================

/** 文件转 Markdown 参数 */
export interface FileToMarkdownParams {
  /** 文件路径 */
  file_path: string
  /** 转换选项 */
  options?: {
    /** 是否启用 OCR (用于图片) */
    enable_ocr?: boolean
  }
}

/** 文件转 Markdown 结果 */
export interface FileToMarkdownResult {
  /** 是否成功 */
  success: boolean
  /** 原文件路径 */
  file_path: string
  /** Markdown 内容 */
  markdown: string
  /** 文档标题 */
  title: string
  /** 耗时 (秒) */
  elapse: number
}

/** 网页转 Markdown 参数 */
export interface UrlToMarkdownParams {
  /** 网页 URL */
  url: string
  /** 转换选项 */
  options?: {
    /** 抓取引擎 */
    engine?: 'auto' | 'trafilatura' | 'browser'
    /** 等待策略 */
    wait_until?: 'load' | 'domcontentloaded' | 'networkidle'
    /** 超时时间 (毫秒) */
    timeout?: number
    /** 正文选择器 */
    content_selector?: string
    /** 等待特定元素 */
    wait_for_selector?: string
    /** 是否保存图片 */
    save_images?: boolean
    /** 图片保存目录 */
    images_dir?: string
  }
}

/** 网页转 Markdown 结果 */
export interface UrlToMarkdownResult {
  /** 是否成功 */
  success: boolean
  /** 原 URL */
  url: string
  /** 页面标题 */
  title: string
  /** Markdown 内容 */
  markdown: string
  /** 图片列表 */
  images: Array<{
    url: string
    local_path: string
    alt: string
  }>
  /** 耗时 (秒) */
  elapse: number
}

/** Markdown 转文件格式 */
export type MarkdownToFileFormat = 'docx' | 'pdf' | 'html' | 'odt' | 'rtf'

/** Markdown 转文件参数 */
export interface MarkdownToFileParams {
  /** Markdown 内容 */
  markdown: string
  /** 输出文件路径 */
  output_path: string
  /** 输出格式 */
  format: MarkdownToFileFormat
  /** 转换选项 */
  options?: {
    /** 参考文档模板 (用于 docx) */
    reference_doc?: string
    /** CSS 文件路径 (用于 html/pdf) */
    css?: string
    /** 是否生成目录 */
    toc?: boolean
    /** 是否生成独立文档 */
    standalone?: boolean
  }
}

/** Markdown 转文件结果 */
export interface MarkdownToFileResult {
  /** 是否成功 */
  success: boolean
  /** 输出文件路径 */
  output_path: string
  /** 输出格式 */
  format: string
  /** 耗时 (秒) */
  elapse: number
}

/** Markdown 文件转文件参数 */
export interface MarkdownFileToFileParams {
  /** Markdown 文件路径 */
  markdown_path: string
  /** 输出文件路径 */
  output_path: string
  /** 输出格式 */
  format: MarkdownToFileFormat
  /** 转换选项 */
  options?: MarkdownToFileParams['options']
}
