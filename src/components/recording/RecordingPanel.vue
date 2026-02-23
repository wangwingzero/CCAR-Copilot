<template>
  <div class="recording-panel" :class="{ 'is-recording': isRecording }">
    <!-- 面板头部 -->
    <div class="panel-header">
      <span class="panel-title">
        <span class="title-icon">🎬</span>
        <span class="title-text">屏幕录制</span>
      </span>
      <button class="close-btn" @click="handleClose" :disabled="isRecording">
        ✕
      </button>
    </div>

    <!-- 录制状态指示器 -->
    <div class="status-section">
      <div class="status-indicator" :class="statusClass">
        <span class="status-dot" />
        <span class="status-text">{{ statusText }}</span>
      </div>
      <div class="time-display">
        <span class="time-icon">⏱️</span>
        <span class="time-value">{{ formattedTime }}</span>
      </div>
    </div>

    <!-- 录制设置（仅在空闲状态显示） -->
    <div v-if="!isRecording" class="settings-section">
      <!-- 录制区域选择 -->
      <div class="form-group">
        <label class="form-label">录制区域</label>
        <div class="region-options">
          <button
            class="region-btn"
            :class="{ active: recordMode === 'fullscreen' }"
            @click="recordMode = 'fullscreen'"
          >
            <span class="btn-icon">🖥️</span>
            <span class="btn-text">全屏</span>
          </button>
          <button
            class="region-btn"
            :class="{ active: recordMode === 'region' }"
            @click="recordMode = 'region'"
          >
            <span class="btn-icon">⬜</span>
            <span class="btn-text">选区</span>
          </button>
        </div>
      </div>

      <!-- 帧率选择 -->
      <div class="form-group">
        <label class="form-label">帧率</label>
        <select v-model="fps" class="form-select">
          <option :value="15">15 FPS (低)</option>
          <option :value="24">24 FPS (电影)</option>
          <option :value="30">30 FPS (标准)</option>
          <option :value="60">60 FPS (高)</option>
        </select>
      </div>

      <!-- 音频选项 -->
      <div class="form-group">
        <label class="form-label">音频</label>
        <div class="audio-options">
          <label class="checkbox-label">
            <input type="checkbox" v-model="systemAudio" />
            <span>系统音频</span>
          </label>
          <label class="checkbox-label">
            <input type="checkbox" v-model="micAudio" />
            <span>麦克风</span>
          </label>
        </div>
      </div>
    </div>

    <!-- 录制信息（录制中显示） -->
    <div v-if="isRecording" class="recording-info">
      <div class="info-item">
        <span class="info-label">文件大小</span>
        <span class="info-value">{{ formattedFileSize }}</span>
      </div>
      <div class="info-item">
        <span class="info-label">帧数</span>
        <span class="info-value">{{ frameCount }}</span>
      </div>
      <div v-if="outputPath" class="info-item output-path">
        <span class="info-label">输出路径</span>
        <span class="info-value path-text" :title="outputPath">{{ truncatedPath }}</span>
      </div>
    </div>

    <!-- 控制按钮 -->
    <div class="controls-section">
      <!-- 空闲状态：开始按钮 -->
      <template v-if="recordingState === 'idle'">
        <button
          class="control-btn start-btn"
          :disabled="isProcessing"
          @click="handleStart"
        >
          <span v-if="isProcessing" class="loading-spinner" />
          <span v-else class="btn-icon">⏺️</span>
          <span class="btn-text">{{ isProcessing ? '准备中...' : '开始录制' }}</span>
        </button>
      </template>

      <!-- 录制中状态：暂停/停止按钮 -->
      <template v-else-if="recordingState === 'recording'">
        <button
          class="control-btn pause-btn"
          :disabled="isProcessing"
          @click="handlePause"
        >
          <span class="btn-icon">⏸️</span>
          <span class="btn-text">暂停</span>
        </button>
        <button
          class="control-btn stop-btn"
          :disabled="isProcessing"
          @click="handleStop"
        >
          <span class="btn-icon">⏹️</span>
          <span class="btn-text">停止</span>
        </button>
      </template>

      <!-- 暂停状态：继续/停止按钮 -->
      <template v-else-if="recordingState === 'paused'">
        <button
          class="control-btn resume-btn"
          :disabled="isProcessing"
          @click="handleResume"
        >
          <span class="btn-icon">▶️</span>
          <span class="btn-text">继续</span>
        </button>
        <button
          class="control-btn stop-btn"
          :disabled="isProcessing"
          @click="handleStop"
        >
          <span class="btn-icon">⏹️</span>
          <span class="btn-text">停止</span>
        </button>
      </template>

      <!-- 编码中状态：等待 -->
      <template v-else-if="recordingState === 'encoding'">
        <div class="encoding-status">
          <span class="loading-spinner" />
          <span class="encoding-text">正在编码视频...</span>
        </div>
      </template>
    </div>

    <!-- 错误提示 -->
    <div v-if="error" class="error-message">
      <span class="error-icon">⚠️</span>
      <span class="error-text">{{ error }}</span>
      <button class="error-dismiss" @click="clearError">✕</button>
    </div>

    <!-- 成功提示 -->
    <Transition name="toast">
      <div v-if="showSuccess" class="success-toast">
        <span class="success-icon">✓</span>
        <span class="success-text">录制完成！</span>
        <button class="toast-action" @click="handleOpenFile">打开文件</button>
      </div>
    </Transition>
  </div>
