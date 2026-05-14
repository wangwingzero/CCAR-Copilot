<script setup lang="ts">
/**
 * 规章查询面板组件
 *
 * 提供 CAAC 规章和规范性文件的搜索、下载功能。
 * 支持本地索引搜索（毫秒级）和在线搜索。
 */

import { ref, computed, inject, onMounted, watch } from 'vue'
import { useRegulationQuery, type DateFilter } from '@/composables/useRegulationQuery'
import { useToast } from '@/composables/useToast'
import { useSettingsStore } from '@/stores/settings'
// Store 状态统一通过 useRegulationQuery composable 访问，不再直接导入 store
import type { RegulationDocument, RegulationDocType, RegulationValidity } from '@/types'
import { invoke } from '@tauri-apps/api/core'
import { emit } from '@tauri-apps/api/event'
import { ask, open as openDialog } from '@tauri-apps/plugin-dialog'
import { open as openShell } from '@tauri-apps/plugin-shell'
import { openPath, revealItemInDir } from '@tauri-apps/plugin-opener'
import { getCurrentWindow } from '@tauri-apps/api/window'
import {
  addScanFolders,
  formatFolderName,
  loadScanFolders,
  removeScanFolder,
  saveScanFolders,
} from './scanFolders'
import { resolveResultSnippet } from './resultSnippets'
import { buildSearchFeedback } from './searchFeedback'

const appWindow = getCurrentWindow()
const isMaximized = ref(false)

// 初始化窗口最大化状态
appWindow.isMaximized().then(v => (isMaximized.value = v))

async function handleMinimize() {
  await appWindow.minimize()
}

async function handleMaximize() {
  if (isMaximized.value) {
    await appWindow.unmaximize()
  } else {
    await appWindow.maximize()
  }
  isMaximized.value = !isMaximized.value
}

// 关闭窗口（触发 Rust 侧 CloseRequested 事件，最小化到托盘）
async function handleWindowClose() {
  try {
    await appWindow.close()
  } catch (e) {
    console.error('关闭窗口失败:', e)
  }
}

/**
 * 打开设置面板。
 *
 * 与系统托盘菜单的"设置"项走完全相同的路径（统一 emit `open-settings` 事件，由
 * `@/App.vue` 的 listener 处理），避免之前依赖 inject 时可能出现的渲染时序差异
 * 导致弹窗空白。inject 的 `openSettings` 仍保留作为兜底（事件发射失败时使用），
 * 这样桌面外环境（如 Vitest）也能工作。
 */
const injectedOpenSettings = inject<() => void>('openSettings', () => {})
async function openSettings(): Promise<void> {
  try {
    await emit('open-settings')
  } catch (err) {
    console.warn('[openSettings] emit 失败，使用 inject 兜底:', err)
    injectedOpenSettings()
  }
}

// 服务器同步检查响应（与 Rust ServerSyncCheckResponse 字段对齐，camelCase）
interface ServerSyncCheckResponse {
  serverLastUpdated: string
  serverTotalCount: number
  localSyncedServerLastUpdated: string | null
  localSyncedAt: string | null
  hasUpdate: boolean
  localRoot: string
  lastSyncStats: Record<string, unknown> | null
}

const serverSyncStatus = ref<ServerSyncCheckResponse | null>(null)
const showSyncBanner = ref(false)
const showSyncCommandDialog = ref(false)
const isFullSyncing = ref(false)

interface FullSyncProgress {
  stage: string
  current: number
  total: number
  message: string
}

interface FullSyncResponse {
  caacTotal: number
  matched: number
  metaUpdated: number
  obsoleteMarked: number
  downloaded: number
  downloadFailed: number
  downloadSkippedNoUrl: number
  archiveRenamed: number
  archiveCopied: number
  archiveMissingSource: number
  serverLastUpdated: string
  syncedAt: string
}

const fullSyncProgress = ref<FullSyncProgress | null>(null)

const fullSyncProgressPercent = computed(() => {
  const p = fullSyncProgress.value
  if (!p || p.total <= 0) return 0
  return Math.min(100, Math.round((p.current / p.total) * 100))
})

const syncCommandText = computed(
  () =>
    'python scripts/align_full.py --apply-meta --apply-download\n' +
    'python scripts/sync_regulation_pdf_library.py'
)

function formatRelativeDate(iso: string | null | undefined): string {
  if (!iso) return '未知'
  const d = new Date(iso)
  if (Number.isNaN(d.getTime())) return iso
  const now = Date.now()
  const diffMs = now - d.getTime()
  const diffHours = diffMs / 3_600_000
  if (diffHours < 1) return '不到 1 小时前'
  if (diffHours < 24) return `约 ${Math.floor(diffHours)} 小时前`
  const diffDays = diffHours / 24
  if (diffDays < 30) return `约 ${Math.floor(diffDays)} 天前`
  return d.toLocaleDateString('zh-CN')
}

async function checkServerSyncStatus(): Promise<void> {
  try {
    const status = await invoke<ServerSyncCheckResponse>('regulation_check_server_manifest')
    serverSyncStatus.value = status
    if (status.hasUpdate) {
      showSyncBanner.value = true
    }
  } catch (err) {
    // 服务器不可达 / 网络问题不应阻碍应用启动
    console.warn('[RegulationPanel] 服务器同步检查失败:', err)
  }
}

const {
  isLoading,
  isInitializing,
  isScanning,
  isSyncing,
  error,
  results,
  searchState,
  downloadingDoc,
  scanProgress,
  syncResult,
  validCount,
  invalidCount,
  canSearch,
  canDownload,
  localDocCount,
  isLocalIndexReady,
  localSearchElapsedMs,
  isLocalSearching,
  // Store 状态（统一通过 composable 访问）
  scanResult,
  // Methods
  search,
  searchLocal,
  searchHybrid,
  initLocalIndex,
  refreshLocalIndexStats,
  download,
  downloadBatchNative,
  processPendingFiles,
  ocrPendingFiles,
  cancelOcrProcessing,
  syncCompare,
  cancelSyncCompare,
  refreshDbStatus: refreshDbStatusForFolders,
  dbSyncStatus,
  scanLocalDir,
  setDocType,
  setValidity,
  setDateFilter,
  showSnippets,
  getSnippet,
} = useRegulationQuery()

const { toast, showToast, hideToast } = useToast()
const settingsStore = useSettingsStore()

async function copySyncCommand(): Promise<void> {
  try {
    await navigator.clipboard.writeText(syncCommandText.value)
    showToast('同步命令已复制到剪贴板', 'success')
  } catch (err) {
    console.error('复制到剪贴板失败:', err)
    showToast('复制失败，请手动选中文本复制', 'error')
  }
}

async function startFullSync(): Promise<void> {
  if (isFullSyncing.value) return
  isFullSyncing.value = true
  isFullSyncCancelRequested.value = false
  fullSyncProgress.value = { stage: 'fetching', current: 0, total: 0, message: '正在启动同步...' }

  const { listen } = await import('@tauri-apps/api/event')
  const unlisten = await listen<FullSyncProgress>(
    'regulation:full-sync-progress',
    event => {
      fullSyncProgress.value = event.payload
    }
  )

  try {
    const result = await invoke<FullSyncResponse>('regulation_full_sync_from_server')
    const archiveTotal = result.archiveRenamed + result.archiveCopied
    showToast(
      `同步完成：匹配 ${result.matched}、更新 ${result.metaUpdated}、` +
        `下载 ${result.downloaded}、归档 ${archiveTotal}`,
      result.downloadFailed > 0 ? 'error' : 'success'
    )
    showSyncBanner.value = false
    showSyncCommandDialog.value = false
    await checkServerSyncStatus()
    // 同步后刷新本地状态和索引
    await refreshDbStatus()
    await refreshLocalIndexStats()
  } catch (err) {
    if (isFullSyncCancelRequested.value || String(err).includes('中止')) {
      showToast('同步已中止', 'info')
      return
    }
    console.error('完整同步失败:', err)
    showToast(`同步失败: ${err}`, 'error')
  } finally {
    unlisten()
    isFullSyncing.value = false
    isFullSyncCancelRequested.value = false
    fullSyncProgress.value = null
  }
}

/**
 * HTML 消毒：只允许安全的高亮标签（<b>, <em>, <mark>），移除其他所有 HTML
 * 防止 XSS 攻击（搜索摘要可能包含来自 OCR / CAAC 网站的不安全内容）
 */
function sanitizeHtml(html: string): string {
  // 保留 <b>, </b>, <em>, </em>, <mark>, </mark> 标签，移除其他所有 HTML 标签
  return html.replace(/<\/?(?!b>|\/b>|em>|\/em>|mark>|\/mark>)[^>]*>/gi, '')
}

/**
 * 在标题中高亮搜索关键词
 * 先转义 HTML 实体，再用 <mark> 包裹匹配的关键词
 */
