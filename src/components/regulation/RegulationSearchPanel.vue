<script setup lang="ts">
/**
 * 规章查询面板组件
 *
 * 提供 CAAC 规章和规范性文件的搜索、下载功能。
 * 支持本地索引搜索（毫秒级）和在线搜索。
 */

import { ref, computed, onMounted, watch } from 'vue'
import { useRegulationQuery, type DateFilter } from '@/composables/useRegulationQuery'
// Store 状态统一通过 useRegulationQuery composable 访问，不再直接导入 store
import type { RegulationDocument, RegulationDocType, RegulationValidity } from '@/types'
import { invoke } from '@tauri-apps/api/core'
import { open as openShell } from '@tauri-apps/plugin-shell'
import { revealItemInDir } from '@tauri-apps/plugin-opener'

const emit = defineEmits<{
  (e: 'close'): void
}>()

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
  download,
  downloadBatchNative,
  processPendingFiles,
  syncCompare,
  refreshDbStatus,
  dbSyncStatus,
  setDocType,
  setValidity,
  setDateFilter,
} = useRegulationQuery()

// 持久化搜索模式
const SEARCH_MODE_KEY = 'regulation-search-mode'
function loadSearchMode(): 'online' | 'local' | 'hybrid' {
  try {
    const saved = localStorage.getItem(SEARCH_MODE_KEY)
    if (saved === 'online' || saved === 'local' || saved === 'hybrid') {
      return saved
    }
  } catch { /* ignore */ }
  return 'hybrid'
}

// 本地状态
const selectedDocs = ref<Set<string>>(new Set())
const showCustomDatePicker = ref(false)
const searchMode = ref<'online' | 'local' | 'hybrid'>(loadSearchMode())
const lastSearchSource = ref<'local' | 'online' | null>(null)

// 监听搜索模式变化，自动持久化
watch(searchMode, (mode) => {
  try {
    localStorage.setItem(SEARCH_MODE_KEY, mode)
  } catch { /* ignore */ }
})
const isProcessingFiles = ref(false)
const isRetryingOcr = ref(false)
const processResult = ref<{ processed: number; indexed: number; needs_ocr: number; failed: number } | null>(null)

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

// 搜索性能提示
const searchPerformanceHint = computed(() => {
  if (lastSearchSource.value === 'local' && localSearchElapsedMs.value > 0) {
    return `本地搜索 ${localSearchElapsedMs.value}ms`
  }
  return null
})

// 初始化本地索引 + 自动发现
onMounted(async () => {
  // 恢复自定义日期选择器状态
  if (searchState.dateFilter === 'custom') {
    showCustomDatePicker.value = true
  }
  await initLocalIndex()
  await refreshDbStatus()

  // 如果本地索引文档数较少，自动触发全盘发现
  if (localDocCount.value < 10) {
    console.log('[RegulationPanel] 本地索引文档较少，触发全盘自动发现...')
    try {
      const result = await invoke<{ new_added?: number }>('regulation_discover_local', {})
      console.log('[RegulationPanel] 全盘发现完成:', result)
      // 刷新状态
      await refreshDbStatus()
      // 重新初始化索引以加载新发现的文件
      if (result?.new_added && result.new_added > 0) {
        await initLocalIndex()
      }
    } catch (err) {
      console.warn('[RegulationPanel] 全盘发现失败:', err)
    }
  }
})

// 获取有效性样式
function getValidityClass(validity: string): string {
  switch (validity) {
    case '有效':
      return 'validity-valid'
    case '失效':
    case '废止':
      return 'validity-invalid'
    default:
      return ''
  }
}

// 获取文档类型标签
function getDocTypeLabel(docType: string): string {
  return docType === 'regulation' ? 'CCAR' : '规范性'
}

