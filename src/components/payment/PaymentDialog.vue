<template>
  <div class="payment-dialog">
    <!-- 对话框头部 -->
    <div class="dialog-header">
      <span class="dialog-title">升级 VIP</span>
      <button class="close-btn" @click="handleClose" :disabled="isProcessing">
        ✕
      </button>
    </div>

    <!-- 计划选择 -->
    <div v-if="!currentOrder" class="plan-selection">
      <div class="plan-card" :class="{ 'is-selected': true }">
        <div class="plan-header">
          <span class="plan-name">终身 VIP</span>
          <span class="plan-tag">一次购买 永久使用</span>
        </div>
        <div class="plan-price">
          <span class="price-current">¥{{ plan.price }}</span>
          <span v-if="plan.originalPrice" class="price-original">¥{{ plan.originalPrice }}</span>
        </div>
        <ul class="plan-features">
          <li v-for="feature in plan.features" :key="feature" class="feature-item">
            <span class="feature-check">✓</span>
            <span class="feature-text">{{ feature }}</span>
          </li>
        </ul>
      </div>

      <!-- 支付方式选择 -->
      <div class="payment-methods">
        <span class="methods-label">支付方式</span>
        <div class="methods-list">
          <button
            v-for="method in paymentMethods"
            :key="method.id"
            class="method-btn"
            :class="{ 'is-selected': selectedMethod === method.id }"
            @click="selectedMethod = method.id"
            :disabled="isProcessing"
          >
            <span class="method-icon">{{ method.icon }}</span>
            <span class="method-name">{{ method.name }}</span>
          </button>
        </div>
      </div>

      <!-- 支付按钮 -->
      <button
        class="pay-btn"
        :class="{ 'is-loading': isCreatingOrder }"
        :disabled="!selectedMethod || isCreatingOrder"
        @click="handleCreateOrder"
      >
        <span v-if="isCreatingOrder" class="loading-spinner-small" />
        <span class="btn-text">{{ isCreatingOrder ? '创建订单中...' : `立即支付 ¥${plan.price}` }}</span>
      </button>
    </div>

    <!-- 支付二维码 -->
    <div v-else class="payment-qrcode">
      <div class="qrcode-header">
        <span class="qrcode-title">请使用{{ selectedMethodName }}扫码支付</span>
        <span class="qrcode-amount">¥{{ currentOrder.amount }}</span>
      </div>

      <!-- 二维码区域 -->
      <div class="qrcode-container">
        <div v-if="currentOrder.qr_code_url" class="qrcode-image">
          <img :src="currentOrder.qr_code_url" alt="支付二维码" />
        </div>
        <div v-else-if="currentOrder.pay_url" class="qrcode-link">
          <a :href="currentOrder.pay_url" target="_blank" class="pay-link-btn">
            点击前往支付
          </a>
        </div>
        <div v-else class="qrcode-loading">
          <div class="loading-spinner" />
          <span>正在生成支付码...</span>
        </div>
      </div>

      <!-- 状态提示 -->
      <div class="payment-status">
        <div v-if="isPolling" class="status-polling">
          <div class="status-indicator polling" />
          <span class="status-text">等待支付中...</span>
        </div>
        <div v-else-if="paymentSuccess" class="status-success">
          <span class="status-icon">✓</span>
          <span class="status-text">支付成功！</span>
        </div>
      </div>

      <!-- 订单信息 -->
      <div class="order-info">
        <span class="order-label">订单号：</span>
        <span class="order-value">{{ currentOrder.order_no }}</span>
      </div>

      <!-- 操作按钮 -->
      <div class="qrcode-actions">
        <button class="action-btn secondary" @click="handleCancelOrder" :disabled="isPolling && paymentSuccess">
          取消订单
        </button>
        <button class="action-btn primary" @click="handleCheckPayment" :disabled="isPolling">
          我已支付
        </button>
      </div>
    </div>

    <!-- 错误提示 -->
    <div v-if="error" class="error-message">
      <span class="error-icon">⚠️</span>
      <span class="error-text">{{ error }}</span>
    </div>

    <!-- 成功提示 -->
    <Transition name="toast">
      <div v-if="showSuccess" class="success-toast">
        🎉 恭喜您成为 VIP 用户！
      </div>
    </Transition>
  </div>
</template>

<script setup lang="ts">
/**
 * 支付对话框组件
 *
 * 功能：
 * - 显示订阅计划
 * - 选择支付方式
 * - 显示支付二维码
 * - 轮询支付状态
 */

