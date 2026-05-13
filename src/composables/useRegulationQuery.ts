/**
 * 规章查询 Composable
 *
 * 提供规章搜索和下载功能的状态管理。
 * 下载成功后自动添加到本地 Tantivy 索引。
 */

import { ref, computed, reactive, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useRegulationStore } from '@/stores/regulation'
import { useRegulationIndex } from './useRegulationIndex'
import type {
  RegulationDocument,
  RegulationSearchResponse,
  RegulationDocType,
  RegulationValidity,
  RegulationScanResponse,
  RegulationSyncCompareResponse,
} from '@/types'

/** 日期筛选选项 */
export type DateFilter = 'all' | '1day' | '7days' | '30days' | 'custom'

/** 搜索状态 */
interface RegulationSearchState {
  /** 搜索关键词 */
  keyword: string
  /** 文档类型 */
  docType: RegulationDocType
  /** 有效性筛选 */
  validity: RegulationValidity
  /** 日期筛选 */
  dateFilter: DateFilter
  /** 自定义起始日期 */
  startDate: string
  /** 自定义结束日期 */
  endDate: string
}

interface RegulationLocalSearchOptions {
  scanFolders?: string[]
}

interface OcrPendingResult {
  success: number
  failed: number
  total: number
  cancelled?: boolean
}

/** localStorage 持久化 key */
const REGULATION_SEARCH_PREFS_KEY = 'regulation-search-prefs'

/** 需要持久化的搜索偏好字段 */
interface RegulationSearchPrefs {
  keyword: string
  docType: RegulationDocType
  validity: RegulationValidity
  dateFilter: DateFilter
  startDate: string
  endDate: string
}

/** 从 localStorage 加载搜索偏好 */
function loadSearchPrefs(): Partial<RegulationSearchPrefs> {
  try {
    const raw = localStorage.getItem(REGULATION_SEARCH_PREFS_KEY)
    if (raw) {
      return JSON.parse(raw)
    }
  } catch (e) {
    console.warn('[RegulationQuery] 加载搜索偏好失败:', e)
  }
  return {}
}

/** 保存搜索偏好到 localStorage */
function saveSearchPrefs(prefs: RegulationSearchPrefs): void {
  try {
    localStorage.setItem(REGULATION_SEARCH_PREFS_KEY, JSON.stringify(prefs))
  } catch (e) {
    console.warn('[RegulationQuery] 保存搜索偏好失败:', e)
  }
}

const INVALID_VALIDITY_LABELS = ['失效', '废止', '历史版本'] as const

function isInvalidValidityLabel(value: string): boolean {
  const normalized = value.trim()
  return INVALID_VALIDITY_LABELS.some(label => label === normalized)
}

function inferDocumentValidity(doc: RegulationDocument): string {
  const explicit = doc.validity.trim()
  if (isInvalidValidityLabel(explicit)) return explicit

  const searchableText = [doc.title, doc.doc_number, doc.file_path, doc.url].join(' ')
  return INVALID_VALIDITY_LABELS.find(label => searchableText.includes(label)) ?? explicit
}

function isInvalidDocument(doc: RegulationDocument): boolean {
  return isInvalidValidityLabel(inferDocumentValidity(doc))
}

function filterBySelectedValidity(
  docs: RegulationDocument[],
  validity: RegulationValidity
): RegulationDocument[] {
  if (validity === 'all') return docs
  const wantInvalid = validity === 'invalid'
  return docs.filter(doc => isInvalidDocument(doc) === wantInvalid)
}