// 处理搜索
async function handleSearch(): Promise<void> {
  selectedDocs.value.clear()
  
  switch (searchMode.value) {
    case 'local':
      // 仅本地搜索
      const localResults = await searchLocal()
      if (localResults.length > 0) {
        lastSearchSource.value = 'local'
      }
      break
    case 'online':
      // 仅在线搜索
      lastSearchSource.value = 'online'
      await search()
      break
    case 'hybrid':
    default:
      // 混合搜索：先本地后在线
      await searchHybrid()
      lastSearchSource.value = isLocalIndexReady.value ? 'local' : 'online'
      break
  }
}

// 打开本地文件
async function handleOpenLocal(doc: RegulationDocument): Promise<void> {
  const filePath = doc.file_path || doc.url?.replace('local://', '') || ''
  if (filePath) {
    try {
      await openShell(filePath)
    } catch {
      // 如果 openShell 失败，尝试在文件管理器中显示
      try {
        await revealItemInDir(filePath)
      } catch (e) {
        console.error('打开文件失败:', e)
      }
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
  }
}

// 处理批量下载
async function handleBatchDownload(): Promise<void> {
  const selectedList = results.value.filter((doc) =>
    selectedDocs.value.has(doc.url)
  )

  if (selectedList.length === 0) {
    return
  }

  const { success, skipped, failed } = await downloadBatchNative(selectedList)
  alert(`下载完成：成功 ${success} 个，跳过 ${skipped} 个，失败 ${failed} 个`)
  selectedDocs.value.clear()
}

// 处理待索引文件（PDF 文本提取 + 索引）
async function handleProcessPending(): Promise<void> {
  if (isProcessingFiles.value) return

  isProcessingFiles.value = true
  processResult.value = null

  try {
    const result = await processPendingFiles(20)
    processResult.value = result
    if (result.processed === 0) {
      alert('没有待处理的文件')
    } else {
      alert(`处理完成：索引 ${result.indexed} 个，需OCR ${result.needs_ocr} 个，失败 ${result.failed} 个`)
    }
  } catch (err) {
    alert(`处理失败: ${err}`)
  } finally {
    isProcessingFiles.value = false
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
    }>('regulation_retry_failed_ocr')

    if (result.processed === 0) {
      alert('没有失败的 OCR 文件需要重试')
    } else {
      alert(`重试完成: 成功 ${result.ocr_success}, 仍失败 ${result.ocr_failed}`)
    }

    // 刷新数据库状态
    await refreshDbStatus()
  } catch (err) {
    alert(`重试失败: ${err}`)
  } finally {
    isRetryingOcr.value = false
  }
}

// 同步对比官网
async function handleSyncCompare(): Promise<void> {
  if (isSyncing.value) return
  await syncCompare('all', 20)
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
    results.value.forEach((doc) => selectedDocs.value.add(doc.url))
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
  const newValidity = searchState.validity === validity ? 'all' : validity
  if (searchState.validity === newValidity) return
  setValidity(newValidity)
  await handleSearch()
}
</script>

