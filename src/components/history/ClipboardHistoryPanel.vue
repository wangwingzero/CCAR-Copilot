/**
 * 剪贴板历史面板组件
 *
 * 显示剪贴板历史记录列表，支持：
 * - 搜索过滤
 * - 点击复制
 * - 置顶/删除
 * - 切换监听状态
 */
<template>
  <div class="clipboard-history-panel">
    <!-- 头部 -->
    <div class="panel-header">
      <h3 class="panel-title">{{ t('clipboard.title') }}</h3>
      <div class="header-actions">
        <button
          class="action-btn"
          :class="{ active: isMonitoring }"
          @click="toggleMonitoring"
          :title="isMonitoring ? t('clipboard.stopMonitoring') : t('clipboard.startMonitoring')"
        >
          <span class="icon">{{ isMonitoring ? '⏸' : '▶' }}</span>
        </button>
        <button
          class="action-btn"
          @click="clearHistory"
          :title="t('clipboard.clearHistory')"
        >
          <span class="icon">🗑</span>
        </button>
      </div>
    </div>

    <!-- 搜索框 -->
    <div class="search-box">
      <input
        type="text"
        v-model="searchQuery"
        :placeholder="t('clipboard.searchPlaceholder')"
        class="search-input"
      />
    </div>

    <!-- 历史列表 -->
    <div class="history-list" v-if="filteredItems.length > 0">
      <div
        v-for="item in filteredItems"
        :key="item.id"
        class="history-item"
        :class="{ pinned: item.pinned }"
        @click="copyItem(item.id)"
      >
        <!-- 图标 -->
        <div class="item-icon">
          {{ item.type === 'text' ? '📝' : '🖼' }}
        </div>

        <!-- 内容 -->
        <div class="item-content">
          <div class="item-preview">{{ item.preview }}</div>
          <div class="item-time">{{ formatTime(item.createdAt) }}</div>
        </div>

        <!-- 操作按钮 -->
        <div class="item-actions">
          <button
            class="item-action-btn"
            :class="{ active: item.pinned }"
            @click.stop="togglePin(item.id)"
            :title="item.pinned ? t('clipboard.unpin') : t('clipboard.pin')"
          >
            📌
          </button>
          <button
            class="item-action-btn delete"
            @click.stop="removeItem(item.id)"
            :title="t('common.delete')"
          >
            ✕
          </button>
        </div>
      </div>
    </div>

    <!-- 空状态 -->
    <div class="empty-state" v-else>
      <div class="empty-icon">📋</div>
      <div class="empty-text">{{ t('clipboard.noHistory') }}</div>
      <div class="empty-hint" v-if="!isMonitoring">
        {{ t('clipboard.startMonitoringHint') }}
      </div>
    </div>

    <!-- 复制成功提示 -->
    <div class="copy-toast" v-if="showCopyToast">
      {{ t('screenshot.copied') }}
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { useClipboardHistoryStore } from '@/stores/clipboardHistory'
import { storeToRefs } from 'pinia'

const { t } = useI18n()
const store = useClipboardHistoryStore()

const { sortedItems, isMonitoring } = storeToRefs(store)

// 搜索查询
const searchQuery = ref('')

// 复制提示
const showCopyToast = ref(false)

// 过滤后的条目
const filteredItems = computed(() => {
  if (!searchQuery.value.trim()) {
    return sortedItems.value
  }
  return store.searchItems(searchQuery.value)
})

// 格式化时间
function formatTime(date: Date): string {
  const now = new Date()
  const diff = now.getTime() - date.getTime()

  // 1 分钟内
  if (diff < 60 * 1000) {
    return t('clipboard.justNow')
  }

  // 1 小时内
  if (diff < 60 * 60 * 1000) {
    const minutes = Math.floor(diff / (60 * 1000))
    return t('clipboard.minutesAgo', { count: minutes })
  }

  // 今天
  if (date.toDateString() === now.toDateString()) {
    return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
  }

  // 昨天
  const yesterday = new Date(now)
  yesterday.setDate(yesterday.getDate() - 1)
  if (date.toDateString() === yesterday.toDateString()) {
    return t('history.yesterday') + ' ' + date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
  }

  // 更早
  return date.toLocaleDateString([], { month: 'short', day: 'numeric' }) +
    ' ' + date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
}

// 切换监听状态
function toggleMonitoring(): void {
  if (isMonitoring.value) {
    store.stopMonitoring()
  } else {
    store.startMonitoring()
  }
}