export function useRegulationQuery() {
  const regulationStore = useRegulationStore()
  const regulationIndex = useRegulationIndex()

  // ============================================
  // State（本地，每个组件实例独立）
  // ============================================

  /** 是否正在加载 */
  const isLoading = ref(false)

  /** 是否正在初始化服务 */
  const isInitializing = ref(false)

  /** 是否正在下载 */
  const isDownloading = ref(false)

  /** 错误信息 */
  const error = ref<string | null>(null)

  /** 搜索结果 */
  const results = ref<RegulationDocument[]>([])

  // 加载持久化的搜索偏好
  const savedPrefs = loadSearchPrefs()

  /** 搜索状态 */
  const searchState = reactive<RegulationSearchState>({
    keyword: savedPrefs.keyword || '',
    docType: savedPrefs.docType || 'all',
    validity: savedPrefs.validity || 'all',
    dateFilter: savedPrefs.dateFilter || 'all',
    startDate: savedPrefs.startDate || '',
    endDate: savedPrefs.endDate || '',
  })

  // 监听搜索偏好变化，自动持久化
  watch(
    () => ({
      keyword: searchState.keyword,
      docType: searchState.docType,
      validity: searchState.validity,
      dateFilter: searchState.dateFilter,
      startDate: searchState.startDate,
      endDate: searchState.endDate,
    }),
    newPrefs => {
      saveSearchPrefs(newPrefs)
    },
    { deep: true }
  )

  /** 当前下载的文档 */
  const downloadingDoc = ref<RegulationDocument | null>(null)

  // ============================================
  // 从 Pinia Store 引用的全局状态（组件切换不丢失）
  // ============================================

  /** 扫描状态（全局） */
  const isScanning = computed(() => regulationStore.isScanning)

  /** 扫描进度（全局） */
  const scanProgress = computed(() => regulationStore.scanProgress)

  /** 同步对比状态（全局） */
  const isSyncing = computed(() => regulationStore.isSyncing)

  /** 同步对比结果（全局） */
  const syncResult = computed(() => regulationStore.syncResult)

  /** 数据库同步状态（全局） */
  const dbSyncStatus = computed(() => regulationStore.dbSyncStatus)

  // ============================================
  // Computed
  // ============================================

  /** 有效文档数量 */
  const validCount = computed(() => results.value.filter(doc => !isInvalidDocument(doc)).length)

  /** 失效文档数量 */
  const invalidCount = computed(() => results.value.filter(isInvalidDocument).length)

  /** 是否可以搜索（只检查是否正在加载，sidecar 会自动初始化） */
  const canSearch = computed(() => !isLoading.value && !isInitializing.value)

  /** 是否可以下载 */
  const canDownload = computed(() => !isDownloading.value && !isInitializing.value)

  /** 本地索引文档数量 */
  const localDocCount = computed(() => regulationIndex.docCount.value)

  /** 本地索引是否已初始化 */
  const isLocalIndexReady = computed(() => regulationIndex.isInitialized.value)

  /** 本地搜索耗时（毫秒） */
  const localSearchElapsedMs = computed(() => regulationIndex.searchElapsedMs.value)

  /** 是否正在本地搜索 */
  const isLocalSearching = computed(() => regulationIndex.isSearching.value)

  /** 摘要预览开关（localStorage 持久化） */
  const SNIPPET_ENABLED_KEY = 'regulation-snippet-enabled'
  const showSnippets = ref(
    (() => {
      try {
        const stored = localStorage.getItem(SNIPPET_ENABLED_KEY)
        return stored === null ? true : stored === 'true'
      } catch {
        return true
      }
    })()
  )
  watch(showSnippets, v => {
    try {
      localStorage.setItem(SNIPPET_ENABLED_KEY, String(v))
    } catch {
      /* ignore */
    }
  })

  /** 获取指定文档的摘要 */
  function getSnippet(docUrl: string): string | undefined {
    return regulationIndex.snippetMap.value.get(docUrl)
  }

  // ============================================
  // Methods
  // ============================================

  /**
   * 初始化本地索引
   * 应在组件挂载时调用
   */
  async function initLocalIndex(): Promise<boolean> {
    return await regulationIndex.initIndex()
  }

  /**
   * 刷新本地索引统计
   */
  async function refreshLocalIndexStats(): Promise<void> {
    await regulationIndex.refreshStats()
  }

  /**
   * 本地搜索规章（毫秒级响应）
   * 搜索已下载并索引的文档
   */
  async function searchLocal(
    options: RegulationLocalSearchOptions = {}
  ): Promise<RegulationDocument[]> {
    if (!searchState.keyword.trim()) {
      results.value = []
      return []
    }

    const { startDate, endDate } = getDateRange()

    const docs = await regulationIndex.localSearch(searchState.keyword, {
      validity: searchState.validity,
      docType: searchState.docType,
      startDate,
      endDate,
      limit: 100,
      scanFolders: options.scanFolders,
    })

    // 更新结果到 UI（标题匹配优先）
    results.value = sortByTitleMatch(
      filterBySelectedValidity(docs, searchState.validity),
      searchState.keyword
    )

    return results.value
  }

  /**
   * 混合搜索：先本地后在线，合并去重
   * 本地结果毫秒级返回先展示，在线结果补充后合并
   */
  async function searchHybrid(options: RegulationLocalSearchOptions = {}): Promise<void> {
    if (isLoading.value) {
      return
    }

    error.value = null
    let localResults: RegulationDocument[] = []

    // 1. 先尝试本地搜索（毫秒级）
    if (searchState.keyword.trim()) {
      localResults = await searchLocal(options)
      if (localResults.length > 0) {
        results.value = localResults
        console.warn(
          `[RegulationQuery] 本地搜索返回 ${localResults.length} 条结果，耗时 ${localSearchElapsedMs.value}ms`
        )
      }
    }

    if (options.scanFolders?.length) {
      return
    }

    // 2. 发起在线搜索
    try {
      isLoading.value = true

      const { startDate, endDate } = getDateRange()

      const onlineResult = await invoke<RegulationSearchResponse>('regulation_online_search', {
        keyword: searchState.keyword,
        docType: searchState.docType === 'all' ? null : searchState.docType,
        validity: searchState.validity === 'all' ? null : searchState.validity,
        startDate: startDate || null,
        endDate: endDate || null,
      })

      // 3. 合并本地 + 在线结果，按 URL 或标题去重
      const existingUrls = new Set(localResults.map((d: RegulationDocument) => d.url))
      const existingTitles = new Set(localResults.map((d: RegulationDocument) => d.title))
      const filteredOnlineDocs = filterBySelectedValidity(onlineResult.documents, searchState.validity)
      const newOnlineDocs = filteredOnlineDocs.filter(
        (d: RegulationDocument) => !existingUrls.has(d.url) && !existingTitles.has(d.title)
      )

      results.value = sortByTitleMatch([...localResults, ...newOnlineDocs], searchState.keyword)
      console.warn(
        `[RegulationQuery] 智能搜索: 本地 ${localResults.length} + 在线新增 ${newOnlineDocs.length} = 总计 ${results.value.length}`
      )
    } catch (err) {
      // 在线搜索失败不影响本地结果
      if (localResults.length === 0) {
        error.value = err instanceof Error ? err.message : String(err)
      } else {
        console.warn('[RegulationQuery] 在线搜索失败，仅显示本地结果:', err)
      }
    } finally {
      isLoading.value = false
    }
  }

  /**
   * 按标题匹配度重新排序结果
   * 标题中包含搜索关键词的文档优先显示，位置越靠前优先级越高
   */
  function sortByTitleMatch(docs: RegulationDocument[], keyword: string): RegulationDocument[] {
    if (!keyword.trim()) return docs
    const kw = keyword.trim().toLowerCase()
    return [...docs].sort((a, b) => {
      const aIdx = a.title.toLowerCase().indexOf(kw)
      const bIdx = b.title.toLowerCase().indexOf(kw)
      // 都不包含 → 保持原序
      if (aIdx === -1 && bIdx === -1) return 0
      // 只有一个包含 → 包含的排前面
      if (aIdx === -1) return 1
      if (bIdx === -1) return -1
      // 都包含 → 关键词位置越靠前越好
      if (aIdx !== bIdx) return aIdx - bIdx
      // 位置相同 → 标题越短越精确
      return a.title.length - b.title.length
    })
  }

  /**
   * 计算日期范围
   */
  function getDateRange(): { startDate: string; endDate: string } {
    const now = new Date()
    let startDate = ''
    const endDate = formatDate(now)

    switch (searchState.dateFilter) {
      case '1day':
        startDate = formatDate(new Date(now.getTime() - 1 * 24 * 60 * 60 * 1000))
        break
      case '7days':
        startDate = formatDate(new Date(now.getTime() - 7 * 24 * 60 * 60 * 1000))
        break
      case '30days':
        startDate = formatDate(new Date(now.getTime() - 30 * 24 * 60 * 60 * 1000))
        break
      case 'custom':
        return {
          startDate: searchState.startDate,
          endDate: searchState.endDate,
        }
      default:
        return { startDate: '', endDate: '' }
    }

    return { startDate, endDate }
  }

  /**
   * 格式化日期为 YYYY-MM-DD
   */
  function formatDate(date: Date): string {
    const year = date.getFullYear()
    const month = String(date.getMonth() + 1).padStart(2, '0')
    const day = String(date.getDate()).padStart(2, '0')
    return `${year}-${month}-${day}`
  }

  /**
   * 搜索规章（Rust 原生在线搜索，不依赖 Python Sidecar）
   */
  async function search(): Promise<void> {
    if (isLoading.value) {
      return
    }

    try {
      isLoading.value = true
      error.value = null

      const { startDate, endDate } = getDateRange()

      // 使用 Rust 原生在线搜索命令（替代 Python Sidecar）
      const result = await invoke<RegulationSearchResponse>('regulation_online_search', {
        keyword: searchState.keyword,
        docType: searchState.docType === 'all' ? null : searchState.docType,
        validity: searchState.validity === 'all' ? null : searchState.validity,
        startDate: startDate || null,
        endDate: endDate || null,
      })

      results.value = sortByTitleMatch(
        filterBySelectedValidity(result.documents, searchState.validity),
        searchState.keyword
      )
    } catch (err) {
      error.value = err instanceof Error ? err.message : String(err)
      results.value = []
    } finally {
      isLoading.value = false
    }
  }

  /**
   * 下载文档
   * 下载成功后自动添加到本地索引
   */
  async function download(document: RegulationDocument): Promise<string | null> {
    if (isDownloading.value) {
      return null
    }

    try {
      isDownloading.value = true
      downloadingDoc.value = document
      error.value = null

      const result = await invoke<{
        success: boolean
        file_path: string
        file_type: string
        error?: string
      }>('regulation_download_single', {
        request: {
          document,
          preferAttachment: true,
        },
      })

      if (result.success) {
        // 下载成功后，添加到本地索引
        try {
          // 构建本地索引文档（添加文件路径）
          const indexDoc = {
            ...document,
            file_path: result.file_path,
            content: '', // 暂不提取正文
          }
          await regulationIndex.addDocument(indexDoc)
          console.warn(`[RegulationQuery] 文档已添加到本地索引: ${document.title}`)
        } catch (indexErr) {
          // 索引失败不影响下载结果
          console.warn('[RegulationQuery] 添加到本地索引失败:', indexErr)
        }

        return result.file_path
      } else {
        error.value = result.error || '下载失败'
        return null
      }
    } catch (err) {
      error.value = err instanceof Error ? err.message : String(err)
      return null
    } finally {
      isDownloading.value = false
      downloadingDoc.value = null
    }
  }

  /**
   * 批量下载文档
   */
  async function downloadBatch(
    documents: RegulationDocument[],
    onProgress?: (current: number, total: number, doc: RegulationDocument) => void
  ): Promise<{ success: number; failed: number }> {
    let success = 0
    let failed = 0

    for (let i = 0; i < documents.length; i++) {
      const doc = documents[i]
      onProgress?.(i + 1, documents.length, doc)

      const result = await download(doc)
      if (result) {
        success++
      } else {
        failed++
      }
    }

    return { success, failed }
  }

  /**
   * 使用 Rust 原生批量下载
   * 利用 Rust crawler + Tantivy 索引
   */
  async function downloadBatchNative(
    documents: RegulationDocument[],
    scanFolders: string[] = []
  ): Promise<{ success: number; skipped: number; failed: number }> {
    try {
      isLoading.value = true
      error.value = null

      // 构建下载项
      const items = documents.map(doc => ({
        url: doc.pdf_url || doc.url,
        title: doc.title,
        doc_number: doc.doc_number || '',
        doc_type: doc.doc_type,
        source_url: doc.url,
      }))

      // 调用 Rust 批量下载命令
      const result = await invoke<{
        success: number
        skipped: number
        failed: number
        failed_urls: string[]
      }>('regulation_batch_download', {
        request: { items },
      })

      // 下载完成后，触发 PDF 文本提取和索引
      await processPendingFiles(10, scanFolders)

      return {
        success: result.success,
        skipped: result.skipped,
        failed: result.failed,
      }
    } catch (err) {
      error.value = err instanceof Error ? err.message : String(err)
      throw err
    } finally {
      isLoading.value = false
    }
  }

  /**
   * 处理待提取文件（PDF 文本提取 + 索引）
   * 应在批量下载后调用
   */
  async function processPendingFiles(
    batchSize = 10,
    scanFolders: string[] = []
  ): Promise<{
    processed: number
    indexed: number
    needs_ocr: number
    failed: number
  }> {
    try {
      const result = await invoke<{
        processed: number
        indexed: number
        needs_ocr: number
        failed: number
      }>('regulation_process_pending', {
        batchSize,
        scanFolders,
      })

      // 刷新本地索引统计
      await regulationIndex.refreshStats()

      return result
    } catch (err) {
      error.value = err instanceof Error ? err.message : String(err)
      throw err
    }
  }

  /**
   * 扫描本地目录，将 PDF / TXT 文件入库 + 入索引
   *
   * 扫描状态保存在 Pinia Store 中，组件切换不会丢失进度。
   *
   * @param dirPath 要扫描的目录路径
   * @param recursive 是否递归扫描子目录，默认 true
   */
  async function scanLocalDir(
    dirPath: string,
    recursive = true
  ): Promise<RegulationScanResponse | null> {
    if (regulationStore.isScanning) {
      return null
    }

    error.value = null

    // 使用全局 store 管理扫描生命周期
    const result = await regulationStore.startScan(dirPath, recursive, async () => {
      if (!regulationIndex.isInitialized.value) {
        await regulationIndex.initIndex()
      }
    })

    // 扫描完成后刷新索引统计
    if (result) {
      await regulationIndex.refreshStats()
    }

    // 错误状态统一从 store.scanError 获取，不再重复复制到 composable 的 error
    return result
  }

  /**
   * 全盘扫描所有 PDF / TXT 文件
   *
   * 遍历 Windows 所有盘符，递归收集 PDF / TXT 并入库索引。
   * 扫描状态保存在 Pinia Store 中，组件切换不会丢失进度。
   */
  async function scanAllDrives(): Promise<RegulationScanResponse | null> {
    if (regulationStore.isScanning) {
      return null
    }

    error.value = null

    const result = await regulationStore.startFullScan(async () => {
      if (!regulationIndex.isInitialized.value) {
        await regulationIndex.initIndex()
      }
    })

    if (result) {
      await regulationIndex.refreshStats()
    }

    return result
  }

  /**
   * 同步对比：从 CAAC 官网全量爬取规章列表，与本地数据库对比
   *
   * @param docType 文档类型：all, regulation, normative
   * @param maxPages 最大爬取页数
   */
  async function syncCompare(
    docType: string = 'all',
    maxPages: number = 20,
    downloadMissing = false
  ): Promise<RegulationSyncCompareResponse | null> {
    if (regulationStore.isSyncing) {
      return null
    }

    try {
      regulationStore.startSyncCompare()
      error.value = null

      const compareResult = await invoke<RegulationSyncCompareResponse>(
        'regulation_sync_compare_online',
        {
          docType,
          maxPages,
        }
      )

      if (downloadMissing && compareResult.new_regulations.length > 0) {
        let downloaded = 0
        let downloadFailed = 0

        const missingDocs: RegulationDocument[] = compareResult.new_regulations.map(reg => ({
          title: reg.title,
          doc_number: reg.doc_number || '',
          validity: reg.online_validity || '',
          doc_type: reg.doc_type as RegulationDocType,
          office_unit: reg.office_unit || '',
          sign_date: reg.sign_date || '',
          publish_date: reg.publish_date || '',
          url: reg.url,
          pdf_url: reg.pdf_url || undefined,
          file_path: '',
          content: '',
        }))

        for (const doc of missingDocs) {
          const filePath = await download(doc)
          if (filePath) {
            downloaded += 1
          } else {
            downloadFailed += 1
          }
        }

        compareResult.downloaded = downloaded
        compareResult.download_failed = downloadFailed
        await refreshDbStatus()
        await regulationIndex.refreshStats()
      }

      regulationStore.finishSyncCompare(compareResult)

      console.warn(
        `[RegulationQuery] 同步完成: 在线 ${compareResult.online_total}, 匹配 ${compareResult.matched}, 新增 ${compareResult.new_regulations.length}, 下载 ${compareResult.downloaded ?? 0}`
      )

      return compareResult
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      if (message.includes('中止') || message.toLowerCase().includes('cancel')) {
        console.warn('[RegulationQuery] 同步已中止')
        error.value = null
        regulationStore.finishSyncCompare(null)
        return null
      }
      error.value = message
      console.error('[RegulationQuery] 同步对比失败:', err)
      regulationStore.finishSyncCompare(null)
      return null
    }
  }

  async function cancelSyncCompare(): Promise<boolean> {
    try {
      await invoke<boolean>('regulation_cancel_sync_compare')
      return true
    } catch (err) {
      error.value = err instanceof Error ? err.message : String(err)
      console.error('[RegulationQuery] 中止同步失败:', err)
      return false
    }
  }

  /**
   * 获取数据库同步状态
   */
  async function refreshDbStatus(scanFolders: string[] = []): Promise<void> {
    await regulationStore.refreshDbStatus(scanFolders)
  }

  /**
   * OCR 处理队列中待处理的 PDF 文件（纯 Rust 实现）
   *
   * 流程：
   * 1. 调用 Rust 后端的 regulation_ocr_pending 命令
   * 2. Rust 端使用 pdfium + PP-OCRv4 进行 PDF OCR
   * 3. 自动将 OCR 结果写入 Tantivy 索引
   *
   * 注意：不再依赖 Python sidecar，所有 OCR 工作在 Rust 端完成。
   */
  async function ocrPendingFiles(
    batchSize: number = 5,
    onProgress?: (current: string, done: number, total: number) => void,
    scanFolders: string[] = []
  ): Promise<OcrPendingResult> {
    let progressCleanup: (() => void) | null = null
    try {
      error.value = null

      // 监听 OCR 进度事件
      if (onProgress) {
        const { listen } = await import('@tauri-apps/api/event')
        let lastDone = 0
        const INVALID_VALIDITY_LABELS = ['失效', '废止', '历史版本']
        const unlisten = await listen<{
          current: string
          validity?: string
          ocr_success?: number
          ocr_failed?: number
          skipped?: number
          current_page?: number
          total_pages?: number
        }>('regulation:ocr-progress', event => {
          const { current, validity, ocr_success, ocr_failed } = event.payload
          if (typeof ocr_success === 'number' && typeof ocr_failed === 'number') {
            lastDone = ocr_success + ocr_failed
          }
          const displayCurrent =
            current && validity && INVALID_VALIDITY_LABELS.includes(validity)
              ? `[${validity}] ${current}`
              : current
          onProgress(displayCurrent, lastDone, batchSize)
        })
        progressCleanup = unlisten
      }

      // 调用 Rust 原生 OCR 命令（pdfium + PP-OCRv4）
      const result = await invoke<{
        processed: number
        ocr_success: number
        ocr_failed: number
        skipped: number
      }>('regulation_ocr_pending', { batchSize, scanFolders })

      // 刷新索引统计
      await regulationIndex.refreshStats()

      console.warn(
        `[RegulationQuery] OCR 处理完成 (Rust 原生): 成功 ${result.ocr_success}, 失败 ${result.ocr_failed}`
      )
      return {
        success: result.ocr_success,
        failed: result.ocr_failed,
        total: result.processed,
      }
    } catch (err) {
      const message = err instanceof Error ? err.message : String(err)
      if (message.includes('中止') || message.toLowerCase().includes('cancel')) {
        console.warn('[RegulationQuery] OCR 已中止')
        error.value = null
        return { success: 0, failed: 0, total: 0, cancelled: true }
      }
      error.value = message
      console.error('[RegulationQuery] OCR 处理失败:', err)
      return { success: 0, failed: 0, total: 0 }
    } finally {
      progressCleanup?.()
    }
  }

  async function cancelOcrProcessing(): Promise<boolean> {
    try {
      await invoke<boolean>('regulation_cancel_ocr')
      return true
    } catch (err) {
      error.value = err instanceof Error ? err.message : String(err)
      console.error('[RegulationQuery] 中止 OCR 失败:', err)
      return false
    }
  }

  /**
   * 清除搜索结果
   */
  function clearResults(): void {
    results.value = []
    error.value = null
  }

  /**
   * 重置搜索状态
   */
  function resetSearch(): void {
    searchState.keyword = ''
    searchState.docType = 'all'
    searchState.validity = 'all'
    searchState.dateFilter = 'all'
    searchState.startDate = ''
    searchState.endDate = ''
    clearResults()
  }

  /**
   * 设置文档类型筛选
   */
  function setDocType(type: RegulationDocType): void {
    searchState.docType = type
  }

  /**
   * 设置有效性筛选
   */
  function setValidity(validity: RegulationValidity): void {
    searchState.validity = validity
  }

  /**
   * 设置日期筛选
   */
  function setDateFilter(filter: DateFilter): void {
    searchState.dateFilter = filter
  }

  return {
    // State
    isLoading,
    isInitializing,
    isDownloading,
    isScanning,
    isSyncing,
    error,
    results,
    searchState,
    downloadingDoc,
    scanProgress,
    syncResult,
    dbSyncStatus,

    // Computed
    validCount,
    invalidCount,
    canSearch,
    canDownload,
    localDocCount,
    isLocalIndexReady,
    localSearchElapsedMs,
    isLocalSearching,
    showSnippets,

    // Store 状态（统一通过 composable 暴露，避免组件直接访问 store）
    scanResult: computed(() => regulationStore.scanResult),
    scanError: computed(() => regulationStore.scanError),
    isOcrProcessing: computed(() => regulationStore.isOcrProcessing),
    ocrProgressText: computed(() => regulationStore.ocrProgressText),
    clearScanResult: () => regulationStore.clearScanResult(),

    // Methods
    search,
    searchLocal,
    searchHybrid,
    initLocalIndex,
    refreshLocalIndexStats,
    download,
    downloadBatch,
    downloadBatchNative,
    processPendingFiles,
    scanLocalDir,
    scanAllDrives,
    syncCompare,
    cancelSyncCompare,
    ocrPendingFiles,
    cancelOcrProcessing,
    refreshDbStatus,
    resetSearch,
    setDocType,
    setValidity,
    setDateFilter,
    getSnippet,
  }
}
