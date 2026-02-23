<template>
  <div class="file-search-section">
    <!-- Service Installation Banner (shown when service is not installed) -->
    <div v-if="!isServiceInstalled && !isLoadingStatus" class="install-banner">
      <div class="install-banner-content">
        <div class="install-icon">🔍</div>
        <div class="install-text">
          <h3>{{ $t('settings.fileSearch.installTitle') }}</h3>
          <p>{{ $t('settings.fileSearch.installDescription') }}</p>
        </div>
      </div>
      <div class="install-actions">
        <button
          class="install-btn"
          :disabled="isInstalling"
          @click="handleInstallService"
        >
          <span v-if="isInstalling" class="loading-spinner small"></span>
          <span v-else>{{ $t('settings.fileSearch.installButton') }}</span>
        </button>
      </div>
      <div v-if="installError" class="install-error">
        {{ installError }}
      </div>
    </div>

    <!-- Index Statistics Group -->
    <SettingsGroup :title="$t('settings.fileSearch.indexStatistics')">
      <!-- Service Status -->
      <SettingItem
        :label="$t('settings.fileSearch.serviceStatus')"
        :help-text="$t('settings.fileSearch.serviceStatusHelp')"
      >
        <div class="status-display">
          <span class="status-badge" :class="serviceStatusClass">
            {{ serviceStatusText }}
          </span>
          <button
            class="refresh-btn"
            :disabled="isLoadingStatus"
            :title="$t('settings.fileSearch.refreshStatus')"
            @click="handleRefreshStatus"
          >
            <span v-if="isLoadingStatus" class="loading-spinner small"></span>
            <span v-else class="refresh-icon">↻</span>
          </button>
          <!-- Start Service Button (shown when service is stopped) -->
          <button
            v-if="serviceStatus?.windowsServiceState === 'stopped'"
            class="start-service-btn"
            :disabled="isStartingService"
            @click="handleStartService"
          >
            <span v-if="isStartingService" class="loading-spinner small"></span>
            <span v-else>{{ $t('settings.fileSearch.startService') }}</span>
          </button>
        </div>
      </SettingItem>

      <!-- Indexed Files Count -->
      <SettingItem
        :label="$t('settings.fileSearch.indexedFiles')"
        :help-text="$t('settings.fileSearch.indexedFilesHelp')"
      >
        <div class="stat-value">
          <span v-if="isLoadingStatus" class="loading-text">--</span>
          <span v-else-if="serviceStatus?.indexedFiles != null">
            {{ $t('settings.fileSearch.filesCount', { count: formatNumber(serviceStatus.indexedFiles) }) }}
          </span>
          <span v-else class="stat-unavailable">--</span>
        </div>
      </SettingItem>

      <!-- Last Update Time -->
      <SettingItem
        :label="$t('settings.fileSearch.lastUpdate')"
        :help-text="$t('settings.fileSearch.lastUpdateHelp')"
      >
        <div class="stat-value">
          <span v-if="isLoadingStatus" class="loading-text">--</span>
          <span v-else-if="serviceStatus?.lastUpdate">
            {{ formatDateTime(serviceStatus.lastUpdate) }}
          </span>
          <span v-else class="stat-unavailable">{{ $t('settings.fileSearch.never') }}</span>
        </div>
      </SettingItem>

      <!-- Scan Progress (shown only when scanning) -->
      <SettingItem
        v-if="serviceStatus?.state === 'scanning'"
        :label="$t('settings.fileSearch.scanProgress', { progress: Math.round((serviceStatus.scanProgress || 0) * 100) })"
      >
        <div class="progress-bar-container">
          <div
            class="progress-bar"
            :style="{ width: `${(serviceStatus.scanProgress || 0) * 100}%` }"
          ></div>
        </div>
      </SettingItem>

      <!-- Rebuild Index Button -->
      <SettingItem
        :label="$t('settings.fileSearch.rebuildIndex')"
        :help-text="$t('settings.fileSearch.rebuildIndexHelp')"
      >
        <button
          class="rebuild-btn"
          :disabled="isRebuilding || !serviceStatus?.isAvailable"
          @click="handleRebuildIndex"
        >
          <span v-if="isRebuilding" class="loading-spinner small"></span>
          <span v-else>{{ $t('settings.fileSearch.rebuildIndex') }}</span>
        </button>
      </SettingItem>
    </SettingsGroup>

    <!-- Drive Selection Group -->
    <SettingsGroup :title="$t('settings.fileSearch.driveSelection')">
      <SettingItem
        :label="$t('settings.fileSearch.selectDrives')"
        :help-text="$t('settings.fileSearch.selectDrivesHelp')"
      >
        <div class="drive-list">
          <div v-if="isLoadingDrives" class="loading-indicator">
            <span class="loading-spinner"></span>
            <span>{{ $t('settings.fileSearch.loadingDrives') }}</span>
          </div>
          <div v-else-if="availableDrives.length === 0" class="no-drives">
            {{ $t('settings.fileSearch.noDrives') }}
          </div>
          <div v-else class="drive-checkboxes">
            <label
              v-for="drive in availableDrives"
              :key="drive.letter"
              class="drive-checkbox"
              :class="{ disabled: !drive.isNtfs }"
            >
              <input
                type="checkbox"
                :checked="selectedDrives.includes(drive.letter)"
                :disabled="!drive.isNtfs"
                @change="handleDriveToggle(drive.letter, ($event.target as HTMLInputElement).checked)"
              />
              <span class="drive-info">
                <span class="drive-letter">{{ drive.letter }}:</span>
                <span class="drive-label">{{ drive.label }}</span>
                <span class="drive-size">({{ formatSize(drive.totalSize) }})</span>
                <span v-if="!drive.isNtfs" class="drive-warning">
                  {{ $t('settings.fileSearch.notNtfs') }}
                </span>
              </span>
            </label>
          </div>
        </div>
      </SettingItem>
    </SettingsGroup>

    <!-- Exclude Folders Group -->
    <SettingsGroup :title="$t('settings.fileSearch.excludeFolders')">
      <SettingItem
        :label="$t('settings.fileSearch.excludedPaths')"
        :help-text="$t('settings.fileSearch.excludedPathsHelp')"
      >
        <div class="exclude-paths-container">
          <div class="exclude-paths-list">
            <div
              v-for="(path, index) in excludePaths"
              :key="index"
              class="exclude-path-item"
            >
              <span class="path-text" :title="path">{{ path }}</span>
              <button
                class="remove-btn"
                :aria-label="$t('settings.fileSearch.removePath')"
                @click="handleRemovePath(index)"
              >
                ×
              </button>
            </div>
            <div v-if="excludePaths.length === 0" class="no-paths">
              {{ $t('settings.fileSearch.noExcludedPaths') }}
            </div>
          </div>
          <button class="add-path-btn" @click="handleAddPath">
            {{ $t('settings.fileSearch.addFolder') }}
          </button>
        </div>
      </SettingItem>
    </SettingsGroup>

    <!-- Search Settings Group -->
    <SettingsGroup :title="$t('settings.fileSearch.searchSettings')">
      <!-- Result Limit -->
      <SettingItem
        :label="$t('settings.fileSearch.resultLimit')"
        :help-text="$t('settings.fileSearch.resultLimitHelp')"
      >
        <div class="result-limit-input">
          <input
            :value="resultLimit"
            type="number"
            class="setting-input"
            min="10"
            max="1000"
            step="10"
            @input="handleResultLimitChange"
          />
          <span class="limit-unit">{{ $t('settings.fileSearch.items') }}</span>
        </div>
      </SettingItem>

      <!-- Apply Configuration Button -->
      <SettingItem :label="$t('settings.fileSearch.applyConfig')">
        <button
          class="apply-btn"
          :disabled="isApplying || !hasChanges"
          @click="handleApplyConfig"
        >
          <span v-if="isApplying" class="loading-spinner small"></span>
          <span v-else>{{ $t('settings.fileSearch.apply') }}</span>
        </button>
      </SettingItem>
    </SettingsGroup>

    <!-- Service Management Group (shown when service is installed) -->
    <SettingsGroup v-if="isServiceInstalled" :title="$t('settings.fileSearch.serviceManagement')">
      <!-- Uninstall Service -->
      <SettingItem
        :label="$t('settings.fileSearch.uninstallService')"
        :help-text="$t('settings.fileSearch.uninstallServiceHelp')"
      >
        <div class="uninstall-controls">
          <label class="cleanup-checkbox">
            <input
              v-model="cleanupIndexOnUninstall"
              type="checkbox"
            />
            <span>{{ $t('settings.fileSearch.cleanupIndex') }}</span>
          </label>
          <button
            class="uninstall-btn"
            :disabled="isUninstalling"
            @click="handleUninstallService"
          >
            <span v-if="isUninstalling" class="loading-spinner small"></span>
            <span v-else>{{ $t('settings.fileSearch.uninstall') }}</span>
          </button>
        </div>
      </SettingItem>
      <div v-if="uninstallError" class="uninstall-error">
        {{ uninstallError }}
      </div>
    </SettingsGroup>
  </div>
