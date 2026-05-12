<script setup lang="ts">
/**
 * AdvancedSection - Advanced Settings Section
 *
 * Provides configuration options for advanced settings:
 * - Proxy configuration (type, host, port)
 * - Debug logging toggle with path selector
 * - Portable mode toggle with restart warning
 * - Import/export buttons
 *
 * Uses the reusable settings control components:
 * - SettingsGroup for card-style grouping
 * - SettingItem for consistent row layout
 * - ToggleSwitch for boolean settings
 *
 * @validates Requirements 9.1, 9.2, 9.3, 9.4, 9.5, 9.6
 */

import { computed, ref, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { open, save } from '@tauri-apps/plugin-dialog'
import { useSettingsStore } from '@/stores/settings'
import SettingsGroup from '@/components/settings/controls/SettingsGroup.vue'
import SettingItem from '@/components/settings/controls/SettingItem.vue'
import ToggleSwitch from '@/components/settings/controls/ToggleSwitch.vue'

// ============================================
// Store
// ============================================

const settingsStore = useSettingsStore()

// ============================================
// State
// ============================================

/** Show portable mode warning */
const showPortableWarning = ref(false)

/** 当前生效的局方文件保存路径 */
const resolvedStoragePath = ref('')

/** 局方分类目录创建提示 */
const storageDirStatus = ref('')

/** AI 知识库导出/同步状态 */
const knowledgeStatus = ref('')

/** AI 知识库是否正在执行导出或同步 */
const isKnowledgeBusy = ref(false)

// ============================================
// 开发者专区解锁
// ============================================
// AI 知识库同步涉及写入生产服务器，此处密码仅为 UI 层门禁，用于防止最终用户误操作。
// 真正的安全边界仍由服务器端 API Token / SSH Key 校验保障 —— 没有 Token/私钥的
// 机器即便绕过前端锁定也无法触达生产环境。SHA-256 哈希对应口令：Hu20100416@@@
const DEV_UNLOCK_SHA256 = '34b43275a071baba25cc22dd14e26dad6b64253fc58ee0eb15c2dbdcfffeb1d5'
const DEV_UNLOCK_SESSION_KEY = 'ccar-copilot-knowledge-unlocked'

const isDeveloperUnlocked = ref(sessionStorage.getItem(DEV_UNLOCK_SESSION_KEY) === '1')
const devPasswordInput = ref('')
const devPasswordError = ref('')
const isUnlocking = ref(false)

async function sha256Hex(text: string): Promise<string> {
  const buf = new TextEncoder().encode(text)
  const digest = await crypto.subtle.digest('SHA-256', buf)
  return Array.from(new Uint8Array(digest))
    .map((b) => b.toString(16).padStart(2, '0'))
    .join('')
}

async function handleTryUnlock(): Promise<void> {
  if (isUnlocking.value) return
  devPasswordError.value = ''
  const input = devPasswordInput.value
  if (!input) {
    devPasswordError.value = '请先输入密码'
    return
  }
  isUnlocking.value = true
  try {
    const hash = await sha256Hex(input)
    if (hash === DEV_UNLOCK_SHA256) {
      isDeveloperUnlocked.value = true
      sessionStorage.setItem(DEV_UNLOCK_SESSION_KEY, '1')
      devPasswordInput.value = ''
    } else {
      devPasswordError.value = '密码不正确'
    }
  } finally {
    isUnlocking.value = false
  }
}

function handleLockDeveloper(): void {
  isDeveloperUnlocked.value = false
  devPasswordInput.value = ''
  devPasswordError.value = ''
  sessionStorage.removeItem(DEV_UNLOCK_SESSION_KEY)
}

// 初始化时获取当前路径
invoke<string>('regulation_get_storage_path').then(path => {
  resolvedStoragePath.value = path
})

// ============================================
// Computed
// ============================================

/**
 * Advanced configuration from store
 * Provides reactive access to current settings
 */
const advancedConfig = computed(() => settingsStore.advanced)

// ============================================
// Watchers
// ============================================

// Watch for portable mode changes to show warning
watch(
  () => advancedConfig.value.portableMode,
  (newValue, oldValue) => {
    if (newValue !== oldValue) {
      showPortableWarning.value = true
      // Hide warning after 5 seconds
      setTimeout(() => {
        showPortableWarning.value = false
      }, 5000)
    }
  }
)

// ============================================
// Event Handlers
// ============================================

/**
 * Handle proxy enabled toggle change
 * @param value - New proxy enabled state
 * @validates Requirements 9.1
 */
function handleProxyEnabledChange(value: boolean): void {
  settingsStore.updateAdvanced({ proxyEnabled: value })
}

/**
 * Handle proxy type change
 * @param event - Change event
 * @validates Requirements 9.2
 */
function handleProxyTypeChange(event: Event): void {
  const target = event.target as HTMLSelectElement
  settingsStore.updateAdvanced({ proxyType: target.value as 'http' | 'socks5' })
}

/**
 * Handle proxy host input
 * @param event - Input event
 * @validates Requirements 9.2
 */
function handleProxyHostInput(event: Event): void {
  const target = event.target as HTMLInputElement
  settingsStore.updateAdvanced({ proxyHost: target.value })
}

/**
 * Handle proxy port input
 * @param event - Input event
 * @validates Requirements 9.2
 */
function handleProxyPortInput(event: Event): void {
  const target = event.target as HTMLInputElement
  const port = parseInt(target.value, 10)
  if (!isNaN(port) && port >= 1 && port <= 65535) {
    settingsStore.updateAdvanced({ proxyPort: port })
  }
}

/**
 * Handle debug logging toggle change
 * @param value - New debug logging state
 * @validates Requirements 9.3
 */
function handleDebugLoggingChange(value: boolean): void {
  settingsStore.updateAdvanced({ debugLogging: value })
}

/**
 * Handle browse log path button click
 * @validates Requirements 9.4
 */
async function handleBrowseLogPath(): Promise<void> {
  const selected = await open({
    directory: true,
    multiple: false,
    title: '选择日志保存目录',
  })

  if (selected && typeof selected === 'string') {
    settingsStore.updateAdvanced({ debugLogPath: selected })
    // 关键路径变更，立即保存确保持久化
    await settingsStore.saveConfig()
  }
}

/**
 * Handle browse regulation storage path button click
 * 选择局方文件保存目录
 */
async function handleBrowseRegulationPath(): Promise<void> {
  const selected = await open({
    directory: true,
    multiple: false,
    title: '选择局方文件保存目录',
  })

  if (selected && typeof selected === 'string') {
    settingsStore.updateAdvanced({ regulationStoragePath: selected })
    resolvedStoragePath.value = selected
    // 关键路径变更，立即保存确保持久化
    await settingsStore.saveConfig()
    await prepareRegulationStorageDirs()
  }
}

/**
 * Reset regulation storage path to default (AppData)
 */
async function handleResetRegulationPath(): Promise<void> {
  settingsStore.updateAdvanced({ regulationStoragePath: '' })
  // 关键路径变更，立即保存确保持久化
  await settingsStore.saveConfig()
  const path = await invoke<string>('regulation_get_storage_path')
  resolvedStoragePath.value = path
  await prepareRegulationStorageDirs()
}

async function prepareRegulationStorageDirs(): Promise<void> {
  storageDirStatus.value = ''
  try {
    const result = await invoke<{ root: string; directories: string[] }>(
      'regulation_prepare_storage_dirs'
    )
    resolvedStoragePath.value = result.root
    storageDirStatus.value = `已创建 ${result.directories.length} 个分类目录`
  } catch (error) {
    storageDirStatus.value = `目录创建失败: ${error}`
  }
}

function handleRegulationAutoSyncChange(value: boolean): void {
  settingsStore.updateAdvanced({ regulationAutoSyncEnabled: value })
}

function handleRegulationWifiOnlyChange(value: boolean): void {
  settingsStore.updateAdvanced({ regulationAutoSyncWifiOnly: value })
}

function handleMineruOcrEnabledChange(value: boolean): void {
  settingsStore.updateAdvanced({
    mineruOcrEnabled: value,
    mineruOcrPreferOnline: value ? advancedConfig.value.mineruOcrPreferOnline : false,
  })
}

function handleMineruOcrPreferOnlineChange(value: boolean): void {
  settingsStore.updateAdvanced({ mineruOcrPreferOnline: value })
}

function handleMineruApiKeyInput(event: Event): void {
  const target = event.target as HTMLInputElement
  settingsStore.updateAdvanced({ mineruApiKey: target.value.trim() })
}

function handleKnowledgeServerSyncChange(value: boolean): void {
  settingsStore.updateAdvanced({ knowledgeServerSyncEnabled: value })
}

function handleKnowledgeAutoSyncChange(value: boolean): void {
  settingsStore.updateAdvanced({ knowledgeAutoSyncAfterRegulationUpdate: value })
}

function handleKnowledgeSyncModeChange(event: Event): void {
  const target = event.target as HTMLSelectElement
  settingsStore.updateAdvanced({ knowledgeSyncMode: target.value as 'api' | 'ssh' })
}

function handleKnowledgeApiUrlInput(event: Event): void {
  const target = event.target as HTMLInputElement
  settingsStore.updateAdvanced({ knowledgeApiUrl: target.value.trim() })
}

function handleKnowledgeApiTokenInput(event: Event): void {
  const target = event.target as HTMLInputElement
  settingsStore.updateAdvanced({ knowledgeApiToken: target.value })
}

function handleKnowledgeHostInput(event: Event): void {
  const target = event.target as HTMLInputElement
  settingsStore.updateAdvanced({ knowledgeServerHost: target.value.trim() })
}

function handleKnowledgePortInput(event: Event): void {
  const target = event.target as HTMLInputElement
  const port = parseInt(target.value, 10)
  if (!isNaN(port) && port >= 1 && port <= 65535) {
    settingsStore.updateAdvanced({ knowledgeServerPort: port })
  }
}

function handleKnowledgeUserInput(event: Event): void {
  const target = event.target as HTMLInputElement
  settingsStore.updateAdvanced({ knowledgeServerUser: target.value.trim() })
}

function handleKnowledgeRemoteDirInput(event: Event): void {
  const target = event.target as HTMLInputElement
  settingsStore.updateAdvanced({ knowledgeServerRemoteDir: target.value.trim() })
}

function handleKnowledgeKeyPathInput(event: Event): void {
  const target = event.target as HTMLInputElement
  settingsStore.updateAdvanced({ knowledgeServerKeyPath: target.value.trim() })
}

async function handleBrowseKnowledgeKeyPath(): Promise<void> {
  const selected = await open({
    multiple: false,
    title: '选择服务器 SSH 私钥',
  })

  if (selected && typeof selected === 'string') {
    settingsStore.updateAdvanced({ knowledgeServerKeyPath: selected })
    await settingsStore.saveConfig()
  }
}

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return '0 MB'
  const mb = bytes / 1024 / 1024
  return `${mb.toFixed(mb >= 100 ? 0 : 1)} MB`
}

