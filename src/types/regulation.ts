/**
 * 规章查询相关类型定义
 */

/** 规章文档 */
export interface RegulationDocument {
  /** 文档标题 */
  title: string
  /** 文号（如 CCAR-121-R7） */
  doc_number: string
  /** 有效性：有效、失效、废止 */
  validity: string
  /** 文档类型：regulation（规章）、normative（规范性文件） */
  doc_type: string
  /** 发布单位 */
  office_unit: string
  /** 签发日期（YYYY-MM-DD） */
  sign_date: string
  /** 发布日期（YYYY-MM-DD） */
  publish_date: string
  /** 原始 URL */
  url: string
  /** PDF 下载 URL */
  pdf_url?: string
  /** 本地文件路径 */
  file_path: string
  /** PDF 正文内容（用于全文搜索） */
  content?: string
}

/** 搜索请求参数 */
export interface RegulationSearchRequest {
  /** 搜索关键词 */
  query: string
  /** 有效性筛选：all, valid, invalid */
  validity?: string
  /** 文档类型：all, regulation, normative */
  doc_type?: string
  /** 返回数量限制 */
  limit?: number
  /** 排序方式：relevance, date_desc, date_asc, title_asc */
  sort?: string
}

/** 搜索响应 */
export interface RegulationSearchResponse {
  /** 搜索结果 */
  documents: RegulationDocument[]
  /** 结果总数 */
  total: number
  /** 搜索耗时（毫秒） */
  elapsed_ms: number
  /** 正文摘要（与 documents 等长，在线结果为 null） */
  snippets?: (string | null)[]
}

/** 索引统计信息 */
export interface RegulationIndexStats {
  /** 文档总数 */
  doc_count: number
  /** 索引路径 */
  index_path: string
  /** 是否已初始化 */
  initialized: boolean
}

/** 文档类型选项 */
export type RegulationDocType = 'all' | 'regulation' | 'normative' | 'standard'

/** 有效性选项 */
export type RegulationValidity = 'all' | 'valid' | 'invalid'

/** 排序方式选项 */
export type RegulationSortOrder = 'relevance' | 'date_desc' | 'date_asc' | 'title_asc'

/** 本地扫描进度 */
export interface RegulationScanProgress {
  /** 已扫描文件数 */
  scanned: number
  /** 发现的 PDF 文件总数 */
  total_found: number
  /** 新文件数（非重复） */
  new_files: number
  /** 重复文件数 */
  duplicates: number
  /** 已索引数 */
  indexed: number
  /** 需要 OCR 数 */
  needs_ocr: number
  /** 失败数 */
  failed: number
  /** 当前正在处理的文件名 */
  current_file: string | null
  /** 当前阶段：discovering / processing / ocr / done */
  phase: string
  /** OCR 已处理数（ocr 阶段） */
  ocr_processed?: number
  /** OCR 总数（ocr 阶段） */
  ocr_total?: number
}

/** 同步对比结果 */
export interface RegulationSyncCompareResponse {
  /** 在线总数 */
  online_total: number
  /** 匹配数（本地已有） */
  matched: number
  /** 新增规章列表 */
  new_regulations: RegulationDiff[]
  /** 状态变化规章列表 */
  changed_regulations: RegulationDiff[]
  /** 仅本地有 */
  local_only: number
  /** 本次同步下载成功数（前端补充） */
  downloaded?: number
  /** 本次同步下载失败数（前端补充） */
  download_failed?: number
}

/** 规章变化项 */
export interface RegulationDiff {
  /** 规章标题 */
  title: string
  /** 文号 */
  doc_number: string
  /** 在线有效性 */
  online_validity: string
  /** 本地有效性 */
  local_validity: string | null
  /** 变化类型：new / validity_changed */
  change_type: string
  /** 在线 URL */
  url: string
  /** 文档类型 */
  doc_type: string
  /** 发布日期 */
  publish_date: string
  /** 签发日期 */
  sign_date: string
  /** 发布单位 */
  office_unit: string
  /** PDF 下载 URL */
  pdf_url: string
}

/** 本地扫描结果 */
export interface RegulationScanResponse {
  /** 发现的 PDF 文件总数 */
  total_found: number
  /** 新文件数 */
  new_files: number
  /** 重复文件数 */
  duplicates: number
  /** 已索引数（文本提取直接索引） */
  indexed: number
  /** 需要 OCR 数 */
  needs_ocr: number
  /** 失败数 */
  failed: number
  /** 跳过的非 PDF 文件数 */
  skipped_non_pdf: number
  /** OCR 成功索引数 */
  ocr_success: number
  /** OCR 失败数 */
  ocr_failed: number
}