</template>

<script setup lang="ts">
/**
 * FileSearchSection - File Search Settings Section
 *
 * Provides configuration options for the file search feature:
 * - Index statistics display (file count, last update time)
 * - Rebuild index button
 * - Drive selection (checkboxes for available NTFS drives)
 * - Exclude folders configuration (list with add/remove)
 * - Result limit configuration (number input)
 *
 * Uses the reusable settings control components:
 * - SettingsGroup for card-style grouping
 * - SettingItem for consistent row layout
 *
 * @validates Requirements 9.1, 9.2, 9.3, 9.4, 9.5, 9.7
 */

import { ref, computed, onMounted, onUnmounted } from 'vue'
import { useI18n } from 'vue-i18n'
import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'
import SettingsGroup from '@/components/settings/controls/SettingsGroup.vue'
import SettingItem from '@/components/settings/controls/SettingItem.vue'

// ============================================
// Types
// ============================================

interface DriveInfo {
  letter: string
  label: string
  fileSystem: string
  totalSize: number
  freeSpace: number
  isNtfs: boolean
}

/**
 * Service status response from Tauri backend
 * @validates Requirements 9.4
 */
interface ServiceStatusResponse {
  state: string
  windowsServiceState: string
  indexedFiles: number | null
  lastUpdate: string | null
  scanProgress: number | null
  scannedFiles: number | null
  isAvailable: boolean
  statusMessage: string
}

