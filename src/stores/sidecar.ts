/**
 * Sidecar 状态管理 Store
 *
 * 管理 Python Sidecar 通信：
 * - Sidecar 进程状态
 * - OCR 服务调用
 * - 翻译服务调用
 * - Anki 服务调用
 * - 其他 Sidecar 服务
 *
 * @validates Requirements 7.1, 7.2, 7.3, 8.1, 9.1, 10.1
 */

import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type {
  OcrResult,
  OcrRecognizeParams,
  TranslateResult,
  TranslateParams,
  TranslateProvider,
  AnkiAddCardParams,
  AnkiAddCardResult,
  WebScrapeParams,
  WebScrapeResult,
  DocumentFormatParams,
  DocumentFormatResult,
  RecordStartParams,
  RecordResult,
  RecordingState,
  RegulationSearchParams,
  RegulationSearchResult,
  RegulationDownloadParams,
  RegulationDownloadResult,
  FileToMarkdownParams,
  FileToMarkdownResult,
  UrlToMarkdownParams,
  UrlToMarkdownResult,
  MarkdownToFileParams,
  MarkdownToFileResult,
} from '@/types'

export const useSidecarStore = defineStore('sidecar', () => {
  // ============================================
  // State
  // ============================================

  /** Sidecar 是否就绪 */
  const isReady = ref(false)

  /** 是否正在处理请求 */
  const isProcessing = ref(false)

  /** 当前处理的服务 */
  const currentService = ref<string | null>(null)

  /** 最后一次错误 */
  const lastError = ref<string | null>(null)

  /** Sidecar 重启次数 */
  const restartCount = ref(0)

  /** 录屏状态 */
  const recordingState = ref<RecordingState>('idle')

  /** 录屏时长 (秒) */
  const recordingDuration = ref(0)

  // ============================================
  // Getters
  // ============================================

  /** 是否可以调用服务 */
  const canCall = computed(() => isReady.value && !isProcessing.value)

  /** 是否正在录屏 */
  const isRecording = computed(() =>
    recordingState.value === 'recording' || recordingState.value === 'paused'
  )

  // ============================================
  // Actions
  // ============================================

  /**
   * 初始化 Sidecar
   */
  async function initialize(): Promise<void> {
    try {
      lastError.value = null
      await invoke('start_sidecar')
      isReady.value = true
      restartCount.value = 0
    } catch (error) {
      lastError.value = error instanceof Error ? error.message : String(error)
      isReady.value = false
      throw error
    }
  }

  /**
   * 停止 Sidecar
   */
  async function shutdown(): Promise<void> {
    try {
      await invoke('stop_sidecar')
      isReady.value = false
    } catch (error) {
      console.error('Failed to stop sidecar:', error)
    }
  }

  /**
   * 重启 Sidecar
   */
  async function restart(): Promise<void> {
    try {
      lastError.value = null
      await invoke('restart_sidecar')
      isReady.value = true
      restartCount.value++
    } catch (error) {
      lastError.value = error instanceof Error ? error.message : String(error)
      isReady.value = false
      throw error
    }
  }

  /**
   * 调用 OCR 服务
   * @param imagePath 图像文件路径
   * @param language 语言 (可选)
   */
  async function callOcr(
    imagePath: string,
    language?: string
  ): Promise<OcrResult> {
    if (!isReady.value) {
      throw new Error('Sidecar is not ready')
    }

    try {
      isProcessing.value = true
      currentService.value = 'ocr'
      lastError.value = null

      const params: OcrRecognizeParams = { imagePath, language }
      const result = await invoke<OcrResult>('call_sidecar', {
        service: 'ocr',
        method: 'recognize',
        params,
      })

      return result
    } catch (error) {
      lastError.value = error instanceof Error ? error.message : String(error)
      throw error
    } finally {
      isProcessing.value = false
      currentService.value = null
    }
  }

  /**
   * 调用翻译服务
   * @param text 待翻译文本
   * @param targetLang 目标语言
   * @param provider 翻译提供商 (可选)
   */
  async function callTranslate(
    text: string,
    targetLang: string,
    provider?: TranslateProvider
  ): Promise<TranslateResult> {
    if (!isReady.value) {
      throw new Error('Sidecar is not ready')
    }

    try {
      isProcessing.value = true
      currentService.value = 'translate'
      lastError.value = null

      const params: TranslateParams = { text, targetLang, provider }
      const result = await invoke<TranslateResult>('call_sidecar', {
        service: 'translate',
        method: 'translate',
        params,
      })

      return result
    } catch (error) {
      lastError.value = error instanceof Error ? error.message : String(error)
      throw error
    } finally {
      isProcessing.value = false
      currentService.value = null
    }
  }

  /**
   * 调用 Anki 制卡服务
   * @param card 卡片参数
   */
  async function callAnki(card: AnkiAddCardParams): Promise<AnkiAddCardResult> {
    if (!isReady.value) {
      throw new Error('Sidecar is not ready')
    }

    try {
      isProcessing.value = true
      currentService.value = 'anki'
      lastError.value = null

      const result = await invoke<AnkiAddCardResult>('call_sidecar', {
        service: 'anki',
        method: 'add_card',
        params: card,
      })

      return result
    } catch (error) {
      lastError.value = error instanceof Error ? error.message : String(error)
      throw error
    } finally {
      isProcessing.value = false
      currentService.value = null
    }
  }

  /**
   * 检查 Anki 连接状态
   */
  async function checkAnkiConnection(): Promise<boolean> {
    if (!isReady.value) {
      return false
    }

    try {
      const result = await invoke<boolean>('call_sidecar', {
        service: 'anki',
        method: 'check_connection',
        params: {},
      })
      return result
    } catch {
      return false
    }
  }

  /**
   * 获取 Anki 牌组列表
   */
  async function getAnkiDecks(): Promise<string[]> {
    if (!isReady.value) {
      throw new Error('Sidecar is not ready')
    }

    try {
      const result = await invoke<string[]>('call_sidecar', {
        service: 'anki',
        method: 'get_decks',
        params: {},
      })
      return result
    } catch (error) {
      lastError.value = error instanceof Error ? error.message : String(error)
      throw error
    }
  }

  /**
   * 调用网页爬取服务
   * @param params 爬取参数
   */
  async function callWebScrape(params: WebScrapeParams): Promise<WebScrapeResult> {
    if (!isReady.value) {
      throw new Error('Sidecar is not ready')
    }

    try {
      isProcessing.value = true
      currentService.value = 'web'
      lastError.value = null

      const result = await invoke<WebScrapeResult>('call_sidecar', {
        service: 'web',
        method: 'scrape',
        params,
      })

      return result
    } catch (error) {
      lastError.value = error instanceof Error ? error.message : String(error)
      throw error
    } finally {
      isProcessing.value = false
      currentService.value = null
    }
  }

  /**
   * 调用公文格式化服务
   * @param params 格式化参数
   */
  async function callDocumentFormat(
    params: DocumentFormatParams
  ): Promise<DocumentFormatResult> {
    if (!isReady.value) {
      throw new Error('Sidecar is not ready')
    }

    try {
      isProcessing.value = true
      currentService.value = 'document'
      lastError.value = null

      const result = await invoke<DocumentFormatResult>('call_sidecar', {
        service: 'document',
        method: 'format',
        params,
      })

      return result
    } catch (error) {
      lastError.value = error instanceof Error ? error.message : String(error)
      throw error
    } finally {
      isProcessing.value = false
      currentService.value = null
    }
  }

  /**
   * 获取打开的 Word/WPS 文档列表
   */
  async function getOpenDocuments(): Promise<{
    success: boolean
    documents: Array<{ name: string; full_path: string; app_type: string }>
    available: boolean
    error?: string
  }> {
    // 确保 Sidecar 运行（自动重启崩溃的 Sidecar）
    await ensureSidecarRunning()

    try {
      isProcessing.value = true
      currentService.value = 'document'
      lastError.value = null

      const result = await callWithRetry<{
        success: boolean
        documents: Array<{ name: string; full_path: string; app_type: string }>
        available: boolean
        error?: string
      }>('document', 'get_open_documents', {})

      return result
    } catch (error) {
      lastError.value = error instanceof Error ? error.message : String(error)
      throw error
    } finally {
      isProcessing.value = false
      currentService.value = null
    }
  }

  /**
   * 通过文档名称格式化文档
   * @param documentName 文档名称
   */
  async function formatDocumentByName(documentName: string): Promise<{
    success: boolean
    document_name: string
    app_type?: string
    message: string
    issues: string[]
    elapse?: number
  }> {
    // 确保 Sidecar 运行（自动重启崩溃的 Sidecar）
    await ensureSidecarRunning()

    try {
      isProcessing.value = true
      currentService.value = 'document'
      lastError.value = null

      const result = await callWithRetry<{
        success: boolean
        document_name: string
        app_type?: string
        message: string
        issues: string[]
        elapse?: number
      }>('document', 'format_by_name', { document_name: documentName })

      return result
    } catch (error) {
      lastError.value = error instanceof Error ? error.message : String(error)
      throw error
    } finally {
      isProcessing.value = false
      currentService.value = null
    }
  }

  /**
   * 开始录屏
   * @param params 录屏参数
   */
  async function startRecording(params: RecordStartParams): Promise<void> {
    if (!isReady.value) {
      throw new Error('Sidecar is not ready')
    }

    try {
      isProcessing.value = true
      currentService.value = 'record'
      lastError.value = null

      await invoke('start_recording', { params })

      recordingState.value = 'recording'
      recordingDuration.value = 0
    } catch (error) {
      lastError.value = error instanceof Error ? error.message : String(error)
      throw error
    } finally {
      isProcessing.value = false
      currentService.value = null
    }
  }

  /**
   * 暂停录屏
   */
  async function pauseRecording(): Promise<void> {
    if (recordingState.value !== 'recording') {
      return
    }

    try {
      await invoke('pause_recording')

      recordingState.value = 'paused'
    } catch (error) {
      lastError.value = error instanceof Error ? error.message : String(error)
      throw error
    }
  }

  /**
   * 恢复录屏
   */
  async function resumeRecording(): Promise<void> {
    if (recordingState.value !== 'paused') {
      return
    }

    try {
      await invoke('resume_recording')

      recordingState.value = 'recording'
    } catch (error) {
      lastError.value = error instanceof Error ? error.message : String(error)
      throw error
    }
  }

  /**
   * 停止录屏
   */
  async function stopRecording(): Promise<RecordResult> {
    if (!isRecording.value) {
      throw new Error('Not recording')
    }

    try {
      isProcessing.value = true
      currentService.value = 'record'
      recordingState.value = 'encoding'

      const result = await invoke<RecordResult>('stop_recording')

      recordingState.value = 'idle'
      recordingDuration.value = 0

      return result
    } catch (error) {
      lastError.value = error instanceof Error ? error.message : String(error)
      recordingState.value = 'idle'
      throw error
    } finally {
      isProcessing.value = false
      currentService.value = null
    }
  }

  /**
   * 更新录屏时长
   * @param duration 时长 (秒)
   */
  function updateRecordingDuration(duration: number): void {
    recordingDuration.value = duration
  }

  /**
   * 调用规章搜索服务
   * @param params 搜索参数
   */
  async function callRegulationSearch(
    params: RegulationSearchParams
  ): Promise<RegulationSearchResult> {
    if (!isReady.value) {
      throw new Error('Sidecar is not ready')
    }

    try {
      isProcessing.value = true
      currentService.value = 'regulation'
      lastError.value = null

      const result = await invoke<RegulationSearchResult>('call_sidecar', {
        service: 'regulation',
        method: 'search',
        params,
      })

      return result
    } catch (error) {
      lastError.value = error instanceof Error ? error.message : String(error)
      throw error
    } finally {
      isProcessing.value = false
      currentService.value = null
    }
  }

  /**
   * 全量爬取规章列表（用于同步对比）
   * @param docType 文档类型：all, regulation, normative
   * @param maxPages 最大爬取页数
   */
  async function callRegulationFetchAll(
    docType: string = 'all',
    maxPages: number = 20,
  ): Promise<RegulationSearchResult> {
    if (!isReady.value) {
      throw new Error('Sidecar is not ready')
    }

    try {
      isProcessing.value = true
      currentService.value = 'regulation'
      lastError.value = null

      const result = await invoke<RegulationSearchResult>('call_sidecar', {
        service: 'regulation',
        method: 'fetch_all',
        params: { doc_type: docType, max_pages: maxPages },
      })

      return result
    } catch (error) {
      lastError.value = error instanceof Error ? error.message : String(error)
      throw error
    } finally {
      isProcessing.value = false
      currentService.value = null
    }
  }

  /**
   * 调用规章下载服务
   * @param params 下载参数
   */
  async function callRegulationDownload(
    params: RegulationDownloadParams
  ): Promise<RegulationDownloadResult> {
    if (!isReady.value) {
      throw new Error('Sidecar is not ready')
    }

    try {
      isProcessing.value = true
      currentService.value = 'regulation'
      lastError.value = null

      const result = await invoke<RegulationDownloadResult>('call_sidecar', {
        service: 'regulation',
        method: 'download',
        params,
      })

      return result
    } catch (error) {
      lastError.value = error instanceof Error ? error.message : String(error)
      throw error
    } finally {
      isProcessing.value = false
      currentService.value = null
    }
  }

  // ============================================
  // 文件转换服务
  // ============================================

  /**
   * 确保 Sidecar 运行，如果崩溃则自动重启
   */
  async function ensureSidecarRunning(): Promise<void> {
    try {
      // 检查后端实际状态
      const isRunning = await invoke<boolean>('check_sidecar_status')
      if (!isRunning) {
        console.log('Sidecar 未运行或已崩溃，尝试启动...')
        isReady.value = false
        await initialize()
      }
    } catch (error) {
      console.warn('检查 Sidecar 状态失败，尝试重新启动:', error)
      isReady.value = false
      await initialize()
    }
  }

  /**
   * 带自动重试的 Sidecar 调用
   * 如果检测到 Sidecar 崩溃，会自动重启并重试一次
   */
  async function callWithRetry<T>(
    service: string,
    method: string,
    params: Record<string, unknown>
  ): Promise<T> {
    try {
      return await invoke<T>('call_sidecar', { service, method, params })
    } catch (error) {
      const errorMsg = error instanceof Error ? error.message : String(error)
      // 检测到 Sidecar 崩溃或未运行
      if (errorMsg.includes('Crashed') || errorMsg.includes('未运行') || errorMsg.includes('未初始化')) {
        console.log('检测到 Sidecar 异常，尝试重启...')
        isReady.value = false
        await restart()
        // 重试一次
        return await invoke<T>('call_sidecar', { service, method, params })
      }
      throw error
    }
  }

  /**
   * 文件转 Markdown
   * 支持 Word/PDF/Excel/PPT/HTML 等格式
   * @param filePath 文件路径
   * @param options 转换选项
   */
  async function fileToMarkdown(
    filePath: string,
    options?: FileToMarkdownParams['options']
  ): Promise<FileToMarkdownResult> {
    // 确保 Sidecar 运行
    await ensureSidecarRunning()

    try {
      isProcessing.value = true
      currentService.value = 'converter'
      lastError.value = null

      const result = await callWithRetry<FileToMarkdownResult>(
        'converter',
        'file_to_markdown',
        { file_path: filePath, options }
      )

      return result
    } catch (error) {
      lastError.value = error instanceof Error ? error.message : String(error)
      throw error
    } finally {
      isProcessing.value = false
      currentService.value = null
    }
  }

  /**
   * 网页转 Markdown
   * @param url 网页 URL
   * @param options 转换选项
   */
  async function urlToMarkdown(
    url: string,
    options?: UrlToMarkdownParams['options']
  ): Promise<UrlToMarkdownResult> {
    // 确保 Sidecar 运行
    await ensureSidecarRunning()

    try {
      isProcessing.value = true
      currentService.value = 'converter'
      lastError.value = null

      const result = await callWithRetry<UrlToMarkdownResult>(
        'converter',
        'url_to_markdown',
        { url, options }
      )

      return result
    } catch (error) {
      lastError.value = error instanceof Error ? error.message : String(error)
      throw error
    } finally {
      isProcessing.value = false
      currentService.value = null
    }
  }

  /**
   * Markdown 转文件
   * 支持 Word/PDF/HTML 等格式
   * @param markdown Markdown 内容
   * @param outputPath 输出文件路径
   * @param format 输出格式 (docx, pdf, html, odt, rtf)
   * @param options 转换选项
   */
  async function markdownToFile(
    markdown: string,
    outputPath: string,
    format: MarkdownToFileParams['format'] = 'docx',
    options?: MarkdownToFileParams['options']
  ): Promise<MarkdownToFileResult> {
    // 确保 Sidecar 运行
    await ensureSidecarRunning()

    try {
      isProcessing.value = true
      currentService.value = 'converter'
      lastError.value = null

      const result = await callWithRetry<MarkdownToFileResult>(
        'converter',
        'markdown_to_file',
        { markdown, output_path: outputPath, format, options }
      )

      return result
    } catch (error) {
      lastError.value = error instanceof Error ? error.message : String(error)
      throw error
    } finally {
      isProcessing.value = false
      currentService.value = null
    }
  }

  /**
   * Markdown 文件转其他格式
   * @param markdownPath Markdown 文件路径
   * @param outputPath 输出文件路径
   * @param format 输出格式
   * @param options 转换选项
   */
  async function markdownFileToFile(
    markdownPath: string,
    outputPath: string,
    format: MarkdownToFileParams['format'] = 'docx',
    options?: MarkdownToFileParams['options']
  ): Promise<MarkdownToFileResult> {
    // 确保 Sidecar 运行
    await ensureSidecarRunning()

    try {
      isProcessing.value = true
      currentService.value = 'converter'
      lastError.value = null

      const result = await callWithRetry<MarkdownToFileResult>(
        'converter',
        'markdown_file_to_file',
        {
          markdown_path: markdownPath,
          output_path: outputPath,
          format,
          options,
        }
      )

      return result
    } catch (error) {
      lastError.value = error instanceof Error ? error.message : String(error)
      throw error
    } finally {
      isProcessing.value = false
      currentService.value = null
    }
  }

  /**
   * 清除错误
   */
  function clearError(): void {
    lastError.value = null
  }

  /**
   * 重置所有状态
   */
  function $reset(): void {
    isReady.value = false
    isProcessing.value = false
    currentService.value = null
    lastError.value = null
    restartCount.value = 0
    recordingState.value = 'idle'
    recordingDuration.value = 0
  }

  return {
    // State
    isReady,
    isProcessing,
    currentService,
    lastError,
    restartCount,
    recordingState,
    recordingDuration,

    // Getters
    canCall,
    isRecording,

    // Actions
    initialize,
    shutdown,
    restart,
    callOcr,
    callTranslate,
    callAnki,
    checkAnkiConnection,
    getAnkiDecks,
    callWebScrape,
    callDocumentFormat,
    getOpenDocuments,
    formatDocumentByName,
    startRecording,
    pauseRecording,
    resumeRecording,
    stopRecording,
    updateRecordingDuration,
    callRegulationSearch,
    callRegulationFetchAll,
    callRegulationDownload,
    fileToMarkdown,
    urlToMarkdown,
    markdownToFile,
    markdownFileToFile,
    clearError,
    $reset,
  }
})
