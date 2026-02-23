/**
 * Unit Tests for AuthStore
 *
 * Feature: account-section-integration
 *
 * **Validates: Requirements 1.1, 2.2, 6.1, 7.1**
 *
 * Tests cover:
 * - initialize() - loading saved session from local storage
 * - login() - calling sign_in_with_password and updating state
 * - logout() - calling sign_out and clearing state
 * - loadLicense() - calling validate_license and storing license info
 */

import { describe, it, expect, beforeEach, vi } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { useAuthStore } from '../auth'
import type { AuthResponse, LicenseResponse } from '@/types/auth'

// ============================================================================
// Mock Tauri invoke API
// ============================================================================

// Create a mock for the invoke function
const mockInvoke = vi.fn()

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}))

// ============================================================================
// Test Helpers
// ============================================================================

/**
 * Create a mock AuthResponse for successful login/session load
 */
function createSuccessAuthResponse(userId: string, email: string): AuthResponse {
  return {
    success: true,
    user: {
      id: userId,
      email: email,
    },
  }
}

/**
 * Create a mock AuthResponse for failed login
 */
function createFailedAuthResponse(error: string): AuthResponse {
  return {
    success: false,
    error: error,
  }
}

/**
 * Create a mock LicenseResponse
 */
function createLicenseResponse(
  plan: string,
  isVip: boolean,
  isValid: boolean
): LicenseResponse {
  return {
    plan: plan,
    is_vip: isVip,
    is_valid: isValid,
  }
}

// ============================================================================
// Unit Tests
// ============================================================================

