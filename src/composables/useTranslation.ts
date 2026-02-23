/**
 * 翻译功能组合式函数
 * 
 * 提供翻译功能的封装：
 * - 调用 Sidecar 翻译服务
 * - 管理加载状态
 * - 错误处理
 * - 支持多个翻译提供商
 * 
 * @validates Requirements 9.1, 9.2
 */

import { ref, computed, ComputedRef } from 'vue'
import { useSidecarStore } from '@/stores/sidecar'
import type { TranslateResult, TranslateProvider } from '@/types'

export interface UseTranslationOptions {
  /** 是否自动初始化 Sidecar */
  autoInit?: boolean
  /** 默认目标语言 */
  defaultTargetLang?: string
  /** 默认翻译提供商 */
  defaultProvider?: TranslateProvider
}

export interface UseTranslationReturn {
  /** 翻译结果 */
  translationResult: ReturnType<typeof ref<TranslateResult | null>>
  /** 是否正在加载 */
  isLoading: ReturnType<typeof ref<boolean>>
  /** 错误信息 */
  error: ReturnType<typeof ref<string | null>>
  /** 是否有结果 */
  hasResult: ComputedRef<boolean>
  /** 当前选择的提供商 */
  currentProvider: ReturnType<typeof ref<TranslateProvider>>
  /** 当前目标语言 */
  currentTargetLang: ReturnType<typeof ref<string>>
  /** 可用的翻译提供商 */
  availableProviders: TranslateProvider[]
  /** 可用的目标语言 */
  availableTargetLangs: { code: string; name: string }[]
  /** 翻译文本 */
  translate: (text: string, targetLang?: string, provider?: TranslateProvider) => Promise<TranslateResult | null>
  /** 清除结果 */
  clearResult: () => void
  /** 设置提供商 */
  setProvider: (provider: TranslateProvider) => void
  /** 设置目标语言 */
  setTargetLang: (lang: string) => void
  /** 获取翻译后的文本 */
  getTranslatedText: () => string
}

/** 可用的翻译提供商 */
const AVAILABLE_PROVIDERS: TranslateProvider[] = ['google', 'deepl', 'baidu']

/** 可用的目标语言 */
const AVAILABLE_TARGET_LANGS = [
  { code: 'zh', name: '中文' },
  { code: 'en', name: 'English' },
  { code: 'ja', name: '日本語' },
  { code: 'ko', name: '한국어' },
  { code: 'fr', name: 'Français' },
  { code: 'de', name: 'Deutsch' },
  { code: 'es', name: 'Español' },
  { code: 'ru', name: 'Русский' },
]

/**
 * 翻译功能组合式函数
 */
export function useTranslation(options: UseTranslationOptions = {}): UseTranslationReturn {
  const { 
    autoInit = true, 
    defaultTargetLang = 'zh',
    defaultProvider = 'google'
  } = options
  
  const sidecarStore = useSidecarStore()
  
  // ============================================
  // State
  // ============================================
  
  /** 翻译结果 */
  const translationResult = ref<TranslateResult | null>(null)
  
  /** 是否正在加载 */
  const isLoading = ref(false)
  
  /** 错误信息 */
  const error = ref<string | null>(null)
  
  /** 当前选择的提供商 */
  const currentProvider = ref<TranslateProvider>(defaultProvider)
  
  /** 当前目标语言 */
  const currentTargetLang = ref<string>(defaultTargetLang)
  
  // ============================================
  // Computed
  // ============================================
  
  /** 是否有结果 */
  const hasResult = computed(() => 
    translationResult.value !== null && translationResult.value.translatedText.length > 0
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
        throw new Error('无法启动翻译服务，请检查 Python Sidecar 是否正确安装')
      }
    }
    
    if (!sidecarStore.isReady) {
      throw new Error('翻译服务未就绪')
    }
  }
  
  /**
   * 翻译文本
   * @param text 待翻译文本
   * @param targetLang 目标语言（可选，使用当前设置）
   * @param provider 翻译提供商（可选，使用当前设置）
   * @returns 翻译结果
   */
  async function translate(
    text: string, 
    targetLang?: string, 
    provider?: TranslateProvider
  ): Promise<TranslateResult | null> {
    if (!text || !text.trim()) {
      error.value = '翻译文本不能为空'
      return null
    }
    
    const finalTargetLang = targetLang ?? currentTargetLang.value
    const finalProvider = provider ?? currentProvider.value
    
    try {
      isLoading.value = true
      error.value = null
      
      // 确保 Sidecar 已初始化
      await ensureSidecar()
      
      // 调用翻译服务
      const result = await sidecarStore.callTranslate(text, finalTargetLang, finalProvider)
      
      translationResult.value = result
      return result
      
    } catch (e) {
      const errorMessage = e instanceof Error ? e.message : String(e)
      error.value = errorMessage
      console.error('Translation failed:', e)
      return null
      
    } finally {
      isLoading.value = false
    }
  }
  
  /**
   * 清除翻译结果
   */
  function clearResult(): void {
    translationResult.value = null
    error.value = null
  }
  
  /**
   * 设置翻译提供商
   */
  function setProvider(provider: TranslateProvider): void {
    currentProvider.value = provider
  }
  
  /**
   * 设置目标语言
   */
  function setTargetLang(lang: string): void {
    currentTargetLang.value = lang
  }
  
  /**
   * 获取翻译后的文本
   */
  function getTranslatedText(): string {
    return translationResult.value?.translatedText ?? ''
  }
  
  return {
    translationResult,
    isLoading,
    error,
    hasResult,
    currentProvider,
    currentTargetLang,
    availableProviders: AVAILABLE_PROVIDERS,
    availableTargetLangs: AVAILABLE_TARGET_LANGS,
    translate,
    clearResult,
    setProvider,
    setTargetLang,
    getTranslatedText,
  }
}
