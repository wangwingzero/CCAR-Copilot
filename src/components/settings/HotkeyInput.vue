<template>
  <div
    class="hotkey-input"
    :class="{ 'is-recording': isRecording, 'has-error': hasConflict }"
    tabindex="0"
    @click="startRecording"
    @keydown="handleKeyDown"
    @blur="stopRecording"
  >
    <span v-if="isRecording" class="recording-hint">
      按下快捷键组合...
    </span>
    <span v-else-if="displayValue" class="hotkey-display">
      {{ displayValue }}
    </span>
    <span v-else class="placeholder">
      点击设置热键
    </span>

    <button
      v-if="modelValue && !isRecording"
      class="clear-btn"
      title="清除"
      @click.stop="handleClear"
    >
      ✕
    </button>
  </div>
</template>

<script setup lang="ts">
/**
 * 热键输入组件
 *
 * 用于捕获和显示快捷键组合。
 * 支持 Ctrl、Alt、Shift、Meta 修饰键 + 普通键。
 *
 * @validates Requirements 3.5, 3.6
 */

import { ref, computed } from 'vue'

// ============================================
// Props & Emits
// ============================================

interface Props {
  /** 当前热键值（如 "Ctrl+Shift+A"） */
  modelValue: string
}

const props = defineProps<Props>()

const emit = defineEmits<{
  (e: 'update:modelValue', value: string): void
  (e: 'change', value: string): void
}>()

// ============================================
// State
// ============================================

/** 是否正在录制 */
const isRecording = ref(false)

/** 是否有冲突 */
const hasConflict = ref(false)

/** 当前按下的修饰键 */
const modifiers = ref<Set<string>>(new Set())

// ============================================
// Computed
// ============================================

/** 显示用的热键文本 */
const displayValue = computed(() => {
  return formatHotkey(props.modelValue)
})

// ============================================
// Methods
// ============================================

/**
 * 格式化热键显示
 */
function formatHotkey(shortcut: string): string {
  if (!shortcut) return ''

  // 将内部格式转换为显示格式
  return shortcut
    .replace(/Ctrl/g, 'Ctrl')
    .replace(/Alt/g, 'Alt')
    .replace(/Shift/g, 'Shift')
    .replace(/Meta/g, '⌘')
    .replace(/\+/g, ' + ')
}

/**
 * 开始录制
 */
function startRecording(): void {
  isRecording.value = true
  modifiers.value.clear()
  hasConflict.value = false
}

/**
 * 停止录制
 */
function stopRecording(): void {
  isRecording.value = false
  modifiers.value.clear()
}

/**
 * 处理键盘按下
 */
function handleKeyDown(event: KeyboardEvent): void {
  if (!isRecording.value) return

  event.preventDefault()
  event.stopPropagation()

  // 收集修饰键
  const mods: string[] = []
  if (event.ctrlKey) mods.push('Ctrl')
  if (event.altKey) mods.push('Alt')
  if (event.shiftKey) mods.push('Shift')
  if (event.metaKey) mods.push('Meta')

  // 获取主键（非修饰键）
  const key = event.key

  // 忽略单独的修饰键
  if (['Control', 'Alt', 'Shift', 'Meta'].includes(key)) {
    modifiers.value = new Set(mods)
    return
  }

  // 需要至少一个修饰键
  if (mods.length === 0) {
    hasConflict.value = true
    return
  }

  // 格式化主键
  let mainKey = key.toUpperCase()
  if (key.length === 1) {
    mainKey = key.toUpperCase()
  } else if (key.startsWith('Arrow')) {
    mainKey = key.replace('Arrow', '')
  } else if (key === ' ') {
    mainKey = 'Space'
  } else if (key === 'Escape') {
    // ESC 取消录制
    stopRecording()
    return
  }

  // 组合热键字符串
  const shortcut = [...mods, mainKey].join('+')

  // 更新值
  emit('update:modelValue', shortcut)
  emit('change', shortcut)

  // 停止录制
  stopRecording()
}

/**
 * 清除热键
 */
function handleClear(): void {
  emit('update:modelValue', '')
  emit('change', '')
}
</script>

<style scoped>
.hotkey-input {
  display: flex;
  align-items: center;
  justify-content: space-between;
  min-width: 150px;
  padding: 6px 12px;
  border: 1px solid rgba(255, 255, 255, 0.2);
  border-radius: 4px;
  background: rgba(0, 0, 0, 0.3);
  cursor: pointer;
  transition: all 0.1s;
  user-select: none;
}

.hotkey-input:hover {
  border-color: rgba(255, 255, 255, 0.3);
}

.hotkey-input:focus {
  outline: none;
  border-color: rgba(66, 133, 244, 0.5);
}

.hotkey-input.is-recording {
  border-color: rgba(66, 133, 244, 0.8);
  background: rgba(66, 133, 244, 0.1);
}

.hotkey-input.has-error {
  border-color: rgba(244, 67, 54, 0.8);
}

.recording-hint {
  color: rgba(66, 133, 244, 0.9);
  font-size: 12px;
  animation: pulse 1s infinite;
}

@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.5; }
}

.hotkey-display {
  color: rgba(255, 255, 255, 0.9);
  font-size: 12px;
  font-family: monospace;
}

.placeholder {
  color: rgba(255, 255, 255, 0.4);
  font-size: 12px;
}

.clear-btn {
  padding: 2px 6px;
  border: none;
  border-radius: 2px;
  background: transparent;
  color: rgba(255, 255, 255, 0.5);
  font-size: 10px;
  cursor: pointer;
  transition: all 0.1s;
}

.clear-btn:hover {
  background: rgba(244, 67, 54, 0.3);
  color: rgba(255, 255, 255, 0.9);
}
</style>
