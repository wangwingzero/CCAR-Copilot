/**
 * 公文格式化功能组合式函数
 * 
 * 提供公文格式化功能的封装：
 * - 调用 Sidecar 公文格式化服务
 * - 管理加载状态
 * - 错误处理
 * - 文件路径验证
 * 
 * @validates Requirements 12.1, 12.2
 */

import { ref, computed, ComputedRef } from 'vue'
import { useSidecarStore } from '@/stores/sidecar'
import type { DocumentFormatParams, DocumentFormatResult } from '@/types'

export interface UseDocumentFormatterOptions {
  /** 是否自动初始化 Sidecar */
  autoInit?: boolean
}

export interface UseDocumentFormatterReturn {
  /** 格式化结果 */
  formatResult: ReturnType<typeof ref<DocumentFormatResult | null>>
  /** 是否正在加载 */
  isLoading: ReturnType<typeof ref<boolean>>
  /** 错误信息 */
  error: ReturnType<typeof ref<string | null>>
  /** 是否有结果 */
  hasResult: ComputedRef<boolean>
  /** 是否完全符合标准 */
  isCompliant: ComputedRef<boolean>
  /** 格式化文档 */
  format: (inputPath: string, outputPath?: string) => Promise<DocumentFormatResult | null>
  /** 清除结果 */
  clearResult: () => void
  /** 验证文件路径 */
  validateFilePath: (path: string) => boolean
  /** 获取输出路径 */
  getOutputPath: () => string
  /** 获取问题列表 */
  getIssues: () => string[]
}

/**
 * 公文格式化功能组合式函数
 */
export function useDocumentFormatter(options: UseDocumentFormatterOptions = {}): UseDocumentFormatterReturn {
  const { autoInit = true } = options
  
  const sidecarStore = useSidecarStore()
  
  // ============================================
  // State
  // ============================================
  
  /** 格式化结果 */
  const formatResult = ref<DocumentFormatResult | null>(null)
  
  /** 是否正在加载 */
  const isLoading = ref(false)
  
  /** 错误信息 */
  const error = ref<string | null>(null)
  
  // ============================================
  // Computed
  // ============================================
  
  /** 是否有结果 */
  const hasResult = computed(() => 
    formatResult.value !== null && formatResult.value.outputPath.length > 0
  )
  
  /** 是否完全符合标准 */
  const isCompliant = computed(() => 
    formatResult.value?.compliant ?? false
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
        throw new Error('无法启动公文格式化服务，请检查 Python Sidecar 是否正确安装')
      }
    }
    
    if (!sidecarStore.isReady) {
      throw new Error('公文格式化服务未就绪')
    }
  }
  
  /**
   * 验证文件路径格式
   * @param path 文件路径
   * @returns 是否有效
   */
  function validateFilePath(path: string): boolean {
    if (!path || path.trim().length === 0) {
      return false
    }
    
    // 检查是否为 Word 文档
    const lowerPath = path.toLowerCase()
    return lowerPath.endsWith('.docx') || lowerPath.endsWith('.doc')
  }
  
  /**
   * 格式化文档
   * @param inputPath 输入文件路径
   * @param outputPath 输出文件路径 (可选)
   * @returns 格式化结果
   */
  async function format(
    inputPath: string, 
    outputPath?: string
  ): Promise<DocumentFormatResult | null> {
    // 验证文件路径
    if (!validateFilePath(inputPath)) {
      error.value = '请选择有效的 Word 文档（.doc 或 .docx 格式）'
      return null
    }
    
    try {
      isLoading.value = true
      error.value = null
      
      // 确保 Sidecar 已初始化
      await ensureSidecar()
      
      // 构建参数
      const params: DocumentFormatParams = {
        inputPath: inputPath.trim(),
        outputPath: outputPath?.trim(),
      }
      
      // 调用格式化服务
      const result = await sidecarStore.callDocumentFormat(params)
      
      formatResult.value = result
      return result
      
    } catch (e) {
      const errorMessage = e instanceof Error ? e.message : String(e)
      error.value = errorMessage
      console.error('Document formatting failed:', e)
      return null
      
    } finally {
      isLoading.value = false
    }
  }
  
  /**
   * 清除格式化结果
   */
  function clearResult(): void {
    formatResult.value = null
    error.value = null
  }
  
  /**
   * 获取输出文件路径
   */
  function getOutputPath(): string {
    return formatResult.value?.outputPath ?? ''
  }
  
  /**
   * 获取问题列表
   */
  function getIssues(): string[] {
    return formatResult.value?.issues ?? []
  }
  
  return {
    formatResult,
    isLoading,
    error,
    hasResult,
    isCompliant,
    format,
    clearResult,
    validateFilePath,
    getOutputPath,
    getIssues,
  }
}
