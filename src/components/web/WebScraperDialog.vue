<template>
  <div class="web-scraper-dialog" :class="{ 'is-loading': isLoading }">
    <!-- 对话框头部 -->
    <div class="dialog-header">
      <span class="dialog-title">🌐 网页爬取</span>
      <button class="close-btn" @click="handleClose" :disabled="isLoading">
        ✕
      </button>
    </div>

    <!-- URL 输入区域 -->
    <div class="url-input-section">
      <div class="form-group">
        <label class="form-label">网页地址</label>
        <div class="url-input-wrapper">
          <input
            ref="urlInputRef"
            v-model="urlInput"
            type="url"
            class="form-input url-input"
            placeholder="https://example.com/article"
            :disabled="isLoading"
            @keydown.enter="handleScrape"
            @input="handleUrlChange"
          />
          <button
            class="scrape-btn"
            :class="{ 'is-loading': isLoading }"
            :disabled="!canScrape"
            @click="handleScrape"
          >
            <span v-if="isLoading" class="loading-spinner-small" />
            <span v-else class="btn-icon">🔍</span>
            <span class="btn-text">{{ isLoading ? '爬取中...' : '爬取' }}</span>
          </button>
        </div>
        <span v-if="urlError" class="url-error">{{ urlError }}</span>
      </div>

      <!-- 选项 -->
      <div class="options-row">
        <label class="checkbox-label">
          <input
            type="checkbox"
            v-model="downloadImages"
            :disabled="isLoading"
          />
          <span class="checkbox-text">下载图片</span>
        </label>
      </div>
    </div>

    <!-- 进度指示器 -->
    <div v-if="isLoading" class="progress-section">
      <div class="progress-bar">
        <div class="progress-bar-inner" :style="{ width: progressWidth }" />
      </div>
      <span class="progress-text">{{ progressText }}</span>
    </div>

    <!-- 结果预览区域 -->
    <template v-if="hasResult && !isLoading">
      <!-- 标题 -->
      <div class="result-header">
        <span class="result-title">📄 {{ resultTitle }}</span>
        <div class="result-actions">
          <button class="action-btn copy-btn" @click="handleCopyMarkdown" title="复制 Markdown">
            📋 复制
          </button>
        </div>
      </div>

      <!-- Markdown 预览 -->
      <div class="markdown-preview" ref="previewRef">
        <div class="markdown-content" v-html="renderedMarkdown" />
      </div>

      <!-- 图片列表 -->
      <div v-if="images.length > 0" class="images-section">
        <div class="images-header">
          <span class="images-title">🖼️ 提取的图片 ({{ images.length }})</span>
        </div>
        <div class="images-grid">
          <div
            v-for="(img, index) in images.slice(0, 6)"
            :key="index"
            class="image-item"
            @click="handleImageClick(img)"
          >
            <img :src="getImageSrc(img)" :alt="`图片 ${index + 1}`" />
          </div>
          <div v-if="images.length > 6" class="more-images">
            +{{ images.length - 6 }} 更多
          </div>
        </div>
      </div>
    </template>

    <!-- 错误提示 -->
    <div v-if="error && !isLoading" class="error-message">
      <span class="error-icon">⚠️</span>
      <span class="error-text">{{ error }}</span>
    </div>

    <!-- 成功提示 -->
    <Transition name="toast">
      <div v-if="showCopySuccess" class="success-toast">
        ✓ Markdown 已复制到剪贴板
      </div>
    </Transition>

    <!-- 复制失败提示 -->
    <Transition name="toast">
      <div v-if="showCopyError" class="error-toast">
        ✕ 复制失败，请重试
      </div>
    </Transition>
  </div>
</template>

<script setup lang="ts">
/**
 * 网页爬取对话框组件
 * 
 * 功能：
 * - URL 输入和验证
 * - 显示爬取进度
 * - Markdown 预览
 * - 复制 Markdown 内容
 * - 显示提取的图片
 * 
 * @validates Requirements 11.1, 11.2
 */

import { ref, computed, watch, onMounted, onBeforeUnmount, nextTick } from 'vue'
import { useWebScraper } from '@/composables/useWebScraper'
import { writeText } from '@tauri-apps/plugin-clipboard-manager'
import { convertFileSrc } from '@tauri-apps/api/core'
import { sanitizeUrl, sanitizeImageSrc } from '@/utils/sanitize'

