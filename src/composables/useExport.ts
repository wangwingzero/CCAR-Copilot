/**
 * 图像导出 Composable
 *
 * 提供 Vue 组件中使用的图像导出功能：
 * - 保存到文件
 * - 复制到剪贴板
 * - 导出状态管理
 * - 用户反馈
 *
 * @validates Requirements 6.6, 18.4
 * @validates Property 12: Image Export Integrity
 * @validates Property 28: Annotation Coordinate System
 */

import { ref, computed, type Ref } from 'vue'
import { BaseDirectory } from '@tauri-apps/plugin-fs'
import {
  exportService,
  quickSaveImage,
  copyImageToClipboard,
  exportCanvasToBlob,
  createMergedCanvas,
  type ExportOptions,
  type ExportResult,
} from '@/services/exportService'
import type { SelectionRegion } from '@/types'

/** 导出状态 */
export type ExportState = 'idle' | 'exporting' | 'success' | 'error'

/** 导出操作类型 */
export type ExportAction = 'save' | 'copy' | 'quickSave'

/** useExport 返回类型 */
export interface UseExportReturn {
  /** 当前导出状态 */
  state: Ref<ExportState>
  /** 是否正在导出 */
  isExporting: Ref<boolean>
  /** 最后一次导出结果 */
  lastResult: Ref<ExportResult | null>
  /** 最后一次错误信息 */
  lastError: Ref<string | null>
  /** 保存到文件 */
  saveToFile: (
    canvas: HTMLCanvasElement,
    options?: Partial<ExportOptions>
  ) => Promise<ExportResult>
  /** 快速保存到图片目录 */
  quickSave: (
    canvas: HTMLCanvasElement,
    options?: Partial<ExportOptions>
  ) => Promise<ExportResult>
  /** 复制到剪贴板 */
  copyToClipboard: (canvas: HTMLCanvasElement) => Promise<ExportResult>
  /** 导出合并画布（背景 + 标注） */
  exportMerged: (
    backgroundCanvas: HTMLCanvasElement | null,
    annotationCanvas: HTMLCanvasElement,
    region?: SelectionRegion,
    dpr?: number,
    action?: ExportAction,
    options?: Partial<ExportOptions>
  ) => Promise<ExportResult>
  /** 重置状态 */
  reset: () => void
}

/**
 * 图像导出 Composable
 *
 * 使用示例：
 * ```typescript
 * const { saveToFile, copyToClipboard, isExporting, lastError } = useExport()
 *
 * // 保存到文件
 * const result = await saveToFile(canvasRef.value)
 * if (result.success) {
 *   console.log('Saved to:', result.path)
 * }
 *
 * // 复制到剪贴板
 * await copyToClipboard(canvasRef.value)
 * ```
 */
