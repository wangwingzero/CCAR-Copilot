<template>
  <div class="account-section">
    <!-- Auth Error Notification (Requirement 8.1, 8.2, 8.3) -->
    <Transition name="error-slide">
      <div 
        v-if="authError" 
        class="auth-error-notification"
        role="alert"
        aria-live="assertive"
      >
        <div class="error-content">
          <svg class="error-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
            <circle cx="12" cy="12" r="10"/>
            <line x1="12" y1="8" x2="12" y2="12"/>
            <line x1="12" y1="16" x2="12.01" y2="16"/>
          </svg>
          <span class="error-message">{{ authError }}</span>
        </div>
        <div class="error-actions">
          <!-- Retry button (Requirement 8.3) -->
          <button 
            class="error-retry-btn" 
            @click="handleRetryAuth"
            :title="$t('settings.account.retry')"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <path d="M1 4v6h6"/>
              <path d="M3.51 15a9 9 0 1 0 2.13-9.36L1 10"/>
            </svg>
            {{ $t('settings.account.retry') }}
          </button>
          <!-- Dismiss button (Requirement 8.3) -->
          <button 
            class="error-dismiss-btn" 
            @click="handleDismissError"
            :aria-label="$t('common.close')"
          >
            <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
              <line x1="18" y1="6" x2="6" y2="18"/>
              <line x1="6" y1="6" x2="18" y2="18"/>
            </svg>
          </button>
        </div>
      </div>
    </Transition>

    <!-- Logged Out State -->
    <SettingsGroup v-if="!isLoggedIn" :title="$t('settings.account.title')">
      <div class="auth-buttons">
        <button 
          class="auth-btn login-btn" 
          :disabled="authLoading"
          @click="handleLogin"
        >
          <span v-if="authLoading && authDialogMode === 'login'" class="btn-spinner"></span>
          <span v-else>{{ $t('settings.account.login') }}</span>
        </button>
        <button 
          class="auth-btn register-btn" 
          :disabled="authLoading"
          @click="handleRegister"
        >
          <span v-if="authLoading && authDialogMode === 'register'" class="btn-spinner"></span>
          <span v-else>{{ $t('settings.account.register') }}</span>
        </button>
      </div>
      <p class="auth-hint">{{ $t('settings.account.loginHint') }}</p>
    </SettingsGroup>

    <!-- Logged In State -->
    <template v-else>
      <!-- User Info Group -->
      <SettingsGroup :title="$t('settings.account.userInfo')">
        <SettingItem :label="$t('settings.account.email')">
          <span class="user-email">{{ userInfo.email }}</span>
        </SettingItem>

        <SettingItem :label="$t('settings.account.vipStatus')">
          <div class="vip-status-container">
            <span :class="['vip-badge', { 'is-vip': userInfo.isVip }]">
              {{ userInfo.isVip ? $t('settings.account.vipActive') : $t('settings.account.vipInactive') }}
            </span>
            <span v-if="userInfo.isVip && userInfo.vipExpiry" class="vip-expiry">
              {{ $t('settings.account.vipExpiry', { date: formatDate(userInfo.vipExpiry) }) }}
            </span>
            <!-- Upgrade prompt for non-VIP users (Requirement 4.4) -->
            <button v-if="!userInfo.isVip" class="upgrade-btn" @click="handleUpgradeVip">
              {{ $t('settings.account.upgradeVip') }}
            </button>
          </div>
        </SettingItem>
      </SettingsGroup>

      <!-- Device Management Group -->
      <SettingsGroup :title="$t('settings.account.devices')">
        <!-- Loading state for device list - Skeleton screen (Requirement 8.1) -->
        <div v-if="isLoadingDevices" class="device-skeleton">
          <div v-for="i in 2" :key="i" class="device-skeleton-item">
            <div class="skeleton-content">
              <div class="skeleton-line skeleton-name"></div>
              <div class="skeleton-line skeleton-os"></div>
              <div class="skeleton-line skeleton-time"></div>
            </div>
            <div class="skeleton-btn"></div>
          </div>
          <p class="loading-text">{{ $t('settings.account.loadingDevices') }}</p>
        </div>
        
        <!-- Error state for device list -->
        <div v-else-if="deviceError" class="device-error">
          <span>{{ deviceError }}</span>
          <button class="retry-btn" @click="loadDevices">
            {{ $t('settings.account.retry') }}
          </button>
        </div>
        
        <!-- Device list (Requirement 5.1, 5.3) -->
        <div v-else class="device-list">
          <div
            v-for="device in devices"
            :key="device.id"
            class="device-item"
          >
            <div class="device-info">
              <div class="device-header">
                <span class="device-name">{{ device.name }}</span>
                <!-- Current device badge (Requirement 5.4) -->
                <span v-if="device.isCurrent" class="current-device-badge">
                  {{ $t('settings.account.currentDevice') }}
                </span>
              </div>
              <!-- OS version display (Requirement 5.3) -->
              <span class="device-os">{{ device.osVersion }}</span>
              <!-- Last active time (Requirement 5.3) -->
              <span class="device-last-active">
                {{ $t('settings.account.lastActive', { date: formatDate(device.lastActive) }) }}
              </span>
            </div>
            <!-- Unbind button for non-current devices (Requirement 5.5) -->
            <button
              v-if="!device.isCurrent"
              class="unbind-btn"
              @click="handleUnbindDevice(device.id)"
            >
              {{ $t('settings.account.unbind') }}
            </button>
          </div>
        </div>
        <!-- Device count display (Requirement 5.7) -->
        <p class="device-limit-hint" :class="{ 'limit-reached': isDeviceLimitReached }">
          {{ $t('settings.account.deviceLimit', { current: devices.length, max: maxDevices }) }}
        </p>
      </SettingsGroup>

      <!-- Logout Group -->
      <SettingsGroup :title="$t('settings.account.actions')">
        <button class="logout-btn" @click="handleLogout">
          {{ $t('settings.account.logout') }}
        </button>
      </SettingsGroup>
    </template>

    <!-- Auth Dialog (Teleport to body for proper z-index handling) -->
    <Teleport to="body">
      <div v-if="showAuthDialog" class="dialog-overlay" @click="showAuthDialog = false">
        <AuthDialog
          :initial-mode="authDialogMode"
          @close="showAuthDialog = false"
          @success="onAuthSuccess"
          @click.stop
        />
      </div>
    </Teleport>
  </div>
