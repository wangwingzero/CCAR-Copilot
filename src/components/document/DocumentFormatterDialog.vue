<template>
  <div class="document-formatter-dialog" :class="{ 'is-loading': isLoading }">
    <!-- 对话框头部 -->
    <div class="dialog-header">
      <span class="dialog-title">📄 公文格式化</span>
      <button class="close-btn" @click="handleClose" :disabled="isLoading">
        ✕
      </button>
    </div>

    <!-- 已打开的文档列表 -->
    <div class="open-docs-section">
      <div class="section-header">
        <span class="section-title">当前打开的文档</span>
        <button
          class="refresh-btn"
          :disabled="isLoadingDocs || isLoading"
          @click="refreshOpenDocs"
          title="刷新列表"
        >
          <span :class="{ 'is-spinning': isLoadingDocs }">🔄</span>
        </button>
      </div>

      <!-- 加载状态 -->
      <div v-if="isLoadingDocs" class="docs-loading">
        <span class="loading-spinner-small" />
        <span>检测中...</span>
      </div>

      <!-- 文档列表 -->
      <div v-else-if="openDocs.length > 0" class="docs-list">
        <div
          v-for="doc in openDocs"
          :key="doc.name"
          class="doc-item"
          :class="{ 'is-selected': selectedDocName === doc.name }"
          @click="selectDoc(doc)"
          @dblclick="handleFormatByName(doc.name)"
        >
          <span class="doc-icon">{{ doc.app_type === 'word' ? '📄' : '📝' }}</span>
          <span class="doc-name">{{ doc.name }}</span>
          <span class="doc-app">{{ doc.app_type === 'word' ? 'Word' : 'WPS' }}</span>
        </div>
      </div>

      <!-- 空状态 -->
      <div v-else class="docs-empty">
        <span class="empty-icon">{{ docsError ? '⚠️' : '📭' }}</span>
        <span class="empty-text" :class="{ 'error-text': !!docsError }">{{ docsError || '未检测到打开的 Word/WPS 文档' }}</span>
        <span v-if="!docsError" class="empty-hint">请先在 Word 或 WPS 中打开要格式化的文档</span>
        <span v-else-if="docsError.includes('权限')" class="empty-hint error-hint">提示：右键点击虎哥截图图标，选择「以管理员身份运行」</span>
      </div>

      <!-- 格式化按钮 (按名称) -->
      <button
        v-if="openDocs.length > 0"
        class="format-btn"
        :class="{ 'is-loading': isLoading }"
        :disabled="!selectedDocName || isLoading"
        @click="handleFormatByName(selectedDocName!)"
      >
        <span v-if="isLoading" class="loading-spinner-small" />
        <span v-else class="btn-icon">⚙️</span>
        <span class="btn-text">{{ isLoading ? '格式化中...' : '开始格式化' }}</span>
      </button>
    </div>

    <!-- 分隔线 -->
    <div class="divider">
      <span class="divider-text">或选择文件</span>
    </div>

    <!-- 文件选择区域 -->
    <div class="file-select-section">
      <div class="form-group">
        <div class="file-input-wrapper">
          <input
            ref="filePathInputRef"
            v-model="filePath"
            type="text"
            class="form-input file-input"
            placeholder="点击选择 Word 文件..."
            :disabled="isLoading"
            readonly
            @click="handleSelectFile"
          />
          <button
            class="browse-btn"
            :disabled="isLoading"
            @click="handleSelectFile"
            title="浏览文件"
          >
            <span class="btn-icon">📁</span>
            <span class="btn-text">浏览</span>
          </button>
        </div>
        <span v-if="fileError" class="file-error">{{ fileError }}</span>
      </div>

      <!-- 格式化按钮 (按路径) -->
      <button
        v-if="filePath"
        class="format-btn format-btn-secondary"
        :class="{ 'is-loading': isLoading }"
        :disabled="!canFormat"
        @click="handleFormat"
      >
        <span v-if="isLoading" class="loading-spinner-small" />
        <span v-else class="btn-icon">⚙️</span>
        <span class="btn-text">{{ isLoading ? '格式化中...' : '格式化选中文件' }}</span>
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
      <!-- 结果头部 -->
      <div class="result-header">
        <span class="result-status" :class="{ 'is-compliant': isCompliant }">
          {{ isCompliant ? '✅ 格式化完成，完全符合标准' : '⚠️ 格式化完成，存在以下问题' }}
        </span>
      </div>

      <!-- 问题列表 -->
      <div v-if="issues.length > 0" class="issues-section">
        <div class="issues-header">
          <span class="issues-title">📋 修复的问题 ({{ issues.length }})</span>
        </div>
        <ul class="issues-list">
          <li v-for="(issue, index) in issues" :key="index" class="issue-item">
            <span class="issue-icon">•</span>
            <span class="issue-text">{{ issue }}</span>
          </li>
        </ul>
      </div>

      <!-- 输出文件信息 -->
      <div class="output-section">
        <div class="output-header">
          <span class="output-title">📁 输出文件</span>
        </div>
        <div class="output-path-wrapper">
          <span class="output-path">{{ outputPath }}</span>
          <div class="output-actions">
            <button class="action-btn open-btn" @click="handleOpenFile" title="打开文件">
              📂 打开
            </button>
            <button class="action-btn folder-btn" @click="handleOpenFolder" title="打开所在文件夹">
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
        ✓ 文档格式化成功
      </div>
    </Transition>
  </div>
