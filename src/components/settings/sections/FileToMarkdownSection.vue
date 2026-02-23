<template>
  <div class="file-to-markdown-section">
    <SettingsGroup :title="$t('settings.fileToMarkdown.title')">
      <!-- Configuration Prompt when API token is empty -->
      <div v-if="!fileToMarkdownConfig.apiToken" class="config-prompt">
        <div class="prompt-icon">⚠️</div>
        <div class="prompt-text">{{ $t('settings.fileToMarkdown.configPrompt') }}</div>
      </div>

      <!-- API Token Input (password masked) -->
      <SettingItem
        :label="$t('settings.fileToMarkdown.apiToken')"
        :help-text="$t('settings.fileToMarkdown.apiTokenHelp')"
      >
        <div class="token-input-group">
          <input
            type="password"
            class="token-input"
            :value="fileToMarkdownConfig.apiToken"
            :placeholder="$t('settings.fileToMarkdown.apiTokenPlaceholder')"
            @input="handleApiTokenChange"
          />
        </div>
      </SettingItem>

      <!-- Link to obtain API token -->
      <SettingItem :label="$t('settings.fileToMarkdown.getToken')">
        <a
          href="https://mineru.net/"
          target="_blank"
          rel="noopener noreferrer"
          class="token-link"
        >
          {{ $t('settings.fileToMarkdown.getTokenLink') }}
          <svg xmlns="http://www.w3.org/2000/svg" width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="external-link-icon">
            <path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6" />
            <polyline points="15 3 21 3 21 9" />
            <line x1="10" y1="14" x2="21" y2="3" />
          </svg>
        </a>
      </SettingItem>

      <!-- Model Version Selector -->
      <SettingItem
        :label="$t('settings.fileToMarkdown.modelVersion')"
        :help-text="$t('settings.fileToMarkdown.modelVersionHelp')"
      >
        <select
          class="model-select"
          :value="fileToMarkdownConfig.modelVersion"
          @change="handleModelVersionChange"
        >
          <option value="pipeline">Pipeline</option>
          <option value="vlm">VLM</option>
        </select>
      </SettingItem>
    </SettingsGroup>
  </div>
</template>

<script setup lang="ts">
/**
 * FileToMarkdownSection - File to Markdown Settings Section
 * @validates Requirements 6.1, 6.2, 6.3, 6.4, 6.5
 */

import { computed } from 'vue'
import { useSettingsStore } from '@/stores/settings'
import SettingsGroup from '@/components/settings/controls/SettingsGroup.vue'
import SettingItem from '@/components/settings/controls/SettingItem.vue'

const settingsStore = useSettingsStore()

const fileToMarkdownConfig = computed(() => settingsStore.fileToMarkdown)

function handleApiTokenChange(event: Event): void {
  const target = event.target as HTMLInputElement
  settingsStore.updateFileToMarkdown({ apiToken: target.value })
}

function handleModelVersionChange(event: Event): void {
  const target = event.target as HTMLSelectElement
  const value = target.value as 'pipeline' | 'vlm'
  settingsStore.updateFileToMarkdown({ modelVersion: value })
}
</script>

<style scoped>
.config-prompt {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 16px;
  margin-bottom: 16px;
  background: rgba(255, 193, 7, 0.1);
  border: 1px solid rgba(255, 193, 7, 0.3);
  border-radius: 6px;
}

.prompt-icon { font-size: 18px; flex-shrink: 0; }
.prompt-text { font-size: 13px; color: rgba(255, 193, 7, 0.9); line-height: 1.4; }

.token-input-group { display: flex; align-items: center; gap: 8px; }

.token-input {
  width: 200px;
  padding: 6px 10px;
  font-size: 13px;
  color: var(--text-primary, rgba(255, 255, 255, 0.9));
  background: var(--bg-hover, rgba(255, 255, 255, 0.1));
  border: 1px solid var(--border-color, rgba(255, 255, 255, 0.1));
  border-radius: 4px;
  outline: none;
}

.token-input:focus {
  border-color: var(--accent-primary, #4285f4);
  background: rgba(255, 255, 255, 0.15);
}

.token-link {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  color: var(--accent-primary, #4285f4);
  font-size: 13px;
  text-decoration: none;
}

.token-link:hover { color: var(--accent-hover, #5a9cf5); text-decoration: underline; }

.model-select {
  min-width: 140px;
  padding: 6px 10px;
  font-size: 13px;
  color: var(--text-primary, rgba(255, 255, 255, 0.9));
  background: var(--bg-hover, rgba(255, 255, 255, 0.1));
  border: 1px solid var(--border-color, rgba(255, 255, 255, 0.1));
  border-radius: 4px;
  cursor: pointer;
}

.model-select:focus { border-color: var(--accent-primary, #4285f4); }
.model-select option { background: var(--bg-secondary, #252525); }
</style>