// ============================================
// Composables
// ============================================

const { t } = useI18n()

// ============================================
// State
// ============================================

/** Service status from backend */
const serviceStatus = ref<ServiceStatusResponse | null>(null)

/** Loading state for service status */
const isLoadingStatus = ref(false)

/** Rebuilding state */
const isRebuilding = ref(false)

/** Installing state */
const isInstalling = ref(false)

/** Starting service state */
const isStartingService = ref(false)

/** Install error message */
const installError = ref<string | null>(null)

/** Uninstalling state */
const isUninstalling = ref(false)

/** Uninstall error message */
const uninstallError = ref<string | null>(null)

/** Whether to cleanup index files on uninstall */
const cleanupIndexOnUninstall = ref(true)

/** Status refresh timer */
let statusRefreshTimer: ReturnType<typeof setInterval> | null = null

/** Available drives from system */
const availableDrives = ref<DriveInfo[]>([])

/** Loading state for drives */
const isLoadingDrives = ref(false)

/** Selected drives for indexing */
const selectedDrives = ref<string[]>(['C'])

/** Excluded paths */
const excludePaths = ref<string[]>([
  'C:\\$Recycle.Bin',
  'C:\\Windows\\Temp',
  'C:\\Windows\\SoftwareDistribution',
])

/** Result limit */
const resultLimit = ref(100)

/** Original values for change detection */
const originalSelectedDrives = ref<string[]>([])
const originalExcludePaths = ref<string[]>([])
const originalResultLimit = ref(100)

