<template>
  <div class="history-panel">
    <!-- 头部工具栏 -->
    <div class="panel-header">
      <div class="header-title">
        <span class="title-icon">📷</span>
        <span class="title-text">截图历史</span>
        <span class="title-count">({{ totalCount }})</span>
      </div>

      <div class="header-actions">
        <button
          v-if="hasSelection"
          class="action-btn export-btn"
          title="导出选中项"
          @click="handleExportSelected"
        >
          <span class="btn-icon">📤</span>
          <span class="btn-text">导出 ({{ selectedCount }})</span>
        </button>
        <button
          v-if="hasSelection"
          class="action-btn delete-btn"
          title="删除选中项"
          @click="handleDeleteSelected"
        >
          <span class="btn-icon">🗑️</span>
          <span class="btn-text">删除 ({{ selectedCount }})</span>
        </button>
        <button
          class="action-btn refresh-btn"
          title="刷新"
          :disabled="isLoading"
          @click="handleRefresh"
        >
          <span class="btn-icon" :class="{ 'is-spinning': isLoading }">🔄</span>
        </button>
      </div>
    </div>

    <!-- 搜索和过滤栏 -->
    <div class="filter-bar">
      <!-- 搜索框 -->
      <div class="search-box">
        <span class="search-icon">🔍</span>
        <input
          v-model="searchQuery"
          type="text"
          class="search-input"
          placeholder="搜索 OCR 文本、标签..."
          @input="handleSearchInput"
          @keydown.enter="handleSearch"
        />
        <button
          v-if="searchQuery"
          class="clear-btn"
          @click="handleClearSearch"
        >
          ✕
        </button>
      </div>

      <!-- 日期过滤 -->
      <div class="date-filter">
        <select v-model="dateFilter" class="filter-select" @change="handleDateFilterChange">
          <option value="">全部时间</option>
          <option value="today">今天</option>
          <option value="week">本周</option>
          <option value="month">本月</option>
          <option value="custom">自定义...</option>
        </select>
      </div>

      <!-- 排序 -->
      <div class="sort-control">
        <select v-model="sortBy" class="filter-select" @change="handleSortChange">
          <option value="createdAt">按时间</option>
          <option value="fileSize">按大小</option>
        </select>
        <button
          class="sort-order-btn"
          :title="sortOrder === 'desc' ? '降序' : '升序'"
          @click="toggleSortOrder"
        >
          {{ sortOrder === 'desc' ? '↓' : '↑' }}
        </button>
      </div>

      <!-- 全选 -->
      <div class="select-all">
        <input
          type="checkbox"
          :checked="isAllSelected"
          :indeterminate="hasSelection && !isAllSelected"
          @change="handleSelectAll"
        />
        <span class="select-label">全选</span>
      </div>
    </div>

    <!-- 统计信息 -->
    <div v-if="stats" class="stats-bar">
      <span class="stat-item">
        <span class="stat-label">总计:</span>
        <span class="stat-value">{{ stats.totalCount }} 张</span>
      </span>
      <span class="stat-item">
        <span class="stat-label">占用:</span>
        <span class="stat-value">{{ formatSize(stats.totalSize) }}</span>
      </span>
      <span class="stat-item">
        <span class="stat-label">今日:</span>
        <span class="stat-value">{{ stats.todayCount }} 张</span>
      </span>
      <span class="stat-item">
        <span class="stat-label">本周:</span>
        <span class="stat-value">{{ stats.weekCount }} 张</span>
      </span>
    </div>

    <!-- 历史记录列表 -->
    <div
      ref="listContainerRef"
      class="history-list"
      @scroll="handleScroll"
    >
      <!-- 加载状态 -->
      <div v-if="isLoading && items.length === 0" class="loading-state">
        <div class="loading-spinner" />
        <span class="loading-text">加载中...</span>
      </div>

      <!-- 空状态 -->
      <div v-else-if="items.length === 0" class="empty-state">
        <span class="empty-icon">📭</span>
        <span class="empty-text">
          {{ searchQuery ? '没有找到匹配的记录' : '暂无截图历史' }}
        </span>
        <button v-if="searchQuery" class="clear-search-btn" @click="handleClearSearch">
          清除搜索
        </button>
      </div>

      <!-- 列表内容 -->
      <template v-else>
        <HistoryItem
          v-for="item in items"
          :key="item.id"
          :item="item"
          :is-selected="selectedIds.has(item.id)"
          @click="handleItemClick"
          @double-click="handleItemDoubleClick"
          @select="handleItemSelect"
          @copy="handleItemCopy"
          @open="handleItemOpen"
          @delete="handleItemDelete"
          @tag-click="handleTagClick"
        />

        <!-- 加载更多 -->
        <div v-if="hasMore" class="load-more">
          <button
            v-if="!isLoading"
            class="load-more-btn"
            @click="handleLoadMore"
          >
            加载更多
          </button>
          <div v-else class="loading-more">
            <div class="loading-spinner small" />
            <span>加载中...</span>
          </div>
        </div>
      </template>
    </div>

    <!-- 删除确认对话框 -->
    <Teleport to="body">
      <div v-if="showDeleteConfirm" class="modal-overlay" @click="cancelDelete">
        <div class="modal-dialog" @click.stop>
          <div class="modal-header">
            <span class="modal-title">确认删除</span>
          </div>
          <div class="modal-body">
            <p>确定要删除选中的 {{ deleteCount }} 个截图吗？</p>
            <p class="warning-text">此操作将同时删除文件，无法恢复！</p>
          </div>
          <div class="modal-footer">
            <button class="modal-btn cancel-btn" @click="cancelDelete">取消</button>
            <button class="modal-btn confirm-btn" @click="confirmDelete">删除</button>
          </div>
        </div>
      </div>
    </Teleport>

    <!-- Toast 提示 -->
    <Transition name="toast">
      <div v-if="showToast" class="toast" :class="toastType">
        {{ toastMessage }}
      </div>
    </Transition>
  </div>
