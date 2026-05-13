<script setup lang="ts">
/**
 * UpdateSection - 更新设置面板
 *
 * 两部分组成:
 *
 * 1. 自动更新配置: 自动检查开关、检查间隔、代理设置
 *    - 存储在 `settingsStore.update` 中,由 Rust 的 `AppConfig.update` 持久化
 *    - Rust 端 `build_updater()` 运行时读取代理配置
 *
 * 2. 更新动作和状态:
 *    - 调用 `useUpdate()` composable 的 `checkForUpdate / downloadAndInstall /
 *      restartApp`
 *    - 显示当前版本、发现的新版本、下载进度、发布说明和重启提示
 */

import { computed, ref } from 'vue'
import { useI18n } from 'vue-i18n'
import { ExternalLink, RefreshCw } from 'lucide-vue-next'
import { open } from '@tauri-apps/plugin-shell'
import { useSettingsStore } from '@/stores/settings'
import { useUpdate } from '@/composables/useUpdate'
import SettingsGroup from '@/components/settings/controls/SettingsGroup.vue'
import SettingItem from '@/components/settings/controls/SettingItem.vue'
import ToggleSwitch from '@/components/settings/controls/ToggleSwitch.vue'

// ============================================
// Store / Composable
// ============================================

const settingsStore = useSettingsStore()
const {
  status,
  currentVersion,
  updateInfo,
  downloadProgress,
  downloadedBytes,
  totalBytes,
  downloadSpeed,
  downloadEtaSeconds,
  latestDownloadUrl,
  isChecking,
  isDownloading,
  isUpdateAvailable,
  isPendingRestart,
  error,
  checkForUpdate,
  downloadAndInstall,
  restartApp,
  skipCurrentVersion,
  retryLastAction,
  getLatestUpdateDownloadUrl,
} = useUpdate()

const { t } = useI18n()
const isOpeningLatestInstaller = ref(false)
const manualDownloadError = ref('')

// ============================================
// Computed
// ============================================

const updateConfig = computed(() => settingsStore.update)

const formatLastCheckTime = computed(() => {
  if (!updateConfig.value.lastCheckTime) return ''
  try {
    const date = new Date(updateConfig.value.lastCheckTime)
    return date.toLocaleString()
  } catch {
    return updateConfig.value.lastCheckTime
  }
})

const formatReleaseDate = computed(() => {
  const raw = updateInfo.value?.date
  if (!raw) return ''
  const d = new Date(raw)
  return Number.isNaN(d.getTime()) ? raw : d.toLocaleString()
})

/** 是否展示更新状态卡片(Idle 且无错误时隐藏,避免多余 UI) */
const showStatusCard = computed(() => {
  const s = status.value.status
  return (
    s === 'Available' ||
    s === 'Downloading' ||
    s === 'Ready' ||
    s === 'Installing' ||
    s === 'PendingRestart' ||
    s === 'UpToDate' ||
    s === 'Error' ||
    !!error.value
  )
})

/** 人类可读的下载大小, 未知时返回 "?" */
function formatBytes(n: number | null): string {
  if (n === null || n === undefined) return '?'
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  if (n < 1024 * 1024 * 1024) return `${(n / 1024 / 1024).toFixed(1)} MB`
  return `${(n / 1024 / 1024 / 1024).toFixed(2)} GB`
}

const formattedDownloadedBytes = computed(() => formatBytes(downloadedBytes.value))
const formattedTotalBytes = computed(() => formatBytes(totalBytes.value))
const progressPercent = computed(() => downloadProgress.value.toFixed(1))

/** 下载速度人读字符串, 未开始采样时返回空 */
const formattedDownloadSpeed = computed(() => {
  if (!downloadSpeed.value || downloadSpeed.value <= 0) return ''
  return formatBytes(downloadSpeed.value) + '/s'
})

/** 剩余时间人读字符串, 无法估计返回空 */
const formattedEta = computed(() => {
  const sec = downloadEtaSeconds.value
  if (sec === null || sec === undefined) return ''
  if (sec <= 0) return ''
  if (sec < 60) return t('settings.update.etaSeconds', { n: sec })
  if (sec < 3600) return t('settings.update.etaMinutes', { n: Math.ceil(sec / 60) })
  return t('settings.update.etaHours', { n: (sec / 3600).toFixed(1) })
})

// ============================================
// Event Handlers
// ============================================