type KnowledgeExportResponse = {
  dbPath: string
  manifest: {
    documentsTotal: number
    documentsWithContent: number
    chunksTotal: number
    dbBytes: number
  }
}

async function handleExportKnowledgeSnapshot(): Promise<void> {
  if (isKnowledgeBusy.value) return
  isKnowledgeBusy.value = true
  knowledgeStatus.value = '正在生成本地 AI 知识库...'

  try {
    const response = await invoke<KnowledgeExportResponse>('regulation_knowledge_export')
    knowledgeStatus.value =
      `已生成 ${response.manifest.documentsTotal} 篇 / ${response.manifest.chunksTotal} 个分块，` +
      `${formatBytes(response.manifest.dbBytes)}：${response.dbPath}`
  } catch (error) {
    knowledgeStatus.value = `生成失败: ${error}`
  } finally {
    isKnowledgeBusy.value = false
  }
}

async function handleSyncKnowledgeServer(): Promise<void> {
  if (isKnowledgeBusy.value) return
  isKnowledgeBusy.value = true
  knowledgeStatus.value = '正在生成并上传 AI 知识库...'

  const advanced = advancedConfig.value
  try {
    if (advanced.knowledgeSyncMode === 'ssh') {
      const response = await invoke<{
        export: KnowledgeExportResponse
        remoteCurrentDir: string
        host: string
      }>('regulation_knowledge_sync_server', {
        request: {
          host: advanced.knowledgeServerHost,
          port: advanced.knowledgeServerPort,
          user: advanced.knowledgeServerUser,
          keyPath: advanced.knowledgeServerKeyPath,
          remoteDir: advanced.knowledgeServerRemoteDir,
        },
      })
      knowledgeStatus.value =
        `已通过 SSH 同步到 ${response.host}:${response.remoteCurrentDir}，` +
        `${response.export.manifest.documentsTotal} 篇 / ` +
        `${response.export.manifest.chunksTotal} 个分块 / ` +
        `${formatBytes(response.export.manifest.dbBytes)}`
    } else {
      const response = await invoke<{
        export: KnowledgeExportResponse
        apiUrl: string
        currentDir: string
      }>('regulation_knowledge_sync_api', {
        request: {
          apiUrl: advanced.knowledgeApiUrl,
          apiToken: advanced.knowledgeApiToken,
        },
      })
      knowledgeStatus.value =
        `已通过 API 上传到 ${response.apiUrl}，` +
        `${response.export.manifest.documentsTotal} 篇 / ` +
        `${response.export.manifest.chunksTotal} 个分块 / ` +
        `${formatBytes(response.export.manifest.dbBytes)}`
    }
  } catch (error) {
    knowledgeStatus.value = `同步失败: ${error}`
  } finally {
    isKnowledgeBusy.value = false
  }
}

