<template>
  <div class="device-manager">
    <!-- 标题 -->
    <div class="manager-header">
      <h3 class="manager-title">设备管理</h3>
      <span class="device-count">{{ devices.length }}/{{ maxDevices }} 台设备</span>
    </div>

    <!-- 当前设备信息 -->
    <div v-if="currentDevice" class="current-device">
      <div class="device-badge">当前设备</div>
      <div class="device-info">
        <span class="device-name">{{ currentDevice.device_name }}</span>
        <span class="device-os">{{ currentDevice.os_version }}</span>
      </div>
      <div class="device-id">
        设备 ID: {{ formatDeviceId(currentDevice.device_id) }}
      </div>
    </div>

    <!-- 加载状态 -->
    <div v-if="isLoading" class="loading-state">
      <div class="loading-spinner" />
      <span class="loading-text">加载设备列表...</span>
    </div>

    <!-- 设备列表 -->
    <div v-else-if="devices.length > 0" class="device-list">
      <div
        v-for="device in devices"
        :key="device.device_id"
        class="device-item"
        :class="{ 'is-current': device.is_current }"
      >
        <div class="device-main">
          <div class="device-icon">
            {{ device.is_current ? '💻' : '🖥️' }}
          </div>
          <div class="device-details">
            <div class="device-name-row">
              <span class="device-name">{{ device.device_name }}</span>
              <span v-if="device.is_current" class="current-tag">当前</span>
            </div>
            <span class="device-os">{{ device.os_version }}</span>
            <span class="device-time">
              最后活跃: {{ formatTime(device.last_active_at) }}
            </span>
          </div>
        </div>
        <button
          v-if="!device.is_current"
          class="unbind-btn"
          :disabled="isUnbinding === device.device_id"
          @click="handleUnbind(device)"
        >
          <span v-if="isUnbinding === device.device_id" class="loading-spinner-small" />
          <span v-else>解绑</span>
        </button>
      </div>
    </div>

    <!-- 空状态 -->
    <div v-else class="empty-state">
      <span class="empty-icon">📱</span>
      <span class="empty-text">暂无绑定设备</span>
    </div>

    <!-- 错误提示 -->
    <div v-if="error" class="error-message">
      <span class="error-icon">⚠️</span>
      <span class="error-text">{{ error }}</span>
    </div>

    <!-- 说明文字 -->
    <div class="manager-footer">
      <p class="hint-text">
        每个账户最多可绑定 {{ maxDevices }} 台设备。如需在新设备上使用，请先解绑其他设备。
      </p>
      <button class="refresh-btn" @click="loadDevices" :disabled="isLoading">
        <span class="refresh-icon">🔄</span>
        刷新列表
      </button>
    </div>

    <!-- 解绑确认对话框 -->
    <Transition name="fade">
      <div v-if="showConfirm" class="confirm-overlay" @click.self="cancelUnbind">
        <div class="confirm-dialog">
          <h4 class="confirm-title">确认解绑设备</h4>
          <p class="confirm-message">
            确定要解绑设备 "{{ deviceToUnbind?.device_name }}" 吗？
            解绑后该设备将无法使用 VIP 功能。
          </p>
          <div class="confirm-actions">
            <button class="confirm-btn secondary" @click="cancelUnbind">取消</button>
            <button class="confirm-btn primary" @click="confirmUnbind">确认解绑</button>
          </div>
        </div>
      </div>
    </Transition>
  </div>
</template>

<script setup lang="ts">
/**
 * 设备管理组件
 *
 * 功能：
 * - 显示已绑定设备列表
 * - 显示当前设备信息
 * - 支持解绑其他设备
 */

