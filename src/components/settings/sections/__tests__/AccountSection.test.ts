/**
 * Unit Tests for AccountSection Component
 *
 * Feature: account-section-integration
 *
 * **Validates: Requirements 4.1, 4.2, 4.3, 4.4**
 *
 * Tests cover:
 * - Logged out state: shows login/register buttons
 * - Logged in state: shows user email
 * - VIP user: shows VIP badge with gold styling
 * - Non-VIP user: shows upgrade button
 */

import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest'
import { mount, VueWrapper, flushPromises } from '@vue/test-utils'
import { createPinia, setActivePinia, defineStore } from 'pinia'
import { createI18n } from 'vue-i18n'
import { ref, computed } from 'vue'
import AccountSection from '../AccountSection.vue'

// ============================================================================
// Mock Tauri invoke API
// ============================================================================

const mockInvoke = vi.fn()

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}))

// ============================================================================
// Create a real Pinia store for testing
// ============================================================================

// Define a test auth store that matches the real store's interface
const useTestAuthStore = defineStore('auth', () => {
  const user = ref<{ id: string; email: string; created_at: string } | null>(null)
  const session = ref<object | null>(null)
  const license = ref<{ plan: string; is_vip: boolean; is_valid: boolean; grace_period_end?: string } | null>(null)
  const isLoading = ref(false)
  const error = ref<string | null>(null)
  const isInitialized = ref(true)

  const isAuthenticated = computed(() => user.value !== null && session.value !== null)
  const isVip = computed(() => license.value?.is_vip === true && license.value?.is_valid === true)
  const displayName = computed(() => user.value?.email?.split('@')[0] ?? '用户')

  async function initialize() {
    // No-op for tests
  }

  async function login() {
    return true
  }

  async function logout() {
    user.value = null
    session.value = null
    license.value = null
  }

  async function loadLicense() {
    // No-op for tests
  }

  function clearError() {
    error.value = null
  }

  // Helper to set state for testing
  function _setTestState(options: {
    isAuthenticated?: boolean
    userEmail?: string
    isVip?: boolean
    licenseGracePeriodEnd?: string
  }) {
    const {
      isAuthenticated: auth = false,
      userEmail = 'test@example.com',
      isVip: vip = false,
      licenseGracePeriodEnd,
    } = options

    if (auth) {
      user.value = {
        id: 'user-123',
        email: userEmail,
        created_at: new Date().toISOString(),
      }
      session.value = {
        access_token: 'test-token',
        refresh_token: 'test-refresh',
        token_type: 'bearer',
        expires_in: 3600,
      }
      license.value = {
        plan: vip ? 'lifetime_vip' : 'free',
        is_vip: vip,
        is_valid: true,
        grace_period_end: licenseGracePeriodEnd,
      }
    } else {
      user.value = null
      session.value = null
      license.value = null
    }
  }

  return {
    user,
    session,
    license,
    isLoading,
    error,
    isInitialized,
    isAuthenticated,
    isVip,
    displayName,
    initialize,
    login,
    logout,
    loadLicense,
    clearError,
    _setTestState,
  }
})

// Mock the auth store module to use our test store
vi.mock('@/stores/auth', () => ({
  useAuthStore: () => useTestAuthStore(),
}))

// ============================================================================
// Test Setup
// ============================================================================

function createTestI18n() {
  return createI18n({
    legacy: false,
    locale: 'en',
    messages: {
      en: {
        common: { close: 'Close' },
        settings: {
          account: {
            title: 'Account',
            login: 'Login',
            register: 'Register',
            logout: 'Logout',
            loginHint: 'Login to sync settings and unlock premium features',
            userInfo: 'User Info',
            email: 'Email',
            vipStatus: 'VIP Status',
            vipActive: 'VIP Member',
            vipInactive: 'Free User',
            vipExpiry: 'Expires: {date}',
            devices: 'Device Management',
            loadingDevices: 'Loading devices...',
            retry: 'Retry',
            lastActive: 'Last active: {date}',
            unbind: 'Unbind',
            currentDevice: 'Current Device',
            deviceLimit: '{current}/{max} devices bound',
            actions: 'Account Actions',
            upgradeVip: 'Upgrade to VIP',
            manageDevices: 'Manage Devices',
          },
        },
      },
    },
  })
}