/**
 * Handle portable mode toggle change
 * Shows warning about restart requirement
 * @param value - New portable mode state
 * @validates Requirements 9.5, 9.6
 */
function handlePortableModeChange(value: boolean): void {
  settingsStore.updateAdvanced({ portableMode: value })
}

/**
 * Handle import settings button click
 */
async function handleImport(): Promise<void> {
  const selected = await open({
    filters: [{ name: 'JSON', extensions: ['json'] }],
    multiple: false,
    title: '导入设置',
  })

  if (selected && typeof selected === 'string') {
    // TODO: 实现导入逻辑
    console.warn('[Settings] Import from:', selected)
  }
}

/**
 * Handle export settings button click
 */
async function handleExport(): Promise<void> {
  const selected = await save({
    filters: [{ name: 'JSON', extensions: ['json'] }],
    defaultPath: 'hugescreenshot-settings.json',
    title: '导出设置',
  })

  if (selected) {
    // TODO: 实现导出逻辑
    console.warn('[Settings] Export to:', selected)
  }
}
</script>

<template>
  <div class="advanced-section">
    <!-- Proxy Configuration Group -->
    <SettingsGroup :title="$t('settings.advanced.proxy')">
      <!-- Proxy Enabled Toggle -->
      <SettingItem
        :label="$t('settings.advanced.proxyEnabled')"
        :help-text="$t('settings.advanced.proxyEnabledHelp')"
      >
        <ToggleSwitch
          :model-value="advancedConfig.proxyEnabled"
          :aria-label="$t('settings.advanced.proxyEnabled')"
          @update:model-value="handleProxyEnabledChange"
        />
      </SettingItem>

      <!-- Proxy Type Selector -->
      <SettingItem v-show="advancedConfig.proxyEnabled" :label="$t('settings.advanced.proxyType')">
        <select
          :value="advancedConfig.proxyType"
          class="setting-select"
          @change="handleProxyTypeChange"
        >
          <option value="http">HTTP</option>
          <option value="socks5">SOCKS5</option>
        </select>
      </SettingItem>

      <!-- Proxy Host Input -->
      <SettingItem v-show="advancedConfig.proxyEnabled" :label="$t('settings.advanced.proxyHost')">
        <input
          :value="advancedConfig.proxyHost"
          type="text"
          class="setting-input"
          placeholder="127.0.0.1"
          @input="handleProxyHostInput"
        />
      </SettingItem>

      <!-- Proxy Port Input -->
      <SettingItem v-show="advancedConfig.proxyEnabled" :label="$t('settings.advanced.proxyPort')">
        <input
          :value="advancedConfig.proxyPort"
          type="number"
          class="setting-input port-input"
          min="1"
          max="65535"
          placeholder="8080"
          @input="handleProxyPortInput"
        />
      </SettingItem>
    </SettingsGroup>

    <!-- Debug Logging Group -->
    <SettingsGroup :title="$t('settings.advanced.debugging')">
      <!-- Debug Logging Toggle -->
      <SettingItem
        :label="$t('settings.advanced.debugLogging')"
        :help-text="$t('settings.advanced.debugLoggingHelp')"
      >
        <ToggleSwitch
          :model-value="advancedConfig.debugLogging"
          :aria-label="$t('settings.advanced.debugLogging')"
          @update:model-value="handleDebugLoggingChange"
        />
      </SettingItem>

      <!-- Debug Log Path -->
      <SettingItem
        v-show="advancedConfig.debugLogging"
        :label="$t('settings.advanced.debugLogPath')"
      >
        <div class="path-input">
          <input
            :value="advancedConfig.debugLogPath"
            type="text"
            class="setting-input path-text"
            readonly
          />
          <button class="browse-btn" @click="handleBrowseLogPath">
            {{ $t('settings.browse') }}
          </button>
        </div>
      </SettingItem>
    </SettingsGroup>

    <!-- Data Management Group -->
    <SettingsGroup :title="$t('settings.advanced.dataManagement')">
      <!-- Regulation Storage Path -->
      <SettingItem
        label="局方文件保存目录"
        help-text="选择父目录后会自动创建 CCAR规章、规范性文件、标准规范 三个分类目录"
      >
        <div class="path-input">
          <input
            :value="advancedConfig.regulationStoragePath || resolvedStoragePath"
            type="text"
            class="setting-input path-text"
            :placeholder="resolvedStoragePath"
            readonly
          />
          <button class="browse-btn" @click="handleBrowseRegulationPath">浏览</button>
          <button
            v-if="advancedConfig.regulationStoragePath"
            class="browse-btn reset-btn"
            @click="handleResetRegulationPath"
          >
            重置
          </button>
        </div>
        <div v-if="storageDirStatus" class="path-hint">
          {{ storageDirStatus }}
        </div>
      </SettingItem>

      <SettingItem
        label="每天自动同步局方官网"
        help-text="应用运行时每天自动对比官网并下载缺失文件"
      >
        <ToggleSwitch
          :model-value="advancedConfig.regulationAutoSyncEnabled"
          aria-label="每天自动同步局方官网"
          @update:model-value="handleRegulationAutoSyncChange"
        />
      </SettingItem>

      <SettingItem
        v-show="advancedConfig.regulationAutoSyncEnabled"
        label="仅 Wi-Fi 时同步"
        help-text="开启后，未连接 Wi-Fi 时会跳过当天自动同步"
      >
        <ToggleSwitch
          :model-value="advancedConfig.regulationAutoSyncWifiOnly"
          aria-label="仅 Wi-Fi 时同步"
          @update:model-value="handleRegulationWifiOnlyChange"
        />
      </SettingItem>

      <SettingItem
        label="MinerU 在线 OCR"
        help-text="扫描版或坏 PDF 可上传 MinerU 解析，失败后自动回退本地 OCR"
      >
        <ToggleSwitch
          :model-value="advancedConfig.mineruOcrEnabled"
          aria-label="MinerU 在线 OCR"
          @update:model-value="handleMineruOcrEnabledChange"
        />
      </SettingItem>

      <template v-if="advancedConfig.mineruOcrEnabled">
        <SettingItem label="优先在线 OCR" help-text="后台待 OCR 队列会先用 MinerU，再回退本地 OCR">
          <ToggleSwitch
            :model-value="advancedConfig.mineruOcrPreferOnline"
            aria-label="优先在线 OCR"
            @update:model-value="handleMineruOcrPreferOnlineChange"
          />
        </SettingItem>

        <SettingItem label="MinerU API Key">
          <input
            :value="advancedConfig.mineruApiKey"
            type="password"
            class="setting-input wide-input"
            placeholder="输入 MinerU API Key"
            autocomplete="off"
            @input="handleMineruApiKeyInput"
          />
        </SettingItem>
      </template>

      <!-- 开发者专区：未输入密码时隐藏 AI 知识库相关配置，避免普通用户误操作 -->
      <SettingItem
        label="开发者专区"
        help-text="包含生成并同步 AI 知识库到服务器的工具，需要输入开发者密码后才会显示。"
        :show-help-below="true"
      >
        <div v-if="isDeveloperUnlocked" class="dev-unlock">
          <span class="dev-unlock-status">已解锁</span>
          <button type="button" class="action-btn" @click="handleLockDeveloper">
            锁定
          </button>
        </div>
        <div v-else class="dev-unlock">
          <input
            v-model="devPasswordInput"
            type="password"
            class="setting-input dev-password"
            placeholder="请输入开发者密码"
            autocomplete="off"
            :disabled="isUnlocking"
            @keydown.enter.prevent="handleTryUnlock"
          />
          <button
            type="button"
            class="action-btn primary"
            :disabled="isUnlocking"
            @click="handleTryUnlock"
          >
            {{ isUnlocking ? '校验中…' : '解锁' }}
          </button>
        </div>
      </SettingItem>
      <div v-if="devPasswordError && !isDeveloperUnlocked" class="dev-password-error">
        {{ devPasswordError }}
      </div>

      <template v-if="isDeveloperUnlocked">
        <SettingItem
          label="AI 知识库同步到服务器"
          help-text="生成给 OpenClaw 使用的轻量 SQLite 知识库，服务器先查库再按需读取 PDF"
        >
          <ToggleSwitch
            :model-value="advancedConfig.knowledgeServerSyncEnabled"
            aria-label="AI 知识库同步到服务器"
            @update:model-value="handleKnowledgeServerSyncChange"
          />
        </SettingItem>

        <template v-if="advancedConfig.knowledgeServerSyncEnabled">
          <SettingItem
            label="局方更新后自动同步 AI 库"
            help-text="每天局方文件同步完成后，自动刷新知识库并发布到服务器"
          >
            <ToggleSwitch
              :model-value="advancedConfig.knowledgeAutoSyncAfterRegulationUpdate"
              aria-label="局方更新后自动同步 AI 库"
              @update:model-value="handleKnowledgeAutoSyncChange"
            />
          </SettingItem>

          <SettingItem label="同步方式">
            <select
              :value="advancedConfig.knowledgeSyncMode"
              class="setting-select sync-mode-select"
              @change="handleKnowledgeSyncModeChange"
            >
              <option value="api">API 上传</option>
              <option value="ssh">SSH 备用</option>
            </select>
          </SettingItem>

          <template v-if="advancedConfig.knowledgeSyncMode !== 'ssh'">
            <SettingItem label="API 地址">
              <input
                :value="advancedConfig.knowledgeApiUrl"
                type="text"
                class="setting-input wide-input"
                placeholder="https://ccar-api.hudawang.cn"
                @input="handleKnowledgeApiUrlInput"
              />
            </SettingItem>

            <SettingItem label="API Token">
              <input
                :value="advancedConfig.knowledgeApiToken"
                type="password"
                class="setting-input wide-input"
                placeholder="输入知识库同步 Token"
                autocomplete="off"
                @input="handleKnowledgeApiTokenInput"
              />
            </SettingItem>
          </template>

          <template v-else>
            <SettingItem label="AI 知识库服务器">
              <div class="knowledge-grid">
                <input
                  :value="advancedConfig.knowledgeServerHost"
                  type="text"
                  class="setting-input compact-input"
                  placeholder="154.9.27.44"
                  @input="handleKnowledgeHostInput"
                />
                <input
                  :value="advancedConfig.knowledgeServerPort"
                  type="number"
                  class="setting-input port-input"
                  min="1"
                  max="65535"
                  placeholder="7668"
                  @input="handleKnowledgePortInput"
                />
                <input
                  :value="advancedConfig.knowledgeServerUser"
                  type="text"
                  class="setting-input user-input"
                  placeholder="root"
                  @input="handleKnowledgeUserInput"
                />
              </div>
            </SettingItem>

            <SettingItem label="服务器发布目录">
              <input
                :value="advancedConfig.knowledgeServerRemoteDir"
                type="text"
                class="setting-input wide-input"
                placeholder="/www/wwwroot/ccar-knowledge-data"
                @input="handleKnowledgeRemoteDirInput"
              />
            </SettingItem>

            <SettingItem label="SSH 私钥">
              <div class="path-input">
                <input
                  :value="advancedConfig.knowledgeServerKeyPath"
                  type="text"
                  class="setting-input path-text"
                  placeholder="C:\\Users\\wangh\\.ssh\\154.9.27.44_id_ed25519"
                  @input="handleKnowledgeKeyPathInput"
                />
                <button class="browse-btn" @click="handleBrowseKnowledgeKeyPath">浏览</button>
              </div>
            </SettingItem>
          </template>

          <SettingItem label="AI 知识库操作">
            <div class="knowledge-actions">
              <button
                class="action-btn"
                :disabled="isKnowledgeBusy"
                @click="handleExportKnowledgeSnapshot"
              >
                生成本地知识库
              </button>
              <button
                class="action-btn primary"
                :disabled="isKnowledgeBusy"
                @click="handleSyncKnowledgeServer"
              >
                生成并同步服务器
              </button>
            </div>
            <div v-if="knowledgeStatus" class="status-message">
              {{ knowledgeStatus }}
            </div>
          </SettingItem>
        </template>
      </template>

      <!-- Portable Mode Toggle -->
      <SettingItem
        :label="$t('settings.advanced.portableMode')"
        :help-text="$t('settings.advanced.portableModeHelp')"
      >
        <ToggleSwitch
          :model-value="advancedConfig.portableMode"
          :aria-label="$t('settings.advanced.portableMode')"
          @update:model-value="handlePortableModeChange"
        />
      </SettingItem>

      <!-- Portable Mode Warning -->
      <div v-if="showPortableWarning" class="warning-message">
        <span class="warning-icon">⚠️</span>
        <span class="warning-text">{{ $t('settings.advanced.portableModeWarning') }}</span>
      </div>

      <!-- Import/Export Buttons -->
      <SettingItem :label="$t('settings.advanced.importExport')">
        <div class="button-group">
          <button class="action-btn" @click="handleImport">
            {{ $t('settings.advanced.import') }}
          </button>
          <button class="action-btn" @click="handleExport">
            {{ $t('settings.advanced.export') }}
          </button>
        </div>
      </SettingItem>
    </SettingsGroup>
  </div>