// 复制条目
async function copyItem(id: string): Promise<void> {
  await store.copyItem(id)
  showCopyToast.value = true
  setTimeout(() => {
    showCopyToast.value = false
  }, 2000)
}

// 切换置顶
function togglePin(id: string): void {
  store.togglePin(id)
}

// 删除条目
function removeItem(id: string): void {
  store.removeItem(id)
}

// 清空历史
function clearHistory(): void {
  if (confirm(t('clipboard.clearConfirm'))) {
    store.clearHistory()
  }
}

// 组件挂载时初始化
onMounted(() => {
  store.initialize()
  // 默认开始监听
  store.startMonitoring()
})

// 组件卸载时停止监听
onUnmounted(() => {
  store.stopMonitoring()
})
</script>

<style scoped>
.clipboard-history-panel {
  display: flex;
  flex-direction: column;
  height: 100%;
  background: var(--bg-primary);
  border-radius: 8px;
  overflow: hidden;
}

.panel-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px 16px;
  border-bottom: 1px solid var(--border-color);
}

.panel-title {
  margin: 0;
  font-size: 16px;
  font-weight: 600;
}

.header-actions {
  display: flex;
  gap: 8px;
}

.action-btn {
  padding: 6px 10px;
  background: var(--bg-secondary);
  border: none;
  border-radius: 4px;
  cursor: pointer;
  transition: all 0.2s;
}

.action-btn:hover {
  background: var(--bg-hover);
}

.action-btn.active {
  background: var(--primary-color);
  color: white;
}

.icon {
  font-size: 14px;
}

.search-box {
  padding: 12px 16px;
  border-bottom: 1px solid var(--border-color);
}

.search-input {
  width: 100%;
  padding: 8px 12px;
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: 6px;
  font-size: 14px;
  outline: none;
  transition: border-color 0.2s;
}

.search-input:focus {
  border-color: var(--primary-color);
}

.history-list {
  flex: 1;
  overflow-y: auto;
  padding: 8px;
}

.history-item {
  display: flex;
  align-items: flex-start;
  gap: 12px;
  padding: 12px;
  background: var(--bg-secondary);
  border-radius: 6px;
  margin-bottom: 8px;
  cursor: pointer;
  transition: all 0.2s;
}

.history-item:hover {
  background: var(--bg-hover);
}

.history-item.pinned {
  border-left: 3px solid var(--primary-color);
}

.item-icon {
  font-size: 20px;
  flex-shrink: 0;
}

.item-content {
  flex: 1;
  min-width: 0;
}

.item-preview {
  font-size: 14px;
  line-height: 1.4;
  color: var(--text-primary);
  word-break: break-word;
  display: -webkit-box;
  -webkit-line-clamp: 3;
  -webkit-box-orient: vertical;
  overflow: hidden;
}

.item-time {
  font-size: 12px;
  color: var(--text-secondary);
  margin-top: 4px;
}

.item-actions {
  display: flex;
  gap: 4px;
  opacity: 0;
  transition: opacity 0.2s;
}

.history-item:hover .item-actions {
  opacity: 1;
}

.item-action-btn {
  padding: 4px 8px;
  background: transparent;
  border: none;
  border-radius: 4px;
  cursor: pointer;
  font-size: 14px;
  opacity: 0.6;
  transition: all 0.2s;
}

.item-action-btn:hover {
  opacity: 1;
  background: var(--bg-tertiary);
}

.item-action-btn.active {
  opacity: 1;
  color: var(--primary-color);
}

.item-action-btn.delete:hover {
  color: var(--error-color);
}

.empty-state {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  padding: 32px;
  text-align: center;
}

.empty-icon {
  font-size: 48px;
  margin-bottom: 16px;
}

.empty-text {
  font-size: 16px;
  color: var(--text-secondary);
  margin-bottom: 8px;
}

.empty-hint {
  font-size: 14px;
  color: var(--text-tertiary);
}

.copy-toast {
  position: fixed;
  bottom: 80px;
  left: 50%;
  transform: translateX(-50%);
  padding: 8px 16px;
  background: rgba(0, 200, 0, 0.9);
  color: white;
  border-radius: 4px;
  font-size: 14px;
  animation: fadeInOut 2s ease-in-out;
  z-index: 1000;
}

@keyframes fadeInOut {
  0% {
    opacity: 0;
    transform: translateX(-50%) translateY(10px);
  }
  20% {
    opacity: 1;
    transform: translateX(-50%) translateY(0);
  }
  80% {
    opacity: 1;
    transform: translateX(-50%) translateY(0);
  }
  100% {
    opacity: 0;
    transform: translateX(-50%) translateY(-10px);
  }
}
</style>