function handleAutoCheckChange(value: boolean): void {
  settingsStore.updateUpdate({ autoCheck: value })
}

/**
 * 触发立即检查更新,并把最后检查时间写入 settings store
 */
async function handleCheckNow(): Promise<void> {
  if (isChecking.value || isDownloading.value) return
  manualDownloadError.value = ''
  try {
    await checkForUpdate()
  } finally {
    settingsStore.updateUpdate({ lastCheckTime: new Date().toISOString() })
  }
}

async function handleDownloadAndInstall(): Promise<void> {
  if (isDownloading.value) return
  await downloadAndInstall()
}

async function handleRestart(): Promise<void> {
  await restartApp()
}

async function handleOpenLatestInstaller(): Promise<void> {
  if (isOpeningLatestInstaller.value) return
  isOpeningLatestInstaller.value = true
  manualDownloadError.value = ''
  try {
    const url = await getLatestUpdateDownloadUrl()
    await open(url)
  } catch (e) {
    manualDownloadError.value = String(e)
  } finally {
    isOpeningLatestInstaller.value = false
  }
}

/** 「重试」按钮: 根据上次失败的动作重新 check / download */
async function handleRetry(): Promise<void> {
  await retryLastAction()
}

/** 「跳过此版本」按钮: 持久化不再提示该版本 */
function handleSkipVersion(): void {
  skipCurrentVersion()
}
</script>

<template>
  <div class="update-section">
    <!-- 版本 & 更新动作 -->
    <SettingsGroup :title="$t('settings.update.title')">
      <!-- 当前版本 -->
      <SettingItem :label="$t('settings.update.currentVersion')">
        <span class="current-version">{{ currentVersion || '—' }}</span>
      </SettingItem>

      <!-- 立即检查按钮 + 上次检查时间 -->
      <SettingItem :label="$t('settings.update.checkNow')">
        <div class="check-now-wrap">
          <button
            class="check-now-btn"
            :disabled="isChecking || isDownloading"
            @click="handleCheckNow"
          >
            <RefreshCw class="btn-icon" :class="{ spinning: isChecking }" :size="14" />
            <span>
              {{
                isChecking
                  ? $t('settings.update.checking')
                  : $t('settings.update.checkNowBtn')
              }}
            </span>
          </button>
          <span v-if="updateConfig.lastCheckTime" class="last-check-inline">
            {{ $t('settings.update.lastCheck') }}: {{ formatLastCheckTime }}
          </span>
        </div>
      </SettingItem>

      <!-- 手动下载安装包 -->
      <SettingItem
        :label="$t('settings.update.manualDownload')"
        :help-text="$t('settings.update.manualDownloadHelp')"
      >
        <div class="manual-download-wrap">
          <button
            data-testid="open-latest-installer"
            class="manual-download-btn"
            :disabled="isOpeningLatestInstaller"
            @click="handleOpenLatestInstaller"
          >
            <ExternalLink class="btn-icon" :size="14" />
            <span>
              {{
                isOpeningLatestInstaller
                  ? $t('settings.update.openingLatestInstaller')
                  : $t('settings.update.openLatestInstaller')
              }}
            </span>
          </button>
          <span v-if="latestDownloadUrl" class="installer-url" :title="latestDownloadUrl">
            {{ latestDownloadUrl }}
          </span>
          <span v-if="manualDownloadError" class="manual-download-error">
            {{ $t('settings.update.manualDownloadError', { message: manualDownloadError }) }}
          </span>
        </div>
      </SettingItem>

      <!-- 更新状态卡片 -->
      <div v-if="showStatusCard" class="status-card" :class="`status-${status.status.toLowerCase()}`">
        <!-- 已是最新版本 -->
        <template v-if="status.status === 'UpToDate'">
          <div class="status-title">{{ $t('settings.update.statusUpToDate') }}</div>
        </template>

        <!-- 发现新版本 -->
        <template v-else-if="isUpdateAvailable && updateInfo">
          <div class="status-title">
            {{ $t('settings.update.statusAvailable') }}
          </div>
          <div class="status-version-compare">
            {{
              $t('settings.update.currentToNew', {
                current: currentVersion || '—',
                next: updateInfo.version,
              })
            }}
          </div>
          <div v-if="formatReleaseDate" class="status-meta">
            {{ $t('settings.update.releaseDate') }}: {{ formatReleaseDate }}
          </div>
          <pre v-if="updateInfo.notes" class="release-notes">{{ updateInfo.notes }}</pre>
          <div class="status-actions">
            <button
              class="primary-btn"
              :disabled="isDownloading"
              @click="handleDownloadAndInstall"
            >
              {{ $t('settings.update.downloadAndInstall') }}
            </button>
            <button class="secondary-btn" @click="handleSkipVersion">
              {{ $t('settings.update.skipThisVersion') }}
            </button>
          </div>
        </template>

        <!-- 下载中 -->
        <template v-else-if="isDownloading">
          <div class="status-title">{{ $t('settings.update.statusDownloading') }}</div>
          <div class="progress-track">
            <div class="progress-fill" :style="{ width: `${progressPercent}%` }"></div>
          </div>
          <div class="status-meta">
            {{ progressPercent }}% — {{ formattedDownloadedBytes }} / {{ formattedTotalBytes }}
          </div>
          <div
            v-if="formattedDownloadSpeed || formattedEta"
            class="status-meta status-meta-row"
          >
            <span v-if="formattedDownloadSpeed">
              {{ $t('settings.update.downloadSpeed') }}: {{ formattedDownloadSpeed }}
            </span>
            <span v-if="formattedEta" class="status-eta">
              {{ $t('settings.update.downloadEta') }}: {{ formattedEta }}
            </span>
          </div>
        </template>

        <!-- 安装中 / 下载完成待安装 -->
        <template v-else-if="status.status === 'Installing' || status.status === 'Ready'">
          <div class="status-title">{{ $t('settings.update.statusInstalling') }}</div>
        </template>

        <!-- 安装完成待重启 -->
        <template v-else-if="isPendingRestart">
          <div class="status-title">{{ $t('settings.update.statusPendingRestart') }}</div>
          <div class="status-actions">
            <button class="primary-btn" @click="handleRestart">
              {{ $t('settings.update.restartNow') }}
            </button>
          </div>
        </template>

        <!-- 错误 -->
        <template v-else-if="status.status === 'Error' || error">
          <div class="status-title status-error-title">
            {{ $t('settings.update.statusError') }}
          </div>
          <div class="status-error-message">
            {{ status.message || error }}
          </div>
          <div class="status-actions">
            <button class="primary-btn" :disabled="isChecking || isDownloading" @click="handleRetry">
              {{ $t('settings.update.retry') }}
            </button>
          </div>
        </template>
      </div>
    </SettingsGroup>

    <!-- 自动检查与代理配置 -->
    <SettingsGroup :title="$t('settings.update.settings')">
      <SettingItem
        :label="$t('settings.update.autoCheck')"
        :help-text="$t('settings.update.autoCheckHelp')"
      >
        <ToggleSwitch
          :model-value="updateConfig.autoCheck"
          :aria-label="$t('settings.update.autoCheck')"
          @update:model-value="handleAutoCheckChange"
        />
      </SettingItem>

    </SettingsGroup>
  </div>
