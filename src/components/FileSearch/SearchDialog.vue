<script setup lang="ts">
import { ref, computed, watch, nextTick, onUnmounted, type ComponentPublicInstance } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { openPath, revealItemInDir } from '@tauri-apps/plugin-opener'
import HighlightedText from './HighlightedText.vue'
import ContextMenu, { type ContextMenuItem } from './ContextMenu.vue'
import { useFileSearchStore } from '@/stores/fileSearch'
import type { FileSearchResult, FileSearchResponse, FileSearchStatusResponse } from './index'

// ============================================
// Props & Emits
// ============================================

interface Props {
  visible: boolean
  initialQuery?: string
}

const props = withDefaults(defineProps<Props>(), {
  visible: false,
  initialQuery: '',
})

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'select', result: FileSearchResult): void
  (e: 'open', result: FileSearchResult): void
  (e: 'open-folder', result: FileSearchResult): void
}>()

// ============================================
// Refs
// ============================================

const searchInputRef = ref<HTMLInputElement | null>(null)
const resultsContainerRef = ref<HTMLDivElement | null>(null)
const resultRefs = ref<(HTMLElement | null)[]>([])

// ============================================
// State
// ============================================

const searchQuery = ref('')
const searchResults = ref<FileSearchResult[]>([])
const totalCount = ref(0)
const searchTimeMs = ref(0)
const selectedIndex = ref(0)
const isSearching = ref(false)
const serviceStatus = ref<FileSearchStatusResponse | null>(null)
const isTimeout = ref(false)

const contextMenu = ref({
  visible: false,
  x: 0,
  y: 0,
  result: null as FileSearchResult | null,
})

const contextMenuItems: ContextMenuItem[] = [
  { id: 'open', label: '打开', icon: '📂', shortcut: 'Enter' },
  { id: 'open-folder', label: '打开所在文件夹', icon: '📁', shortcut: 'Ctrl+Enter' },
  { id: 'copy-path', label: '复制路径', icon: '📋', shortcut: 'Ctrl+C' },
]

let searchDebounceTimer: ReturnType<typeof setTimeout> | null = null
let searchRequestId = 0

const MAX_SEARCH_QUERY_LENGTH = 120
const MAX_KEYWORD_TOKENS = 6
const SEARCH_DEBOUNCE_MS = 300
const SEARCH_TIMEOUT_MS = 10000

// ============================================
// Store
// ============================================

const fileSearchStore = useFileSearchStore()

// ============================================
// Computed
// ============================================

const serviceStatusClass = computed(() => {
  if (!serviceStatus.value) return ''
  switch (serviceStatus.value.status) {
    case 'ready':
      return 'status-running'
    case 'scanning':
      return 'status-scanning'
    case 'idle':
      return 'status-pending'
    case 'error':
      return 'status-stopped'
    default:
      return ''
  }
})

const serviceStatusText = computed(() => {
  if (!serviceStatus.value) return ''
  switch (serviceStatus.value.status) {
    case 'ready':
      return `已索引 ${serviceStatus.value.indexedFiles.toLocaleString()} 个文件`
    case 'scanning':
      return `扫描中 ${serviceStatus.value.scannedFiles.toLocaleString()} 个文件`
    case 'idle':
      return '索引准备中...'
    case 'error':
      return `索引错误: ${serviceStatus.value.error ?? '未知'}`
    default:
      return ''
  }
})

// ============================================
// Helpers
// ============================================

function normalizeSearchText(text: string): string {
  return text.replace(/\s+/g, ' ').trim()
}

function extractKeywordTokens(text: string): string[] {
  const matched = text.match(/[\u4e00-\u9fff]{2,}|[A-Za-z0-9._-]{2,}/g) ?? []
  const unique: string[] = []
  const seen = new Set<string>()

  for (const token of matched) {
    const normalized = token.toLowerCase()
    if (seen.has(normalized)) continue
    seen.add(normalized)
    unique.push(token)
    if (unique.length >= MAX_KEYWORD_TOKENS) break
  }

  return unique
}

