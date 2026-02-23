/**
 * OCR 功能组合式函数
 * 
 * 提供 OCR 识别功能的封装：
 * - 调用 Sidecar OCR 服务
 * - 管理加载状态
 * - 错误处理
 * - 结果缓存
 * 
 * @validates Requirements 8.1, 8.4
 */

import { ref, computed, ComputedRef } from 'vue'
import { useSidecarStore } from '@/stores/sidecar'
import type { OcrResult, OcrTextBox } from '@/types'

export interface UseOcrOptions {
  /** 是否自动初始化 Sidecar */
  autoInit?: boolean
}

export interface UseOcrReturn {
  /** OCR 结果 */
  ocrResult: ReturnType<typeof ref<OcrResult | null>>
  /** 是否正在加载 */
  isLoading: ReturnType<typeof ref<boolean>>
  /** 错误信息 */
  error: ReturnType<typeof ref<string | null>>
  /** 是否有结果 */
  hasResult: ComputedRef<boolean>
  /** 识别图像 */
  recognize: (imagePath: string) => Promise<OcrResult | null>
  /** 清除结果 */
  clearResult: () => void
  /** 获取文字框文本 */
  getBoxText: (box: OcrTextBox) => string
  /** 获取全部文本 */
  getAllText: () => string
}

/**
 * OCR 功能组合式函数
 */
export function useOcr(options: UseOcrOptions = {}): UseOcrReturn {
  const { autoInit = true } = options
  
  const sidecarStore = useSidecarStore()
  
  // ============================================
  // State
  // ============================================
  
  /** OCR 结果 */
  const ocrResult = ref<OcrResult | null>(null)
  
  /** 是否正在加载 */
  const isLoading = ref(false)
  
  /** 错误信息 */
  const error = ref<string | null>(null)
  
  // ============================================
  // Computed
  // ============================================
  
  /** 是否有结果 */
  const hasResult = computed(() => 
    ocrResult.value !== null && ocrResult.value.boxes.length > 0
  )
  
  // ============================================
  // Methods
  // ============================================
  
  /**
   * 确保 Sidecar 已初始化
   */
  async function ensureSidecar(): Promise<void> {
    if (!sidecarStore.isReady && autoInit) {
      try {
        await sidecarStore.initialize()
      } catch (e) {
        throw new Error('无法启动 OCR 服务，请检查 Python Sidecar 是否正确安装')
      }
    }
    
    if (!sidecarStore.isReady) {
      throw new Error('OCR 服务未就绪')
    }
  }
  
  /**
   * 识别图像中的文字
   * @param imagePath 图像文件路径
   * @returns OCR 结果
   */
  async function recognize(imagePath: string): Promise<OcrResult | null> {
    if (!imagePath) {
      error.value = '图像路径不能为空'
      return null
    }
    
    try {
      isLoading.value = true
      error.value = null
      
      // 确保 Sidecar 已初始化
      await ensureSidecar()
      
      // 调用 OCR 服务
      const result = await sidecarStore.callOcr(imagePath)
      
      ocrResult.value = result
      return result
      
    } catch (e) {
      const errorMessage = e instanceof Error ? e.message : String(e)
      error.value = errorMessage
      console.error('OCR recognition failed:', e)
      return null
      
    } finally {
      isLoading.value = false
    }
  }
  
  /**
   * 清除 OCR 结果
   */
  function clearResult(): void {
    ocrResult.value = null
    error.value = null
  }
  
  /**
   * 获取文字框文本
   */
  function getBoxText(box: OcrTextBox): string {
    return box.text
  }
  
  /**
   * 获取全部文本
   */
  function getAllText(): string {
    return ocrResult.value?.text ?? ''
  }
  
  return {
    ocrResult,
    isLoading,
    error,
    hasResult,
    recognize,
    clearResult,
    getBoxText,
    getAllText,
  }
}