</template>

<script setup lang="ts">
/**
 * AccountSection - Account Settings Section
 *
 * Provides account management functionality:
 * - Login/register buttons when logged out
 * - User info and VIP status when logged in
 * - Device list (max 3) with unbind option
 * - Logout button
 *
 * Uses the reusable settings control components:
 * - SettingsGroup for card-style grouping
 * - SettingItem for consistent row layout
 *
 * Integrates with AuthStore for authentication state management.
 *
 * @validates Requirements 1.1, 1.2, 1.3, 2.1, 2.2, 2.3, 2.4, 2.5, 2.6
 */

import { ref, computed, onMounted } from 'vue'
import { storeToRefs } from 'pinia'
import SettingsGroup from '@/components/settings/controls/SettingsGroup.vue'
import SettingItem from '@/components/settings/controls/SettingItem.vue'
import AuthDialog from '@/components/auth/AuthDialog.vue'
import { useAuthStore } from '@/stores/auth'

// ============================================
// Types
// ============================================

import { invoke } from '@tauri-apps/api/core'
import type { DeviceInfo, DeviceListResponse, UnbindDeviceResponse } from '@/types/auth'

/**
 * Device interface for template compatibility
 * Maps from backend DeviceInfo to frontend display format
 */
interface Device {
  id: string
  name: string
  osVersion: string
  lastActive: string
  isCurrent: boolean
}

// ============================================
// Store Integration
// ============================================

/** Auth store instance */
const authStore = useAuthStore()

