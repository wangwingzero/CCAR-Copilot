<template>
  <Transition name="popup">
    <div v-if="visible" class="upgrade-prompt" :class="promptClass">
      <!-- 关闭按钮 -->
      <button v-if="dismissible" class="dismiss-btn" @click="handleDismiss">
        ✕
      </button>

      <!-- 图标 -->
      <div class="prompt-icon" :class="iconClass">
        {{ iconEmoji }}
      </div>

      <!-- 标题 -->
      <h3 class="prompt-title">{{ title }}</h3>

      <!-- 消息 -->
      <p class="prompt-message">{{ message }}</p>

      <!-- 使用量信息（如果有） -->
      <div v-if="usageInfo" class="usage-info">
        <div class="usage-bar">
          <div class="usage-fill" :style="{ width: usagePercent + '%' }" />
        </div>
        <span class="usage-text">
          今日已使用 {{ usageInfo.current }}/{{ usageInfo.limit }} 次
        </span>
      </div>

      <!-- VIP 特权列表 -->
      <ul v-if="showBenefits" class="vip-benefits">
        <li v-for="benefit in vipBenefits" :key="benefit" class="benefit-item">
          <span class="benefit-check">✓</span>
          <span class="benefit-text">{{ benefit }}</span>
        </li>
      </ul>

      <!-- 操作按钮 -->
      <div class="prompt-actions">
        <button v-if="showLater" class="action-btn secondary" @click="handleLater">
          稍后再说
        </button>
        <button class="action-btn primary" @click="handleUpgrade">
          <span class="btn-icon">⭐</span>
          <span class="btn-text">{{ upgradeButtonText }}</span>
        </button>
      </div>

      <!-- 重置时间提示 -->
      <p v-if="resetTime" class="reset-hint">
        使用次数将于 {{ resetTime }} 重置
      </p>
    </div>
  </Transition>
</template>

<script setup lang="ts">
/**
 * 升级提示组件
 *
 * 在以下场景显示：
 * - 用户使用 VIP 专属功能
 * - 用户达到每日使用限制
 * - 免费试用结束
 */

import { computed } from 'vue'

// ============================================
// Props & Emits
// ============================================

interface UsageInfo {
  current: number
  limit: number
}

interface Props {
  /** 是否显示 */
  visible?: boolean
  /** 提示类型 */
  type?: 'feature' | 'limit' | 'trial'
  /** 功能名称 */
  featureName?: string
  /** 自定义标题 */
  customTitle?: string
  /** 自定义消息 */
  customMessage?: string
  /** 使用量信息 */
  usageInfo?: UsageInfo | null
  /** 重置时间 */
  resetTime?: string
  /** 是否可关闭 */
  dismissible?: boolean
  /** 是否显示稍后再说按钮 */
  showLater?: boolean
  /** 是否显示 VIP 特权 */
  showBenefits?: boolean
}

const props = withDefaults(defineProps<Props>(), {
  visible: false,
  type: 'feature',
  featureName: '',
  customTitle: '',
  customMessage: '',
  usageInfo: null,
  resetTime: '',
  dismissible: true,
  showLater: true,
  showBenefits: true,
})

const emit = defineEmits<{
  (e: 'dismiss'): void
  (e: 'upgrade'): void
  (e: 'later'): void
}>()

// ============================================
// Computed
// ============================================

/** 提示样式类 */
const promptClass = computed(() => ({
  [`type-${props.type}`]: true,
}))

/** 图标样式类 */
const iconClass = computed(() => ({
  'is-limit': props.type === 'limit',
  'is-feature': props.type === 'feature',
  'is-trial': props.type === 'trial',
}))

/** 图标 emoji */
const iconEmoji = computed(() => {
  switch (props.type) {
    case 'limit': return '⏱️'
    case 'trial': return '🎁'
    default: return '⭐'
  }
})

/** 标题 */
const title = computed(() => {
  if (props.customTitle) return props.customTitle
  switch (props.type) {
    case 'limit':
      return '今日使用次数已达上限'
    case 'trial':
      return '免费试用已结束'
    default:
      return 'VIP 专属功能'
  }
})

/** 消息 */
const message = computed(() => {
  if (props.customMessage) return props.customMessage
  switch (props.type) {
    case 'limit':
      return `${props.featureName || '该功能'}的免费使用次数已用完，升级 VIP 解锁无限使用。`
    case 'trial':
      return '您的免费试用期已结束，升级 VIP 继续享用全部功能。'
    default:
      return `${props.featureName || '该功能'}为 VIP 专属功能，升级后即可使用。`
  }
})

/** 使用量百分比 */
const usagePercent = computed(() => {
  if (!props.usageInfo || props.usageInfo.limit === 0) return 0
  return Math.min(100, (props.usageInfo.current / props.usageInfo.limit) * 100)
})