</template>

<script setup lang="ts">
/**
 * 公文格式化对话框组件
 *
 * 功能：
 * - 自动检测当前打开的 Word/WPS 文档
 * - 默认选中第一个文档，直接点击即可格式化
 * - 支持文件选择对话框
 * - 显示格式化进度和结果
 *
 * @validates Requirements 12.1, 12.2
 */

import { ref, computed, watch, onMounted, onBeforeUnmount } from 'vue'
import { open } from '@tauri-apps/plugin-dialog'
import { openPath, revealItemInDir } from '@tauri-apps/plugin-opener'
import { useDocumentFormatter } from '@/composables/useDocumentFormatter'
import { useSidecarStore } from '@/stores/sidecar'

// ============================================
// Props & Emits
// ============================================

interface Props {
  /** 初始文件路径 */
  initialPath?: string
  /** 是否显示对话框 */
  visible?: boolean
}

const props = withDefaults(defineProps<Props>(), {
  initialPath: '',
  visible: true,
})

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'success', result: { outputPath: string; issues: string[]; compliant: boolean }): void
  (e: 'error', message: string): void
}>()

// ============================================
// Composables & Stores
// ============================================

const sidecarStore = useSidecarStore()

const {
  isLoading,
  error: formatError,
  hasResult: hasFileResult,
  isCompliant: isFileCompliant,
  format,
  clearResult,
  validateFilePath,
  getOutputPath,
  getIssues,
} = useDocumentFormatter()

// ============================================
// State
// ============================================

/** 文件路径输入 */
const filePath = ref('')

/** 文件验证错误 */
const fileError = ref<string | null>(null)

/** 是否显示成功提示 */
const showSuccess = ref(false)

/** 进度文本 */
const progressText = ref('正在准备...')

/** 进度百分比 */
const progressPercent = ref(0)

/** 文件路径输入框引用 */
const filePathInputRef = ref<HTMLInputElement | null>(null)

/** 活跃的定时器 ID（用于组件卸载时清理） */
let activeProgressInterval: ReturnType<typeof setInterval> | null = null
let activeTimeouts: ReturnType<typeof setTimeout>[] = []

/** 打开的文档列表 */
interface OpenDoc {
  name: string
  full_path: string
  app_type: string
}
const openDocs = ref<OpenDoc[]>([])

/** 是否正在加载文档列表 */
const isLoadingDocs = ref(false)

/** 文档列表加载错误 */
const docsError = ref<string | null>(null)

