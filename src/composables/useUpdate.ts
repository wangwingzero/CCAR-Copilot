/**
 * 自动更新 Composable
 *
 * 提供应用自动更新功能的响应式接口。
 *
 * # 功能
 *
 * - 检查更新
 * - 下载并安装更新
 * - 获取/设置更新配置
 * - 重启应用
 *
 * # Requirements
 *
 * - 19.1: 启动时和定期检查更新
 * - 19.2: 更新可用时通知用户并显示发布说明
 * - 19.3: 后台下载和安装更新
 * - 19.4: 更新失败时回滚到上一版本
 * - 19.5: 支持增量更新以减少下载大小
 * - 19.6: 用户可以在设置中禁用自动更新
 */

import { ref, computed, onMounted, watch } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type { Composer } from 'vue-i18n'
import { i18n } from '@/locales'
import { useToast } from '@/composables/useToast'
import { useSettingsStore } from '@/stores/settings'

/** 翻译 helper(不依赖 setup scope, 可在模块级 watch / event handler 里用) */
function tr(key: string, named?: Record<string, unknown>): string {
  const composer = i18n.global as unknown as Composer
  return named ? composer.t(key, named) : composer.t(key)
}

/** 模块级 toast 句柄(useToast 内部是单例,任何位置 import 都是同一份 state) */
const { showToast } = useToast()

/** 延迟拿 Pinia store 引用(避免模块加载时 Pinia 还没初始化) */
let _settingsStore: ReturnType<typeof useSettingsStore> | null = null
function getSettingsStore() {
  if (!_settingsStore) {
    _settingsStore = useSettingsStore()
  }
  return _settingsStore
}

/**
 * 更新信息
 */
export interface UpdateInfo {
  /** 新版本号 */
  version: string
  /** 发布说明 */
  notes: string | null
  /** 发布日期 */
  date: string | null
  /** 下载大小（字节） */
  downloadSize: number | null
  /** 最新安装包下载地址 */
  downloadUrl?: string | null
  /** Rust snake_case 兼容字段 */
  download_url?: string | null
}

/**
 * 更新状态类型
 */
export type UpdateStatusType =
  | 'Idle'
  | 'Checking'
  | 'Available'
  | 'Downloading'
  | 'Ready'
  | 'Installing'
  | 'PendingRestart'
  | 'UpToDate'
  | 'Error'

/**
 * 更新状态
 */
export interface UpdateStatus {
  status: UpdateStatusType
  info?: UpdateInfo
  progress?: number
  message?: string
}

/**
 * 更新 Composable 内部配置（对应 Rust 返回的 snake_case 格式）
 *
 * 注意：此类型与 @/types/config.ts 中的 UpdateConfig 是不同层面的配置。
 * - @/types/config.ts 的 UpdateConfig 对应 settings store 的前端配置
 * - 此类型对应 Rust 后端 get_update_config / set_update_config 命令的配置
 */
export interface RustUpdateConfig {
  /** 是否启用自动更新 */
  auto_update_enabled: boolean
  /** 检查更新间隔（小时） */
  check_interval_hours: number
  /** 是否在启动时检查更新 */
  check_on_startup: boolean
  /** 是否自动下载更新 */
  auto_download: boolean
  /** 是否自动安装更新 */
  auto_install: boolean
}

/**
 * 自动更新 Composable
 *
 * @example
 * ```vue
 * <script setup lang="ts">
 * import { useUpdate } from '@/composables/useUpdate'
 *
 * const {
 *   status,
 *   currentVersion,
 *   isUpdateAvailable,
 *   checkForUpdate,
 *   downloadAndInstall,
 *   restartApp
 * } = useUpdate()
 *
 * // 检查更新
 * await checkForUpdate()
 *
 * // 如果有更新，下载并安装
 * if (isUpdateAvailable.value) {
 *   await downloadAndInstall()
 * }
 * </script>
 * ```
 */
// ============================================
// 模块级单例状态(跨组件 mount/unmount 持久)
// ============================================
//
// 为什么状态要提到模块顶层而不是 useUpdate() 内部:
//
// SettingsPanel 在 App.vue 用 `v-if="showSettings"` 包裹,关闭设置面板会真正
// 卸载 SettingsPanel 子树(含 UpdateSection)。如果状态放在 useUpdate() 内部
// 局部 ref,每次重新打开设置面板都会从 Idle 开始,导致:
//
//   1. 用户点「下载并安装」开始下载 (status=Downloading)
//   2. 因为 CDN 慢或者其他原因用户关掉设置面板等等
//   3. 重新打开设置面板 -> UpdateSection 重新 mount -> useUpdate() 重置 ->
//      status 回到 Idle -> 「下载并安装」按钮消失、进度条消失,什么都看不见
//   4. 但 Rust 端的 download_and_install_update 还在后台慢慢跑
//   5. 进度事件继续推送但前端 listener 已经被 onUnmounted 解除了 -> 状态再也
//      更新不了
//
// 把状态提到模块级 + listeners 永不卸载,UpdateSection 重新挂载后能立即看到
// 当前真实的下载进度。
const status = ref<UpdateStatus>({ status: 'Idle' })
const currentVersion = ref<string>('')
const config = ref<RustUpdateConfig>({
  auto_update_enabled: true,
  check_interval_hours: 24,
  check_on_startup: true,
  auto_download: true,
  auto_install: false,
})
const isLoading = ref(false)
const error = ref<string | null>(null)
const latestDownloadUrl = ref<string | null>(null)

