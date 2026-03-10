/**
 * 设置状态管理 Store
 *
 * 管理应用配置：
 * - 通用设置
 * - 通知设置
 * - 更新设置
 * - 高级设置
 */

import { defineStore } from 'pinia'
import { ref, computed, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type {
  AppConfig,
  GeneralConfig,
  NotificationConfig,
  UpdateConfig,
  AdvancedConfig,
} from '@/types'
import { DEFAULT_CONFIG } from '@/types'

export const useSettingsStore = defineStore('settings', () => {
  // ============================================
  // State
  // ============================================

  const config = ref<AppConfig>(structuredClone(DEFAULT_CONFIG))
  const isLoaded = ref(false)
  const isDirty = ref(false)
  const isSaving = ref(false)
  const lastError = ref<string | null>(null)

  let autoSaveTimer: ReturnType<typeof setTimeout> | null = null
  const AUTO_SAVE_DELAY_MS = 500

  // ============================================
  // 自动保存逻辑
  // ============================================

  function triggerAutoSave(): void {
    if (autoSaveTimer) {
      clearTimeout(autoSaveTimer)
    }

    autoSaveTimer = setTimeout(async () => {
      if (isDirty.value && !isSaving.value) {
        try {
          await saveConfig()
        } catch (error) {
          console.error('自动保存失败:', error)
        }
      }
    }, AUTO_SAVE_DELAY_MS)
  }

  watch(isDirty, (newValue) => {
    if (newValue && isLoaded.value) {
      triggerAutoSave()
    }
  })

  // ============================================
  // Getters
  // ============================================

  const general = computed(() => config.value.general)
  const notification = computed(() => config.value.notification)
  const update = computed(() => config.value.update)
  const advanced = computed(() => config.value.advanced)

  const currentTheme = computed(() => {
    if (config.value.general.theme === 'system') {
      if (typeof window !== 'undefined' && window.matchMedia) {
        return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
      }
      return 'light'
    }
    return config.value.general.theme
  })

  const currentLanguage = computed(() => config.value.general.language)

  // ============================================
  // Actions
  // ============================================

  async function loadConfig(): Promise<void> {
    try {
      lastError.value = null
      const loadedConfig = await invoke<AppConfig>('load_config')
      config.value = loadedConfig
      isLoaded.value = true
      isDirty.value = false
    } catch (error) {
      lastError.value = error instanceof Error ? error.message : String(error)
      config.value = structuredClone(DEFAULT_CONFIG)
      isLoaded.value = true
      throw error
    }
  }

  async function saveConfig(): Promise<void> {
    try {
      isSaving.value = true
      lastError.value = null
      await invoke('save_config', { config: config.value })
      isDirty.value = false
    } catch (error) {
      lastError.value = error instanceof Error ? error.message : String(error)
      throw error
    } finally {
      isSaving.value = false
    }
  }

  function updateGeneral(updates: Partial<GeneralConfig>): void {
    config.value.general = { ...config.value.general, ...updates }
    isDirty.value = true
  }

  function updateNotification(updates: Partial<NotificationConfig>): void {
    config.value.notification = { ...config.value.notification, ...updates }
    isDirty.value = true
  }

  function updateUpdate(updates: Partial<UpdateConfig>): void {
    config.value.update = { ...config.value.update, ...updates }
    isDirty.value = true
  }

  function updateAdvanced(updates: Partial<AdvancedConfig>): void {
    config.value.advanced = { ...config.value.advanced, ...updates }
    isDirty.value = true
  }

  function resetToDefault(): void {
    config.value = structuredClone(DEFAULT_CONFIG)
    isDirty.value = true
  }

  function resetSection(section: keyof AppConfig): void {
    config.value = { ...config.value, [section]: structuredClone(DEFAULT_CONFIG[section]) }
    isDirty.value = true
  }

  async function exportConfig(filePath: string): Promise<void> {
    try {
      lastError.value = null
      await invoke('export_config', { filePath, config: config.value })
    } catch (error) {
      lastError.value = error instanceof Error ? error.message : String(error)
      throw error
    }
  }

  async function importConfig(filePath: string): Promise<void> {
    try {
      lastError.value = null
      const importedConfig = await invoke<AppConfig>('import_config', { filePath })
      config.value = importedConfig
      isDirty.value = true
    } catch (error) {
      lastError.value = error instanceof Error ? error.message : String(error)
      throw error
    }
  }

  async function setAutoStart(enabled: boolean): Promise<void> {
    try {
      lastError.value = null
      await invoke('set_auto_start', { enabled })
      config.value.general.autoStart = enabled
      isDirty.value = true
    } catch (error) {
      lastError.value = error instanceof Error ? error.message : String(error)
      throw error
    }
  }

  async function checkAutoStart(): Promise<boolean> {
    try {
      const enabled = await invoke<boolean>('check_auto_start')
      config.value.general.autoStart = enabled
      return enabled
    } catch (error) {
      console.error('Failed to check auto start:', error)
      return false
    }
  }

  function $reset(): void {
    if (autoSaveTimer) {
      clearTimeout(autoSaveTimer)
      autoSaveTimer = null
    }
    config.value = structuredClone(DEFAULT_CONFIG)
    isLoaded.value = false
    isDirty.value = false
    isSaving.value = false
    lastError.value = null
  }

  return {
    // State
    config,
    isLoaded,
    isDirty,
    isSaving,
    lastError,

    // Getters
    general,
    notification,
    update,
    advanced,
    currentTheme,
    currentLanguage,

    // Actions
    loadConfig,
    saveConfig,
    updateGeneral,
    updateNotification,
    updateUpdate,
    updateAdvanced,
    resetToDefault,
    resetSection,
    exportConfig,
    importConfig,
    setAutoStart,
    checkAutoStart,
    $reset,
  }
})
