/**
 * 认证状态管理 Store
 *
 * 管理用户认证状态、会话信息和许可证状态
 */

import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import type {
  AuthUser,
  AuthSession,
  AuthResponse,
  AuthUserInfo,
  LicenseInfo,
  LicenseResponse,
  FeatureAccess
} from '@/types/auth'

/**
 * 将后端返回的简化用户信息转换为前端使用的 AuthUser
 * 遵循 "后端追求极致传输，前端追求领域建模" 原则
 */
function transformUserInfo(info: AuthUserInfo): AuthUser {
  return {
    id: info.id,
    email: info.email || '',
    created_at: new Date().toISOString(), // 后端未返回，使用当前时间
  }
}

export const useAuthStore = defineStore('auth', () => {
  // ============================================
  // State
  // ============================================

  /** 当前用户 */
  const user = ref<AuthUser | null>(null)

  /** 当前会话 */
  const session = ref<AuthSession | null>(null)

  /** 许可证信息 */
  const license = ref<LicenseInfo | null>(null)

  /** 是否正在加载 */
  const isLoading = ref(false)

  /** 错误信息 */
  const error = ref<string | null>(null)

  /** 是否已初始化 */
  const isInitialized = ref(false)

  // ============================================
  // Computed
  // ============================================

  /** 是否已登录 */
  const isAuthenticated = computed(() => !!user.value && !!session.value)

  /** 是否是 VIP（使用后端返回的 is_vip 字段） */
  const isVip = computed(() => license.value?.is_vip === true && license.value?.is_valid === true)

  /** 用户显示名称 */
  const displayName = computed(() => user.value?.nickname || user.value?.email?.split('@')[0] || '用户')

  // ============================================
  // Actions
  // ============================================

  /**
   * 初始化认证状态（加载保存的会话）
   * 
   * 后端 load_saved_session 返回简化的 AuthUserInfo，
   * 不包含完整的 session 信息（token 等由后端管理）
   */
  async function initialize(): Promise<void> {
    if (isInitialized.value) return

    isLoading.value = true
    error.value = null

    try {
      // 尝试加载保存的会话
      const response = await invoke<AuthResponse>('load_saved_session')

      if (response.success && response.user) {
        // 后端返回简化的 AuthUserInfo，转换为前端 AuthUser
        user.value = transformUserInfo(response.user)
        // 后端管理 session，前端只需标记已认证状态
        // 创建一个占位 session 表示已登录
        session.value = {
          access_token: '', // 由后端管理
          refresh_token: '', // 由后端管理
          token_type: 'bearer',
          expires_in: 0,
          user: user.value
        }

        // 加载许可证信息
        await loadLicense()
      }
    } catch (e) {
      console.warn('加载会话失败:', e)
    } finally {
      isLoading.value = false
      isInitialized.value = true
    }
  }

  /**
   * 使用邮箱密码登录
   * 
   * 后端返回简化的 AuthUserInfo，不包含完整 session 信息
   */
  async function login(email: string, password: string): Promise<boolean> {
    isLoading.value = true
    error.value = null

    try {
      const response = await invoke<AuthResponse>('sign_in_with_password', {
        request: { email, password }
      })

      if (response.success && response.user) {
        // 后端返回简化的 AuthUserInfo，转换为前端 AuthUser
        user.value = transformUserInfo(response.user)
        // 创建占位 session 表示已登录（token 由后端管理）
        session.value = {
          access_token: '',
          refresh_token: '',
          token_type: 'bearer',
          expires_in: 0,
          user: user.value
        }

        // 加载许可证信息
        await loadLicense()

        return true
      } else {
        error.value = response.error || '登录失败'
        return false
      }
    } catch (e) {
      error.value = e instanceof Error ? e.message : '登录请求失败'
      return false
    } finally {
      isLoading.value = false
    }
  }

  /**
   * 注册新用户
   * 
   * 后端返回简化的 AuthUserInfo，不包含完整 session 信息
   */
  async function signUp(email: string, password: string, nickname?: string): Promise<boolean> {
    isLoading.value = true
    error.value = null

    try {
      const response = await invoke<AuthResponse>('sign_up', {
        request: { email, password, nickname }
      })

      if (response.success) {
        // 注册成功，可能需要验证邮箱
        if (response.user) {
          user.value = transformUserInfo(response.user)
          // 创建占位 session 表示已登录（token 由后端管理）
          session.value = {
            access_token: '',
            refresh_token: '',
            token_type: 'bearer',
            expires_in: 0,
            user: user.value
          }
        }
        return true
      } else {
        error.value = response.error || '注册失败'
        return false
      }
    } catch (e) {
      error.value = e instanceof Error ? e.message : '注册请求失败'
      return false
    } finally {
      isLoading.value = false
    }
  }

  /**
   * 退出登录
   */
  async function logout(): Promise<void> {
    isLoading.value = true

    try {
      await invoke('sign_out')
    } catch (e) {
      console.warn('退出登录失败:', e)
    } finally {
      // 清除本地状态
      user.value = null
      session.value = null
      license.value = null
      isLoading.value = false
    }
  }

  /**
   * 加载许可证信息
   * 
   * 后端返回结构：
   * - get_cached_license: Option<LicenseResponse>（可能为 null）
   * - validate_license: LicenseResponse（直接返回，无 success 字段）
   */
  async function loadLicense(): Promise<void> {
    if (!session.value) return

    try {
      // 先尝试从缓存获取（返回 LicenseResponse | null）
      const cachedResponse = await invoke<LicenseResponse | null>('get_cached_license')

      if (cachedResponse) {
        // 将后端 LicenseResponse 转换为前端 LicenseInfo
        license.value = {
          plan: cachedResponse.plan as 'free' | 'lifetime_vip',
          is_valid: cachedResponse.is_valid,
          is_vip: cachedResponse.is_vip,
          grace_period_end: cachedResponse.grace_period_end,
        }
        return
      }

      // 缓存无效，从服务器验证（直接返回 LicenseResponse）
      const response = await invoke<LicenseResponse>('validate_license')

      // 将后端 LicenseResponse 转换为前端 LicenseInfo
      license.value = {
        plan: response.plan as 'free' | 'lifetime_vip',
        is_valid: response.is_valid,
        is_vip: response.is_vip,
        grace_period_end: response.grace_period_end,
      }
    } catch (e) {
      console.warn('加载许可证失败:', e)
    }
  }

  /**
   * 刷新会话
   * 
   * 后端返回简化的 AuthUserInfo，不包含完整 session 信息
   */
  async function refreshSession(): Promise<boolean> {
    if (!session.value) return false

    try {
      const response = await invoke<AuthResponse>('refresh_session')

      if (response.success && response.user) {
        // 更新用户信息
        user.value = transformUserInfo(response.user)
        // 更新 session 中的用户信息
        session.value = {
          ...session.value,
          user: user.value
        }
        return true
      }
      return false
    } catch (e) {
      console.warn('刷新会话失败:', e)
      return false
    }
  }

  /**
   * 请求重置密码
   */
  async function resetPassword(email: string): Promise<boolean> {
    isLoading.value = true
    error.value = null

    try {
      const response = await invoke<{ success: boolean; error?: string }>('reset_password', {
        email
      })

      if (!response.success) {
        error.value = response.error || '重置密码失败'
      }
      return response.success
    } catch (e) {
      error.value = e instanceof Error ? e.message : '请求失败'
      return false
    } finally {
      isLoading.value = false
    }
  }

  /**
   * 检查功能访问权限
   */
  async function checkFeatureAccess(feature: string): Promise<FeatureAccess | null> {
    try {
      return await invoke<FeatureAccess>('check_feature_access', { feature })
    } catch (e) {
      console.warn('检查功能权限失败:', e)
      return null
    }
  }

  /**
   * 使用功能（检查并增加计数）
   */
  async function useFeature(feature: string): Promise<FeatureAccess | null> {
    try {
      return await invoke<FeatureAccess>('use_feature', { feature })
    } catch (e) {
      console.warn('使用功能失败:', e)
      return null
    }
  }

  /**
   * 清除错误
   */
  function clearError(): void {
    error.value = null
  }

  return {
    // State
    user,
    session,
    license,
    isLoading,
    error,
    isInitialized,

    // Computed
    isAuthenticated,
    isVip,
    displayName,

    // Actions
    initialize,
    login,
    signUp,
    logout,
    loadLicense,
    refreshSession,
    resetPassword,
    checkFeatureAccess,
    useFeature,
    clearError,
  }
})