/**
 * Extract reactive state from store using storeToRefs
 * This ensures reactivity is preserved when destructuring
 * @validates Requirements 1.1, 1.2, 1.3, 8.1
 */
const { user, license, isLoading: authLoading, error: authError } = storeToRefs(authStore)

// ============================================
// Computed Properties (from AuthStore)
// ============================================

/**
 * Whether user is logged in
 * Computed from AuthStore's isAuthenticated getter
 * @validates Requirements 1.2, 1.3
 */
const isLoggedIn = computed(() => authStore.isAuthenticated)

/**
 * User email address
 * @validates Requirements 4.1
 */
const userEmail = computed(() => user.value?.email ?? '')

/**
 * Whether user is VIP
 * @validates Requirements 4.2, 4.3
 */
const isVip = computed(() => authStore.isVip)

/**
 * User info object for template compatibility
 * Combines store state into the expected format
 */
const userInfo = computed(() => ({
  email: userEmail.value,
  isVip: isVip.value,
  vipExpiry: license.value?.grace_period_end,
}))

// ============================================
// Local State
// ============================================

/** List of bound devices */
const devices = ref<Device[]>([])

/** Maximum number of devices allowed */
const maxDevices = ref(3)

/** Loading state for device list */
const isLoadingDevices = ref(false)

/** Error message for device operations */
const deviceError = ref<string | null>(null)

// ============================================
// AuthDialog State
// ============================================

/**
 * Whether to show the auth dialog
 * @validates Requirements 2.1, 3.1
 */
const showAuthDialog = ref(false)

/**
 * Auth dialog mode: 'login' or 'register'
 * @validates Requirements 2.1, 3.1
 */
const authDialogMode = ref<'login' | 'register'>('login')

// ============================================
// Computed
// ============================================

/**
 * Check if device limit is reached
 */
const isDeviceLimitReached = computed(() => devices.value.length >= maxDevices.value)

// ============================================
// Lifecycle Hooks
// ============================================

/**
 * Initialize AuthStore on component mount
 * This loads any saved session from local storage
 * @validates Requirements 1.1, 1.2, 1.3
 */
onMounted(async () => {
  // Initialize auth store to load saved session
  // This will restore user state if a valid session exists
  await authStore.initialize()
  
  // Load device list if user is logged in
  // @validates Requirements 5.1, 5.2
  if (authStore.isAuthenticated) {
    await loadDevices()
  }
})

// ============================================
// Methods
// ============================================

/**
 * Format date for display
 * @param dateString - ISO date string
 */
function formatDate(dateString: string): string {
  try {
    const date = new Date(dateString)
    return date.toLocaleDateString()
  } catch {
    return dateString
  }
}

/**
 * Convert backend DeviceInfo to frontend Device format
 * @param deviceInfo - Backend device info
 * @returns Frontend device format
 */
function mapDeviceInfo(deviceInfo: DeviceInfo): Device {
  return {
    id: deviceInfo.device_id,
    name: deviceInfo.device_name,
    osVersion: deviceInfo.os_version,
    lastActive: deviceInfo.last_active_at,
    isCurrent: deviceInfo.is_current,
  }
}

/**
 * Load bound devices from backend
 * Calls get_bound_devices command and updates local state
 * @validates Requirements 5.1, 5.2, 5.3, 5.7
 */
async function loadDevices(): Promise<void> {
  if (!authStore.isAuthenticated) {
    devices.value = []
    return
  }

  isLoadingDevices.value = true
  deviceError.value = null

  try {
    const response = await invoke<DeviceListResponse>('get_bound_devices')
    
    if (response.success && response.devices) {
      // Map backend DeviceInfo to frontend Device format
      devices.value = response.devices.map(mapDeviceInfo)
      // Update max devices from backend response
      if (response.max_devices !== undefined) {
        maxDevices.value = response.max_devices
      }
      console.log(`Loaded ${devices.value.length} devices (max: ${maxDevices.value})`)
    } else {
      deviceError.value = response.error || '获取设备列表失败'
      console.error('Failed to load devices:', response.error)
    }
  } catch (error) {
    deviceError.value = error instanceof Error ? error.message : '获取设备列表失败'
    console.error('Error loading devices:', error)
  } finally {
    isLoadingDevices.value = false
  }
}

