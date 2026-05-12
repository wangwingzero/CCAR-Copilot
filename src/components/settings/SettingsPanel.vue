<script setup lang="ts">
/**
 * 设置面板组件 - 侧边栏布局版本
 *
 * 支持的设置类别：
 * - 基础设置：通用
 * - 系统设置：通知、更新、关于
 */

import { ref, reactive, onMounted, watch, toRaw } from 'vue'
import { useSettingsStore } from '@/stores/settings'
import { useTheme } from '@/composables/useTheme'
import { useLocale } from '@/composables/useLocale'
import type { AppConfig } from '@/types'
import { DEFAULT_CONFIG } from '@/types'
import SettingsSidebar from './SettingsSidebar.vue'
import NotificationSection from './sections/NotificationSection.vue'
import UpdateSection from './sections/UpdateSection.vue'
import AdvancedSection from './sections/AdvancedSection.vue'
import AboutSection from './sections/AboutSection.vue'

// ============================================
// Store & Composables
// ============================================

const settingsStore = useSettingsStore()
const { setTheme } = useTheme()
const { supportedLocales, changeLocale } = useLocale()

// ============================================
// State
// ============================================

const activeCategory = ref('general')
const localConfig = reactive<AppConfig>(structuredClone(DEFAULT_CONFIG))
const showResetConfirm = ref(false)

const settingsToast = reactive({
  visible: false,
  message: '',
  type: 'error' as 'success' | 'error',
})
let settingsToastTimer: ReturnType<typeof setTimeout> | null = null

function showSettingsToast(message: string, type: 'success' | 'error' = 'error'): void {
  if (settingsToastTimer) clearTimeout(settingsToastTimer)
  settingsToast.message = message
  settingsToast.type = type
  settingsToast.visible = true
  settingsToastTimer = setTimeout(() => {
    settingsToast.visible = false
  }, 3000)
}

// ============================================
// Lifecycle
// ============================================

onMounted(async () => {
  await loadSettings()
})

watch(
  () => settingsStore.config,
  (newConfig) => {
    Object.assign(localConfig, structuredClone(toRaw(newConfig)))
  },
  { deep: true }
)

// ============================================
// Methods
// ============================================

async function loadSettings(): Promise<void> {
  try {
    await settingsStore.loadConfig()
    Object.assign(localConfig, structuredClone(toRaw(settingsStore.config)))
  } catch (error) {
    console.error('Failed to load settings:', error)
    const detail = error instanceof Error ? error.message : String(error)
    showSettingsToast(`加载设置失败，使用默认配置: ${detail}`)
  }
}

function handleReset(): void {
  settingsStore.resetToDefault()
  Object.assign(localConfig, structuredClone(DEFAULT_CONFIG))
  showResetConfirm.value = false
}

function handleGeneralChange(): void {
  settingsStore.updateGeneral({
    language: localConfig.general.language,
    theme: localConfig.general.theme,
    minimizeToTray: localConfig.general.minimizeToTray,
  })
  setTheme(localConfig.general.theme)
}

function handleLanguageChange(): void {
  changeLocale(localConfig.general.language as 'zh-CN' | 'en-US')
  handleGeneralChange()
}

async function handleAutoStartChange(): Promise<void> {
  try {
    await settingsStore.setAutoStart(localConfig.general.autoStart)
  } catch (error) {
    localConfig.general.autoStart = !localConfig.general.autoStart
    console.error('Failed to set auto start:', error)
    showSettingsToast('设置开机自启动失败')
  }
}
</script>