// 下载字节累计与总大小(供 UI 展示)
const downloadedBytes = ref(0)
const totalBytes = ref<number | null>(null)

// 下载速度(B/s) 采用 EMA 平滑,避免瞬时数据抖动
const downloadSpeed = ref(0)
let lastBytesSampled = 0
let lastBytesSampledAt = 0

// 上次失败的动作(供「重试」按钮使用)
let lastFailedAction: 'check' | 'download' | null = null
let downloadPromise: Promise<UpdateStatus> | null = null
let checkRequestSeq = 0

// 单例资源,初始化一次后不释放,直到应用退出
const unlisteners: UnlistenFn[] = []
let initPromise: Promise<void> | null = null

// 计算属性
const isUpdateAvailable = computed(() => status.value.status === 'Available')
const isPendingRestart = computed(() => status.value.status === 'PendingRestart')
const isChecking = computed(() => status.value.status === 'Checking')
const isDownloading = computed(() => status.value.status === 'Downloading')
const updateInfo = computed(() => status.value.info)
const downloadProgress = computed(() => {
  if (totalBytes.value && totalBytes.value > 0) {
    return Math.min(100, (downloadedBytes.value / totalBytes.value) * 100)
  }
  return status.value.progress ?? 0
})

/** 剩余秒数,无法估计时返回 null */
const downloadEtaSeconds = computed<number | null>(() => {
  if (!totalBytes.value || downloadSpeed.value <= 0) return null
  const remaining = totalBytes.value - downloadedBytes.value
  if (remaining <= 0) return 0
  return Math.ceil(remaining / downloadSpeed.value)
})

function isUpdateFlowActive(s: UpdateStatusType): boolean {
  return s === 'Downloading' || s === 'Ready' || s === 'Installing' || s === 'PendingRestart'
}

// ============================================
// 状态变化时弹 Toast(模块级 watch,永久生效)
// ============================================
//
// 用户不一定停在「设置 -> 更新」面板,所以重要状态变化(发现新版本 / 下载完成 /
// 出错)主动弹 toast 提示。这是 v0.1.6 的关键 UX 改进。
watch(
  () => status.value.status,
  (newStatus, prevStatus) => {
    if (newStatus === prevStatus) return
    if (newStatus === 'Available') {
      const v = updateInfo.value?.version ?? ''
      showToast(tr('settings.update.toastNewVersion', { version: v }), 'info', 6000)
    }
    else if (newStatus === 'PendingRestart') {
      const v = updateInfo.value?.version ?? ''
      showToast(tr('settings.update.toastDownloaded', { version: v }), 'success', 6000)
    }
    else if (newStatus === 'Error') {
      const msg = status.value.message ?? error.value ?? ''
      showToast(tr('settings.update.toastError', { message: msg }), 'error', 6000)
    }
  }
)

