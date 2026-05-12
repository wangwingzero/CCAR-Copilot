<script setup lang="ts">
/**
 * SettingItem - Single setting row with label and control slot
 *
 * Provides a consistent row layout for individual settings:
 * - Label on the left side
 * - Control (toggle, slider, input, etc.) on the right side via slot
 * - Optional help text displayed as tooltip or small text below label
 * - Border separator between items (except last item)
 * - Dark theme styling using CSS variables
 *
 * @validates Requirements 12.2, 12.5
 */

import { ref } from 'vue'

interface Props {
  /** Setting label - displayed on the left side */
  label: string
  /** Optional help text - displayed as tooltip on hover icon */
  helpText?: string
  /** Show help text below label instead of just tooltip (default: false) */
  showHelpBelow?: boolean
}

const props = withDefaults(defineProps<Props>(), {
  helpText: '',
  showHelpBelow: false,
})

// 用户点击问号后展开内联说明（再次点击折叠）。Tauri webview 中原生 :title
// hover tooltip 经常不弹出，而且 hover 在触摸/笔输入下也不可达，因此加入显式
// 点击切换以保证说明始终可访问。
const helpExpanded = ref(false)

function toggleHelp(): void {
  if (props.helpText) {
    helpExpanded.value = !helpExpanded.value
  }
}
</script>

<template>
  <div class="setting-item">
    <div class="item-label-area">
      <div class="label-row">
        <span class="item-label">{{ label }}</span>
        <button
          v-if="helpText"
          type="button"
          class="help-icon"
          :title="helpText"
          :aria-expanded="helpExpanded"
          :aria-label="`查看「${label}」说明`"
          @click="toggleHelp"
        >
          <svg
            xmlns="http://www.w3.org/2000/svg"
            width="14"
            height="14"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
          >
            <circle cx="12" cy="12" r="10" />
            <path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3" />
            <path d="M12 17h.01" />
          </svg>
        </button>
      </div>
      <p v-if="helpText && (showHelpBelow || helpExpanded)" class="help-text">
        {{ helpText }}
      </p>
    </div>
    <div class="item-control">
      <slot></slot>
    </div>
  </div>
</template>

<style scoped>
.setting-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-height: var(--item-height, 40px);
  padding: 12px 0;
  border-bottom: 1px solid var(--border-color, rgba(255, 255, 255, 0.1));
  gap: 16px;
}

/* Remove border from last item */
.setting-item:last-child {
  border-bottom: none;
  padding-bottom: 0;
}

/* First item doesn't need top padding */
.setting-item:first-child {
  padding-top: 0;
}

.item-label-area {
  flex: 1;
  min-width: 0;
}

.label-row {
  display: flex;
  align-items: center;
  gap: 6px;
}

.item-label {
  font-size: 14px;
  font-weight: 400;
  color: var(--text-primary, rgba(255, 255, 255, 0.9));
  line-height: 1.4;
}

.help-icon {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  width: 18px;
  height: 18px;
  padding: 0;
  border: none;
  background: transparent;
  border-radius: 50%;
  color: var(--text-muted, rgba(255, 255, 255, 0.4));
  cursor: pointer;
  transition: color 0.15s ease, background-color 0.15s ease;
}

.help-icon:hover {
  color: var(--text-secondary, rgba(255, 255, 255, 0.6));
  background: var(--bg-hover, rgba(0, 0, 0, 0.06));
}

.help-icon[aria-expanded='true'] {
  color: var(--primary-color, #1890ff);
  background: rgba(24, 144, 255, 0.12);
}

.help-icon:focus-visible {
  outline: 2px solid var(--primary-color, #1890ff);
  outline-offset: 1px;
}

.help-text {
  margin: 4px 0 0 0;
  font-size: 12px;
  color: var(--text-muted, rgba(255, 255, 255, 0.4));
  line-height: 1.4;
}

.item-control {
  flex: 0 1 min(560px, 62%);
  min-width: 0;
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  justify-content: center;
  gap: 6px;
}

/* Ensure controls have consistent minimum width */
.item-control :deep(input),
.item-control :deep(select) {
  min-width: 120px;
}

.item-control :deep(.path-input),
.item-control :deep(.knowledge-grid),
.item-control :deep(.knowledge-actions),
.item-control :deep(.status-message),
.item-control :deep(.button-group),
.item-control :deep(.wide-input) {
  align-self: stretch;
}

/* Ensure toggle switches don't shrink */
.item-control :deep(.toggle-switch) {
  flex-shrink: 0;
}

@media (max-width: 720px) {
  .setting-item {
    align-items: stretch;
    flex-direction: column;
    gap: 8px;
  }

  .item-control {
    flex: none;
    width: 100%;
    align-items: stretch;
  }

  .item-control :deep(input),
  .item-control :deep(select) {
    min-width: 0;
  }
}
</style>