</template>

<script setup lang="ts">
/**
 * 录屏控制面板组件
 *
 * 功能：
 * - 显示录制状态（空闲、录制中、暂停、编码中）
 * - 显示录制时间（格式化为 MM:SS）
 * - 支持开始/暂停/继续/停止操作
 * - 显示文件大小和帧数
 * - 支持选择录制区域或全屏
 * - 支持配置帧率和音频选项
 *
 * @validates Requirements 13.1, 13.6
 */

import { ref, computed, watch, onMounted, onUnmounted } from 'vue'
import { openPath } from '@tauri-apps/plugin-opener'
import { save } from '@tauri-apps/plugin-dialog'
import { useSidecarStore } from '@/stores/sidecar'
import type { RecordStartParams, RecordingState } from '@/types'

// ============================================
// Props & Emits
// ============================================

interface Props {
  /** 是否显示面板 */
  visible?: boolean
  /** 初始录制区域 */
  initialRegion?: {
    x: number
    y: number
    width: number
    height: number
  }
}

const props = withDefaults(defineProps<Props>(), {
  visible: true,
})

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'start'): void
  (e: 'stop', result: { outputPath: string; duration: number; fileSize: number }): void
  (e: 'error', message: string): void
  (e: 'select-region'): void
}>()

// ============================================
// Store
// ============================================

const sidecarStore = useSidecarStore()

// ============================================
// State
// ============================================

/** 录制模式：全屏或选区 */
const recordMode = ref<'fullscreen' | 'region'>('fullscreen')

/** 帧率 */
const fps = ref(30)

/** 是否录制系统音频 */
const systemAudio = ref(false)

/** 是否录制麦克风 */
const micAudio = ref(false)

/** 选定的录制区域 */
const selectedRegion = ref<{ x: number; y: number; width: number; height: number } | null>(null)

/** 错误信息 */
const error = ref<string | null>(null)

/** 是否显示成功提示 */
const showSuccess = ref(false)

/** 最后录制的文件路径 */
const lastOutputPath = ref<string | null>(null)

/** 文件大小（字节） */
const fileSize = ref(0)

/** 帧数 */
const frameCount = ref(0)

/** 输出路径 */
const outputPath = ref<string | null>(null)

/** 计时器 ID */
let timerInterval: ReturnType<typeof setInterval> | null = null

/** 本地录制时长（用于实时更新） */
const localDuration = ref(0)

// ============================================
// Computed
// ============================================

/** 录制状态 */
const recordingState = computed<RecordingState>(() => sidecarStore.recordingState)

/** 是否正在录制（包括暂停） */
const isRecording = computed(() => sidecarStore.isRecording)

/** 是否正在处理请求 */
const isProcessing = computed(() => sidecarStore.isProcessing && sidecarStore.currentService === 'record')

/** 状态样式类 */
const statusClass = computed(() => ({
  'is-idle': recordingState.value === 'idle',
  'is-recording': recordingState.value === 'recording',
  'is-paused': recordingState.value === 'paused',
  'is-encoding': recordingState.value === 'encoding',
}))