export function useUpdate() {

  /**
   * 获取当前版本
   */
  async function fetchCurrentVersion(): Promise<void> {
    try {
      currentVersion.value = await invoke<string>('get_current_version')
    } catch (e) {
      console.error('获取当前版本失败:', e)
      error.value = String(e)
    }
  }

  /**
   * 获取更新配置
   */
  async function fetchConfig(): Promise<void> {
    try {
      config.value = await invoke<RustUpdateConfig>('get_update_config')
    } catch (e) {
      console.error('获取更新配置失败:', e)
      error.value = String(e)
    }
  }

  /**
   * 保存更新配置
   *
   * @param newConfig - 新的更新配置
   */
  async function saveConfig(newConfig: RustUpdateConfig): Promise<void> {
    try {
      await invoke('set_update_config', { config: newConfig })
      config.value = newConfig
    } catch (e) {
      console.error('保存更新配置失败:', e)
      error.value = String(e)
      throw e
    }
  }

  /**
   * 检查更新
   *
   * @returns 更新状态
   */
  async function checkForUpdate(): Promise<UpdateStatus> {
    if (!config.value.auto_update_enabled) {
      status.value = { status: 'Idle' }
      return status.value
    }

    isLoading.value = true
    error.value = null
    status.value = { status: 'Checking' }
    const requestSeq = ++checkRequestSeq

    try {
      const result = await invoke<UpdateStatus>('check_for_update')

      if (requestSeq !== checkRequestSeq || isUpdateFlowActive(status.value.status)) {
        return status.value
      }

      // 用户跳过的版本不再显示「发现新版本」卡片,视觉上当成 UpToDate 处理
      if (result.status === 'Available' && result.info) {
        const skipped = getSettingsStore().update.skipVersion
        if (skipped && skipped === result.info.version) {
          console.warn('[useUpdate] 跳过用户已忽略的版本:', skipped)
          status.value = { status: 'UpToDate' }
          lastFailedAction = null
          return status.value
        }
      }

      status.value = result
      lastFailedAction = null

      // 注意：不在这里自动触发下载。用户在「设置 → 更新」里看到「下载并安装」按钮
      // 后主动点击才开始下载，期间显示进度条；完成后再由用户主动点击「立即重启」
      // 完成升级。auto_download/auto_install 配置仍保留供未来扩展（例如静默后台
      // 更新模式），当前 UI 流程一律要求用户确认。

      return result
    } catch (e) {
      console.error('检查更新失败:', e)
      if (requestSeq !== checkRequestSeq || isUpdateFlowActive(status.value.status)) {
        return status.value
      }
      error.value = String(e)
      status.value = { status: 'Error', message: String(e) }
      lastFailedAction = 'check'
      return status.value
    } finally {
      isLoading.value = false
    }
  }

  /**
   * 下载并安装更新
   *
   * @returns 更新状态
   */
  async function downloadAndInstall(): Promise<UpdateStatus> {
    if (downloadPromise) {
      return downloadPromise
    }

    isLoading.value = true
    error.value = null
    downloadedBytes.value = 0
    totalBytes.value = null
    downloadSpeed.value = 0
    lastBytesSampled = 0
    lastBytesSampledAt = 0
    checkRequestSeq += 1
    status.value = { status: 'Downloading', progress: 0 }

    downloadPromise = (async () => {
      try {
        const result = await invoke<UpdateStatus>('download_and_install_update')
        status.value = result
        lastFailedAction = null

        // 注意：不在这里自动触发重启。下载并安装完成后状态切到 PendingRestart，
        // UI 显示「立即重启」按钮，由用户主动点击调用 restartApp() 完成升级。

        return result
      } catch (e) {
        const message = String(e)
        if (message.includes('已有更新下载任务正在进行')) {
          return status.value
        }

        console.error('下载更新失败:', e)
        error.value = message
        status.value = { status: 'Error', message }
        lastFailedAction = 'download'
        return status.value
      } finally {
        downloadPromise = null
        isLoading.value = false
      }
    })()

    return downloadPromise
  }

  /**
   * 获取最新版 Windows 安装包地址,供用户手动用浏览器下载安装。
   */
  async function getLatestUpdateDownloadUrl(): Promise<string> {
    const urlFromCurrentCheck = updateInfo.value?.downloadUrl ?? updateInfo.value?.download_url
    if (urlFromCurrentCheck) {
      latestDownloadUrl.value = urlFromCurrentCheck
      return urlFromCurrentCheck
    }

    const url = await invoke<string>('get_latest_update_download_url')
    latestDownloadUrl.value = url
    return url
  }

  /**
   * 重启应用以完成更新
   */
  async function restartApp(): Promise<void> {
    try {
      status.value = { status: 'Installing' }
      await invoke('restart_app')
    } catch (e) {
      console.error('重启应用失败:', e)
      error.value = String(e)
      status.value = { status: 'Error', message: String(e) }
    }
  }

  /**
   * 忽略当前更新(仅清状态,不持久化)
   */
  function dismissUpdate(): void {
    status.value = { status: 'Idle' }
  }

  /**
   * 跳过当前发现的版本(持久化到 settings.update.skipVersion)
   *
   * 下次 checkForUpdate 即使后端返回这个版本,也不会再显示「发现新版本」卡片
   * 和 toast 通知,直到有更高版本发布。
   */
  function skipCurrentVersion(): void {
    const v = updateInfo.value?.version
    if (v) {
      getSettingsStore().updateUpdate({ skipVersion: v })
      console.warn('[useUpdate] 用户跳过版本:', v)
    }
    status.value = { status: 'Idle' }
  }

  /**
   * 重试上次失败的动作
   *
   * - 上次失败是 check  -> 重新 checkForUpdate
   * - 上次失败是 download -> 重新 downloadAndInstall
   * - 未知失败          -> 默认重新 check
   */
  async function retryLastAction(): Promise<void> {
    const action = lastFailedAction
    error.value = null
    if (action === 'download') {
      await downloadAndInstall()
    } else {
      await checkForUpdate()
    }
  }

  /**
   * 注册 Rust 侧推送的下载事件监听
   *
   * - `update://download-started`  一次,带 totalSize(可能为 null,服务器未返回 Content-Length)
   * - `update://download-progress` 多次,带累计 downloaded + total
   * - `update://download-finished` 一次,下载完成准备安装
   */
  async function setupEventListeners(): Promise<void> {
    // Tauri IPC 未注入(例如 Vitest/jsdom、浏览器开发预览)时 listen 会抛出
    // "Cannot read properties of undefined (reading 'transformCallback')".
    // 这里吞掉错误,保证 composable 在单测中也能被安全 mount。
    try {
      const started = await listen<{ totalSize: number | null }>(
        'update://download-started',
        (event) => {
          totalBytes.value = event.payload.totalSize
          downloadedBytes.value = 0
          downloadSpeed.value = 0
          lastBytesSampled = 0
          lastBytesSampledAt = Date.now()
        }
      )
      const progress = await listen<{ downloaded: number; total: number | null }>(
        'update://download-progress',
        (event) => {
          downloadedBytes.value = event.payload.downloaded
          if (event.payload.total !== null) {
            totalBytes.value = event.payload.total
          }
          // EMA 平滑的下载速度: 每隔 >= 250ms 采样一次,避免短间隔抖动
          const now = Date.now()
          if (lastBytesSampledAt > 0) {
            const dt = (now - lastBytesSampledAt) / 1000
            if (dt >= 0.25) {
              const instant = (downloadedBytes.value - lastBytesSampled) / dt
              if (instant >= 0) {
                downloadSpeed.value = downloadSpeed.value > 0
                  ? 0.7 * downloadSpeed.value + 0.3 * instant
                  : instant
              }
              lastBytesSampled = downloadedBytes.value
              lastBytesSampledAt = now
            }
          } else {
            lastBytesSampled = downloadedBytes.value
            lastBytesSampledAt = now
          }
        }
      )
      const finished = await listen('update://download-finished', () => {
        // 下载完成后立即将字节拉满,避免进度条停在 99%
        if (totalBytes.value) {
          downloadedBytes.value = totalBytes.value
        }
      })
      // 托盘菜单「检查更新」入口: Rust 端 emit 'tray-check-update' 时静默触发
      // 一次检查; 若发现新版本,上面 watch(status) 会弹 toast 引导用户进设置
      const trayCheck = await listen('tray-check-update', () => {
        console.warn('[useUpdate] 收到托盘「检查更新」事件')
        void checkForUpdate()
      })
      unlisteners.push(started, progress, finished, trayCheck)
    } catch (e) {
      console.warn('注册更新事件监听失败(非 Tauri 环境?):', e)
    }
  }

  /**
   * 初始化(模块级单例 + Promise 去重)
   *
   * 多个组件同时挂载时只会跑一次真正的 fetch + setup,后来者直接复用同一个
   * Promise。应用整个生命周期内不再卸载 listeners,保证 Rust 推送的进度事件
   * 始终能被前端收到,即使 SettingsPanel 在下载过程中被关闭。
   */
  function initialize(): Promise<void> {
    if (initPromise) {
      return initPromise
    }
    initPromise = (async () => {
      await fetchCurrentVersion()
      await fetchConfig()
      await setupEventListeners()

      // 只在启动时检查一次,不再做周期轮询,避免后台重复打扰用户。
      if (config.value.check_on_startup && config.value.auto_update_enabled) {
        setTimeout(() => {
          void checkForUpdate()
        }, 5000)
      }
    })().catch((e) => {
      console.error('useUpdate 初始化失败:', e)
      // 失败时重置 Promise,允许下次重试
      initPromise = null
      throw e
    })
    return initPromise
  }

  // 生命周期: 仅触发一次性初始化。不在 onUnmounted 释放 listeners/interval,
  // 让模块级状态在多次 mount/unmount 间持续有效。
  onMounted(() => {
    void initialize()
  })

  return {
    // 状态
    status,
    currentVersion,
    config,
    isLoading,
    error,
    downloadedBytes,
    totalBytes,
    downloadSpeed,
    latestDownloadUrl,

    // 计算属性
    isUpdateAvailable,
    isPendingRestart,
    isChecking,
    isDownloading,
    updateInfo,
    downloadProgress,
    downloadEtaSeconds,

    // 方法
    checkForUpdate,
    downloadAndInstall,
    restartApp,
    dismissUpdate,
    skipCurrentVersion,
    retryLastAction,
    getLatestUpdateDownloadUrl,
    saveConfig,
    fetchConfig,
    fetchCurrentVersion,
  }
}
