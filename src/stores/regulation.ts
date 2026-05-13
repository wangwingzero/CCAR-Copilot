/**
 * 规章查询全局状态 Store
 *
 * 管理扫描、OCR 等长时间运行任务的状态。
 * 使用 Pinia 全局 store 确保在组件切换时状态不丢失。
 */

import { defineStore } from 'pinia'
import { ref } from 'vue'
import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  RegulationScanProgress,
  RegulationScanResponse,
  RegulationSyncCompareResponse,
} from '@/types'

export const useRegulationStore = defineStore('regulation', () => {
  // ============================================
  // 扫描状态（全局持久）
  // ============================================

  /** 是否正在扫描 */
  const isScanning = ref(false)

  /** 扫描进度 */
  const scanProgress = ref<RegulationScanProgress | null>(null)

  /** 扫描结果（扫描完成后设置） */
  const scanResult = ref<RegulationScanResponse | null>(null)

  /** 扫描错误 */
  const scanError = ref<string | null>(null)

  /** 扫描进度事件监听器清理函数 */
  let scanProgressUnlisten: UnlistenFn | null = null

  // ============================================
  // OCR 状态（全局持久）
  // ============================================

  /** 是否正在 OCR 处理 */
  const isOcrProcessing = ref(false)

  /** OCR 进度文本 */
  const ocrProgressText = ref('')

  // ============================================
  // 同步对比状态（全局持久）
  // ============================================

  /** 是否正在同步 */
  const isSyncing = ref(false)

  /** 同步结果 */
  const syncResult = ref<RegulationSyncCompareResponse | null>(null)

  // ============================================
  // 数据库状态（全局持久）
  // ============================================

  /** 数据库同步状态 */
  const dbSyncStatus = ref<{
    total_files: number
    pending_ocr: number
    processing_ocr: number
    done_ocr: number
    failed_ocr: number
    indexed: number
  } | null>(null)

  // ============================================
  // Methods
  // ============================================

  /**
   * 开始扫描本地目录
   *
   * 管理扫描生命周期：注册事件监听、调用后端命令、清理资源。
   * 由于状态在 Pinia store 中，组件切换不会影响扫描进度显示。
   */
  async function startScan(
    dirPath: string,
    recursive: boolean = true,
    initIndexFn?: () => Promise<void>,
  ): Promise<RegulationScanResponse | null> {
    if (isScanning.value) {
      return null
    }

    try {
      isScanning.value = true
      scanProgress.value = null
      scanResult.value = null
      scanError.value = null

      // 确保索引已初始化
      if (initIndexFn) {
        await initIndexFn()
      }

      // 注册扫描进度事件监听（全局级别）
      scanProgressUnlisten = await listen<RegulationScanProgress>(
        'regulation:scan-progress',
        (event) => {
          scanProgress.value = event.payload
        },
      )

      // 调用 Rust 扫描命令
      const result = await invoke<RegulationScanResponse>(
        'regulation_scan_local_dir',
        {
          dirPath,
          recursive,
          localCopyMode: 'register_only',
          targetDir: dirPath,
        },
      )

      scanResult.value = result

      console.warn(
        `[RegulationStore] 扫描完成: 发现 ${result.total_found}, 新增 ${result.new_files}, 重复 ${result.duplicates}, 索引 ${result.indexed}`,
      )

      return result
    } catch (err) {
      scanError.value = err instanceof Error ? err.message : String(err)
      console.error('[RegulationStore] 扫描失败:', err)
      return null
    } finally {
      isScanning.value = false
      // 清理事件监听
      if (scanProgressUnlisten) {
        scanProgressUnlisten()
        scanProgressUnlisten = null
      }
    }
  }

  /**
   * 开始全盘扫描所有 PDF / TXT 文件
   *
   * 遍历 Windows 所有盘符，递归收集 PDF / TXT 并入库索引。
   */
  async function startFullScan(
    initIndexFn?: () => Promise<void>,
  ): Promise<RegulationScanResponse | null> {
    if (isScanning.value) {
      return null
    }

    try {
      isScanning.value = true
      scanProgress.value = null
      scanResult.value = null
      scanError.value = null

      if (initIndexFn) {
        await initIndexFn()
      }

      scanProgressUnlisten = await listen<RegulationScanProgress>(
        'regulation:scan-progress',
        (event) => {
          scanProgress.value = event.payload
        },
      )

      const result = await invoke<RegulationScanResponse>(
        'regulation_scan_all_drives',
        { autoOcr: true },
      )

      scanResult.value = result
      return result
    } catch (err) {
      scanError.value = err instanceof Error ? err.message : String(err)
      console.error('[RegulationStore] 全盘扫描失败:', err)
      return null
    } finally {
      isScanning.value = false
      if (scanProgressUnlisten) {
        scanProgressUnlisten()
        scanProgressUnlisten = null
      }
    }
  }

  /**
   * 刷新数据库同步状态
   */
  async function refreshDbStatus(scanFolders: string[] = []): Promise<void> {
    try {
      const status = await invoke<{
        total_files: number
        pending_ocr: number
        processing_ocr: number
        done_ocr: number
        failed_ocr: number
        indexed: number
      }>('regulation_get_sync_status', {
        scanFolders,
      })

      dbSyncStatus.value = status
    } catch (err) {
      console.warn('[RegulationStore] 获取同步状态失败:', err)
    }
  }

  /**
   * 清除扫描结果
   */
  function clearScanResult(): void {
    scanResult.value = null
    scanError.value = null
  }

  // ============================================
  // 同步对比状态管理（封装，避免外部直接修改）
  // ============================================

  /**
   * 开始同步对比
   */
  function startSyncCompare(): void {
    isSyncing.value = true
    syncResult.value = null
  }

  /**
   * 完成同步对比
   */
  function finishSyncCompare(result: RegulationSyncCompareResponse | null): void {
    syncResult.value = result
    isSyncing.value = false
  }

  return {
    // 扫描状态（只读）
    isScanning,
    scanProgress,
    scanResult,
    scanError,

    // OCR 状态
    isOcrProcessing,
    ocrProgressText,

    // 同步状态（只读）
    isSyncing,
    syncResult,

    // 数据库状态
    dbSyncStatus,

    // Methods
    startScan,
    startFullScan,
    startSyncCompare,
    finishSyncCompare,
    refreshDbStatus,
    clearScanResult,
  }
})