import { ref, computed, onMounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type { DeviceInfo, DeviceListResponse, UnbindDeviceResponse } from '@/types/auth'

// ============================================
// Props & Emits
// ============================================

const emit = defineEmits<{
  (e: 'device-unbound', deviceId: string): void
}>()

// ============================================
// State
// ============================================

/** 设备列表 */
const devices = ref<DeviceInfo[]>([])

/** 最大设备数 */
const maxDevices = ref(3)

/** 是否正在加载 */
const isLoading = ref(false)

/** 正在解绑的设备 ID */
const isUnbinding = ref<string | null>(null)

/** 错误信息 */
const error = ref<string | null>(null)

/** 是否显示确认对话框 */
const showConfirm = ref(false)

/** 待解绑的设备 */
const deviceToUnbind = ref<DeviceInfo | null>(null)

// ============================================
// Computed
// ============================================

/** 当前设备 */
const currentDevice = computed(() => devices.value.find(d => d.is_current))

// ============================================
// Methods
// ============================================

/**
 * 加载设备列表
 */
async function loadDevices(): Promise<void> {
  isLoading.value = true
  error.value = null

  try {
    const response = await invoke<DeviceListResponse>('get_bound_devices')

    if (response.success && response.devices) {
      devices.value = response.devices
      if (response.max_devices) {
        maxDevices.value = response.max_devices
      }
    } else {
      error.value = response.error || '获取设备列表失败'
    }
  } catch (e) {
    error.value = e instanceof Error ? e.message : '请求失败'
  } finally {
    isLoading.value = false
  }
}

/**
 * 处理解绑按钮点击
 */
function handleUnbind(device: DeviceInfo): void {
  deviceToUnbind.value = device
  showConfirm.value = true
}

/**
 * 取消解绑
 */
function cancelUnbind(): void {
  showConfirm.value = false
  deviceToUnbind.value = null
}

/**
 * 确认解绑
 */
async function confirmUnbind(): Promise<void> {
  if (!deviceToUnbind.value) return

  const device = deviceToUnbind.value
  showConfirm.value = false
  isUnbinding.value = device.device_id
  error.value = null

  try {
    const response = await invoke<UnbindDeviceResponse>('unbind_device', {
      deviceId: device.device_id,
    })

    if (response.success) {
      // 从列表中移除
      devices.value = devices.value.filter(d => d.device_id !== device.device_id)
      emit('device-unbound', device.device_id)
    } else {
      error.value = response.error || '解绑失败'
    }
  } catch (e) {
    error.value = e instanceof Error ? e.message : '解绑请求失败'
  } finally {
    isUnbinding.value = null
    deviceToUnbind.value = null
  }
}

/**
 * 格式化设备 ID（只显示前后几位）
 */
function formatDeviceId(id: string): string {
  if (id.length <= 12) return id
  return `${id.slice(0, 6)}...${id.slice(-6)}`
}

/**
 * 格式化时间
 */
function formatTime(isoString: string): string {
  try {
    const date = new Date(isoString)
    const now = new Date()
    const diff = now.getTime() - date.getTime()

    // 小于 1 分钟
    if (diff < 60 * 1000) {
      return '刚刚'
    }
    // 小于 1 小时
    if (diff < 60 * 60 * 1000) {
      return `${Math.floor(diff / 60 / 1000)} 分钟前`
    }
    // 小于 24 小时
    if (diff < 24 * 60 * 60 * 1000) {
      return `${Math.floor(diff / 60 / 60 / 1000)} 小时前`
    }
    // 小于 7 天
    if (diff < 7 * 24 * 60 * 60 * 1000) {
      return `${Math.floor(diff / 24 / 60 / 60 / 1000)} 天前`
    }
    // 其他：显示日期
    return date.toLocaleDateString('zh-CN')
  } catch {
    return isoString
  }
}

// ============================================
// Lifecycle
// ============================================

onMounted(() => {
  loadDevices()
})
</script>

<style scoped>
.device-manager {
  display: flex;
  flex-direction: column;
  gap: 16px;
  padding: 20px;
  background: rgba(30, 30, 30, 0.98);
  border-radius: 12px;
  border: 1px solid rgba(255, 255, 255, 0.1);
  min-width: 360px;
  max-width: 420px;
}

/* 标题 */
.manager-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding-bottom: 12px;
  border-bottom: 1px solid rgba(255, 255, 255, 0.1);
}

.manager-title {
  margin: 0;
  color: #fff;
  font-size: 18px;
  font-weight: 600;
}

.device-count {
  color: rgba(255, 255, 255, 0.5);
  font-size: 13px;
}

/* 当前设备 */
.current-device {
  padding: 12px;
  background: linear-gradient(135deg, rgba(66, 133, 244, 0.15) 0%, rgba(76, 175, 80, 0.15) 100%);
  border: 1px solid rgba(66, 133, 244, 0.3);
  border-radius: 8px;
  position: relative;
}

.device-badge {
  position: absolute;
  top: -8px;
  right: 12px;
  padding: 2px 8px;
  background: #4285f4;
  border-radius: 4px;
  color: #fff;
  font-size: 10px;
  font-weight: 500;
}

.current-device .device-info {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.current-device .device-name {
  color: #fff;
  font-size: 15px;
  font-weight: 500;
}

.current-device .device-os {
  color: rgba(255, 255, 255, 0.6);
  font-size: 12px;
}

.current-device .device-id {
  margin-top: 8px;
  padding-top: 8px;
  border-top: 1px solid rgba(255, 255, 255, 0.1);
  color: rgba(255, 255, 255, 0.4);
  font-size: 11px;
  font-family: monospace;
}

/* 加载状态 */
.loading-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  padding: 24px;
}

