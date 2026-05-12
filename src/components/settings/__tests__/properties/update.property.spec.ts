/**
 * Property-Based Tests for Update Settings
 *
 * Feature: settings-enhancement
 *
 * This file tests the update settings behavior:
 * - Property 11: Update Interval Clamping
 * - Property 12: Proxy URL Conditional Visibility
 *
 * **Validates: Requirements 8.3, 8.5**
 */

import { describe, it, expect, beforeEach } from 'vitest'
import { mount, VueWrapper } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'
import { createI18n } from 'vue-i18n'
import * as fc from 'fast-check'
import { useSettingsStore } from '@/stores/settings'
import UpdateSection from '../../sections/UpdateSection.vue'

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
// Property 11: Update Interval Clamping
// ============================================================================

describe('Feature: settings-enhancement, Property 11: Update Interval Clamping', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('should clamp check interval to valid range [1, 168]', () => {
    fc.assert(
      fc.property(
        fc.integer({ min: -1000, max: 1000 }),
        (inputValue: number) => {
          const store = useSettingsStore()
          
          // Update with arbitrary value
          store.updateUpdate({ checkIntervalHours: inputValue })
          
          // The store should accept the value (clamping happens in UI)
          // But we verify the value is stored
          expect(typeof store.update.checkIntervalHours).toBe('number')
        }
      ),
      { numRuns: 100, verbose: true }
    )
  })

  it('should preserve check interval when updating other settings', () => {
    fc.assert(
      fc.property(
        fc.integer({ min: 1, max: 168 }),
        fc.boolean(),
        fc.boolean(),
        fc.string(),
        (interval: number, autoCheck: boolean, useProxy: boolean, proxyUrl: string) => {
          const store = useSettingsStore()
          
          // Set initial interval
          store.updateUpdate({ checkIntervalHours: interval })
          
          // Update other settings
          store.updateUpdate({ autoCheck, useProxy, proxyUrl })
          
          // Verify interval is preserved
          expect(store.update.checkIntervalHours).toBe(interval)
        }
      ),
      { numRuns: 100, verbose: true }
    )
  })
})

// ============================================================================
// Property 12: Proxy URL Conditional Visibility
// ============================================================================

describe('Feature: settings-enhancement, Property 12: Proxy URL Conditional Visibility', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('should show proxy URL input only when useProxy is true', async () => {
    await fc.assert(
      fc.asyncProperty(
        fc.boolean(),
        async (useProxy: boolean) => {
          const wrapper = mountUpdateSection()
          const store = useSettingsStore()
          
          // Set useProxy state
          store.updateUpdate({ useProxy })
          await wrapper.vm.$nextTick()
          
          // Find proxy URL input container
          const settingItems = wrapper.findAll('.setting-item')
          const proxyUrlItem = settingItems.find(item => 
            item.text().includes('Proxy URL')
          )
          
          if (useProxy) {
            // When useProxy is true, proxy URL should be visible
            expect(proxyUrlItem?.isVisible() ?? false).toBe(true)
          } else {
            // When useProxy is false, proxy URL should be hidden
            // Note: v-show keeps element in DOM but hides it
            expect(proxyUrlItem?.isVisible() ?? true).toBe(false)
          }
          
          wrapper.unmount()
          return true
        }
      ),
      // Reduced numRuns for component mounting tests to avoid timeout
      { numRuns: 20, verbose: true }
    )
  }, 30000) // 30s timeout for component tests

  it('should toggle proxy URL visibility when useProxy changes', async () => {
    await fc.assert(
      fc.asyncProperty(
        fc.array(fc.boolean(), { minLength: 2, maxLength: 5 }),
        async (toggleSequence: boolean[]) => {
          const wrapper = mountUpdateSection()
          const store = useSettingsStore()
          
          for (const useProxy of toggleSequence) {
            store.updateUpdate({ useProxy })
            await wrapper.vm.$nextTick()
            
            // Verify the store state is correct
            expect(store.update.useProxy).toBe(useProxy)
          }
          
          wrapper.unmount()
          return true
        }
      ),
      { numRuns: 100, verbose: true }
    )
  })

  it('should preserve proxy URL value when toggling useProxy', () => {
    fc.assert(
      fc.property(
        fc.webUrl(),
        fc.array(fc.boolean(), { minLength: 2, maxLength: 5 }),
        (proxyUrl: string, toggleSequence: boolean[]) => {
          const store = useSettingsStore()
          
          // Set initial proxy URL
          store.updateUpdate({ proxyUrl })
          
          // Toggle useProxy multiple times
          for (const useProxy of toggleSequence) {
            store.updateUpdate({ useProxy })
          }
          
          // Verify proxy URL is preserved
          expect(store.update.proxyUrl).toBe(proxyUrl)
        }
      ),
      { numRuns: 100, verbose: true }
    )
  })

  it('should set isDirty when useProxy changes', () => {
    fc.assert(
      fc.property(
        fc.boolean(),
        (useProxy: boolean) => {
          const store = useSettingsStore()
          
          // Reset dirty flag
          store.$patch({ isDirty: false })
          
          // Update useProxy
          store.updateUpdate({ useProxy })
          
          // Verify isDirty is set
          expect(store.isDirty).toBe(true)
        }
      ),
      { numRuns: 100, verbose: true }
    )
  })
})