function buildSafeSearchKeyword(raw: string): string {
  const normalized = normalizeSearchText(raw)
  if (!normalized) return ''
  if (normalized.length <= MAX_SEARCH_QUERY_LENGTH) return normalized

  const extracted = extractKeywordTokens(normalized)
  if (extracted.length > 0) {
    return extracted.join(' ').slice(0, MAX_SEARCH_QUERY_LENGTH).trim()
  }
  return normalized.slice(0, MAX_SEARCH_QUERY_LENGTH)
}

async function withTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
  timeoutMessage: string,
): Promise<T> {
  let timeoutId: ReturnType<typeof setTimeout> | null = null
  try {
    return await Promise.race([
      promise,
      new Promise<T>((_resolve, reject) => {
        timeoutId = setTimeout(() => reject(new Error(timeoutMessage)), timeoutMs)
      }),
    ])
  } finally {
    if (timeoutId) clearTimeout(timeoutId)
  }
}

// ============================================
// Methods - Search
// ============================================

async function performSearch(): Promise<void> {
  const keyword = buildSafeSearchKeyword(searchQuery.value)

  if (!keyword) {
    searchResults.value = []
    totalCount.value = 0
    searchTimeMs.value = 0
    selectedIndex.value = 0
    return
  }

  const currentRequestId = ++searchRequestId
  isSearching.value = true
  isTimeout.value = false

  try {
    const response = await withTimeout(
      invoke<FileSearchResponse>('file_search', {
        keyword,
        matchMode: 'fuzzy',
        limit: 100,
        offset: 0,
      }),
      SEARCH_TIMEOUT_MS,
      '搜索超时，请缩短关键词后重试',
    )

    if (currentRequestId !== searchRequestId) return

    searchResults.value = response.results
    totalCount.value = response.totalCount
    searchTimeMs.value = response.searchTimeMs
    selectedIndex.value = 0

    fileSearchStore.saveState(
      keyword,
      response.results,
      response.totalCount,
      response.searchTimeMs,
      0,
    )

    if (resultsContainerRef.value) {
      resultsContainerRef.value.scrollTop = 0
    }
  } catch (error) {
    console.error('Search failed:', error)
    if (currentRequestId === searchRequestId) {
      const isTimeoutError = error instanceof Error && error.message.includes('超时')
      isTimeout.value = isTimeoutError
      searchResults.value = []
      totalCount.value = 0
      searchTimeMs.value = 0
    }
  } finally {
    if (currentRequestId === searchRequestId) {
      isSearching.value = false
    }
  }
}

function handleSearchInput(): void {
  if (searchQuery.value.length > MAX_SEARCH_QUERY_LENGTH * 2) {
    searchQuery.value = buildSafeSearchKeyword(searchQuery.value)
  }
  if (searchDebounceTimer) clearTimeout(searchDebounceTimer)
  searchDebounceTimer = setTimeout(() => {
    performSearch()
  }, SEARCH_DEBOUNCE_MS)
}

function handleClearSearch(): void {
  searchQuery.value = ''
  searchResults.value = []
  totalCount.value = 0
  searchTimeMs.value = 0
  selectedIndex.value = 0
  searchInputRef.value?.focus()
}

async function fetchServiceStatus(): Promise<void> {
  try {
    const status = await invoke<FileSearchStatusResponse>('get_file_search_status')
    serviceStatus.value = status
  } catch (error) {
    console.error('Failed to get service status:', error)
    serviceStatus.value = null
  }
}

// ============================================
// Methods - Keyboard Navigation
// ============================================

function navigateDown(): void {
  if (searchResults.value.length === 0) return
  selectedIndex.value = (selectedIndex.value + 1) % searchResults.value.length
  scrollSelectedIntoView()
}

function navigateUp(): void {
  if (searchResults.value.length === 0) return
  selectedIndex.value =
    (selectedIndex.value - 1 + searchResults.value.length) % searchResults.value.length
  scrollSelectedIntoView()
}

function handleEnter(event: KeyboardEvent): void {
  if (searchResults.value.length === 0) return

  const selectedResult = searchResults.value[selectedIndex.value]
  if (!selectedResult) return

  if (event.ctrlKey) {
    openFolder(selectedResult)
    emit('open-folder', selectedResult)
  } else {
    openFile(selectedResult)
    emit('open', selectedResult)
  }
}