<template>
  <div class="settings-panel">
    <!-- 头部 -->
    <div class="panel-header">
      <div class="header-title">
        <span class="title-text">{{ $t('settings.title') }}</span>
      </div>
      <div class="header-actions">
        <button
          class="action-btn reset-btn"
          :title="$t('settings.resetToDefault')"
          @click="showResetConfirm = true"
        >
          <span class="btn-icon">↺</span>
          <span class="btn-text">{{ $t('settings.resetToDefault') }}</span>
        </button>
      </div>
    </div>

    <!-- 主体区域：侧边栏 + 内容区 -->
    <div class="panel-body">
      <!-- 侧边栏导航 -->
      <SettingsSidebar v-model="activeCategory" />

      <!-- 设置内容区 -->
      <div class="settings-content">
        <!-- 通用设置 -->
        <div v-show="activeCategory === 'general'" class="settings-section">
          <h3 class="section-title">{{ $t('settings.general') }}</h3>

          <div class="setting-item">
            <label class="setting-label">{{ $t('settings.language') }}</label>
            <select
              v-model="localConfig.general.language"
              class="setting-select"
              @change="handleLanguageChange"
            >
              <option v-for="locale in supportedLocales" :key="locale.code" :value="locale.code">
                {{ locale.nativeName }}
              </option>
            </select>
          </div>

          <div class="setting-item">
            <label class="setting-label">{{ $t('settings.theme') }}</label>
            <select
              v-model="localConfig.general.theme"
              class="setting-select"
              @change="handleGeneralChange"
            >
              <option value="system">{{ $t('settings.themeSystem') }}</option>
              <option value="light">{{ $t('settings.themeLight') }}</option>
              <option value="dark">{{ $t('settings.themeDark') }}</option>
            </select>
          </div>

          <div class="setting-item">
            <label class="setting-label">{{ $t('settings.autoStart') }}</label>
            <div class="setting-toggle">
              <input
                v-model="localConfig.general.autoStart"
                type="checkbox"
                class="toggle-input"
                @change="handleAutoStartChange"
              />
              <span class="toggle-slider"></span>
            </div>
          </div>

          <div class="setting-item">
            <label class="setting-label">{{ $t('settings.minimizeToTray') }}</label>
            <div class="setting-toggle">
              <input
                v-model="localConfig.general.minimizeToTray"
                type="checkbox"
                class="toggle-input"
                @change="handleGeneralChange"
              />
              <span class="toggle-slider"></span>
            </div>
          </div>
        </div>

        <!-- 通知设置 -->
        <div v-show="activeCategory === 'notification'" class="settings-section">
          <NotificationSection />
        </div>

        <!-- 更新设置 -->
        <div v-show="activeCategory === 'update'" class="settings-section">
          <UpdateSection />
        </div>

        <!-- 高级设置 -->
        <div v-show="activeCategory === 'advanced'" class="settings-section">
          <AdvancedSection />
        </div>

        <!-- 关于 -->
        <div v-show="activeCategory === 'about'" class="settings-section">
          <AboutSection />
        </div>
      </div>
    </div>

    <!-- 底部状态栏 -->
    <div class="panel-footer">
      <span v-if="settingsStore.lastError" class="error-text">
        {{ settingsStore.lastError }}
      </span>
      <span v-else-if="settingsStore.isSaving" class="saving-text">
        {{ $t('settings.saving') }}
      </span>
      <span v-else class="status-text">
        {{ $t('settings.saved') }}
      </span>
    </div>

    <!-- 重置确认对话框 -->
    <div v-if="showResetConfirm" class="modal-overlay" @click.self="showResetConfirm = false">
      <div class="modal-dialog">
        <div class="modal-header">
          <span class="modal-title">{{ $t('common.confirm') }}</span>
        </div>
        <div class="modal-body">
          {{ $t('settings.resetConfirm') }}
        </div>
        <div class="modal-footer">
          <button class="modal-btn cancel-btn" @click="showResetConfirm = false">
            {{ $t('common.cancel') }}
          </button>
          <button class="modal-btn confirm-btn" @click="handleReset">
            {{ $t('common.reset') }}
          </button>
        </div>
      </div>
    </div>

    <!-- 操作反馈提示 -->
    <Transition name="toast">
      <div v-if="settingsToast.visible" :class="['settings-toast', settingsToast.type]">
        {{ settingsToast.message }}
      </div>
    </Transition>
  </div>
</template>

<style scoped>
.settings-panel {
  --bg-primary: var(--color-bg-primary);
  --bg-secondary: var(--color-bg-secondary);
  --bg-hover: var(--color-bg-tertiary);
  --bg-active: var(--color-accent-light);
  --text-primary: var(--color-text-primary);
  --text-secondary: var(--color-text-secondary);
  --text-muted: var(--color-text-tertiary);
  --accent-primary: var(--color-accent);
  --accent-hover: var(--color-accent-hover);
  --border-color: var(--color-border);
  --sidebar-width: 220px;
  --content-padding: 24px;
  --group-gap: 20px;
}

.settings-panel {
  display: flex;
  flex-direction: column;
  height: 100%;
  min-height: 0;
  overflow: hidden;
  background: var(--bg-primary);
  color: var(--text-primary);
  font-size: 13px;
}

.panel-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  min-height: 58px;
  padding: 12px 58px 12px 20px;
  background: var(--bg-secondary);
  border-bottom: 1px solid var(--border-color);
  flex-shrink: 0;
}

.header-title {
  display: flex;
  align-items: center;
  gap: 8px;
}

.title-text {
  font-size: 16px;
  font-weight: 600;
}

.header-actions {
  display: flex;
  gap: 8px;
}

.action-btn {
  display: flex;
  align-items: center;
  gap: 4px;
  padding: 6px 12px;
  border: none;
  border-radius: 4px;
  background: var(--bg-hover);
  color: var(--text-primary);
  font-size: 12px;
  cursor: pointer;
  transition: background-color 0.1s;
}

.action-btn:hover {
  opacity: 0.8;
}

.reset-btn:hover {
  background: var(--color-error-light);
}

.panel-body {
  display: flex;
  flex: 1;
  min-height: 0;
  overflow: hidden;
}

