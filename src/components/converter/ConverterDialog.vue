<template>
  <div class="converter-dialog">
    <!-- 对话框头部 -->
    <div class="dialog-header">
      <span class="dialog-title">🔄 MD转换工具</span>
      <button class="close-btn" @click="handleClose" :disabled="isLoading">
        ✕
      </button>
    </div>

    <!-- 双向转换卡片布局 -->
    <div class="conversion-grid">
      <!-- 网页 ⇌ MD 组 -->
      <div class="conversion-group">
        <div class="group-header">
          <span class="group-icon">🌐</span>
          <span class="group-title">网页 ⇌ Markdown</span>
        </div>
        <div class="group-buttons">
          <button
            class="direction-btn"
            :class="{ 'is-active': activeTab === 'url-to-md' }"
            :disabled="isLoading"
            @click="activeTab = 'url-to-md'"
          >
            <span class="direction-from">网页</span>
            <span class="direction-arrow">→</span>
            <span class="direction-to">MD</span>
          </button>
          <button
            class="direction-btn"
            :class="{ 'is-active': activeTab === 'md-to-url' }"
            :disabled="isLoading"
            @click="activeTab = 'md-to-url'"
          >
            <span class="direction-from">MD</span>
            <span class="direction-arrow">→</span>
            <span class="direction-to">网页</span>
          </button>
        </div>
      </div>

      <!-- 文件 ⇌ MD 组 -->
      <div class="conversion-group">
        <div class="group-header">
          <span class="group-icon">📄</span>
          <span class="group-title">文件 ⇌ Markdown</span>
        </div>
        <div class="group-buttons">
          <button
            class="direction-btn"
            :class="{ 'is-active': activeTab === 'file-to-md' }"
            :disabled="isLoading"
            @click="activeTab = 'file-to-md'"
          >
            <span class="direction-from">文件</span>
            <span class="direction-arrow">→</span>
            <span class="direction-to">MD</span>
          </button>
          <button
            class="direction-btn"
            :class="{ 'is-active': activeTab === 'md-to-file' }"
            :disabled="isLoading"
            @click="activeTab = 'md-to-file'"
          >
            <span class="direction-from">MD</span>
            <span class="direction-arrow">→</span>
            <span class="direction-to">文件</span>
          </button>
        </div>
      </div>
    </div>

    <!-- 文件转 Markdown -->
    <div v-show="activeTab === 'file-to-md'" class="tab-content">
      <!-- 拖拽上传区域 -->
      <div
        class="drop-zone"
        :class="{ 'is-dragging': isDragging, 'has-file': selectedFile }"
        @dragover.prevent="handleDragOver"
        @dragleave.prevent="handleDragLeave"
        @drop.prevent="handleFileDrop"
        @click="handleFileSelect"
      >
        <template v-if="!selectedFile">
          <span class="drop-icon">📂</span>
          <span class="drop-text">拖拽文件到此处，或点击选择</span>
          <span class="drop-hint">支持 Word、PDF、Excel、PPT、HTML、图片等</span>
        </template>
        <template v-else>
          <span class="file-icon">{{ getFileIcon(selectedFile) }}</span>
          <span class="file-name">{{ getFileName(selectedFile) }}</span>
          <button class="clear-file-btn" @click.stop="clearSelectedFile">✕</button>
        </template>
      </div>

      <!-- 转换按钮 -->
      <button
        class="convert-btn"
        :class="{ 'is-loading': isLoading }"
        :disabled="!selectedFile || isLoading"
        @click="handleFileToMarkdown"
      >
        <span v-if="isLoading" class="loading-spinner-small" />
        <span v-else class="btn-icon">⚡</span>
        <span class="btn-text">{{ isLoading ? '转换中...' : '转换为 Markdown' }}</span>
      </button>

      <!-- 设置区域 -->
      <div class="settings-section">
        <div class="settings-header" @click="toggleFileSettings">
          <span class="settings-icon">⚙️</span>
          <span class="settings-title">转换设置</span>
          <span class="settings-arrow" :class="{ expanded: showFileSettings }">▼</span>
        </div>
        <div v-show="showFileSettings" class="settings-content">
          <!-- 转换引擎选择 -->
          <div class="setting-row">
            <span class="setting-label">转换引擎</span>
            <div class="engine-options">
              <button
                class="engine-btn"
                :class="{ 'is-active': fileToMarkdownConfig.engine === 'local' }"
                @click="handleEngineChange('local')"
              >
                💻 本地转换
              </button>
              <button
                class="engine-btn"
                :class="{ 'is-active': fileToMarkdownConfig.engine === 'mineru' }"
                @click="handleEngineChange('mineru')"
              >
                ☁️ MinerU API
              </button>
            </div>
          </div>

          <!-- 本地转换说明 -->
          <div v-if="fileToMarkdownConfig.engine === 'local'" class="config-info">
            <span class="info-icon">💡</span>
            <span class="info-text">混合引擎：PDF 使用 pymupdf4llm（高质量），Word/PPT/Excel 使用 MarkItDown，图片建议用 OCR 工作台</span>
          </div>

          <!-- MinerU API 配置（仅在选择 MinerU 时显示） -->
          <template v-if="fileToMarkdownConfig.engine === 'mineru'">
            <div v-if="!fileToMarkdownConfig.apiToken" class="config-prompt">
              <span class="prompt-icon">⚠️</span>
              <span class="prompt-text">请配置 MinerU API Token 以使用云端转换功能</span>
            </div>
            <div class="setting-row">
              <span class="setting-label">API Token</span>
              <input
                type="password"
                class="token-input"
                :value="fileToMarkdownConfig.apiToken"
                placeholder="输入 API Token"
                @input="handleApiTokenChange"
              />
            </div>
            <div class="setting-row">
              <span class="setting-label">获取 Token</span>
              <a
                href="https://mineru.net/apiManage/token"
                target="_blank"
                rel="noopener noreferrer"
                class="token-link"
              >
                前往 MinerU 官网 ↗
              </a>
            </div>
          </template>
        </div>
      </div>
    </div>

    <!-- 网页转 Markdown -->
    <div v-show="activeTab === 'url-to-md'" class="tab-content">
      <div class="form-group">
        <label class="form-label">网页地址</label>
        <div class="url-input-wrapper">
          <input
            v-model="urlInput"
            type="url"
            class="form-input url-input"
            placeholder="https://example.com/article"
            :disabled="isLoading"
            @keydown.enter="handleUrlToMarkdown"
          />
        </div>
      </div>

      <!-- 抓取模式选择 -->
      <div class="form-group">
        <label class="form-label">抓取模式</label>
        <div class="engine-options">
          <button
            class="engine-btn"
            :class="{ 'is-active': urlEngine === 'auto' }"
            :disabled="isLoading"
            @click="urlEngine = 'auto'"
            title="优先快速提取，失败时自动切换浏览器"
          >
            ⚡ 智能
          </button>
          <button
            class="engine-btn"
            :class="{ 'is-active': urlEngine === 'trafilatura' }"
            :disabled="isLoading"
            @click="urlEngine = 'trafilatura'"
            title="纯HTTP请求，速度最快"
          >
            🚀 极速
          </button>
          <button
            class="engine-btn"
            :class="{ 'is-active': urlEngine === 'browser' }"
            :disabled="isLoading"
            @click="urlEngine = 'browser'"
            title="使用浏览器渲染，适合JS动态页面"
          >
            🌐 浏览器
          </button>
        </div>
        <span class="engine-hint">{{
          urlEngine === 'auto' ? '自动选择最佳通道（推荐）' :
          urlEngine === 'trafilatura' ? '最快速度，适合博客/新闻/文档' :
          '支持JS渲染，适合SPA/动态页面'
        }}</span>
      </div>

      <!-- 转换按钮 -->
      <button
        class="convert-btn"
        :class="{ 'is-loading': isLoading }"
        :disabled="!urlInput.trim() || isLoading"
        @click="handleUrlToMarkdown"
      >
        <span v-if="isLoading" class="loading-spinner-small" />
        <span v-else class="btn-icon">🌐</span>
        <span class="btn-text">{{ isLoading ? '抓取中...' : '抓取并转换' }}</span>
      </button>
    </div>

    <!-- Markdown 转文件 -->
    <div v-show="activeTab === 'md-to-file'" class="tab-content">
      <!-- Markdown 输入 -->
      <div class="form-group">
        <label class="form-label">Markdown 内容</label>
        <textarea
          v-model="markdownInput"
          class="markdown-textarea"
          placeholder="# 标题&#10;&#10;在此输入或粘贴 Markdown 内容..."
          :disabled="isLoading"
          rows="8"
        />
      </div>

      <!-- 输出格式选择 -->
      <div class="form-group">
        <label class="form-label">输出格式</label>
        <div class="format-options">
          <button
            v-for="fmt in outputFormats"
            :key="fmt.value"
            class="format-btn"
            :class="{ 'is-active': outputFormat === fmt.value }"
            :disabled="isLoading"
            @click="outputFormat = fmt.value"
          >
            <span class="format-icon">{{ fmt.icon }}</span>
            <span class="format-label">{{ fmt.label }}</span>
          </button>
        </div>
      </div>

      <!-- 转换按钮 -->
      <button
        class="convert-btn"
        :class="{ 'is-loading': isLoading }"
        :disabled="!markdownInput.trim() || isLoading"
        @click="handleMarkdownToFile"
      >
        <span v-if="isLoading" class="loading-spinner-small" />
        <span v-else class="btn-icon">💾</span>
        <span class="btn-text">{{ isLoading ? '生成中...' : `生成 ${outputFormat.toUpperCase()} 文件` }}</span>
      </button>
    </div>

    <!-- Markdown 转网页 -->
    <div v-show="activeTab === 'md-to-url'" class="tab-content">
      <!-- Markdown 输入 -->
      <div class="form-group">
        <label class="form-label">Markdown 内容</label>
        <textarea
          v-model="markdownForHtml"
          class="markdown-textarea"
          placeholder="# 标题&#10;&#10;在此输入或粘贴 Markdown 内容..."
          :disabled="isLoading"
          rows="6"
        />
      </div>

      <!-- HTML 预览（默认显示） -->
      <div v-if="markdownForHtml.trim()" class="html-preview-wrapper">
        <div class="preview-header">
          <span class="preview-title">📄 预览</span>
        </div>
        <div class="html-preview" v-html="renderedHtml" />
      </div>

      <!-- 转换按钮 -->
      <button
        class="convert-btn"
        :class="{ 'is-loading': isLoading }"
        :disabled="!markdownForHtml.trim() || isLoading"
        @click="handleMarkdownToHtml"
      >
        <span v-if="isLoading" class="loading-spinner-small" />
        <span v-else class="btn-icon">🌐</span>
        <span class="btn-text">{{ isLoading ? '生成中...' : '生成 HTML 页面' }}</span>
      </button>
    </div>

    <!-- 进度指示器 -->
    <div v-if="isLoading" class="progress-section">
      <div class="progress-bar">
        <div class="progress-bar-inner" :style="{ width: progressWidth }" />
      </div>
      <span class="progress-text">{{ progressText }}</span>
    </div>

    <!-- 结果区域 -->
    <template v-if="hasResult && !isLoading">
      <!-- Markdown 结果 -->
      <div v-if="resultMarkdown" class="result-section">
        <div class="result-header">
          <span class="result-title">📄 {{ resultTitle || '转换结果' }}</span>
          <div class="result-actions">
            <button class="action-btn copy-btn" @click="handleCopyMarkdown" title="复制">
              📋 复制
            </button>
            <button class="action-btn save-btn" @click="handleSaveMarkdown" title="保存">
              💾 保存
            </button>
          </div>
        </div>
        <div class="markdown-preview">
          <pre class="markdown-content">{{ resultMarkdown }}</pre>
        </div>
      </div>

      <!-- 文件结果 -->
      <div v-if="resultFilePath" class="result-section">
        <div class="result-header">
          <span class="result-title">✅ 文件已生成</span>
        </div>
        <div class="output-path-wrapper">
          <span class="output-path">{{ resultFilePath }}</span>
          <div class="output-actions">
            <button class="action-btn open-btn" @click="handleOpenFile" title="打开文件">
              📂 打开
            </button>
            <button class="action-btn folder-btn" @click="handleOpenFolder" title="打开文件夹">
              📁 文件夹
            </button>
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
      <div v-if="showSuccess" class="success-toast">
        ✓ {{ successMessage }}
      </div>
    </Transition>

    <!-- 错误提示 -->
    <Transition name="toast">
      <div v-if="showError" class="error-toast">
        ✕ {{ errorMessage }}
      </div>
    </Transition>
  </div>