/** 选中的文档名称 */
const selectedDocName = ref<string | null>(null)

/** 按名称格式化的结果 */
const formatByNameResult = ref<{
  success: boolean
  document_name: string
  message: string
  issues: string[]
} | null>(null)

// ============================================
// Computed
// ============================================

/** 是否可以格式化 */
const canFormat = computed(() =>
  !isLoading.value &&
  filePath.value.trim().length > 0 &&
  !fileError.value
)

/** 是否有结果 (任一模式) */
const hasResult = computed(() =>
  hasFileResult.value || (formatByNameResult.value?.success ?? false)
)

/** 是否完全符合标准 (任一模式) */
const isCompliant = computed(() => {
  if (formatByNameResult.value?.success) {
    return formatByNameResult.value.issues.length === 0
  }
  return isFileCompliant.value
})

/** 错误信息 */
const error = computed(() => formatError.value)

/** 输出路径 */
const outputPath = computed(() => {
  // 优先返回按名称格式化的结果
  if (formatByNameResult.value?.success) {
    return formatByNameResult.value.document_name
  }
  return getOutputPath()
})

/** 问题列表 */
const issues = computed(() => {
  // 优先返回按名称格式化的结果
  if (formatByNameResult.value?.success) {
    return formatByNameResult.value.issues
  }
  return getIssues()
})

/** 进度条宽度 */
const progressWidth = computed(() => `${progressPercent.value}%`)

// ============================================
// Methods
// ============================================

/**
 * 刷新打开的文档列表
 */
async function refreshOpenDocs(): Promise<void> {
  if (isLoadingDocs.value) return

  try {
    isLoadingDocs.value = true
    docsError.value = null

    // 使用纯 Rust 实现（不依赖 Sidecar，通过 Win32 API 窗口枚举检测文档）
    // 这种方法不受管理员/普通用户权限隔离影响
    console.log('[DocumentFormatter] Calling get_open_documents_native (Rust)...')
    const { invoke } = await import('@tauri-apps/api/core')
    const nativeResult = await invoke<{
      success: boolean
      documents: Array<{ name: string; fullPath: string; appType: string }>
      available: boolean
      error?: string
    }>('get_open_documents_native')

    console.log('[DocumentFormatter] Native result:', nativeResult)

    if (nativeResult.success) {
      // 转换 camelCase 为前端格式
      openDocs.value = nativeResult.documents.map(d => ({
        name: d.name,
        full_path: d.fullPath || '',
        app_type: d.appType,
      }))
      // 默认选中第一个文档
      if (openDocs.value.length > 0 && !selectedDocName.value) {
        selectedDocName.value = openDocs.value[0].name
      }
    } else {
      docsError.value = nativeResult.error || '获取文档列表失败'
      openDocs.value = []
    }
  } catch (e) {
    console.error('[DocumentFormatter] Failed to get open documents:', e)
    // 显示更详细的错误信息
    const errorMsg = e instanceof Error ? e.message : String(e)
    docsError.value = errorMsg || '获取文档列表失败'
    openDocs.value = []
  } finally {
    isLoadingDocs.value = false
  }
}

/**
 * 选择文档
 */
function selectDoc(doc: OpenDoc): void {
  selectedDocName.value = doc.name
}

/**
 * 按文档名称格式化
 *
 * 如果文档有完整路径，通过 COM 格式化（传统方式）
 * 如果没有完整路径（来自原生窗口检测），则弹出文件对话框让用户选择文件，
 * 然后使用 python-docx 进行文件级格式化（不依赖 COM，不受权限影响）
 */