export function useExport(): UseExportReturn {
  // ============================================
  // State
  // ============================================

  /** 当前导出状态 */
  const state = ref<ExportState>('idle')

  /** 最后一次导出结果 */
  const lastResult = ref<ExportResult | null>(null)

  /** 最后一次错误信息 */
  const lastError = ref<string | null>(null)

  // ============================================
  // Computed
  // ============================================

  /** 是否正在导出 */
  const isExporting = computed(() => state.value === 'exporting')

  // ============================================
  // Methods
  // ============================================

  /**
   * 开始导出操作
   */
  function startExport(): void {
    state.value = 'exporting'
    lastError.value = null
  }

  /**
   * 完成导出操作
   */
  function finishExport(result: ExportResult): void {
    lastResult.value = result
    if (result.success) {
      state.value = 'success'
    } else {
      state.value = 'error'
      lastError.value = result.error || 'Unknown error'
    }
  }

  /**
   * 保存到文件
   *
   * 显示保存对话框让用户选择保存位置。
   *
   * @param canvas Canvas 元素
   * @param options 导出选项
   * @returns 导出结果
   */
  async function saveToFile(
    canvas: HTMLCanvasElement,
    options?: Partial<ExportOptions>
  ): Promise<ExportResult> {
    startExport()

    try {
      const result = await exportService.saveCanvasToFile(canvas, options)
      finishExport(result)
      return result
    } catch (error) {
      const errorMessage =
        error instanceof Error ? error.message : String(error)
      const result: ExportResult = {
        success: false,
        error: errorMessage,
      }
      finishExport(result)
      return result
    }
  }

  /**
   * 快速保存到图片目录
   *
   * 不显示对话框，直接保存到用户的图片目录。
   *
   * @param canvas Canvas 元素
   * @param options 导出选项
   * @returns 导出结果
   */
  async function quickSave(
    canvas: HTMLCanvasElement,
    options?: Partial<ExportOptions>
  ): Promise<ExportResult> {
    startExport()

    try {
      const format = options?.format || 'png'
      const quality = options?.quality || 0.92
      const blob = await exportCanvasToBlob(canvas, format, quality)
      const result = await quickSaveImage(blob, BaseDirectory.Picture, options)
      finishExport(result)
      return result
    } catch (error) {
      const errorMessage =
        error instanceof Error ? error.message : String(error)
      const result: ExportResult = {
        success: false,
        error: errorMessage,
      }
      finishExport(result)
      return result
    }
  }

  /**
   * 复制到剪贴板
   *
   * @param canvas Canvas 元素
   * @returns 导出结果
   */
  async function copyToClipboard(
    canvas: HTMLCanvasElement
  ): Promise<ExportResult> {
    startExport()

    try {
      const result = await exportService.copyCanvasToClipboard(canvas)
      finishExport(result)
      return result
    } catch (error) {
      const errorMessage =
        error instanceof Error ? error.message : String(error)
      const result: ExportResult = {
        success: false,
        error: errorMessage,
      }
      finishExport(result)
      return result
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
   * @param region 选区信息
   * @param dpr 设备像素比
   * @param action 导出操作类型
   * @param options 导出选项
   * @returns 导出结果
   *
   * @validates Property 12: Image Export Integrity
   * @validates Property 28: Annotation Coordinate System
   */
  async function exportMerged(
    backgroundCanvas: HTMLCanvasElement | null,
    annotationCanvas: HTMLCanvasElement,
    region?: SelectionRegion,
    dpr: number = 1,
    action: ExportAction = 'copy',
    options?: Partial<ExportOptions>
  ): Promise<ExportResult> {
    startExport()

    try {
      // 创建合并画布
      const mergedCanvas = createMergedCanvas(
        backgroundCanvas,
        annotationCanvas,
        region,
        dpr
      )

      let result: ExportResult

      switch (action) {
        case 'save':
          result = await exportService.saveCanvasToFile(mergedCanvas, options)
          break
        case 'quickSave': {
          const format = options?.format || 'png'
          const quality = options?.quality || 0.92
          const blob = await exportCanvasToBlob(mergedCanvas, format, quality)
          result = await quickSaveImage(blob, BaseDirectory.Picture, options)
          break
        }
        case 'copy':
        default: {
          // 剪贴板需要 PNG 格式
          const blob = await exportCanvasToBlob(mergedCanvas, 'png', 1)
          result = await copyImageToClipboard(blob)
          break
        }
      }

      finishExport(result)
      return result
    } catch (error) {
      const errorMessage =
        error instanceof Error ? error.message : String(error)
      const result: ExportResult = {
        success: false,
        error: errorMessage,
      }
      finishExport(result)
      return result
    }
  }

  /**
   * 重置状态
   */
  function reset(): void {
    state.value = 'idle'
    lastResult.value = null
    lastError.value = null
  }

  return {
    state,
    isExporting,
    lastResult,
    lastError,
    saveToFile,
    quickSave,
    copyToClipboard,
    exportMerged,
    reset,
  }
}

/**
 * 高 DPI 坐标转换工具
 *
 * 遵循「逻辑坐标负责交互，物理像素负责输出」原则
 */
export const dprUtils = {
  /**
   * 逻辑像素转物理像素
   * @param logical 逻辑像素值
   * @param dpr 设备像素比
   * @returns 物理像素值
   */
  toPhysical(logical: number, dpr: number): number {
    return Math.round(logical * dpr)
  },

  /**
   * 物理像素转逻辑像素
   * @param physical 物理像素值
   * @param dpr 设备像素比
   * @returns 逻辑像素值
   */
  toLogical(physical: number, dpr: number): number {
    return physical / dpr
  },

  /**
   * 获取当前设备像素比
   * @returns 设备像素比
   */
  getCurrentDpr(): number {
    return window.devicePixelRatio || 1
  },
}