// ============================================
// Event Handlers
// ============================================

/**
 * Handle login button click
 * Opens login dialog/page
 * @validates Requirements 2.1
 */
function handleLogin(): void {
  authDialogMode.value = 'login'
  showAuthDialog.value = true
}

/**
 * Handle register button click
 * Opens registration dialog/page
 * @validates Requirements 3.1
 */
function handleRegister(): void {
  authDialogMode.value = 'register'
  showAuthDialog.value = true
}

/**
 * Handle auth dialog success event
 * Closes the dialog and refreshes state
 * @param authUser - The authenticated user info
 * @validates Requirements 2.3, 3.3
 */
async function onAuthSuccess(authUser: { id: string; email: string }): Promise<void> {
  showAuthDialog.value = false
  console.log('Auth success:', authUser.email)
  // AuthStore is already updated by AuthDialog, UI will react automatically
  // Load device list after successful login
  await loadDevices()
}

/**
 * Handle logout button click
 * Logs out the current user via AuthStore
 * @validates Requirements 6.1, 6.2, 6.3, 6.4
 */
async function handleLogout(): Promise<void> {
  // Call AuthStore logout which handles:
  // - Backend sign_out command
  // - Clearing user state, session, and license
  // - Clearing persisted session from local storage
  await authStore.logout()
  // Clear local device list
  devices.value = []
  console.log('Logout completed')
}

/**
 * Handle unbind device button click
 * Removes a device from the bound devices list via backend command
 * @param deviceId - ID of the device to unbind
 * @validates Requirements 5.5, 5.6
 */
async function handleUnbindDevice(deviceId: string): Promise<void> {
  deviceError.value = null
  
  try {
    const response = await invoke<UnbindDeviceResponse>('unbind_device', { deviceId })
    
    if (response.success) {
      // Remove the device from local list (Requirement 5.6)
      devices.value = devices.value.filter(d => d.id !== deviceId)
      console.log('Device unbound successfully:', deviceId)
    } else {
      deviceError.value = response.error || '解绑设备失败'
      console.error('Failed to unbind device:', response.error)
    }
  } catch (error) {
    deviceError.value = error instanceof Error ? error.message : '解绑设备失败'
    console.error('Error unbinding device:', error)
  }
}

/**
 * Handle upgrade VIP button click
 * Opens the VIP upgrade page or dialog
 * @validates Requirements 4.4
 */
function handleUpgradeVip(): void {
  // TODO: Implement VIP upgrade flow - could open a payment page or dialog
  console.log('Upgrade to VIP clicked')
  // For now, we can open an external link or show a dialog
  // This will be implemented when the payment system is ready
}

/**
 * Handle dismiss error button click
 * Clears the auth error from the store
 * @validates Requirements 8.3
 */
function handleDismissError(): void {
  authStore.clearError()
}

/**
 * Handle retry auth button click
 * Clears the error and reopens the auth dialog
 * @validates Requirements 8.3
 */
function handleRetryAuth(): void {
  // Clear the current error
  authStore.clearError()
  // Reopen the auth dialog in the same mode
  showAuthDialog.value = true
}
</script>

<style scoped>
.account-section {
  display: flex;
  flex-direction: column;
  gap: 24px;
}

/* Auth Error Notification Styles (Requirement 8.1, 8.2, 8.3) */
.auth-error-notification {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 12px 16px;
  background: rgba(244, 67, 54, 0.15);
  border: 1px solid rgba(244, 67, 54, 0.3);
  border-radius: 8px;
  margin-bottom: 8px;
}

.error-content {
  display: flex;
  align-items: center;
  gap: 10px;
  flex: 1;
  min-width: 0;
}

.error-icon {
  width: 20px;
  height: 20px;
  color: #f44336;
  flex-shrink: 0;
}