</template>

<script setup lang="ts">
/**
 * 文件转换工具对话框组件
 *
 * 功能：
 * - 文件转 Markdown（Word/PDF/Excel/PPT/HTML）
 * - 网页转 Markdown
 * - Markdown 转文件（Word/PDF/HTML）
 * - Markdown 转网页（HTML）
 */

import { ref, computed, watch, onUnmounted } from 'vue'
import { open, save } from '@tauri-apps/plugin-dialog'
import { openPath, revealItemInDir } from '@tauri-apps/plugin-opener'
import { writeText } from '@tauri-apps/plugin-clipboard-manager'
import { invoke } from '@tauri-apps/api/core'
import { useSidecarStore } from '@/stores/sidecar'
import { useSettingsStore } from '@/stores/settings'
import { sanitizeUrl } from '@/utils/sanitize'
import type { MarkdownToFileFormat, FileToMarkdownEngine } from '@/types'

/** 简单的 Markdown 转 HTML 解析器 */
function parseMarkdown(md: string): string {
  let html = md
    // 转义 HTML 特殊字符
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    // 代码块
    .replace(/```(\w*)\n([\s\S]*?)```/g, '<pre><code>$2</code></pre>')
    // 行内代码
    .replace(/`([^`]+)`/g, '<code>$1</code>')
    // 标题
    .replace(/^### (.+)$/gm, '<h3>$1</h3>')
    .replace(/^## (.+)$/gm, '<h2>$1</h2>')
    .replace(/^# (.+)$/gm, '<h1>$1</h1>')
    // 粗体和斜体
    .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
    .replace(/\*([^*]+)\*/g, '<em>$1</em>')
    // 链接（sanitizeUrl 防止 javascript: 等危险协议）
    .replace(/\[([^\]]+)\]\(([^)]+)\)/g, (_match, text, url) => `<a href="${sanitizeUrl(url)}">${text}</a>`)
    // 引用
    .replace(/^> (.+)$/gm, '<blockquote>$1</blockquote>')
    // 无序列表
    .replace(/^[*-] (.+)$/gm, '<li>$1</li>')
    // 分隔线
    .replace(/^---$/gm, '<hr>')
    // 段落
    .replace(/\n\n/g, '</p><p>')
    .replace(/\n/g, '<br>')

  // 包装列表项
  html = html.replace(/(<li>.*<\/li>)+/g, '<ul>$&</ul>')

  return `<p>${html}</p>`
}

// ============================================
// Props & Emits
// ============================================

const props = defineProps<{
  visible?: boolean
  defaultTab?: 'file' | 'url' | 'export'
}>()

const emit = defineEmits<{
  (e: 'close'): void
}>()

// ============================================
// Store
// ============================================

const sidecarStore = useSidecarStore()
const settingsStore = useSettingsStore()

// ============================================
// Computed - Settings
// ============================================

/** File to Markdown settings from store */
const fileToMarkdownConfig = computed(() => settingsStore.fileToMarkdown)

// ============================================
// Constants
// ============================================

const outputFormats: Array<{ value: MarkdownToFileFormat; icon: string; label: string }> = [
  { value: 'docx', icon: '📘', label: 'Word' },
  { value: 'pdf', icon: '📕', label: 'PDF' },
  { value: 'html', icon: '🌐', label: 'HTML' },
]

const fileExtensions: Record<string, string[]> = {
  word: ['docx', 'doc'],
  pdf: ['pdf'],
  excel: ['xlsx', 'xls'],
  ppt: ['pptx', 'ppt'],
  html: ['html', 'htm'],
  text: ['txt', 'md', 'csv', 'json', 'xml'],
}

// ============================================
// State
// ============================================

const activeTab = ref('url-to-md')
const isLoading = ref(false)
const error = ref<string | null>(null)

// 同步 defaultTab 到 activeTab
watch(
  () => props.defaultTab,
  (newTab) => {
    if (newTab) {
      const tabMap: Record<string, string> = {
        file: 'file-to-md',
        url: 'url-to-md',
        export: 'md-to-file',
        html: 'md-to-url',
      }
      activeTab.value = tabMap[newTab] || 'url-to-md'
    }
  },
  { immediate: true }
)

// 文件转 MD
const selectedFile = ref<string | null>(null)
const isDragging = ref(false)

// 网页转 MD
const urlInput = ref('')
const urlEngine = ref<'auto' | 'trafilatura' | 'browser'>('auto')

// MD 转文件
const markdownInput = ref('')
const outputFormat = ref<MarkdownToFileFormat>('docx')

// MD 转网页（默认 GitHub 风格）
const markdownForHtml = ref('')

// 结果
const resultMarkdown = ref<string | null>(null)
const resultTitle = ref<string | null>(null)
const resultFilePath = ref<string | null>(null)

// 进度
const progressPercent = ref(0)
const progressText = ref('')

// 成功提示
const showSuccess = ref(false)
const successMessage = ref('')

// 错误提示
const showError = ref(false)
const errorMessage = ref('')

// 设置面板展开状态
const showFileSettings = ref(false)

// ============================================
// Computed
// ============================================

const hasResult = computed(() => !!resultMarkdown.value || !!resultFilePath.value)
const progressWidth = computed(() => `${progressPercent.value}%`)

/** 渲染 Markdown 为 HTML（用于预览） */
const renderedHtml = computed(() => {
  if (!markdownForHtml.value.trim()) return ''
  try {
    return parseMarkdown(markdownForHtml.value)
  } catch {
    return '<p style="color: #ff6b6b;">Markdown 解析失败</p>'
  }
})

// ============================================
// Methods
// ============================================

function clearResult(): void {
  resultMarkdown.value = null
  resultTitle.value = null
  resultFilePath.value = null
  error.value = null
}

function clearSelectedFile(): void {
  selectedFile.value = null
}

function getFileIcon(filePath: string): string {
  const ext = filePath.split('.').pop()?.toLowerCase() || ''
  if (fileExtensions.word.includes(ext)) return '📘'
  if (fileExtensions.pdf.includes(ext)) return '📕'
  if (fileExtensions.excel.includes(ext)) return '📊'
  if (fileExtensions.ppt.includes(ext)) return '📙'
  if (fileExtensions.html.includes(ext)) return '🌐'
  return '📄'
}

function getFileName(filePath: string): string {
  return filePath.split(/[/\\]/).pop() || filePath
}

function showSuccessToast(message: string): void {
  successMessage.value = message
  showSuccess.value = true
  const tid = setTimeout(() => {
    showSuccess.value = false
  }, 2000)
  activeTimeouts.push(tid)
}

function showErrorToast(message: string): void {
  errorMessage.value = message
  showError.value = true
  const tid = setTimeout(() => {
    showError.value = false
  }, 3000)
  activeTimeouts.push(tid)
}

// 活跃的定时器 ID（用于组件卸载时清理）
let activeProgressInterval: ReturnType<typeof setInterval> | null = null
let activeTimeouts: ReturnType<typeof setTimeout>[] = []

function simulateProgress(stages: Array<{ percent: number; text: string }>): void {
  // 清理上一个进度定时器
  if (activeProgressInterval) {
    clearInterval(activeProgressInterval)
    activeProgressInterval = null
  }

  progressPercent.value = 0
  progressText.value = stages[0]?.text || '处理中...'

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
  }, 600)
}

// 组件卸载时清理定时器
onUnmounted(() => {
  if (activeProgressInterval) {
    clearInterval(activeProgressInterval)
    activeProgressInterval = null
  }
  // 清理所有 setTimeout
  activeTimeouts.forEach(tid => clearTimeout(tid))
  activeTimeouts = []
})

// 拖拽处理
function handleDragOver(_e: DragEvent): void {
  isDragging.value = true
}

function handleDragLeave(_e: DragEvent): void {
  isDragging.value = false
}

function handleFileDrop(e: DragEvent): void {
  isDragging.value = false
  const files = e.dataTransfer?.files
  if (files && files.length > 0) {
    // Tauri 拖拽返回的是路径
    const file = files[0]
    if ('path' in file && typeof (file as File & { path?: string }).path === 'string') {
      selectedFile.value = (file as File & { path?: string }).path!
    }
  }
}

async function handleFileSelect(): Promise<void> {
  if (isLoading.value) return

  try {
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [
        { name: 'Word', extensions: ['docx', 'doc'] },
        { name: 'PDF', extensions: ['pdf'] },
        { name: 'Excel', extensions: ['xlsx', 'xls'] },
        { name: 'PowerPoint', extensions: ['pptx', 'ppt'] },
        { name: 'HTML', extensions: ['html', 'htm'] },
        { name: '图片', extensions: ['png', 'jpg', 'jpeg', 'gif', 'bmp', 'webp'] },
        { name: '文本文件', extensions: ['txt', 'md', 'csv', 'json', 'xml'] },
        { name: '所有文件', extensions: ['*'] },
      ],
      title: '选择要转换的文件',
    })

    if (selected && typeof selected === 'string') {
      selectedFile.value = selected
    }
  } catch (e) {
    console.error('Failed to open file dialog:', e)
    showErrorToast('打开文件选择器失败')
  }
}

// 文件转 Markdown
async function handleFileToMarkdown(): Promise<void> {
  if (!selectedFile.value || isLoading.value) return

  // 先让用户选择保存位置，默认为源文件同目录同名 .md 文件
  const sourceFile = selectedFile.value
  const defaultMdPath = sourceFile.replace(/\.[^.]+$/, '.md')

  const savePath = await save({
    filters: [{ name: 'Markdown', extensions: ['md'] }],
    defaultPath: defaultMdPath,
    title: '保存 Markdown 文件',
  })

  if (!savePath) return // 用户取消

  clearResult()
  isLoading.value = true

  simulateProgress([
    { percent: 20, text: '正在读取文件...' },
    { percent: 50, text: '正在解析内容...' },
    { percent: 80, text: '正在生成 Markdown...' },
    { percent: 95, text: '正在保存文件...' },
  ])

  try {
    const result = await sidecarStore.fileToMarkdown(selectedFile.value)

    if (result.success) {
      resultMarkdown.value = result.markdown
      resultTitle.value = result.title || getFileName(selectedFile.value)

      // 保存到用户选择的路径（使用 Rust 后端绕过前端 fs 权限限制）
      await invoke('save_text_file', { path: savePath, content: result.markdown })
      resultFilePath.value = savePath
      showSuccessToast(`已保存到 ${getFileName(savePath)}`)
    } else {
      error.value = '转换失败'
    }
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    isLoading.value = false
  }
}

// 网页转 Markdown（默认保存图片、包含链接、60秒超时）
async function handleUrlToMarkdown(): Promise<void> {
  if (!urlInput.value.trim() || isLoading.value) return

  clearResult()
  isLoading.value = true

  simulateProgress([
    { percent: 15, text: '正在连接...' },
    { percent: 35, text: '正在加载页面...' },
    { percent: 60, text: '正在解析内容...' },
    { percent: 85, text: '正在生成 Markdown...' },
  ])

  try {
    const result = await sidecarStore.urlToMarkdown(urlInput.value, {
      engine: urlEngine.value,
      save_images: urlEngine.value === 'browser',  // 仅浏览器模式保存图片
    })

    if (result.success) {
      resultMarkdown.value = result.markdown
      resultTitle.value = result.title
      showSuccessToast('抓取成功')
    } else {
      error.value = '抓取失败'
    }
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    isLoading.value = false
  }
}

// Markdown 转文件
async function handleMarkdownToFile(): Promise<void> {
  if (!markdownInput.value.trim() || isLoading.value) return

  // 选择保存路径
  const savePath = await save({
    filters: [
      { name: outputFormat.value.toUpperCase(), extensions: [outputFormat.value] },
    ],
    defaultPath: `document.${outputFormat.value}`,
    title: '保存文件',
  })

  if (!savePath) return

  clearResult()
  isLoading.value = true

  simulateProgress([
    { percent: 20, text: '正在解析 Markdown...' },
    { percent: 50, text: '正在转换格式...' },
    { percent: 80, text: '正在生成文件...' },
  ])

  try {
    const result = await sidecarStore.markdownToFile(
      markdownInput.value,
      savePath,
      outputFormat.value
    )

    if (result.success) {
      resultFilePath.value = result.output_path
      showSuccessToast('文件已生成')
    } else {
      error.value = '生成失败'
    }
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    isLoading.value = false
  }
}

// Markdown 转网页 (HTML)
async function handleMarkdownToHtml(): Promise<void> {
  if (!markdownForHtml.value.trim() || isLoading.value) return

  // 选择保存路径
  const savePath = await save({
    filters: [{ name: 'HTML', extensions: ['html'] }],
    defaultPath: 'document.html',
    title: '保存 HTML 文件',
  })

  if (!savePath) return

  clearResult()
  isLoading.value = true

  simulateProgress([
    { percent: 30, text: '正在解析 Markdown...' },
    { percent: 70, text: '正在生成 HTML...' },
    { percent: 90, text: '正在保存文件...' },
  ])

  try {
    // 渲染 Markdown 为 HTML
    const htmlContent = parseMarkdown(markdownForHtml.value)

    // 生成完整的 HTML 页面（GitHub 风格）
    const fullHtml = generateHtmlPage(htmlContent, 'github')

    // 保存到文件
    await invoke('save_text_file', { path: savePath, content: fullHtml })
    resultFilePath.value = savePath
    showSuccessToast('HTML 页面已生成')
  } catch (e) {
    error.value = e instanceof Error ? e.message : String(e)
  } finally {
    isLoading.value = false
  }
}

/** 生成完整的 HTML 页面 */
function generateHtmlPage(content: string, theme: 'github' | 'notion' | 'minimal'): string {
  const themeStyles: Record<string, string> = {
    github: `
      body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; max-width: 800px; margin: 0 auto; padding: 40px 20px; line-height: 1.6; color: #24292e; }
      h1, h2, h3 { border-bottom: 1px solid #eaecef; padding-bottom: 0.3em; }
      code { background: #f6f8fa; padding: 0.2em 0.4em; border-radius: 3px; font-size: 85%; }
      pre { background: #f6f8fa; padding: 16px; border-radius: 6px; overflow-x: auto; }
      blockquote { border-left: 4px solid #dfe2e5; padding-left: 16px; color: #6a737d; margin: 16px 0; }
      a { color: #0366d6; text-decoration: none; }
      a:hover { text-decoration: underline; }
      table { border-collapse: collapse; width: 100%; }
      th, td { border: 1px solid #dfe2e5; padding: 8px 12px; }
      th { background: #f6f8fa; }
    `,
    notion: `
      body { font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif; max-width: 720px; margin: 0 auto; padding: 60px 20px; line-height: 1.7; color: #37352f; }
      h1 { font-size: 2.5em; font-weight: 700; margin-top: 1.5em; }
      h2 { font-size: 1.875em; font-weight: 600; margin-top: 1.4em; }
      h3 { font-size: 1.5em; font-weight: 600; margin-top: 1.3em; }
      code { background: rgba(135, 131, 120, 0.15); padding: 0.2em 0.4em; border-radius: 3px; font-size: 85%; color: #eb5757; }
      pre { background: #f7f6f3; padding: 20px; border-radius: 4px; overflow-x: auto; }
      blockquote { border-left: 3px solid #000; padding-left: 20px; color: #6b6b6b; font-style: italic; }
      a { color: #37352f; text-decoration: underline; }
    `,
    minimal: `
      body { font-family: Georgia, 'Times New Roman', serif; max-width: 650px; margin: 0 auto; padding: 80px 20px; line-height: 1.8; color: #333; }
      h1, h2, h3 { font-weight: normal; margin-top: 2em; }
      code { font-family: 'SF Mono', Monaco, monospace; font-size: 0.9em; }
      pre { background: #fafafa; padding: 20px; border: 1px solid #eee; overflow-x: auto; }
      blockquote { border-left: 2px solid #ccc; padding-left: 20px; color: #666; margin: 24px 0; }
      a { color: #333; }
    `,
  }

  return `<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>Markdown 文档</title>
  <style>${themeStyles[theme]}</style>
</head>
<body>
${content}
</body>
</html>`
}

// 复制 Markdown
async function handleCopyMarkdown(): Promise<void> {
  if (!resultMarkdown.value) return

  try {
    await writeText(resultMarkdown.value)
    showSuccessToast('已复制到剪贴板')
  } catch (e) {
    console.error('Failed to copy:', e)
    showErrorToast('复制失败，请重试')
  }
}

// 保存 Markdown
async function handleSaveMarkdown(): Promise<void> {
  if (!resultMarkdown.value) return

  const savePath = await save({
    filters: [{ name: 'Markdown', extensions: ['md'] }],
    defaultPath: `${resultTitle.value || 'document'}.md`,
    title: '保存 Markdown 文件',
  })

  if (!savePath) return

  try {
    // 使用 Rust 后端保存文件，绕过前端 fs 权限限制
    await invoke('save_text_file', { path: savePath, content: resultMarkdown.value })
    showSuccessToast('文件已保存')
  } catch (e) {
    console.error('Failed to save:', e)
    const errorMsg = e instanceof Error ? e.message : String(e)
    showErrorToast(`保存失败: ${errorMsg}`)
  }
}

// 打开文件
async function handleOpenFile(): Promise<void> {
  if (!resultFilePath.value) return

  try {
    await openPath(resultFilePath.value)
  } catch (e) {
    console.error('Failed to open file:', e)
    showErrorToast('打开文件失败')
  }
}

// 打开文件夹
async function handleOpenFolder(): Promise<void> {
  if (!resultFilePath.value) return

  try {
    await revealItemInDir(resultFilePath.value)
  } catch (e) {
    console.error('Failed to open folder:', e)
    showErrorToast('打开文件夹失败')
  }
}

// 关闭对话框
function handleClose(): void {
  emit('close')
}

// ============================================
// Settings Handlers
// ============================================

function toggleFileSettings(): void {
  showFileSettings.value = !showFileSettings.value
}

// File to Markdown settings
function handleEngineChange(engine: FileToMarkdownEngine): void {
  settingsStore.updateFileToMarkdown({ engine })
}

function handleApiTokenChange(event: Event): void {
  const target = event.target as HTMLInputElement
  settingsStore.updateFileToMarkdown({ apiToken: target.value })
}
</script>

<style scoped>
.converter-dialog {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 16px;
  background: rgba(30, 30, 30, 0.98);
  border-radius: 8px;
  border: 1px solid rgba(255, 255, 255, 0.1);
  min-width: 480px;
  max-width: 600px;
  max-height: 85vh;
  overflow: hidden;
  position: relative;
}

/* 头部 */
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

/* 双向转换卡片布局 */
.conversion-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 12px;
}

.conversion-group {
  padding: 12px;
  background: rgba(0, 0, 0, 0.2);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 8px;
}

.group-header {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 10px;
}

.group-icon {
  font-size: 16px;
}

.group-title {
  color: rgba(255, 255, 255, 0.7);
  font-size: 12px;
  font-weight: 500;
}

.group-buttons {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.direction-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 10px 12px;
  background: rgba(255, 255, 255, 0.03);
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 6px;
  color: rgba(255, 255, 255, 0.6);
  font-size: 13px;
  cursor: pointer;
  transition: all 0.15s;
}

.direction-btn:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.06);
  border-color: rgba(255, 255, 255, 0.15);
  color: rgba(255, 255, 255, 0.8);
}

.direction-btn.is-active {
  background: rgba(66, 133, 244, 0.15);
  border-color: rgba(66, 133, 244, 0.4);
  color: #4285f4;
}

.direction-btn.is-active .direction-arrow {
  color: #4285f4;
}

.direction-from,
.direction-to {
  font-weight: 500;
}

.direction-arrow {
  color: rgba(255, 255, 255, 0.3);
  font-size: 12px;
}

/* 内容区域 */
.tab-content {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

/* 拖拽区域 */
.drop-zone {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 32px 24px;
  background: rgba(0, 0, 0, 0.2);
  border: 2px dashed rgba(255, 255, 255, 0.15);
  border-radius: 8px;
  cursor: pointer;
  transition: all 0.2s;
}

.drop-zone:hover {
  border-color: rgba(66, 133, 244, 0.4);
  background: rgba(66, 133, 244, 0.05);
}

.drop-zone.is-dragging {
  border-color: rgba(66, 133, 244, 0.6);
  background: rgba(66, 133, 244, 0.1);
  transform: scale(1.01);
}

.drop-zone.has-file {
  flex-direction: row;
  padding: 16px 20px;
  border-style: solid;
  border-color: rgba(66, 133, 244, 0.3);
  background: rgba(66, 133, 244, 0.05);
}

.drop-icon {
  font-size: 32px;
  opacity: 0.6;
}

.drop-text {
  color: rgba(255, 255, 255, 0.7);
  font-size: 14px;
}

.drop-hint {
  color: rgba(255, 255, 255, 0.4);
  font-size: 12px;
}

.file-icon {
  font-size: 24px;
}

.file-name {
  flex: 1;
  color: #fff;
  font-size: 14px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.clear-file-btn {
  width: 24px;
  height: 24px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(255, 255, 255, 0.1);
  border: none;
  border-radius: 4px;
  color: rgba(255, 255, 255, 0.6);
  font-size: 12px;
  cursor: pointer;
  transition: all 0.15s;
}

.clear-file-btn:hover {
  background: rgba(244, 67, 54, 0.3);
  color: #ff6b6b;
}

/* 表单 */
.form-group {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.form-label {
  color: rgba(255, 255, 255, 0.7);
  font-size: 12px;
  font-weight: 500;
}

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

.markdown-textarea {
  width: 100%;
  padding: 12px;
  background: rgba(0, 0, 0, 0.3);
  border: 1px solid rgba(255, 255, 255, 0.15);
  border-radius: 4px;
  color: #fff;
  font-size: 13px;
  font-family: 'Consolas', 'Monaco', monospace;
  line-height: 1.5;
  outline: none;
  resize: vertical;
  min-height: 120px;
  transition: border-color 0.15s;
}

.markdown-textarea:hover {
  border-color: rgba(255, 255, 255, 0.25);
}

.markdown-textarea:focus {
  border-color: rgba(66, 133, 244, 0.6);
}

/* 选项 */
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

.checkbox-label input[type='checkbox'] {
  width: 14px;
  height: 14px;
  cursor: pointer;
}

.checkbox-text {
  color: rgba(255, 255, 255, 0.7);
  font-size: 12px;
}

/* 格式选择 */
.format-options {
  display: flex;
  gap: 8px;
}

.format-btn {
  flex: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  padding: 12px 16px;
  background: rgba(0, 0, 0, 0.2);
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: 6px;
  color: rgba(255, 255, 255, 0.7);
  cursor: pointer;
  transition: all 0.15s;
}

.format-btn:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.05);
  border-color: rgba(255, 255, 255, 0.2);
}

.format-btn.is-active {
  background: rgba(66, 133, 244, 0.2);
  border-color: rgba(66, 133, 244, 0.4);
  color: #4285f4;
}

.format-icon {
  font-size: 20px;
}

.format-label {
  font-size: 12px;
}

/* 转换按钮 */
.convert-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 12px 20px;
  background: rgba(66, 133, 244, 0.8);
  border: none;
  border-radius: 6px;
  color: #fff;
  font-size: 14px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.15s;
}

.convert-btn:hover:not(:disabled) {
  background: rgba(66, 133, 244, 1);
}

.convert-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.btn-icon {
  font-size: 14px;
}

/* 进度 */
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
.result-section {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.result-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.result-title {
  color: #fff;
  font-size: 13px;
  font-weight: 500;
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

.copy-btn,
.save-btn {
  background: rgba(255, 255, 255, 0.1);
  color: rgba(255, 255, 255, 0.8);
}

.copy-btn:hover,
.save-btn:hover {
  background: rgba(255, 255, 255, 0.15);
  color: #fff;
}

.open-btn {
  background: rgba(66, 133, 244, 0.8);
  color: #fff;
}

.open-btn:hover {
  background: rgba(66, 133, 244, 1);
}

.folder-btn {
  background: rgba(255, 255, 255, 0.1);
  color: rgba(255, 255, 255, 0.8);
}

.folder-btn:hover {
  background: rgba(255, 255, 255, 0.15);
  color: #fff;
}

/* Markdown 预览 */
.markdown-preview {
  max-height: 200px;
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
  margin: 0;
  color: rgba(255, 255, 255, 0.9);
  font-size: 12px;
  font-family: 'Consolas', 'Monaco', monospace;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-word;
}

/* 输出路径 */
.output-path-wrapper {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px 12px;
  background: rgba(0, 0, 0, 0.2);
  border-radius: 4px;
}

.output-path {
  color: rgba(255, 255, 255, 0.9);
  font-size: 12px;
  word-break: break-all;
  line-height: 1.4;
}

.output-actions {
  display: flex;
  gap: 8px;
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

/* 错误提示 */
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
  to {
    transform: rotate(360deg);
  }
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

/* 设置区域 */
.settings-section {
  margin-top: 8px;
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: 6px;
  overflow: hidden;
}

.settings-header {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 12px;
  background: rgba(0, 0, 0, 0.2);
  cursor: pointer;
  user-select: none;
  transition: background 0.15s;
}

.settings-header:hover {
  background: rgba(0, 0, 0, 0.3);
}

.settings-icon {
  font-size: 14px;
}

.settings-title {
  flex: 1;
  color: rgba(255, 255, 255, 0.7);
  font-size: 12px;
}

.settings-arrow {
  color: rgba(255, 255, 255, 0.4);
  font-size: 10px;
  transition: transform 0.2s;
}

.settings-arrow.expanded {
  transform: rotate(180deg);
}

.settings-content {
  padding: 12px;
  background: rgba(0, 0, 0, 0.1);
  border-top: 1px solid rgba(255, 255, 255, 0.05);
}

.setting-row {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 8px 0;
}

.setting-row:not(:last-child) {
  border-bottom: 1px solid rgba(255, 255, 255, 0.05);
}

.setting-label {
  color: rgba(255, 255, 255, 0.7);
  font-size: 12px;
}

/* Toggle Switch */
.toggle-switch {
  position: relative;
  width: 36px;
  height: 20px;
  cursor: pointer;
}

.toggle-switch input {
  opacity: 0;
  width: 0;
  height: 0;
}

.toggle-slider {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: rgba(255, 255, 255, 0.2);
  border-radius: 10px;
  transition: background 0.2s;
}

.toggle-slider::before {
  content: '';
  position: absolute;
  width: 16px;
  height: 16px;
  left: 2px;
  bottom: 2px;
  background: white;
  border-radius: 50%;
  transition: transform 0.2s;
}

.toggle-switch input:checked + .toggle-slider {
  background: rgba(66, 133, 244, 0.8);
}

.toggle-switch input:checked + .toggle-slider::before {
  transform: translateX(16px);
}

/* Slider */
.slider-group {
  display: flex;
  align-items: center;
  gap: 8px;
}

.setting-slider {
  width: 100px;
  height: 4px;
  border-radius: 2px;
  background: rgba(255, 255, 255, 0.2);
  appearance: none;
  cursor: pointer;
}

.setting-slider::-webkit-slider-thumb {
  appearance: none;
  width: 14px;
  height: 14px;
  border-radius: 50%;
  background: #4285f4;
  cursor: pointer;
}

.slider-value {
  min-width: 32px;
  color: rgba(255, 255, 255, 0.6);
  font-size: 11px;
  text-align: right;
}

/* Token Input */
.token-input {
  width: 140px;
  padding: 4px 8px;
  font-size: 12px;
  color: rgba(255, 255, 255, 0.9);
  background: rgba(0, 0, 0, 0.3);
  border: 1px solid rgba(255, 255, 255, 0.15);
  border-radius: 4px;
  outline: none;
}

.token-input:focus {
  border-color: rgba(66, 133, 244, 0.6);
}

.token-link {
  color: #4285f4;
  font-size: 12px;
  text-decoration: none;
}

.token-link:hover {
  text-decoration: underline;
}

/* Engine Options */
.engine-options {
  display: flex;
  gap: 6px;
}

.engine-btn {
  padding: 5px 10px;
  font-size: 11px;
  background: rgba(255, 255, 255, 0.05);
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: 4px;
  color: rgba(255, 255, 255, 0.6);
  cursor: pointer;
  transition: all 0.15s;
}

.engine-btn:hover {
  background: rgba(255, 255, 255, 0.08);
  border-color: rgba(255, 255, 255, 0.2);
  color: rgba(255, 255, 255, 0.8);
}

.engine-btn.is-active {
  background: rgba(66, 133, 244, 0.2);
  border-color: rgba(66, 133, 244, 0.4);
  color: #4285f4;
}

.engine-hint {
  font-size: 11px;
  color: rgba(255, 255, 255, 0.4);
  margin-top: 2px;
}

/* Config Info */
.config-info {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  margin-bottom: 4px;
  background: rgba(66, 133, 244, 0.08);
  border: 1px solid rgba(66, 133, 244, 0.2);
  border-radius: 4px;
}

.info-icon {
  font-size: 14px;
  flex-shrink: 0;
}

.info-text {
  font-size: 11px;
  color: rgba(130, 177, 255, 0.9);
  line-height: 1.3;
}

/* Config Prompt */
.config-prompt {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
  margin-bottom: 8px;
  background: rgba(255, 193, 7, 0.1);
  border: 1px solid rgba(255, 193, 7, 0.3);
  border-radius: 4px;
}

.prompt-icon {
  font-size: 14px;
  flex-shrink: 0;
}

.prompt-text {
  font-size: 11px;
  color: rgba(255, 193, 7, 0.9);
  line-height: 1.3;
}

/* HTML 预览 */
.html-preview-wrapper {
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: 6px;
  overflow: hidden;
}

.preview-header {
  display: flex;
  align-items: center;
  padding: 8px 12px;
  background: rgba(0, 0, 0, 0.2);
  border-bottom: 1px solid rgba(255, 255, 255, 0.05);
}

.preview-title {
  color: rgba(255, 255, 255, 0.7);
  font-size: 12px;
}

.html-preview {
  max-height: 180px;
  padding: 16px;
  background: #fff;
  color: #333;
  font-size: 13px;
  line-height: 1.6;
  overflow-y: auto;
}

.html-preview::-webkit-scrollbar {
  width: 6px;
}

.html-preview::-webkit-scrollbar-track {
  background: rgba(0, 0, 0, 0.05);
}

.html-preview::-webkit-scrollbar-thumb {
  background: rgba(0, 0, 0, 0.2);
  border-radius: 3px;
}

.html-preview h1,
.html-preview h2,
.html-preview h3 {
  margin-top: 0.5em;
  margin-bottom: 0.5em;
  color: #24292e;
}

.html-preview p {
  margin: 0.5em 0;
}

.html-preview code {
  background: #f6f8fa;
  padding: 0.2em 0.4em;
  border-radius: 3px;
  font-size: 85%;
}

.html-preview pre {
  background: #f6f8fa;
  padding: 12px;
  border-radius: 4px;
  overflow-x: auto;
}

.html-preview blockquote {
  border-left: 4px solid #dfe2e5;
  padding-left: 16px;
  color: #6a737d;
  margin: 8px 0;
}

.html-preview ul,
.html-preview ol {
  padding-left: 24px;
  margin: 0.5em 0;
}

.html-preview a {
  color: #0366d6;
}
</style>