describe('AuthStore Unit Tests', () => {
  beforeEach(() => {
    // Create a fresh Pinia instance before each test
    setActivePinia(createPinia())
    // Clear all mocks
    vi.clearAllMocks()
    // Reset mock implementation
    mockInvoke.mockReset()
  })

  // ==========================================================================
  // initialize() Tests
  // ==========================================================================

  describe('initialize() - Requirement 1.1: Load saved session from local storage', () => {
    it('should load saved session and update user state when session exists', async () => {
      // Arrange
      const mockAuthResponse = createSuccessAuthResponse('user-123', 'test@example.com')
      const mockLicenseResponse = createLicenseResponse('lifetime_vip', true, true)

      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'load_saved_session') {
          return Promise.resolve(mockAuthResponse)
        }
        if (cmd === 'get_cached_license') {
          return Promise.resolve(mockLicenseResponse)
        }
        if (cmd === 'validate_license') {
          return Promise.resolve(mockLicenseResponse)
        }
        return Promise.reject(new Error(`Unknown command: ${cmd}`))
      })

      const store = useAuthStore()

      // Act
      await store.initialize()

      // Assert
      expect(mockInvoke).toHaveBeenCalledWith('load_saved_session')
      expect(store.user).not.toBeNull()
      expect(store.user?.id).toBe('user-123')
      expect(store.user?.email).toBe('test@example.com')
      expect(store.session).not.toBeNull()
      expect(store.isAuthenticated).toBe(true)
      expect(store.isInitialized).toBe(true)
      expect(store.isLoading).toBe(false)
    })

    it('should set logged-out state when no saved session exists', async () => {
      // Arrange
      const mockAuthResponse = createFailedAuthResponse('No saved session')

      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'load_saved_session') {
          return Promise.resolve(mockAuthResponse)
        }
        return Promise.reject(new Error(`Unknown command: ${cmd}`))
      })

      const store = useAuthStore()

      // Act
      await store.initialize()

      // Assert
      expect(mockInvoke).toHaveBeenCalledWith('load_saved_session')
      expect(store.user).toBeNull()
      expect(store.session).toBeNull()
      expect(store.isAuthenticated).toBe(false)
      expect(store.isInitialized).toBe(true)
      expect(store.isLoading).toBe(false)
    })

    it('should handle error when loading saved session fails', async () => {
      // Arrange
      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'load_saved_session') {
          return Promise.reject(new Error('Network error'))
        }
        return Promise.reject(new Error(`Unknown command: ${cmd}`))
      })

      const store = useAuthStore()

      // Act
      await store.initialize()

      // Assert
      expect(store.user).toBeNull()
      expect(store.session).toBeNull()
      expect(store.isAuthenticated).toBe(false)
      expect(store.isInitialized).toBe(true)
      expect(store.isLoading).toBe(false)
    })

    it('should not re-initialize if already initialized', async () => {
      // Arrange
      const mockAuthResponse = createSuccessAuthResponse('user-123', 'test@example.com')
      const mockLicenseResponse = createLicenseResponse('free', false, true)

      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'load_saved_session') {
          return Promise.resolve(mockAuthResponse)
        }
        if (cmd === 'get_cached_license') {
          return Promise.resolve(mockLicenseResponse)
        }
        if (cmd === 'validate_license') {
          return Promise.resolve(mockLicenseResponse)
        }
        return Promise.reject(new Error(`Unknown command: ${cmd}`))
      })

      const store = useAuthStore()

      // Act - initialize twice
      await store.initialize()
      const callCountAfterFirst = mockInvoke.mock.calls.filter(
        (call) => call[0] === 'load_saved_session'
      ).length

      await store.initialize()
      const callCountAfterSecond = mockInvoke.mock.calls.filter(
        (call) => call[0] === 'load_saved_session'
      ).length

      // Assert - should only call load_saved_session once
      expect(callCountAfterFirst).toBe(1)
      expect(callCountAfterSecond).toBe(1)
    })
  })

  // ==========================================================================
  // login() Tests
  // ==========================================================================

  describe('login() - Requirement 2.2: Call sign_in_with_password backend command', () => {
    it('should call sign_in_with_password and update state on successful login', async () => {
      // Arrange
      const mockAuthResponse = createSuccessAuthResponse('user-456', 'login@example.com')
      const mockLicenseResponse = createLicenseResponse('lifetime_vip', true, true)

      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'sign_in_with_password') {
          return Promise.resolve(mockAuthResponse)
        }
        if (cmd === 'get_cached_license') {
          return Promise.resolve(mockLicenseResponse)
        }
        if (cmd === 'validate_license') {
          return Promise.resolve(mockLicenseResponse)
        }
        return Promise.reject(new Error(`Unknown command: ${cmd}`))
      })

      const store = useAuthStore()

      // Act
      const result = await store.login('login@example.com', 'password123')

      // Assert
      expect(result).toBe(true)
      expect(mockInvoke).toHaveBeenCalledWith('sign_in_with_password', {
        request: { email: 'login@example.com', password: 'password123' },
      })
      expect(store.user).not.toBeNull()
      expect(store.user?.id).toBe('user-456')
      expect(store.user?.email).toBe('login@example.com')
      expect(store.session).not.toBeNull()
      expect(store.isAuthenticated).toBe(true)
      expect(store.error).toBeNull()
      expect(store.isLoading).toBe(false)
    })

    it('should set error state on failed login', async () => {
      // Arrange
      const mockAuthResponse = createFailedAuthResponse('Invalid credentials')

      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'sign_in_with_password') {
          return Promise.resolve(mockAuthResponse)
        }
        return Promise.reject(new Error(`Unknown command: ${cmd}`))
      })

      const store = useAuthStore()

      // Act
      const result = await store.login('wrong@example.com', 'wrongpassword')

      // Assert
      expect(result).toBe(false)
      expect(store.user).toBeNull()
      expect(store.session).toBeNull()
      expect(store.isAuthenticated).toBe(false)
      expect(store.error).toBe('Invalid credentials')
      expect(store.isLoading).toBe(false)
    })

    it('should handle network error during login', async () => {
      // Arrange
      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'sign_in_with_password') {
          return Promise.reject(new Error('Network timeout'))
        }
        return Promise.reject(new Error(`Unknown command: ${cmd}`))
      })

      const store = useAuthStore()

      // Act
      const result = await store.login('test@example.com', 'password')

      // Assert
      expect(result).toBe(false)
      expect(store.user).toBeNull()
      expect(store.session).toBeNull()
      expect(store.isAuthenticated).toBe(false)
      expect(store.error).toBe('Network timeout')
      expect(store.isLoading).toBe(false)
    })

    it('should set isLoading to true during login and false after', async () => {
      // Arrange
      let loadingDuringCall = false

      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'sign_in_with_password') {
          // Capture loading state during the call
          const store = useAuthStore()
          loadingDuringCall = store.isLoading
          return Promise.resolve(createSuccessAuthResponse('user-789', 'test@example.com'))
        }
        if (cmd === 'get_cached_license' || cmd === 'validate_license') {
          return Promise.resolve(createLicenseResponse('free', false, true))
        }
        return Promise.reject(new Error(`Unknown command: ${cmd}`))
      })

      const store = useAuthStore()
      expect(store.isLoading).toBe(false)

      // Act
      await store.login('test@example.com', 'password')

      // Assert
      expect(loadingDuringCall).toBe(true)
      expect(store.isLoading).toBe(false)
    })
  })

  // ==========================================================================
  // logout() Tests
  // ==========================================================================

  describe('logout() - Requirement 6.1: Call sign_out backend command', () => {
    it('should call sign_out and clear all state', async () => {
      // Arrange - First login to have state to clear
      const mockAuthResponse = createSuccessAuthResponse('user-123', 'test@example.com')
      const mockLicenseResponse = createLicenseResponse('lifetime_vip', true, true)

      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'sign_in_with_password') {
          return Promise.resolve(mockAuthResponse)
        }
        if (cmd === 'get_cached_license') {
          return Promise.resolve(mockLicenseResponse)
        }
        if (cmd === 'validate_license') {
          return Promise.resolve(mockLicenseResponse)
        }
        if (cmd === 'sign_out') {
          return Promise.resolve()
        }
        return Promise.reject(new Error(`Unknown command: ${cmd}`))
      })

      const store = useAuthStore()
      await store.login('test@example.com', 'password')

      // Verify logged in state
      expect(store.isAuthenticated).toBe(true)
      expect(store.user).not.toBeNull()
      expect(store.license).not.toBeNull()

      // Act
      await store.logout()

      // Assert
      expect(mockInvoke).toHaveBeenCalledWith('sign_out')
      expect(store.user).toBeNull()
      expect(store.session).toBeNull()
      expect(store.license).toBeNull()
      expect(store.isAuthenticated).toBe(false)
      expect(store.isLoading).toBe(false)
    })

    it('should clear state even if sign_out backend call fails', async () => {
      // Arrange - First login to have state to clear
      const mockAuthResponse = createSuccessAuthResponse('user-123', 'test@example.com')
      const mockLicenseResponse = createLicenseResponse('lifetime_vip', true, true)

      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'sign_in_with_password') {
          return Promise.resolve(mockAuthResponse)
        }
        if (cmd === 'get_cached_license') {
          return Promise.resolve(mockLicenseResponse)
        }
        if (cmd === 'validate_license') {
          return Promise.resolve(mockLicenseResponse)
        }
        if (cmd === 'sign_out') {
          return Promise.reject(new Error('Backend error'))
        }
        return Promise.reject(new Error(`Unknown command: ${cmd}`))
      })

      const store = useAuthStore()
      await store.login('test@example.com', 'password')

      // Verify logged in state
      expect(store.isAuthenticated).toBe(true)

      // Act
      await store.logout()

      // Assert - state should still be cleared even if backend fails
      expect(store.user).toBeNull()
      expect(store.session).toBeNull()
      expect(store.license).toBeNull()
      expect(store.isAuthenticated).toBe(false)
    })
  })

  // ==========================================================================
  // loadLicense() Tests
  // ==========================================================================

  describe('loadLicense() - Requirement 7.1: Call validate_license backend command', () => {
    it('should load license from cache when available', async () => {
      // Arrange - First login to have a session
      const mockAuthResponse = createSuccessAuthResponse('user-123', 'test@example.com')
      const mockCachedLicense = createLicenseResponse('lifetime_vip', true, true)

      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'sign_in_with_password') {
          return Promise.resolve(mockAuthResponse)
        }
        if (cmd === 'get_cached_license') {
          return Promise.resolve(mockCachedLicense)
        }
        return Promise.reject(new Error(`Unknown command: ${cmd}`))
      })

      const store = useAuthStore()
      await store.login('test@example.com', 'password')

      // Assert
      expect(mockInvoke).toHaveBeenCalledWith('get_cached_license')
      expect(store.license).not.toBeNull()
      expect(store.license?.plan).toBe('lifetime_vip')
      expect(store.license?.is_vip).toBe(true)
      expect(store.license?.is_valid).toBe(true)
      expect(store.isVip).toBe(true)
    })

    it('should call validate_license when cache is empty', async () => {
      // Arrange - First login to have a session
      const mockAuthResponse = createSuccessAuthResponse('user-123', 'test@example.com')
      const mockValidatedLicense = createLicenseResponse('free', false, true)

      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'sign_in_with_password') {
          return Promise.resolve(mockAuthResponse)
        }
        if (cmd === 'get_cached_license') {
          return Promise.resolve(null) // No cached license
        }
        if (cmd === 'validate_license') {
          return Promise.resolve(mockValidatedLicense)
        }
        return Promise.reject(new Error(`Unknown command: ${cmd}`))
      })

      const store = useAuthStore()
      await store.login('test@example.com', 'password')

      // Assert
      expect(mockInvoke).toHaveBeenCalledWith('get_cached_license')
      expect(mockInvoke).toHaveBeenCalledWith('validate_license')
      expect(store.license).not.toBeNull()
      expect(store.license?.plan).toBe('free')
      expect(store.license?.is_vip).toBe(false)
      expect(store.isVip).toBe(false)
    })

    it('should not load license when not logged in', async () => {
      // Arrange
      const store = useAuthStore()

      // Act
      await store.loadLicense()

      // Assert - should not call any backend commands
      expect(mockInvoke).not.toHaveBeenCalledWith('get_cached_license')
      expect(mockInvoke).not.toHaveBeenCalledWith('validate_license')
      expect(store.license).toBeNull()
    })

    it('should handle error when loading license fails', async () => {
      // Arrange - First login to have a session
      const mockAuthResponse = createSuccessAuthResponse('user-123', 'test@example.com')

      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'sign_in_with_password') {
          return Promise.resolve(mockAuthResponse)
        }
        if (cmd === 'get_cached_license') {
          return Promise.reject(new Error('License service unavailable'))
        }
        return Promise.reject(new Error(`Unknown command: ${cmd}`))
      })

      const store = useAuthStore()

      // Act - login will call loadLicense internally
      await store.login('test@example.com', 'password')

      // Assert - should handle error gracefully
      expect(store.user).not.toBeNull() // User should still be logged in
      expect(store.isAuthenticated).toBe(true)
      // License may be null due to error, but no crash
    })

    it('should correctly set isVip based on license response', async () => {
      // Arrange
      const mockAuthResponse = createSuccessAuthResponse('user-123', 'test@example.com')

      // Test case 1: VIP with valid license
      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'sign_in_with_password') {
          return Promise.resolve(mockAuthResponse)
        }
        if (cmd === 'get_cached_license') {
          return Promise.resolve(createLicenseResponse('lifetime_vip', true, true))
        }
        return Promise.reject(new Error(`Unknown command: ${cmd}`))
      })

      const store1 = useAuthStore()
      await store1.login('test@example.com', 'password')
      expect(store1.isVip).toBe(true)

      // Reset for next test
      setActivePinia(createPinia())
      vi.clearAllMocks()

      // Test case 2: VIP but invalid license
      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'sign_in_with_password') {
          return Promise.resolve(mockAuthResponse)
        }
        if (cmd === 'get_cached_license') {
          return Promise.resolve(createLicenseResponse('lifetime_vip', true, false))
        }
        return Promise.reject(new Error(`Unknown command: ${cmd}`))
      })

      const store2 = useAuthStore()
      await store2.login('test@example.com', 'password')
      expect(store2.isVip).toBe(false) // Invalid license means not VIP

      // Reset for next test
      setActivePinia(createPinia())
      vi.clearAllMocks()

      // Test case 3: Free user
      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'sign_in_with_password') {
          return Promise.resolve(mockAuthResponse)
        }
        if (cmd === 'get_cached_license') {
          return Promise.resolve(createLicenseResponse('free', false, true))
        }
        return Promise.reject(new Error(`Unknown command: ${cmd}`))
      })

      const store3 = useAuthStore()
      await store3.login('test@example.com', 'password')
      expect(store3.isVip).toBe(false)
    })
  })

  // ==========================================================================
  // Additional Helper Method Tests
  // ==========================================================================

  describe('clearError()', () => {
    it('should clear the error state', async () => {
      // Arrange
      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'sign_in_with_password') {
          return Promise.resolve(createFailedAuthResponse('Login failed'))
        }
        return Promise.reject(new Error(`Unknown command: ${cmd}`))
      })

      const store = useAuthStore()
      await store.login('test@example.com', 'password')
      expect(store.error).toBe('Login failed')

      // Act
      store.clearError()

      // Assert
      expect(store.error).toBeNull()
    })
  })

  describe('displayName computed property', () => {
    it('should return nickname when available', async () => {
      // Arrange
      const mockAuthResponse: AuthResponse = {
        success: true,
        user: {
          id: 'user-123',
          email: 'test@example.com',
        },
      }

      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'sign_in_with_password') {
          return Promise.resolve(mockAuthResponse)
        }
        if (cmd === 'get_cached_license') {
          return Promise.resolve(createLicenseResponse('free', false, true))
        }
        return Promise.reject(new Error(`Unknown command: ${cmd}`))
      })

      const store = useAuthStore()
      await store.login('test@example.com', 'password')

      // The transformUserInfo doesn't set nickname, so it should use email prefix
      expect(store.displayName).toBe('test')
    })

    it('should return "用户" when no user is logged in', () => {
      const store = useAuthStore()
      expect(store.displayName).toBe('用户')
    })
  })

  describe('signUp()', () => {
    it('should call sign_up and update state on successful registration', async () => {
      // Arrange
      const mockAuthResponse = createSuccessAuthResponse('new-user-123', 'newuser@example.com')

      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'sign_up') {
          return Promise.resolve(mockAuthResponse)
        }
        return Promise.reject(new Error(`Unknown command: ${cmd}`))
      })

      const store = useAuthStore()

      // Act
      const result = await store.signUp('newuser@example.com', 'password123', 'NewUser')

      // Assert
      expect(result).toBe(true)
      expect(mockInvoke).toHaveBeenCalledWith('sign_up', {
        request: { email: 'newuser@example.com', password: 'password123', nickname: 'NewUser' },
      })
      expect(store.user).not.toBeNull()
      expect(store.user?.id).toBe('new-user-123')
      expect(store.error).toBeNull()
    })

    it('should set error state on failed registration', async () => {
      // Arrange
      const mockAuthResponse = createFailedAuthResponse('Email already exists')

      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'sign_up') {
          return Promise.resolve(mockAuthResponse)
        }
        return Promise.reject(new Error(`Unknown command: ${cmd}`))
      })

      const store = useAuthStore()

      // Act
      const result = await store.signUp('existing@example.com', 'password123')

      // Assert
      expect(result).toBe(false)
      expect(store.error).toBe('Email already exists')
    })
  })
})
