/**
 * 截图状态管理 Store
 *
 * 管理截图流程的所有状态：
 * - 截图模式和状态
 * - 捕获的图像
 * - 选区信息
 * - 标注对象
 *
 * @validates Requirements 5.1, 5.2, 5.3, 6.6
 */

import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type {
  CaptureResult,
  MonitorInfo,
  WindowInfo,
  SelectionRegion,
  CaptureMode,
  CaptureState,
  AnnotationObject,
  AnnotationTool,
  AnnotationStyle,
} from '@/types'
import { DEFAULT_ANNOTATION_STYLE } from '@/types'
import {
  exportService,
  createMergedCanvas,
  type ExportFormat,
  type ExportOptions,
  type ExportResult,
} from '@/services/exportService'

export const useScreenshotStore = defineStore('screenshot', () => {
  // ============================================
  // State
  // ============================================

  /** 当前截图状态 */
  const captureState = ref<CaptureState>('idle')

  /** 截图模式 */
  const captureMode = ref<CaptureMode>('region')

  /** 是否正在截图中 */
  const isCapturing = ref(false)

  /** 捕获的图像 (monitorId -> asset:// URL) */
  const capturedImages = ref<Map<number, string>>(new Map())

  /** 显示器信息列表 */
  const monitors = ref<MonitorInfo[]>([])

  /** 检测到的窗口信息 */
  const detectedWindow = ref<WindowInfo | null>(null)

  /** 用户选择的区域 */
  const selectedRegion = ref<SelectionRegion | null>(null)

  /** 标注对象列表 */
  const annotations = ref<AnnotationObject[]>([])

  /** 当前选中的标注工具 */
  const currentTool = ref<AnnotationTool>('select')

  /** 当前工具样式 */
  const toolStyle = ref<AnnotationStyle>({ ...DEFAULT_ANNOTATION_STYLE })

  /** 最后一次错误信息 */
  const lastError = ref<string | null>(null)

  // ============================================
  // Getters
  // ============================================

  /** 是否有选区 */
  const hasSelection = computed(() => selectedRegion.value !== null)

  /** 是否有标注 */
  const hasAnnotations = computed(() => annotations.value.length > 0)

  /** 选中的标注对象 */
  const selectedAnnotations = computed(() =>
    annotations.value.filter((a) => a.selected)
  )

  /** 当前状态是否允许标注 */
  const canAnnotate = computed(() =>
    captureState.value === 'annotating' && hasSelection.value
  )

  // ============================================
  // Actions
  // ============================================

  /**
   * 开始截图流程
   * 捕获所有显示器的屏幕，进入选区模式
   */
  async function startCapture(): Promise<void> {
    try {
      lastError.value = null
      isCapturing.value = true
      captureState.value = 'capturing'

      // 获取显示器信息
      monitors.value = await invoke<MonitorInfo[]>('get_monitors')

      // 捕获所有显示器
      const results = await invoke<CaptureResult[]>('capture_all_monitors')

      // 存储捕获结果
      capturedImages.value.clear()
      for (const result of results) {
        capturedImages.value.set(result.monitorId, result.path)
      }

      // 进入选区模式
      captureState.value = 'selecting'
    } catch (error) {
      lastError.value = error instanceof Error ? error.message : String(error)
      captureState.value = 'idle'
      throw error
    } finally {
      isCapturing.value = false
    }
  }

  /**
   * 完成选区，进入标注模式
   * @param region 用户选择的区域
   */
  async function finishSelection(region: SelectionRegion): Promise<void> {
    selectedRegion.value = region
    captureState.value = 'annotating'
  }

  /**
   * 检测指定坐标下的窗口
   * @param x 逻辑像素 X 坐标
   * @param y 逻辑像素 Y 坐标
   */
  async function detectWindowAt(x: number, y: number): Promise<WindowInfo | null> {
    try {
      const window = await invoke<WindowInfo | null>('detect_window_at', { x, y })
      detectedWindow.value = window
      return window
    } catch (error) {
      console.error('Window detection failed:', error)
      return null
    }
  }

  /**
   * 选择检测到的窗口作为选区
   */
  function selectDetectedWindow(): void {
    if (detectedWindow.value) {
      const window = detectedWindow.value
      selectedRegion.value = {
        x: window.rect.x,
        y: window.rect.y,
        width: window.rect.width,
        height: window.rect.height,
        monitorId: 0, // TODO: 根据窗口位置确定显示器
        physicalRect: window.physicalRect,
      }
      captureState.value = 'annotating'
    }
  }

  /**
   * 添加标注对象
   * @param annotation 标注对象
   */
  function addAnnotation(annotation: AnnotationObject): void {
    annotations.value.push(annotation)
  }

  /**
   * 更新标注对象
   * @param id 标注 ID
   * @param updates 更新内容
   */
  function updateAnnotation(
    id: string,
    updates: Partial<AnnotationObject>
  ): void {
    const index = annotations.value.findIndex((a) => a.id === id)
    if (index !== -1) {
      annotations.value[index] = { ...annotations.value[index], ...updates }
    }
  }

  /**
   * 删除标注对象
   * @param id 标注 ID
   */
  function removeAnnotation(id: string): void {
    const index = annotations.value.findIndex((a) => a.id === id)
    if (index !== -1) {
      annotations.value.splice(index, 1)
    }
  }

  /**
   * 清除所有标注
   */
  function clearAnnotations(): void {
    annotations.value = []
  }

  /**
   * 设置当前工具
   * @param tool 工具类型
   */
  function setCurrentTool(tool: AnnotationTool): void {
    currentTool.value = tool
  }

  /**
   * 更新工具样式
   * @param style 样式更新
   */
  function updateToolStyle(style: Partial<AnnotationStyle>): void {
    toolStyle.value = { ...toolStyle.value, ...style }
  }

  /**
   * 导出图像到文件
   *
   * 使用前端 Canvas 导出功能，通过 Tauri 文件系统 API 保存。
   * 需要传入标注画布引用。
   *
   * @param canvas 标注画布元素
   * @param format 导出格式
   * @returns 导出结果
   *
   * @validates Requirements 6.6
   * @validates Property 12: Image Export Integrity
   */
  async function exportImage(
    canvas: HTMLCanvasElement,
    format: ExportFormat = 'png'
  ): Promise<ExportResult> {
    if (!selectedRegion.value) {
      return {
        success: false,
        error: 'No region selected',
      }
    }

    try {
      captureState.value = 'exporting'
      lastError.value = null

      const result = await exportService.saveCanvasToFile(canvas, { format })

      if (!result.success) {
        lastError.value = result.error || 'Export failed'
      }

      return result
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : String(error)
      lastError.value = errorMessage
      return {
        success: false,
        error: errorMessage,
      }
    } finally {
      captureState.value = 'annotating'
    }
  }

  /**
   * 复制图像到剪贴板
   *
   * 使用前端 Canvas 导出功能，通过 Tauri 剪贴板 API 复制。
   * 需要传入标注画布引用。
   *
   * @param canvas 标注画布元素
   * @returns 导出结果
   *
   * @validates Requirements 6.6
   * @validates Property 12: Image Export Integrity
   */
  async function copyToClipboard(canvas: HTMLCanvasElement): Promise<ExportResult> {
    if (!selectedRegion.value) {
      return {
        success: false,
        error: 'No region selected',
      }
    }

    try {
      captureState.value = 'exporting'
      lastError.value = null

      const result = await exportService.copyCanvasToClipboard(canvas)

      if (!result.success) {
        lastError.value = result.error || 'Copy to clipboard failed'
      }

      return result
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : String(error)
      lastError.value = errorMessage
      return {
        success: false,
        error: errorMessage,
      }
    } finally {
      captureState.value = 'annotating'
    }
  }

  /**
   * 导出合并画布（背景 + 标注）
   *
   * 将背景图像和标注层合并后导出。
   * 处理高 DPI 场景，确保输出图像质量。
   *
   * @param backgroundCanvas 背景画布
   * @param annotationCanvas 标注画布
   * @param action 导出操作类型 ('save' | 'copy')
   * @param options 导出选项
   * @returns 导出结果
   *
   * @validates Property 12: Image Export Integrity
   * @validates Property 28: Annotation Coordinate System
   */
  async function exportMergedCanvas(
    backgroundCanvas: HTMLCanvasElement | null,
    annotationCanvas: HTMLCanvasElement,
    action: 'save' | 'copy' = 'copy',
    options?: Partial<ExportOptions>
  ): Promise<ExportResult> {
    if (!selectedRegion.value) {
      return {
        success: false,
        error: 'No region selected',
      }
    }

    try {
      captureState.value = 'exporting'
      lastError.value = null

      // 获取当前显示器的 DPR
      const monitor = monitors.value.find(m => m.id === selectedRegion.value?.monitorId)
      const dpr = monitor?.scaleFactor || window.devicePixelRatio || 1

      // 创建合并画布
      const mergedCanvas = createMergedCanvas(
        backgroundCanvas,
        annotationCanvas,
        selectedRegion.value,
        dpr
      )

      let result: ExportResult

      if (action === 'save') {
        result = await exportService.saveCanvasToFile(mergedCanvas, options)
      } else {
        result = await exportService.copyCanvasToClipboard(mergedCanvas)
      }

      if (!result.success) {
        lastError.value = result.error || 'Export failed'
      }

      return result
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : String(error)
      lastError.value = errorMessage
      return {
        success: false,
        error: errorMessage,
      }
    } finally {
      captureState.value = 'annotating'
    }
  }

  /**
   * 取消截图，重置状态
   */
  function cancelCapture(): void {
    captureState.value = 'idle'
    capturedImages.value.clear()
    selectedRegion.value = null
    detectedWindow.value = null
    annotations.value = []
    lastError.value = null
  }

  /**
   * 重置所有状态
   */
  function $reset(): void {
    captureState.value = 'idle'
    captureMode.value = 'region'
    isCapturing.value = false
    capturedImages.value.clear()
    monitors.value = []
    detectedWindow.value = null
    selectedRegion.value = null
    annotations.value = []
    currentTool.value = 'select'
    toolStyle.value = { ...DEFAULT_ANNOTATION_STYLE }
    lastError.value = null
  }

  return {
    // State
    captureState,
    captureMode,
    isCapturing,
    capturedImages,
    monitors,
    detectedWindow,
    selectedRegion,
    annotations,
    currentTool,
    toolStyle,
    lastError,

    // Getters
    hasSelection,
    hasAnnotations,
    selectedAnnotations,
    canAnnotate,

    // Actions
    startCapture,
    finishSelection,
    detectWindowAt,
    selectDetectedWindow,
    addAnnotation,
    updateAnnotation,
    removeAnnotation,
    clearAnnotations,
    setCurrentTool,
    updateToolStyle,
    exportImage,
    copyToClipboard,
    exportMergedCanvas,
    cancelCapture,
    $reset,
  }
})
