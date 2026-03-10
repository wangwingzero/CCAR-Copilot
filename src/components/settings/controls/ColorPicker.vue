<script setup lang="ts">
/**
 * ColorPicker - Color selection control with preview
 *
 * Provides a styled color picker with:
 * - Native color input element for color selection
 * - Preview square showing the currently selected color
 * - Dark theme styling using CSS variables
 * - Immediate visual feedback when color changes
 *
 * The preview element's background color always matches the selected color,
 * providing instant visual feedback to the user.
 *
 * @validates Requirements 4.3, 4.6
 * @property Property 8: Color Preview Consistency
 *   For any valid hex color value selected in the color picker,
 *   the preview element's background color SHALL match the selected color.
 */

interface Props {
  /** Current color value (hex format like "#FF0000") */
  modelValue: string
}

defineProps<Props>()

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void
}>()

/**
 * Handle input event from color picker
 * Emits the selected color value in uppercase hex format
 */
function handleInput(event: Event) {
  const target = event.target as HTMLInputElement
  // Normalize to uppercase hex format
  const color = target.value.toUpperCase()
  emit('update:modelValue', color)
}
</script>

<template>
  <div class="color-picker">
    <div
      class="color-preview"
      :style="{ backgroundColor: modelValue }"
      :title="modelValue"
    ></div>
    <input
      type="color"
      class="color-input"
      :value="modelValue"
      @input="handleInput"
    />
  </div>
</template>

<style scoped>
.color-picker {
  display: flex;
  align-items: center;
  gap: 8px;
}

.color-preview {
  width: 28px;
  height: 28px;
  border-radius: 4px;
  border: 1px solid var(--border-color, rgba(255, 255, 255, 0.1));
  flex-shrink: 0;
  transition: border-color 0.15s ease;
}

.color-preview:hover {
  border-color: var(--text-muted, rgba(255, 255, 255, 0.4));
}

.color-input {
  width: 40px;
  height: 28px;
  padding: 0;
  border: 1px solid var(--border-color, rgba(255, 255, 255, 0.1));
  border-radius: 4px;
  background: var(--bg-secondary, #252525);
  cursor: pointer;
  transition: border-color 0.15s ease;
  /* Hide the default color input styling */
  -webkit-appearance: none;
  -moz-appearance: none;
  appearance: none;
}

.color-input::-webkit-color-swatch-wrapper {
  padding: 2px;
}

.color-input::-webkit-color-swatch {
  border: none;
  border-radius: 2px;
}

.color-input::-moz-color-swatch {
  border: none;
  border-radius: 2px;
}

.color-input:hover {
  border-color: var(--text-muted, rgba(255, 255, 255, 0.4));
}

.color-input:focus {
  outline: none;
  border-color: var(--accent-primary, #4285f4);
  box-shadow: 0 0 0 2px rgba(66, 133, 244, 0.2);
}
</style>
