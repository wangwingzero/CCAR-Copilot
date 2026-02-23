/**
 * 网页爬取功能组合式函数
 * 
 * 提供网页爬取功能的封装：
 * - 调用 Sidecar 网页爬取服务
 * - 管理加载状态
 * - 错误处理
 * - URL 验证
 * 
 * @validates Requirements 11.1, 11.2
 */

import { ref, computed, ComputedRef } from 'vue'
import { useSidecarStore } from '@/stores/sidecar'
import type { WebScrapeParams, WebScrapeResult } from '@/types'

export interface UseWebScraperOptions {
  /** 是否自动初始化 Sidecar */
  autoInit?: boolean
  /** 是否下载图片 */
  downloadImages?: boolean
}

export interface UseWebScraperReturn {
  /** 爬取结果 */
  scrapeResult: ReturnType<typeof ref<WebScrapeResult | null>>
  /** 是否正在加载 */
  isLoading: ReturnType<typeof ref<boolean>>
  /** 错误信息 */
  error: ReturnType<typeof ref<string | null>>
  /** 是否有结果 */
  hasResult: ComputedRef<boolean>
  /** 爬取网页 */
  scrape: (url: string, options?: Partial<WebScrapeParams>) => Promise<WebScrapeResult | null>
  /** 清除结果 */
  clearResult: () => void
  /** 验证 URL */
  validateUrl: (url: string) => boolean
  /** 获取 Markdown 内容 */
  getMarkdown: () => string
  /** 获取标题 */
  getTitle: () => string
  /** 获取图片列表 */
  getImages: () => string[]
}

/**
 * 网页爬取功能组合式函数
 */
export function useWebScraper(options: UseWebScraperOptions = {}): UseWebScraperReturn {
  const { autoInit = true, downloadImages = true } = options
  
  const sidecarStore = useSidecarStore()
  
  // ============================================
  // State
  // ============================================
  
  /** 爬取结果 */
  const scrapeResult = ref<WebScrapeResult | null>(null)
  
  /** 是否正在加载 */
  const isLoading = ref(false)
  
  /** 错误信息 */
  const error = ref<string | null>(null)
  
  // ============================================
  // Computed
  // ============================================
  
  /** 是否有结果 */
  const hasResult = computed(() => 
    scrapeResult.value !== null && scrapeResult.value.markdown.length > 0
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
        throw new Error('无法启动网页爬取服务，请检查 Python Sidecar 是否正确安装')
      }
    }
    
    if (!sidecarStore.isReady) {
      throw new Error('网页爬取服务未就绪')
    }
  }
  
  /**
   * 验证 URL 格式
   * @param url URL 字符串
   * @returns 是否有效
   */
  function validateUrl(url: string): boolean {
    if (!url || url.trim().length === 0) {
      return false
    }
    
    try {
      const urlObj = new URL(url)
      return urlObj.protocol === 'http:' || urlObj.protocol === 'https:'
    } catch {
      return false
    }
  }
  
  /**
   * 爬取网页
   * @param url 目标 URL
   * @param scrapeOptions 爬取选项
   * @returns 爬取结果
   */
  async function scrape(
    url: string, 
    scrapeOptions?: Partial<WebScrapeParams>
  ): Promise<WebScrapeResult | null> {
    // 验证 URL
    if (!validateUrl(url)) {
      error.value = '请输入有效的 URL（以 http:// 或 https:// 开头）'
      return null
    }
    
    try {
      isLoading.value = true
      error.value = null
      
      // 确保 Sidecar 已初始化
      await ensureSidecar()
      
      // 构建参数
      const params: WebScrapeParams = {
        url: url.trim(),
        downloadImages: scrapeOptions?.downloadImages ?? downloadImages,
        outputDir: scrapeOptions?.outputDir,
      }
      
      // 调用爬取服务
      const result = await sidecarStore.callWebScrape(params)
      
      scrapeResult.value = result
      return result
      
    } catch (e) {
      const errorMessage = e instanceof Error ? e.message : String(e)
      error.value = errorMessage
      console.error('Web scraping failed:', e)
      return null
      
    } finally {
      isLoading.value = false
    }
  }
  
  /**
   * 清除爬取结果
   */
  function clearResult(): void {
    scrapeResult.value = null
    error.value = null
  }
  
  /**
   * 获取 Markdown 内容
   */
  function getMarkdown(): string {
    return scrapeResult.value?.markdown ?? ''
  }
  
  /**
   * 获取标题
   */
  function getTitle(): string {
    return scrapeResult.value?.title ?? ''
  }
  
  /**
   * 获取图片列表
   */
  function getImages(): string[] {
    return scrapeResult.value?.images ?? []
  }
  
  return {
    scrapeResult,
    isLoading,
    error,
    hasResult,
    scrape,
    clearResult,
    validateUrl,
    getMarkdown,
    getTitle,
    getImages,
  }
}