/** Applying state */
const isApplying = ref(false)

// ============================================
// Computed
// ============================================

/**
 * Check if the service is installed
 * @validates Requirements 1.1, 1.2
 */
const isServiceInstalled = computed(() => {
  if (!serviceStatus.value) return true // Assume installed while loading
  return serviceStatus.value.windowsServiceState !== 'not_installed'
})

/**
 * Check if there are unsaved changes
 */
const hasChanges = computed(() => {
  const drivesChanged = JSON.stringify(selectedDrives.value.sort()) !== 
    JSON.stringify(originalSelectedDrives.value.sort())
  const pathsChanged = JSON.stringify(excludePaths.value) !== 
    JSON.stringify(originalExcludePaths.value)
  const limitChanged = resultLimit.value !== originalResultLimit.value
  return drivesChanged || pathsChanged || limitChanged
})

/**
 * Get service status CSS class based on state
 * @validates Requirements 9.4
 */
const serviceStatusClass = computed(() => {
  if (!serviceStatus.value) return 'status-unknown'
  
  switch (serviceStatus.value.state) {
    case 'running':
      return 'status-running'
    case 'scanning':
      return 'status-scanning'
    case 'starting':
      return 'status-starting'
    case 'stopped':
      return 'status-stopped'
    default:
      return 'status-unknown'
  }
})

/**
 * Get localized service status text
 * @validates Requirements 9.4
 */
const serviceStatusText = computed(() => {
  if (!serviceStatus.value) return t('settings.fileSearch.statusUnknown')
  
  switch (serviceStatus.value.state) {
    case 'running':
      return t('settings.fileSearch.statusRunning')
    case 'scanning':
      return t('settings.fileSearch.statusScanning')
    case 'starting':
      return t('settings.fileSearch.statusStarting')
    case 'stopped':
      if (serviceStatus.value.windowsServiceState === 'not_installed') {
        return t('settings.fileSearch.statusNotInstalled')
      }
      return t('settings.fileSearch.statusStopped')
    default:
      return t('settings.fileSearch.statusUnknown')
  }
})

// ============================================
// Lifecycle
// ============================================

onMounted(async () => {
  // Load service status immediately
  await loadServiceStatus()
  
  // Start periodic status refresh (every 10 seconds)
  statusRefreshTimer = setInterval(loadServiceStatus, 10000)
  
  await loadAvailableDrives()
  // Store original values for change detection
  originalSelectedDrives.value = [...selectedDrives.value]
  originalExcludePaths.value = [...excludePaths.value]
  originalResultLimit.value = resultLimit.value
})

onUnmounted(() => {
  // Clean up timer to prevent memory leaks
  if (statusRefreshTimer) {
    clearInterval(statusRefreshTimer)
    statusRefreshTimer = null
  }
})

// ============================================
// Methods
// ============================================

/**
 * Load service status from backend
 * @validates Requirements 9.4
 */
async function loadServiceStatus(): Promise<void> {
  // Don't show loading indicator for background refresh
  const isInitialLoad = serviceStatus.value === null
  if (isInitialLoad) {
    isLoadingStatus.value = true
  }
  
  try {
    const status = await invoke<ServiceStatusResponse>('get_search_service_status')
    serviceStatus.value = status
  } catch (error) {
    console.error('Failed to load service status:', error)
    // Keep previous status on error, or set to unavailable
    if (!serviceStatus.value) {
      serviceStatus.value = {
        state: 'stopped',
        windowsServiceState: 'unknown',
        indexedFiles: null,
        lastUpdate: null,
        scanProgress: null,
        scannedFiles: null,
        isAvailable: false,
        statusMessage: String(error),
      }
    }
  } finally {
    isLoadingStatus.value = false
  }
}

/**
 * Handle manual status refresh
 * @validates Requirements 9.4
 */
async function handleRefreshStatus(): Promise<void> {
  isLoadingStatus.value = true
  await loadServiceStatus()
}

/**
 * Handle rebuild index button click
 * @validates Requirements 9.5
 */
