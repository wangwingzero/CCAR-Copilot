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

import { ref, computed, onMounted, onUnmounted } from 'vue'
import { invoke } from '@tauri-apps/api/core'

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
export function useUpdate() {
  // 状态
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

  // 定时检查更新的定时器
  let checkInterval: ReturnType<typeof setInterval> | null = null

  // 计算属性
  const isUpdateAvailable = computed(() => status.value.status === 'Available')
  const isPendingRestart = computed(() => status.value.status === 'PendingRestart')
  const isChecking = computed(() => status.value.status === 'Checking')
  const isDownloading = computed(() => status.value.status === 'Downloading')
  const updateInfo = computed(() => status.value.info)
  const downloadProgress = computed(() => status.value.progress ?? 0)

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

      // 重新设置定时检查
      setupAutoCheck()
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

    try {
      const result = await invoke<UpdateStatus>('check_for_update')
      status.value = result

      // 如果有更新且配置了自动下载，则自动开始下载
      if (result.status === 'Available' && config.value.auto_download) {
        // 延迟一下再开始下载，让用户看到更新可用的状态
        setTimeout(() => {
          downloadAndInstall().catch((e) => {
            console.error('自动下载更新失败:', e)
          })
        }, 1000)
      }

      return result
    } catch (e) {
      console.error('检查更新失败:', e)
      error.value = String(e)
      status.value = { status: 'Error', message: String(e) }
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
    isLoading.value = true
    error.value = null
    status.value = { status: 'Downloading', progress: 0 }

    try {
      const result = await invoke<UpdateStatus>('download_and_install_update')
      status.value = result

      // 如果配置了自动安装且更新已准备好，则自动重启
      if (result.status === 'PendingRestart' && config.value.auto_install) {
        setTimeout(() => {
          restartApp().catch((e) => {
            console.error('自动重启失败:', e)
          })
        }, 3000)
      }

      return result
    } catch (e) {
      console.error('下载更新失败:', e)
      error.value = String(e)
      status.value = { status: 'Error', message: String(e) }
      return status.value
    } finally {
      isLoading.value = false
    }
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
   * 忽略当前更新
   */
  function dismissUpdate(): void {
    status.value = { status: 'Idle' }
  }

  /**
   * 设置自动检查更新
   */
  function setupAutoCheck(): void {
    // 清除现有的定时器
    if (checkInterval) {
      clearInterval(checkInterval)
      checkInterval = null
    }

    // 如果启用了自动更新，设置定时检查
    if (config.value.auto_update_enabled && config.value.check_interval_hours > 0) {
      const intervalMs = config.value.check_interval_hours * 60 * 60 * 1000
      checkInterval = setInterval(() => {
        checkForUpdate()
      }, intervalMs)
    }
  }

  /**
   * 初始化
   */
  async function initialize(): Promise<void> {
    await fetchCurrentVersion()
    await fetchConfig()

    // 设置自动检查
    setupAutoCheck()

    // 如果配置了启动时检查，则检查更新
    if (config.value.check_on_startup && config.value.auto_update_enabled) {
      // 延迟几秒再检查，避免影响启动速度
      setTimeout(() => {
        checkForUpdate()
      }, 5000)
    }
  }

  // 生命周期
  onMounted(() => {
    initialize()
  })

  onUnmounted(() => {
    if (checkInterval) {
      clearInterval(checkInterval)
      checkInterval = null
    }
  })

  return {
    // 状态
    status,
    currentVersion,
    config,
    isLoading,
    error,

    // 计算属性
    isUpdateAvailable,
    isPendingRestart,
    isChecking,
    isDownloading,
    updateInfo,
    downloadProgress,

    // 方法
    checkForUpdate,
    downloadAndInstall,
    restartApp,
    dismissUpdate,
    saveConfig,
    fetchConfig,
    fetchCurrentVersion,
  }
}