<template>
  <div class="regulation-panel">
    <!-- 标题栏 -->
    <div class="panel-header">
      <h2>规章查询</h2>
      <button class="close-btn" @click="emit('close')" title="关闭">
        <svg viewBox="0 0 24 24" width="20" height="20">
          <path
            fill="currentColor"
            d="M19 6.41L17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z"
          />
        </svg>
      </button>
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
          <span class="dash-stat-label">待处理</span>
        </div>
        <div class="dash-stat failed" v-if="dbSyncStatus.failed_ocr > 0">
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
          title="待处理"
        ></div>
        <div
          class="dashboard-bar-fill failed"
          :style="{ width: `${(dbSyncStatus.failed_ocr / dbSyncStatus.total_files) * 100}%` }"
          title="失败"
        ></div>
      </div>
    </div>

    <!-- 搜索区域 -->
    <div class="search-section">
      <!-- 搜索框 -->
      <div class="search-bar">
        <input
          v-model="searchState.keyword"
          type="text"
          placeholder="输入关键词搜索..."
          class="search-input"
          @keydown.enter="handleSearch"
        />
        <button
          class="search-btn"
          :disabled="!canSearch"
          @click="handleSearch"
        >
          <template v-if="isInitializing">
            <span class="btn-spinner"></span>
            启动服务中...
          </template>
          <template v-else-if="isLoading || isLocalSearching">
            <span class="btn-spinner"></span>
            搜索中...
          </template>
          <template v-else>
            搜索
          </template>
        </button>
      </div>

      <!-- 搜索模式切换 -->
      <div class="search-mode-section">
        <div class="mode-buttons">
          <button
            v-for="option in searchModeOptions"
            :key="option.value"
            :class="['mode-btn', { active: searchMode === option.value }]"
            :title="option.desc"
            @click="searchMode = option.value"
          >
            {{ option.label }}
          </button>
        </div>
        <div v-if="isLocalIndexReady" class="local-index-info">
          <span class="index-badge">
            本地已索引 {{ localDocCount }} 篇
          </span>
          <span v-if="searchPerformanceHint" class="perf-hint">
            {{ searchPerformanceHint }}
          </span>
          <button
            class="process-btn"
            :disabled="isProcessingFiles"
            @click="handleProcessPending"
            title="处理已下载但未索引的 PDF 文件"
          >
            {{ isProcessingFiles ? '处理中...' : '处理待索引' }}
          </button>
          <button
            class="sync-btn"
            :disabled="isSyncing"
            @click="handleSyncCompare"
            title="从 CAAC 官网全量爬取规章列表，与本地对比差异"
          >
            {{ isSyncing ? '同步中...' : '同步对比官网' }}
          </button>
        </div>

        <!-- 扫描进度 -->
        <div v-if="isScanning && scanProgress" class="scan-progress">
          <div class="scan-progress-bar">
            <div
              class="scan-progress-fill"
              :style="{ width: scanProgressPercent }"
            ></div>
          </div>
          <div class="scan-progress-info">
            <span v-if="scanProgress.phase === 'discovering'">正在发现文件...</span>
            <span v-else-if="scanProgress.phase === 'ocr'">
              OCR 识别中 {{ scanProgress.ocr_processed ?? 0 }}/{{ scanProgress.ocr_total ?? 0 }}
              （文本提取已完成 {{ scanProgress.indexed }} 个）
            </span>
            <span v-else>
              {{ scanProgress.scanned }}/{{ scanProgress.total_found }}
              | 新增 {{ scanProgress.new_files }}
              | 重复 {{ scanProgress.duplicates }}
              | 索引 {{ scanProgress.indexed }}
              | 待OCR {{ scanProgress.needs_ocr }}
            </span>
          </div>
          <div v-if="scanProgress.current_file" class="scan-current-file" :title="scanProgress.current_file">
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
          <span v-if="scanResult.ocr_failed > 0" class="scan-failed">| OCR 失败 {{ scanResult.ocr_failed }}</span>
          <span v-if="scanResult.failed > 0" class="scan-failed">| 失败 {{ scanResult.failed }}</span>
          <button
            v-if="scanResult.ocr_failed > 0 || (dbSyncStatus && dbSyncStatus.failed_ocr > 0)"
            class="retry-ocr-btn"
            :disabled="isRetryingOcr"
            @click="handleRetryFailedOcr"
            title="重新处理失败的 OCR 文件"
          >
            {{ isRetryingOcr ? '重试中...' : `重试 OCR (${scanResult.ocr_failed || dbSyncStatus?.failed_ocr || 0})` }}
          </button>
        </div>

        <!-- 同步对比结果 -->
        <div v-if="syncResult && !isSyncing" class="sync-result">
          <div class="sync-summary">
            <span>同步对比: 在线 {{ syncResult.online_total }} 条</span>
            <span>| 已匹配 {{ syncResult.matched }}</span>
            <span class="sync-new" v-if="syncResult.new_regulations.length > 0">
              | 新增 {{ syncResult.new_regulations.length }}
            </span>
            <span v-if="syncResult.changed_regulations.length > 0">
              | 变化 {{ syncResult.changed_regulations.length }}
            </span>
            <span v-if="syncResult.local_only > 0">| 仅本地 {{ syncResult.local_only }}</span>
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
              <span :class="['sync-item-validity', reg.online_validity === '有效' ? 'valid' : 'invalid']">
                {{ reg.online_validity }}
              </span>
            </div>
            <div v-if="syncResult.new_regulations.length > 20" class="sync-more">
              ... 还有 {{ syncResult.new_regulations.length - 20 }} 条
            </div>
          </div>
        </div>
      </div>

      <!-- 筛选条件 -->
      <div class="filter-section">
        <!-- 文档类型 -->
        <div class="filter-group">
          <span class="filter-label">类型：</span>
          <div class="filter-buttons">
            <button
              v-for="option in docTypeOptions"
              :key="option.value"
              :class="['filter-btn', { active: searchState.docType === option.value }]"
              @click="handleDocTypeChange(option.value)"
            >
              {{ option.label }}
            </button>
          </div>
        </div>

        <!-- 日期筛选 -->
        <div class="filter-group">
          <span class="filter-label">时间：</span>
          <div class="filter-buttons">
            <button
              v-for="option in dateFilterOptions"
              :key="option.value"
              :class="['filter-btn', { active: searchState.dateFilter === option.value }]"
              @click="handleDateFilterChange(option.value)"
            >
              {{ option.label }}
            </button>
          </div>
        </div>

        <!-- 自定义日期 -->
        <div v-if="showCustomDatePicker" class="custom-date-picker">
          <input
            v-model="searchState.startDate"
            type="date"
            class="date-input"
          />
          <span>至</span>
          <input
            v-model="searchState.endDate"
            type="date"
            class="date-input"
          />
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
      <span v-if="hasSelection" class="selection-info">
        已选 {{ selectedCount }} 项
      </span>
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

      <div v-else-if="isLoading" class="loading">
        <div class="spinner"></div>
        <span>正在搜索...</span>
      </div>

      <div v-else-if="!hasResults && !error" class="empty-state">
        <p>输入关键词开始搜索</p>
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
              <span :class="['validity-badge', getValidityClass(doc.validity)]">
                {{ doc.validity }}
              </span>
            </div>

            <h3 class="card-title" @click="openUrl(doc.url)">
              {{ doc.title }}
            </h3>

            <div class="card-meta">
              <span v-if="doc.doc_number" class="meta-item">
                文号: {{ doc.doc_number }}
              </span>
              <span v-if="doc.publish_date" class="meta-item">
                发布: {{ doc.publish_date }}
              </span>
              <span v-if="doc.office_unit" class="meta-item">
                {{ doc.office_unit }}
              </span>
            </div>
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
  </div>
