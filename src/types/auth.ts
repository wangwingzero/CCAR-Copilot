/**
 * 认证相关类型定义
 */

/** 用户信息 */
export interface AuthUser {
  /** 用户 ID */
  id: string
  /** 邮箱 */
  email: string
  /** 昵称 */
  nickname?: string
  /** 头像 URL */
  avatar_url?: string
  /** 创建时间 */
  created_at: string
}

/** 认证会话 */
export interface AuthSession {
  /** 访问令牌 */
  access_token: string
  /** 刷新令牌 */
  refresh_token: string
  /** 令牌类型 */
  token_type: string
  /** 过期时间（秒） */
  expires_in: number
  /** 过期时间戳 */
  expires_at?: number
  /** 用户信息 */
  user: AuthUser
}

/** 登录请求 */
export interface LoginRequest {
  /** 邮箱 */
  email: string
  /** 密码 */
  password: string
}

/** 注册请求 */
export interface SignUpRequest {
  /** 邮箱 */
  email: string
  /** 密码 */
  password: string
  /** 昵称（可选） */
  nickname?: string
}

/** 后端返回的简化用户信息 */
export interface AuthUserInfo {
  /** 用户 ID */
  id: string
  /** 邮箱（可能为空） */
  email?: string
}

/** 认证响应（匹配后端 AuthResponse 结构） */
export interface AuthResponse {
  /** 是否成功 */
  success: boolean
  /** 用户信息（简化版，成功时返回） */
  user?: AuthUserInfo
  /** 错误信息（失败时） */
  error?: string
}

/** 许可证信息（前端使用的完整模型） */
export interface LicenseInfo {
  /** 订阅计划 */
  plan: 'free' | 'lifetime_vip'
  /** 是否有效 */
  is_valid: boolean
  /** 是否是 VIP */
  is_vip: boolean
  /** 用户 ID */
  user_id?: string
  /** 宽限期结束时间 */
  grace_period_end?: string
}

/**
 * 许可证响应（匹配后端 LicenseResponse 结构）
 * 
 * 后端直接返回许可证信息，不包含 success 字段
 * - validate_license: 返回 LicenseResponse
 * - get_cached_license: 返回 Option<LicenseResponse>（可能为 null）
 */
export interface LicenseResponse {
  /** 订阅计划 */
  plan: string
  /** 是否是 VIP */
  is_vip: boolean
  /** 是否有效 */
  is_valid: boolean
  /** 宽限期结束时间（可选） */
  grace_period_end?: string
}

/** 功能访问结果 */
export interface FeatureAccess {
  /** 是否允许访问 */
  allowed: boolean
  /** 功能名称 */
  feature: string
  /** 功能等级 */
  tier: 'free' | 'vip' | 'beta'
  /** 是否是 VIP 用户 */
  is_vip: boolean
  /** 使用次数 */
  usage_count?: number
  /** 使用限制 */
  usage_limit?: number
  /** 限制原因 */
  deny_reason?: string
  /** 升级提示 */
  upgrade_hint?: string
}

/** 使用统计 */
export interface UsageStats {
  /** 功能名称 */
  feature: string
  /** 今日使用次数 */
  today_count: number
  /** 每日限制 */
  daily_limit: number
  /** 是否达到限制 */
  is_limited: boolean
  /** 重置时间 */
  reset_at: string
}

// ============================================
// 设备管理相关
// ============================================

/** 设备信息 */
export interface DeviceInfo {
  /** 设备唯一 ID（机器指纹） */
  device_id: string
  /** 设备名称 */
  device_name: string
  /** 操作系统版本 */
  os_version: string
  /** 是否是当前设备 */
  is_current: boolean
  /** 绑定时间 */
  bound_at: string
  /** 最后活跃时间 */
  last_active_at: string
}

/** 设备列表响应 */
export interface DeviceListResponse {
  /** 是否成功 */
  success: boolean
  /** 设备列表 */
  devices?: DeviceInfo[]
  /** 最大设备数 */
  max_devices?: number
  /** 错误信息 */
  error?: string
}

/** 解绑设备响应 */
export interface UnbindDeviceResponse {
  /** 是否成功 */
  success: boolean
  /** 错误信息 */
  error?: string
}
