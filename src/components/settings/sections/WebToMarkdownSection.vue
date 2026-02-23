<template>
  <div class="web-to-markdown-section">
    <SettingsGroup :title="$t('settings.webToMarkdown.title')">
      <!-- Include Images Toggle -->
      <SettingItem
        :label="$t('settings.webToMarkdown.includeImages')"
        :help-text="$t('settings.webToMarkdown.includeImagesHelp')"
      >
        <ToggleSwitch
          :model-value="webToMarkdownConfig.includeImages"
          :aria-label="$t('settings.webToMarkdown.includeImages')"
          @update:model-value="handleIncludeImagesChange"
        />
      </SettingItem>

      <!-- Include Links Toggle -->
      <SettingItem
        :label="$t('settings.webToMarkdown.includeLinks')"
        :help-text="$t('settings.webToMarkdown.includeLinksHelp')"
      >
        <ToggleSwitch
          :model-value="webToMarkdownConfig.includeLinks"
          :aria-label="$t('settings.webToMarkdown.includeLinks')"
          @update:model-value="handleIncludeLinksChange"
        />
      </SettingItem>

      <!-- Timeout Slider -->
      <SettingItem
        :label="$t('settings.webToMarkdown.timeout')"
        :help-text="$t('settings.webToMarkdown.timeoutHelp')"
      >
        <SliderControl
          :model-value="webToMarkdownConfig.timeout"
          :min="5"
          :max="120"
          :step="1"
          suffix="s"
          @update:model-value="handleTimeoutChange"
        />
      </SettingItem>
    </SettingsGroup>
  </div>
</template>

<script setup lang="ts">
/**
 * WebToMarkdownSection - Web to Markdown (网页转MD) Settings Section
 *
 * Provides configuration options for web page to markdown conversion:
 * - Include images toggle (whether to include images in the converted markdown)
 * - Include links toggle (whether to preserve hyperlinks in the converted markdown)
 * - Timeout slider (5-120 seconds, controls how long to wait for page loading)
 *
 * Uses the reusable settings control components:
 * - SettingsGroup for card-style grouping
 * - SettingItem for consistent row layout with help text
 * - SliderControl for timeout with value clamping
 * - ToggleSwitch for boolean settings
 *
 * @validates Requirements 5.1, 5.2, 5.3, 5.4, 5.5
 */

import { computed } from 'vue'
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
// Computed
// ============================================

/**
 * Web to Markdown configuration from store
 * Provides reactive access to current settings
 */
const webToMarkdownConfig = computed(() => settingsStore.webToMarkdown)

// ============================================
// Event Handlers
// ============================================

/**
 * Handle include images toggle change
 * When enabled, images from the web page will be included in the markdown output
 *
 * @param value - New include images state
 * @validates Requirements 5.2
 */
function handleIncludeImagesChange(value: boolean): void {
  settingsStore.updateWebToMarkdown({ includeImages: value })
}

/**
 * Handle include links toggle change
 * When enabled, hyperlinks from the web page will be preserved in the markdown output
 *
 * @param value - New include links state
 * @validates Requirements 5.3
 */
function handleIncludeLinksChange(value: boolean): void {
  settingsStore.updateWebToMarkdown({ includeLinks: value })
}

/**
 * Handle timeout slider change
 * Updates the store with the new timeout value
 * The SliderControl already clamps the value to [5, 120]
 *
 * @param value - New timeout value in seconds (5-120)
 * @validates Requirements 5.4
 */
function handleTimeoutChange(value: number): void {
  settingsStore.updateWebToMarkdown({ timeout: value })
}
</script>

<style scoped>
.web-to-markdown-section {
  /* Section container - inherits dark theme from parent */
}
</style>