</template>

<style scoped>
.regulation-panel {
  --bg-primary: #1a1a1a;
  --bg-secondary: #242424;
  --bg-hover: #333;
  --text-primary: #e0e0e0;
  --text-secondary: #888;
  --border-color: #333;
  --primary-color: #4a9eff;
  --primary-color-dark: #3a8eef;
  --primary-color-light: rgba(74, 158, 255, 0.15);

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
  padding: 16px 20px;
  border-bottom: 1px solid var(--border-color, #e5e5e5);
}

.panel-header h2 {
  margin: 0;
  font-size: 18px;
  font-weight: 600;
}

.close-btn {
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
  transition: background-color 0.2s;
}

.close-btn:hover {
  background: var(--bg-hover, #f5f5f5);
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

.index-badge {
  color: var(--text-secondary, #666);
}

.perf-hint {
  color: #52c41a;
  font-weight: 500;
}

.process-btn {
  padding: 4px 10px;
  border: 1px solid var(--border-color, #333);
  background: transparent;
  color: var(--text-secondary, #888);
  border-radius: 4px;
  font-size: 11px;
  cursor: pointer;
  transition: all 0.2s;
}

.process-btn:hover:not(:disabled) {
  background: var(--bg-hover, #333);
  color: var(--text-primary, #e0e0e0);
}

.process-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

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

.scan-btn:hover:not(:disabled) {
  background: #52c41a;
  color: white;
}

.scan-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.ocr-progress {
  margin-top: 6px;
  padding: 4px 12px;
  font-size: 11px;
  color: #722ed1;
  background: rgba(114, 46, 209, 0.08);
  border-radius: 4px;
}

.sync-btn {
  padding: 4px 10px;
  border: 1px solid #faad14;
  background: transparent;
  color: #faad14;
  border-radius: 4px;
  font-size: 11px;
  cursor: pointer;
  transition: all 0.2s;
}

.sync-btn:hover:not(:disabled) {
  background: #faad14;
  color: white;
}

.sync-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* 同步对比结果 */
.sync-result {
  margin-top: 8px;
  border: 1px solid rgba(250, 173, 20, 0.3);
  border-radius: 6px;
  overflow: hidden;
}

.sync-summary {
  padding: 6px 12px;
  background: rgba(250, 173, 20, 0.1);
  font-size: 11px;
  color: #faad14;
  display: flex;
  gap: 4px;
  flex-wrap: wrap;
}

.sync-new {
  color: #52c41a;
  font-weight: 500;
}

.sync-new-list {
  max-height: 200px;
  overflow-y: auto;
}

.sync-list-header {
  padding: 6px 12px;
  font-size: 11px;
  font-weight: 500;
  color: var(--text-secondary, #888);
  background: var(--bg-secondary, #242424);
  border-bottom: 1px solid var(--border-color, #333);
}

.sync-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 4px 12px;
  font-size: 11px;
  border-bottom: 1px solid var(--border-color, #333);
}

.sync-item:last-child {
  border-bottom: none;
}

.sync-item-title {
  flex: 1;
  cursor: pointer;
  color: var(--text-primary, #e0e0e0);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.sync-item-title:hover {
  color: var(--primary-color, #4a9eff);
}

.sync-item-meta {
  color: var(--text-secondary, #888);
  white-space: nowrap;
}

.sync-item-validity {
  padding: 1px 6px;
  border-radius: 3px;
  font-size: 10px;
  white-space: nowrap;
}

.sync-item-validity.valid {
  background: rgba(82, 196, 26, 0.15);
  color: #52c41a;
}

.sync-item-validity.invalid {
  background: rgba(255, 77, 79, 0.15);
  color: #ff4d4f;
}

.sync-more {
  padding: 4px 12px;
  font-size: 10px;
  color: var(--text-secondary, #888);
  text-align: center;
}

/* 扫描进度 */
.scan-progress {
  margin-top: 8px;
  padding: 8px 12px;
  background: var(--bg-secondary, #242424);
  border: 1px solid var(--border-color, #333);
  border-radius: 6px;
}

.scan-progress-bar {
  height: 4px;
  background: var(--border-color, #333);
  border-radius: 2px;
  overflow: hidden;
  margin-bottom: 6px;
}

.scan-progress-fill {
  height: 100%;
  background: #52c41a;
  transition: width 0.3s ease;
  border-radius: 2px;
}

.scan-progress-info {
  font-size: 11px;
  color: var(--text-secondary, #888);
}

.scan-current-file {
  font-size: 11px;
  color: var(--text-secondary, #666);
  margin-top: 4px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* 扫描结果 */
.scan-result {
  margin-top: 8px;
  padding: 6px 12px;
  background: rgba(82, 196, 26, 0.1);
  border: 1px solid rgba(82, 196, 26, 0.3);
  border-radius: 4px;
  font-size: 11px;
  color: #52c41a;
  display: flex;
  gap: 4px;
  flex-wrap: wrap;
  align-items: center;
}

.scan-failed {
  color: #ff4d4f;
}

.retry-ocr-btn {
  padding: 2px 8px;
  margin-left: 6px;
  border: 1px solid #ff4d4f;
  background: transparent;
  color: #ff4d4f;
  border-radius: 3px;
  font-size: 11px;
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
  min-width: 40px;
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
  align-items: center;
  justify-content: center;
  padding: 40px;
  color: var(--text-secondary, #666);
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
</style>