import { ref, computed, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { useAuthStore } from '@/stores/auth'
import type { PaymentMethod, PaymentOrder, CreateOrderResponse, QueryOrderResponse } from '@/types/payment'

// ============================================
// Props & Emits
// ============================================

interface Props {
  /** 是否显示 */
  visible?: boolean
}

withDefaults(defineProps<Props>(), {
  visible: true,
})

const emit = defineEmits<{
  (e: 'close'): void
  (e: 'success'): void
}>()

// ============================================
// Store
// ============================================

const authStore = useAuthStore()

// ============================================
// State
// ============================================

/** 订阅计划 */
const plan = ref({
  id: 'lifetime_vip',
  name: '终身 VIP',
  price: 99,
  originalPrice: 199,
  description: '一次购买，永久使用',
  features: [
    '无限制使用所有功能',
    '无每日使用次数限制',
    '录屏功能',
    'Anki 导出功能',
    '批量 OCR 处理',
    '优先技术支持',
    '后续新功能免费',
  ],
})

/** 支付方式列表 */
const paymentMethods = ref<{ id: PaymentMethod; name: string; icon: string }[]>([
  { id: 'alipay', name: '支付宝', icon: '💳' },
  { id: 'wechat', name: '微信支付', icon: '💬' },
])

/** 选择的支付方式 */
const selectedMethod = ref<PaymentMethod>('alipay')

/** 当前订单 */
const currentOrder = ref<PaymentOrder | null>(null)

/** 是否正在创建订单 */
const isCreatingOrder = ref(false)

/** 是否正在轮询支付状态 */
const isPolling = ref(false)

/** 支付成功 */
const paymentSuccess = ref(false)

/** 错误信息 */
const error = ref<string | null>(null)

/** 是否显示成功提示 */
const showSuccess = ref(false)

/** 轮询定时器 */
let pollingTimer: ReturnType<typeof setInterval> | null = null

// ============================================
// Computed
// ============================================

/** 是否正在处理 */
const isProcessing = computed(() => isCreatingOrder.value || isPolling.value)

/** 选择的支付方式名称 */
const selectedMethodName = computed(() => {
  const method = paymentMethods.value.find(m => m.id === selectedMethod.value)
  return method?.name || ''
})

// ============================================
// Methods
// ============================================

/**
 * 创建订单
 */
async function handleCreateOrder(): Promise<void> {
  if (!selectedMethod.value) return

  isCreatingOrder.value = true
  error.value = null

  try {
    const response = await invoke<CreateOrderResponse>('create_payment_order', {
      request: {
        plan_id: plan.value.id,
        amount: plan.value.price,
        payment_method: selectedMethod.value,
      }
    })

    if (response.success && response.order) {
      currentOrder.value = response.order
      // 开始轮询支付状态
      startPolling()
    } else {
      error.value = response.error || '创建订单失败'
    }
  } catch (e) {
    error.value = e instanceof Error ? e.message : '创建订单请求失败'
  } finally {
    isCreatingOrder.value = false
  }
}

/**
 * 开始轮询支付状态
 */
function startPolling(): void {
  if (pollingTimer) {
    clearInterval(pollingTimer)
  }

  isPolling.value = true

  // 每 3 秒检查一次支付状态
  pollingTimer = setInterval(async () => {
    if (!currentOrder.value) {
      stopPolling()
      return
    }

    try {
      const response = await invoke<QueryOrderResponse>('query_payment_order', {
        orderNo: currentOrder.value.order_no,
      })

      if (response.success && response.is_paid) {
        handlePaymentSuccess()
      }
    } catch (e) {
      console.warn('查询订单状态失败:', e)
    }
  }, 3000)

  // 5 分钟后停止轮询
  setTimeout(() => {
    if (isPolling.value && !paymentSuccess.value) {
      stopPolling()
      error.value = '订单已超时，请重新支付'
    }
  }, 5 * 60 * 1000)
}

/**
 * 停止轮询
 */
function stopPolling(): void {
  if (pollingTimer) {
    clearInterval(pollingTimer)
    pollingTimer = null
  }
  isPolling.value = false
}

/**
 * 手动检查支付状态
 */
async function handleCheckPayment(): Promise<void> {
  if (!currentOrder.value) return

  error.value = null

  try {
    const response = await invoke<QueryOrderResponse>('check_payment_status', {
      orderNo: currentOrder.value.order_no,
    })

    if (response.success && response.is_paid) {
      handlePaymentSuccess()
    } else {
      error.value = '尚未检测到支付，请稍后再试'
    }
  } catch (e) {
    error.value = e instanceof Error ? e.message : '查询支付状态失败'
  }
}

/**
 * 处理支付成功
 */
async function handlePaymentSuccess(): Promise<void> {
  stopPolling()
  paymentSuccess.value = true
  showSuccess.value = true

  // 刷新许可证状态
  await authStore.loadLicense()

  // 延迟关闭
  setTimeout(() => {
    showSuccess.value = false
    emit('success')
    handleClose()
  }, 2000)
}

/**
 * 取消订单
 */
function handleCancelOrder(): void {
  stopPolling()
  currentOrder.value = null
  paymentSuccess.value = false
  error.value = null
}

/**
 * 关闭对话框
 */
function handleClose(): void {
  stopPolling()
  emit('close')
}

// ============================================
// Lifecycle
// ============================================

onUnmounted(() => {
  stopPolling()
})
</script>

<style scoped>
.payment-dialog {
  display: flex;
  flex-direction: column;
  gap: 16px;
  padding: 20px;
  background: rgba(30, 30, 30, 0.98);
  border-radius: 12px;
  border: 1px solid rgba(255, 255, 255, 0.1);
  min-width: 380px;
  max-width: 440px;
  position: relative;
}

/* 对话框头部 */
.dialog-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding-bottom: 12px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.1);
}

