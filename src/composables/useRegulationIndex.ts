/**
 * 规章本地索引 Composable
 *
 * 提供规章本地全文搜索功能，使用 Rust Tantivy 引擎。
 * 搜索已下载的规章文档，实现毫秒级响应。
 */

import { ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type {
  RegulationDocument,
  RegulationDocType,
  RegulationValidity,
} from '@/types'

// 本地索引专用类型（与 Rust 命令对应）
interface RegulationSearchRequest {
  query: string
  validity?: string
  doc_type?: string
  start_date?: string
  end_date?: string
  scan_folders?: string[]
  limit?: number
  sort?: string
}

interface RegulationSearchResponse {
  documents: RegulationDocument[]
  total: number
  elapsed_ms: number
  snippets?: (string | null)[]
}

interface RegulationIndexStats {
  doc_count: number
  index_path: string
  initialized: boolean
}

type RegulationSortOrder = 'relevance' | 'date_desc' | 'date_asc' | 'title_asc'

/** 索引状态 */
const isInitialized = ref(false)
const isInitializing = ref(false)
const indexStats = ref<RegulationIndexStats | null>(null)

/** 搜索状态 */
const isSearching = ref(false)
const searchResults = ref<RegulationDocument[]>([])
const searchElapsedMs = ref(0)
const lastError = ref<string | null>(null)

/** 摘要映射（按 doc.url → snippet） */
const snippetMap = ref<Map<string, string>>(new Map())

/**
 * 规章本地索引 Composable
 */
export function useRegulationIndex() {
  /**
   * 初始化索引
   */
  async function initIndex(): Promise<boolean> {
    if (isInitialized.value || isInitializing.value) {
      return isInitialized.value
    }

    isInitializing.value = true
    lastError.value = null

    try {
      const stats = await invoke<RegulationIndexStats>('regulation_index_init')
      indexStats.value = stats
      isInitialized.value = stats.initialized
      console.warn(`[RegulationIndex] 初始化完成，文档数: ${stats.doc_count}`)
      return true
    } catch (error) {
      lastError.value = String(error)
      console.error('[RegulationIndex] 初始化失败:', error)
      return false
    } finally {
      isInitializing.value = false
    }
  }

  /**
   * 本地搜索规章
   */
  async function localSearch(
    query: string,
    options: {
      validity?: RegulationValidity
      docType?: RegulationDocType
      startDate?: string
      endDate?: string
      scanFolders?: string[]
      limit?: number
      sort?: RegulationSortOrder
    } = {}
  ): Promise<RegulationDocument[]> {
    if (!isInitialized.value) {
      await initIndex()
    }

    if (!query.trim()) {
      searchResults.value = []
      return []
    }

    isSearching.value = true
    lastError.value = null

    try {
      const request: RegulationSearchRequest = {
        query,
        validity: options.validity || 'all',
        doc_type: options.docType || 'all',
        start_date: options.startDate || '',
        end_date: options.endDate || '',
        scan_folders: options.scanFolders || [],
        limit: options.limit || 100,
        sort: options.sort || 'relevance',
      }

      const response = await invoke<RegulationSearchResponse>('regulation_local_search', {
        request,
      })

      searchResults.value = response.documents
      searchElapsedMs.value = response.elapsed_ms

      // 构建摘要映射
      const newMap = new Map<string, string>()
      if (response.snippets) {
        response.documents.forEach((doc, i) => {
          const snippet = response.snippets![i]
          if (snippet) {
            newMap.set(doc.url, snippet)
          }
        })
      }
      snippetMap.value = newMap

      console.warn(
        `[RegulationIndex] 搜索 "${query}" 返回 ${response.total} 条结果，耗时 ${response.elapsed_ms}ms`
      )

      return response.documents
    } catch (error) {
      lastError.value = String(error)
      console.error('[RegulationIndex] 搜索失败:', error)
      return []
    } finally {
      isSearching.value = false
    }
  }

  /**
   * 添加文档到索引
   */
  async function addDocument(document: RegulationDocument): Promise<boolean> {
    if (!isInitialized.value) {
      await initIndex()
    }

    try {
      const added = await invoke<boolean>('regulation_index_add', { document })
      if (added) {
        // 更新统计信息
        await refreshStats()
      }
      return added
    } catch (error) {
      lastError.value = String(error)
      console.error('[RegulationIndex] 添加文档失败:', error)
      return false
    }
  }

  /**
   * 批量添加文档到索引
   */
  async function addDocuments(documents: RegulationDocument[]): Promise<number> {
    if (!isInitialized.value) {
      await initIndex()
    }

    try {
      const count = await invoke<number>('regulation_index_add_batch', { documents })
      if (count > 0) {
        await refreshStats()
      }
      return count
    } catch (error) {
      lastError.value = String(error)
      console.error('[RegulationIndex] 批量添加失败:', error)
      return 0
    }
  }

  /**
   * 检查文档是否已索引
   */
  async function isDocumentIndexed(url: string): Promise<boolean> {
    if (!isInitialized.value) {
      return false
    }

    try {
      return await invoke<boolean>('regulation_index_exists', { url })
    } catch (error) {
      console.error('[RegulationIndex] 检查文档失败:', error)
      return false
    }
  }

  /**
   * 刷新索引统计信息
   */
  async function refreshStats(): Promise<RegulationIndexStats | null> {
    try {
      const stats = await invoke<RegulationIndexStats>('regulation_index_stats')
      indexStats.value = stats
      return stats
    } catch (error) {
      console.error('[RegulationIndex] 获取统计信息失败:', error)
      return null
    }
  }

  /**
   * 清空索引
   */
  async function clearIndex(): Promise<boolean> {
    try {
      await invoke('regulation_index_clear')
      await refreshStats()
      searchResults.value = []
      return true
    } catch (error) {
      lastError.value = String(error)
      console.error('[RegulationIndex] 清空索引失败:', error)
      return false
    }
  }

  return {
    // 状态
    isInitialized: computed(() => isInitialized.value),
    isInitializing: computed(() => isInitializing.value),
    isSearching: computed(() => isSearching.value),
    indexStats: computed(() => indexStats.value),
    searchResults: computed(() => searchResults.value),
    searchElapsedMs: computed(() => searchElapsedMs.value),
    lastError: computed(() => lastError.value),
    docCount: computed(() => indexStats.value?.doc_count ?? 0),
    snippetMap: computed(() => snippetMap.value),

    // 方法
    initIndex,
    localSearch,
    addDocument,
    addDocuments,
    isDocumentIndexed,
    refreshStats,
    clearIndex,
  }
}
