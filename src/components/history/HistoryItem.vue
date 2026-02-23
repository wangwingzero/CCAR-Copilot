<template>
  <div
    class="history-item"
    :class="{
      'is-selected': isSelected,
      'is-hovered': isHovered,
    }"
    @click="handleClick"
    @dblclick="handleDoubleClick"
    @mouseenter="isHovered = true"
    @mouseleave="isHovered = false"
    @contextmenu.prevent="handleContextMenu"
  >
    <!-- 选择框 -->
    <div class="item-checkbox" @click.stop="handleCheckboxClick">
      <input
        type="checkbox"
        :checked="isSelected"
        @change="handleCheckboxChange"
      />
    </div>

    <!-- 缩略图 -->
    <div class="item-thumbnail">
      <img
        v-if="thumbnailSrc"
        :src="thumbnailSrc"
        :alt="item.ocrText || '截图'"
        loading="lazy"
        @error="handleImageError"
      />
      <div v-else class="thumbnail-placeholder">
        <span class="placeholder-icon">🖼️</span>
      </div>
    </div>

    <!-- 信息区域 -->
    <div class="item-info">
      <div class="item-header">
        <span class="item-date">{{ formattedDate }}</span>
        <span class="item-size">{{ formattedSize }}</span>
      </div>

      <div class="item-dimensions">
        {{ item.width }} × {{ item.height }}
      </div>

      <!-- OCR 文本预览 -->
      <div v-if="item.ocrText" class="item-ocr-preview" :title="item.ocrText">
        {{ truncatedOcrText }}
      </div>

      <!-- 标签 -->
      <div v-if="item.tags && item.tags.length > 0" class="item-tags">
        <span
          v-for="tag in displayTags"
          :key="tag"
          class="tag"
          @click.stop="$emit('tag-click', tag)"
        >
          {{ tag }}
        </span>
        <span v-if="item.tags.length > 3" class="tag-more">
          +{{ item.tags.length - 3 }}
        </span>
      </div>
    </div>

    <!-- 操作按钮 -->
    <div class="item-actions" @click.stop>
      <button
        class="action-btn copy-btn"
        title="复制到剪贴板"
        @click="handleCopy"
      >
        📋
      </button>
      <button
        class="action-btn open-btn"
        title="打开文件"
        @click="handleOpen"
      >
        📂
      </button>
      <button
        class="action-btn delete-btn"
        title="删除"
        @click="handleDelete"
      >
        🗑️
      </button>
    </div>
  </div>
</template>

<script setup lang="ts">
/**
 * 历史记录项组件
 *
 * 显示单个历史记录项，包括缩略图、信息和操作按钮。
 * 支持选择、双击打开、右键菜单等交互。
 *
 * @validates Requirements 14.3, 14.5
 */

import { ref, computed } from 'vue'
import { convertFileSrc } from '@tauri-apps/api/core'
import type { HistoryItem } from '@/types'

// ============================================
// Props & Emits
// ============================================

interface Props {
  /** 历史记录项 */
  item: HistoryItem
  /** 是否选中 */
  isSelected?: boolean
}

const props = withDefaults(defineProps<Props>(), {
  isSelected: false,
})

const emit = defineEmits<{
  (e: 'click', item: HistoryItem): void
  (e: 'double-click', item: HistoryItem): void
  (e: 'select', item: HistoryItem, selected: boolean): void
  (e: 'copy', item: HistoryItem): void
  (e: 'open', item: HistoryItem): void
  (e: 'delete', item: HistoryItem): void
  (e: 'context-menu', item: HistoryItem, event: MouseEvent): void
  (e: 'tag-click', tag: string): void
}>()

// ============================================
// State
// ============================================

const isHovered = ref(false)
const imageError = ref(false)

// ============================================
// Computed
// ============================================

/** 缩略图源 URL */
const thumbnailSrc = computed(() => {
  if (imageError.value) return null

  // 优先使用缩略图，否则使用原图
  const path = props.item.thumbnailPath || props.item.filePath
  if (!path) return null

  // 转换为 Tauri asset 协议 URL
  return convertFileSrc(path)
})

/** 格式化日期 */
const formattedDate = computed(() => {
  const date = new Date(props.item.createdAt)
  const now = new Date()
  const diff = now.getTime() - date.getTime()

  // 今天
  if (diff < 24 * 60 * 60 * 1000 && date.getDate() === now.getDate()) {
    return date.toLocaleTimeString('zh-CN', {
      hour: '2-digit',
      minute: '2-digit',
    })
  }

  // 昨天
  const yesterday = new Date(now)
  yesterday.setDate(yesterday.getDate() - 1)
  if (date.getDate() === yesterday.getDate()) {
    return `昨天 ${date.toLocaleTimeString('zh-CN', {
      hour: '2-digit',
      minute: '2-digit',
    })}`
  }

  // 本周
  if (diff < 7 * 24 * 60 * 60 * 1000) {
    const weekdays = ['周日', '周一', '周二', '周三', '周四', '周五', '周六']
    return `${weekdays[date.getDay()]} ${date.toLocaleTimeString('zh-CN', {
      hour: '2-digit',
      minute: '2-digit',
    })}`
  }

  // 更早
  return date.toLocaleDateString('zh-CN', {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  })
})