function handleKeyDown(event: KeyboardEvent): void {
  if (event.target === searchInputRef.value) return

  switch (event.key) {
    case 'ArrowDown':
      event.preventDefault()
      navigateDown()
      break
    case 'ArrowUp':
      event.preventDefault()
      navigateUp()
      break
    case 'Enter':
      event.preventDefault()
      handleEnter(event)
      break
    case 'Escape':
      event.preventDefault()
      handleClose()
      break
  }
}

function handleWindowEscape(event: KeyboardEvent): void {
  if (!props.visible || event.key !== 'Escape') return
  event.preventDefault()
  event.stopPropagation()
  handleClose()
}

function scrollSelectedIntoView(): void {
  nextTick(() => {
    const selectedEl = resultRefs.value[selectedIndex.value]
    if (selectedEl) {
      selectedEl.scrollIntoView({ behavior: 'smooth', block: 'nearest' })
    }
  })
}

function setResultRef(el: Element | ComponentPublicInstance | null, index: number): void {
  resultRefs.value[index] = el as HTMLElement | null
}

// ============================================
// Methods - Result Actions
// ============================================

function handleResultClick(result: FileSearchResult): void {
  emit('select', result)
}

function handleResultDoubleClick(result: FileSearchResult): void {
  openFile(result)
  emit('open', result)
}

function handleResultContextMenu(event: MouseEvent, result: FileSearchResult): void {
  contextMenu.value = {
    visible: true,
    x: event.clientX,
    y: event.clientY,
    result,
  }
}

function closeContextMenu(): void {
  contextMenu.value.visible = false
}

async function handleContextMenuSelect(item: ContextMenuItem): Promise<void> {
  const result = contextMenu.value.result
  if (!result) return

  switch (item.id) {
    case 'open':
      await openFile(result)
      break
    case 'open-folder':
      await openFolder(result)
      break
    case 'copy-path':
      await copyPath(result)
      break
  }
}

async function openFile(result: FileSearchResult): Promise<void> {
  try {
    await openPath(result.path)
  } catch (error) {
    console.error('Failed to open file:', error)
  }
}

async function openFolder(result: FileSearchResult): Promise<void> {
  try {
    await revealItemInDir(result.path)
  } catch (error) {
    console.error('Failed to reveal in folder:', error)
  }
}

async function copyPath(result: FileSearchResult): Promise<void> {
  try {
    await navigator.clipboard.writeText(result.path)
  } catch (error) {
    console.error('Failed to copy path:', error)
  }
}

function handleClose(): void {
  searchRequestId += 1
  isSearching.value = false
  if (searchDebounceTimer) {
    clearTimeout(searchDebounceTimer)
    searchDebounceTimer = null
  }
  emit('close')
}

// ============================================
// Formatting
// ============================================

function getFileIcon(filename: string): string {
  const ext = filename.split('.').pop()?.toLowerCase() || ''

  const iconMap: Record<string, string> = {
    pdf: '📕',
    doc: '📘',
    docx: '📘',
    xls: '📗',
    xlsx: '📗',
    ppt: '📙',
    pptx: '📙',
    txt: '📄',
    md: '📝',
    jpg: '🖼️',
    jpeg: '🖼️',
    png: '🖼️',
    gif: '🖼️',
    bmp: '🖼️',
    svg: '🖼️',
    webp: '🖼️',
    mp4: '🎬',
    avi: '🎬',
    mkv: '🎬',
    mov: '🎬',
    mp3: '🎵',
    wav: '🎵',
    flac: '🎵',
    zip: '📦',
    rar: '📦',
    '7z': '📦',
    tar: '📦',
    gz: '📦',
    js: '📜',
    ts: '📜',
    py: '🐍',
    rs: '🦀',
    vue: '💚',
    html: '🌐',
    css: '🎨',
    json: '📋',
    exe: '⚙️',
    msi: '⚙️',
    bat: '⚙️',
    sh: '⚙️',
  }

  return iconMap[ext] || '📄'
}