.dialog-title {
  color: #fff;
  font-size: 18px;
  font-weight: 600;
}

.close-btn {
  width: 28px;
  height: 28px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: transparent;
  border: none;
  border-radius: 6px;
  color: rgba(255, 255, 255, 0.6);
  font-size: 14px;
  cursor: pointer;
  transition: all 0.15s;
}

.close-btn:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.1);
  color: #fff;
}

.close-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* 计划选择 */
.plan-selection {
  display: flex;
  flex-direction: column;
  gap: 16px;
}

.plan-card {
  padding: 16px;
  background: linear-gradient(135deg, rgba(255, 193, 7, 0.1) 0%, rgba(255, 152, 0, 0.1) 100%);
  border: 2px solid rgba(255, 193, 7, 0.4);
  border-radius: 12px;
}

.plan-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 12px;
}

.plan-name {
  color: #ffc107;
  font-size: 18px;
  font-weight: 600;
}

.plan-tag {
  padding: 4px 8px;
  background: rgba(255, 193, 7, 0.2);
  border-radius: 4px;
  color: #ffc107;
  font-size: 11px;
}

.plan-price {
  display: flex;
  align-items: baseline;
  gap: 8px;
  margin-bottom: 16px;
}

.price-current {
  color: #fff;
  font-size: 32px;
  font-weight: 700;
}

.price-original {
  color: rgba(255, 255, 255, 0.4);
  font-size: 16px;
  text-decoration: line-through;
}

.plan-features {
  list-style: none;
  padding: 0;
  margin: 0;
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.feature-item {
  display: flex;
  align-items: center;
  gap: 8px;
}

.feature-check {
  color: #4caf50;
  font-size: 14px;
}

.feature-text {
  color: rgba(255, 255, 255, 0.8);
  font-size: 13px;
}

/* 支付方式 */
.payment-methods {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.methods-label {
  color: rgba(255, 255, 255, 0.6);
  font-size: 13px;
}

.methods-list {
  display: flex;
  gap: 8px;
}

.method-btn {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 12px;
  background: rgba(255, 255, 255, 0.05);
  border: 1px solid rgba(255, 255, 255, 0.15);
  border-radius: 8px;
  color: rgba(255, 255, 255, 0.8);
  font-size: 14px;
  cursor: pointer;
  transition: all 0.15s;
}

.method-btn:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.1);
  border-color: rgba(255, 255, 255, 0.25);
}

.method-btn.is-selected {
  background: rgba(66, 133, 244, 0.15);
  border-color: rgba(66, 133, 244, 0.6);
  color: #fff;
}

.method-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.method-icon {
  font-size: 18px;
}

/* 支付按钮 */
.pay-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 14px;
  background: linear-gradient(135deg, #ffc107 0%, #ff9800 100%);
  border: none;
  border-radius: 8px;
  color: #000;
  font-size: 16px;
  font-weight: 600;
  cursor: pointer;
  transition: all 0.2s;
}

.pay-btn:hover:not(:disabled) {
  transform: translateY(-1px);
  box-shadow: 0 4px 12px rgba(255, 193, 7, 0.3);
}

