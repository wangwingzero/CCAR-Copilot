import { flushPromises, mount, type VueWrapper } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { createI18n } from 'vue-i18n'
import { beforeEach, describe, expect, it, vi } from 'vitest'

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

type Deferred<T> = {
  promise: Promise<T>
  resolve: (value: T) => void
  reject: (reason?: unknown) => void
}

function deferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void
  let reject!: (reason?: unknown) => void
  const promise = new Promise<T>((res, rej) => {
    resolve = res
    reject = rej
  })

  return { promise, resolve, reject }
}

function createTestI18n() {
  return createI18n({
    legacy: false,
    locale: 'en',
    messages: {
      en: {
        settings: {
          update: {
            title: 'Update',
            settings: 'Startup Check',
            currentVersion: 'Current Version',
            autoCheck: 'Auto Check',
            autoCheckHelp: 'Automatically check for updates',
            checkNow: 'Check Now',
            checkNowBtn: 'Check',
            checking: 'Checking...',
            lastCheck: 'Last Check',
            manualDownload: 'Manual Update',
            manualDownloadHelp: 'Open the latest Windows installer in your browser',
            openLatestInstaller: 'Open Installer',
            openingLatestInstaller: 'Opening...',
            manualDownloadError: 'Failed to open installer URL: {message}',
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
            currentToNew: 'Current {current} -> New {next}',
            downloadSpeed: 'Speed',
            downloadEta: 'ETA',
            etaSeconds: '{n}s',
            etaMinutes: '{n} min',
            etaHours: '{n} h',
            toastNewVersion: 'New version {version} available',
            toastDownloaded: '{version} downloaded',
            toastError: 'Update failed: {message}',
          },
        },
      },
    },
  })
}

async function mountFreshUpdateSection(): Promise<VueWrapper> {
  const pinia = createPinia()
  setActivePinia(pinia)
  const UpdateSection = await import('../sections/UpdateSection.vue')

  return mount(UpdateSection.default, {
    global: {
      plugins: [createTestI18n(), pinia],
    },
  })
}

function mockBaseUpdateCommands(): void {
  mocks.listen.mockResolvedValue(vi.fn())
  mocks.open.mockResolvedValue(undefined)
  mocks.invoke.mockImplementation((command: string) => {
    if (command === 'get_current_version') return Promise.resolve('0.1.10')
    if (command === 'get_update_config') {
      return Promise.resolve({
        auto_update_enabled: true,
        check_interval_hours: 24,
        check_on_startup: false,
        auto_download: true,
        auto_install: false,
      })
    }
    if (command === 'check_for_update') {
      return Promise.resolve({
        status: 'Available',
        info: {
          version: '0.1.11',
          notes: 'RELEASE_NOTES_SHOULD_NOT_RENDER',
          date: '2026-05-14T03:45:09Z',
          downloadSize: null,
        },
      })
    }
    return Promise.resolve(undefined)
  })
}

describe('UpdateSection update flow', () => {
  beforeEach(() => {
    vi.resetModules()
    setActivePinia(createPinia())
    mocks.invoke.mockReset()
    mocks.listen.mockReset()
    mocks.open.mockReset()
  })

  it('does not render release notes when an update is available', async () => {
    mockBaseUpdateCommands()
    const wrapper = await mountFreshUpdateSection()

    await flushPromises()
    await wrapper.find('.check-now-btn').trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('Update available')
    expect(wrapper.text()).toContain('Current 0.1.10 -> New 0.1.11')
    expect(wrapper.text()).not.toContain('RELEASE_NOTES_SHOULD_NOT_RENDER')

    wrapper.unmount()
  })

  it('shows the progress bar immediately after starting the update', async () => {
    const downloadResult = deferred<{ status: 'PendingRestart' }>()
    mockBaseUpdateCommands()
    mocks.invoke.mockImplementation((command: string) => {
      if (command === 'download_and_install_update') return downloadResult.promise
      if (command === 'get_current_version') return Promise.resolve('0.1.10')
      if (command === 'get_update_config') {
        return Promise.resolve({
          auto_update_enabled: true,
          check_interval_hours: 24,
          check_on_startup: false,
          auto_download: true,
          auto_install: false,
        })
      }
      if (command === 'check_for_update') {
        return Promise.resolve({
          status: 'Available',
          info: {
            version: '0.1.11',
            notes: 'RELEASE_NOTES_SHOULD_NOT_RENDER',
            date: null,
            downloadSize: null,
          },
        })
      }
      return Promise.resolve(undefined)
    })

    const wrapper = await mountFreshUpdateSection()
    await flushPromises()
    await wrapper.find('.check-now-btn').trigger('click')
    await flushPromises()

    await wrapper.find('.primary-btn').trigger('click')
    await flushPromises()

    expect(wrapper.text()).toContain('Downloading update...')
    expect(wrapper.find('.progress-track').exists()).toBe(true)
    expect(wrapper.text()).not.toContain('RELEASE_NOTES_SHOULD_NOT_RENDER')

    downloadResult.resolve({ status: 'PendingRestart' })
    await flushPromises()
    wrapper.unmount()
  })
})
