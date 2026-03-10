/**
 * 类型定义统一导出
 */

// 配置
export type {
  GeneralConfig,
  NotificationConfig,
  UpdateConfig,
  AdvancedConfig,
  AppConfig,
} from './config'
export { DEFAULT_CONFIG } from './config'

// 规章本地索引（Rust Tantivy）
export type {
  RegulationDocument,
  RegulationSearchRequest,
  RegulationSearchResponse,
  RegulationIndexStats,
  RegulationDocType,
  RegulationValidity,
  RegulationSortOrder,
  RegulationScanProgress,
  RegulationScanResponse,
  RegulationSyncCompareResponse,
  RegulationDiff,
} from './regulation'
