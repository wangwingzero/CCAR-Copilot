/**
 * useToast - 轻量级 Toast 通知 composable
 *
 * 替代原生 alert()，提供非阻塞的消息通知。
 * 支持 success / error / info 三种类型。
 *
 * @example
 * ```ts
 * const { showToast } = useToast()
 * showToast('操作成功', 'success')
 * showToast('操作失败', 'error')
 * ```
 */
import { ref, readonly } from 'vue'

export type ToastType = 'success' | 'error' | 'info'

export interface ToastState {
  visible: boolean
  message: string
  type: ToastType
}

const toastState = ref<ToastState>({
  visible: false,
  message: '',
  type: 'info',
})

let toastTimer: ReturnType<typeof setTimeout> | null = null

/**
 * 显示 Toast 通知
 * @param message - 消息内容
 * @param type - 消息类型（默认 info）
 * @param duration - 显示时长（ms，默认 3000）
 */
function showToast(message: string, type: ToastType = 'info', duration = 3000): void {
  if (toastTimer) clearTimeout(toastTimer)

  toastState.value = {
    visible: true,
    message,
    type,
  }

  toastTimer = setTimeout(() => {
    toastState.value = { ...toastState.value, visible: false }
  }, duration)
}

/** 手动关闭 Toast */
function hideToast(): void {
  if (toastTimer) clearTimeout(toastTimer)
  toastState.value = { ...toastState.value, visible: false }
}

export function useToast() {
  return {
    toast: readonly(toastState),
    showToast,
    hideToast,
  }
}
