/**
 * 应用配置类型定义
 * 对应 Rust: src-tauri/src/database/settings.rs
 */

/** 通用设置 */
export interface GeneralConfig {
  /** 界面语言 */
  language: 'zh-CN' | 'en-US'
  /** 主题 */
  theme: 'light' | 'dark' | 'system'
  /** 开机自启动 */
  autoStart: boolean
  /** 最小化到托盘 */
  minimizeToTray: boolean
}

/** 通知设置 */
export interface NotificationConfig {
  /** 启动时显示通知 */
  startup: boolean
  /** 软件更新时显示通知 */
  softwareUpdate: boolean
}

/** 更新设置 */
export interface UpdateConfig {
  /** 自动检查更新 */
  autoCheck: boolean
  /** 检查间隔（小时） */
  checkIntervalHours: number
  /** 使用代理 */
  useProxy: boolean
  /** 代理 URL */
  proxyUrl: string
  /** 上次检查时间 */
  lastCheckTime: string
  /** 跳过的版本号 */
  skipVersion: string
}

/** 高级设置 */
export interface AdvancedConfig {
  /** 代理启用 */
  proxyEnabled: boolean
  /** 代理类型 */
  proxyType: 'http' | 'socks5'
  /** 代理主机 */
  proxyHost: string
  /** 代理端口 */
  proxyPort: number
  /** 调试日志 */
  debugLogging: boolean
  /** 调试日志路径 */
  debugLogPath: string
  /** 便携模式 */
  portableMode: boolean
}

/** 完整应用配置 */
export interface AppConfig {
  general: GeneralConfig
  notification: NotificationConfig
  update: UpdateConfig
  advanced: AdvancedConfig
}

/** 默认配置 */
export const DEFAULT_CONFIG: AppConfig = {
  general: {
    language: 'zh-CN',
    theme: 'system',
    autoStart: false,
    minimizeToTray: true,
  },
  notification: {
    startup: true,
    softwareUpdate: true,
  },
  update: {
    autoCheck: true,
    checkIntervalHours: 24,
    useProxy: false,
    proxyUrl: 'https://ghproxy.net/',
    lastCheckTime: '',
    skipVersion: '',
  },
  advanced: {
    proxyEnabled: false,
    proxyType: 'http',
    proxyHost: '',
    proxyPort: 8080,
    debugLogging: false,
    debugLogPath: '',
    portableMode: false,
  },
}