</template>

<style scoped>
.advanced-section {
  display: flex;
  flex-direction: column;
  gap: 20px;
  padding-bottom: 8px;
}

.advanced-section :deep(.settings-group) {
  margin-bottom: 0;
}

.setting-select {
  width: 120px;
  padding: 6px 10px;
  border: 1px solid var(--color-border, #38383a);
  border-radius: var(--radius-sm, 6px);
  background: var(--color-input-bg, rgba(118, 118, 128, 0.24));
  color: var(--color-text-primary, #fff);
  font-size: 13px;
}

.setting-select:focus {
  outline: none;
  border-color: var(--color-accent, #0a84ff);
}

.setting-input {
  width: 200px;
  max-width: 100%;
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

.port-input {
  width: 100px;
}

.sync-mode-select {
  width: 140px;
}

.path-input {
  display: flex;
  align-items: center;
  gap: 8px;
  min-width: 0;
  width: 100%;
}

.path-text {
  flex: 1;
  min-width: 0;
}

.path-hint {
  margin-top: 6px;
  color: var(--color-text-secondary, #8e8e93);
  font-size: 12px;
}

.browse-btn {
  flex: 0 0 auto;
  padding: 6px 12px;
  border: 1px solid var(--color-border, #38383a);
  border-radius: var(--radius-sm, 6px);
  background: var(--color-input-bg, rgba(118, 118, 128, 0.24));
  color: var(--color-text-primary, #fff);
  font-size: 13px;
  cursor: pointer;
  transition: background-color var(--transition-fast, 0.15s);
}

.browse-btn:hover {
  background: var(--color-bg-tertiary, #3a3a3c);
}

.knowledge-grid {
  display: grid;
  grid-template-columns: minmax(160px, 1fr) minmax(86px, 96px) minmax(72px, 92px);
  gap: 8px;
  width: 100%;
}

.knowledge-grid .setting-input {
  min-width: 0;
}

.compact-input,
.wide-input {
  width: 100%;
}

.user-input {
  width: 100%;
}

.knowledge-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.status-message {
  margin-top: 8px;
  color: var(--color-text-secondary, #8e8e93);
  font-size: 12px;
  line-height: 1.5;
  overflow-wrap: anywhere;
}

.dev-unlock {
  display: flex;
  align-items: center;
  gap: 8px;
  flex: 1 1 auto;
  min-width: 0;
  justify-content: flex-end;
}

.dev-unlock .dev-password {
  flex: 1;
  min-width: 0;
}

.dev-unlock-status {
  font-size: 12px;
  padding: 2px 10px;
  color: var(--color-accent, #0a84ff);
  border: 1px solid var(--color-accent, #0a84ff);
  border-radius: 999px;
  background: rgba(10, 132, 255, 0.08);
}

.dev-password-error {
  margin: 6px 0 0;
  padding: 6px 12px;
  color: var(--color-warning, #ff9f0a);
  font-size: 12px;
  background: rgba(255, 159, 10, 0.08);
  border: 1px solid rgba(255, 159, 10, 0.35);
  border-radius: var(--radius-sm, 6px);
}

.reset-btn {
  color: var(--color-warning, #ff9f0a);
  border-color: var(--color-warning, #ff9f0a);
}

.warning-message {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px;
  background: var(--color-warning-light, rgba(255, 159, 10, 0.15));
  border: 1px solid var(--color-warning, #ff9f0a);
  border-radius: var(--radius-sm, 6px);
  margin-top: 8px;
}

.warning-icon {
  font-size: 16px;
}

.warning-text {
  color: var(--color-warning, #ff9f0a);
  font-size: 12px;
}

.button-group {
  display: flex;
  gap: 8px;
}

.action-btn {
  min-height: 32px;
  padding: 6px 16px;
  border: 1px solid var(--color-border, #38383a);
  border-radius: var(--radius-sm, 6px);
  background: var(--color-input-bg, rgba(118, 118, 128, 0.24));
  color: var(--color-text-primary, #fff);
  font-size: 13px;
  cursor: pointer;
  transition: background-color var(--transition-fast, 0.15s);
}

.action-btn:hover {
  background: var(--color-bg-tertiary, #3a3a3c);
}

.action-btn.primary {
  border-color: var(--color-accent, #0a84ff);
  color: var(--color-accent, #0a84ff);
}

.action-btn:disabled {
  cursor: not-allowed;
  opacity: 0.55;
}

@media (max-width: 720px) {
  .setting-select,
  .setting-input,
  .port-input,
  .user-input {
    width: 100%;
  }

  .path-input {
    flex-wrap: wrap;
  }

  .path-text {
    flex-basis: 100%;
  }

  .browse-btn {
    flex: 1 1 96px;
  }

  .knowledge-grid {
    grid-template-columns: 1fr;
  }

  .knowledge-actions,
  .button-group {
    width: 100%;
  }

  .knowledge-actions .action-btn,
  .button-group .action-btn {
    flex: 1 1 160px;
  }
}
</style>