function mountAccountSection(authOptions: {
  isAuthenticated?: boolean
  userEmail?: string
  isVip?: boolean
  licenseGracePeriodEnd?: string
} = {}): VueWrapper {
  const i18n = createTestI18n()
  const pinia = createPinia()
  setActivePinia(pinia)

  // Get the store and set the test state BEFORE mounting
  const store = useTestAuthStore()
  store._setTestState(authOptions)

  return mount(AccountSection, {
    global: {
      plugins: [pinia, i18n],
      stubs: { teleport: true, AuthDialog: true },
    },
  })
}

// ============================================================================
// Unit Tests
// ============================================================================

describe('AccountSection Component Tests', () => {
  beforeEach(() => {
    vi.clearAllMocks()
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'get_bound_devices') {
        return Promise.resolve({ success: true, devices: [], max_devices: 3 })
      }
      if (cmd === 'load_saved_session') {
        return Promise.resolve({ success: false })
      }
      return Promise.resolve()
    })
  })

  afterEach(() => {
    vi.clearAllMocks()
  })

  describe('Logged Out State - Requirement 1.3', () => {
    it('should show login button when user is not logged in', async () => {
      const wrapper = mountAccountSection({ isAuthenticated: false })
      await flushPromises()
      const loginBtn = wrapper.find('.login-btn')
      expect(loginBtn.exists()).toBe(true)
      expect(loginBtn.text()).toBe('Login')
      wrapper.unmount()
    })

    it('should show register button when user is not logged in', async () => {
      const wrapper = mountAccountSection({ isAuthenticated: false })
      await flushPromises()
      const registerBtn = wrapper.find('.register-btn')
      expect(registerBtn.exists()).toBe(true)
      expect(registerBtn.text()).toBe('Register')
      wrapper.unmount()
    })

    it('should show login hint when user is not logged in', async () => {
      const wrapper = mountAccountSection({ isAuthenticated: false })
      await flushPromises()
      const hint = wrapper.find('.auth-hint')
      expect(hint.exists()).toBe(true)
      expect(hint.text()).toContain('Login to sync settings')
      wrapper.unmount()
    })

    it('should not show user info section when logged out', async () => {
      const wrapper = mountAccountSection({ isAuthenticated: false })
      await flushPromises()
      expect(wrapper.find('.user-email').exists()).toBe(false)
      expect(wrapper.find('.vip-badge').exists()).toBe(false)
      wrapper.unmount()
    })

    it('should not show logout button when logged out', async () => {
      const wrapper = mountAccountSection({ isAuthenticated: false })
      await flushPromises()
      expect(wrapper.find('.logout-btn').exists()).toBe(false)
      wrapper.unmount()
    })
  })

  describe('Logged In State - Requirement 4.1: Display user email', () => {
    it('should display user email when logged in', async () => {
      const wrapper = mountAccountSection({ isAuthenticated: true, userEmail: 'user@example.com' })
      await flushPromises()
      const userEmail = wrapper.find('.user-email')
      expect(userEmail.exists()).toBe(true)
      expect(userEmail.text()).toBe('user@example.com')
      wrapper.unmount()
    })

    it('should not show login/register buttons when logged in', async () => {
      const wrapper = mountAccountSection({ isAuthenticated: true })
      await flushPromises()
      expect(wrapper.find('.login-btn').exists()).toBe(false)
      expect(wrapper.find('.register-btn').exists()).toBe(false)
      wrapper.unmount()
    })

    it('should show logout button when logged in', async () => {
      const wrapper = mountAccountSection({ isAuthenticated: true })
      await flushPromises()
      const logoutBtn = wrapper.find('.logout-btn')
      expect(logoutBtn.exists()).toBe(true)
      expect(logoutBtn.text()).toBe('Logout')
      wrapper.unmount()
    })

    it('should display different email addresses correctly', async () => {
      const emails = ['test@example.com', 'user.name@domain.org', 'admin@company.co.uk']
      for (const email of emails) {
        const wrapper = mountAccountSection({ isAuthenticated: true, userEmail: email })
        await flushPromises()
        expect(wrapper.find('.user-email').text()).toBe(email)
        wrapper.unmount()
      }
    })
  })

  describe('VIP Status Display - Requirements 4.2, 4.3', () => {
    it('should display VIP badge when user is VIP - Requirement 4.2', async () => {
      const wrapper = mountAccountSection({ isAuthenticated: true, isVip: true })
      await flushPromises()
      const vipBadge = wrapper.find('.vip-badge')
      expect(vipBadge.exists()).toBe(true)
      expect(vipBadge.text()).toBe('VIP Member')
      wrapper.unmount()
    })

    it('should have is-vip class on VIP badge for gold styling - Requirement 4.3', async () => {
      const wrapper = mountAccountSection({ isAuthenticated: true, isVip: true })
      await flushPromises()
      expect(wrapper.find('.vip-badge').classes()).toContain('is-vip')
      wrapper.unmount()
    })

    it('should display Free User badge when user is not VIP - Requirement 4.2', async () => {
      const wrapper = mountAccountSection({ isAuthenticated: true, isVip: false })
      await flushPromises()
      const vipBadge = wrapper.find('.vip-badge')
      expect(vipBadge.exists()).toBe(true)
      expect(vipBadge.text()).toBe('Free User')
      wrapper.unmount()
    })

    it('should not have is-vip class when user is not VIP', async () => {
      const wrapper = mountAccountSection({ isAuthenticated: true, isVip: false })
      await flushPromises()
      expect(wrapper.find('.vip-badge').classes()).not.toContain('is-vip')
      wrapper.unmount()
    })

    it('should display VIP expiry date when available', async () => {
      const wrapper = mountAccountSection({ isAuthenticated: true, isVip: true, licenseGracePeriodEnd: '2025-12-31T00:00:00Z' })
      await flushPromises()
      const vipExpiry = wrapper.find('.vip-expiry')
      expect(vipExpiry.exists()).toBe(true)
      expect(vipExpiry.text()).toContain('Expires')
      wrapper.unmount()
    })
  })

  describe('Non-VIP Upgrade Prompt - Requirement 4.4', () => {
    it('should show upgrade button for non-VIP users', async () => {
      const wrapper = mountAccountSection({ isAuthenticated: true, isVip: false })
      await flushPromises()
      const upgradeBtn = wrapper.find('.upgrade-btn')
      expect(upgradeBtn.exists()).toBe(true)
      expect(upgradeBtn.text()).toBe('Upgrade to VIP')
      wrapper.unmount()
    })

    it('should not show upgrade button for VIP users', async () => {
      const wrapper = mountAccountSection({ isAuthenticated: true, isVip: true })
      await flushPromises()
      expect(wrapper.find('.upgrade-btn').exists()).toBe(false)
      wrapper.unmount()
    })

    it('should not show upgrade button when logged out', async () => {
      const wrapper = mountAccountSection({ isAuthenticated: false })
      await flushPromises()
      expect(wrapper.find('.upgrade-btn').exists()).toBe(false)
      wrapper.unmount()
    })
  })

  describe('Button Click Handlers', () => {
    it('should open auth dialog in login mode when login button is clicked', async () => {
      const wrapper = mountAccountSection({ isAuthenticated: false })
      await flushPromises()
      const loginBtn = wrapper.find('.login-btn')
      expect(loginBtn.exists()).toBe(true)
      await loginBtn.trigger('click')
      await flushPromises()
      const vm = wrapper.vm as any
      expect(vm.showAuthDialog).toBe(true)
      expect(vm.authDialogMode).toBe('login')
      wrapper.unmount()
    })

    it('should open auth dialog in register mode when register button is clicked', async () => {
      const wrapper = mountAccountSection({ isAuthenticated: false })
      await flushPromises()
      const registerBtn = wrapper.find('.register-btn')
      expect(registerBtn.exists()).toBe(true)
      await registerBtn.trigger('click')
      await flushPromises()
      const vm = wrapper.vm as any
      expect(vm.showAuthDialog).toBe(true)
      expect(vm.authDialogMode).toBe('register')
      wrapper.unmount()
    })
  })

  describe('Device Management Section - Requirement 5.7', () => {
    it('should show device limit hint when logged in', async () => {
      const wrapper = mountAccountSection({ isAuthenticated: true })
      await flushPromises()
      await new Promise(resolve => setTimeout(resolve, 50))
      await flushPromises()
      const deviceLimitHint = wrapper.find('.device-limit-hint')
      expect(deviceLimitHint.exists()).toBe(true)
      expect(deviceLimitHint.text()).toContain('devices bound')
      wrapper.unmount()
    })

    it('should not show device section when logged out', async () => {
      const wrapper = mountAccountSection({ isAuthenticated: false })
      await flushPromises()
      expect(wrapper.find('.device-list').exists()).toBe(false)
      expect(wrapper.find('.device-limit-hint').exists()).toBe(false)
      wrapper.unmount()
    })
  })
})
