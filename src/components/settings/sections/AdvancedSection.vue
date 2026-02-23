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
      <SettingItem
        v-show="advancedConfig.proxyEnabled"
        :label="$t('settings.advanced.proxyType')"
      >
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
      <SettingItem
        v-show="advancedConfig.proxyEnabled"
        :label="$t('settings.advanced.proxyHost')"
      >
        <input
          :value="advancedConfig.proxyHost"
          type="text"
          class="setting-input"
          placeholder="127.0.0.1"
          @input="handleProxyHostInput"
        />
      </SettingItem>

      <!-- Proxy Port Input -->
      <SettingItem
        v-show="advancedConfig.proxyEnabled"
        :label="$t('settings.advanced.proxyPort')"
      >
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
    // TODO: Implement import logic
    console.log('Import from:', selected)
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
    // TODO: Implement export logic
    console.log('Export to:', selected)
  }
}
</script>

<style scoped>
.advanced-section {
  display: flex;
  flex-direction: column;
  gap: 24px;
}

.setting-select {
  width: 120px;
  padding: 6px 10px;
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: 4px;
  background: rgba(255, 255, 255, 0.05);
  color: rgba(255, 255, 255, 0.9);
  font-size: 13px;
}

.setting-select:focus {
  outline: none;
  border-color: #4285f4;
}

.setting-input {
  width: 200px;
  padding: 6px 10px;
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: 4px;
  background: rgba(255, 255, 255, 0.05);
  color: rgba(255, 255, 255, 0.9);
  font-size: 13px;
}

.setting-input:focus {
  outline: none;
  border-color: #4285f4;
}

.setting-input::placeholder {
  color: rgba(255, 255, 255, 0.4);
}

.port-input {
  width: 100px;
}

.path-input {
  display: flex;
  gap: 8px;
}

.path-text {
  flex: 1;
}

.browse-btn {
  padding: 6px 12px;
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: 4px;
  background: rgba(255, 255, 255, 0.05);
  color: rgba(255, 255, 255, 0.9);
  font-size: 13px;
  cursor: pointer;
  transition: background-color 0.1s;
}

.browse-btn:hover {
  background: rgba(255, 255, 255, 0.1);
}

.warning-message {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 12px;
  background: rgba(255, 193, 7, 0.1);
  border: 1px solid rgba(255, 193, 7, 0.3);
  border-radius: 4px;
  margin-top: 8px;
}

.warning-icon {
  font-size: 16px;
}

.warning-text {
  color: rgba(255, 193, 7, 0.9);
  font-size: 12px;
}

.button-group {
  display: flex;
  gap: 8px;
}

.action-btn {
  padding: 6px 16px;
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: 4px;
  background: rgba(255, 255, 255, 0.05);
  color: rgba(255, 255, 255, 0.9);
  font-size: 13px;
  cursor: pointer;
  transition: background-color 0.1s;
}

.action-btn:hover {
  background: rgba(255, 255, 255, 0.1);
}
</style>