async function handleRebuildIndex(): Promise<void> {
  if (isRebuilding.value) return
  
  isRebuilding.value = true
  try {
    await invoke('rebuild_search_index')
    console.log('Index rebuild request sent successfully')
    // Refresh status to show scanning progress
    await loadServiceStatus()
  } catch (error) {
    console.error('Failed to rebuild index:', error)
  } finally {
    isRebuilding.value = false
  }
}

/**
 * Handle install service button click
 * @validates Requirements 1.1, 1.2
 */
async function handleInstallService(): Promise<void> {
  if (isInstalling.value) return
  
  isInstalling.value = true
  installError.value = null
  
  try {
    const result = await invoke<{ success: boolean; message: string; needsRestart: boolean }>('install_file_search_service')
    
    if (result.success) {
      console.log('Service installed successfully:', result.message)
      // Refresh status to show the new state
      await loadServiceStatus()
    } else {
      installError.value = result.message
    }
  } catch (error) {
    console.error('Failed to install service:', error)
    installError.value = String(error)
  } finally {
    isInstalling.value = false
  }
}

/**
 * Handle uninstall service button click
 * @validates Requirements 1.7
 */
async function handleUninstallService(): Promise<void> {
  if (isUninstalling.value) return
  
  isUninstalling.value = true
  uninstallError.value = null
  
  try {
    const result = await invoke<{ success: boolean; message: string; needsRestart: boolean }>(
      'uninstall_file_search_service',
      { cleanupIndex: cleanupIndexOnUninstall.value }
    )
    
    if (result.success) {
      console.log('Service uninstalled successfully:', result.message)
      // Refresh status to show the new state
      await loadServiceStatus()
    } else {
      uninstallError.value = result.message
    }
  } catch (error) {
    console.error('Failed to uninstall service:', error)
    uninstallError.value = String(error)
  } finally {
    isUninstalling.value = false
  }
}

/**
 * Handle start service button click
 * @validates Requirements 1.5
 */
async function handleStartService(): Promise<void> {
  if (isStartingService.value) return
  
  isStartingService.value = true
  
  try {
    await invoke('start_search_service')
    console.log('Service start request sent successfully')
    // Refresh status to show the new state
    await loadServiceStatus()
  } catch (error) {
    console.error('Failed to start service:', error)
  } finally {
    isStartingService.value = false
  }
}

/**
 * Format number with thousand separators
 */
function formatNumber(num: number): string {
  return num.toLocaleString()
}

/**
 * Format ISO date string to localized datetime
 */
function formatDateTime(isoString: string): string {
  try {
    const date = new Date(isoString)
    return date.toLocaleString()
  } catch {
    return isoString
  }
}

/**
 * Load available drives from system
 * @validates Requirements 9.2
 */
async function loadAvailableDrives(): Promise<void> {
  isLoadingDrives.value = true
  try {
    const drives = await invoke<DriveInfo[]>('get_available_drives')
    availableDrives.value = drives
    
    // Auto-select all NTFS drives if none selected
    if (selectedDrives.value.length === 0) {
      selectedDrives.value = drives
        .filter(d => d.isNtfs)
        .map(d => d.letter)
    }
  } catch (error) {
    console.error('Failed to load drives:', error)
    availableDrives.value = []
  } finally {
    isLoadingDrives.value = false
  }
}

/**
 * Format file size to human readable string
 */
function formatSize(bytes: number): string {
  if (bytes === 0) return '0 B'
  const k = 1024
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB']
  const i = Math.floor(Math.log(bytes) / Math.log(k))
  return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i]
}

// ============================================
// Event Handlers
// ============================================

/**
 * Handle drive checkbox toggle
 * @param letter - Drive letter
 * @param checked - Whether the drive is selected
 * @validates Requirements 9.2
 */
function handleDriveToggle(letter: string, checked: boolean): void {
  if (checked) {
    if (!selectedDrives.value.includes(letter)) {
      selectedDrives.value = [...selectedDrives.value, letter]
    }
  } else {
    selectedDrives.value = selectedDrives.value.filter(d => d !== letter)
  }
}