/** 状态文本 */
const statusText = computed(() => {
  switch (recordingState.value) {
    case 'idle':
      return '准备就绪'
    case 'recording':
      return '正在录制'
    case 'paused':
      return '已暂停'
    case 'encoding':
      return '编码中'
    default:
      return '未知状态'
  }
})

/** 格式化的录制时间 (MM:SS) */
const formattedTime = computed(() => {
  const seconds = localDuration.value
  const mins = Math.floor(seconds / 60)
  const secs = Math.floor(seconds % 60)
  return `${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')}`
})

/** 格式化的文件大小 */
const formattedFileSize = computed(() => {
  const bytes = fileSize.value
  if (bytes === 0) return '0 B'
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / 1024 / 1024).toFixed(2)} MB`
})

/** 截断的输出路径 */
const truncatedPath = computed(() => {
  const path = outputPath.value
  if (!path) return ''
  if (path.length <= 40) return path
  return '...' + path.slice(-37)
})

// ============================================
// Methods
// ============================================

/**
 * 开始录制
 */
async function handleStart(): Promise<void> {
  try {
    error.value = null

    // 如果选择了选区模式但没有选区，触发选区事件
    if (recordMode.value === 'region' && !selectedRegion.value) {
      emit('select-region')
      return
    }

    // 获取输出路径
    const savePath = await save({
      defaultPath: `recording_${Date.now()}.mp4`,
      filters: [{ name: 'MP4 视频', extensions: ['mp4'] }],
      title: '保存录制文件',
    })

    if (!savePath) {
      return // 用户取消
    }

    // 构建录制参数
    const params: RecordStartParams = {
      fps: fps.value,
      systemAudio: systemAudio.value,
      micAudio: micAudio.value,
      outputPath: savePath,
    }

    // 如果是选区模式，添加区域参数
    if (recordMode.value === 'region' && selectedRegion.value) {
      params.region = selectedRegion.value
    }

    // 开始录制
    await sidecarStore.startRecording(params)

    // 保存输出路径
    outputPath.value = savePath

    // 启动计时器
    startTimer()

    emit('start')
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e)
    error.value = message
    emit('error', message)
  }
}

/**
 * 暂停录制
 */
async function handlePause(): Promise<void> {
  try {
    error.value = null
    await sidecarStore.pauseRecording()
    stopTimer()
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e)
    error.value = message
    emit('error', message)
  }
}

/**
 * 恢复录制
 */
async function handleResume(): Promise<void> {
  try {
    error.value = null
    await sidecarStore.resumeRecording()
    startTimer()
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e)
    error.value = message
    emit('error', message)
  }
}

/**
 * 停止录制
 */
async function handleStop(): Promise<void> {
  try {
    error.value = null
    stopTimer()

    const result = await sidecarStore.stopRecording()

    // 保存结果
    lastOutputPath.value = result.outputPath
    fileSize.value = result.fileSize

    // 显示成功提示
    showSuccess.value = true
    setTimeout(() => {
      showSuccess.value = false
    }, 5000)

    // 重置状态
    localDuration.value = 0
    frameCount.value = 0
    outputPath.value = null

    emit('stop', {
      outputPath: result.outputPath,
      duration: result.duration,
      fileSize: result.fileSize,
    })
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e)
    error.value = message
    emit('error', message)
  }
}

/**
 * 关闭面板
 */
function handleClose(): void {
  if (isRecording.value) {
    // 录制中不允许关闭
    return
  }
  emit('close')
}

/**
 * 打开录制文件
 */
async function handleOpenFile(): Promise<void> {
  if (!lastOutputPath.value) return

  try {
    await openPath(lastOutputPath.value)
  } catch (e) {
    console.error('Failed to open file:', e)
  }
}

/**
 * 清除错误
 */
function clearError(): void {
  error.value = null
}

/**
 * 启动计时器
 */
function startTimer(): void {
  if (timerInterval) {
    clearInterval(timerInterval)
  }

  timerInterval = setInterval(() => {
    if (recordingState.value === 'recording') {
      localDuration.value += 1
      // 同步更新 store 中的时长
      sidecarStore.updateRecordingDuration(localDuration.value)
    }
  }, 1000)
}

/**
 * 停止计时器
 */
function stopTimer(): void {
  if (timerInterval) {
    clearInterval(timerInterval)
    timerInterval = null
  }
}

// ============================================
// Watchers
// ============================================

// 监听初始区域
watch(
  () => props.initialRegion,
  (newRegion) => {
    if (newRegion) {
      selectedRegion.value = newRegion
      recordMode.value = 'region'
    }
  },
  { immediate: true }
)

// 监听录制状态变化
watch(recordingState, (newState) => {
  if (newState === 'idle') {
    stopTimer()
  }
})

// ============================================
// Lifecycle
// ============================================

onMounted(() => {
  // 初始化时重置状态
  localDuration.value = 0
  fileSize.value = 0
  frameCount.value = 0
})

onUnmounted(() => {
  // 清理计时器
  stopTimer()
})
</script>


<style scoped>
.recording-panel {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 16px;
  background: rgba(30, 30, 30, 0.98);
  border-radius: 8px;
  border: 1px solid rgba(255, 255, 255, 0.1);
  min-width: 320px;
  max-width: 400px;
  position: relative;
}

.recording-panel.is-recording {
  border-color: rgba(244, 67, 54, 0.5);
  box-shadow: 0 0 20px rgba(244, 67, 54, 0.2);
}

/* 面板头部 */
.panel-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding-bottom: 8px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.1);
}

.panel-title {
  display: flex;
  align-items: center;
  gap: 8px;
  color: #fff;
  font-size: 15px;
  font-weight: 500;
}

.title-icon {
  font-size: 16px;
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
  opacity: 0.3;
  cursor: not-allowed;
}

/* 状态区域 */
.status-section {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 12px;
  background: rgba(0, 0, 0, 0.3);
  border-radius: 6px;
}

.status-indicator {
  display: flex;
  align-items: center;
  gap: 8px;
}

.status-dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: rgba(255, 255, 255, 0.3);
  transition: all 0.3s;
}

.status-indicator.is-idle .status-dot {
  background: rgba(255, 255, 255, 0.3);
}

.status-indicator.is-recording .status-dot {
  background: #f44336;
  animation: pulse 1s ease-in-out infinite;
}

.status-indicator.is-paused .status-dot {
  background: #ffa726;
}

.status-indicator.is-encoding .status-dot {
  background: #4285f4;
  animation: pulse 0.5s ease-in-out infinite;
}

@keyframes pulse {
  0%, 100% {
    opacity: 1;
    transform: scale(1);
  }
  50% {
    opacity: 0.6;
    transform: scale(0.9);
  }
}

.status-text {
  color: rgba(255, 255, 255, 0.9);
  font-size: 13px;
  font-weight: 500;
}

.time-display {
  display: flex;
  align-items: center;
  gap: 6px;
}

.time-icon {
  font-size: 14px;
}

.time-value {
  font-family: 'Consolas', 'Monaco', monospace;
  font-size: 18px;
  font-weight: 600;
  color: #fff;
  letter-spacing: 1px;
}

/* 设置区域 */
.settings-section {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

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

.region-options {
  display: flex;
  gap: 8px;
}

.region-btn {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 10px 12px;
  background: rgba(255, 255, 255, 0.05);
  border: 1px solid rgba(255, 255, 255, 0.15);
  border-radius: 4px;
  color: rgba(255, 255, 255, 0.7);
  font-size: 13px;
  cursor: pointer;
  transition: all 0.15s;
}

.region-btn:hover {
  background: rgba(255, 255, 255, 0.1);
  border-color: rgba(255, 255, 255, 0.25);
}

.region-btn.active {
  background: rgba(66, 133, 244, 0.2);
  border-color: rgba(66, 133, 244, 0.5);
  color: #fff;
}

.btn-icon {
  font-size: 14px;
}

.form-select {
  padding: 10px 12px;
  background: rgba(0, 0, 0, 0.3);
  border: 1px solid rgba(255, 255, 255, 0.15);
  border-radius: 4px;
  color: #fff;
  font-size: 13px;
  outline: none;
  cursor: pointer;
  transition: border-color 0.15s;
}

.form-select:hover {
  border-color: rgba(255, 255, 255, 0.25);
}

.form-select:focus {
  border-color: rgba(66, 133, 244, 0.6);
}

.form-select option {
  background: #2a2a2a;
  color: #fff;
}

.audio-options {
  display: flex;
  gap: 16px;
}

.checkbox-label {
  display: flex;
  align-items: center;
  gap: 6px;
  color: rgba(255, 255, 255, 0.8);
  font-size: 13px;
  cursor: pointer;
}

.checkbox-label input[type="checkbox"] {
  width: 16px;
  height: 16px;
  accent-color: #4285f4;
  cursor: pointer;
}

/* 录制信息 */
.recording-info {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 12px;
  background: rgba(0, 0, 0, 0.2);
  border-radius: 4px;
}

.info-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.info-item.output-path {
  flex-direction: column;
  align-items: flex-start;
  gap: 4px;
}

.info-label {
  color: rgba(255, 255, 255, 0.5);
  font-size: 12px;
}

.info-value {
  color: rgba(255, 255, 255, 0.9);
  font-size: 13px;
  font-weight: 500;
}

.path-text {
  font-size: 11px;
  font-family: 'Consolas', 'Monaco', monospace;
  color: rgba(255, 255, 255, 0.6);
  word-break: break-all;
}

/* 控制按钮 */
.controls-section {
  display: flex;
  gap: 8px;
  margin-top: 4px;
}

.control-btn {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 12px 16px;
  border: none;
  border-radius: 6px;
  font-size: 14px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.15s;
}

.control-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.start-btn {
  background: linear-gradient(135deg, #f44336, #e91e63);
  color: #fff;
}

.start-btn:hover:not(:disabled) {
  background: linear-gradient(135deg, #e53935, #d81b60);
  transform: translateY(-1px);
  box-shadow: 0 4px 12px rgba(244, 67, 54, 0.4);
}

.pause-btn {
  background: rgba(255, 167, 38, 0.8);
  color: #fff;
}

.pause-btn:hover:not(:disabled) {
  background: rgba(255, 167, 38, 1);
}

.resume-btn {
  background: rgba(76, 175, 80, 0.8);
  color: #fff;
}

.resume-btn:hover:not(:disabled) {
  background: rgba(76, 175, 80, 1);
}

.stop-btn {
  background: rgba(244, 67, 54, 0.8);
  color: #fff;
}

.stop-btn:hover:not(:disabled) {
  background: rgba(244, 67, 54, 1);
}

.encoding-status {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 10px;
  padding: 12px;
  background: rgba(66, 133, 244, 0.1);
  border-radius: 6px;
}

.encoding-text {
  color: rgba(255, 255, 255, 0.8);
  font-size: 13px;
}

/* 加载动画 */
.loading-spinner {
  width: 16px;
  height: 16px;
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

/* 错误提示 */
.error-message {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 12px;
  background: rgba(244, 67, 54, 0.15);
  border: 1px solid rgba(244, 67, 54, 0.3);
  border-radius: 4px;
}

.error-icon {
  font-size: 14px;
}

.error-text {
  flex: 1;
  color: #ff6b6b;
  font-size: 12px;
}

.error-dismiss {
  width: 20px;
  height: 20px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: none;
  border-radius: 4px;
  color: rgba(255, 255, 255, 0.5);
  font-size: 12px;
  cursor: pointer;
  transition: all 0.15s;
}

.error-dismiss:hover {
  background: rgba(255, 255, 255, 0.1);
  color: #fff;
}

/* 成功提示 */
.success-toast {
  position: absolute;
  bottom: 16px;
  left: 16px;
  right: 16px;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px 16px;
  background: rgba(76, 175, 80, 0.95);
  border-radius: 6px;
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
  z-index: 100;
}

.success-icon {
  font-size: 16px;
  color: #fff;
}

.success-text {
  flex: 1;
  color: #fff;
  font-size: 13px;
  font-weight: 500;
}

.toast-action {
  padding: 6px 12px;
  background: rgba(255, 255, 255, 0.2);
  border: none;
  border-radius: 4px;
  color: #fff;
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.15s;
}

.toast-action:hover {
  background: rgba(255, 255, 255, 0.3);
}

/* Toast 动画 */
.toast-enter-active,
.toast-leave-active {
  transition: all 0.3s ease;
}

.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translateY(10px);
}
</style>