/** 升级按钮文字 */
const upgradeButtonText = computed(() => {
  switch (props.type) {
    case 'limit':
      return '升级 VIP 无限使用'
    case 'trial':
      return '立即升级'
    default:
      return '解锁此功能'
  }
})

/** VIP 特权列表 */
const vipBenefits = [
  '无限使用所有功能',
  '无每日次数限制',
  '优先技术支持',
]

// ============================================
// Methods
// ============================================

function handleDismiss(): void {
  emit('dismiss')
}

function handleUpgrade(): void {
  emit('upgrade')
}

function handleLater(): void {
  emit('later')
}
</script>

<style scoped>
.upgrade-prompt {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  padding: 24px;
  background: rgba(30, 30, 30, 0.98);
  border-radius: 16px;
  border: 1px solid rgba(255, 255, 255, 0.1);
  min-width: 320px;
  max-width: 380px;
  position: relative;
  text-align: center;
}

/* 类型变体 */
.upgrade-prompt.type-feature {
  border-color: rgba(255, 193, 7, 0.3);
}

.upgrade-prompt.type-limit {
  border-color: rgba(255, 152, 0, 0.3);
}

.upgrade-prompt.type-trial {
  border-color: rgba(156, 39, 176, 0.3);
}

/* 关闭按钮 */
.dismiss-btn {
  position: absolute;
  top: 12px;
  right: 12px;
  width: 24px;
  height: 24px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: none;
  border-radius: 4px;
  color: rgba(255, 255, 255, 0.4);
  font-size: 12px;
  cursor: pointer;
  transition: all 0.15s;
}

.dismiss-btn:hover {
  background: rgba(255, 255, 255, 0.1);
  color: rgba(255, 255, 255, 0.8);
}

/* 图标 */
.prompt-icon {
  width: 56px;
  height: 56px;
  display: flex;
  align-items: center;
  justify-content: center;
  font-size: 28px;
  background: linear-gradient(135deg, rgba(255, 193, 7, 0.2) 0%, rgba(255, 152, 0, 0.2) 100%);
  border-radius: 50%;
}

.prompt-icon.is-limit {
  background: linear-gradient(135deg, rgba(255, 152, 0, 0.2) 0%, rgba(244, 67, 54, 0.2) 100%);
}

.prompt-icon.is-trial {
  background: linear-gradient(135deg, rgba(156, 39, 176, 0.2) 0%, rgba(103, 58, 183, 0.2) 100%);
}

/* 标题 */
.prompt-title {
  margin: 0;
  color: #fff;
  font-size: 18px;
  font-weight: 600;
}

/* 消息 */
.prompt-message {
  margin: 0;
  color: rgba(255, 255, 255, 0.7);
  font-size: 14px;
  line-height: 1.5;
}

/* 使用量信息 */
.usage-info {
  width: 100%;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.usage-bar {
  width: 100%;
  height: 6px;
  background: rgba(255, 255, 255, 0.1);
  border-radius: 3px;
  overflow: hidden;
}

.usage-fill {
  height: 100%;
  background: linear-gradient(90deg, #ff9800 0%, #f44336 100%);
  border-radius: 3px;
  transition: width 0.3s ease;
}

.usage-text {
  color: rgba(255, 255, 255, 0.5);
  font-size: 12px;
}

/* VIP 特权 */
.vip-benefits {
  list-style: none;
  padding: 12px 16px;
  margin: 0;
  background: rgba(255, 193, 7, 0.08);
  border-radius: 8px;
  width: 100%;
}

.benefit-item {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 4px 0;
}

.benefit-check {
  color: #4caf50;
  font-size: 12px;
}

.benefit-text {
  color: rgba(255, 255, 255, 0.8);
  font-size: 13px;
  text-align: left;
}

/* 操作按钮 */
.prompt-actions {
  display: flex;
  gap: 12px;
  width: 100%;
  margin-top: 4px;
}

.action-btn {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 12px 16px;
  border: none;
  border-radius: 8px;
  font-size: 14px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.2s;
}

.action-btn.secondary {
  background: rgba(255, 255, 255, 0.1);
  color: rgba(255, 255, 255, 0.8);
}

.action-btn.secondary:hover {
  background: rgba(255, 255, 255, 0.15);
}

.action-btn.primary {
  background: linear-gradient(135deg, #ffc107 0%, #ff9800 100%);
  color: #000;
}

.action-btn.primary:hover {
  transform: translateY(-1px);
  box-shadow: 0 4px 12px rgba(255, 193, 7, 0.3);
}

.btn-icon {
  font-size: 16px;
}

/* 重置时间提示 */
.reset-hint {
  margin: 0;
  color: rgba(255, 255, 255, 0.4);
  font-size: 11px;
}

/* 弹出动画 */
.popup-enter-active,
.popup-leave-active {
  transition: all 0.25s ease;
}

.popup-enter-from,
.popup-leave-to {
  opacity: 0;
  transform: scale(0.95) translateY(10px);
}
</style>
