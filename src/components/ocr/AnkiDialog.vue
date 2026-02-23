<template>
  <div class="anki-dialog" :class="{ 'is-loading': isLoading }">
    <!-- 对话框头部 -->
    <div class="dialog-header">
      <span class="dialog-title">📚 添加到 Anki</span>
      <button class="close-btn" @click="handleClose" :disabled="isLoading">
        ✕
      </button>
    </div>

    <!-- 连接状态检查 -->
    <div v-if="isCheckingConnection" class="connection-checking">
      <div class="loading-spinner" />
      <span class="checking-text">正在检查 Anki 连接...</span>
    </div>

    <!-- 未连接状态 -->
    <div v-else-if="!isConnected" class="not-connected">
      <span class="warning-icon">⚠️</span>
      <span class="warning-text">{{ connectionError || '无法连接到 Anki' }}</span>
      <p class="help-text">
        请确保：<br/>
        1. Anki 已启动<br/>
        2. 已安装 AnkiConnect 插件
      </p>
      <button class="retry-btn" @click="handleRetryConnection">
        🔄 重新连接
      </button>
    </div>

    <!-- 制卡表单 -->
    <template v-else>
      <!-- 图片预览 -->
      <div v-if="imageSrc" class="image-preview">
        <img :src="imageSrc" alt="截图预览" class="preview-image" />
      </div>

      <!-- 表单内容 -->
      <div class="form-content">
        <!-- 牌组选择 -->
        <div class="form-group">
          <label class="form-label">牌组</label>
          <select 
            v-model="selectedDeck" 
            class="form-select"
            :disabled="isLoading || deckList.length === 0"
          >
            <option v-if="deckList.length === 0" value="">加载中...</option>
            <option 
              v-for="deck in deckList" 
              :key="deck" 
              :value="deck"
            >
              {{ deck }}
            </option>
          </select>
        </div>

        <!-- 笔记类型选择 -->
        <div class="form-group">
          <label class="form-label">笔记类型</label>
          <select 
            v-model="selectedNoteType" 
            class="form-select"
            :disabled="isLoading || noteTypeList.length === 0"
          >
            <option v-if="noteTypeList.length === 0" value="">加载中...</option>
            <option 
              v-for="noteType in noteTypeList" 
              :key="noteType" 
              :value="noteType"
            >
              {{ noteType }}
            </option>
          </select>
        </div>

        <!-- 正面内容 -->
        <div class="form-group">
          <label class="form-label">正面（OCR 文字）</label>
          <textarea
            v-model="frontContent"
            class="form-textarea"
            placeholder="输入卡片正面内容..."
            :disabled="isLoading"
            rows="3"
          />
        </div>

        <!-- 背面内容 -->
        <div class="form-group">
          <label class="form-label">背面（可选）</label>
          <textarea
            v-model="backContent"
            class="form-textarea"
            placeholder="输入卡片背面内容..."
            :disabled="isLoading"
            rows="3"
          />
        </div>

        <!-- 标签 -->
        <div class="form-group">
          <label class="form-label">标签（用空格分隔）</label>
          <input
            v-model="tagsInput"
            type="text"
            class="form-input"
            placeholder="例如: 英语 单词 截图"
            :disabled="isLoading"
          />
        </div>
      </div>

      <!-- 操作按钮 -->
      <div class="dialog-actions">
        <button 
          class="action-btn cancel-btn" 
          @click="handleClose"
          :disabled="isLoading"
        >
          取消
        </button>
        <button 
          class="action-btn submit-btn"
          :class="{ 'is-loading': isLoading }"
          :disabled="!canSubmit"
          @click="handleSubmit"
        >
          <span v-if="isLoading" class="loading-spinner-small" />
          <span class="btn-icon">{{ isLoading ? '' : '✓' }}</span>
          <span class="btn-text">{{ isLoading ? '添加中...' : '添加卡片' }}</span>
        </button>
      </div>

      <!-- 错误提示 -->
      <div v-if="error" class="error-message">
        <span class="error-icon">⚠️</span>
        <span class="error-text">{{ error }}</span>
      </div>

      <!-- 成功提示 -->
      <Transition name="toast">
        <div v-if="showSuccess" class="success-toast">
          ✓ 卡片已添加到 Anki
        </div>
      </Transition>
    </template>
  </div>
</template>

<script setup lang="ts">
/**
 * Anki 制卡对话框组件
 * 
 * 功能：
 * - 选择牌组和笔记类型
 * - 编辑卡片正面/背面内容
 * - 添加标签
 * - 显示制卡结果
 * 
 * @validates Requirements 10.1, 10.3
 */

