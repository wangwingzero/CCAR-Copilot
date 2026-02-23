/**
 * 剪贴板历史 Store
 *
 * 管理剪贴板历史记录，支持：
 * - 自动监听剪贴板变化（轮询方式）
 * - 存储文本和图片内容
 * - 历史记录搜索和过滤
 * - 置顶（PIN）功能
 */

import { ref, computed } from 'vue'
import { defineStore } from 'pinia'
import {
  readText as clipboardReadText,
  writeText as clipboardWriteText,
} from '@tauri-apps/plugin-clipboard-manager'

/**
 * 剪贴板条目类型
 */
export type ClipboardItemType = 'text' | 'image'

/**
 * 剪贴板历史条目
 */
export interface ClipboardHistoryItem {
  /** 唯一 ID */
  id: string
  /** 类型 */
  type: ClipboardItemType
  /** 文本内容（type 为 text 时有效） */
  text?: string
  /** 图片数据 URL（type 为 image 时有效） */
  imageDataUrl?: string
  /** 预览文本（用于显示） */
  preview: string
  /** 创建时间 */
  createdAt: Date
  /** 是否置顶 */
  pinned: boolean
}

/**
 * 剪贴板历史 Store
 */
export const useClipboardHistoryStore = defineStore('clipboardHistory', () => {
  // 状态
  const items = ref<ClipboardHistoryItem[]>([])
  const isMonitoring = ref(false)
  const lastContent = ref<string | null>(null)
  const maxItems = ref(100)

  // 监听定时器
  let monitorInterval: ReturnType<typeof setInterval> | null = null

  // 计算属性：按置顶和时间排序
  const sortedItems = computed(() => {
    return [...items.value].sort((a, b) => {
      // 置顶的排在前面
      if (a.pinned && !b.pinned) return -1
      if (!a.pinned && b.pinned) return 1
      // 按时间倒序
      return b.createdAt.getTime() - a.createdAt.getTime()
    })
  })

  // 计算属性：文本条目
  const textItems = computed(() => {
    return sortedItems.value.filter((item) => item.type === 'text')
  })

  // 计算属性：图片条目
  const imageItems = computed(() => {
    return sortedItems.value.filter((item) => item.type === 'image')
  })

  /**
   * 生成唯一 ID
   */
  function generateId(): string {
    return `${Date.now()}-${Math.random().toString(36).substr(2, 9)}`
  }

  /**
   * 生成预览文本
   */
  function generatePreview(text: string, maxLength = 100): string {
    const trimmed = text.trim().replace(/\s+/g, ' ')
    if (trimmed.length <= maxLength) {
      return trimmed
    }
    return trimmed.substring(0, maxLength) + '...'
  }

  /**
   * 添加条目
   */
  function addItem(item: Omit<ClipboardHistoryItem, 'id' | 'createdAt' | 'pinned'>): void {
    // 检查是否重复
    const isDuplicate = items.value.some((existing) => {
      if (item.type === 'text' && existing.type === 'text') {
        return existing.text === item.text
      }
      return false
    })

    if (isDuplicate) {
      return
    }

    const newItem: ClipboardHistoryItem = {
      ...item,
      id: generateId(),
      createdAt: new Date(),
      pinned: false,
    }

    items.value.unshift(newItem)

    // 限制最大条目数
    if (items.value.length > maxItems.value) {
      // 删除未置顶的最旧条目
      const unpinnedItems = items.value.filter((i) => !i.pinned)
      if (unpinnedItems.length > 0) {
        const oldest = unpinnedItems[unpinnedItems.length - 1]
        const index = items.value.findIndex((i) => i.id === oldest.id)
        if (index !== -1) {
          items.value.splice(index, 1)
        }
      }
    }

    // 保存到本地存储
    saveToStorage()
  }

  /**
   * 删除条目
   */
  function removeItem(id: string): void {
    const index = items.value.findIndex((item) => item.id === id)
    if (index !== -1) {
      items.value.splice(index, 1)
      saveToStorage()
    }
  }

  /**
   * 清空历史
   */
  function clearHistory(): void {
    // 保留置顶的条目
    items.value = items.value.filter((item) => item.pinned)
    saveToStorage()
  }

  /**
   * 切换置顶状态
   */
  function togglePin(id: string): void {
    const item = items.value.find((i) => i.id === id)
    if (item) {
      item.pinned = !item.pinned
      saveToStorage()
    }
  }

  /**
   * 复制条目到剪贴板
   */
  async function copyItem(id: string): Promise<void> {
    const item = items.value.find((i) => i.id === id)
    if (!item) return

    if (item.type === 'text' && item.text) {
      // 临时停止监听，避免重复添加
      const wasMonitoring = isMonitoring.value
      stopMonitoring()

      await clipboardWriteText(item.text)
      lastContent.value = item.text

      // 恢复监听
      if (wasMonitoring) {
        setTimeout(() => {
          startMonitoring()
        }, 500)
      }
    }
    // 图片复制需要不同的处理方式
  }

  /**
   * 搜索历史
   */
  function searchItems(query: string): ClipboardHistoryItem[] {
    if (!query.trim()) {
      return sortedItems.value
    }

    const lowerQuery = query.toLowerCase()
    return sortedItems.value.filter((item) => {
      if (item.type === 'text' && item.text) {
        return item.text.toLowerCase().includes(lowerQuery)
      }
      return false
    })
  }

  /**
   * 检查剪贴板内容变化
   */
  async function checkClipboardChange(): Promise<void> {
    try {
      // 检查文本内容
      const text = await clipboardReadText()
      if (text && text !== lastContent.value) {
        lastContent.value = text
        addItem({
          type: 'text',
          text,
          preview: generatePreview(text),
        })
      }

      // 检查图片内容（如果支持）
      // 注意：Tauri clipboard-manager 的图片读取可能需要特殊处理
      // const image = await clipboardReadImage()
      // if (image) {
      //   // 处理图片
      // }
    } catch (e) {
      // 读取失败时静默忽略
      console.debug('剪贴板读取失败:', e)
    }
  }

  /**
   * 开始监听剪贴板
   */
  function startMonitoring(): void {
    if (isMonitoring.value) return

    isMonitoring.value = true

    // 立即检查一次
    checkClipboardChange()

    // 每 500ms 检查一次
    monitorInterval = setInterval(checkClipboardChange, 500)
  }

  /**
   * 停止监听剪贴板
   */
  function stopMonitoring(): void {
    if (monitorInterval) {
      clearInterval(monitorInterval)
      monitorInterval = null
    }
    isMonitoring.value = false
  }

  /**
   * 保存到本地存储
   */
  function saveToStorage(): void {
    try {
      const data = items.value.map((item) => ({
        ...item,
        createdAt: item.createdAt.toISOString(),
      }))
      localStorage.setItem('clipboard-history', JSON.stringify(data))
    } catch (e) {
      console.error('保存剪贴板历史失败:', e)
    }
  }

  /**
   * 从本地存储加载
   */
  function loadFromStorage(): void {
    try {
      const data = localStorage.getItem('clipboard-history')
      if (data) {
        const parsed = JSON.parse(data)
        items.value = parsed.map((item: any) => ({
          ...item,
          createdAt: new Date(item.createdAt),
        }))
      }
    } catch (e) {
      console.error('加载剪贴板历史失败:', e)
    }
  }

  /**
   * 初始化
   */
  function initialize(): void {
    loadFromStorage()
  }

  return {
    // 状态
    items,
    sortedItems,
    textItems,
    imageItems,
    isMonitoring,
    maxItems,

    // 方法
    addItem,
    removeItem,
    clearHistory,
    togglePin,
    copyItem,
    searchItems,
    startMonitoring,
    stopMonitoring,
    initialize,
  }
})
