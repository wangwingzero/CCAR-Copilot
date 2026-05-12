<script setup lang="ts">
import { ref, provide, onMounted, onUnmounted } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { invoke } from '@tauri-apps/api/core'
import { X } from 'lucide-vue-next'
import ErrorBoundary from '@/components/ErrorBoundary.vue'
import RegulationSearchPanel from '@/components/regulation/RegulationSearchPanel.vue'
import { SettingsPanel } from '@/components/settings'
import { SearchDialog } from '@/components/FileSearch'
import { useSettingsStore } from '@/stores/settings'
import { useRegulationQuery } from '@/composables/useRegulationQuery'

const showFileSearch = ref(false)
const showSettings = ref(false)
const settingsStore = useSettingsStore()
const { initLocalIndex, syncCompare } = useRegulationQuery()
const REGULATION_AUTO_SYNC_DATE_KEY = 'regulation-auto-sync-date'

// 提供给子组件调用
provide('openSettings', () => {
  showSettings.value = true
})

function handleGlobalKeydown(event: KeyboardEvent) {
  // Ctrl+P 打开文件搜索
  if (event.ctrlKey && event.key === 'p') {
    event.preventDefault()
    showFileSearch.value = true
  }
  // Ctrl+, 打开设置
  if (event.ctrlKey && event.key === ',') {
    event.preventDefault()
    showSettings.value = true
  }
  // Esc 关闭设置
  if (event.key === 'Escape' && showSettings.value) {
    showSettings.value = false
  }
}

let unlistenSettings: (() => void) | null = null
let autoSyncTimer: ReturnType<typeof setTimeout> | null = null
let autoSyncInterval: ReturnType<typeof setInterval> | null = null
let isCheckingAutoSync = false

function getLocalDateKey(date = new Date()): string {
  const year = date.getFullYear()
  const month = String(date.getMonth() + 1).padStart(2, '0')
  const day = String(date.getDate()).padStart(2, '0')
  return `${year}-${month}-${day}`
}

async function runRegulationAutoSyncIfDue(): Promise<void> {
  if (isCheckingAutoSync) return
  isCheckingAutoSync = true

  try {
    if (!settingsStore.isLoaded) {
      await settingsStore.loadConfig()
    }

    const advanced = settingsStore.advanced
    if (!advanced.regulationAutoSyncEnabled) return

    const today = getLocalDateKey()
    if (localStorage.getItem(REGULATION_AUTO_SYNC_DATE_KEY) === today) return

    if (advanced.regulationAutoSyncWifiOnly) {
      const wifiConnected = await invoke<boolean>('regulation_is_wifi_connected')
      if (!wifiConnected) return
    }

    await initLocalIndex()
    const result = await syncCompare('all', 20, true)
    if (result) {
      localStorage.setItem(REGULATION_AUTO_SYNC_DATE_KEY, today)
      await syncKnowledgeSnapshotIfEnabled()
    }
  } catch (error) {
    console.warn('[RegulationAutoSync] 自动同步跳过:', error)
  } finally {
    isCheckingAutoSync = false
  }
}

async function syncKnowledgeSnapshotIfEnabled(): Promise<void> {
  const advanced = settingsStore.advanced
  if (
    !advanced.knowledgeServerSyncEnabled ||
    !advanced.knowledgeAutoSyncAfterRegulationUpdate
  ) {
    return
  }

  try {
    await invoke('regulation_knowledge_sync_server', {
      request: {
        host: advanced.knowledgeServerHost,
        port: advanced.knowledgeServerPort,
        user: advanced.knowledgeServerUser,
        keyPath: advanced.knowledgeServerKeyPath,
        remoteDir: advanced.knowledgeServerRemoteDir,
      },
    })
  } catch (error) {
    console.warn('[KnowledgeSync] AI 知识库同步失败:', error)
  }
}

onMounted(async () => {
  window.addEventListener('keydown', handleGlobalKeydown)
  // 监听托盘"设置"菜单点击
  unlistenSettings = await listen('open-settings', () => {
    showSettings.value = true
  })
  autoSyncTimer = setTimeout(runRegulationAutoSyncIfDue, 15_000)
  autoSyncInterval = setInterval(runRegulationAutoSyncIfDue, 60 * 60 * 1000)
})

onUnmounted(() => {
  window.removeEventListener('keydown', handleGlobalKeydown)
  unlistenSettings?.()
  if (autoSyncTimer) {
    clearTimeout(autoSyncTimer)
    autoSyncTimer = null
  }
  if (autoSyncInterval) {
    clearInterval(autoSyncInterval)
    autoSyncInterval = null
  }
})
</script>

<template>
  <main class="app-root">
    <ErrorBoundary>
      <RegulationSearchPanel />
    </ErrorBoundary>

    <!-- 设置面板覆盖层 -->
    <Transition name="settings-fade">
      <div v-if="showSettings" class="settings-overlay" @click.self="showSettings = false">
        <div class="settings-container">
          <button class="settings-close-btn" @click="showSettings = false">
            <X :size="18" :stroke-width="2" />
          </button>
          <SettingsPanel />
        </div>
      </div>
    </Transition>

    <SearchDialog :visible="showFileSearch" @close="showFileSearch = false" />
  </main>
</template>

<style scoped>
.app-root {
  width: 100vw;
  height: 100vh;
  overflow: hidden;
  background: var(--color-bg-primary, #f5f7fa);
}

.settings-overlay {
  position: fixed;
  inset: 0;
  z-index: 1000;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 24px;
  background: rgba(0, 0, 0, 0.48);
  backdrop-filter: blur(2px);
}

.settings-container {
  position: relative;
  width: clamp(860px, 72vw, 1120px);
  max-width: calc(100vw - 48px);
  height: clamp(620px, 78vh, 820px);
  max-height: calc(100vh - 48px);
  background: var(--color-bg-primary, #fff);
  border-radius: 8px;
  border: 1px solid var(--color-border, rgba(0, 0, 0, 0.12));
  box-shadow: 0 18px 60px rgba(0, 0, 0, 0.28);
  overflow: hidden;
}

.settings-close-btn {
  position: absolute;
  top: 14px;
  right: 14px;
  z-index: 20;
  display: flex;
  align-items: center;
  justify-content: center;
  width: 32px;
  height: 32px;
  border: none;
  background: var(--color-bg-tertiary, rgba(0, 0, 0, 0.04));
  color: var(--text-secondary, #666);
  cursor: pointer;
  border-radius: 6px;
  transition:
    background-color 0.15s,
    color 0.15s;
}

.settings-close-btn:hover {
  background: var(--bg-hover, rgba(0, 0, 0, 0.08));
  color: var(--color-text-primary, #111);
}

@media (max-width: 900px), (max-height: 700px) {
  .settings-overlay {
    padding: 12px;
  }

  .settings-container {
    width: calc(100vw - 24px);
    max-width: calc(100vw - 24px);
    height: calc(100vh - 24px);
    max-height: calc(100vh - 24px);
  }
}

.settings-fade-enter-active,
.settings-fade-leave-active {
  transition: opacity 0.2s ease;
}
.settings-fade-enter-from,
.settings-fade-leave-to {
  opacity: 0;
}
</style>
