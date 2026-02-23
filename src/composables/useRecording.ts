/**
 * 录屏功能组合式函数
 *
 * 提供录屏功能的封装：
 * - 调用 Sidecar 录屏服务
 * - 管理录制状态和时长
 * - 错误处理
 * - 时间格式化
 *
 * @validates Requirements 13.1, 13.6
 */

import { ref, computed, onUnmounted, ComputedRef, Ref } from 'vue'
import { useSidecarStore } from '@/stores/sidecar'
import type { RecordStartParams, RecordResult, RecordingState } from '@/types'

export interface UseRecordingOptions {
  /** 是否自动初始化 Sidecar */
  autoInit?: boolean
  /** 默认帧率 */
  defaultFps?: number
}

export interface RecordingRegion {
  x: number
  y: number
  width: number
  height: number
}

export interface UseRecordingReturn {
  /** 录制状态 */
  recordingState: ComputedRef<RecordingState>
  /** 是否正在录制（包括暂停） */
  isRecording: ComputedRef<boolean>
  /** 是否正在处理请求 */
  isProcessing: ComputedRef<boolean>
  /** 录制时长（秒） */
  duration: Ref<number>
  /** 格式化的录制时间 (MM:SS) */
  formattedTime: ComputedRef<string>
  /** 格式化的录制时间 (HH:MM:SS) */
  formattedTimeLong: ComputedRef<string>
  /** 错误信息 */
  error: Ref<string | null>
  /** 最后录制结果 */
  lastResult: Ref<RecordResult | null>
  /** 开始录制 */
  start: (params: RecordStartParams) => Promise<void>
  /** 暂停录制 */
  pause: () => Promise<void>
  /** 恢复录制 */
  resume: () => Promise<void>
  /** 停止录制 */
  stop: () => Promise<RecordResult>
  /** 清除错误 */
  clearError: () => void
  /** 重置状态 */
  reset: () => void
}

/**
 * 录屏功能组合式函数
 */
export function useRecording(options: UseRecordingOptions = {}): UseRecordingReturn {
  const { defaultFps = 30 } = options

  const sidecarStore = useSidecarStore()

  // ============================================
  // State
  // ============================================

  /** 本地录制时长（秒） */
  const duration = ref(0)

  /** 错误信息 */
  const error = ref<string | null>(null)

  /** 最后录制结果 */
  const lastResult = ref<RecordResult | null>(null)

  /** 计时器 ID */
  let timerInterval: ReturnType<typeof setInterval> | null = null

  /** 录制开始时间 */
  let startTime: number | null = null

  /** 暂停时的累计时长 */
  let pausedDuration = 0

  // ============================================
  // Computed
  // ============================================

  /** 录制状态 */
  const recordingState = computed<RecordingState>(() => sidecarStore.recordingState)

  /** 是否正在录制（包括暂停） */
  const isRecording = computed(() => sidecarStore.isRecording)

  /** 是否正在处理请求 */
  const isProcessing = computed(
    () => sidecarStore.isProcessing && sidecarStore.currentService === 'record'
  )

  /** 格式化的录制时间 (MM:SS) */
  const formattedTime = computed(() => {
    const secs = Math.floor(duration.value)
    const mins = Math.floor(secs / 60)
    const remainingSecs = secs % 60
    return `${mins.toString().padStart(2, '0')}:${remainingSecs.toString().padStart(2, '0')}`
  })

  /** 格式化的录制时间 (HH:MM:SS) */
  const formattedTimeLong = computed(() => {
    const secs = Math.floor(duration.value)
    const hours = Math.floor(secs / 3600)
    const mins = Math.floor((secs % 3600) / 60)
    const remainingSecs = secs % 60
    return `${hours.toString().padStart(2, '0')}:${mins.toString().padStart(2, '0')}:${remainingSecs.toString().padStart(2, '0')}`
  })

  // ============================================
  // Methods
  // ============================================

  /**
   * 确保录屏服务已就绪
   * 
   * 注意：录屏功能已改为原生 Rust 实现，不再依赖 Python Sidecar。
   * 此方法保留兼容性，但不再检查 Sidecar 状态。
   */
  async function ensureReady(): Promise<void> {
    // 原生 Rust 录屏引擎随应用启动自动初始化，无需额外检查
  }

  /**
   * 启动计时器
   */
  function startTimer(): void {
    stopTimer()
    startTime = Date.now()

    timerInterval = setInterval(() => {
      if (recordingState.value === 'recording' && startTime !== null) {
        duration.value = pausedDuration + (Date.now() - startTime) / 1000
        // 同步更新 store 中的时长
        sidecarStore.updateRecordingDuration(duration.value)
      }
    }, 100) // 100ms 更新一次，更平滑
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

  /**
   * 暂停计时器
   */
  function pauseTimer(): void {
    if (startTime !== null) {
      pausedDuration = duration.value
      startTime = null
    }
    stopTimer()
  }

  /**
   * 开始录制
   * @param params 录制参数
   */
  async function start(params: RecordStartParams): Promise<void> {
    try {
      error.value = null

      // 确保录屏服务已就绪
      await ensureReady()

      // 设置默认帧率
      if (!params.fps) {
        params.fps = defaultFps
      }

      // 开始录制
      await sidecarStore.startRecording(params)

      // 重置计时
      duration.value = 0
      pausedDuration = 0
      lastResult.value = null

      // 启动计时器
      startTimer()
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e)
      error.value = message
      throw e
    }
  }

  /**
   * 暂停录制
   */
  async function pause(): Promise<void> {
    try {
      error.value = null
      await sidecarStore.pauseRecording()
      pauseTimer()
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e)
      error.value = message
      throw e
    }
  }

  /**
   * 恢复录制
   */
  async function resume(): Promise<void> {
    try {
      error.value = null
      await sidecarStore.resumeRecording()
      startTimer()
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e)
      error.value = message
      throw e
    }
  }

  /**
   * 停止录制
   * @returns 录制结果
   */
  async function stop(): Promise<RecordResult> {
    try {
      error.value = null
      stopTimer()

      const result = await sidecarStore.stopRecording()

      // 保存结果
      lastResult.value = result

      // 重置计时
      duration.value = 0
      pausedDuration = 0
      startTime = null

      return result
    } catch (e) {
      const message = e instanceof Error ? e.message : String(e)
      error.value = message
      throw e
    }
  }

  /**
   * 清除错误
   */
  function clearError(): void {
    error.value = null
  }

  /**
   * 重置状态
   */
  function reset(): void {
    stopTimer()
    duration.value = 0
    pausedDuration = 0
    startTime = null
    error.value = null
    lastResult.value = null
  }

  // ============================================
  // Lifecycle
  // ============================================

  // 组件卸载时清理计时器
  onUnmounted(() => {
    stopTimer()
  })

  return {
    recordingState,
    isRecording,
    isProcessing,
    duration,
    formattedTime,
    formattedTimeLong,
    error,
    lastResult,
    start,
    pause,
    resume,
    stop,
    clearError,
    reset,
  }
}