// ============================================
// Props & Emits
// ============================================

interface Props {
  /** 初始 URL */
  initialUrl?: string
  /** 是否显示对话框 */
  visible?: boolean
}

const props = withDefaults(defineProps<Props>(), {
  initialUrl: '',
  visible: true,
})

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'success', result: { markdown: string; title: string; images: string[] }): void
  (e: 'error', message: string): void
}>()

// ============================================
// Composables
// ============================================

const {
  isLoading,
  error: scrapeError,
  hasResult,
  scrape,
  clearResult,
  validateUrl,
  getMarkdown,
  getTitle,
  getImages,
} = useWebScraper()

// ============================================
// State
// ============================================

/** URL 输入 */
const urlInput = ref('')

/** URL 验证错误 */
const urlError = ref<string | null>(null)

/** 是否下载图片 */
const downloadImages = ref(true)

/** 是否显示复制成功提示 */
const showCopySuccess = ref(false)

/** 是否显示复制失败提示 */
const showCopyError = ref(false)

/** 进度文本 */
const progressText = ref('正在连接...')

/** 进度百分比 */
const progressPercent = ref(0)

/** 进度定时器 */
let activeProgressInterval: ReturnType<typeof setInterval> | null = null

/** 活跃的超时定时器 */
let activeTimeouts: ReturnType<typeof setTimeout>[] = []

/** URL 输入框引用 */
const urlInputRef = ref<HTMLInputElement | null>(null)

/** 预览区域引用 */
const previewRef = ref<HTMLDivElement | null>(null)

// ============================================
// Computed
// ============================================

/** 是否可以爬取 */
const canScrape = computed(() => 
  !isLoading.value && 
  urlInput.value.trim().length > 0 &&
  !urlError.value
)

/** 错误信息 */
const error = computed(() => scrapeError.value)

/** 结果标题 */
const resultTitle = computed(() => getTitle() || '未知标题')

/** 图片列表 */
const images = computed(() => getImages())

/** 进度条宽度 */
const progressWidth = computed(() => `${progressPercent.value}%`)