/**
 * Handle add exclude path button click
 * @validates Requirements 9.3
 */
async function handleAddPath(): Promise<void> {
  const selected = await open({
    directory: true,
    multiple: false,
    title: '选择要排除的文件夹',
  })

  if (selected && typeof selected === 'string') {
    if (!excludePaths.value.includes(selected)) {
      excludePaths.value = [...excludePaths.value, selected]
    }
  }
}

/**
 * Handle remove exclude path
 * @param index - Index of path to remove
 * @validates Requirements 9.3
 */
function handleRemovePath(index: number): void {
  excludePaths.value = excludePaths.value.filter((_, i) => i !== index)
}

/**
 * Handle result limit change
 * @param event - Input event
 * @validates Requirements 9.7
 */
function handleResultLimitChange(event: Event): void {
  const target = event.target as HTMLInputElement
  const value = parseInt(target.value, 10)
  if (!isNaN(value) && value >= 10 && value <= 1000) {
    resultLimit.value = value
  }
}

/**
 * Apply configuration to the index service
 * @validates Requirements 9.6
 */
async function handleApplyConfig(): Promise<void> {
  if (isApplying.value || !hasChanges.value) return
  
  isApplying.value = true
  try {
    await invoke('update_search_config', {
      config: {
        volumes: selectedDrives.value,
        excludePaths: excludePaths.value,
        resultLimit: resultLimit.value,
      },
    })
    
    // Update original values after successful apply
    originalSelectedDrives.value = [...selectedDrives.value]
    originalExcludePaths.value = [...excludePaths.value]
    originalResultLimit.value = resultLimit.value
    
    console.log('File search configuration applied successfully')
  } catch (error) {
    console.error('Failed to apply configuration:', error)
  } finally {
    isApplying.value = false
  }
}
</script>

<style scoped>
.file-search-section {
  display: flex;
  flex-direction: column;
  gap: 24px;
}

/* Service Installation Banner Styles */
.install-banner {
  display: flex;
  flex-direction: column;
  gap: 16px;
  padding: 20px;
  background: linear-gradient(135deg, rgba(66, 133, 244, 0.15), rgba(52, 168, 83, 0.1));
  border: 1px solid rgba(66, 133, 244, 0.3);
  border-radius: 12px;
}

.install-banner-content {
  display: flex;
  align-items: flex-start;
  gap: 16px;
}

.install-icon {
  font-size: 32px;
  line-height: 1;
}

.install-text h3 {
  margin: 0 0 8px 0;
  font-size: 16px;
  font-weight: 600;
  color: rgba(255, 255, 255, 0.95);
}

.install-text p {
  margin: 0;
  font-size: 13px;
  color: rgba(255, 255, 255, 0.7);
  line-height: 1.5;
}

.install-actions {
  display: flex;
  gap: 12px;
}

.install-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 10px 24px;
  border: none;
  border-radius: 6px;
  background: linear-gradient(135deg, #4285f4, #34a853);
  color: white;
  font-size: 14px;
  font-weight: 600;
  cursor: pointer;
  transition: all 0.2s;
  box-shadow: 0 2px 8px rgba(66, 133, 244, 0.3);
}

.install-btn:hover:not(:disabled) {
  transform: translateY(-1px);
  box-shadow: 0 4px 12px rgba(66, 133, 244, 0.4);
}

.install-btn:active:not(:disabled) {
  transform: translateY(0);
}

.install-btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.install-error {
  padding: 10px 14px;
  background: rgba(244, 67, 54, 0.15);
  border: 1px solid rgba(244, 67, 54, 0.3);
  border-radius: 6px;
  color: #f44336;
  font-size: 13px;
}

/* Start Service Button */
.start-service-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 4px 12px;
  border: 1px solid rgba(76, 175, 80, 0.5);
  border-radius: 4px;
  background: rgba(76, 175, 80, 0.1);
  color: #4caf50;
  font-size: 12px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.1s;
}

