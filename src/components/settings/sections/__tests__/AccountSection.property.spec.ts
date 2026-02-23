/**
 * Property-Based Tests for AccountSection Device Info Completeness
 *
 * Feature: account-section-integration, Property 1: Device Info Completeness
 *
 * **Validates: Requirements 5.3**
 *
 * Property Definition:
 * For any device in the bound devices list, the rendered output SHALL contain
 * the device name, OS version, and last active time.
 *
 * This property ensures that regardless of the device data returned by the backend,
 * the UI always displays all required information fields.
 */

import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { createPinia, setActivePinia, defineStore } from 'pinia'
import { createI18n } from 'vue-i18n'
import { ref, computed } from 'vue'
import * as fc from 'fast-check'
import AccountSection from '../AccountSection.vue'
import type { DeviceInfo } from '@/types/auth'

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

// ============================================================================
// Arbitraries for Device Info
// ============================================================================

/**
 * Generate a valid alphanumeric string for device IDs and names
 * This avoids issues with HTML rendering trimming whitespace
 */
const alphanumericString = (minLength: number, maxLength: number): fc.Arbitrary<string> =>
  fc.string({
    unit: fc.constantFrom(...'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_'),
    minLength,
    maxLength,
  })

/**
 * Generate a valid ISO date string within a reasonable range
 */
const validIsoDateString: fc.Arbitrary<string> = fc
  .integer({ min: Date.UTC(2020, 0, 1), max: Date.UTC(2030, 11, 31) })
  .map(timestamp => new Date(timestamp).toISOString())

/**
 * Arbitrary for generating valid DeviceInfo objects
 *
 * Constraints:
 * - device_id: alphanumeric string (machine fingerprint)
 * - device_name: alphanumeric string (human-readable device name)
 * - os_version: alphanumeric string (can be empty for unknown OS)
 * - is_current: boolean
 * - bound_at: valid ISO date string
 * - last_active_at: valid ISO date string
 */
const deviceInfoArbitrary: fc.Arbitrary<DeviceInfo> = fc.record({
  device_id: alphanumericString(1, 32),
  device_name: alphanumericString(1, 50),
  os_version: alphanumericString(0, 30),
  is_current: fc.boolean(),
  bound_at: validIsoDateString,
  last_active_at: validIsoDateString,
})

/**
 * Arbitrary for generating a list of devices (1-3 devices, matching max_devices limit)
 */
const deviceListArbitrary: fc.Arbitrary<DeviceInfo[]> = fc.array(deviceInfoArbitrary, { minLength: 1, maxLength: 3 })

// ============================================================================
// Helper Functions
// ============================================================================

/**
 * Format date for display - matches the component's formatDate function
 * @param dateString - ISO date string
 * @returns Formatted date string
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
 * Mount AccountSection with specific device list
 * @param devices - List of devices to display
 */
async function mountWithDevices(devices: DeviceInfo[]) {
  const i18n = createTestI18n()
  const pinia = createPinia()
  setActivePinia(pinia)

  // Set up mock to return the specified devices
  mockInvoke.mockImplementation((cmd: string) => {
    if (cmd === 'get_bound_devices') {
      return Promise.resolve({
        success: true,
        devices: devices,
        max_devices: 3,
      })
    }
    if (cmd === 'load_saved_session') {
      return Promise.resolve({ success: false })
    }
    return Promise.resolve()
  })

  // Set authenticated state
  const store = useTestAuthStore()
  store._setTestState({ isAuthenticated: true, userEmail: 'test@example.com', isVip: true })

  const wrapper = mount(AccountSection, {
    global: {
      plugins: [pinia, i18n],
      stubs: { teleport: true, AuthDialog: true },
    },
  })

  // Wait for component to mount and load devices
  await flushPromises()
  // Additional wait for async device loading
  await new Promise(resolve => setTimeout(resolve, 50))
  await flushPromises()

  return wrapper
}

/**
 * Extract device item text content from wrapper
 * @param wrapper - Vue test wrapper
 * @param index - Device index
 * @returns Object containing device item text parts
 */
function extractDeviceItemContent(wrapper: ReturnType<typeof mount>, index: number) {
  const deviceItems = wrapper.findAll('.device-item')
  if (index >= deviceItems.length) {
    return null
  }

  const deviceItem = deviceItems[index]
  return {
    fullText: deviceItem.text(),
    name: deviceItem.find('.device-name')?.text() ?? '',
    osVersion: deviceItem.find('.device-os')?.text() ?? '',
    lastActive: deviceItem.find('.device-last-active')?.text() ?? '',
  }
}

// ============================================================================
// Property-Based Tests
// ============================================================================

