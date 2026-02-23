<template>
  <div class="mouse-highlight-section">
    <SettingsGroup :title="$t('settings.mouseHighlight.title')">
      <MouseHighlightSettings
        :config="mouseHighlightConfig"
        @update:config="handleConfigUpdate"
      />
    </SettingsGroup>
  </div>
</template>

<script setup lang="ts">
/**
 * MouseHighlightSection - Mouse Highlight (鼠标高亮) Settings Section
 *
 * Wraps the MouseHighlightSettings component in a SettingsGroup for
 * consistent styling with other settings sections.
 *
 * The MouseHighlightSettings component provides comprehensive configuration:
 * - Enable/disable toggle for mouse highlight feature
 * - Theme selection (4 preset color themes)
 * - Effect toggles (circle, spotlight, cursor magnify, click ripple)
 * - Parameter sliders for each effect
 *
 * Uses the reusable settings control components:
 * - SettingsGroup for card-style grouping
 * - MouseHighlightSettings for the actual configuration UI
 *
 * @validates Requirements 4.1, 4.2, 4.3, 4.4, 4.5, 4.6
 */

import { computed } from 'vue'
import { useSettingsStore } from '@/stores/settings'
import SettingsGroup from '@/components/settings/controls/SettingsGroup.vue'
import MouseHighlightSettings from '@/components/settings/MouseHighlightSettings.vue'
import type { MouseHighlightConfig } from '@/types'

// ============================================
// Store
// ============================================

const settingsStore = useSettingsStore()

// ============================================
// Computed
// ============================================

/**
 * Mouse highlight configuration from store
 * Provides reactive access to current settings
 */
const mouseHighlightConfig = computed(() => settingsStore.mouseHighlight)

// ============================================
// Event Handlers
// ============================================

/**
 * Handle configuration update from MouseHighlightSettings
 * Updates the store with the new configuration
 *
 * @param config - Updated mouse highlight configuration
 * @validates Requirements 4.2, 4.3, 4.4, 4.5, 4.6
 */
function handleConfigUpdate(config: MouseHighlightConfig): void {
  settingsStore.updateMouseHighlight(config)
}
</script>

<style scoped>
.mouse-highlight-section {
  /* Section container - inherits dark theme from parent */
}

/* Override SettingsGroup content padding for MouseHighlightSettings */
.mouse-highlight-section :deep(.group-content) {
  padding: 0;
}

/* MouseHighlightSettings has its own internal padding */
.mouse-highlight-section :deep(.mouse-highlight-settings) {
  padding: var(--content-padding, 24px);
  padding-top: 16px;
}
</style>