.start-service-btn:hover:not(:disabled) {
  background: rgba(76, 175, 80, 0.2);
  border-color: #4caf50;
}

.start-service-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* Drive Selection Styles */
.drive-list {
  width: 100%;
}

.loading-indicator {
  display: flex;
  align-items: center;
  gap: 8px;
  color: rgba(255, 255, 255, 0.6);
  font-size: 13px;
}

.loading-spinner {
  width: 16px;
  height: 16px;
  border: 2px solid rgba(255, 255, 255, 0.2);
  border-top-color: #4285f4;
  border-radius: 50%;
  animation: spin 0.8s linear infinite;
}

.loading-spinner.small {
  width: 12px;
  height: 12px;
  border-width: 1.5px;
}

@keyframes spin {
  to {
    transform: rotate(360deg);
  }
}

.no-drives {
  color: rgba(255, 255, 255, 0.5);
  font-size: 13px;
  font-style: italic;
}

.drive-checkboxes {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.drive-checkbox {
  display: flex;
  align-items: center;
  gap: 8px;
  cursor: pointer;
  padding: 8px 12px;
  border-radius: 4px;
  background: rgba(255, 255, 255, 0.03);
  transition: background-color 0.1s;
}

.drive-checkbox:hover:not(.disabled) {
  background: rgba(255, 255, 255, 0.06);
}

.drive-checkbox.disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.drive-checkbox input[type="checkbox"] {
  width: 16px;
  height: 16px;
  accent-color: #4285f4;
  cursor: inherit;
}

.drive-info {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
}

.drive-letter {
  font-weight: 600;
  color: rgba(255, 255, 255, 0.9);
}

.drive-label {
  color: rgba(255, 255, 255, 0.7);
}

.drive-size {
  color: rgba(255, 255, 255, 0.5);
  font-size: 12px;
}

.drive-warning {
  color: #ff9800;
  font-size: 11px;
  margin-left: 4px;
}

/* Exclude Paths Styles */
.exclude-paths-container {
  display: flex;
  flex-direction: column;
  gap: 12px;
  width: 100%;
}

.exclude-paths-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  max-height: 200px;
  overflow-y: auto;
  padding: 8px;
  background: rgba(0, 0, 0, 0.2);
  border-radius: 4px;
}

.exclude-path-item {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 6px 8px;
  background: rgba(255, 255, 255, 0.05);
  border-radius: 4px;
}

.path-text {
  flex: 1;
  font-size: 12px;
  color: rgba(255, 255, 255, 0.8);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.remove-btn {
  width: 20px;
  height: 20px;
  padding: 0;
  border: none;
  border-radius: 50%;
  background: rgba(255, 255, 255, 0.1);
  color: rgba(255, 255, 255, 0.6);
  font-size: 14px;
  line-height: 1;
  cursor: pointer;
  transition: all 0.1s;
}

.remove-btn:hover {
  background: rgba(255, 82, 82, 0.3);
  color: #ff5252;
}

.no-paths {
  color: rgba(255, 255, 255, 0.4);
  font-size: 12px;
  font-style: italic;
  text-align: center;
  padding: 12px;
}

.add-path-btn {
  padding: 8px 16px;
  border: 1px dashed rgba(255, 255, 255, 0.2);
  border-radius: 4px;
  background: transparent;
  color: rgba(255, 255, 255, 0.7);
  font-size: 13px;
  cursor: pointer;
  transition: all 0.1s;
}

.add-path-btn:hover {
  border-color: #4285f4;
  color: #4285f4;
  background: rgba(66, 133, 244, 0.1);
}

/* Result Limit Styles */
.result-limit-input {
  display: flex;
  align-items: center;
  gap: 8px;
}

.setting-input {
  width: 100px;
  padding: 6px 10px;
  border: 1px solid rgba(255, 255, 255, 0.1);
  border-radius: 4px;
  background: rgba(255, 255, 255, 0.05);
  color: rgba(255, 255, 255, 0.9);
  font-size: 13px;
}

.setting-input:focus {
  outline: none;
  border-color: #4285f4;
}

.limit-unit {
  color: rgba(255, 255, 255, 0.5);
  font-size: 13px;
}

/* Apply Button Styles */
.apply-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  min-width: 80px;
  padding: 8px 20px;
  border: none;
  border-radius: 4px;
  background: #4285f4;
  color: white;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.1s;
}