.error-message {
  color: #ff6b6b;
  font-size: 13px;
  line-height: 1.4;
  word-break: break-word;
}

.error-actions {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-shrink: 0;
  margin-left: 12px;
}

.error-retry-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 6px 12px;
  border: 1px solid rgba(255, 255, 255, 0.3);
  border-radius: 4px;
  background: rgba(255, 255, 255, 0.1);
  color: rgba(255, 255, 255, 0.9);
  font-size: 12px;
  cursor: pointer;
  transition: all 0.15s ease;
}

.error-retry-btn:hover {
  background: rgba(255, 255, 255, 0.15);
  border-color: rgba(255, 255, 255, 0.4);
}

.error-retry-btn svg {
  width: 14px;
  height: 14px;
}

.error-dismiss-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  padding: 0;
  border: none;
  border-radius: 4px;
  background: transparent;
  color: rgba(255, 255, 255, 0.6);
  cursor: pointer;
  transition: all 0.15s ease;
}

.error-dismiss-btn:hover {
  background: rgba(255, 255, 255, 0.1);
  color: rgba(255, 255, 255, 0.9);
}

.error-dismiss-btn svg {
  width: 16px;
  height: 16px;
}

/* Error slide transition */
.error-slide-enter-active,
.error-slide-leave-active {
  transition: all 0.2s ease;
}

.error-slide-enter-from,
.error-slide-leave-to {
  opacity: 0;
  transform: translateY(-10px);
}

.auth-buttons {
  display: flex;
  gap: 12px;
  margin-bottom: 12px;
}

.auth-btn {
  flex: 1;
  padding: 12px 24px;
  border: none;
  border-radius: 6px;
  font-size: 14px;
  font-weight: 500;
  cursor: pointer;
  transition: background-color 0.1s;
}

.login-btn {
  background: #4285f4;
  color: white;
}

.login-btn:hover {
  background: #5a9cf5;
}

.register-btn {
  background: rgba(255, 255, 255, 0.1);
  color: rgba(255, 255, 255, 0.9);
  border: 1px solid rgba(255, 255, 255, 0.2);
}

.register-btn:hover {
  background: rgba(255, 255, 255, 0.15);
}

.auth-hint {
  color: rgba(255, 255, 255, 0.5);
  font-size: 12px;
  text-align: center;
}

.user-email {
  color: rgba(255, 255, 255, 0.9);
}

.vip-badge {
  display: inline-block;
  padding: 2px 8px;
  border-radius: 4px;
  font-size: 12px;
  background: rgba(255, 255, 255, 0.1);
  color: rgba(255, 255, 255, 0.6);
}

.vip-badge.is-vip {
  background: rgba(255, 193, 7, 0.2);
  color: #ffc107;
}

.vip-status-container {
  display: flex;
  align-items: center;
  gap: 8px;
  flex-wrap: wrap;
}

.upgrade-btn {
  padding: 4px 12px;
  border: 1px solid rgba(255, 193, 7, 0.5);
  border-radius: 4px;
  background: rgba(255, 193, 7, 0.1);
  color: #ffc107;
  font-size: 12px;
  cursor: pointer;
  transition: all 0.1s;
}

.upgrade-btn:hover {
  background: rgba(255, 193, 7, 0.2);
  border-color: #ffc107;
}

.vip-expiry {
  margin-left: 8px;
  color: rgba(255, 255, 255, 0.5);
  font-size: 12px;
}

.device-list {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.device-loading {
  padding: 24px;
  text-align: center;
  color: rgba(255, 255, 255, 0.5);
  font-size: 13px;
}

/* Skeleton loading for device list (Requirement 8.1) */
.device-skeleton {
  display: flex;
  flex-direction: column;
  gap: 12px;
}

.device-skeleton-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 12px;
  background: rgba(255, 255, 255, 0.05);
  border-radius: 6px;
}