/** 渲染后的 Markdown (简单转换) */
const renderedMarkdown = computed(() => {
  const markdown = getMarkdown()
  if (!markdown) return ''
  
  // 简单的 Markdown 转 HTML
  // 实际项目中可以使用 markdown-it 等库
  return markdown
    // 转义 HTML
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    // 标题
    .replace(/^### (.+)$/gm, '<h3>$1</h3>')
    .replace(/^## (.+)$/gm, '<h2>$1</h2>')
    .replace(/^# (.+)$/gm, '<h1>$1</h1>')
    // 粗体和斜体
    .replace(/\*\*(.+?)\*\*/g, '<strong>$1</strong>')
    .replace(/\*(.+?)\*/g, '<em>$1</em>')
    // 代码块
    .replace(/```(\w*)\n([\s\S]*?)```/g, '<pre><code class="language-$1">$2</code></pre>')
    // 行内代码
    .replace(/`([^`]+)`/g, '<code>$1</code>')
    // 图片（必须在链接之前处理，否则 ![alt](url) 会被链接 regex 部分匹配）
    // sanitizeImageSrc 只允许 http/https 协议，阻止 data:/javascript: 等
    .replace(/!\[([^\]]*)\]\(([^)]+)\)/g, (_match, alt, src) => {
      const safeSrc = sanitizeImageSrc(src)
      return safeSrc ? `<img src="${safeSrc}" alt="${alt}" />` : `<span>[图片: ${alt}]</span>`
    })
    // 链接（sanitizeUrl 防止 javascript: 等危险协议注入）
    .replace(/\[([^\]]+)\]\(([^)]+)\)/g, (_match, text, url) => `<a href="${sanitizeUrl(url)}" target="_blank" rel="noopener">${text}</a>`)
    // 列表
    .replace(/^- (.+)$/gm, '<li>$1</li>')
    .replace(/(<li>.*<\/li>\n?)+/g, '<ul>$&</ul>')
    // 段落
    .replace(/\n\n/g, '</p><p>')
    .replace(/^(.+)$/gm, (match) => {
      if (match.startsWith('<')) return match
      return `<p>${match}</p>`
    })
})

// ============================================
// Methods
// ============================================

/**
 * 处理 URL 输入变化
 */
function handleUrlChange(): void {
  const url = urlInput.value.trim()
  
  if (!url) {
    urlError.value = null
    return
  }
  
  if (!validateUrl(url)) {
    urlError.value = '请输入有效的 URL（以 http:// 或 https:// 开头）'
  } else {
    urlError.value = null
  }
}

/**
 * 模拟进度更新
 */
function simulateProgress(): void {
  // 清理上一个进度定时器
  if (activeProgressInterval) {
    clearInterval(activeProgressInterval)
    activeProgressInterval = null
  }

  progressPercent.value = 0
  progressText.value = '正在连接...'
  
  const stages = [
    { percent: 20, text: '正在加载页面...' },
    { percent: 50, text: '正在解析内容...' },
    { percent: 70, text: '正在提取图片...' },
    { percent: 90, text: '正在生成 Markdown...' },
  ]
  
  let stageIndex = 0
  activeProgressInterval = setInterval(() => {
    if (!isLoading.value || stageIndex >= stages.length) {
      if (activeProgressInterval) {
        clearInterval(activeProgressInterval)
        activeProgressInterval = null
      }
      if (!isLoading.value) {
        progressPercent.value = 100
        progressText.value = '完成'
      }
      return
    }
    
    const stage = stages[stageIndex]
    progressPercent.value = stage.percent
    progressText.value = stage.text
    stageIndex++
  }, 800)
}

/**
 * 执行爬取
 */
async function handleScrape(): Promise<void> {
  if (!canScrape.value) return
  
  // 清除之前的结果
  clearResult()
  
  // 开始模拟进度
  simulateProgress()
  
  // 执行爬取
  const result = await scrape(urlInput.value, {
    downloadImages: downloadImages.value,
  })
  
  if (result) {
    progressPercent.value = 100
    progressText.value = '完成'
    
    emit('success', {
      markdown: result.markdown,
      title: result.title,
      images: result.images,
    })
    
    // 滚动到预览区域
    await nextTick()
    previewRef.value?.scrollIntoView({ behavior: 'smooth' })
  } else {
    emit('error', error.value || '爬取失败')
  }
}

/**
 * 复制 Markdown 到剪贴板
 */
async function handleCopyMarkdown(): Promise<void> {
  const markdown = getMarkdown()
  if (!markdown) return
  
  try {
    await writeText(markdown)
    showCopySuccess.value = true
    const tid = setTimeout(() => {
      showCopySuccess.value = false
    }, 2000)
    activeTimeouts.push(tid)
  } catch (e) {
    console.error('Failed to copy markdown:', e)
    showCopyError.value = true
    const tid = setTimeout(() => {
      showCopyError.value = false
    }, 2000)
    activeTimeouts.push(tid)
  }
}

/**
 * 获取图片源 URL
 */
function getImageSrc(imagePath: string): string {
  // 如果是本地路径，转换为 asset:// 协议
  if (imagePath.startsWith('http://') || imagePath.startsWith('https://')) {
    return imagePath
  }
  return convertFileSrc(imagePath)
}

/**
 * 处理图片点击
 */
function handleImageClick(imagePath: string): void {
  // 可以在这里实现图片预览功能
  console.log('Image clicked:', imagePath)
}

/**
 * 关闭对话框
 */
function handleClose(): void {
  emit('close')
}

// ============================================
// Watchers
// ============================================

// 监听初始 URL
watch(() => props.initialUrl, (newVal) => {
  if (newVal) {
    urlInput.value = newVal
    handleUrlChange()
  }
}, { immediate: true })

// 监听可见性变化
watch(() => props.visible, (newVal) => {
  if (newVal) {
    // 聚焦到输入框
    nextTick(() => {
      urlInputRef.value?.focus()
    })
  }
})

// ============================================
// Lifecycle
// ============================================

onMounted(() => {
  if (props.visible) {
    urlInputRef.value?.focus()
  }
})

onBeforeUnmount(() => {
  // 清理进度定时器
  if (activeProgressInterval) {
    clearInterval(activeProgressInterval)
    activeProgressInterval = null
  }
  // 清理所有 setTimeout
  activeTimeouts.forEach(tid => clearTimeout(tid))
  activeTimeouts = []
})
</script>

<style scoped>
.web-scraper-dialog {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 16px;
  background: rgba(30, 30, 30, 0.98);
  border-radius: 8px;
  border: 1px solid rgba(255, 255, 255, 0.1);
  min-width: 400px;
  max-width: 600px;
  max-height: 80vh;
  overflow: hidden;
  position: relative;
}

/* 对话框头部 */
.dialog-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding-bottom: 8px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.1);
}

.dialog-title {
  color: #fff;
  font-size: 15px;
  font-weight: 500;
}

.close-btn {
  width: 24px;
  height: 24px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: none;
  border-radius: 4px;
  color: rgba(255, 255, 255, 0.6);
  font-size: 14px;
  cursor: pointer;
  transition: all 0.15s;
}

.close-btn:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.1);
  color: #fff;
}

.close-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* URL 输入区域 */
.url-input-section {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.form-group {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.form-label {
  color: rgba(255, 255, 255, 0.7);
  font-size: 12px;
  font-weight: 500;
}
</style>

<style scoped>
.url-input-wrapper {
  display: flex;
  gap: 8px;
}

.form-input,
.url-input {
  flex: 1;
  padding: 10px 12px;
  background: rgba(0, 0, 0, 0.3);
  border: 1px solid rgba(255, 255, 255, 0.15);
  border-radius: 4px;
  color: #fff;
  font-size: 13px;
  outline: none;
  transition: border-color 0.15s;
}

.form-input:hover,
.url-input:hover {
  border-color: rgba(255, 255, 255, 0.25);
}

.form-input:focus,
.url-input:focus {
  border-color: rgba(66, 133, 244, 0.6);
}

.form-input:disabled,
.url-input:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.url-error {
  color: #ff6b6b;
  font-size: 11px;
  margin-top: 2px;
}

.scrape-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 10px 16px;
  background: rgba(66, 133, 244, 0.8);
  border: none;
  border-radius: 4px;
  color: #fff;
  font-size: 13px;
  cursor: pointer;
  transition: all 0.15s;
  white-space: nowrap;
}

.scrape-btn:hover:not(:disabled) {
  background: rgba(66, 133, 244, 1);
}

.scrape-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.scrape-btn.is-loading {
  background: rgba(66, 133, 244, 0.6);
}

.btn-icon {
  font-size: 14px;
}

/* 选项行 */
.options-row {
  display: flex;
  gap: 16px;
}

.checkbox-label {
  display: flex;
  align-items: center;
  gap: 6px;
  cursor: pointer;
}

.checkbox-label input[type="checkbox"] {
  width: 14px;
  height: 14px;
  cursor: pointer;
}

.checkbox-text {
  color: rgba(255, 255, 255, 0.7);
  font-size: 12px;
}
</style>

<style scoped>
/* 进度区域 */
.progress-section {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 12px;
  background: rgba(0, 0, 0, 0.2);
  border-radius: 4px;
}

.progress-bar {
  height: 4px;
  background: rgba(255, 255, 255, 0.1);
  border-radius: 2px;
  overflow: hidden;
}

.progress-bar-inner {
  height: 100%;
  background: linear-gradient(90deg, #4285f4, #34a853);
  border-radius: 2px;
  transition: width 0.3s ease;
}

.progress-text {
  color: rgba(255, 255, 255, 0.6);
  font-size: 12px;
  text-align: center;
}

/* 结果区域 */
.result-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 8px 0;
  border-bottom: 1px solid rgba(255, 255, 255, 0.1);
}

.result-title {
  color: #fff;
  font-size: 14px;
  font-weight: 500;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  max-width: 300px;
}

.result-actions {
  display: flex;
  gap: 8px;
}

.action-btn {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 6px 12px;
  border: none;
  border-radius: 4px;
  font-size: 12px;
  cursor: pointer;
  transition: all 0.15s;
}

.copy-btn {
  background: rgba(255, 255, 255, 0.1);
  color: rgba(255, 255, 255, 0.8);
}

.copy-btn:hover {
  background: rgba(255, 255, 255, 0.15);
  color: #fff;
}
</style>

<style scoped>
/* Markdown 预览 */
.markdown-preview {
  flex: 1;
  min-height: 200px;
  max-height: 300px;
  overflow-y: auto;
  padding: 12px;
  background: rgba(0, 0, 0, 0.2);
  border-radius: 4px;
  border: 1px solid rgba(255, 255, 255, 0.1);
}

.markdown-preview::-webkit-scrollbar {
  width: 6px;
}

.markdown-preview::-webkit-scrollbar-track {
  background: rgba(255, 255, 255, 0.05);
  border-radius: 3px;
}

.markdown-preview::-webkit-scrollbar-thumb {
  background: rgba(255, 255, 255, 0.2);
  border-radius: 3px;
}

.markdown-content {
  color: rgba(255, 255, 255, 0.9);
  font-size: 13px;
  line-height: 1.6;
}

.markdown-content :deep(h1) {
  font-size: 20px;
  font-weight: 600;
  margin: 16px 0 8px;
  color: #fff;
}

.markdown-content :deep(h2) {
  font-size: 17px;
  font-weight: 600;
  margin: 14px 0 6px;
  color: #fff;
}

.markdown-content :deep(h3) {
  font-size: 15px;
  font-weight: 600;
  margin: 12px 0 4px;
  color: #fff;
}

.markdown-content :deep(p) {
  margin: 8px 0;
}

.markdown-content :deep(a) {
  color: #4285f4;
  text-decoration: none;
}

.markdown-content :deep(a:hover) {
  text-decoration: underline;
}

.markdown-content :deep(code) {
  background: rgba(255, 255, 255, 0.1);
  padding: 2px 6px;
  border-radius: 3px;
  font-family: 'Consolas', 'Monaco', monospace;
  font-size: 12px;
}

.markdown-content :deep(pre) {
  background: rgba(0, 0, 0, 0.3);
  padding: 12px;
  border-radius: 4px;
  overflow-x: auto;
  margin: 8px 0;
}

.markdown-content :deep(pre code) {
  background: transparent;
  padding: 0;
}

.markdown-content :deep(ul),
.markdown-content :deep(ol) {
  margin: 8px 0;
  padding-left: 20px;
}

.markdown-content :deep(li) {
  margin: 4px 0;
}

.markdown-content :deep(img) {
  max-width: 100%;
  border-radius: 4px;
  margin: 8px 0;
}

.markdown-content :deep(strong) {
  font-weight: 600;
  color: #fff;
}

.markdown-content :deep(em) {
  font-style: italic;
}
</style>

<style scoped>
/* 图片区域 */
.images-section {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.images-header {
  display: flex;
  align-items: center;
}

.images-title {
  color: rgba(255, 255, 255, 0.7);
  font-size: 12px;
  font-weight: 500;
}

.images-grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 8px;
}

.image-item {
  aspect-ratio: 1;
  border-radius: 4px;
  overflow: hidden;
  background: rgba(0, 0, 0, 0.3);
  cursor: pointer;
  transition: transform 0.15s;
}

.image-item:hover {
  transform: scale(1.05);
}

.image-item img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.more-images {
  display: flex;
  align-items: center;
  justify-content: center;
  aspect-ratio: 1;
  border-radius: 4px;
  background: rgba(255, 255, 255, 0.1);
  color: rgba(255, 255, 255, 0.6);
  font-size: 12px;
}

/* 错误提示 */
.error-message {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 12px;
  background: rgba(244, 67, 54, 0.15);
  border: 1px solid rgba(244, 67, 54, 0.3);
  border-radius: 4px;
}

.error-icon {
  font-size: 14px;
}

.error-text {
  color: #ff6b6b;
  font-size: 12px;
}

/* 成功提示 */
.success-toast {
  position: absolute;
  bottom: 16px;
  left: 50%;
  transform: translateX(-50%);
  padding: 10px 20px;
  background: rgba(76, 175, 80, 0.95);
  border-radius: 4px;
  color: #fff;
  font-size: 13px;
  font-weight: 500;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.3);
  z-index: 100;
}

/* 错误提示 toast */
.error-toast {
  position: absolute;
  bottom: 16px;
  left: 50%;
  transform: translateX(-50%);
  padding: 10px 20px;
  background: rgba(244, 67, 54, 0.95);
  border-radius: 4px;
  color: #fff;
  font-size: 13px;
  font-weight: 500;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.3);
  z-index: 100;
}

/* 加载动画 */
.loading-spinner-small {
  width: 14px;
  height: 14px;
  border: 2px solid rgba(255, 255, 255, 0.3);
  border-top-color: #fff;
  border-radius: 50%;
  animation: spin 0.6s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
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