.apply-btn:hover:not(:disabled) {
  background: #3367d6;
}

.apply-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* Index Statistics Styles */
.status-display {
  display: flex;
  align-items: center;
  gap: 8px;
}

.status-badge {
  display: inline-flex;
  align-items: center;
  padding: 4px 12px;
  border-radius: 12px;
  font-size: 12px;
  font-weight: 500;
}

.status-badge.status-running {
  background: rgba(76, 175, 80, 0.2);
  color: #4caf50;
}

.status-badge.status-scanning {
  background: rgba(33, 150, 243, 0.2);
  color: #2196f3;
}

.status-badge.status-starting {
  background: rgba(255, 193, 7, 0.2);
  color: #ffc107;
}

.status-badge.status-stopped {
  background: rgba(158, 158, 158, 0.2);
  color: #9e9e9e;
}

.status-badge.status-unknown {
  background: rgba(158, 158, 158, 0.2);
  color: #9e9e9e;
}

.refresh-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 28px;
  height: 28px;
  padding: 0;
  border: none;
  border-radius: 50%;
  background: rgba(255, 255, 255, 0.05);
  color: rgba(255, 255, 255, 0.6);
  cursor: pointer;
  transition: all 0.1s;
}

.refresh-btn:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.1);
  color: rgba(255, 255, 255, 0.9);
}

.refresh-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.refresh-icon {
  font-size: 16px;
}

.stat-value {
  font-size: 13px;
  color: rgba(255, 255, 255, 0.9);
}

.stat-unavailable {
  color: rgba(255, 255, 255, 0.4);
}

.loading-text {
  color: rgba(255, 255, 255, 0.4);
}

/* Progress Bar Styles */
.progress-bar-container {
  width: 100%;
  max-width: 200px;
  height: 6px;
  background: rgba(255, 255, 255, 0.1);
  border-radius: 3px;
  overflow: hidden;
}

.progress-bar {
  height: 100%;
  background: linear-gradient(90deg, #4285f4, #34a853);
  border-radius: 3px;
  transition: width 0.3s ease;
}

/* Rebuild Button Styles */
.rebuild-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  min-width: 100px;
  padding: 8px 16px;
  border: 1px solid rgba(255, 152, 0, 0.5);
  border-radius: 4px;
  background: rgba(255, 152, 0, 0.1);
  color: #ff9800;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.1s;
}

.rebuild-btn:hover:not(:disabled) {
  background: rgba(255, 152, 0, 0.2);
  border-color: #ff9800;
}

.rebuild-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

/* Uninstall Controls Styles */
.uninstall-controls {
  display: flex;
  align-items: center;
  gap: 16px;
}

.cleanup-checkbox {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 12px;
  color: rgba(255, 255, 255, 0.7);
  cursor: pointer;
}

.cleanup-checkbox input[type="checkbox"] {
  width: 14px;
  height: 14px;
  accent-color: #4285f4;
  cursor: pointer;
}

.uninstall-btn {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  min-width: 80px;
  padding: 8px 16px;
  border: 1px solid rgba(244, 67, 54, 0.5);
  border-radius: 4px;
  background: rgba(244, 67, 54, 0.1);
  color: #f44336;
  font-size: 13px;
  font-weight: 500;
  cursor: pointer;
  transition: all 0.1s;
}

.uninstall-btn:hover:not(:disabled) {
  background: rgba(244, 67, 54, 0.2);
  border-color: #f44336;
}

.uninstall-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.uninstall-error {
  padding: 10px 14px;
  background: rgba(244, 67, 54, 0.15);
  border: 1px solid rgba(244, 67, 54, 0.3);
  border-radius: 6px;
  color: #f44336;
  font-size: 13px;
  margin-top: 8px;
}
</style>
