<script setup lang="ts">
/**
 * ErrorBoundary - Vue 错误边界组件
 *
 * 捕获子组件渲染错误，展示降级 UI 并提供重试按钮。
 * 防止单个组件异常导致整个应用白屏。
 */
import { ref, onErrorCaptured } from 'vue'

const hasError = ref(false)
const errorMessage = ref('')

onErrorCaptured((err: Error) => {
  hasError.value = true
  errorMessage.value = err.message || '未知错误'
  console.error('[ErrorBoundary] 捕获渲染错误:', err)
  // 返回 false 阻止错误继续向上传播
  return false
})

function handleRetry(): void {
  hasError.value = false
  errorMessage.value = ''
}
</script>

<template>
  <slot v-if="!hasError" />
  <div v-else class="error-boundary">
    <div class="error-boundary__content">
      <div class="error-boundary__icon">⚠️</div>
      <h2 class="error-boundary__title">页面渲染出错</h2>
      <p class="error-boundary__message">{{ errorMessage }}</p>
      <button class="error-boundary__retry" @click="handleRetry">
        重试
      </button>
    </div>
  </div>
</template>

<style scoped>
.error-boundary {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 100%;
  height: 100%;
  min-height: 200px;
  background: var(--color-bg-primary, #f5f7fa);
}

.error-boundary__content {
  text-align: center;
  padding: 32px;
  max-width: 400px;
}

.error-boundary__icon {
  font-size: 48px;
  margin-bottom: 16px;
}

.error-boundary__title {
  font-size: 18px;
  font-weight: 600;
  color: var(--color-text-primary, #333);
  margin: 0 0 8px;
}

.error-boundary__message {
  font-size: 14px;
  color: var(--color-text-secondary, #666);
  margin: 0 0 24px;
  word-break: break-word;
}

.error-boundary__retry {
  padding: 8px 24px;
  font-size: 14px;
  font-weight: 500;
  color: #fff;
  background: var(--color-primary, #4a9eff);
  border: none;
  border-radius: 6px;
  cursor: pointer;
  transition: opacity 0.2s;
}

.error-boundary__retry:hover {
  opacity: 0.85;
}
</style>