describe('Feature: account-section-integration, Property 1: Device Info Completeness', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  afterEach(() => {
    vi.clearAllMocks()
  })

  /**
   * Property 1: Device Info Completeness
   *
   * For any device in the bound devices list, the rendered output SHALL contain
   * the device name, OS version, and last active time.
   *
   * **Validates: Requirements 5.3**
   *
   * THE AccountSection SHALL display each device's name, OS version, and last active time
   */
  it('Property 1: For any device, all required fields (name, OS version, last active time) should be displayed', async () => {
    await fc.assert(
      fc.asyncProperty(deviceListArbitrary, async (devices) => {
        const wrapper = await mountWithDevices(devices)

        try {
          // Verify each device's required fields are displayed
          for (let i = 0; i < devices.length; i++) {
            const device = devices[i]
            const content = extractDeviceItemContent(wrapper, i)

            // Property assertion: content must exist
            expect(content).not.toBeNull()

            if (content) {
              // Property 1a: Device name must be displayed
              expect(content.name).toBe(device.device_name)

              // Property 1b: OS version must be displayed
              expect(content.osVersion).toBe(device.os_version)

              // Property 1c: Last active time must be displayed (formatted)
              const formattedDate = formatDate(device.last_active_at)
              expect(content.lastActive).toContain(formattedDate)
            }
          }

          return true
        } finally {
          wrapper.unmount()
        }
      }),
      {
        numRuns: 20,
      }
    )
  }, 30000)

  /**
   * Property 1a: Device Name Completeness
   *
   * For any device with a non-empty name, the name SHALL be visible in the rendered output.
   */
  it('Property 1a: Device name should always be displayed for any valid device', async () => {
    await fc.assert(
      fc.asyncProperty(deviceInfoArbitrary, async (device) => {
        const wrapper = await mountWithDevices([device])

        try {
          const content = extractDeviceItemContent(wrapper, 0)
          expect(content).not.toBeNull()
          expect(content?.name).toBe(device.device_name)
          return true
        } finally {
          wrapper.unmount()
        }
      }),
      {
        numRuns: 20,
      }
    )
  }, 30000)

  /**
   * Property 1b: OS Version Completeness
   *
   * For any device, the OS version SHALL be visible in the rendered output.
   */
  it('Property 1b: OS version should always be displayed for any valid device', async () => {
    await fc.assert(
      fc.asyncProperty(deviceInfoArbitrary, async (device) => {
        const wrapper = await mountWithDevices([device])

        try {
          const content = extractDeviceItemContent(wrapper, 0)
          expect(content).not.toBeNull()
          expect(content?.osVersion).toBe(device.os_version)
          return true
        } finally {
          wrapper.unmount()
        }
      }),
      {
        numRuns: 20,
      }
    )
  }, 30000)

  /**
   * Property 1c: Last Active Time Completeness
   *
   * For any device, the last active time SHALL be visible in the rendered output.
   */
  it('Property 1c: Last active time should always be displayed for any valid device', async () => {
    await fc.assert(
      fc.asyncProperty(deviceInfoArbitrary, async (device) => {
        const wrapper = await mountWithDevices([device])

        try {
          const content = extractDeviceItemContent(wrapper, 0)
          expect(content).not.toBeNull()

          // The last active time should contain the formatted date
          const formattedDate = formatDate(device.last_active_at)
          expect(content?.lastActive).toContain(formattedDate)
          return true
        } finally {
          wrapper.unmount()
        }
      }),
      {
        numRuns: 20,
      }
    )
  }, 30000)

  /**
   * Property 2: Current Device Badge
   *
   * For any device where is_current is true, the current device badge SHALL be displayed.
   *
   * **Validates: Requirements 5.4**
   */
  it('Property 2: Current device badge should be displayed when is_current is true', async () => {
    // Generate device with is_current = true
    const currentDeviceArbitrary = deviceInfoArbitrary.map(d => ({ ...d, is_current: true }))

    await fc.assert(
      fc.asyncProperty(currentDeviceArbitrary, async (device) => {
        const wrapper = await mountWithDevices([device])

        try {
          const deviceItems = wrapper.findAll('.device-item')
          expect(deviceItems.length).toBe(1)

          const currentBadge = deviceItems[0].find('.current-device-badge')
          expect(currentBadge.exists()).toBe(true)
          expect(currentBadge.text()).toBe('Current Device')
          return true
        } finally {
          wrapper.unmount()
        }
      }),
      {
        numRuns: 20,
      }
    )
  }, 30000)

  /**
   * Property 3: Non-Current Device Unbind Button
   *
   * For any device where is_current is false, the unbind button SHALL be displayed.
   *
   * **Validates: Requirements 5.5**
   */
  it('Property 3: Unbind button should be displayed for non-current devices', async () => {
    // Generate device with is_current = false
    const nonCurrentDeviceArbitrary = deviceInfoArbitrary.map(d => ({ ...d, is_current: false }))

    await fc.assert(
      fc.asyncProperty(nonCurrentDeviceArbitrary, async (device) => {
        const wrapper = await mountWithDevices([device])

        try {
          const deviceItems = wrapper.findAll('.device-item')
          expect(deviceItems.length).toBe(1)

          const unbindBtn = deviceItems[0].find('.unbind-btn')
          expect(unbindBtn.exists()).toBe(true)
          expect(unbindBtn.text()).toBe('Unbind')
          return true
        } finally {
          wrapper.unmount()
        }
      }),
      {
        numRuns: 20,
      }
    )
  }, 30000)

  /**
   * Property 4: Device Count Display
   *
   * For any list of devices, the device count display SHALL show the correct count.
   *
   * **Validates: Requirements 5.7**
   */
  it('Property 4: Device count should correctly reflect the number of devices', async () => {
    await fc.assert(
      fc.asyncProperty(deviceListArbitrary, async (devices) => {
        const wrapper = await mountWithDevices(devices)

        try {
          const deviceLimitHint = wrapper.find('.device-limit-hint')
          expect(deviceLimitHint.exists()).toBe(true)

          // The hint should contain the device count
          const hintText = deviceLimitHint.text()
          expect(hintText).toContain(`${devices.length}/3`)
          return true
        } finally {
          wrapper.unmount()
        }
      }),
      {
        numRuns: 20,
      }
    )
  }, 30000)
})