</template>

<script setup lang="ts">
/**
 * 历史记录面板组件
 *
 * 功能：
 * - 显示截图历史记录列表
 * - 搜索和过滤（按日期、标签、OCR 文本）
 * - 批量选择和操作
 * - 删除和导出
 *
 * @validates Requirements 14.3, 14.4, 14.5, 14.6
 */

import { ref, computed, onMounted, onUnmounted, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { open as openDialog } from '@tauri-apps/plugin-dialog'
import { writeText } from '@tauri-apps/plugin-clipboard-manager'
import { open as shellOpen } from '@tauri-apps/plugin-shell'
import HistoryItem from './HistoryItem.vue'
import { useHistoryStore } from '@/stores/history'
import type { HistoryItem as HistoryItemType } from '@/types'

// ============================================
// Store
// ============================================

const historyStore = useHistoryStore()

// ============================================
// Refs
// ============================================

const listContainerRef = ref<HTMLDivElement | null>(null)

// ============================================
// State
// ============================================

/** 搜索关键词 */
const searchQuery = ref('')

/** 日期过滤 */
const dateFilter = ref('')

/** 排序字段 */
const sortBy = ref<'createdAt' | 'fileSize'>('createdAt')

/** 排序方向 */
const sortOrder = ref<'asc' | 'desc'>('desc')

/** 删除确认对话框 */
const showDeleteConfirm = ref(false)

/** 待删除数量 */
const deleteCount = ref(0)

/** 待删除的 ID 列表 */
const pendingDeleteIds = ref<number[]>([])

/** Toast 显示 */
const showToast = ref(false)

/** Toast 消息 */
const toastMessage = ref('')

/** Toast 类型 */
const toastType = ref<'success' | 'error' | 'info'>('success')

/** 搜索防抖定时器 */
let searchDebounceTimer: ReturnType<typeof setTimeout> | null = null

// ============================================
// Computed
// ============================================

const items = computed(() => historyStore.items)
const totalCount = computed(() => historyStore.totalCount)
const hasMore = computed(() => historyStore.hasMore)
const isLoading = computed(() => historyStore.isLoading)
const stats = computed(() => historyStore.stats)
const selectedIds = computed(() => historyStore.selectedIds)
const selectedCount = computed(() => historyStore.selectedCount)
const hasSelection = computed(() => historyStore.hasSelection)
const isAllSelected = computed(() => historyStore.isAllSelected)

// ============================================
// Methods
// ============================================

/**
 * 格式化文件大小
 */
function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`
}

/**
 * 显示 Toast 提示
 */
function showToastMessage(message: string, type: 'success' | 'error' | 'info' = 'success'): void {
  toastMessage.value = message
  toastType.value = type
  showToast.value = true
  setTimeout(() => {
    showToast.value = false
  }, 2000)
}

/**
 * 获取日期范围
 */
function getDateRange(filter: string): { startDate?: string; endDate?: string } {
  const now = new Date()
  const today = new Date(now.getFullYear(), now.getMonth(), now.getDate())

  switch (filter) {
    case 'today':
      return {
        startDate: today.toISOString(),
        endDate: new Date(today.getTime() + 24 * 60 * 60 * 1000).toISOString(),
      }
    case 'week': {
      const weekStart = new Date(today)
      weekStart.setDate(weekStart.getDate() - weekStart.getDay())
      return {
        startDate: weekStart.toISOString(),
      }
    }
    case 'month': {
      const monthStart = new Date(now.getFullYear(), now.getMonth(), 1)
      return {
        startDate: monthStart.toISOString(),
      }
    }
    default:
      return {}
  }
}

/**
 * 加载历史记录
 */
async function loadHistory(): Promise<void> {
  const dateRange = getDateRange(dateFilter.value)
  await historyStore.loadHistory({
    query: searchQuery.value || undefined,
    startDate: dateRange.startDate,
    endDate: dateRange.endDate,
    sortBy: sortBy.value,
    sortOrder: sortOrder.value,
  })
}

/**
 * 处理搜索输入（防抖）
 */
function handleSearchInput(): void {
  if (searchDebounceTimer) {
    clearTimeout(searchDebounceTimer)
  }
  searchDebounceTimer = setTimeout(() => {
    loadHistory()
  }, 300)
}

/**
 * 处理搜索回车
 */
function handleSearch(): void {
  if (searchDebounceTimer) {
    clearTimeout(searchDebounceTimer)
  }
  loadHistory()
}

/**
 * 清除搜索
 */
function handleClearSearch(): void {
  searchQuery.value = ''
  loadHistory()
}

/**
 * 处理日期过滤变化
 */
function handleDateFilterChange(): void {
  if (dateFilter.value === 'custom') {
    // TODO: 显示日期选择器
    dateFilter.value = ''
    return
  }
  loadHistory()
}

/**
 * 处理排序变化
 */
function handleSortChange(): void {
  loadHistory()
}

/**
 * 切换排序方向
 */
function toggleSortOrder(): void {
  sortOrder.value = sortOrder.value === 'desc' ? 'asc' : 'desc'
  loadHistory()
}

/**
 * 处理全选
 */
function handleSelectAll(): void {
  historyStore.toggleSelectAll()
}

/**
 * 处理刷新
 */
async function handleRefresh(): Promise<void> {
  await historyStore.loadStats()
  await loadHistory()
  showToastMessage('刷新成功', 'success')
}

/**
 * 处理滚动加载更多
 */
function handleScroll(): void {
  if (!listContainerRef.value || isLoading.value || !hasMore.value) return

  const { scrollTop, scrollHeight, clientHeight } = listContainerRef.value
  // 距离底部 100px 时加载更多
  if (scrollHeight - scrollTop - clientHeight < 100) {
    historyStore.loadMore()
  }
}

/**
 * 处理加载更多按钮
 */
function handleLoadMore(): void {
  historyStore.loadMore()
}

/**
 * 处理项目点击
 */
function handleItemClick(item: HistoryItemType): void {
  // 单击选中/取消选中
  historyStore.toggleSelection(item.id)
}

/**
 * 处理项目双击
 */
async function handleItemDoubleClick(item: HistoryItemType): Promise<void> {
  // 双击打开文件
  try {
    await shellOpen(item.filePath)
  } catch (error) {
    showToastMessage('打开文件失败', 'error')
  }
}

/**
 * 处理项目选择
 */
function handleItemSelect(item: HistoryItemType, selected: boolean): void {
  if (selected) {
    historyStore.selectedIds.add(item.id)
  } else {
    historyStore.selectedIds.delete(item.id)
  }
}

/**
 * 处理复制
 */
async function handleItemCopy(item: HistoryItemType): Promise<void> {
  try {
    // 复制 OCR 文本或文件路径
    const text = item.ocrText || item.filePath
    await writeText(text)
    showToastMessage('已复制到剪贴板', 'success')
  } catch (error) {
    showToastMessage('复制失败', 'error')
  }
}

/**
 * 处理打开文件
 */
async function handleItemOpen(item: HistoryItemType): Promise<void> {
  try {
    await shellOpen(item.filePath)
  } catch (error) {
    showToastMessage('打开文件失败', 'error')
  }
}

/**
 * 处理删除单个项目
 */
function handleItemDelete(item: HistoryItemType): void {
  pendingDeleteIds.value = [item.id]
  deleteCount.value = 1
  showDeleteConfirm.value = true
}

/**
 * 处理标签点击
 */
function handleTagClick(tag: string): void {
  searchQuery.value = tag
  loadHistory()
}

/**
 * 处理导出选中项
 */
async function handleExportSelected(): Promise<void> {
  try {
    // 选择导出目录
    const outputDir = await openDialog({
      directory: true,
      title: '选择导出目录',
    })

    if (!outputDir) return

    const paths = await historyStore.exportSelected(outputDir as string)
    showToastMessage(`成功导出 ${paths.length} 个文件`, 'success')
  } catch (error) {
    showToastMessage('导出失败: ' + (error as Error).message, 'error')
  }
}

/**
 * 处理删除选中项
 */
function handleDeleteSelected(): void {
  pendingDeleteIds.value = Array.from(historyStore.selectedIds)
  deleteCount.value = pendingDeleteIds.value.length
  showDeleteConfirm.value = true
}

/**
 * 取消删除
 */
function cancelDelete(): void {
  showDeleteConfirm.value = false
  pendingDeleteIds.value = []
  deleteCount.value = 0
}

/**
 * 确认删除
 */
async function confirmDelete(): Promise<void> {
  try {
    if (pendingDeleteIds.value.length === 1) {
      await historyStore.deleteItem(pendingDeleteIds.value[0])
    } else {
      await historyStore.deleteItems(pendingDeleteIds.value)
    }
    showToastMessage(`成功删除 ${deleteCount.value} 个截图`, 'success')
    await historyStore.loadStats()
  } catch (error) {
    showToastMessage('删除失败: ' + (error as Error).message, 'error')
  } finally {
    cancelDelete()
  }
}

// ============================================
// Lifecycle
// ============================================

onMounted(async () => {
  // 初始化数据库
  try {
    const appDataDir = await import('@tauri-apps/api/path').then(m => m.appDataDir())
    const dbPath = `${appDataDir}history.db`
    await invoke('init_history_database', { dbPath })

    // 加载统计信息和历史记录
    await historyStore.loadStats()
    await loadHistory()
  } catch (error) {
    console.error('初始化历史记录失败:', error)
    showToastMessage('加载历史记录失败', 'error')
  }
})

onUnmounted(() => {
  if (searchDebounceTimer) {
    clearTimeout(searchDebounceTimer)
  }
})

// 监听搜索参数变化
watch([sortBy, sortOrder], () => {
  loadHistory()
})
</script>

<style scoped>
.history-panel {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: rgba(30, 30, 30, 0.98);
  color: #fff;
  font-family: 'Microsoft YaHei', sans-serif;
}

/* 头部工具栏 */
.panel-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 12px 16px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.1);
}

.header-title {
  display: flex;
  align-items: center;
  gap: 8px;
}

.title-icon {
  font-size: 18px;
}

.title-text {
  font-size: 16px;
  font-weight: 600;
}

.title-count {
  color: rgba(255, 255, 255, 0.5);
  font-size: 13px;
}

.header-actions {
  display: flex;
  gap: 8px;
}

.action-btn {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 6px 12px;
  border: none;
  border-radius: 6px;
  background: rgba(255, 255, 255, 0.1);
  color: #fff;
  font-size: 12px;
  cursor: pointer;
  transition: background-color 0.1s;
}

.action-btn:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.2);
}

.action-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.export-btn:hover:not(:disabled) {
  background: rgba(76, 175, 80, 0.4);
}

.delete-btn:hover:not(:disabled) {
  background: rgba(244, 67, 54, 0.4);
}

.refresh-btn {
  padding: 6px 8px;
}

.btn-icon.is-spinning {
  animation: spin 1s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

/* 过滤栏 */
.filter-bar {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 16px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.1);
  flex-wrap: wrap;
}

.search-box {
  flex: 1;
  min-width: 200px;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 12px;
  background: rgba(255, 255, 255, 0.08);
  border-radius: 6px;
  border: 1px solid transparent;
  transition: border-color 0.1s;
}

.search-box:focus-within {
  border-color: rgba(66, 133, 244, 0.5);
}

.search-icon {
  font-size: 14px;
  opacity: 0.6;
}

.search-input {
  flex: 1;
  background: transparent;
  border: none;
  outline: none;
  color: #fff;
  font-size: 13px;
}

.search-input::placeholder {
  color: rgba(255, 255, 255, 0.4);
}

.clear-btn {
  padding: 2px 6px;
  background: transparent;
  border: none;
  color: rgba(255, 255, 255, 0.5);
  font-size: 12px;
  cursor: pointer;
  border-radius: 3px;
}

.clear-btn:hover {
  background: rgba(255, 255, 255, 0.1);
  color: #fff;
}

.filter-select {
  padding: 6px 10px;
  background: rgba(255, 255, 255, 0.08);
  border: 1px solid transparent;
  border-radius: 6px;
  color: #fff;
  font-size: 12px;
  cursor: pointer;
  outline: none;
}

.filter-select:focus {
  border-color: rgba(66, 133, 244, 0.5);
}

.filter-select option {
  background: #2a2a2a;
  color: #fff;
}

.sort-control {
  display: flex;
  align-items: center;
  gap: 4px;
}

.sort-order-btn {
  width: 28px;
  height: 28px;
  padding: 0;
  background: rgba(255, 255, 255, 0.08);
  border: none;
  border-radius: 4px;
  color: #fff;
  font-size: 14px;
  cursor: pointer;
  transition: background-color 0.1s;
}

.sort-order-btn:hover {
  background: rgba(255, 255, 255, 0.15);
}

.select-all {
  display: flex;
  align-items: center;
  gap: 6px;
}

.select-all input[type='checkbox'] {
  width: 16px;
  height: 16px;
  cursor: pointer;
  accent-color: #4285f4;
}

.select-label {
  color: rgba(255, 255, 255, 0.7);
  font-size: 12px;
}

/* 统计栏 */
.stats-bar {
  display: flex;
  gap: 16px;
  padding: 8px 16px;
  background: rgba(255, 255, 255, 0.03);
  border-bottom: 1px solid rgba(255, 255, 255, 0.05);
}

.stat-item {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 12px;
}

.stat-label {
  color: rgba(255, 255, 255, 0.5);
}

.stat-value {
  color: rgba(255, 255, 255, 0.9);
}

/* 历史列表 */
.history-list {
  flex: 1;
  overflow-y: auto;
  padding: 8px;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.history-list::-webkit-scrollbar {
  width: 8px;
}

.history-list::-webkit-scrollbar-track {
  background: rgba(255, 255, 255, 0.05);
  border-radius: 4px;
}

.history-list::-webkit-scrollbar-thumb {
  background: rgba(255, 255, 255, 0.2);
  border-radius: 4px;
}

.history-list::-webkit-scrollbar-thumb:hover {
  background: rgba(255, 255, 255, 0.3);
}

/* 加载状态 */
.loading-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  padding: 40px;
}

.loading-spinner {
  width: 32px;
  height: 32px;
  border: 3px solid rgba(255, 255, 255, 0.2);
  border-top-color: #4285f4;
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

.loading-spinner.small {
  width: 16px;
  height: 16px;
  border-width: 2px;
}

.loading-text {
  color: rgba(255, 255, 255, 0.6);
  font-size: 14px;
}

/* 空状态 */
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  padding: 60px 20px;
}

.empty-icon {
  font-size: 48px;
  opacity: 0.5;
}

.empty-text {
  color: rgba(255, 255, 255, 0.5);
  font-size: 14px;
}

.clear-search-btn {
  margin-top: 8px;
  padding: 8px 16px;
  background: rgba(66, 133, 244, 0.8);
  border: none;
  border-radius: 6px;
  color: #fff;
  font-size: 13px;
  cursor: pointer;
  transition: background-color 0.1s;
}

.clear-search-btn:hover {
  background: rgba(66, 133, 244, 1);
}

/* 加载更多 */
.load-more {
  display: flex;
  justify-content: center;
  padding: 16px;
}

.load-more-btn {
  padding: 8px 24px;
  background: rgba(255, 255, 255, 0.1);
  border: none;
  border-radius: 6px;
  color: #fff;
  font-size: 13px;
  cursor: pointer;
  transition: background-color 0.1s;
}

.load-more-btn:hover {
  background: rgba(255, 255, 255, 0.2);
}

.loading-more {
  display: flex;
  align-items: center;
  gap: 8px;
  color: rgba(255, 255, 255, 0.6);
  font-size: 13px;
}

/* 模态对话框 */
.modal-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: rgba(0, 0, 0, 0.6);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 10000;
}

.modal-dialog {
  width: 360px;
  background: #2a2a2a;
  border-radius: 12px;
  box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4);
  overflow: hidden;
}

.modal-header {
  padding: 16px 20px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.1);
}

.modal-title {
  font-size: 16px;
  font-weight: 600;
  color: #fff;
}

.modal-body {
  padding: 20px;
}

.modal-body p {
  margin: 0 0 8px;
  color: rgba(255, 255, 255, 0.9);
  font-size: 14px;
}

.warning-text {
  color: #ff6b6b !important;
  font-size: 13px !important;
}

.modal-footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  padding: 12px 20px;
  border-top: 1px solid rgba(255, 255, 255, 0.1);
}

.modal-btn {
  padding: 8px 20px;
  border: none;
  border-radius: 6px;
  font-size: 13px;
  cursor: pointer;
  transition: background-color 0.1s;
}

.cancel-btn {
  background: rgba(255, 255, 255, 0.1);
  color: #fff;
}

.cancel-btn:hover {
  background: rgba(255, 255, 255, 0.2);
}

.confirm-btn {
  background: rgba(244, 67, 54, 0.8);
  color: #fff;
}

.confirm-btn:hover {
  background: rgba(244, 67, 54, 1);
}

/* Toast 提示 */
.toast {
  position: fixed;
  bottom: 24px;
  left: 50%;
  transform: translateX(-50%);
  padding: 10px 20px;
  border-radius: 6px;
  font-size: 13px;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
  z-index: 10001;
}

.toast.success {
  background: rgba(76, 175, 80, 0.95);
  color: #fff;
}

.toast.error {
  background: rgba(244, 67, 54, 0.95);
  color: #fff;
}

.toast.info {
  background: rgba(33, 150, 243, 0.95);
  color: #fff;
}

/* Toast 动画 */
.toast-enter-active,
.toast-leave-active {
  transition: all 0.2s ease;
}

.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translateX(-50%) translateY(10px);
}
</style>
