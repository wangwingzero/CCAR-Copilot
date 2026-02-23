/**
 * Anki 制卡功能组合式函数
 * 
 * 提供 Anki 制卡功能的封装：
 * - 检查 Anki 连接状态
 * - 获取牌组和笔记类型列表
 * - 创建 Anki 卡片
 * - 管理加载状态和错误处理
 * 
 * @validates Requirements 10.1, 10.3
 */

import { ref, computed, ComputedRef } from 'vue'
import { useSidecarStore } from '@/stores/sidecar'
import type { AnkiAddCardParams, AnkiAddCardResult, AnkiCardFields } from '@/types'

export interface UseAnkiOptions {
  /** 是否自动初始化 Sidecar */
  autoInit?: boolean
  /** 默认牌组名称 */
  defaultDeck?: string
  /** 默认笔记类型 */
  defaultNoteType?: string
}

export interface UseAnkiReturn {
  /** 是否已连接 Anki */
  isConnected: ReturnType<typeof ref<boolean>>
  /** 是否正在加载 */
  isLoading: ReturnType<typeof ref<boolean>>
  /** 是否正在检查连接 */
  isCheckingConnection: ReturnType<typeof ref<boolean>>
  /** 错误信息 */
  error: ReturnType<typeof ref<string | null>>
  /** 可用的牌组列表 */
  decks: ReturnType<typeof ref<string[]>>
  /** 可用的笔记类型列表 */
  noteTypes: ReturnType<typeof ref<string[]>>
  /** 当前选择的牌组 */
  currentDeck: ReturnType<typeof ref<string>>
  /** 当前选择的笔记类型 */
  currentNoteType: ReturnType<typeof ref<string>>
  /** 最后创建的卡片结果 */
  lastResult: ReturnType<typeof ref<AnkiAddCardResult | null>>
  /** 是否可以创建卡片 */
  canCreateCard: ComputedRef<boolean>
  /** 检查 Anki 连接 */
  checkConnection: () => Promise<boolean>
  /** 加载牌组列表 */
  loadDecks: () => Promise<string[]>
  /** 加载笔记类型列表 */
  loadNoteTypes: () => Promise<string[]>
  /** 创建卡片 */
  createCard: (params: AnkiAddCardParams) => Promise<AnkiAddCardResult | null>
  /** 快速创建图片卡片 */
  createImageCard: (imagePath: string, text?: string, tags?: string[]) => Promise<AnkiAddCardResult | null>
  /** 设置当前牌组 */
  setDeck: (deck: string) => void
  /** 设置当前笔记类型 */
  setNoteType: (noteType: string) => void
  /** 清除错误 */
  clearError: () => void
  /** 重置状态 */
  reset: () => void
}

/** 默认牌组名称 */
const DEFAULT_DECK = '默认'

/** 默认笔记类型 */
const DEFAULT_NOTE_TYPE = '基本型'

/** 虎哥截图专用笔记类型 */
const HUGE_NOTE_TYPE = '虎哥截图'

/**
 * Anki 制卡功能组合式函数
 */
