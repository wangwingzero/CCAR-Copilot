/**
 * Property-Based Tests for Update Settings
 *
 * Feature: settings-enhancement
 *
 * This file tests the update settings behavior:
 * - Property 11: Update interval controls are not shown
 * - Property 12: Update proxy controls are not shown
 *
 * **Validates: Requirements 8.3, 8.5**
 */

import { describe, it, expect, beforeEach, vi } from 'vitest'
import { flushPromises, mount, VueWrapper } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { createI18n } from 'vue-i18n'
import * as fc from 'fast-check'
import { useSettingsStore } from '@/stores/settings'
import UpdateSection from '../../sections/UpdateSection.vue'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
  open: vi.fn(),
}))

vi.mock('@tauri-apps/api/core', () => ({
  invoke: mocks.invoke,
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: mocks.listen,
}))

vi.mock('@tauri-apps/plugin-shell', () => ({
  open: mocks.open,
}))

// ============================================================================
// Test Setup
// ============================================================================

/**
 * Create i18n instance for testing
 */
function createTestI18n() {
  return createI18n({
    legacy: false,
    locale: 'en',
    messages: {
      en: {
        settings: {
          update: {
            title: 'Update',
            settings: 'Auto-Check & Proxy',
            currentVersion: 'Current Version',
            autoCheck: 'Auto Check',
            autoCheckHelp: 'Automatically check for updates',
            checkInterval: 'Check Interval',
            checkIntervalHelp: 'How often to check for updates',
            useProxy: 'Use Proxy',
            useProxyHelp: 'Use proxy for update checks',
            proxyUrl: 'Proxy URL',
            proxyUrlHelp: 'Proxy server URL',
            checkNow: 'Check Now',
            checkNowBtn: 'Check',
            checking: 'Checking...',
            lastCheck: 'Last Check',
            manualDownload: 'Manual Update',
            manualDownloadHelp: 'Open the latest Windows installer in your browser',
            openLatestInstaller: 'Open Installer',
            openingLatestInstaller: 'Opening...',
            manualDownloadError: 'Failed to open installer URL: {message}',
            // v0.1.6 新增 - 同步测试 i18n fixture,避免 intlify warning
            statusError: 'Failed to check for updates',
            statusUpToDate: 'You are up to date',
            statusAvailable: 'Update available',
            statusDownloading: 'Downloading update...',
            statusInstalling: 'Installing update...',
            statusPendingRestart: 'Update downloaded, restart to apply',
            downloadAndInstall: 'Download and Install',
            restartNow: 'Restart Now',
            releaseDate: 'Release Date',
            retry: 'Retry',
            skipThisVersion: 'Skip this version',
            currentToNew: 'Current {current} → New {next}',
            downloadSpeed: 'Speed',
            downloadEta: 'ETA',
            etaSeconds: '{n}s',
            etaMinutes: '{n} min',
            etaHours: '{n} h',
            toastNewVersion: 'New version {version} available',
            toastDownloaded: '{version} downloaded',
            toastError: 'Update failed: {message}',
          }
        }
      }
    }
  })
}

/**
 * Mount UpdateSection with test configuration
 */
function mountUpdateSection(): VueWrapper {
  const i18n = createTestI18n()
  const pinia = createPinia()
  setActivePinia(pinia)
  
  return mount(UpdateSection, {
    global: {
      plugins: [i18n, pinia],
    },
  })
}

// ============================================================================
// Property 11: Update Interval Controls Removed
// ============================================================================

describe('Feature: settings-enhancement, Property 11: Update Interval Controls Removed', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    mocks.invoke.mockReset()
    mocks.invoke.mockImplementation((command: string) => {
      if (command === 'get_current_version') return Promise.resolve('0.1.9')
      if (command === 'get_update_config') {
        return Promise.resolve({
          auto_update_enabled: true,
          check_interval_hours: 24,
          check_on_startup: true,
          auto_download: true,
          auto_install: false,
        })
      }
      if (command === 'get_latest_update_download_url') {
        return Promise.resolve(
          'https://ccar-update.031986.xyz/downloads/CCAR%20Copilot_0.1.9_x64-setup.exe'
        )
      }
      return Promise.resolve(undefined)
    })
    mocks.listen.mockReset()
    mocks.listen.mockResolvedValue(vi.fn())
    mocks.open.mockReset()
    mocks.open.mockResolvedValue(undefined)
  })

  it('does not render update interval controls', () => {
    const wrapper = mountUpdateSection()

    expect(wrapper.text()).not.toContain('Check Interval')

    wrapper.unmount()
  })

  it('preserves hidden check interval value when updating visible settings', () => {
    fc.assert(
      fc.property(
        fc.integer({ min: 1, max: 168 }),
        fc.boolean(),
        (interval: number, autoCheck: boolean) => {
          const store = useSettingsStore()
          
          // Set initial interval
          store.updateUpdate({ checkIntervalHours: interval })

          // Update other settings
          store.updateUpdate({ autoCheck })
          
          // Verify interval is preserved
          expect(store.update.checkIntervalHours).toBe(interval)
        }
      ),
      { numRuns: 100, verbose: true }
    )
  })
})

// ============================================================================
// Property 12: Update Proxy Controls Removed
// ============================================================================

describe('Feature: settings-enhancement, Property 12: Update Proxy Controls Removed', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    mocks.invoke.mockReset()
    mocks.invoke.mockImplementation((command: string) => {
      if (command === 'get_current_version') return Promise.resolve('0.1.9')
      if (command === 'get_update_config') {
        return Promise.resolve({
          auto_update_enabled: true,
          check_interval_hours: 24,
          check_on_startup: true,
          auto_download: true,
          auto_install: false,
        })
      }
      if (command === 'get_latest_update_download_url') {
        return Promise.resolve(
          'https://ccar-update.031986.xyz/downloads/CCAR%20Copilot_0.1.9_x64-setup.exe'
        )
      }
      return Promise.resolve(undefined)
    })
    mocks.listen.mockReset()
    mocks.listen.mockResolvedValue(vi.fn())
    mocks.open.mockReset()
    mocks.open.mockResolvedValue(undefined)
  })

  it('does not render update proxy controls', () => {
    const wrapper = mountUpdateSection()

    expect(wrapper.text()).not.toContain('Use Proxy')
    expect(wrapper.text()).not.toContain('Proxy URL')

    wrapper.unmount()
  })

  it('opens the latest installer link in the browser', async () => {
    const wrapper = mountUpdateSection()

    await wrapper.find('[data-testid="open-latest-installer"]').trigger('click')
    await flushPromises()

    expect(mocks.invoke).toHaveBeenCalledWith('get_latest_update_download_url')
    expect(mocks.open).toHaveBeenCalledWith(
      'https://ccar-update.031986.xyz/downloads/CCAR%20Copilot_0.1.9_x64-setup.exe'
    )

    wrapper.unmount()
  })
})
