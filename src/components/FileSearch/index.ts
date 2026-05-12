/**
 * 文件搜索组件导出
 */

export { default as SearchDialog } from './SearchDialog.vue'
export { default as HighlightedText } from './HighlightedText.vue'

/** 搜索结果项（对应 Rust FileSearchResultItem） */
export interface FileSearchResult {
  name: string
  path: string
  size: number
  /** Unix 时间戳（秒） */
  modifiedSecs: number
  isDirectory: boolean
  score: number
  matchIndices: [number, number][]
}

/** 搜索响应（对应 Rust FileSearchResponse） */
export interface FileSearchResponse {
  results: FileSearchResult[]
  totalCount: number
  searchTimeMs: number
}

/** 索引状态响应（对应 Rust FileSearchStatusResponse） */
export interface FileSearchStatusResponse {
  status: 'idle' | 'scanning' | 'ready' | 'error'
  indexedFiles: number
  scannedFiles: number
  scanTimeMs: number
  isScanning: boolean
  error: string | null
}