export function useAnki(options: UseAnkiOptions = {}): UseAnkiReturn {
  const { 
    autoInit = true, 
    defaultDeck = DEFAULT_DECK,
    defaultNoteType = DEFAULT_NOTE_TYPE
  } = options
  
  const sidecarStore = useSidecarStore()
  
  // ============================================
  // State
  // ============================================
  
  /** 是否已连接 Anki */
  const isConnected = ref(false)
  
  /** 是否正在加载 */
  const isLoading = ref(false)
  
  /** 是否正在检查连接 */
  const isCheckingConnection = ref(false)
  
  /** 错误信息 */
  const error = ref<string | null>(null)
  
  /** 可用的牌组列表 */
  const decks = ref<string[]>([])
  
  /** 可用的笔记类型列表 */
  const noteTypes = ref<string[]>([])
  
  /** 当前选择的牌组 */
  const currentDeck = ref<string>(defaultDeck)
  
  /** 当前选择的笔记类型 */
  const currentNoteType = ref<string>(defaultNoteType)
  
  /** 最后创建的卡片结果 */
  const lastResult = ref<AnkiAddCardResult | null>(null)
  
  // ============================================
  // Computed
  // ============================================
  
  /** 是否可以创建卡片 */
  const canCreateCard = computed(() => 
    isConnected.value && 
    !isLoading.value && 
    currentDeck.value.length > 0 &&
    currentNoteType.value.length > 0
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
        throw new Error('无法启动 Anki 服务，请检查 Python Sidecar 是否正确安装')
      }
    }
    
    if (!sidecarStore.isReady) {
      throw new Error('Anki 服务未就绪')
    }
  }
  
  /**
   * 检查 Anki 连接状态
   * @returns 是否已连接
   */
  async function checkConnection(): Promise<boolean> {
    try {
      isCheckingConnection.value = true
      error.value = null
      
      // 确保 Sidecar 已初始化
      await ensureSidecar()
      
      // 检查 Anki 连接
      const connected = await sidecarStore.checkAnkiConnection()
      isConnected.value = connected
      
      if (!connected) {
        error.value = '无法连接到 Anki，请确保 Anki 已启动并安装了 AnkiConnect 插件'
      }
      
      return connected
      
    } catch (e) {
      const errorMessage = e instanceof Error ? e.message : String(e)
      error.value = errorMessage
      isConnected.value = false
      console.error('Anki connection check failed:', e)
      return false
      
    } finally {
      isCheckingConnection.value = false
    }
  }
  
  /**
   * 加载牌组列表
   * @returns 牌组列表
   */
  async function loadDecks(): Promise<string[]> {
    try {
      isLoading.value = true
      error.value = null
      
      // 确保 Sidecar 已初始化
      await ensureSidecar()
      
      // 获取牌组列表
      const deckList = await sidecarStore.getAnkiDecks()
      decks.value = deckList
      
      // 如果当前牌组不在列表中，选择第一个
      if (deckList.length > 0 && !deckList.includes(currentDeck.value)) {
        currentDeck.value = deckList[0]
      }
      
      return deckList
      
    } catch (e) {
      const errorMessage = e instanceof Error ? e.message : String(e)
      error.value = errorMessage
      console.error('Failed to load Anki decks:', e)
      return []
      
    } finally {
      isLoading.value = false
    }
  }
  
  /**
   * 加载笔记类型列表
   * @returns 笔记类型列表
   */
  async function loadNoteTypes(): Promise<string[]> {
    try {
      isLoading.value = true
      error.value = null
      
      // 确保 Sidecar 已初始化
      await ensureSidecar()
      
      // 暂时使用默认笔记类型列表
      // TODO: 从 Anki 获取实际的笔记类型列表
      const defaultNoteTypes = [DEFAULT_NOTE_TYPE, HUGE_NOTE_TYPE, '基本型（含反向卡片）', '填空题']
      noteTypes.value = defaultNoteTypes
      
      return defaultNoteTypes
      
    } catch {
      // 如果获取失败，使用默认列表
      const defaultNoteTypes = [DEFAULT_NOTE_TYPE, HUGE_NOTE_TYPE]
      noteTypes.value = defaultNoteTypes
      return defaultNoteTypes
      
    } finally {
      isLoading.value = false
    }
  }
  
  /**
   * 创建 Anki 卡片
   * @param params 卡片参数
   * @returns 创建结果
   */
  async function createCard(params: AnkiAddCardParams): Promise<AnkiAddCardResult | null> {
    if (!isConnected.value) {
      error.value = '请先连接到 Anki'
      return null
    }
    
    try {
      isLoading.value = true
      error.value = null
      
      // 确保 Sidecar 已初始化
      await ensureSidecar()
      
      // 创建卡片
      const result = await sidecarStore.callAnki(params)
      lastResult.value = result
      
      return result
      
    } catch (e) {
      const errorMessage = e instanceof Error ? e.message : String(e)
      error.value = errorMessage
      console.error('Failed to create Anki card:', e)
      return null
      
    } finally {
      isLoading.value = false
    }
  }
  
  /**
   * 快速创建图片卡片
   * @param imagePath 图片路径
   * @param text OCR 识别的文字（可选）
   * @param tags 标签（可选）
   * @returns 创建结果
   */
  async function createImageCard(
    imagePath: string, 
    text?: string, 
    tags?: string[]
  ): Promise<AnkiAddCardResult | null> {
    const fields: AnkiCardFields = {
      '正面': text || '',
      '背面': '',
    }
    
    // 如果使用虎哥截图笔记类型，使用不同的字段
    if (currentNoteType.value === HUGE_NOTE_TYPE) {
      fields['图片'] = ''  // 图片会通过 imagePath 参数传递
      fields['文字'] = text || ''
    }
    
    const params: AnkiAddCardParams = {
      deck: currentDeck.value,
      noteType: currentNoteType.value,
      fields,
      tags: tags || [],
      imagePath,
    }
    
    return createCard(params)
  }
  
  /**
   * 设置当前牌组
   */
  function setDeck(deck: string): void {
    currentDeck.value = deck
  }
  
  /**
   * 设置当前笔记类型
   */
  function setNoteType(noteType: string): void {
    currentNoteType.value = noteType
  }
  
  /**
   * 清除错误
   */
  function clearError(): void {
    error.value = null
  }
  
  /**
   * 重置状态
   */
  function reset(): void {
    isConnected.value = false
    isLoading.value = false
    isCheckingConnection.value = false
    error.value = null
    decks.value = []
    noteTypes.value = []
    currentDeck.value = defaultDeck
    currentNoteType.value = defaultNoteType
    lastResult.value = null
  }
  
  return {
    isConnected,
    isLoading,
    isCheckingConnection,
    error,
    decks,
    noteTypes,
    currentDeck,
    currentNoteType,
    lastResult,
    canCreateCard,
    checkConnection,
    loadDecks,
    loadNoteTypes,
    createCard,
    createImageCard,
    setDeck,
    setNoteType,
    clearError,
    reset,
  }
}
