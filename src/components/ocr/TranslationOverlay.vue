/**
 * 屏幕翻译覆盖层组件
 *
 * 在截图上方显示翻译结果的覆盖层，支持：
 * - OCR 识别后自动翻译
 * - 翻译结果覆盖显示
 * - 点击复制翻译文本
 * - 切换显示原文/译文
 */
<template>
  <div
    class="translation-overlay"
    v-if="visible && translatedBlocks.length > 0"
    :style="overlayStyle"
  >
    <!-- 翻译文本块 -->
    <div
      v-for="(block, index) in translatedBlocks"
      :key="index"
      class="translation-block"
      :style="getBlockStyle(block)"
      @click="copyBlockText(block)"
      :title="t('translation.clickToCopy')"
    >
      <div class="original-text" v-if="showOriginal">
        {{ block.original }}
      </div>
      <div class="translated-text">
        {{ block.translated }}
      </div>
    </div>

    <!-- 控制栏 -->
    <div class="overlay-controls">
      <button
        class="control-btn"
        @click="toggleShowOriginal"
        :title="showOriginal ? t('translation.hideOriginal') : t('translation.showOriginal')"
      >
        {{ showOriginal ? '隐藏原文' : '显示原文' }}
      </button>
      <button
        class="control-btn"
        @click="copyAllText"
        :title="t('translation.copyAll')"
      >
        复制全部
      </button>
      <button
        class="control-btn close-btn"
        @click="close"
        :title="t('common.close')"
      >
        ✕
      </button>
    </div>

    <!-- 复制成功提示 -->
    <div class="copy-toast" v-if="showCopyToast">
      {{ t('screenshot.copied') }}
    </div>
  </div>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'
import { useI18n } from 'vue-i18n'

const { t } = useI18n()

// Props
interface TranslationBlock {
  /** 原始文本 */
  original: string
  /** 翻译后文本 */
  translated: string
  /** 文本区域边界 (相对于截图) */
  bounds: {
    x: number
    y: number
    width: number
    height: number
  }
}

interface Props {
  /** 是否显示覆盖层 */
  visible: boolean
  /** 翻译文本块列表 */
  translatedBlocks: TranslationBlock[]
  /** 覆盖层位置 */
  position?: {
    x: number
    y: number
    width: number
    height: number
  }
}

const props = withDefaults(defineProps<Props>(), {
  visible: false,
  translatedBlocks: () => [],
  position: () => ({ x: 0, y: 0, width: 800, height: 600 }),
})

// Emits
const emit = defineEmits<{
  (e: 'close'): void
  (e: 'copy', text: string): void
}>()

// 状态
const showOriginal = ref(false)
const showCopyToast = ref(false)

// 覆盖层样式
const overlayStyle = computed(() => ({
  left: `${props.position?.x ?? 0}px`,
  top: `${props.position?.y ?? 0}px`,
  width: `${props.position?.width ?? 800}px`,
  height: `${props.position?.height ?? 600}px`,
}))

// 获取文本块样式
function getBlockStyle(block: TranslationBlock) {
  return {
    left: `${block.bounds.x}px`,
    top: `${block.bounds.y}px`,
    width: `${block.bounds.width}px`,
    minHeight: `${block.bounds.height}px`,
  }
}

// 切换显示原文
function toggleShowOriginal() {
  showOriginal.value = !showOriginal.value
}

// 复制单个文本块
async function copyBlockText(block: TranslationBlock) {
  const text = showOriginal.value
    ? `${block.original}\n---\n${block.translated}`
    : block.translated

  await copyToClipboard(text)
  emit('copy', text)
}

// 复制全部文本
async function copyAllText() {
  const allText = props.translatedBlocks
    .map((block) =>
      showOriginal.value
        ? `${block.original}\n---\n${block.translated}`
        : block.translated
    )
    .join('\n\n')

  await copyToClipboard(allText)
  emit('copy', allText)
}

// 复制到剪贴板
async function copyToClipboard(text: string) {
  try {
    await navigator.clipboard.writeText(text)
    showCopyToast.value = true
    setTimeout(() => {
      showCopyToast.value = false
    }, 2000)
  } catch (e) {
    console.error('复制失败:', e)
  }
}

// 关闭覆盖层
function close() {
  emit('close')
}
</script>

<style scoped>
.translation-overlay {
  position: fixed;
  z-index: 9999;
  pointer-events: auto;
  background: transparent;
}

.translation-block {
  position: absolute;
  padding: 4px 8px;
  background: rgba(0, 0, 0, 0.85);
  color: white;
  border-radius: 4px;
  font-size: 14px;
  line-height: 1.4;
  cursor: pointer;
  transition: all 0.2s;
  overflow: hidden;
  word-wrap: break-word;
}

.translation-block:hover {
  background: rgba(0, 0, 0, 0.95);
  transform: scale(1.02);
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.3);
}

.original-text {
  color: rgba(255, 255, 255, 0.7);
  font-size: 12px;
  margin-bottom: 4px;
  padding-bottom: 4px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.2);
}

.translated-text {
  color: white;
}

.overlay-controls {
  position: absolute;
  bottom: -40px;
  left: 50%;
  transform: translateX(-50%);
  display: flex;
  gap: 8px;
  padding: 8px;
  background: rgba(0, 0, 0, 0.8);
  border-radius: 8px;
}

.control-btn {
  padding: 6px 12px;
  background: rgba(255, 255, 255, 0.1);
  color: white;
  border: none;
  border-radius: 4px;
  font-size: 12px;
  cursor: pointer;
  transition: background 0.2s;
}

.control-btn:hover {
  background: rgba(255, 255, 255, 0.2);
}

.close-btn {
  background: rgba(255, 0, 0, 0.3);
}

.close-btn:hover {
  background: rgba(255, 0, 0, 0.5);
}

.copy-toast {
  position: fixed;
  bottom: 80px;
  left: 50%;
  transform: translateX(-50%);
  padding: 8px 16px;
  background: rgba(0, 200, 0, 0.9);
  color: white;
  border-radius: 4px;
  font-size: 14px;
  animation: fadeInOut 2s ease-in-out;
}

@keyframes fadeInOut {
  0% {
    opacity: 0;
    transform: translateX(-50%) translateY(10px);
  }
  20% {
    opacity: 1;
    transform: translateX(-50%) translateY(0);
  }
  80% {
    opacity: 1;
    transform: translateX(-50%) translateY(0);
  }
  100% {
    opacity: 0;
    transform: translateX(-50%) translateY(-10px);
  }
}
</style>