async function handleFormatByName(docName: string): Promise<void> {
  if (!docName || isLoading.value) return

  // 查找对应的文档对象
  const doc = openDocs.value.find(d => d.name === docName)

  // 如果文档没有完整路径（来自原生窗口检测），使用文件级格式化
  if (!doc?.full_path) {
    console.log('[DocumentFormatter] 文档无完整路径，打开文件对话框让用户选择')
    try {
      const selected = await open({
        multiple: false,
        directory: false,
        defaultPath: undefined,
        filters: [
          {
            name: 'Word 文档',
            extensions: ['docx', 'doc']
          }
        ],
        title: `请选择文档: ${docName}`
      })

      if (selected && typeof selected === 'string') {
        // 用户选择了文件，使用文件级格式化
        filePath.value = selected
        validateFile()
        // 自动触发格式化
        await handleFormat()
        return
      } else {
        // 用户取消了选择
        return
      }
    } catch (e) {
      console.error('Failed to open file dialog:', e)
      return
    }
  }

  // 有完整路径的情况，使用 COM 格式化（传统方式）
  clearResult()
  formatByNameResult.value = null
  simulateProgress()

  try {
    const result = await sidecarStore.formatDocumentByName(docName)

    progressPercent.value = 100
    progressText.value = '完成'

    if (result.success) {
      formatByNameResult.value = result

      // 显示成功提示
      showSuccess.value = true
      const tid = setTimeout(() => {
        showSuccess.value = false
      }, 2000)
      activeTimeouts.push(tid)

      emit('success', {
        outputPath: result.document_name,
        issues: result.issues,
        compliant: result.issues.length === 0,
      })
    } else {
      emit('error', '格式化失败')
    }
  } catch (e) {
    const errorMessage = e instanceof Error ? e.message : String(e)
    emit('error', errorMessage)
  }
}

/**
 * 处理文件选择
 */
async function handleSelectFile(): Promise<void> {
  if (isLoading.value) return
  
  try {
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [
        {
          name: 'Word 文档',
          extensions: ['docx', 'doc']
        }
      ],
      title: '选择要格式化的 Word 文档'
    })
    
    if (selected && typeof selected === 'string') {
      filePath.value = selected
      validateFile()
    }
  } catch (e) {
    console.error('Failed to open file dialog:', e)
  }
}

/**
 * 验证文件
 */
function validateFile(): void {
  const path = filePath.value.trim()
  
  if (!path) {
    fileError.value = null
    return
  }
  
  if (!validateFilePath(path)) {
    fileError.value = '请选择有效的 Word 文档（.doc 或 .docx 格式）'
  } else {
    fileError.value = null
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
  progressText.value = '正在准备...'
  
  const stages = [
    { percent: 15, text: '正在读取文档...' },
    { percent: 35, text: '正在分析文档结构...' },
    { percent: 55, text: '正在调整页边距...' },
    { percent: 70, text: '正在设置字体格式...' },
    { percent: 85, text: '正在调整段落间距...' },
    { percent: 95, text: '正在保存文档...' },
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
  }, 600)
}

/**
 * 执行格式化
 */
async function handleFormat(): Promise<void> {
  if (!canFormat.value) return
  
  // 清除之前的结果
  clearResult()
  
  // 开始模拟进度
  simulateProgress()
  
  // 执行格式化
  const result = await format(filePath.value)
  
  if (result) {
    progressPercent.value = 100
    progressText.value = '完成'
    
    // 显示成功提示
    showSuccess.value = true
    const tid = setTimeout(() => {
      showSuccess.value = false
    }, 2000)
    activeTimeouts.push(tid)
    
    emit('success', {
      outputPath: result.outputPath,
      issues: result.issues,
      compliant: result.compliant,
    })
  } else {
    emit('error', error.value || '格式化失败')
  }
}

/**
 * 打开输出文件
 */
async function handleOpenFile(): Promise<void> {
  const path = getOutputPath()
  if (!path) return
  
  try {
    await openPath(path)
  } catch (e) {
    console.error('Failed to open file:', e)
    emit('error', '打开文件失败')
  }
}

/**
 * 打开输出文件所在文件夹
 */