function formatSize(bytes: number): string {
  if (bytes === 0) return '0 B'
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`
}

function formatDate(timestampSecs: number): string {
  if (!timestampSecs) return ''
  const date = new Date(timestampSecs * 1000)
  const now = new Date()
  const diff = now.getTime() - date.getTime()

  if (diff < 24 * 60 * 60 * 1000 && date.getDate() === now.getDate()) {
    return date.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' })
  }

  const yesterday = new Date(now)
  yesterday.setDate(yesterday.getDate() - 1)
  if (
    date.getDate() === yesterday.getDate() &&
    date.getMonth() === yesterday.getMonth() &&
    date.getFullYear() === yesterday.getFullYear()
  ) {
    return '昨天'
  }

  if (diff < 7 * 24 * 60 * 60 * 1000) {
    const weekdays = ['周日', '周一', '周二', '周三', '周四', '周五', '周六']
    return weekdays[date.getDay()]
  }

  if (date.getFullYear() === now.getFullYear()) {
    return date.toLocaleDateString('zh-CN', { month: 'short', day: 'numeric' })
  }

  return date.toLocaleDateString('zh-CN', { year: 'numeric', month: 'short', day: 'numeric' })
}

function truncatePath(path: string): string {
  const maxLength = 60
  if (path.length <= maxLength) return path

  const parts = path.split('\\')
  if (parts.length <= 3) return path

  const drive = parts[0]
  const filename = parts[parts.length - 1]
  const parent = parts[parts.length - 2]

  return `${drive}\\...\\${parent}\\${filename}`
}

// ============================================
// Lifecycle
// ============================================

watch(
  () => props.visible,
  async (newVisible) => {
    if (newVisible) {
      window.addEventListener('keydown', handleWindowEscape, true)

      if (props.initialQuery) {
        searchQuery.value = buildSafeSearchKeyword(props.initialQuery)
      } else if (fileSearchStore.hasValidState) {
        const state = fileSearchStore.getState()
        searchQuery.value = buildSafeSearchKeyword(state.query)
        searchResults.value = state.results
        totalCount.value = state.totalCount
        searchTimeMs.value = state.searchTimeMs
        selectedIndex.value = state.selectedIndex
      } else {
        const savedQuery = fileSearchStore.initialize()
        searchQuery.value = buildSafeSearchKeyword(savedQuery)
      }

      await nextTick()
      searchInputRef.value?.focus()
      searchInputRef.value?.select()

      fetchServiceStatus()

      if (searchQuery.value && searchResults.value.length === 0) {
        performSearch()
      }
    } else {
      window.removeEventListener('keydown', handleWindowEscape, true)
      if (searchDebounceTimer) {
        clearTimeout(searchDebounceTimer)
        searchDebounceTimer = null
      }
    }
  },
  { immediate: true },
)

onUnmounted(() => {
  window.removeEventListener('keydown', handleWindowEscape, true)
  if (searchDebounceTimer) {
    clearTimeout(searchDebounceTimer)
    searchDebounceTimer = null
  }
})

defineExpose({
  focus: () => searchInputRef.value?.focus(),
  clear: handleClearSearch,
  search: performSearch,
})
</script>

<template>
  <Teleport to="body">
    <Transition name="dialog-fade">
      <div
        v-if="visible"
        class="search-dialog-overlay"
        @click.self="handleClose"
        @keydown="handleKeyDown"
      >
        <div
          class="search-dialog"
          role="dialog"
          aria-modal="true"
          aria-labelledby="search-dialog-title"
        >
          <!-- 搜索头部 -->
          <div class="search-header">
            <div class="search-input-wrapper">
              <span class="search-icon">🔍</span>
              <input
                ref="searchInputRef"
                v-model="searchQuery"
                type="text"
                class="search-input"
                placeholder="搜索文件..."
                aria-label="搜索文件"
                @input="handleSearchInput"
                @keydown.down.prevent="navigateDown"
                @keydown.up.prevent="navigateUp"
                @keydown.enter.prevent="handleEnter"
                @keydown.esc.prevent="handleClose"
              />
              <button
                v-if="searchQuery"
                class="clear-btn"
                title="清除搜索"
                @click="handleClearSearch"
              >
                ✕
              </button>
            </div>
            <button
              class="close-btn"
              title="关闭 (Esc)"
              @click="handleClose"
            >
              ✕
            </button>
          </div>

          <!-- 搜索状态栏 -->
          <div class="search-status-bar">
            <div class="status-left">
              <span v-if="isSearching" class="status-searching">
                <span class="loading-spinner"></span>
                搜索中...
              </span>
              <span v-else-if="searchResults.length > 0" class="status-results">
                找到 <strong>{{ totalCount }}</strong> 个结果
                <span v-if="searchTimeMs > 0" class="search-time">
                  ({{ searchTimeMs }}ms)
                </span>
              </span>
              <span v-else-if="isTimeout && !isSearching" class="status-timeout">
                搜索超时，请缩短关键词后重试
              </span>
              <span v-else-if="searchQuery && !isSearching" class="status-empty">
                未找到匹配的文件
              </span>
              <span v-else class="status-hint">
                输入关键词开始搜索
              </span>
            </div>
            <div class="status-right">
              <span v-if="serviceStatus" class="service-status" :class="serviceStatusClass">
                {{ serviceStatusText }}
              </span>
            </div>
          </div>

          <!-- 搜索结果列表 -->
          <div
            ref="resultsContainerRef"
            class="search-results"
            role="listbox"
            aria-label="搜索结果"
          >
            <!-- 加载状态 -->
            <div v-if="isSearching && searchResults.length === 0" class="loading-state">
              <div class="loading-spinner large"></div>
              <span class="loading-text">正在搜索...</span>
            </div>

            <!-- 超时状态 -->
            <div v-else-if="isTimeout && searchQuery" class="empty-state">
              <span class="empty-icon">⏱</span>
              <span class="empty-text">搜索超时</span>
              <span class="empty-hint">文件数量过多，请尝试使用更精确的关键词</span>
            </div>

            <!-- 空状态 -->
            <div v-else-if="searchResults.length === 0 && searchQuery" class="empty-state">
              <span class="empty-icon">📭</span>
              <span class="empty-text">没有找到匹配的文件</span>
              <span class="empty-hint">尝试使用不同的关键词</span>
            </div>

            <!-- 初始状态 -->
            <div v-else-if="searchResults.length === 0" class="initial-state">
              <span class="initial-icon">🔍</span>
              <span class="initial-text">输入关键词搜索文件</span>
              <div class="keyboard-hints">
                <span class="hint-item"><kbd>↑</kbd><kbd>↓</kbd> 导航</span>
                <span class="hint-item"><kbd>Enter</kbd> 打开</span>
                <span class="hint-item"><kbd>Esc</kbd> 关闭</span>
              </div>
            </div>

            <!-- 结果列表 -->
            <template v-else>
              <div
                v-for="(result, index) in searchResults"
                :key="result.path"
                :ref="el => setResultRef(el, index)"
                class="result-item"
                :class="{ 'is-selected': index === selectedIndex }"
                role="option"
                :aria-selected="index === selectedIndex"
                :title="result.path"
                @click="handleResultClick(result)"
                @dblclick="handleResultDoubleClick(result)"
                @contextmenu.prevent="handleResultContextMenu($event, result)"
                @mouseenter="selectedIndex = index"
              >
                <!-- 文件图标 -->
                <div class="result-icon">
                  <span v-if="result.isDirectory">📁</span>
                  <span v-else>{{ getFileIcon(result.name) }}</span>
                </div>

                <!-- 文件信息 -->
                <div class="result-info">
                  <div class="result-name">
                    <HighlightedText
                      :text="result.name"
                      :match-indices="result.matchIndices"
                    />
                  </div>
                  <div class="result-path" :title="result.path">
                    {{ truncatePath(result.path) }}
                  </div>
                </div>

                <!-- 文件元数据 -->
                <div class="result-meta">
                  <span class="result-size">{{ formatSize(result.size) }}</span>
                  <span class="result-date">{{ formatDate(result.modifiedSecs) }}</span>
                </div>
              </div>
            </template>
          </div>

          <!-- 底部提示 -->
          <div class="search-footer">
            <div class="footer-hints">
              <span class="hint-item"><kbd>↑</kbd><kbd>↓</kbd> 选择</span>
              <span class="hint-item"><kbd>Enter</kbd> 打开文件</span>
              <span class="hint-item"><kbd>Ctrl+Enter</kbd> 打开文件夹</span>
              <span class="hint-item"><kbd>Esc</kbd> 关闭</span>
            </div>
          </div>
        </div>
      </div>
    </Transition>

    <!-- 右键菜单 -->
    <ContextMenu
      :visible="contextMenu.visible"
      :items="contextMenuItems"
      :x="contextMenu.x"
      :y="contextMenu.y"
      @close="closeContextMenu"
      @select="handleContextMenuSelect"
    />
  </Teleport>
</template>

<style scoped>
/* ============================================
 * Overlay & Dialog
 * ============================================ */
.search-dialog-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: flex-start;
  justify-content: center;
  padding-top: 10vh;
  z-index: 9999;
  backdrop-filter: blur(4px);
}

.search-dialog {
  width: 680px;
  max-width: 90vw;
  max-height: 70vh;
  background: var(--color-bg-dialog, #1e1e2e);
  border-radius: 12px;
  box-shadow: 0 16px 48px rgba(0, 0, 0, 0.4);
  display: flex;
  flex-direction: column;
  overflow: hidden;
  border: 1px solid var(--color-border, #383850);
}

/* ============================================
 * Search Header
 * ============================================ */
.search-header {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px 16px;
  border-bottom: 1px solid var(--color-border, #383850);
  flex-shrink: 0;
}

.search-input-wrapper {
  flex: 1;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 12px;
  background: var(--color-surface-subtle, rgba(255, 255, 255, 0.05));
  border-radius: 8px;
  border: 1px solid transparent;
  transition: border-color 0.15s ease;
}

.search-input-wrapper:focus-within {
  border-color: var(--color-accent, #3b82f6);
}

.search-icon {
  font-size: 16px;
  opacity: 0.6;
  flex-shrink: 0;
}

.search-input {
  flex: 1;
  min-width: 0;
  background: transparent;
  border: none;
  outline: none;
  color: var(--color-text-primary, #e0e0e0);
  font-size: 15px;
  font-family: inherit;
}

.search-input::placeholder {
  color: var(--color-text-tertiary, #666);
}

.clear-btn,
.close-btn {
  width: 28px;
  height: 28px;
  padding: 0;
  border: none;
  border-radius: 6px;
  background: transparent;
  color: var(--color-text-tertiary, #666);
  font-size: 14px;
  cursor: pointer;
  display: flex;
  align-items: center;
  justify-content: center;
  transition: background-color 0.1s, color 0.1s;
  flex-shrink: 0;
}

.clear-btn:hover,
.close-btn:hover {
  background: var(--color-surface-muted, rgba(255, 255, 255, 0.08));
  color: var(--color-text-primary, #e0e0e0);
}

/* ============================================
 * Status Bar
 * ============================================ */
.search-status-bar {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 16px;
  background: var(--color-surface-subtle, rgba(255, 255, 255, 0.03));
  border-bottom: 1px solid var(--color-border, #383850);
  font-size: 12px;
  flex-shrink: 0;
}

.status-left {
  display: flex;
  align-items: center;
  gap: 8px;
}

.status-searching {
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--color-accent, #3b82f6);
}

.status-results {
  color: var(--color-text-secondary, #aaa);
}

.status-results strong {
  color: var(--color-text-primary, #e0e0e0);
}

.search-time {
  color: var(--color-text-tertiary, #666);
}

.status-empty {
  color: var(--color-text-tertiary, #666);
}

.status-timeout {
  color: var(--color-warning, #f59e0b);
}

.status-hint {
  color: var(--color-text-tertiary, #666);
}

.status-right {
  display: flex;
  align-items: center;
}

.service-status {
  padding: 2px 8px;
  border-radius: 4px;
  font-size: 11px;
}

.service-status.status-running {
  background: rgba(34, 197, 94, 0.15);
  color: var(--color-success, #22c55e);
}

.service-status.status-scanning {
  background: rgba(245, 158, 11, 0.15);
  color: var(--color-warning, #f59e0b);
}

.service-status.status-pending {
  background: rgba(59, 130, 246, 0.15);
  color: var(--color-accent, #3b82f6);
}

.service-status.status-stopped {
  background: rgba(239, 68, 68, 0.15);
  color: var(--color-error, #ef4444);
}

/* ============================================
 * Search Results
 * ============================================ */
.search-results {
  flex: 1;
  overflow-y: auto;
  overflow-x: hidden;
  min-height: 200px;
  max-height: 400px;
}

.search-results::-webkit-scrollbar {
  width: 6px;
}

.search-results::-webkit-scrollbar-track {
  background: transparent;
}

.search-results::-webkit-scrollbar-thumb {
  background: var(--color-border, #383850);
  border-radius: 3px;
}

.search-results::-webkit-scrollbar-thumb:hover {
  background: var(--color-text-tertiary, #666);
}

/* Loading State */
.loading-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  padding: 40px;
  height: 100%;
}

.loading-spinner {
  width: 16px;
  height: 16px;
  border: 2px solid var(--color-border, #383850);
  border-top-color: var(--color-accent, #3b82f6);
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

.loading-spinner.large {
  width: 32px;
  height: 32px;
  border-width: 3px;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

.loading-text {
  color: var(--color-text-tertiary, #666);
  font-size: 13px;
}

/* Empty & Initial State */
.empty-state,
.initial-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 40px;
  height: 100%;
}

.empty-icon,
.initial-icon {
  font-size: 32px;
  opacity: 0.5;
}

.empty-text,
.initial-text {
  color: var(--color-text-secondary, #aaa);
  font-size: 14px;
}

.empty-hint {
  color: var(--color-text-tertiary, #666);
  font-size: 12px;
}

.keyboard-hints {
  display: flex;
  gap: 16px;
  margin-top: 8px;
}

.hint-item {
  display: flex;
  align-items: center;
  gap: 4px;
  color: var(--color-text-tertiary, #666);
  font-size: 12px;
}

kbd {
  display: inline-block;
  padding: 2px 6px;
  background: var(--color-surface-subtle, rgba(255, 255, 255, 0.05));
  border: 1px solid var(--color-border, #383850);
  border-radius: 4px;
  font-size: 11px;
  font-family: inherit;
  color: var(--color-text-secondary, #aaa);
  line-height: 1;
}

/* Result Items */
.result-item {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 16px;
  cursor: pointer;
  transition: background-color 0.08s ease;
  border-bottom: 1px solid transparent;
}

.result-item:hover {
  background: var(--color-surface-muted, rgba(255, 255, 255, 0.05));
}

.result-item.is-selected {
  background: var(--color-accent-light, rgba(59, 130, 246, 0.12));
}

.result-icon {
  font-size: 20px;
  width: 28px;
  text-align: center;
  flex-shrink: 0;
}

.result-info {
  flex: 1;
  min-width: 0;
  overflow: hidden;
}

.result-name {
  font-size: 14px;
  color: var(--color-text-primary, #e0e0e0);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

.result-path {
  font-size: 11px;
  color: var(--color-text-tertiary, #666);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
  margin-top: 2px;
}

.result-meta {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 2px;
  flex-shrink: 0;
}

.result-size {
  font-size: 11px;
  color: var(--color-text-tertiary, #666);
  white-space: nowrap;
}

.result-date {
  font-size: 11px;
  color: var(--color-text-tertiary, #666);
  white-space: nowrap;
}

/* ============================================
 * Footer
 * ============================================ */
.search-footer {
  padding: 8px 16px;
  border-top: 1px solid var(--color-border, #383850);
  flex-shrink: 0;
}

.footer-hints {
  display: flex;
  gap: 16px;
  justify-content: center;
}

/* ============================================
 * Transitions
 * ============================================ */
.dialog-fade-enter-active {
  transition: opacity 0.15s ease;
}

.dialog-fade-leave-active {
  transition: opacity 0.1s ease;
}

.dialog-fade-enter-from,
.dialog-fade-leave-to {
  opacity: 0;
}

.dialog-fade-enter-active .search-dialog {
  animation: dialog-in 0.2s ease-out;
}

@keyframes dialog-in {
  from {
    opacity: 0;
    transform: translateY(-20px) scale(0.96);
  }
  to {
    opacity: 1;
    transform: translateY(0) scale(1);
  }
}
</style>
