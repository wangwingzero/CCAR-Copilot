/**
 * 文件搜索状态 Store
 *
 * 管理文件搜索对话框的状态持久化
 */

import { ref, computed } from 'vue'
import { defineStore } from 'pinia'

/**
 * 搜索结果项
 */
export interface FileSearchResult {
  /** 文件名 */
  name: string
  /** 完整路径 */
  path: string
  /** 文件大小（字节） */
  size: number
  /** 修改时间（Unix 时间戳秒） */
  modifiedSecs: number
  /** 是否为目录 */
  isDirectory: boolean
  /** 匹配分数 */
  score: number
  /** 匹配位置索引 */
  matchIndices: [number, number][]
}

/** 状态过期时间（5分钟） */
const STATE_EXPIRY_MS = 5 * 60 * 1000

/** localStorage key */
const STORAGE_KEY = 'file-search-state'

/**
 * 文件搜索 Store
 */
export const useFileSearchStore = defineStore('fileSearch', () => {
  const query = ref('')
  const results = ref<FileSearchResult[]>([])
  const totalCount = ref(0)
  const searchTimeMs = ref(0)
  const selectedIndex = ref(0)
  const lastSearchTime = ref(0)

  const hasValidState = computed(() => {
    if (!query.value || results.value.length === 0) return false
    return Date.now() - lastSearchTime.value < STATE_EXPIRY_MS
  })

  function saveState(
    newQuery: string,
    newResults: FileSearchResult[],
    newTotalCount: number,
    newSearchTimeMs: number,
    newSelectedIndex: number = 0,
  ): void {
    query.value = newQuery
    results.value = newResults
    totalCount.value = newTotalCount
    searchTimeMs.value = newSearchTimeMs
    selectedIndex.value = newSelectedIndex
    lastSearchTime.value = Date.now()
    saveToStorage()
  }

  function setSelectedIndex(index: number): void {
    if (index >= 0 && index < results.value.length) {
      selectedIndex.value = index
    }
  }

  function clearState(): void {
    query.value = ''
    results.value = []
    totalCount.value = 0
    searchTimeMs.value = 0
    selectedIndex.value = 0
    lastSearchTime.value = 0
    clearStorage()
  }

  function getState() {
    return {
      query: query.value,
      results: results.value,
      totalCount: totalCount.value,
      searchTimeMs: searchTimeMs.value,
      selectedIndex: selectedIndex.value,
      lastSearchTime: lastSearchTime.value,
    }
  }

  function saveToStorage(): void {
    try {
      const data = {
        query: query.value,
        lastSearchTime: lastSearchTime.value,
      }
      localStorage.setItem(STORAGE_KEY, JSON.stringify(data))
    } catch {
      // ignore
    }
  }

  function loadFromStorage(): string {
    try {
      const data = localStorage.getItem(STORAGE_KEY)
      if (data) {
        const parsed = JSON.parse(data)
        if (Date.now() - parsed.lastSearchTime < STATE_EXPIRY_MS) {
          return parsed.query || ''
        }
      }
    } catch {
      // ignore
    }
    return ''
  }

  function clearStorage(): void {
    try {
      localStorage.removeItem(STORAGE_KEY)
    } catch {
      // ignore
    }
  }

  function initialize(): string {
    const savedQuery = loadFromStorage()
    if (savedQuery) {
      query.value = savedQuery
    }
    return savedQuery
  }

  return {
    query,
    results,
    totalCount,
    searchTimeMs,
    selectedIndex,
    lastSearchTime,
    hasValidState,
    saveState,
    setSelectedIndex,
    clearState,
    getState,
    initialize,
  }
})
