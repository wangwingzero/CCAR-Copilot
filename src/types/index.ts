/**
 * 类型定义统一导出
 *
 * 使用方式:
 * import type { CaptureResult, AnnotationObject, SidecarRequest } from '@/types'
 */

// 截图相关
export type {
  Rect,
  CaptureResult,
  MonitorInfo,
  WindowInfo,
  SelectionRegion,
  CaptureMode,
  CaptureState,
} from './screenshot'

// 标注相关
export type {
  AnnotationTool,
  Point,
  AnnotationStyle,
  AnnotationObject,
} from './annotation'
export { DEFAULT_ANNOTATION_STYLE } from './annotation'

// Sidecar 通信
export type {
  SidecarService,
  SidecarRequest,
  SidecarResponse,
  // OCR
  OcrRecognizeParams,
  OcrTextBox,
  OcrResult,
  // 翻译
  TranslateProvider,
  TranslateParams,
  TranslateResult,
  // Anki
  AnkiCardFields,
  AnkiAddCardParams,
  AnkiAddCardResult,
  // 网页爬取
  WebScrapeParams,
  WebScrapeResult,
  // 公文格式化
  DocumentFormatParams,
  DocumentFormatResult,
  // 录屏
  RecordStartParams,
  RecordingState,
  RecordResult,
  // 规章查询
  RegulationDocType,
  RegulationValidity,
  RegulationDocument,
  RegulationSearchParams,
  RegulationSearchResult,
  RegulationDownloadParams,
  RegulationDownloadResult,
  // 文件转换
  FileToMarkdownParams,
  FileToMarkdownResult,
  UrlToMarkdownParams,
  UrlToMarkdownResult,
  MarkdownToFileFormat,
  MarkdownToFileParams,
  MarkdownToFileResult,
  MarkdownFileToFileParams,
} from './sidecar'

// 配置
export type {
  GeneralConfig,
  HotkeyConfig,
  ScreenshotConfig,
  AnnotationConfig,
  OcrConfig,
  AnkiConfig,
  RecordingConfig,
  PinImageConfig,
  MouseHighlightConfig,
  MouseHighlightTheme,
  MouseHighlightThemeColors,
  WebToMarkdownConfig,
  FileToMarkdownConfig,
  FileToMarkdownEngine,
  NotificationConfig,
  UpdateConfig,
  AdvancedConfig,
  AppConfig,
} from './config'
export { DEFAULT_CONFIG, MOUSE_HIGHLIGHT_THEMES, MOUSE_HIGHLIGHT_LIMITS } from './config'

// 历史记录
export type {
  HistoryItem,
  HistoryMetadata,
  HistorySearchParams,
  HistorySearchResult,
  HistoryStats,
} from './history'

// 认证相关
export type {
  AuthUser,
  AuthSession,
  LoginRequest,
  SignUpRequest,
  AuthResponse,
  LicenseInfo,
  LicenseResponse,
  FeatureAccess,
  UsageStats,
  DeviceInfo,
  DeviceListResponse,
  UnbindDeviceResponse,
} from './auth'

// 支付相关
export type {
  PaymentMethod,
  PaymentStatus,
  SubscriptionPlan,
  PaymentOrder,
  CreateOrderRequest,
  CreateOrderResponse,
  QueryOrderResponse,
} from './payment'

// 规章本地索引（Rust Tantivy）- 使用 sidecar.ts 中的类型
// RegulationDocument, RegulationDocType, RegulationValidity 已从 sidecar 导出
export type {
  RegulationSearchRequest,
  RegulationSearchResponse,
  RegulationIndexStats,
  RegulationSortOrder,
  RegulationScanProgress,
  RegulationScanResponse,
  RegulationSyncCompareResponse,
  RegulationDiff,
} from './regulation'
