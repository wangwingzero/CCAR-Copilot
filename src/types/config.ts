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
  /** 局方文件保存目录（空字符串表示使用默认目录） */
  regulationStoragePath: string
  /** 每天自动同步局方官网文件 */
  regulationAutoSyncEnabled: boolean
  /** 自动同步仅在 Wi-Fi 连接时执行 */
  regulationAutoSyncWifiOnly: boolean
  /** 启用 MinerU 在线 OCR */
  mineruOcrEnabled: boolean
  /** 优先使用 MinerU 在线 OCR，失败后回退本地 OCR */
  mineruOcrPreferOnline: boolean
  /** MinerU API Key */
  mineruApiKey: string
  /** 启用 AI 知识库服务器同步 */
  knowledgeServerSyncEnabled: boolean
  /** 局方本地更新完成后自动刷新并同步 AI 知识库 */
  knowledgeAutoSyncAfterRegulationUpdate: boolean
  /** AI 知识库同步方式 */
  knowledgeSyncMode: 'api' | 'ssh'
  /** AI 知识库 API 地址 */
  knowledgeApiUrl: string
  /** AI 知识库 API Token */
  knowledgeApiToken: string
  /** AI 知识库服务器地址 */
  knowledgeServerHost: string
  /** AI 知识库 SSH 端口 */
  knowledgeServerPort: number
  /** AI 知识库 SSH 用户 */
  knowledgeServerUser: string
  /** AI 知识库 SSH 私钥路径 */
  knowledgeServerKeyPath: string
  /** AI 知识库服务器发布目录 */
  knowledgeServerRemoteDir: string
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
    regulationStoragePath: '',
    regulationAutoSyncEnabled: false,
    regulationAutoSyncWifiOnly: true,
    mineruOcrEnabled: false,
    mineruOcrPreferOnline: false,
    mineruApiKey: '',
    knowledgeServerSyncEnabled: true,
    knowledgeAutoSyncAfterRegulationUpdate: true,
    knowledgeSyncMode: 'api',
    knowledgeApiUrl: 'https://ccar-api.hudawang.cn',
    knowledgeApiToken: '',
    knowledgeServerHost: '154.9.27.44',
    knowledgeServerPort: 7668,
    knowledgeServerUser: 'root',
    knowledgeServerKeyPath: 'C:\\Users\\wangh\\.ssh\\154.9.27.44_id_ed25519',
    knowledgeServerRemoteDir: '/www/wwwroot/ccar-knowledge-data',
  },
}