import { ref, computed, watch, onMounted } from 'vue'
import { useAnki } from '@/composables/useAnki'
import type { AnkiAddCardParams, AnkiCardFields } from '@/types'

// ============================================
// Props & Emits
// ============================================

interface Props {
  /** 图片源 URL */
  imageSrc?: string
  /** 图片文件路径（用于 Anki） */
  imagePath?: string
  /** OCR 识别的文字 */
  ocrText?: string
  /** 是否显示对话框 */
  visible?: boolean
}

const props = withDefaults(defineProps<Props>(), {
  imageSrc: '',
  imagePath: '',
  ocrText: '',
  visible: true,
})

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'success', cardId: number): void
  (e: 'error', message: string): void
}>()

// ============================================
// Composables
// ============================================

const {
  isConnected,
  isLoading,
  isCheckingConnection,
  error: ankiError,
  decks,
  noteTypes,
  currentDeck,
  currentNoteType,
  checkConnection,
  loadDecks,
  loadNoteTypes,
  createCard,
  setDeck,
  setNoteType,
} = useAnki()

// ============================================
// State
// ============================================

/** 选择的牌组 */
const selectedDeck = ref('')

/** 选择的笔记类型 */
const selectedNoteType = ref('')

/** 正面内容 */
const frontContent = ref('')

/** 背面内容 */
const backContent = ref('')

/** 标签输入 */
const tagsInput = ref('')

/** 连接错误信息 */
const connectionError = ref<string | null>(null)

/** 表单错误信息 */
const error = ref<string | null>(null)

/** 是否显示成功提示 */
const showSuccess = ref(false)

// ============================================
// Computed
// ============================================

/** 牌组列表（确保非空数组） */
const deckList = computed(() => decks.value ?? [])

/** 笔记类型列表（确保非空数组） */
const noteTypeList = computed(() => noteTypes.value ?? [])

/** 是否可以提交 */
const canSubmit = computed(() => 
  isConnected.value &&
  !isLoading.value &&
  selectedDeck.value.length > 0 &&
  selectedNoteType.value.length > 0 &&
  (frontContent.value.trim().length > 0 || props.imagePath)
)

/** 解析后的标签数组 */
const parsedTags = computed(() => 
  tagsInput.value
    .split(/\s+/)
    .map(tag => tag.trim())
    .filter(tag => tag.length > 0)
)

// ============================================
// Methods
// ============================================

/**
 * 初始化对话框
 */
async function initialize(): Promise<void> {
  // 设置初始内容
  frontContent.value = props.ocrText || ''
  
  // 检查连接
  const connected = await checkConnection()
  
  if (connected) {
    connectionError.value = null
    
    // 加载牌组和笔记类型
    await Promise.all([loadDecks(), loadNoteTypes()])
    
    // 设置默认选择
    if (deckList.value.length > 0) {
      selectedDeck.value = currentDeck.value || deckList.value[0]
    }
    if (noteTypeList.value.length > 0) {
      selectedNoteType.value = currentNoteType.value || noteTypeList.value[0]
    }
  } else {
    connectionError.value = ankiError.value || '无法连接到 Anki'
  }
}

/**
 * 重试连接
 */
async function handleRetryConnection(): Promise<void> {
  connectionError.value = null
  await initialize()
}

/**
 * 关闭对话框
 */
function handleClose(): void {
  emit('close')
}

/**
 * 提交表单
 */
async function handleSubmit(): Promise<void> {
  if (!canSubmit.value) return
  
  error.value = null
  
  // 构建字段
  const fields: AnkiCardFields = {
    '正面': frontContent.value,
    '背面': backContent.value,
  }
  
  // 构建参数
  const params: AnkiAddCardParams = {
    deck: selectedDeck.value,
    noteType: selectedNoteType.value,
    fields,
    tags: parsedTags.value,
    imagePath: props.imagePath || undefined,
  }
  
  // 创建卡片
  const result = await createCard(params)
  
  if (result) {
    // 显示成功提示
    showSuccess.value = true
    setTimeout(() => {
      showSuccess.value = false
    }, 2000)
    
    // 触发成功事件
    emit('success', result.cardId)
    
    // 保存当前选择
    setDeck(selectedDeck.value)
    setNoteType(selectedNoteType.value)
    
    // 延迟关闭
    setTimeout(() => {
      handleClose()
    }, 1500)
  } else {
    error.value = ankiError.value || '添加卡片失败'
    emit('error', error.value)
  }
}

// ============================================
// Watchers
// ============================================