/** 格式化文件大小 */
const formattedSize = computed(() => {
  const size = props.item.fileSize
  if (!size) return ''

  if (size < 1024) return `${size} B`
  if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`
  return `${(size / (1024 * 1024)).toFixed(1)} MB`
})

/** 截断的 OCR 文本 */
const truncatedOcrText = computed(() => {
  const text = props.item.ocrText
  if (!text) return ''
  if (text.length <= 50) return text
  return text.slice(0, 50) + '...'
})

/** 显示的标签（最多3个） */
const displayTags = computed(() => {
  return props.item.tags?.slice(0, 3) || []
})

// ============================================
// Methods
// ============================================

function handleClick(): void {
  emit('click', props.item)
}

function handleDoubleClick(): void {
  emit('double-click', props.item)
}

function handleCheckboxClick(): void {
  emit('select', props.item, !props.isSelected)
}

function handleCheckboxChange(event: Event): void {
  const target = event.target as HTMLInputElement
  emit('select', props.item, target.checked)
}

function handleCopy(): void {
  emit('copy', props.item)
}

function handleOpen(): void {
  emit('open', props.item)
}

function handleDelete(): void {
  emit('delete', props.item)
}

function handleContextMenu(event: MouseEvent): void {
  emit('context-menu', props.item, event)
}

function handleImageError(): void {
  imageError.value = true
}
</script>

<style scoped>
.history-item {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 8px 12px;
  background: rgba(255, 255, 255, 0.03);
  border-radius: 8px;
  cursor: pointer;
  transition: background-color 0.1s ease;
  user-select: none;
}

.history-item:hover,
.history-item.is-hovered {
  background: rgba(255, 255, 255, 0.08);
}

.history-item.is-selected {
  background: rgba(66, 133, 244, 0.2);
  outline: 1px solid rgba(66, 133, 244, 0.5);
}

/* 选择框 */
.item-checkbox {
  flex-shrink: 0;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 20px;
  height: 20px;
}

.item-checkbox input[type='checkbox'] {
  width: 16px;
  height: 16px;
  cursor: pointer;
  accent-color: #4285f4;
}

/* 缩略图 */
.item-thumbnail {
  flex-shrink: 0;
  width: 80px;
  height: 60px;
  border-radius: 4px;
  overflow: hidden;
  background: rgba(0, 0, 0, 0.3);
}

.item-thumbnail img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.thumbnail-placeholder {
  width: 100%;
  height: 100%;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(255, 255, 255, 0.05);
}

.placeholder-icon {
  font-size: 24px;
  opacity: 0.5;
}

/* 信息区域 */
.item-info {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.item-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.item-date {
  color: rgba(255, 255, 255, 0.9);
  font-size: 13px;
  font-weight: 500;
}

.item-size {
  color: rgba(255, 255, 255, 0.5);
  font-size: 11px;
}

.item-dimensions {
  color: rgba(255, 255, 255, 0.5);
  font-size: 11px;
}

.item-ocr-preview {
  color: rgba(255, 255, 255, 0.6);
  font-size: 12px;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}

/* 标签 */
.item-tags {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  margin-top: 2px;
}

.tag {
  padding: 2px 6px;
  background: rgba(66, 133, 244, 0.3);
  border-radius: 3px;
  color: rgba(255, 255, 255, 0.8);
  font-size: 10px;
  cursor: pointer;
  transition: background-color 0.1s;
}

.tag:hover {
  background: rgba(66, 133, 244, 0.5);
}

.tag-more {
  padding: 2px 6px;
  color: rgba(255, 255, 255, 0.5);
  font-size: 10px;
}

/* 操作按钮 */
.item-actions {
  flex-shrink: 0;
  display: flex;
  gap: 4px;
  opacity: 0;
  transition: opacity 0.1s;
}

.history-item:hover .item-actions,
.history-item.is-selected .item-actions {
  opacity: 1;
}

.action-btn {
  width: 28px;
  height: 28px;
  padding: 0;
  border: none;
  border-radius: 4px;
  background: rgba(255, 255, 255, 0.1);
  font-size: 14px;
  cursor: pointer;
  transition: background-color 0.1s;
}

.action-btn:hover {
  background: rgba(255, 255, 255, 0.2);
}

.copy-btn:hover {
  background: rgba(33, 150, 243, 0.4);
}

.open-btn:hover {
  background: rgba(76, 175, 80, 0.4);
}

.delete-btn:hover {
  background: rgba(244, 67, 54, 0.4);
}
</style>