.pay-btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
  transform: none;
  box-shadow: none;
}

.pay-btn.is-loading {
  background: rgba(255, 193, 7, 0.6);
}

/* 支付二维码 */
.payment-qrcode {
  display: flex;
  flex-direction: column;
  gap: 16px;
  align-items: center;
}

.qrcode-header {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
}

.qrcode-title {
  color: rgba(255, 255, 255, 0.8);
  font-size: 14px;
}

.qrcode-amount {
  color: #ffc107;
  font-size: 24px;
  font-weight: 600;
}

.qrcode-container {
  width: 200px;
  height: 200px;
  display: flex;
  align-items: center;
  justify-content: center;
  background: #fff;
  border-radius: 8px;
  overflow: hidden;
}

.qrcode-image img {
  width: 100%;
  height: 100%;
  object-fit: contain;
}

.qrcode-link {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 100%;
  height: 100%;
}

.pay-link-btn {
  padding: 12px 24px;
  background: #4285f4;
  color: #fff;
  text-decoration: none;
  border-radius: 6px;
  font-size: 14px;
}

.qrcode-loading {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  color: #666;
  font-size: 12px;
}

/* 支付状态 */
.payment-status {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
}

.status-polling {
  display: flex;
  align-items: center;
  gap: 8px;
}

.status-indicator {
  width: 8px;
  height: 8px;
  border-radius: 50%;
}

.status-indicator.polling {
  background: #ffc107;
  animation: pulse 1.5s ease-in-out infinite;
}

@keyframes pulse {
  0%, 100% { opacity: 1; }
  50% { opacity: 0.4; }
}

.status-success {
  display: flex;
  align-items: center;
  gap: 8px;
}

.status-icon {
  color: #4caf50;
  font-size: 18px;
}

.status-text {
  color: rgba(255, 255, 255, 0.8);
  font-size: 13px;
}

.status-success .status-text {
  color: #4caf50;
}

/* 订单信息 */
.order-info {
  display: flex;
  align-items: center;
  gap: 4px;
  font-size: 12px;
}

.order-label {
  color: rgba(255, 255, 255, 0.5);
}

.order-value {
  color: rgba(255, 255, 255, 0.7);
  font-family: monospace;
}

/* 操作按钮 */
.qrcode-actions {
  display: flex;
  gap: 12px;
  width: 100%;
}

.action-btn {
  flex: 1;
  padding: 10px 16px;
  border: none;
  border-radius: 6px;
  font-size: 14px;
  cursor: pointer;
  transition: all 0.15s;
}

.action-btn.secondary {
  background: rgba(255, 255, 255, 0.1);
  color: rgba(255, 255, 255, 0.8);
}

.action-btn.secondary:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.15);
}

.action-btn.primary {
  background: rgba(66, 133, 244, 0.8);
  color: #fff;
}

.action-btn.primary:hover:not(:disabled) {
  background: rgba(66, 133, 244, 1);
}

.action-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* 加载动画 */
.loading-spinner {
  width: 24px;
  height: 24px;
  border: 3px solid rgba(0, 0, 0, 0.2);
  border-top-color: #4285f4;
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

.loading-spinner-small {
  width: 16px;
  height: 16px;
  border: 2px solid rgba(0, 0, 0, 0.2);
  border-top-color: #000;
  border-radius: 50%;
  animation: spin 0.6s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

/* 错误提示 */
.error-message {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 10px 12px;
  background: rgba(244, 67, 54, 0.15);
  border: 1px solid rgba(244, 67, 54, 0.3);
  border-radius: 6px;
}

.error-icon {
  font-size: 14px;
  flex-shrink: 0;
}

.error-text {
  color: #ff6b6b;
  font-size: 13px;
}

/* 成功提示 */
.success-toast {
  position: absolute;
  top: 50%;
  left: 50%;
  transform: translate(-50%, -50%);
  padding: 16px 24px;
  background: rgba(76, 175, 80, 0.95);
  border-radius: 8px;
  color: #fff;
  font-size: 16px;
  font-weight: 500;
  box-shadow: 0 4px 20px rgba(0, 0, 0, 0.4);
  z-index: 100;
}

/* Toast 动画 */
.toast-enter-active,
.toast-leave-active {
  transition: all 0.3s ease;
}

.toast-enter-from,
.toast-leave-to {
  opacity: 0;
  transform: translate(-50%, -50%) scale(0.9);
}
</style>
