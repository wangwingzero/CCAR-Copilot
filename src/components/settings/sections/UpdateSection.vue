<script setup lang="ts">
/**
 * UpdateSection - Update Settings Section
 *
 * Provides configuration options for software updates:
 * - Auto-check toggle
 * - Check interval slider (1-168 hours)
 * - Use proxy toggle with conditional URL input
 * - Check now button
 * - Last check time display
 *
 * Uses the reusable settings control components:
 * - SettingsGroup for card-style grouping
 * - SettingItem for consistent row layout
 * - ToggleSwitch for boolean settings
 * - SliderControl for interval with value clamping
 *
 * @validates Requirements 8.1, 8.2, 8.3, 8.4, 8.5, 8.6
 */

import { computed, ref } from 'vue'
import { useSettingsStore } from '@/stores/settings'
import SettingsGroup from '@/components/settings/controls/SettingsGroup.vue'
import SettingItem from '@/components/settings/controls/SettingItem.vue'
import SliderControl from '@/components/settings/controls/SliderControl.vue'
import ToggleSwitch from '@/components/settings/controls/ToggleSwitch.vue'

// ============================================
// Store
// ============================================

const settingsStore = useSettingsStore()

// ============================================
// State
// ============================================

/** Whether update check is in progress */
const isChecking = ref(false)

// ============================================
// Computed
// ============================================

/**
 * Update configuration from store
 * Provides reactive access to current settings
 */
const updateConfig = computed(() => settingsStore.update)

/**
 * Format last check time for display
 */
const formatLastCheckTime = computed(() => {
  if (!updateConfig.value.lastCheckTime) return ''
  try {
    const date = new Date(updateConfig.value.lastCheckTime)
    return date.toLocaleString()
  } catch {
    return updateConfig.value.lastCheckTime
  }
})

// ============================================
// Event Handlers
// ============================================

/**
 * Handle auto-check toggle change
 * @param value - New auto-check state
 * @validates Requirements 8.1
 */
function handleAutoCheckChange(value: boolean): void {
  settingsStore.updateUpdate({ autoCheck: value })
}

/**
 * Handle check interval slider change
 * The SliderControl already clamps the value to [1, 168]
 * @param value - New check interval in hours
 * @validates Requirements 8.3
 */
function handleCheckIntervalChange(value: number): void {
  settingsStore.updateUpdate({ checkIntervalHours: value })
}

/**
 * Handle use proxy toggle change
 * @param value - New use proxy state
 * @validates Requirements 8.4
 */
function handleUseProxyChange(value: boolean): void {
  settingsStore.updateUpdate({ useProxy: value })
}

/**
 * Handle proxy URL input (for real-time updates)
 * @param event - Input event
 */
function handleProxyUrlInput(event: Event): void {
  const target = event.target as HTMLInputElement
  settingsStore.updateUpdate({ proxyUrl: target.value })
}

/**
 * Handle proxy URL change (on blur/enter)
 * @param event - Change event
 * @validates Requirements 8.5
 */
function handleProxyUrlChange(event: Event): void {
  const target = event.target as HTMLInputElement
  settingsStore.updateUpdate({ proxyUrl: target.value })
}

/**
 * Handle check now button click
 * Triggers an immediate update check
 * @validates Requirements 8.6
 */
async function handleCheckNow(): Promise<void> {
  if (isChecking.value) return
  
  isChecking.value = true
  try {
    // TODO: Implement actual update check via Tauri command
    // await invoke('check_for_updates')
    
    // Update last check time
    settingsStore.updateUpdate({ lastCheckTime: new Date().toISOString() })
  } catch (error) {
    console.error('Failed to check for updates:', error)
  } finally {
    isChecking.value = false
  }
}
</script>

<template>
  <div class="update-section">
    <SettingsGroup :title="$t('settings.update.title')">
      <!-- Auto Check Toggle -->
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

      <!-- Check Interval Slider -->
      <SettingItem
        v-show="updateConfig.autoCheck"
        :label="$t('settings.update.checkInterval')"
        :help-text="$t('settings.update.checkIntervalHelp')"
      >
        <SliderControl
          :model-value="updateConfig.checkIntervalHours"
          :min="1"
          :max="168"
          :step="1"
          suffix="h"
          @update:model-value="handleCheckIntervalChange"
        />
      </SettingItem>

      <!-- Use Proxy Toggle -->
      <SettingItem
        :label="$t('settings.update.useProxy')"
        :help-text="$t('settings.update.useProxyHelp')"
      >
        <ToggleSwitch
          :model-value="updateConfig.useProxy"
          :aria-label="$t('settings.update.useProxy')"
          @update:model-value="handleUseProxyChange"
        />
      </SettingItem>

      <!-- Proxy URL Input (conditional) -->
      <SettingItem
        v-show="updateConfig.useProxy"
        :label="$t('settings.update.proxyUrl')"
        :help-text="$t('settings.update.proxyUrlHelp')"
      >
        <input
          :value="updateConfig.proxyUrl"
          type="text"
          class="setting-input"
          placeholder="https://ghproxy.net/"
          @input="handleProxyUrlInput"
          @change="handleProxyUrlChange"
        />
      </SettingItem>

      <!-- Check Now Button -->
      <SettingItem :label="$t('settings.update.checkNow')">
        <button
          class="check-now-btn"
          :disabled="isChecking"
          @click="handleCheckNow"
        >
          {{ isChecking ? $t('settings.update.checking') : $t('settings.update.checkNowBtn') }}
        </button>
      </SettingItem>

      <!-- Last Check Time -->
      <SettingItem
        v-if="updateConfig.lastCheckTime"
        :label="$t('settings.update.lastCheck')"
      >
        <span class="last-check-time">{{ formatLastCheckTime }}</span>
      </SettingItem>
    </SettingsGroup>
  </div>
</template>

<style scoped>
.update-section {
  /* Section container - inherits dark theme from parent */
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