function highlightTitle(title: string, keyword: string): string {
  if (!keyword.trim()) return escapeHtml(title)
  const escaped = escapeHtml(title)
  const words = keyword
    .trim()
    .split(/\s+/)
    .filter(w => w.length > 0)
  if (words.length === 0) return escaped
  const pattern = words.map(w => w.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')).join('|')
  return escaped.replace(new RegExp(pattern, 'gi'), '<mark>$&</mark>')
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#x27;')
}

function getDisplaySnippet(doc: RegulationDocument): string | undefined {
  return resolveResultSnippet(doc, getSnippet(doc.url))
}

// 持久化搜索模式
const SEARCH_MODE_KEY = 'regulation-search-mode'
const DISCOVER_LOCAL_KEY = 'regulation-last-local-discover-at'
const DISCOVER_LOCAL_INTERVAL_MS = 24 * 60 * 60 * 1000
const AUTO_OCR_BATCH_SIZE = 20
const AUTO_OCR_EMPTY_ROUND_LIMIT = 2
function loadSearchMode(): 'online' | 'local' | 'hybrid' {
  try {
    const saved = localStorage.getItem(SEARCH_MODE_KEY)
    if (saved === 'online' || saved === 'local' || saved === 'hybrid') {
      return saved
    }
  } catch {
    /* ignore */
  }
  return 'local'
}

function shouldRunLocalDiscover(): boolean {
  try {
    const lastRun = Number(localStorage.getItem(DISCOVER_LOCAL_KEY) || '0')
    return !lastRun || Date.now() - lastRun > DISCOVER_LOCAL_INTERVAL_MS
  } catch {
    return true
  }
}

function markLocalDiscoverRan(): void {
  try {
    localStorage.setItem(DISCOVER_LOCAL_KEY, String(Date.now()))
  } catch {
    /* ignore */
  }
}

// 本地状态
const selectedDocs = ref<Set<string>>(new Set())
const showCustomDatePicker = ref(false)
const searchMode = ref<'online' | 'local' | 'hybrid'>(loadSearchMode())
const lastSearchSource = ref<'local' | 'online' | null>(null)
const hasSearched = ref(false)
const lastSearchKeyword = ref('')

// 监听搜索模式变化，自动持久化
watch(searchMode, mode => {
  try {
    localStorage.setItem(SEARCH_MODE_KEY, mode)
  } catch {
    /* ignore */
  }
})
const isProcessingFiles = ref(false)
const isAutoOcrRunning = ref(false)
const processingStage = ref<'idle' | 'extracting' | 'ocr'>('idle')
const processingProgressText = ref('')
const isOcrCancelRequested = ref(false)
const isSyncCancelRequested = ref(false)
const isFullSyncCancelRequested = ref(false)
const isRetryingOcr = ref(false)
const isRequeueingForMineru = ref(false)
const isCleaningInvalid = ref(false)
const isRealigningFilenames = ref(false)

// OCR 引擎统计 + 重做对话框
type OcrEngineStats = {
  pdfium: number
  ppOcrv4: number
  mineru: number
  unknown: number
  scanOnly: number
  nonMineru: number
  totalDone: number
}
type RequeueScope =
  | 'scan_only'
  | 'pp_ocrv4'
  | 'pdfium'
  | 'unknown'
  | 'non_mineru'
  | 'all_done'
const showRequeueDialog = ref(false)
const requeueScope = ref<RequeueScope>('scan_only')
const ocrEngineStats = ref<OcrEngineStats | null>(null)
const isLoadingOcrStats = ref(false)

const processResult = ref<{
  processed: number
  indexed: number
  needs_ocr: number
  failed: number
} | null>(null)
const scanFolders = ref<string[]>(loadScanFolders())
const isScanningSelectedFolders = ref(false)
const currentScanFolder = ref('')
const showMaintenanceMenu = ref(false)

watch(
  scanFolders,
  folders => {
    saveScanFolders(folders)
    void refreshDbStatusForFolders([...folders])
  },
  { deep: true }
)

async function refreshDbStatus(): Promise<void> {
  await refreshDbStatusForFolders([...scanFolders.value])
}

// 状态统一通过 useRegulationQuery composable 获取（已在上方解构）

// 扫描进度百分比（同时支持 processing 和 ocr 阶段）
const scanProgressPercent = computed(() => {
  const sp = scanProgress.value
  if (!sp) return '0%'

  if (sp.phase === 'ocr' && sp.ocr_total && sp.ocr_total > 0) {
    // OCR 阶段：基于扫描100% + OCR进度
    const scanPart = 50 // 扫描阶段占 50%
    const ocrPart = ((sp.ocr_processed ?? 0) / sp.ocr_total) * 50
    return `${Math.min(scanPart + ocrPart, 100)}%`
  }

  if (sp.total_found > 0) {
    // 扫描阶段
    const hasOcr = sp.needs_ocr > 0
    const maxPercent = hasOcr ? 50 : 100 // 如果有 OCR 文件，扫描阶段最多 50%
    return `${Math.min((sp.scanned / sp.total_found) * maxPercent, maxPercent)}%`
  }

  return '0%'
})

// 文档类型选项
const docTypeOptions: { value: RegulationDocType; label: string }[] = [
  { value: 'all', label: '全部' },
  { value: 'normative', label: '规范性文件' },
  { value: 'regulation', label: 'CCAR 规章' },
  { value: 'standard', label: '标准规范' },
]

// 日期筛选选项
const dateFilterOptions: { value: DateFilter; label: string }[] = [
  { value: 'all', label: '全部时间' },
  { value: '1day', label: '近 1 天' },
  { value: '7days', label: '近 7 天' },
  { value: '30days', label: '近 30 天' },
  { value: 'custom', label: '自定义' },
]

const validityFilterOptions: { value: RegulationValidity; label: string }[] = [
  { value: 'all', label: '全部' },
  { value: 'valid', label: '有效' },
  { value: 'invalid', label: '失效/废止' },
]

// 搜索模式选项
const searchModeOptions = [
  { value: 'hybrid', label: '智能搜索', desc: '本地优先，在线补充' },
  { value: 'local', label: '本地搜索', desc: '仅搜索已下载文档' },
  { value: 'online', label: '在线搜索', desc: '搜索 CAAC 官网' },
] as const

// 计算属性
const totalCount = computed(() => results.value.length)
const hasResults = computed(() => results.value.length > 0)
const hasSelection = computed(() => selectedDocs.value.size > 0)
const selectedCount = computed(() => selectedDocs.value.size)
const canScanSelectedFolders = computed(
  () => scanFolders.value.length > 0 && !isScanning.value && !isScanningSelectedFolders.value
)
const activeValidityLabel = computed(
  () =>
    validityFilterOptions.find(option => option.value === searchState.validity)?.label ?? '全部'
)
const emptyStateMessage = computed(() => {
  const keyword = lastSearchKeyword.value.trim()
  return keyword ? `未找到与“${keyword}”相关的内容` : '输入关键词开始搜索'
})
const emptyStateHint = computed(() => {
  if (!hasSearched.value || searchState.validity === 'all') return ''
  return `当前有效性筛选：${activeValidityLabel.value}`
})
const processPendingButtonLabel = computed(() => {
  if (processingStage.value === 'extracting') return '提取文本中...'
  if (processingStage.value === 'ocr') return isAutoOcrRunning.value ? '后台OCR中...' : 'OCR 识别中...'
  return '处理待索引/OCR'
})

const indexedSearchScope = computed(() => dbSyncStatus.value?.indexed ?? localDocCount.value)
const pendingOcrCount = computed(() => dbSyncStatus.value?.pending_ocr ?? 0)
const searchFeedback = computed(() => {
  return buildSearchFeedback({
    elapsedMs: lastSearchSource.value === 'local' ? localSearchElapsedMs.value : null,
    hasSearched: hasSearched.value,
    indexedCount: indexedSearchScope.value,
    isLoading: isLoading.value,
    isLocalSearching: isLocalSearching.value,
    keyword: lastSearchKeyword.value,
    pendingOcr: pendingOcrCount.value,
    resultCount: totalCount.value,
    source: lastSearchSource.value,
  })
})

// 初始化本地索引 + 自动发现
onMounted(async () => {
  // 恢复自定义日期选择器状态
  if (searchState.dateFilter === 'custom') {
    showCustomDatePicker.value = true
  }
  await configureManagedRegulationRoot(scanFolders.value[0])
  await initLocalIndex()
  await refreshDbStatus()

  // 每天轻量校验一次局方目录。发现文件后后台 OCR 队列会自动接管待识别文件。
  if (localDocCount.value < 10 || shouldRunLocalDiscover()) {
    console.warn('[RegulationPanel] 校验局方本地目录...')
    try {
      const result = await invoke<{ new_added?: number }>('regulation_discover_local', {
        localCopyMode: 'register_only',
      })
      console.warn('[RegulationPanel] 局方目录校验完成:', result)
      markLocalDiscoverRan()
      if (result?.new_added && result.new_added > 0) {
        await refreshDbStatus()
        await refreshLocalIndexStats()
      }
    } catch (err) {
      console.warn('[RegulationPanel] 局方目录校验失败:', err)
    }
  }

  void startBackgroundOcrQueue('startup')

  // 不阻塞：后台检查服务器镜像是否有更新
  void checkServerSyncStatus()
})

async function startBackgroundOcrQueue(reason: string): Promise<void> {
  if (isProcessingFiles.value || isAutoOcrRunning.value) return

  const scopedScanFolders = [...scanFolders.value]
  let pending = 0
  try {
    await refreshDbStatus()
    pending = dbSyncStatus.value?.pending_ocr ?? 0
    if (pending <= 0) return
  } catch (err) {
    console.warn('[RegulationPanel] 后台 OCR 队列检查失败:', err)
    return
  }

  isProcessingFiles.value = true
  isAutoOcrRunning.value = true
  isOcrCancelRequested.value = false
  processingStage.value = 'ocr'
  processingProgressText.value = `后台 OCR 队列启动：待识别 ${pending} 个`

  let totalSuccess = 0
  let totalFailed = 0
  let totalProcessed = 0
  let emptyRounds = 0
  let wasCancelled = false

  console.warn(`[RegulationPanel] 后台 OCR 队列启动: ${reason}, pending=${pending}`)

  try {
    while (true) {
      if (isOcrCancelRequested.value) break
      await refreshDbStatus()
      const remaining = dbSyncStatus.value?.pending_ocr ?? 0
      if (remaining <= 0) break

      const batchSize = Math.min(AUTO_OCR_BATCH_SIZE, remaining)
      processingProgressText.value = `后台 OCR 识别中：本批 ${batchSize} 个，剩余约 ${remaining} 个`

      const result = await ocrPendingFiles(
        batchSize,
        (current, done, total) => {
          const currentName = current ? `：${current}` : ''
          processingProgressText.value = `后台 OCR ${done}/${total}，剩余约 ${remaining} 个${currentName}`
        },
        scopedScanFolders
      )

      if (result.cancelled || isOcrCancelRequested.value) {
        wasCancelled = true
        showToast('后台 OCR 已中止', 'info')
        break
      }

      totalSuccess += result.success
      totalFailed += result.failed
      totalProcessed += result.total

      await refreshDbStatus()
      await refreshLocalIndexStats()

      if (result.total === 0) {
        emptyRounds += 1
        if (emptyRounds >= AUTO_OCR_EMPTY_ROUND_LIMIT) {
          console.warn('[RegulationPanel] 后台 OCR 队列连续空转，暂停自动处理')
          break
        }
      } else {
        emptyRounds = 0
      }
    }

    if (!wasCancelled && totalProcessed > 0) {
      if (searchState.keyword.trim()) {
        await handleSearch()
      }
      showToast(
        `后台 OCR 完成：处理 ${totalProcessed} 个，索引 ${totalSuccess} 个，失败 ${totalFailed} 个`,
        totalFailed > 0 ? 'error' : 'success'
      )
    }
  } catch (err) {
    console.error('[RegulationPanel] 后台 OCR 失败:', err)
    showToast(`后台 OCR 暂停: ${err}`, 'error')
  } finally {
    isAutoOcrRunning.value = false
    isProcessingFiles.value = false
    isOcrCancelRequested.value = false
    processingStage.value = 'idle'
    processingProgressText.value = ''
  }
}

const INVALID_VALIDITY_LABELS = ['失效', '废止', '历史版本'] as const

function inferDocumentValidity(doc: RegulationDocument): string {
  const explicit = doc.validity.trim()
  if (INVALID_VALIDITY_LABELS.some(label => label === explicit)) return explicit

  const searchableText = [doc.title, doc.doc_number, doc.file_path, doc.url].join(' ')
  return INVALID_VALIDITY_LABELS.find(label => searchableText.includes(label)) ?? explicit
}

// 获取有效性样式
function getValidityClass(doc: RegulationDocument): string {
  switch (inferDocumentValidity(doc)) {
    case '有效':
      return 'validity-valid'
    case '失效':
    case '废止':
    case '历史版本':
      return 'validity-invalid'
    default:
      return ''
  }
}

function getValidityLabel(doc: RegulationDocument): string {
  return inferDocumentValidity(doc) || '未标注'
}

// 获取文档类型标签
function getDocTypeLabel(docType: string): string {
  return docType === 'regulation' ? 'CCAR' : '规范性'
}

// 处理搜索
async function handleSearch(): Promise<void> {
  // 防止并发触发：当上一次搜索仍在进行时（按钮虽 disabled，但 Enter 键和
  // 筛选项变更等内部触发不受按钮 disabled 限制），直接拒绝本次请求；
  // 否则会出现 lastSearchKeyword 已更新为新词、但 results/snippetMap 仍是
  // 旧搜索的产物，导致搜索框与结果列表的高亮关键词不一致。
  if (isLoading.value || isLocalSearching.value) {
    return
  }
  selectedDocs.value.clear()
  const keyword = searchState.keyword.trim()
  hasSearched.value = keyword.length > 0
  lastSearchKeyword.value = keyword
  const scopedScanFolders = [...scanFolders.value]
  const searchOptions = { scanFolders: scopedScanFolders }

  switch (searchMode.value) {
    case 'local': {
      // 仅本地搜索
      await searchLocal(searchOptions)
      lastSearchSource.value = 'local'
      break
    }
    case 'online':
      if (scopedScanFolders.length > 0) {
        await searchLocal(searchOptions)
        lastSearchSource.value = 'local'
        break
      }
      // 仅在线搜索
      lastSearchSource.value = 'online'
      await search()
      break
    case 'hybrid':
    default:
      // 混合搜索：先本地后在线
      await searchHybrid(searchOptions)
      lastSearchSource.value = isLocalIndexReady.value ? 'local' : 'online'
      break
  }
}

async function cancelFullSync(): Promise<void> {
  if (!isFullSyncing.value || isFullSyncCancelRequested.value) return
  isFullSyncCancelRequested.value = true
  fullSyncProgress.value = {
    stage: fullSyncProgress.value?.stage ?? 'cancel',
    current: fullSyncProgress.value?.current ?? 0,
    total: fullSyncProgress.value?.total ?? 0,
    message: '正在中止同步...',
  }
  try {
    await invoke<boolean>('regulation_cancel_full_sync')
  } catch (err) {
    console.error('请求中止完整同步失败:', err)
    showToast(`中止失败: ${err}`, 'error')
  }
}

// 打开本地文件
//
// Tauri 2 下打开本地文件要用 plugin-opener 的 `openPath`（对应 capability
// `opener:allow-open-path`，scope 已经配 `path: **`）；`plugin-shell.open`
// 的默认 scope 只允许 `http(s)://`、`mailto:`、`tel:`，拿文件路径调用会被
// scope 拒绝，用户表现就是「按了没反应」。
async function handleOpenLocal(doc: RegulationDocument): Promise<void> {
  const filePath = doc.file_path || doc.url?.replace('local://', '') || ''
  if (!filePath) {
    console.warn('handleOpenLocal: doc 缺少 file_path 和 local:// url', doc)
    showToast('该文档还没下载到本地，无法打开', 'error')
    return
  }

  try {
    // 用系统默认应用打开（Windows 上 .pdf 走 PDF 阅读器）
    await openPath(filePath)
  } catch (err) {
    console.error('openPath 失败，回退到在文件管理器中显示:', err)
    try {
      await revealItemInDir(filePath)
    } catch (err2) {
      console.error('revealItemInDir 也失败:', err2)
      showToast(`打开文件失败: ${err instanceof Error ? err.message : String(err)}`, 'error')
    }
  }
}

// 处理下载
async function handleDownload(doc: RegulationDocument): Promise<void> {
  const filePath = await download(doc)
  if (filePath) {
    // 下载成功，可以选择打开文件夹
    try {
      await revealItemInDir(filePath)
    } catch {
      // 忽略错误
    }
    void startBackgroundOcrQueue('single-download')
  }
}

// 处理批量下载
async function handleBatchDownload(): Promise<void> {
  const selectedList = results.value.filter((doc: RegulationDocument) =>
    selectedDocs.value.has(doc.url)
  )

  if (selectedList.length === 0) {
    return
  }

  const { success, skipped, failed } = await downloadBatchNative(selectedList, [...scanFolders.value])
  showToast(
    `下载完成：成功 ${success} 个，跳过 ${skipped} 个，失败 ${failed} 个`,
    failed > 0 ? 'error' : 'success'
  )
  selectedDocs.value.clear()
  void startBackgroundOcrQueue('batch-download')
}

// 处理待索引文件（PDF 文本提取 + 索引）
async function handleProcessPending(): Promise<void> {
  if (isProcessingFiles.value) return

  isProcessingFiles.value = true
  isOcrCancelRequested.value = false
  processingStage.value = 'extracting'
  processingProgressText.value = ''
  processResult.value = null

  try {
    const scopedScanFolders = [...scanFolders.value]
    const result = await processPendingFiles(20, scopedScanFolders)
    processResult.value = result
    if (result.processed === 0) {
      showToast('没有待处理的文件', 'info')
    } else if (result.needs_ocr > 0) {
      showToast(
        `普通文本提取完成：索引 ${result.indexed} 个，开始 OCR ${result.needs_ocr} 个`,
        'info'
      )
      processingStage.value = 'ocr'
      processingProgressText.value = `准备 OCR ${result.needs_ocr} 个文件`

      const ocrResult = await ocrPendingFiles(
        result.needs_ocr,
        (current, done, total) => {
          processingProgressText.value = `${done}/${total} ${current}`
        },
        scopedScanFolders
      )

      if (ocrResult.cancelled || isOcrCancelRequested.value) {
        showToast('OCR 已中止，未处理的文件会保留在待 OCR 队列', 'info')
        return
      }

      await refreshDbStatus()
      await refreshLocalIndexStats()
      if (searchState.keyword.trim()) {
        await handleSearch()
      }

      showToast(
        `处理完成：普通索引 ${result.indexed} 个，OCR 索引 ${ocrResult.success} 个，失败 ${
          result.failed + ocrResult.failed
        } 个`,
        result.failed + ocrResult.failed > 0 ? 'error' : 'success'
      )
    } else {
      await refreshDbStatus()
      await refreshLocalIndexStats()
      if (result.indexed > 0 && searchState.keyword.trim()) {
        await handleSearch()
      }
      showToast(
        `处理完成：索引 ${result.indexed} 个，无需 OCR，失败 ${result.failed} 个`,
        result.failed > 0 ? 'error' : 'success'
      )
    }
  } catch (err) {
    showToast(`处理失败: ${err}`, 'error')
  } finally {
    const shouldResumeBackground = !isOcrCancelRequested.value
    isProcessingFiles.value = false
    isOcrCancelRequested.value = false
    processingStage.value = 'idle'
    processingProgressText.value = ''
    if (shouldResumeBackground) {
      void startBackgroundOcrQueue('manual-process')
    }
  }
}

async function handleCancelOcr(): Promise<void> {
  if (!isProcessingFiles.value || processingStage.value !== 'ocr' || isOcrCancelRequested.value) {
    return
  }
  isOcrCancelRequested.value = true
  processingProgressText.value = '正在中止 OCR...'
  const ok = await cancelOcrProcessing()
  if (ok) {
    showToast('已请求中止 OCR，当前页/当前文件结束后会停止', 'info')
  } else {
    showToast('请求中止 OCR 失败', 'error')
  }
}

async function handleProcessPendingAction(): Promise<void> {
  if (isProcessingFiles.value && processingStage.value === 'ocr') {
    await handleCancelOcr()
    return
  }
  await handleProcessPending()
}

// 打开 OCR 引擎重做对话框：加载各引擎统计后弹窗
async function openRequeueDialog(): Promise<void> {
  if (isRequeueingForMineru.value) return
  showRequeueDialog.value = true
  ocrEngineStats.value = null
  isLoadingOcrStats.value = true
  try {
    const stats = await invoke<{
      pdfium: number
      ppOcrv4: number
      mineru: number
      unknown: number
      scanOnly: number
      nonMineru: number
      totalDone: number
    }>('regulation_ocr_engine_stats', {
      scanFolders: [...scanFolders.value],
    })
    ocrEngineStats.value = stats
    // 默认选中：优先 scan_only（= pp_ocrv4 + unknown，不含 pdfium）
    // 表象依据：pdfium 重跑无质量提升，不应被默认勾选
    if (stats.scanOnly > 0) {
      requeueScope.value = 'scan_only'
    } else if (stats.ppOcrv4 > 0) {
      requeueScope.value = 'pp_ocrv4'
    } else if (stats.unknown > 0) {
      requeueScope.value = 'unknown'
    } else if (stats.nonMineru > 0) {
      requeueScope.value = 'non_mineru'
    } else {
      requeueScope.value = 'scan_only'
    }
  } catch (err) {
    console.error('加载 OCR 引擎统计失败:', err)
    showToast(`加载 OCR 引擎统计失败: ${err}`, 'error')
    showRequeueDialog.value = false
  } finally {
    isLoadingOcrStats.value = false
  }
}

function closeRequeueDialog(): void {
  if (isRequeueingForMineru.value) return
  showRequeueDialog.value = false
}

// 当前选中 scope 的预计数量
const requeueExpectedCount = computed<number>(() => {
  const s = ocrEngineStats.value
  if (!s) return 0
  switch (requeueScope.value) {
    case 'scan_only':
      return s.scanOnly
    case 'pp_ocrv4':
      return s.ppOcrv4
    case 'pdfium':
      return s.pdfium
    case 'unknown':
      return s.unknown
    case 'non_mineru':
      return s.nonMineru
    case 'all_done':
      return s.totalDone
    default:
      return 0
  }
})

// 按选定 scope 执行重做
async function confirmRequeue(): Promise<void> {
  if (isRequeueingForMineru.value) return
  if (requeueExpectedCount.value === 0) {
    showToast('当前范围没有需要重做的记录', 'info')
    return
  }

  isRequeueingForMineru.value = true
  try {
    const result = await invoke<{
      candidateCount: number
      deletedFromIndex: number
      resetToPending: number
      sampleTitles: string[]
    }>('regulation_requeue_ocr_by_engine', {
      filter: { scope: requeueScope.value, scanFolders: [...scanFolders.value] },
    })

    if (result.candidateCount === 0) {
      showToast('没有找到需要重做的记录', 'info')
    } else {
      const samplePreview = result.sampleTitles.slice(0, 3).join('、')
      showToast(
        `已重置 ${result.resetToPending} 条（如：${samplePreview}），从索引删除 ${result.deletedFromIndex} 个。` +
          '请点「处理待索引/OCR」启动 MinerU 重做。',
        'success'
      )
    }
    showRequeueDialog.value = false
    await refreshDbStatus()
  } catch (err) {
    console.error('重置失败:', err)
    showToast(`重置失败: ${err}`, 'error')
  } finally {
    isRequeueingForMineru.value = false
  }
}

// 重试失败的 OCR 文件
async function handleRetryFailedOcr(): Promise<void> {
  if (isRetryingOcr.value) return

  isRetryingOcr.value = true
  try {
    const result = await invoke<{
      processed: number
      ocr_success: number
      ocr_failed: number
      skipped: number
    }>('regulation_retry_failed_ocr', {
      scanFolders: [...scanFolders.value],
    })

    if (result.processed === 0) {
      showToast('没有失败的 OCR 文件需要重试', 'info')
    } else {
      showToast(
        `重试完成: 成功 ${result.ocr_success}, 仍失败 ${result.ocr_failed}`,
        result.ocr_failed > 0 ? 'error' : 'success'
      )
    }

    // 刷新数据库状态
    await refreshDbStatus()
  } catch (err) {
    showToast(`重试失败: ${err}`, 'error')
  } finally {
    isRetryingOcr.value = false
  }
}

// 清理无效的规章记录：后缀不是 PDF/TXT 或文件物理不存在
// 这些记录无论怎么重试 OCR 都不可能成功，需要从数据库/索引中移除
async function handleCleanInvalid(): Promise<void> {
  if (isCleaningInvalid.value) return

  const confirmed = await ask(
    '将从数据库 + 索引中删除以下两类无效规章记录：\n\n' +
      '• 文件后缀不是 .pdf/.txt（例如 .doc/.docx 历史遗留）\n' +
      '• 数据库中登记的路径在磁盘上已经不存在\n\n' +
      '⚠️ 此操作不可撤销，但不会删除磁盘上的实际文件。\n\n' +
      '是否继续？',
    { title: '清理无效规章记录', kind: 'warning' }
  )
  if (!confirmed) return

  isCleaningInvalid.value = true
  try {
    const result = await invoke<{
      candidateCount: number
      nonPdfCount: number
      missingFileCount: number
      deletedFromIndex: number
      deletedFromDb: number
      sampleTitles: string[]
    }>('regulation_cleanup_invalid_files')

    if (result.candidateCount === 0) {
      showToast('没有需要清理的无效记录', 'info')
    } else {
      showToast(
        `清理完成: 共 ${result.deletedFromDb} 条 ` +
          `(不支持类型 ${result.nonPdfCount}, 文件丢失 ${result.missingFileCount})`,
        'success'
      )
    }

    // 刷新数据库状态
    await refreshDbStatus()
  } catch (err) {
    showToast(`清理失败: ${err}`, 'error')
  } finally {
    isCleaningInvalid.value = false
  }
}

// 一键对齐 PDF/TXT 文件名：把磁盘上的 <hash>.ext 改为「文号_标题.ext」
async function handleRealignFilenames(): Promise<void> {
  if (isRealigningFilenames.value) return

  const confirmed = await ask(
    '将批量重命名磁盘上的 PDF/TXT 文件为可读格式：\n\n' +
      '• 优先使用「文号_标题.ext」\n' +
      '• 缺失文号时使用「标题.ext」\n' +
      '• 遇到重名会自动追加 sha256 短缀\n\n' +
      '本操作会同步更新数据库 file_path 和搜索索引，\n' +
      '不会移动文件到其它目录，仅在原目录内 rename。\n\n' +
      '是否继续？',
    { title: '一键对齐 PDF 文件名', kind: 'warning' }
  )
  if (!confirmed) return

  isRealigningFilenames.value = true
  try {
    if (!isLocalIndexReady.value) {
      await initLocalIndex()
    }

    const result = await invoke<{
      totalScanned: number
      skippedInvalid: number
      alreadyAligned: number
      renamed: number
      failed: number
      indexUpdated: number
      samples: string[]
      failureSamples: string[]
    }>('regulation_realign_pdf_filenames')

    if (result.renamed === 0 && result.failed === 0) {
      showToast(
        `无需对齐: 扫描 ${result.totalScanned} 条, 已对齐 ${result.alreadyAligned}, 跳过无效 ${result.skippedInvalid}`,
        'info'
      )
    } else {
      const level = result.failed > 0 ? 'error' : 'success'
      showToast(
        `重命名完成: 成功 ${result.renamed} 条 (索引已同步 ${result.indexUpdated}), 失败 ${result.failed}`,
        level
      )
      if (result.samples.length > 0) {
        // 在控制台打印样本，方便用户对照
        console.info('[RealignFilenames] 样本:', result.samples)
      }
      if (result.failureSamples.length > 0) {
        console.warn('[RealignFilenames] 失败样本:', result.failureSamples)
      }
    }

    await refreshDbStatus()
    await refreshLocalIndexStats()
    if (searchState.keyword.trim()) {
      await handleSearch()
    }
  } catch (err) {
    showToast(`对齐失败: ${err}`, 'error')
  } finally {
    isRealigningFilenames.value = false
  }
}

// 同步对比官网
async function handleSyncCompare(): Promise<void> {
  if (isSyncing.value) {
    if (isSyncCancelRequested.value) return
    isSyncCancelRequested.value = true
    const ok = await cancelSyncCompare()
    showToast(ok ? '已请求中止同步' : '请求中止同步失败', ok ? 'info' : 'error')
    return
  }
  isSyncCancelRequested.value = false
  const result = await syncCompare('all', 20, true)
  if (!result) {
    showToast(isSyncCancelRequested.value ? '同步已中止' : '同步失败，请稍后重试', isSyncCancelRequested.value ? 'info' : 'error')
    isSyncCancelRequested.value = false
    return
  }
  isSyncCancelRequested.value = false

  const downloaded = result.downloaded ?? 0
  const failed = result.download_failed ?? 0
  if (downloaded > 0 || failed > 0) {
    showToast(
      `同步完成：新增 ${result.new_regulations.length} 条，下载 ${downloaded} 个，失败 ${failed} 个`,
      failed > 0 ? 'error' : 'success'
    )
  } else {
    showToast(
      result.new_regulations.length > 0
        ? `同步完成：发现新增 ${result.new_regulations.length} 条`
        : '同步完成：本地已是最新',
      'success'
    )
  }

  void startBackgroundOcrQueue('sync-compare')
}

async function handleAddScanFolder(): Promise<void> {
  if (isScanning.value || isScanningSelectedFolders.value) return

  try {
    const selected = await openDialog({
      directory: true,
      multiple: true,
      title: '选择要扫描的文件夹',
    })

    if (!selected) return

    const previousLength = scanFolders.value.length
    const nextFolders = addScanFolders(scanFolders.value, selected)
    const newlyAdded = nextFolders.slice(previousLength)
    scanFolders.value = nextFolders

    if (newlyAdded.length > 0) {
      await configureManagedRegulationRoot(nextFolders[0])
      showToast(`已添加 ${newlyAdded.length} 个文件夹，开始扫描...`, 'success')
      await scanFolderQueue(newlyAdded)
    } else {
      showToast('选择的文件夹已在列表中', 'info')
    }
  } catch (err) {
    showToast(`选择文件夹失败: ${err}`, 'error')
  }
}

async function configureManagedRegulationRoot(folder: string | undefined): Promise<void> {
  if (!folder) return

  if (!settingsStore.isLoaded) {
    await settingsStore.loadConfig()
  }

  if (settingsStore.advanced.regulationStoragePath !== folder) {
    settingsStore.updateAdvanced({ regulationStoragePath: folder })
    await settingsStore.saveConfig()
  }

  await invoke('regulation_prepare_storage_dirs')
}

function handleRemoveScanFolder(folder: string): void {
  if (isScanning.value || isScanningSelectedFolders.value) return
  scanFolders.value = removeScanFolder(scanFolders.value, folder)
}

function handleClearScanFolders(): void {
  if (isScanning.value || isScanningSelectedFolders.value) return
  scanFolders.value = []
}

async function scanFolderQueue(folders: string[]): Promise<void> {
  if (isScanning.value || isScanningSelectedFolders.value) return

  if (folders.length === 0) {
    showToast('请先添加要扫描的文件夹', 'info')
    return
  }

  isScanningSelectedFolders.value = true
  currentScanFolder.value = ''
  let success = 0
  let failed = 0

  try {
    for (const folder of folders) {
      currentScanFolder.value = folder
      const result = await scanLocalDir(folder, true)
      if (result) {
        success += 1
      } else {
        failed += 1
      }
    }

    await refreshDbStatus()
    void startBackgroundOcrQueue('folder-scan')
    showToast(
      `文件夹扫描完成：成功 ${success} 个，失败 ${failed} 个`,
      failed > 0 ? 'error' : 'success'
    )
  } catch (err) {
    showToast(`扫描失败: ${err}`, 'error')
  } finally {
    isScanningSelectedFolders.value = false
    currentScanFolder.value = ''
  }
}

async function handleScanSelectedFolders(): Promise<void> {
  await scanFolderQueue([...scanFolders.value])
}

async function handleScanFolder(folder: string): Promise<void> {
  await scanFolderQueue([folder])
}

// 切换选择
function toggleSelection(doc: RegulationDocument): void {
  if (selectedDocs.value.has(doc.url)) {
    selectedDocs.value.delete(doc.url)
  } else {
    selectedDocs.value.add(doc.url)
  }
}

// 全选/取消全选
function toggleSelectAll(): void {
  if (selectedDocs.value.size === results.value.length) {
    selectedDocs.value.clear()
  } else {
    results.value.forEach((doc: RegulationDocument) => selectedDocs.value.add(doc.url))
  }
}

// 打开链接
async function openUrl(url: string): Promise<void> {
  try {
    await openShell(url)
  } catch {
    // 忽略错误
  }
}

// 处理文档类型变化
async function handleDocTypeChange(type: RegulationDocType): Promise<void> {
  if (searchState.docType === type) return
  setDocType(type)
  await handleSearch()
}

// 处理日期筛选变化
async function handleDateFilterChange(filter: DateFilter): Promise<void> {
  if (searchState.dateFilter === filter) return
  setDateFilter(filter)
  if (filter === 'custom') {
    showCustomDatePicker.value = true
  } else {
    showCustomDatePicker.value = false
    await handleSearch()
  }
}

// 处理有效性筛选点击
async function handleValidityClick(validity: RegulationValidity): Promise<void> {
  if (searchState.validity === validity) return
  setValidity(validity)
  await handleSearch()
}
</script>

<template>
  <div class="regulation-panel">
    <!-- 标题栏 -->
    <div class="panel-header" data-tauri-drag-region>
      <h2>规章查询</h2>
      <div class="header-actions">
        <button class="settings-btn" title="设置" aria-label="设置" @click="openSettings">
          <svg viewBox="0 0 24 24" width="16" height="16">
            <path
              fill="none"
              stroke="currentColor"
              stroke-width="2"
              stroke-linecap="round"
              stroke-linejoin="round"
              d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"
            />
            <circle cx="12" cy="12" r="3" fill="none" stroke="currentColor" stroke-width="2" />
          </svg>
        </button>
      </div>
      <div class="window-controls">
        <button class="control-btn" title="最小化" aria-label="最小化" @click="handleMinimize">
          <svg viewBox="0 0 12 12" width="12" height="12">
            <rect fill="currentColor" x="1" y="5.5" width="10" height="1" />
          </svg>
        </button>
        <button class="control-btn" title="最大化" aria-label="最大化" @click="handleMaximize">
          <svg viewBox="0 0 12 12" width="12" height="12">
            <rect
              fill="none"
              stroke="currentColor"
              stroke-width="1"
              x="1.5"
              y="1.5"
              width="9"
              height="9"
            />
          </svg>
        </button>
        <button
          class="control-btn close-btn"
          title="最小化到托盘"
          aria-label="最小化到托盘"
          @click="handleWindowClose"
        >
          <svg viewBox="0 0 12 12" width="12" height="12">
            <path
              fill="currentColor"
              d="M1.41 0L0 1.41 4.59 6 0 10.59 1.41 12 6 7.41 10.59 12 12 10.59 7.41 6 12 1.41 10.59 0 6 4.59z"
            />
          </svg>
        </button>
      </div>
    </div>

    <!-- 数据库统计仪表板 -->
    <div v-if="dbSyncStatus && dbSyncStatus.total_files > 0" class="dashboard-section">
      <div class="dashboard-stats">
        <div class="dash-stat">
          <span class="dash-stat-value">{{ dbSyncStatus.total_files }}</span>
          <span class="dash-stat-label">总文件</span>
        </div>
        <div class="dash-stat indexed">
          <span class="dash-stat-value">{{ dbSyncStatus.indexed }}</span>
          <span class="dash-stat-label">已索引</span>
        </div>
        <div class="dash-stat pending">
          <span class="dash-stat-value">{{ dbSyncStatus.pending_ocr }}</span>
          <span class="dash-stat-label">待OCR</span>
        </div>
        <div v-if="dbSyncStatus.failed_ocr > 0" class="dash-stat failed">
          <span class="dash-stat-value">{{ dbSyncStatus.failed_ocr }}</span>
          <span class="dash-stat-label">失败</span>
        </div>
      </div>
      <div class="dashboard-bar">
        <div
          class="dashboard-bar-fill indexed"
          :style="{ width: `${(dbSyncStatus.indexed / dbSyncStatus.total_files) * 100}%` }"
          title="已索引"
        ></div>
        <div
          class="dashboard-bar-fill pending"
          :style="{ width: `${(dbSyncStatus.pending_ocr / dbSyncStatus.total_files) * 100}%` }"
          title="待 OCR"
        ></div>
        <div
          class="dashboard-bar-fill failed"
          :style="{ width: `${(dbSyncStatus.failed_ocr / dbSyncStatus.total_files) * 100}%` }"
          title="失败"
        ></div>
      </div>
    </div>

    <!-- 服务器同步提示 banner（未同步时） -->
    <div
      v-if="!isFullSyncing && showSyncBanner && serverSyncStatus"
      class="server-sync-banner"
      role="status"
    >
      <div class="sync-banner-icon">↻</div>
      <div class="sync-banner-text">
        <strong>局方规章有更新</strong>
        <p>
          服务器：{{ serverSyncStatus.serverTotalCount }} 条
          · 更新于 {{ formatRelativeDate(serverSyncStatus.serverLastUpdated) }}
          <template v-if="serverSyncStatus.localSyncedAt">
            · 上次同步 {{ formatRelativeDate(serverSyncStatus.localSyncedAt) }}
          </template>
          <template v-else> · 本地尚未同步过 </template>
        </p>
      </div>
      <div class="sync-banner-actions">
        <button class="sync-btn-primary" :disabled="isFullSyncing" @click="startFullSync">
          立即同步
        </button>
        <button class="sync-btn-default" @click="showSyncCommandDialog = true">命令行方式</button>
        <button class="sync-btn-text" @click="showSyncBanner = false">忽略</button>
      </div>
    </div>

    <!-- 同步中进度 banner -->
    <div
      v-if="isFullSyncing && fullSyncProgress"
      class="server-sync-progress-banner"
      role="status"
      aria-live="polite"
    >
      <div class="sync-banner-icon sync-spinning">↺</div>
      <div class="sync-banner-text">
        <strong>正在同步局方规章...</strong>
        <p>{{ fullSyncProgress.message }}</p>
        <div v-if="fullSyncProgress.total > 0" class="sync-progress-bar">
          <div
            class="sync-progress-fill"
            :style="{ width: fullSyncProgressPercent + '%' }"
          ></div>
        </div>
      </div>
      <button
        class="sync-btn-default"
        :disabled="isFullSyncCancelRequested"
        @click="cancelFullSync"
      >
        {{ isFullSyncCancelRequested ? '中止中...' : '中止同步' }}
      </button>
    </div>

    <!-- 同步命令对话框 -->
    <div
      v-if="showSyncCommandDialog"
      class="sync-modal-overlay"
      @click="showSyncCommandDialog = false"
    >
      <div class="sync-modal" role="dialog" aria-labelledby="sync-modal-title" @click.stop>
        <h3 id="sync-modal-title">同步局方规章到本地</h3>
        <p class="sync-modal-desc">在项目根目录的 PowerShell 中执行：</p>
        <pre class="sync-command-block">{{ syncCommandText }}</pre>
        <p class="sync-modal-note">
          运行完成后，重启应用会自动检测同步状态。执行期间请不要打开应用，避免锁住数据库。
        </p>
        <div class="sync-modal-actions">
          <button
            class="sync-btn-primary"
            :disabled="isFullSyncing"
            @click="startFullSync"
          >
            应用内立即同步
          </button>
          <button class="sync-btn-default" @click="copySyncCommand">复制命令</button>
          <button class="sync-btn-default" @click="showSyncCommandDialog = false">关闭</button>
        </div>
      </div>
    </div>

    <!-- OCR 引擎重做对话框 -->
    <div
      v-if="showRequeueDialog"
      class="requeue-modal-overlay"
      @click="closeRequeueDialog"
    >
      <div
        class="requeue-modal"
        role="dialog"
        aria-labelledby="requeue-modal-title"
        @click.stop
      >
        <h3 id="requeue-modal-title">按 OCR 引擎重做</h3>
        <p class="requeue-modal-desc">
          选择重做范围。被选中的记录会从 Tantivy 索引删除并重置为 pending，下次「处理待索引/OCR」时会优先用 MinerU 重新处理。
        </p>

        <div v-if="isLoadingOcrStats" class="requeue-loading">加载引擎统计中...</div>

        <div v-else-if="ocrEngineStats" class="requeue-stats">
          <div class="requeue-stat-item">
            <span class="stat-label">pp_ocrv4（本地 OCR）</span>
            <span class="stat-value">{{ ocrEngineStats.ppOcrv4 }}</span>
          </div>
          <div class="requeue-stat-item">
            <span class="stat-label">pdfium（PDF 文本提取）</span>
            <span class="stat-value">{{ ocrEngineStats.pdfium }}</span>
          </div>
          <div class="requeue-stat-item">
            <span class="stat-label">mineru（已用 MinerU）</span>
            <span class="stat-value">{{ ocrEngineStats.mineru }}</span>
          </div>
          <div class="requeue-stat-item">
            <span class="stat-label">unknown（无法判定）</span>
            <span class="stat-value">{{ ocrEngineStats.unknown }}</span>
          </div>
          <div class="requeue-stat-item requeue-stat-total">
            <span class="stat-label">合计 done</span>
            <span class="stat-value">{{ ocrEngineStats.totalDone }}</span>
          </div>
        </div>

        <div v-if="ocrEngineStats" class="requeue-scope-list">
          <label class="requeue-scope-item requeue-scope-recommended">
            <input
              v-model="requeueScope"
              type="radio"
              value="scan_only"
              :disabled="isRequeueingForMineru"
            />
            <span>
              <strong>仅扫描件</strong>（pp_ocrv4 + unknown =
              {{ ocrEngineStats.scanOnly }} 条）— 推荐
              <em class="requeue-scope-hint">pdfium 是文本型 PDF，重做无质量提升</em>
            </span>
          </label>
          <label class="requeue-scope-item">
            <input
              v-model="requeueScope"
              type="radio"
              value="pp_ocrv4"
              :disabled="isRequeueingForMineru"
            />
            <span>
              仅 <strong>pp_ocrv4</strong>（{{ ocrEngineStats.ppOcrv4 }} 条）
            </span>
          </label>
          <label class="requeue-scope-item">
            <input
              v-model="requeueScope"
              type="radio"
              value="unknown"
              :disabled="isRequeueingForMineru"
            />
            <span>
              仅 <strong>unknown</strong>（{{ ocrEngineStats.unknown }} 条）
            </span>
          </label>
          <label class="requeue-scope-item requeue-scope-danger">
            <input
              v-model="requeueScope"
              type="radio"
              value="non_mineru"
              :disabled="isRequeueingForMineru"
            />
            <span>
              含 <strong>pdfium</strong>：非 MinerU（{{ ocrEngineStats.nonMineru }} 条）— 慎用
            </span>
          </label>
          <label class="requeue-scope-item requeue-scope-danger">
            <input
              v-model="requeueScope"
              type="radio"
              value="pdfium"
              :disabled="isRequeueingForMineru"
            />
            <span>
              仅 <strong>pdfium</strong>（{{ ocrEngineStats.pdfium }} 条）— 慎用
            </span>
          </label>
          <label class="requeue-scope-item requeue-scope-danger">
            <input
              v-model="requeueScope"
              type="radio"
              value="all_done"
              :disabled="isRequeueingForMineru"
            />
            <span>
              <strong>全部 done</strong>（{{ ocrEngineStats.totalDone }} 条）— 极慎用
            </span>
          </label>
        </div>

        <p v-if="ocrEngineStats" class="requeue-modal-note">
          预计将重置 <strong>{{ requeueExpectedCount }}</strong> 条记录。
        </p>

        <div class="requeue-modal-actions">
          <button
            class="requeue-btn-primary"
            :disabled="
              isRequeueingForMineru ||
              isLoadingOcrStats ||
              requeueExpectedCount === 0
            "
            @click="confirmRequeue"
          >
            {{ isRequeueingForMineru ? '重置中...' : '确认重置' }}
          </button>
          <button
            class="requeue-btn-default"
            :disabled="isRequeueingForMineru"
            @click="closeRequeueDialog"
          >
            取消
          </button>
        </div>
      </div>
    </div>

    <!-- 搜索区域 -->
    <div class="search-section" role="search" aria-label="规章搜索">
      <!-- 搜索框 -->
      <div class="search-bar">
        <input
          v-model="searchState.keyword"
          type="text"
          placeholder="输入关键词搜索..."
          class="search-input"
          aria-label="规章搜索关键词"
          @keydown.enter="handleSearch"
        />
        <button class="search-btn" :disabled="!canSearch || isLocalSearching" @click="handleSearch">
          <template v-if="isInitializing">
            <span class="btn-spinner"></span>
            启动服务中...
          </template>
          <template v-else-if="isLoading || isLocalSearching">
            <span class="btn-spinner"></span>
            搜索中...
          </template>
          <template v-else> 搜索 </template>
        </button>
      </div>

      <!-- 搜索模式切换 -->
      <div class="search-mode-section">
        <div class="mode-buttons" role="tablist" aria-label="搜索模式">
          <button
            v-for="option in searchModeOptions"
            :key="option.value"
            :class="['mode-btn', { active: searchMode === option.value }]"
            :title="option.desc"
            :aria-selected="searchMode === option.value"
            role="tab"
            @click="searchMode = option.value"
          >
            {{ option.label }}
          </button>
        </div>
        <label class="snippet-toggle-label">
          <input v-model="showSnippets" type="checkbox" />
          显示内容摘要
        </label>
        <div v-if="isLocalIndexReady" class="local-index-info">
          <span class="index-status-dot" title="本地索引已就绪"></span>
          <span class="index-status-text">本地索引就绪</span>
        </div>
      </div>

      <div v-if="searchFeedback" class="search-feedback" role="status" aria-live="polite">
        <span class="search-feedback-summary">{{ searchFeedback.summary }}</span>
        <span v-if="searchFeedback.hint" class="search-feedback-hint">
          {{ searchFeedback.hint }}
        </span>
      </div>

      <!-- 主操作区(常用) -->
      <div class="action-bar" role="toolbar" aria-label="规章管理操作">
        <button
          class="action-btn primary"
          :disabled="isScanning || isScanningSelectedFolders"
          title="选择本地文件夹,程序会递归收集 PDF / TXT 并加入索引"
          @click="handleAddScanFolder"
        >
          <span class="action-icon" aria-hidden="true">📁</span>
          {{ isScanningSelectedFolders ? '扫描中...' : '添加并扫描本地文件夹' }}
        </button>
        <button
          class="action-btn"
          :disabled="isProcessingFiles && processingStage !== 'ocr'"
          title="先提取 PDF 自带文字;扫描件会自动进入 OCR 识别并写入索引"
          @click="handleProcessPendingAction"
        >
          {{
            isProcessingFiles && processingStage === 'ocr'
              ? isOcrCancelRequested
                ? '中止中...'
                : '中止 OCR'
              : processPendingButtonLabel
          }}
        </button>
        <button
          class="action-btn"
          :class="{ warning: isSyncing }"
          title="从 CAAC 官网全量爬取规章列表,与本地对比差异"
          @click="handleSyncCompare"
        >
          <span class="action-icon" aria-hidden="true">↻</span>
          {{ isSyncing ? (isSyncCancelRequested ? '中止中...' : '中止同步') : '同步对比官网' }}
        </button>

        <div class="action-spacer"></div>

        <button
          class="maintenance-trigger"
          :class="{ active: showMaintenanceMenu }"
          :aria-expanded="showMaintenanceMenu"
          aria-controls="maintenance-menu"
          title="高级维护工具(对齐文件名 / 重做 OCR / 清理记录 等)"
          @click="showMaintenanceMenu = !showMaintenanceMenu"
        >
          维护工具
          <span class="caret" aria-hidden="true">{{ showMaintenanceMenu ? '▴' : '▾' }}</span>
        </button>
      </div>

      <!-- 维护工具折叠区 -->
      <div v-if="showMaintenanceMenu" id="maintenance-menu" class="maintenance-menu" role="menu">
        <button
          class="maintenance-item"
          role="menuitem"
          :disabled="isRealigningFilenames || isProcessingFiles"
          @click="handleRealignFilenames"
        >
          <span class="maint-name">{{
            isRealigningFilenames ? '对齐中...' : '一键对齐文件名'
          }}</span>
          <span class="maint-desc">把磁盘上的 hash 文件名批量重命名为「文号_标题.ext」</span>
        </button>
        <button
          class="maintenance-item"
          role="menuitem"
          :disabled="isRequeueingForMineru || isProcessingFiles"
          @click="openRequeueDialog"
        >
          <span class="maint-name">{{
            isRequeueingForMineru ? '重置中...' : '重做扫描件 OCR（优先 MinerU）'
          }}</span>
          <span class="maint-desc">默认只重置扫描件范围，之后走主 OCR 队列</span>
        </button>
        <button
          v-if="(dbSyncStatus?.failed_ocr ?? 0) > 0"
          class="maintenance-item warning"
          role="menuitem"
          :disabled="isRetryingOcr || isProcessingFiles"
          @click="handleRetryFailedOcr"
        >
          <span class="maint-name">{{
            isRetryingOcr ? '重试中...' : `重试失败 OCR (${dbSyncStatus?.failed_ocr ?? 0})`
          }}</span>
          <span class="maint-desc">把 failed 记录重置为 pending 并立即重新 OCR</span>
        </button>
        <button
          v-if="(dbSyncStatus?.failed_ocr ?? 0) > 0"
          class="maintenance-item"
          role="menuitem"
          :disabled="isCleaningInvalid || isProcessingFiles"
          @click="handleCleanInvalid"
        >
          <span class="maint-name">{{ isCleaningInvalid ? '清理中...' : '清理无效记录' }}</span>
          <span class="maint-desc">删除磁盘已不存在或后缀不是 PDF/TXT 的残留记录</span>
        </button>
      </div>

      <!-- 扫描文件夹列表(始终显示,空态有引导卡片) -->
      <div class="scan-folder-section">
        <div class="scan-folder-header">
          <span>已添加扫描文件夹 {{ scanFolders.length }} 个</span>
          <div class="scan-folder-header-actions">
            <button
              v-if="scanFolders.length > 0"
              class="scan-folder-rescan"
              :disabled="!canScanSelectedFolders"
              title="重扫所有已添加的文件夹"
              @click="handleScanSelectedFolders"
            >
              {{ isScanningSelectedFolders ? '重扫中...' : '重扫全部' }}
            </button>
            <button
              v-if="scanFolders.length > 0"
              class="scan-folder-clear"
              :disabled="isScanning || isScanningSelectedFolders"
              @click="handleClearScanFolders"
            >
              清空
            </button>
          </div>
        </div>

        <div v-if="scanFolders.length === 0" class="scan-folder-empty">
          <span class="scan-folder-empty-icon" aria-hidden="true">📂</span>
          <div class="scan-folder-empty-text">
            <p class="scan-folder-empty-title">还没添加扫描文件夹</p>
            <p class="scan-folder-empty-hint">
              点上方「<strong>📁 添加并扫描本地文件夹</strong>」按钮选择 PDF/TXT
              所在目录,程序会递归收集所有 PDF/TXT 加入索引
            </p>
          </div>
        </div>

        <div v-else class="scan-folder-list">
          <div
            v-for="folder in scanFolders"
            :key="folder"
            :class="['scan-folder-item', { active: currentScanFolder === folder }]"
          >
            <div class="scan-folder-text">
              <span class="scan-folder-name">{{ formatFolderName(folder) }}</span>
              <span class="scan-folder-path" :title="folder">{{ folder }}</span>
            </div>
            <span
              v-if="isScanningSelectedFolders && currentScanFolder === folder"
              class="scan-folder-status"
            >
              扫描中
            </span>
            <button
              class="scan-folder-action"
              :disabled="isScanning || isScanningSelectedFolders"
              title="重新扫描此文件夹"
              @click="handleScanFolder(folder)"
            >
              扫描
            </button>
            <button
              class="scan-folder-remove"
              :disabled="isScanning || isScanningSelectedFolders"
              @click="handleRemoveScanFolder(folder)"
            >
              移除
            </button>
          </div>
        </div>
      </div>

      <div v-if="isProcessingFiles && processingProgressText" class="processing-status">
        {{ processingProgressText }}
      </div>

      <!-- 当前扫描文件夹横幅(批量扫描时显示) -->
      <div
        v-if="isScanningSelectedFolders && currentScanFolder"
        class="scanning-folder-banner"
        role="status"
        aria-live="polite"
      >
        <span class="scanning-folder-spinner" aria-hidden="true"></span>
        <div class="scanning-folder-text">
          <strong>正在扫描文件夹</strong>
          <span class="scanning-folder-path" :title="currentScanFolder">{{ currentScanFolder }}</span>
        </div>
        <span class="scanning-folder-count"
          >{{ scanFolders.indexOf(currentScanFolder) + 1 }} / {{ scanFolders.length }}</span
        >
      </div>

      <!-- 扫描进度 -->
      <div v-if="isScanning && scanProgress" class="scan-progress">
        <div class="scan-progress-bar">
          <div class="scan-progress-fill" :style="{ width: scanProgressPercent }"></div>
        </div>
        <div class="scan-progress-info">
          <span v-if="scanProgress.phase === 'discovering'">正在发现文件...</span>
          <span v-else-if="scanProgress.phase === 'ocr'">
            OCR 识别中 {{ scanProgress.ocr_processed ?? 0 }}/{{
              scanProgress.ocr_total ?? 0
            }}
            （文本提取已完成 {{ scanProgress.indexed }} 个）
          </span>
          <span v-else>
            {{ scanProgress.scanned }}/{{ scanProgress.total_found }} | 新增
            {{ scanProgress.new_files }} | 重复 {{ scanProgress.duplicates }} | 索引
            {{ scanProgress.indexed }} | 待OCR {{ scanProgress.needs_ocr }}
          </span>
        </div>
        <div
          v-if="scanProgress.current_file"
          class="scan-current-file"
          :title="scanProgress.current_file"
        >
          {{ scanProgress.current_file }}
        </div>
      </div>

      <!-- 扫描结果 -->
      <div v-if="scanResult && !isScanning" class="scan-result">
        <span>扫描完成: 发现 {{ scanResult.total_found }} 个文件</span>
        <span>| 新增 {{ scanResult.new_files }}</span>
        <span>| 重复 {{ scanResult.duplicates }}</span>
        <span>| 直接索引 {{ scanResult.indexed }}</span>
        <span v-if="scanResult.ocr_success > 0">| OCR 索引 {{ scanResult.ocr_success }}</span>
        <span v-if="scanResult.ocr_failed > 0" class="scan-failed"
          >| OCR 失败 {{ scanResult.ocr_failed }}</span
        >
        <span v-if="scanResult.failed > 0" class="scan-failed"
          >| 失败 {{ scanResult.failed }}</span
        >
        <button
          v-if="scanResult.ocr_failed > 0 || (dbSyncStatus && dbSyncStatus.failed_ocr > 0)"
          class="retry-ocr-btn"
          :disabled="isRetryingOcr"
          title="重新处理失败的 OCR 文件"
          @click="handleRetryFailedOcr"
        >
          {{
            isRetryingOcr
              ? '重试中...'
              : `重试 OCR (${scanResult.ocr_failed || dbSyncStatus?.failed_ocr || 0})`
          }}
        </button>
      </div>

      <!-- 同步对比结果 -->
      <div v-if="syncResult && !isSyncing" class="sync-result">
        <div class="sync-summary">
          <span>同步对比: 在线 {{ syncResult.online_total }} 条</span>
          <span>| 已匹配 {{ syncResult.matched }}</span>
          <span v-if="syncResult.new_regulations.length > 0" class="sync-new">
            | 新增 {{ syncResult.new_regulations.length }}
          </span>
          <span v-if="syncResult.changed_regulations.length > 0">
            | 变化 {{ syncResult.changed_regulations.length }}
          </span>
          <span v-if="syncResult.local_only > 0">| 仅本地 {{ syncResult.local_only }}</span>
          <span v-if="(syncResult.downloaded ?? 0) > 0" class="sync-new">
            | 已下载 {{ syncResult.downloaded }}
          </span>
          <span v-if="(syncResult.download_failed ?? 0) > 0" class="scan-failed">
            | 下载失败 {{ syncResult.download_failed }}
          </span>
        </div>
        <div v-if="syncResult.new_regulations.length > 0" class="sync-new-list">
          <div class="sync-list-header">新增规章 ({{ syncResult.new_regulations.length }} 条)</div>
          <div
            v-for="reg in syncResult.new_regulations.slice(0, 20)"
            :key="reg.url"
            class="sync-item"
          >
            <span class="sync-item-title" @click="openUrl(reg.url)">{{ reg.title }}</span>
            <span v-if="reg.doc_number" class="sync-item-meta">{{ reg.doc_number }}</span>
            <span
              :class="[
                'sync-item-validity',
                reg.online_validity === '有效' ? 'valid' : 'invalid',
              ]"
            >
              {{ reg.online_validity }}
            </span>
          </div>
          <div v-if="syncResult.new_regulations.length > 20" class="sync-more">
            ... 还有 {{ syncResult.new_regulations.length - 20 }} 条
          </div>
        </div>
      </div>

      <!-- 筛选条件 -->
      <div class="filter-section">
        <!-- 文档类型 -->
        <div class="filter-group">
          <span class="filter-label">类型：</span>
          <div class="filter-buttons" role="tablist" aria-label="文档类型筛选">
            <button
              v-for="option in docTypeOptions"
              :key="option.value"
              :class="['filter-btn', { active: searchState.docType === option.value }]"
              :aria-selected="searchState.docType === option.value"
              role="tab"
              @click="handleDocTypeChange(option.value)"
            >
              {{ option.label }}
            </button>
          </div>
        </div>

        <!-- 有效性筛选 -->
        <div class="filter-group">
          <span class="filter-label">有效性：</span>
          <div class="filter-buttons" role="tablist" aria-label="有效性筛选">
            <button
              v-for="option in validityFilterOptions"
              :key="option.value"
              :class="['filter-btn', { active: searchState.validity === option.value }]"
              :aria-selected="searchState.validity === option.value"
              role="tab"
              @click="handleValidityClick(option.value)"
            >
              {{ option.label }}
            </button>
          </div>
        </div>

        <!-- 日期筛选 -->
        <div class="filter-group">
          <span class="filter-label">时间：</span>
          <div class="filter-buttons" role="tablist" aria-label="日期筛选">
            <button
              v-for="option in dateFilterOptions"
              :key="option.value"
              :class="['filter-btn', { active: searchState.dateFilter === option.value }]"
              :aria-selected="searchState.dateFilter === option.value"
              role="tab"
              @click="handleDateFilterChange(option.value)"
            >
              {{ option.label }}
            </button>
          </div>
        </div>

        <!-- 自定义日期 -->
        <div v-if="showCustomDatePicker" class="custom-date-picker">
          <input v-model="searchState.startDate" type="date" class="date-input" />
          <span>至</span>
          <input v-model="searchState.endDate" type="date" class="date-input" />
        </div>
      </div>
    </div>

    <!-- 统计卡片 -->
    <div v-if="hasResults" class="stats-cards">
      <div
        :class="['stat-card', { active: searchState.validity === 'all' }]"
        @click="handleValidityClick('all')"
      >
        <span class="stat-value">{{ totalCount }}</span>
        <span class="stat-label">全部</span>
      </div>
      <div
        :class="['stat-card valid', { active: searchState.validity === 'valid' }]"
        @click="handleValidityClick('valid')"
      >
        <span class="stat-value">{{ validCount }}</span>
        <span class="stat-label">有效</span>
      </div>
      <div
        :class="['stat-card invalid', { active: searchState.validity === 'invalid' }]"
        @click="handleValidityClick('invalid')"
      >
        <span class="stat-value">{{ invalidCount }}</span>
        <span class="stat-label">失效</span>
      </div>
    </div>

    <!-- 错误提示 -->
    <div v-if="error" class="error-message">
      {{ error }}
    </div>

    <!-- 批量操作栏 -->
    <div v-if="hasResults" class="batch-actions">
      <label class="checkbox-label">
        <input
          type="checkbox"
          :checked="selectedCount === totalCount && totalCount > 0"
          :indeterminate="selectedCount > 0 && selectedCount < totalCount"
          @change="toggleSelectAll"
        />
        全选
      </label>
      <span v-if="hasSelection" class="selection-info"> 已选 {{ selectedCount }} 项 </span>
      <button
        v-if="hasSelection"
        class="batch-download-btn"
        :disabled="!canDownload"
        @click="handleBatchDownload"
      >
        批量下载
      </button>
    </div>

    <!-- 结果列表 -->
    <div class="results-section">
      <div v-if="isInitializing" class="loading">
        <div class="spinner"></div>
        <span>正在启动规章查询服务...</span>
      </div>

      <div v-else-if="isLoading || isLocalSearching" class="loading">
        <div class="spinner"></div>
        <span>{{ isLocalSearching ? '正在本地搜索...' : '正在搜索...' }}</span>
      </div>

      <div v-else-if="!hasResults && !error" class="empty-state">
        <p>{{ emptyStateMessage }}</p>
        <p v-if="emptyStateHint" class="empty-state-hint">{{ emptyStateHint }}</p>
      </div>

      <div v-else class="results-list">
        <div
          v-for="doc in results"
          :key="doc.url"
          :class="['result-card', { selected: selectedDocs.has(doc.url) }]"
        >
          <div class="card-checkbox">
            <input
              type="checkbox"
              :checked="selectedDocs.has(doc.url)"
              @change="toggleSelection(doc)"
            />
          </div>

          <div class="card-content">
            <div class="card-header">
              <span :class="['doc-type-badge', doc.doc_type]">
                {{ getDocTypeLabel(doc.doc_type) }}
              </span>
              <span :class="['validity-badge', getValidityClass(doc)]">
                {{ getValidityLabel(doc) }}
              </span>
            </div>

            <!-- eslint-disable vue/no-v-html -- highlightTitle() 先 escapeHtml 再 <mark> 包裹，安全 -->
            <!--
              使用 lastSearchKeyword 而不是 searchState.keyword：摘要 HTML 是后端按
              上次实际查询的关键词生成的，标题高亮也只能用上次实际查询关键词，否则
              用户在搜索框中边输边改时，标题与摘要会出现两套不同的高亮，造成困惑。
            -->
            <h3
              class="card-title"
              @click="openUrl(doc.url)"
              v-html="highlightTitle(doc.title, lastSearchKeyword)"
            ></h3>
            <!-- eslint-enable vue/no-v-html -->

            <div class="card-meta">
              <span v-if="doc.doc_number" class="meta-item"> 文号: {{ doc.doc_number }} </span>
              <span v-if="doc.publish_date" class="meta-item"> 发布: {{ doc.publish_date }} </span>
              <span v-if="doc.office_unit" class="meta-item">
                {{ doc.office_unit }}
              </span>
            </div>

            <!-- eslint-disable vue/no-v-html -- 已通过 sanitizeHtml() 白名单消毒，仅保留 b/em/mark 标签 -->
            <div
              v-if="showSnippets && getDisplaySnippet(doc)"
              class="card-snippet"
              v-html="sanitizeHtml(getDisplaySnippet(doc)!)"
            ></div>
            <!-- eslint-enable vue/no-v-html -->
          </div>

          <div class="card-actions">
            <button
              v-if="doc.file_path || doc.url?.startsWith('local://')"
              class="open-btn"
              @click="handleOpenLocal(doc)"
            >
              打开
            </button>
            <button
              v-else
              class="download-btn"
              :disabled="!canDownload || downloadingDoc?.url === doc.url"
              @click="handleDownload(doc)"
            >
              {{ downloadingDoc?.url === doc.url ? '下载中...' : '下载' }}
            </button>
          </div>
        </div>
      </div>
    </div>

    <!-- Toast 通知 -->
    <Transition name="toast-slide">
      <div
        v-if="toast.visible"
        :class="['toast-notification', `toast-${toast.type}`]"
        @click="hideToast"
      >
        {{ toast.message }}
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.regulation-panel {
  /* 统一引用全局主题变量，确保主题切换正常传播 */
  --bg-primary: var(--color-bg-primary, #1c1c1e);
  --bg-secondary: var(--color-bg-secondary, #2c2c2e);
  --bg-hover: var(--color-bg-tertiary, #3a3a3c);
  --text-primary: var(--color-text-primary, #fff);
  --text-secondary: var(--color-text-secondary, #ebebf599);
  --border-color: var(--color-border, #38383a);
  --primary-color: var(--color-accent, #0a84ff);
  --primary-color-dark: var(--color-accent-active, #007aff);
  --primary-color-light: var(--color-accent-light, rgba(10, 132, 255, 0.15));

  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg-primary);
  color: var(--text-primary);
}

/* 标题栏 */
.panel-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 0 0 0 20px;
  height: 40px;
  border-bottom: 1px solid var(--border-color, #e5e5e5);
}

.panel-header h2 {
  margin: 0;
  font-size: 18px;
  font-weight: 600;
}

.header-actions {
  display: flex;
  align-items: center;
  margin-left: auto;
  -webkit-app-region: no-drag;
}

.settings-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  border: none;
  background: transparent;
  color: var(--text-secondary, #666);
  cursor: pointer;
  border-radius: 4px;
  transition: background-color 0.15s;
}

.settings-btn:hover {
  background: var(--bg-hover, rgba(0, 0, 0, 0.06));
}

.window-controls {
  display: flex;
  -webkit-app-region: no-drag;
  height: 100%;
}

.control-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 46px;
  height: 100%;
  border: none;
  background: transparent;
  color: var(--text-secondary, #666);
  cursor: pointer;
  transition: background-color 0.15s;
}

.control-btn:hover {
  background: var(--bg-hover, rgba(0, 0, 0, 0.06));
}

.close-btn:hover {
  background: #e81123 !important;
  color: #fff;
}

/* 数据库统计仪表板 */
.dashboard-section {
  padding: 10px 20px;
  border-bottom: 1px solid var(--border-color, #333);
}

.dashboard-stats {
  display: flex;
  gap: 16px;
  margin-bottom: 8px;
}

.dash-stat {
  display: flex;
  align-items: baseline;
  gap: 4px;
}

.dash-stat-value {
  font-size: 16px;
  font-weight: 600;
  color: var(--text-primary, #e0e0e0);
}

.dash-stat-label {
  font-size: 11px;
  color: var(--text-secondary, #888);
}

.dash-stat.indexed .dash-stat-value {
  color: #52c41a;
}

.dash-stat.pending .dash-stat-value {
  color: #faad14;
}

.dash-stat.failed .dash-stat-value {
  color: #ff4d4f;
}

.dashboard-bar {
  display: flex;
  height: 4px;
  background: var(--border-color, #333);
  border-radius: 2px;
  overflow: hidden;
}

.dashboard-bar-fill {
  height: 100%;
  transition: width 0.5s ease;
}

.dashboard-bar-fill.indexed {
  background: #52c41a;
}

.dashboard-bar-fill.pending {
  background: #faad14;
}

.dashboard-bar-fill.failed {
  background: #ff4d4f;
}

/* 搜索区域 */
.search-section {
  padding: 16px 20px;
  border-bottom: 1px solid var(--border-color, #e5e5e5);
}

/* 服务器同步 banner */
.server-sync-banner {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 16px;
  margin: 12px 20px 0;
  background: rgba(24, 144, 255, 0.08);
  border: 1px solid rgba(24, 144, 255, 0.3);
  border-radius: 6px;
}

.sync-banner-icon {
  font-size: 20px;
  color: #1890ff;
  flex-shrink: 0;
}

.sync-banner-text {
  flex: 1;
  min-width: 0;
}

.sync-banner-text strong {
  display: block;
  font-size: 14px;
  color: #1890ff;
  margin-bottom: 2px;
}

.sync-banner-text p {
  margin: 0;
  font-size: 12px;
  color: var(--text-secondary, #666);
  line-height: 1.5;
}

.sync-banner-actions {
  display: flex;
  gap: 8px;
  flex-shrink: 0;
}

.sync-btn-primary {
  padding: 6px 14px;
  background: #1890ff;
  color: white;
  border: none;
  border-radius: 4px;
  font-size: 12px;
  cursor: pointer;
  transition: background 0.2s;
}

.sync-btn-primary:hover {
  background: #40a9ff;
}

.sync-btn-text {
  padding: 6px 12px;
  background: transparent;
  color: var(--text-secondary, #999);
  border: none;
  font-size: 12px;
  cursor: pointer;
}

.sync-btn-text:hover {
  color: var(--text-primary, #333);
}

.sync-btn-default {
  padding: 8px 16px;
  background: transparent;
  color: var(--text-primary, #333);
  border: 1px solid var(--border-color, #d9d9d9);
  border-radius: 4px;
  font-size: 13px;
  cursor: pointer;
}

.sync-btn-default:hover {
  border-color: #1890ff;
  color: #1890ff;
}

/* 同步命令对话框 */
.sync-modal-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 9999;
}

.sync-modal {
  background: var(--bg-primary, #fff);
  padding: 24px;
  border-radius: 8px;
  max-width: 600px;
  width: 90%;
  max-height: 80vh;
  overflow-y: auto;
  box-shadow: 0 6px 24px rgba(0, 0, 0, 0.2);
}

.sync-modal h3 {
  margin: 0 0 12px;
  font-size: 16px;
  color: var(--text-primary, #333);
}

.sync-modal-desc {
  margin: 0 0 8px;
  font-size: 13px;
  color: var(--text-secondary, #666);
}

.sync-command-block {
  background: var(--bg-secondary, #f5f5f5);
  padding: 12px 14px;
  border-radius: 4px;
  font-family: 'Consolas', 'Monaco', 'Courier New', monospace;
  font-size: 12px;
  line-height: 1.6;
  overflow-x: auto;
  user-select: text;
  white-space: pre-wrap;
  word-break: break-all;
  color: var(--text-primary, #333);
  margin: 0;
}

.sync-modal-note {
  margin: 12px 0;
  font-size: 12px;
  color: var(--text-secondary, #999);
  line-height: 1.6;
}

.sync-modal-actions {
  display: flex;
  gap: 8px;
  justify-content: flex-end;
  margin-top: 16px;
}

.sync-btn-primary:disabled,
.sync-btn-default:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* OCR 引擎重做对话框 */
.requeue-modal-overlay {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 9999;
}

.requeue-modal {
  background: var(--bg-primary, #fff);
  padding: 22px 24px;
  border-radius: 8px;
  width: min(520px, 90vw);
  max-height: 85vh;
  overflow: auto;
  box-shadow: 0 6px 24px rgba(0, 0, 0, 0.2);
}

.requeue-modal h3 {
  margin: 0 0 10px;
  font-size: 16px;
  color: var(--text-primary, #333);
}

.requeue-modal-desc {
  margin: 0 0 12px;
  font-size: 13px;
  line-height: 1.6;
  color: var(--text-secondary, #666);
}

.requeue-loading {
  padding: 16px 0;
  text-align: center;
  font-size: 13px;
  color: var(--text-secondary, #999);
}

.requeue-stats {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 6px 16px;
  padding: 10px 12px;
  margin-bottom: 14px;
  background: rgba(24, 144, 255, 0.04);
  border: 1px solid rgba(24, 144, 255, 0.15);
  border-radius: 6px;
}

.requeue-stat-item {
  display: flex;
  justify-content: space-between;
  align-items: baseline;
  font-size: 12px;
  color: var(--text-secondary, #666);
}

.requeue-stat-item .stat-value {
  font-weight: 600;
  color: var(--text-primary, #333);
  font-variant-numeric: tabular-nums;
}

.requeue-stat-total {
  grid-column: 1 / -1;
  padding-top: 6px;
  border-top: 1px dashed rgba(0, 0, 0, 0.08);
}

.requeue-scope-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin-bottom: 12px;
}

.requeue-scope-item {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 8px 10px;
  font-size: 13px;
  border: 1px solid rgba(0, 0, 0, 0.08);
  border-radius: 6px;
  cursor: pointer;
  transition: background 0.15s, border-color 0.15s;
  color: var(--text-primary, #333);
}

.requeue-scope-item:hover {
  background: rgba(24, 144, 255, 0.04);
  border-color: rgba(24, 144, 255, 0.4);
}

.requeue-scope-item input[type='radio'] {
  margin-top: 2px;
}

.requeue-scope-danger {
  border-color: rgba(255, 77, 79, 0.3);
}

.requeue-scope-danger:hover {
  background: rgba(255, 77, 79, 0.05);
  border-color: rgba(255, 77, 79, 0.5);
}

.requeue-scope-recommended {
  border-color: rgba(82, 196, 26, 0.45);
  background: rgba(82, 196, 26, 0.05);
}

.requeue-scope-recommended:hover {
  background: rgba(82, 196, 26, 0.08);
  border-color: rgba(82, 196, 26, 0.6);
}

.requeue-scope-hint {
  display: block;
  margin-top: 2px;
  font-size: 11px;
  font-style: normal;
  color: var(--text-secondary, #999);
}

.requeue-modal-note {
  margin: 6px 0 0;
  font-size: 12px;
  color: var(--text-secondary, #999);
}

.requeue-modal-note strong {
  color: #1890ff;
  font-variant-numeric: tabular-nums;
}

.requeue-modal-actions {
  display: flex;
  gap: 8px;
  justify-content: flex-end;
  margin-top: 16px;
}

.requeue-btn-primary {
  padding: 6px 14px;
  background: #1890ff;
  color: white;
  border: none;
  border-radius: 4px;
  cursor: pointer;
  font-size: 13px;
  transition: background 0.2s;
}

.requeue-btn-primary:hover:not(:disabled) {
  background: #40a9ff;
}

.requeue-btn-default {
  padding: 6px 14px;
  background: transparent;
  color: var(--text-primary, #333);
  border: 1px solid var(--border-secondary, #d9d9d9);
  border-radius: 4px;
  font-size: 13px;
  cursor: pointer;
}

.requeue-btn-default:hover:not(:disabled) {
  border-color: #1890ff;
  color: #1890ff;
}

.requeue-btn-primary:disabled,
.requeue-btn-default:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* 同步中进度 banner */
.server-sync-progress-banner {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 16px;
  margin: 12px 20px 0;
  background: rgba(82, 196, 26, 0.08);
  border: 1px solid rgba(82, 196, 26, 0.3);
  border-radius: 6px;
}

.server-sync-progress-banner .sync-banner-icon {
  color: #52c41a;
}

.server-sync-progress-banner .sync-banner-text strong {
  color: #52c41a;
}

.sync-spinning {
  display: inline-block;
  animation: sync-spin 1.5s linear infinite;
}

@keyframes sync-spin {
  from {
    transform: rotate(0deg);
  }
  to {
    transform: rotate(360deg);
  }
}

.sync-progress-bar {
  width: 100%;
  height: 4px;
  background: rgba(82, 196, 26, 0.15);
  border-radius: 2px;
  margin-top: 6px;
  overflow: hidden;
}

.sync-progress-fill {
  height: 100%;
  background: #52c41a;
  border-radius: 2px;
  transition: width 0.25s ease;
}

.search-bar {
  display: flex;
  gap: 8px;
  margin-bottom: 12px;
}

.search-input {
  flex: 1;
  padding: 10px 14px;
  border: 1px solid var(--border-color, #e5e5e5);
  border-radius: 6px;
  font-size: 14px;
  outline: none;
  transition: border-color 0.2s;
}

.search-input:focus {
  border-color: var(--primary-color, #1890ff);
}

.search-btn {
  padding: 10px 20px;
  background: var(--primary-color, #1890ff);
  color: white;
  border: none;
  border-radius: 6px;
  font-size: 14px;
  cursor: pointer;
  transition: background-color 0.2s;
}

.search-btn:hover:not(:disabled) {
  background: var(--primary-color-dark, #096dd9);
}

.search-btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.search-btn .btn-spinner {
  display: inline-block;
  width: 14px;
  height: 14px;
  border: 2px solid rgba(255, 255, 255, 0.3);
  border-top-color: white;
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
  margin-right: 6px;
  vertical-align: middle;
}

/* 搜索模式切换 */
.search-mode-section {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 12px;
  flex-wrap: wrap;
  gap: 8px;
}

.mode-buttons {
  display: flex;
  gap: 4px;
  background: var(--bg-secondary, #fafafa);
  padding: 3px;
  border-radius: 6px;
  border: 1px solid var(--border-color, #e5e5e5);
}

.mode-btn {
  padding: 5px 12px;
  border: none;
  background: transparent;
  color: var(--text-secondary, #666);
  border-radius: 4px;
  font-size: 12px;
  cursor: pointer;
  transition: all 0.2s;
}

.mode-btn:hover {
  color: var(--text-primary, #333);
}

.mode-btn.active {
  background: var(--primary-color, #1890ff);
  color: white;
}

.local-index-info {
  display: flex;
  align-items: center;
  gap: 12px;
  font-size: 12px;
}

.search-feedback {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 8px;
  margin: -2px 0 12px;
  font-size: 12px;
  line-height: 1.5;
}

.search-feedback-summary {
  color: #52c41a;
  font-weight: 500;
}

.search-feedback-hint {
  color: var(--text-secondary, #999);
}

.scan-add-btn,
.scan-btn {
  padding: 4px 10px;
  border: 1px solid #52c41a;
  background: transparent;
  color: #52c41a;
  border-radius: 4px;
  font-size: 11px;
  cursor: pointer;
  transition: all 0.2s;
}

.scan-add-btn:hover:not(:disabled),
.scan-btn:hover:not(:disabled) {
  background: #52c41a;
  color: white;
}

.scan-add-btn:disabled,
.scan-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.scan-failed {
  color: #ff4d4f;
}

.retry-ocr-btn {
  padding: 5px 12px;
  margin-left: 6px;
  border: 1px solid #ff4d4f;
  background: #fff1f0;
  color: #ff4d4f;
  border-radius: 4px;
  font-size: 12px;
  cursor: pointer;
  transition: all 0.2s;
}

.retry-ocr-btn:hover:not(:disabled) {
  background: #ff4d4f;
  color: white;
}

.retry-ocr-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* 筛选条件 */
.filter-section {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.filter-group {
  display: flex;
  align-items: center;
  gap: 8px;
}

.filter-label {
  font-size: 13px;
  color: var(--text-secondary, #666);
  min-width: 52px;
}

.filter-buttons {
  display: flex;
  gap: 6px;
  flex-wrap: wrap;
}

.filter-btn {
  padding: 4px 12px;
  border: 1px solid var(--border-color, #e5e5e5);
  background: var(--bg-secondary, #fafafa);
  color: var(--text-primary, #333);
  border-radius: 4px;
  font-size: 12px;
  cursor: pointer;
  transition: all 0.2s;
}

.filter-btn:hover {
  border-color: var(--primary-color, #1890ff);
}

.filter-btn.active {
  background: var(--primary-color, #1890ff);
  border-color: var(--primary-color, #1890ff);
  color: white;
}

.custom-date-picker {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 8px;
  padding-left: 48px;
}

.date-input {
  padding: 6px 10px;
  border: 1px solid var(--border-color, #e5e5e5);
  border-radius: 4px;
  font-size: 13px;
}

/* 统计卡片 */
.stats-cards {
  display: flex;
  gap: 12px;
  padding: 12px 20px;
  border-bottom: 1px solid var(--border-color, #e5e5e5);
}

.stat-card {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  padding: 12px;
  background: var(--bg-secondary, #fafafa);
  border: 1px solid var(--border-color, #e5e5e5);
  border-radius: 8px;
  cursor: pointer;
  transition: all 0.2s;
}

.stat-card:hover {
  border-color: var(--primary-color, #1890ff);
}

.stat-card.active {
  background: var(--primary-color-light, #e6f7ff);
  border-color: var(--primary-color, #1890ff);
}

.stat-card.valid .stat-value {
  color: #52c41a;
}

.stat-card.invalid .stat-value {
  color: #ff4d4f;
}

.stat-value {
  font-size: 24px;
  font-weight: 600;
}

.stat-label {
  font-size: 12px;
  color: var(--text-secondary, #666);
  margin-top: 4px;
}

/* 错误信息 */
.error-message {
  padding: 12px 20px;
  background: #fff2f0;
  color: #ff4d4f;
  border-bottom: 1px solid #ffccc7;
}

/* 批量操作栏 */
.batch-actions {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 8px 20px;
  background: var(--bg-secondary, #fafafa);
  border-bottom: 1px solid var(--border-color, #e5e5e5);
}

.checkbox-label {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 13px;
  cursor: pointer;
}

.selection-info {
  font-size: 13px;
  color: var(--text-secondary, #666);
}

.batch-download-btn {
  padding: 6px 14px;
  background: var(--primary-color, #1890ff);
  color: white;
  border: none;
  border-radius: 4px;
  font-size: 13px;
  cursor: pointer;
}

.batch-download-btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

/* 结果区域 */
.results-section {
  flex: 1;
  overflow-y: auto;
  padding: 12px 20px;
}

.loading {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 40px;
  color: var(--text-secondary, #666);
}

.spinner {
  width: 32px;
  height: 32px;
  border: 3px solid var(--border-color, #e5e5e5);
  border-top-color: var(--primary-color, #1890ff);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
  margin-bottom: 12px;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 40px;
  color: var(--text-secondary, #666);
  text-align: center;
}

.empty-state-hint {
  color: var(--text-tertiary, #999);
  font-size: 13px;
}

/* 结果列表 */
.results-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.result-card {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  padding: 14px;
  background: var(--bg-primary, #ffffff);
  border: 1px solid var(--border-color, #e5e5e5);
  border-radius: 8px;
  transition: all 0.2s;
}

.result-card:hover {
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.08);
}

.result-card.selected {
  background: var(--primary-color-light, #e6f7ff);
  border-color: var(--primary-color, #1890ff);
}

.card-checkbox {
  padding-top: 2px;
}

.card-content {
  flex: 1;
  min-width: 0;
}

.card-header {
  display: flex;
  gap: 8px;
  margin-bottom: 8px;
}

.doc-type-badge {
  padding: 2px 8px;
  border-radius: 4px;
  font-size: 11px;
  font-weight: 500;
}

.doc-type-badge.regulation {
  background: #e6f7ff;
  color: #1890ff;
}

.doc-type-badge.normative {
  background: #f6ffed;
  color: #52c41a;
}

.validity-badge {
  padding: 2px 8px;
  border-radius: 4px;
  font-size: 11px;
  font-weight: 500;
}

.validity-badge.validity-valid {
  background: #f6ffed;
  color: #52c41a;
}

.validity-badge.validity-invalid {
  background: #fff2f0;
  color: #ff4d4f;
}

.card-title {
  margin: 0 0 8px 0;
  font-size: 14px;
  font-weight: 500;
  line-height: 1.5;
  cursor: pointer;
  color: var(--text-primary, #333);
  transition: color 0.2s;
}

.card-title:hover {
  color: var(--primary-color, #1890ff);
}

.card-title :deep(mark) {
  background: rgba(74, 158, 255, 0.25);
  color: var(--primary-color, #4a9eff);
  padding: 0 2px;
  border-radius: 2px;
}

.card-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
}

.meta-item {
  font-size: 12px;
  color: var(--text-secondary, #666);
}

.card-actions {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.download-btn {
  padding: 6px 14px;
  background: var(--bg-secondary, #fafafa);
  border: 1px solid var(--border-color, #e5e5e5);
  border-radius: 4px;
  font-size: 12px;
  cursor: pointer;
  transition: all 0.2s;
  white-space: nowrap;
}

.download-btn:hover:not(:disabled) {
  background: var(--primary-color, #1890ff);
  border-color: var(--primary-color, #1890ff);
  color: white;
}

.download-btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.open-btn {
  padding: 6px 14px;
  background: var(--color-success, #34c759);
  color: white;
  border: none;
  border-radius: 4px;
  font-size: 12px;
  cursor: pointer;
  transition: all 0.2s;
  white-space: nowrap;
}

.open-btn:hover {
  opacity: 0.85;
}

/* 摘要预览开关 */
.snippet-toggle-label {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 12px;
  color: var(--text-secondary, #888);
  cursor: pointer;
  user-select: none;
  white-space: nowrap;
}

.snippet-toggle-label input[type='checkbox'] {
  margin: 0;
  cursor: pointer;
}

/* 搜索结果摘要 */
.card-snippet {
  margin-top: 8px;
  padding: 6px 10px;
  border-left: 3px solid var(--primary-color, #4a9eff);
  background: var(--bg-secondary, #242424);
  font-size: 12px;
  line-height: 1.6;
  color: var(--text-secondary, #888);
  /* 放宽到约 11 行，配合后端 420 字符摘要，避免只看到半句上下文 */
  max-height: 212px;
  overflow: hidden;
  word-break: break-word;
}

.card-snippet :deep(mark) {
  background: rgba(74, 158, 255, 0.25);
  color: var(--primary-color, #4a9eff);
  padding: 0 1px;
  border-radius: 2px;
}

/* Toast 通知 */
.toast-notification {
  position: fixed;
  bottom: 24px;
  left: 50%;
  transform: translateX(-50%);
  padding: 10px 24px;
  border-radius: 8px;
  font-size: 14px;
  font-weight: 500;
  color: #fff;
  z-index: 9999;
  cursor: pointer;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.25);
  max-width: 80%;
  text-align: center;
  word-break: break-word;
}

.toast-success {
  background: #22c55e;
}

.toast-error {
  background: #ef4444;
}

.toast-info {
  background: var(--primary-color, #4a9eff);
}

.toast-slide-enter-active,
.toast-slide-leave-active {
  transition: all 0.3s ease;
}

.toast-slide-enter-from,
.toast-slide-leave-to {
  opacity: 0;
  transform: translateX(-50%) translateY(16px);
}

/* ============================================
 * v0.1.7 UX 重构: 主操作栏 + 维护工具折叠 + 文件夹空态
 * ============================================ */

/* 索引就绪指示器(替代之前的"本地已索引 X 篇" badge) */
.index-status-dot {
  display: inline-block;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #52c41a;
  box-shadow: 0 0 4px rgba(82, 196, 26, 0.6);
}

.index-status-text {
  color: var(--text-secondary, #999);
  font-size: 12px;
}

/* 主操作栏 */
.action-bar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  margin-top: 10px;
  padding-top: 10px;
  border-top: 1px dashed var(--border-color, #2f2f33);
}

.action-bar .action-btn {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 6px 14px;
  border: 1px solid var(--border-color, #333);
  background: transparent;
  color: var(--text-primary, #e0e0e0);
  border-radius: 6px;
  font-size: 13px;
  cursor: pointer;
  transition: all 0.2s;
}

.action-bar .action-btn:hover:not(:disabled) {
  border-color: #52c41a;
  color: #52c41a;
}

.action-bar .action-btn.primary {
  border-color: #52c41a;
  color: #52c41a;
  background: rgba(82, 196, 26, 0.06);
  font-weight: 500;
}

.action-bar .action-btn.primary:hover:not(:disabled) {
  background: #52c41a;
  color: white;
}

.action-bar .action-btn.warning {
  border-color: #ff4d4f;
  color: #ff4d4f;
  background: rgba(255, 77, 79, 0.06);
}

.action-bar .action-btn.warning:hover:not(:disabled) {
  background: #ff4d4f;
  color: white;
}

.action-bar .action-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.action-bar .action-icon {
  font-size: 14px;
  line-height: 1;
}

.action-spacer {
  flex: 1;
  min-width: 0;
}

/* 维护工具折叠开关 */
.maintenance-trigger {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  padding: 6px 12px;
  border: 1px dashed var(--border-color, #444);
  background: transparent;
  color: var(--text-secondary, #999);
  border-radius: 6px;
  font-size: 12px;
  cursor: pointer;
  transition: all 0.2s;
}

.maintenance-trigger:hover {
  border-style: solid;
  color: var(--text-primary, #e0e0e0);
}

.maintenance-trigger.active {
  border-style: solid;
  border-color: #1890ff;
  color: #1890ff;
}

.maintenance-trigger .caret {
  font-size: 10px;
  opacity: 0.7;
}

/* 维护工具菜单 */
.maintenance-menu {
  margin-top: 8px;
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
  gap: 8px;
  padding: 10px;
  background: var(--bg-secondary, #2a2a2c);
  border: 1px solid var(--border-color, #333);
  border-radius: 6px;
}

.maintenance-item {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 4px;
  padding: 10px 12px;
  background: var(--bg-primary, #1c1c1e);
  border: 1px solid var(--border-color, #333);
  border-radius: 4px;
  text-align: left;
  cursor: pointer;
  transition: all 0.2s;
}

.maintenance-item:hover:not(:disabled) {
  border-color: #1890ff;
  background: rgba(24, 144, 255, 0.04);
}

.maintenance-item.warning {
  border-color: rgba(255, 77, 79, 0.4);
}

.maintenance-item.warning:hover:not(:disabled) {
  border-color: #ff4d4f;
  background: rgba(255, 77, 79, 0.06);
}

.maintenance-item:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.maintenance-item .maint-name {
  font-size: 13px;
  font-weight: 500;
  color: var(--text-primary, #e0e0e0);
}

.maintenance-item .maint-desc {
  font-size: 11px;
  color: var(--text-secondary, #888);
  line-height: 1.4;
}

/* 扫描文件夹列表头部容器 */
.scan-folder-header-actions {
  display: flex;
  gap: 6px;
  align-items: center;
}

.scan-folder-rescan {
  padding: 2px 10px;
  border: 1px solid rgba(82, 196, 26, 0.45);
  background: transparent;
  color: #52c41a;
  border-radius: 4px;
  font-size: 11px;
  cursor: pointer;
  transition: all 0.2s;
}

.scan-folder-rescan:hover:not(:disabled) {
  background: #52c41a;
  color: white;
}

.scan-folder-rescan:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* 文件夹列表空态卡片 */
.scan-folder-empty {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 16px 18px;
  background: var(--bg-primary, #1c1c1e);
}

.scan-folder-empty-icon {
  font-size: 28px;
  opacity: 0.6;
  flex-shrink: 0;
}

.scan-folder-empty-text {
  flex: 1;
  min-width: 0;
}

.scan-folder-empty-title {
  margin: 0 0 4px 0;
  font-size: 13px;
  font-weight: 500;
  color: var(--text-primary, #e0e0e0);
}

.scan-folder-empty-hint {
  margin: 0;
  font-size: 11px;
  color: var(--text-secondary, #888);
  line-height: 1.5;
}

.scan-folder-empty-hint strong {
  color: #52c41a;
  font-weight: 500;
}

/* 批量扫描文件夹时的横幅 */
.scanning-folder-banner {
  display: flex;
  align-items: center;
  gap: 12px;
  margin-top: 10px;
  padding: 10px 14px;
  background: rgba(24, 144, 255, 0.08);
  border: 1px solid rgba(24, 144, 255, 0.32);
  border-radius: 6px;
  font-size: 12px;
}

.scanning-folder-spinner {
  flex-shrink: 0;
  width: 14px;
  height: 14px;
  border: 2px solid rgba(24, 144, 255, 0.3);
  border-top-color: #1890ff;
  border-radius: 50%;
  animation: scanning-folder-spin 0.8s linear infinite;
}

@keyframes scanning-folder-spin {
  to {
    transform: rotate(360deg);
  }
}

.scanning-folder-text {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.scanning-folder-text strong {
  color: #1890ff;
  font-size: 12px;
  font-weight: 500;
}

.scanning-folder-path {
  color: var(--text-secondary, #999);
  font-size: 11px;
  font-family: 'Consolas', 'Courier New', monospace;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.scanning-folder-count {
  flex-shrink: 0;
  color: #1890ff;
  font-size: 12px;
  font-weight: 500;
  padding: 2px 8px;
  background: rgba(24, 144, 255, 0.15);
  border-radius: 10px;
}
</style>