.settings-content {
  flex: 1;
  min-width: 0;
  overflow-y: auto;
  padding: 28px 32px 56px;
  background: var(--bg-secondary);
  overscroll-behavior: contain;
}

.settings-section {
  width: min(860px, 100%);
  max-width: none;
}

.section-title {
  margin: 0 0 16px 0;
  padding-bottom: 8px;
  font-size: 14px;
  font-weight: 600;
  color: var(--text-primary);
  border-bottom: 1px solid var(--border-color);
}

.setting-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 12px 0;
  border-bottom: 1px solid var(--color-border-light);
}

.setting-label {
  color: var(--text-secondary);
  font-size: 13px;
}

.setting-select {
  padding: 6px 12px;
  border: 1px solid var(--color-border);
  border-radius: 4px;
  background: var(--color-input-bg);
  color: var(--text-primary);
  font-size: 12px;
  cursor: pointer;
}

.setting-select:focus {
  outline: none;
  border-color: var(--color-border-focus);
}

.setting-toggle {
  position: relative;
  width: 44px;
  height: 24px;
}

.toggle-input {
  position: absolute;
  opacity: 0;
  width: 100%;
  height: 100%;
  cursor: pointer;
  z-index: 1;
}

.toggle-slider {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: var(--color-border);
  border-radius: 12px;
  transition: background-color 0.2s;
}

.toggle-slider::before {
  content: '';
  position: absolute;
  width: 18px;
  height: 18px;
  left: 3px;
  bottom: 3px;
  background: white;
  border-radius: 50%;
  transition: transform 0.2s;
}

.toggle-input:checked + .toggle-slider {
  background: var(--accent-primary);
}

.toggle-input:checked + .toggle-slider::before {
  transform: translateX(20px);
}

.panel-footer {
  display: flex;
  align-items: center;
  min-height: 36px;
  padding: 0 20px;
  background: var(--bg-secondary);
  border-top: 1px solid var(--border-color);
  font-size: 11px;
  flex-shrink: 0;
}

.error-text {
  color: #f44336;
}

.saving-text {
  color: #2196f3;
}

.status-text {
  color: var(--text-muted);
}

.modal-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: var(--color-bg-overlay);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
}

.modal-dialog {
  width: 360px;
  background: var(--color-bg-elevated);
  border-radius: 8px;
  box-shadow: var(--shadow-lg);
}

.modal-header {
  padding: 16px;
  border-bottom: 1px solid var(--border-color);
}

.modal-title {
  font-size: 14px;
  font-weight: 600;
}

.modal-body {
  padding: 16px;
  color: var(--text-secondary);
  font-size: 13px;
  line-height: 1.5;
}

.modal-footer {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  padding: 12px 16px;
  border-top: 1px solid var(--border-color);
}

.modal-btn {
  padding: 8px 16px;
  border: none;
  border-radius: 4px;
  font-size: 12px;
  cursor: pointer;
  transition: background-color 0.1s;
}

.cancel-btn {
  background: var(--bg-hover);
  color: var(--text-primary);
}

.cancel-btn:hover {
  opacity: 0.8;
}

.confirm-btn {
  background: var(--color-error);
  color: var(--color-text-inverse);
}

.confirm-btn:hover {
  opacity: 0.9;
}

.settings-content::-webkit-scrollbar {
  width: 6px;
}

.settings-content::-webkit-scrollbar-track {
  background: transparent;
}

.settings-content::-webkit-scrollbar-thumb {
  background: var(--border-color);
  border-radius: 3px;
}

.settings-content::-webkit-scrollbar-thumb:hover {
  background: var(--text-muted);
}

.settings-toast {
  position: fixed;
  bottom: 24px;
  left: 50%;
  transform: translateX(-50%);
  padding: 10px 20px;
  border-radius: 4px;
  color: #fff;
  font-size: 13px;
  font-weight: 500;
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.3);
  z-index: 1000;
}

.settings-toast.error {
  background: rgba(244, 67, 54, 0.95);
}

.settings-toast.success {
  background: rgba(76, 175, 80, 0.95);
}

.toast-enter-active,
.toast-leave-active {
  transition: all 0.2s ease;
}

.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translateX(-50%) translateY(8px);
}

@media (max-width: 900px) {
  .settings-panel {
    --sidebar-width: 180px;
    --content-padding: 20px;
  }

  .settings-content {
    padding: 24px 24px 52px;
  }

  .settings-section {
    width: 100%;
  }
}

@media (max-width: 640px) {
  .panel-header {
    min-height: 54px;
    padding: 10px 52px 10px 16px;
  }

  .panel-body {
    flex-direction: column;
  }

  .settings-content {
    padding: 20px 16px 48px;
  }

  .setting-item {
    align-items: flex-start;
    flex-direction: column;
    gap: 8px;
  }
}
</style>