.skeleton-content {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.skeleton-line {
  background: linear-gradient(90deg, rgba(255, 255, 255, 0.1) 25%, rgba(255, 255, 255, 0.2) 50%, rgba(255, 255, 255, 0.1) 75%);
  background-size: 200% 100%;
  animation: skeleton-shimmer 1.5s infinite;
  border-radius: 4px;
}

.skeleton-name {
  width: 120px;
  height: 14px;
}

.skeleton-os {
  width: 80px;
  height: 11px;
}

.skeleton-time {
  width: 100px;
  height: 11px;
}

.skeleton-btn {
  width: 60px;
  height: 24px;
  background: linear-gradient(90deg, rgba(255, 255, 255, 0.1) 25%, rgba(255, 255, 255, 0.2) 50%, rgba(255, 255, 255, 0.1) 75%);
  background-size: 200% 100%;
  animation: skeleton-shimmer 1.5s infinite;
  border-radius: 4px;
}

@keyframes skeleton-shimmer {
  0% {
    background-position: 200% 0;
  }
  100% {
    background-position: -200% 0;
  }
}

.loading-text {
  text-align: center;
  color: rgba(255, 255, 255, 0.5);
  font-size: 12px;
  margin-top: 8px;
}

/* Button spinner for auth buttons (Requirement 8.1) */
.btn-spinner {
  display: inline-block;
  width: 16px;
  height: 16px;
  border: 2px solid rgba(255, 255, 255, 0.3);
  border-top-color: white;
  border-radius: 50%;
  animation: btn-spin 0.8s linear infinite;
}

@keyframes btn-spin {
  to {
    transform: rotate(360deg);
  }
}

.auth-btn:disabled {
  opacity: 0.7;
  cursor: not-allowed;
}

.device-error {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 12px;
  padding: 16px;
  background: rgba(244, 67, 54, 0.1);
  border-radius: 6px;
  color: #f44336;
  font-size: 13px;
}

.retry-btn {
  padding: 6px 16px;
  border: 1px solid rgba(255, 255, 255, 0.3);
  border-radius: 4px;
  background: transparent;
  color: rgba(255, 255, 255, 0.8);
  font-size: 12px;
  cursor: pointer;
  transition: all 0.1s;
}

.retry-btn:hover {
  background: rgba(255, 255, 255, 0.1);
}

.device-item {
  display: flex;
  justify-content: space-between;
  align-items: center;
  padding: 12px;
  background: rgba(255, 255, 255, 0.05);
  border-radius: 6px;
}

.device-info {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.device-header {
  display: flex;
  align-items: center;
  gap: 8px;
}

.device-name {
  color: rgba(255, 255, 255, 0.9);
  font-size: 13px;
}

.device-os {
  color: rgba(255, 255, 255, 0.6);
  font-size: 11px;
}

.device-last-active {
  color: rgba(255, 255, 255, 0.5);
  font-size: 11px;
}

.unbind-btn {
  padding: 4px 12px;
  border: 1px solid rgba(244, 67, 54, 0.5);
  border-radius: 4px;
  background: transparent;
  color: #f44336;
  font-size: 12px;
  cursor: pointer;
  transition: all 0.1s;
}

.unbind-btn:hover {
  background: rgba(244, 67, 54, 0.1);
}

.current-device-badge {
  padding: 4px 12px;
  background: rgba(76, 175, 80, 0.2);
  border-radius: 4px;
  color: #4caf50;
  font-size: 12px;
}

.device-limit-hint {
  color: rgba(255, 255, 255, 0.5);
  font-size: 12px;
  margin-top: 8px;
}

.device-limit-hint.limit-reached {
  color: #ff9800;
}

.logout-btn {
  padding: 10px 24px;
  border: 1px solid rgba(244, 67, 54, 0.5);
  border-radius: 6px;
  background: transparent;
  color: #f44336;
  font-size: 14px;
  cursor: pointer;
  transition: all 0.1s;
}

.logout-btn:hover {
  background: rgba(244, 67, 54, 0.1);
}

/* Dialog Overlay */
.dialog-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: rgba(0, 0, 0, 0.6);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 9999;
  backdrop-filter: blur(2px);
}
</style>