async function handleOpenFolder(): Promise<void> {
  const path = getOutputPath()
  if (!path) return
  
  try {
    // 使用 revealItemInDir 在资源管理器中定位文件
    await revealItemInDir(path)
  } catch (e) {
    console.error('Failed to open folder:', e)
    emit('error', '打开文件夹失败')
  }
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

// 监听初始路径
watch(() => props.initialPath, (newVal) => {
  if (newVal) {
    filePath.value = newVal
    validateFile()
  }
}, { immediate: true })

// 监听可见性变化
watch(() => props.visible, async (newVal) => {
  if (newVal) {
    // 刷新打开的文档列表
    await refreshOpenDocs()
  }
})

// ============================================
// Lifecycle
// ============================================

onMounted(async () => {
  if (props.visible) {
    // 自动加载打开的文档列表
    await refreshOpenDocs()
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
.document-formatter-dialog {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 16px;
  background: rgba(30, 30, 30, 0.98);
  border-radius: 8px;
  border: 1px solid rgba(255, 255, 255, 0.1);
  min-width: 450px;
  max-width: 550px;
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

/* 打开的文档区域 */
.open-docs-section {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.section-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.section-title {
  color: rgba(255, 255, 255, 0.7);
  font-size: 12px;
  font-weight: 500;
}

.refresh-btn {
  width: 24px;
  height: 24px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: none;
  border-radius: 4px;
  color: rgba(255, 255, 255, 0.6);
  font-size: 12px;
  cursor: pointer;
  transition: all 0.15s;
}

.refresh-btn:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.1);
  color: #fff;
}

.refresh-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.refresh-btn .is-spinning {
  animation: spin 0.8s linear infinite;
}

/* 文档列表 */
.docs-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  max-height: 150px;
  overflow-y: auto;
  padding: 4px;
  background: rgba(0, 0, 0, 0.2);
  border-radius: 4px;
}

.docs-list::-webkit-scrollbar {
  width: 6px;
}

.docs-list::-webkit-scrollbar-track {
  background: rgba(255, 255, 255, 0.05);
  border-radius: 3px;
}

.docs-list::-webkit-scrollbar-thumb {
  background: rgba(255, 255, 255, 0.2);
  border-radius: 3px;
}

.doc-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 12px;
  background: rgba(255, 255, 255, 0.05);
  border-radius: 4px;
  cursor: pointer;
  transition: all 0.15s;
}

.doc-item:hover {
  background: rgba(255, 255, 255, 0.1);
}

.doc-item.is-selected {
  background: rgba(66, 133, 244, 0.3);
  border: 1px solid rgba(66, 133, 244, 0.5);
}

.doc-icon {
  font-size: 16px;
}

.doc-name {
  flex: 1;
  color: #fff;
  font-size: 13px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.doc-app {
  color: rgba(255, 255, 255, 0.5);
  font-size: 11px;
  padding: 2px 6px;
  background: rgba(255, 255, 255, 0.1);
  border-radius: 3px;
}

/* 空状态 */
.docs-empty {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 24px 16px;
  background: rgba(0, 0, 0, 0.2);
  border-radius: 4px;
}

.empty-icon {
  font-size: 28px;
  opacity: 0.6;
}

.empty-text {
  color: rgba(255, 255, 255, 0.6);
  font-size: 13px;
  text-align: center;
}

.empty-hint {
  color: rgba(255, 255, 255, 0.4);
  font-size: 11px;
  text-align: center;
}

.error-text {
  color: rgba(255, 180, 60, 0.9);
  white-space: pre-line;
  line-height: 1.6;
}

.error-hint {
  color: rgba(100, 200, 255, 0.8);
  margin-top: 4px;
}

/* 加载状态 */
.docs-loading {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 20px;
  background: rgba(0, 0, 0, 0.2);
  border-radius: 4px;
  color: rgba(255, 255, 255, 0.6);
  font-size: 12px;
}

/* 分隔线 */
.divider {
  display: flex;
  align-items: center;
  gap: 12px;
  margin: 4px 0;
}

.divider::before,
.divider::after {
  content: '';
  flex: 1;
  height: 1px;
  background: rgba(255, 255, 255, 0.1);
}

.divider-text {
  color: rgba(255, 255, 255, 0.4);
  font-size: 11px;
}

/* 文件选择区域 */
.file-select-section {
  display: flex;
  flex-direction: column;
  gap: 12px;
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

.file-input-wrapper {
  display: flex;
  gap: 8px;
}

.form-input,
.file-input {
  flex: 1;
  padding: 10px 12px;
  background: rgba(0, 0, 0, 0.3);
  border: 1px solid rgba(255, 255, 255, 0.15);
  border-radius: 4px;
  color: #fff;
  font-size: 13px;
  outline: none;
  transition: border-color 0.15s;
  cursor: pointer;
}

.form-input:hover,
.file-input:hover {
  border-color: rgba(255, 255, 255, 0.25);
}

.form-input:focus,
.file-input:focus {
  border-color: rgba(66, 133, 244, 0.6);
}

.form-input:disabled,
.file-input:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.file-error {
  color: #ff6b6b;
  font-size: 11px;
  margin-top: 2px;
}

.file-hint {
  color: rgba(255, 255, 255, 0.5);
  font-size: 11px;
  margin-top: 2px;
}

.browse-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 10px 14px;
  background: rgba(255, 255, 255, 0.1);
  border: 1px solid rgba(255, 255, 255, 0.15);
  border-radius: 4px;
  color: rgba(255, 255, 255, 0.9);
  font-size: 13px;
  cursor: pointer;
  transition: all 0.15s;
  white-space: nowrap;
}

.browse-btn:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.15);
  border-color: rgba(255, 255, 255, 0.25);
}

.browse-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.btn-icon {
  font-size: 14px;
}

.format-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 12px 20px;
  background: rgba(66, 133, 244, 0.8);
  border: none;
  border-radius: 4px;
  color: #fff;
  font-size: 14px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.15s;
}