</template>

<style scoped>
.current-version {
  color: var(--color-text-primary, #fff);
  font-size: 13px;
  font-variant-numeric: tabular-nums;
}

.check-now-wrap {
  display: flex;
  align-items: center;
  gap: 12px;
  flex-wrap: wrap;
}

.manual-download-wrap {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 6px;
  min-width: 0;
}

.last-check-inline {
  color: var(--color-text-secondary, #ebebf599);
  font-size: 12px;
}

.status-card {
  margin: 8px 0 4px;
  padding: 12px 14px;
  border: 1px solid var(--color-border, #38383a);
  border-radius: var(--radius-md, 8px);
  background: var(--color-surface-alt, rgba(118, 118, 128, 0.16));
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.status-card.status-available {
  border-color: var(--color-accent, #0a84ff);
}

.status-card.status-uptodate .status-title {
  color: var(--color-success, #30d158);
}

.status-card.status-error {
  border-color: var(--color-danger, #ff453a);
}

.status-title {
  color: var(--color-text-primary, #fff);
  font-size: 13px;
  font-weight: 600;
}

.status-meta {
  color: var(--color-text-secondary, #ebebf599);
  font-size: 12px;
  font-variant-numeric: tabular-nums;
}

.release-notes {
  margin: 0;
  padding: 8px 10px;
  background: var(--color-input-bg, rgba(118, 118, 128, 0.24));
  border-radius: var(--radius-sm, 6px);
  color: var(--color-text-primary, #fff);
  font-size: 12px;
  line-height: 1.5;
  white-space: pre-wrap;
  word-break: break-word;
  max-height: 180px;
  overflow: auto;
}

.status-actions {
  display: flex;
  gap: 8px;
  margin-top: 4px;
}

.primary-btn {
  padding: 6px 16px;
  border: none;
  border-radius: var(--radius-sm, 6px);
  background: var(--color-accent, #0a84ff);
  color: #fff;
  font-size: 13px;
  cursor: pointer;
  transition: background-color var(--transition-fast, 0.15s);
}

.primary-btn:hover:not(:disabled) {
  background: var(--color-accent-hover, #409cff);
}

.primary-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.progress-track {
  position: relative;
  width: 100%;
  height: 6px;
  border-radius: 999px;
  background: var(--color-input-bg, rgba(118, 118, 128, 0.24));
  overflow: hidden;
}

.progress-fill {
  height: 100%;
  background: var(--color-accent, #0a84ff);
  transition: width 0.2s ease-out;
}

.status-version-compare {
  color: var(--color-text-primary, #fff);
  font-size: 13px;
  font-variant-numeric: tabular-nums;
  letter-spacing: 0.2px;
  opacity: 0.9;
}

.status-meta-row {
  display: flex;
  flex-wrap: wrap;
  gap: 14px;
}

.status-eta {
  /* 剩余时间与速度同行显示，表示同一个“实时下载详情”信息家族 */
}

.secondary-btn {
  padding: 6px 16px;
  border: 1px solid var(--color-border, #38383a);
  border-radius: var(--radius-sm, 6px);
  background: transparent;
  color: var(--color-text-secondary, #ebebf599);
  font-size: 13px;
  cursor: pointer;
  transition: color var(--transition-fast, 0.15s), border-color var(--transition-fast, 0.15s);
}

.secondary-btn:hover:not(:disabled) {
  color: var(--color-text-primary, #fff);
  border-color: var(--color-text-secondary, #ebebf599);
}

.secondary-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.manual-download-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 6px 14px;
  border: 1px solid var(--color-accent, #0a84ff);
  border-radius: var(--radius-sm, 6px);
  background: rgba(10, 132, 255, 0.12);
  color: var(--color-accent, #0a84ff);
  font-size: 13px;
  cursor: pointer;
  transition: background-color var(--transition-fast, 0.15s),
    color var(--transition-fast, 0.15s);
}

.manual-download-btn:hover:not(:disabled) {
  background: var(--color-accent, #0a84ff);
  color: #fff;
}

.manual-download-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.btn-icon {
  flex: 0 0 auto;
}

.spinning {
  animation: update-spin 0.9s linear infinite;
}

@keyframes update-spin {
  to {
    transform: rotate(360deg);
  }
}

.installer-url {
  max-width: min(440px, 100%);
  color: var(--color-text-secondary, #ebebf599);
  font-size: 11px;
  font-variant-numeric: tabular-nums;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.manual-download-error {
  max-width: min(440px, 100%);
  color: var(--color-danger, #ff453a);
  font-size: 12px;
  line-height: 1.4;
  text-align: right;
  word-break: break-word;
}

.status-error-title {
  color: var(--color-danger, #ff453a);
}

.status-error-message {
  color: var(--color-danger, #ff453a);
  font-size: 12px;
  word-break: break-word;
}

.setting-input {
  width: 200px;
  padding: 6px 10px;
  border: 1px solid var(--color-border, #38383a);
  border-radius: var(--radius-sm, 6px);
  background: var(--color-input-bg, rgba(118, 118, 128, 0.24));
  color: var(--color-text-primary, #fff);
  font-size: 13px;
}

.setting-input:focus {
  outline: none;
  border-color: var(--color-accent, #0a84ff);
}

.setting-input::placeholder {
  color: var(--color-text-tertiary, #ebebf54d);
}

.check-now-btn {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 6px 16px;
  border: none;
  border-radius: var(--radius-sm, 6px);
  background: var(--color-accent, #0a84ff);
  color: white;
  font-size: 13px;
  cursor: pointer;
  transition: background-color var(--transition-fast, 0.15s);
}

.check-now-btn:hover:not(:disabled) {
  background: var(--color-accent-hover, #409cff);
}

.check-now-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.last-check-time {
  color: var(--color-text-secondary, #ebebf599);
  font-size: 12px;
}
</style>