// 同步牌组选择
watch(selectedDeck, (newVal) => {
  if (newVal) {
    setDeck(newVal)
  }
})

// 同步笔记类型选择
watch(selectedNoteType, (newVal) => {
  if (newVal) {
    setNoteType(newVal)
  }
})

// 监听 OCR 文字变化
watch(() => props.ocrText, (newVal) => {
  if (newVal && !frontContent.value) {
    frontContent.value = newVal
  }
})

// 监听可见性变化
watch(() => props.visible, (newVal) => {
  if (newVal) {
    initialize()
  }
})

// ============================================
// Lifecycle
// ============================================

onMounted(() => {
  if (props.visible) {
    initialize()
  }
})
</script>

<style scoped>
.anki-dialog {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 16px;
  background: rgba(30, 30, 30, 0.98);
  border-radius: 8px;
  border: 1px solid rgba(255, 255, 255, 0.1);
  min-width: 320px;
  max-width: 400px;
  max-height: 600px;
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

/* 连接检查状态 */
.connection-checking {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  padding: 32px;
}

.checking-text {
  color: rgba(255, 255, 255, 0.7);
  font-size: 13px;
}

/* 未连接状态 */
.not-connected {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 24px;
  text-align: center;
}

.warning-icon {
  font-size: 32px;
}

.warning-text {
  color: #ff9800;
  font-size: 14px;
  font-weight: 500;
}

.help-text {
  color: rgba(255, 255, 255, 0.5);
  font-size: 12px;
  line-height: 1.6;
  margin: 8px 0;
}

.retry-btn {
  margin-top: 8px;
  padding: 8px 16px;
  background: rgba(66, 133, 244, 0.8);
  border: none;
  border-radius: 4px;
  color: #fff;
  font-size: 13px;
  cursor: pointer;
  transition: background 0.15s;
}

.retry-btn:hover {
  background: rgba(66, 133, 244, 1);
}

/* 图片预览 */
.image-preview {
  border-radius: 6px;
  overflow: hidden;
  background: #000;
  max-height: 120px;
}

.preview-image {
  display: block;
  width: 100%;
  height: auto;
  max-height: 120px;
  object-fit: contain;
}

/* 表单内容 */
.form-content {
  display: flex;
  flex-direction: column;
  gap: 12px;
  flex: 1;
  overflow-y: auto;
  padding-right: 4px;
}

.form-content::-webkit-scrollbar {
  width: 6px;
}

.form-content::-webkit-scrollbar-track {
  background: rgba(255, 255, 255, 0.05);
  border-radius: 3px;
}

.form-content::-webkit-scrollbar-thumb {
  background: rgba(255, 255, 255, 0.2);
  border-radius: 3px;
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

.form-select,
.form-input,
.form-textarea {
  padding: 8px 10px;
  background: rgba(0, 0, 0, 0.3);
  border: 1px solid rgba(255, 255, 255, 0.15);
  border-radius: 4px;
  color: #fff;
  font-size: 13px;
  outline: none;
  transition: border-color 0.15s;
}

.form-select:hover,
.form-input:hover,
.form-textarea:hover {
  border-color: rgba(255, 255, 255, 0.25);
}

.form-select:focus,
.form-input:focus,
.form-textarea:focus {
  border-color: rgba(66, 133, 244, 0.6);
}

.form-select:disabled,
.form-input:disabled,
.form-textarea:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.form-select option {
  background: #2a2a2a;
  color: #fff;
}

.form-textarea {
  resize: none;
  font-family: 'Microsoft YaHei', sans-serif;
  line-height: 1.5;
}

/* 操作按钮 */
.dialog-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  padding-top: 12px;
  border-top: 1px solid rgba(255, 255, 255, 0.1);
}

.action-btn {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 8px 16px;
  border: none;
  border-radius: 4px;
  font-size: 13px;
  cursor: pointer;
  transition: all 0.15s;
}

.cancel-btn {
  background: rgba(255, 255, 255, 0.1);
  color: rgba(255, 255, 255, 0.8);
}

.cancel-btn:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.15);
}

.submit-btn {
  background: rgba(76, 175, 80, 0.8);
  color: #fff;
}

.submit-btn:hover:not(:disabled) {
  background: rgba(76, 175, 80, 1);
}

.submit-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.submit-btn.is-loading {
  background: rgba(76, 175, 80, 0.6);
}

.btn-icon {
  font-size: 14px;
}

/* 加载动画 */
.loading-spinner {
  width: 24px;
  height: 24px;
  border: 3px solid rgba(255, 255, 255, 0.2);
  border-top-color: #4285f4;
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

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