.format-btn:hover:not(:disabled) {
  background: rgba(66, 133, 244, 1);
}

.format-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.format-btn.is-loading {
  background: rgba(66, 133, 244, 0.6);
}

.format-btn-secondary {
  background: rgba(255, 255, 255, 0.1);
  border: 1px solid rgba(255, 255, 255, 0.15);
}

.format-btn-secondary:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.15);
  border-color: rgba(255, 255, 255, 0.25);
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
  align-items: center;
  padding: 10px 12px;
  background: rgba(0, 0, 0, 0.2);
  border-radius: 4px;
}

.result-status {
  color: #ffa726;
  font-size: 13px;
  font-weight: 500;
}

.result-status.is-compliant {
  color: #66bb6a;
}

/* 问题列表 */
.issues-section {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.issues-header {
  display: flex;
  align-items: center;
}

.issues-title {
  color: rgba(255, 255, 255, 0.7);
  font-size: 12px;
  font-weight: 500;
}

.issues-list {
  list-style: none;
  margin: 0;
  padding: 0;
  max-height: 150px;
  overflow-y: auto;
  background: rgba(0, 0, 0, 0.2);
  border-radius: 4px;
  padding: 8px 12px;
}

.issues-list::-webkit-scrollbar {
  width: 6px;
}

.issues-list::-webkit-scrollbar-track {
  background: rgba(255, 255, 255, 0.05);
  border-radius: 3px;
}

.issues-list::-webkit-scrollbar-thumb {
  background: rgba(255, 255, 255, 0.2);
  border-radius: 3px;
}

.issue-item {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 4px 0;
  color: rgba(255, 255, 255, 0.8);
  font-size: 12px;
  line-height: 1.4;
}

.issue-icon {
  color: #ffa726;
  font-size: 10px;
  margin-top: 2px;
}

.issue-text {
  flex: 1;
}
</style>

<style scoped>
/* 输出文件区域 */
.output-section {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.output-header {
  display: flex;
  align-items: center;
}

.output-title {
  color: rgba(255, 255, 255, 0.7);
  font-size: 12px;
  font-weight: 500;
}

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
