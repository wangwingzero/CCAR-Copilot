/**
 * 单词卡功能 Composable（v2 - 原生 Rust 实现，不依赖 Python Sidecar）
 *
 * 所有 AnkiConnect 操作通过 Rust 原生 HTTP 客户端完成，
 * 消除了 Python Sidecar 启动和通信的不稳定性。
 */

import { ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'

// ============================================
// 类型定义
// ============================================

export interface WordQueryResult {
  word: string
  phonetic: string
  definition: string
  audio_path: string | null
  audio_filename: string | null
  image_path: string | null
  image_filename: string | null
}

export interface ImportWordResult {
  word: string
  success: boolean
  status: string
  note_id?: number | null
  index?: number
  total?: number
}

export interface ImportResult {
  success_count: number
  total_count: number
  results: ImportWordResult[]
}

export interface ExtractWordsResult {
  words: string[]
}

/** AnkiConnect 连接状态 */
interface AnkiConnectionStatus {
  connected: boolean
  version?: number | null
  error?: string | null
}

// ============================================
// Composable
// ============================================

export function useWordCard() {
  const isImporting = ref(false)
  const importProgress = ref(0)
  const importTotal = ref(0)
  const importCurrentWord = ref('')
  const error = ref<string | null>(null)
  const lastImportResult = ref<ImportResult | null>(null)

  const progressPercent = computed(() => {
    if (importTotal.value === 0) return 0
    return Math.round((importProgress.value / importTotal.value) * 100)
  })

  /**
   * 从文本中提取英文单词（原生 Rust 实现）
   */
  async function extractEnglishWords(text: string): Promise<string[]> {
    try {
      const result = await invoke<string[]>('extract_english_words_native', { text })
      return result || []
    } catch (e) {
      console.error('[WordCard] 提取单词失败:', e)
      return []
    }
  }

  /**
   * 批量导入单词到 Anki（原生 Rust 实现）
   */
  async function importWords(
    words: string[],
    deckName: string,
    screenshotPath?: string
  ): Promise<ImportResult | null> {
    if (isImporting.value) {
      error.value = '正在导入中，请等待完成'
      return null
    }

    try {
      isImporting.value = true
      importProgress.value = 0
      importTotal.value = words.length
      importCurrentWord.value = ''
      error.value = null

      const result = await invoke<ImportResult>('import_words_to_anki', {
        words,
        deckName,
        screenshotPath: screenshotPath || null,
      })

      lastImportResult.value = result
      importProgress.value = result.total_count
      return result
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e)
      error.value = msg
      console.error('[WordCard] 导入失败:', e)
      return null
    } finally {
      isImporting.value = false
      importCurrentWord.value = ''
    }
  }

  /**
   * 确保单词卡模板存在
   */
  async function ensureModel(): Promise<boolean> {
    try {
      await invoke('ensure_anki_model')
      return true
    } catch (e) {
      console.error('[WordCard] 确保模板失败:', e)
      return false
    }
  }

  /**
   * 检查 Anki 连接（原生 Rust 实现，直接 HTTP 调用 AnkiConnect）
   */
  async function checkConnection(): Promise<boolean> {
    try {
      const result = await invoke<AnkiConnectionStatus>('check_anki_connection')
      return result.connected
    } catch (e) {
      console.warn('[WordCard] 连接检查失败:', e)
      return false
    }
  }

  /**
   * 获取牌组列表（原生 Rust 实现）
   */
  async function getDecks(): Promise<string[]> {
    try {
      return await invoke<string[]>('get_anki_decks')
    } catch (e) {
      console.warn('[WordCard] 获取牌组失败:', e)
      return []
    }
  }

  function clearError() {
    error.value = null
  }

  return {
    // State
    isImporting,
    importProgress,
    importTotal,
    importCurrentWord,
    error,
    lastImportResult,
    progressPercent,
    // Methods
    extractEnglishWords,
    importWords,
    ensureModel,
    checkConnection,
    getDecks,
    clearError,
  }
}
