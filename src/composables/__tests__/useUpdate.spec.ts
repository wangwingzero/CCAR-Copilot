import { defineComponent } from 'vue'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import { beforeEach, describe, expect, it, vi } from 'vitest'

const mocks = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
}))

vi.mock('@tauri-apps/api/core', () => ({
  invoke: mocks.invoke,
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: mocks.listen,
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

describe('useUpdate', () => {
  beforeEach(() => {
    vi.useFakeTimers()
    vi.resetModules()
    setActivePinia(createPinia())
    mocks.invoke.mockReset()
    mocks.listen.mockReset()
    mocks.listen.mockResolvedValue(vi.fn())
  })

  it('does not let a stale update check overwrite an active download', async () => {
    type UseUpdateApi = ReturnType<typeof import('../useUpdate')['useUpdate']>
    const checkResult = deferred<UseUpdateApi['status']['value']>()
    const downloadResult = deferred<UseUpdateApi['status']['value']>()

    mocks.invoke.mockImplementation((command: string) => {
      if (command === 'get_current_version') return Promise.resolve('0.1.10')
      if (command === 'get_update_config') {
        return Promise.resolve({
          auto_update_enabled: true,
          check_interval_hours: 24,
          check_on_startup: true,
          auto_download: true,
          auto_install: false,
        })
      }
      if (command === 'check_for_update') return checkResult.promise
      if (command === 'download_and_install_update') return downloadResult.promise
      return Promise.resolve(undefined)
    })

    const { useUpdate } = await import('../useUpdate')
    let api!: ReturnType<typeof useUpdate>
    const Harness = defineComponent({
      setup() {
        api = useUpdate()
        return () => null
      },
    })

    const wrapper = mount(Harness, {
      global: {
        plugins: [createPinia()],
      },
    })

    const checkPromise = api.checkForUpdate()
    const downloadPromise = api.downloadAndInstall()

    expect(api.status.value.status).toBe('Downloading')

    checkResult.resolve({
      status: 'Available',
      info: {
        version: '0.1.11',
        notes: null,
        date: null,
        downloadSize: null,
      },
    })
    await checkPromise

    expect(api.status.value.status).toBe('Downloading')

    downloadResult.resolve({ status: 'PendingRestart' })
    await downloadPromise
    wrapper.unmount()
  })
})