.loading-spinner {
  width: 24px;
  height: 24px;
  border: 3px solid rgba(255, 255, 255, 0.2);
  border-top-color: #4285f4;
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

.loading-spinner-small {
  width: 14px;
  height: 14px;
  border: 2px solid rgba(255, 255, 255, 0.2);
  border-top-color: #fff;
  border-radius: 50%;
  animation: spin 0.6s linear infinite;
}

@keyframes spin {
  to { transform: rotate(360deg); }
}

.loading-text {
  color: rgba(255, 255, 255, 0.6);
  font-size: 13px;
}

/* 设备列表 */
.device-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.device-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  padding: 12px;
  background: rgba(255, 255, 255, 0.05);
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: 8px;
  transition: all 0.15s;
}

.device-item:hover {
  background: rgba(255, 255, 255, 0.08);
}

.device-item.is-current {
  background: rgba(66, 133, 244, 0.1);
  border-color: rgba(66, 133, 244, 0.3);
}

.device-main {
  display: flex;
  align-items: center;
  gap: 12px;
}

.device-icon {
  font-size: 24px;
}

.device-details {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.device-name-row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.device-name {
  color: #fff;
  font-size: 14px;
  font-weight: 500;
}

.current-tag {
  padding: 1px 6px;
  background: rgba(66, 133, 244, 0.2);
  border-radius: 3px;
  color: #4285f4;
  font-size: 10px;
}

.device-os {
  color: rgba(255, 255, 255, 0.5);
  font-size: 12px;
}

.device-time {
  color: rgba(255, 255, 255, 0.4);
  font-size: 11px;
}

/* 解绑按钮 */
.unbind-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  min-width: 48px;
  padding: 6px 12px;
  background: rgba(244, 67, 54, 0.15);
  border: 1px solid rgba(244, 67, 54, 0.3);
  border-radius: 4px;
  color: #f44336;
  font-size: 12px;
  cursor: pointer;
  transition: all 0.15s;
}

.unbind-btn:hover:not(:disabled) {
  background: rgba(244, 67, 54, 0.25);
  border-color: rgba(244, 67, 54, 0.5);
}

.unbind-btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

/* 空状态 */
.empty-state {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 24px;
}

.empty-icon {
  font-size: 32px;
  opacity: 0.5;
}

.empty-text {
  color: rgba(255, 255, 255, 0.5);
  font-size: 14px;
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

/* 底部说明 */
.manager-footer {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding-top: 12px;
  border-top: 1px solid rgba(255, 255, 255, 0.1);
}

.hint-text {
  margin: 0;
  color: rgba(255, 255, 255, 0.4);
  font-size: 12px;
  line-height: 1.5;
}

.refresh-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 8px 16px;
  background: rgba(255, 255, 255, 0.08);
  border: 1px solid rgba(255, 255, 255, 0.15);
  border-radius: 6px;
  color: rgba(255, 255, 255, 0.8);
  font-size: 13px;
  cursor: pointer;
  transition: all 0.15s;
}

.refresh-btn:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.12);
}

.refresh-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.refresh-icon {
  font-size: 14px;
}

/* 确认对话框 */
.confirm-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: rgba(0, 0, 0, 0.6);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
}

.confirm-dialog {
  padding: 20px;
  background: rgba(40, 40, 40, 0.98);
  border-radius: 12px;
  border: 1px solid rgba(255, 255, 255, 0.15);
  min-width: 300px;
  max-width: 360px;
}

.confirm-title {
  margin: 0 0 12px 0;
  color: #fff;
  font-size: 16px;
  font-weight: 600;
}

.confirm-message {
  margin: 0 0 20px 0;
  color: rgba(255, 255, 255, 0.7);
  font-size: 14px;
  line-height: 1.5;
}

.confirm-actions {
  display: flex;
  gap: 12px;
  justify-content: flex-end;
}

.confirm-btn {
  padding: 8px 16px;
  border: none;
  border-radius: 6px;
  font-size: 14px;
  cursor: pointer;
  transition: all 0.15s;
}

.confirm-btn.secondary {
  background: rgba(255, 255, 255, 0.1);
  color: rgba(255, 255, 255, 0.8);
}

.confirm-btn.secondary:hover {
  background: rgba(255, 255, 255, 0.15);
}

.confirm-btn.primary {
  background: #f44336;
  color: #fff;
}

.confirm-btn.primary:hover {
  background: #e53935;
}

/* 动画 */
.fade-enter-active,
.fade-leave-active {
  transition: opacity 0.2s ease;
}

.fade-enter-from,
.fade-leave-to {
  opacity: 0;
}
</style>
