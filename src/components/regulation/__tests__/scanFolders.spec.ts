import { describe, expect, it } from 'vitest'
import {
  addScanFolders,
  formatFolderName,
  isPathInScanFolders,
  loadScanFolders,
  removeScanFolder,
  saveScanFolders,
} from '../scanFolders'

describe('scanFolders', () => {
  it('adds selected folders in order and skips duplicates', () => {
    const folders = addScanFolders(['D:\\CCAR'], [
      'D:\\Manuals',
      'D:/Manuals/',
      'E:\\Standards',
    ])

    expect(folders).toEqual(['D:\\CCAR', 'D:\\Manuals', 'E:\\Standards'])
  })

  it('ignores cancelled or empty folder selections', () => {
    expect(addScanFolders(['D:\\CCAR'], null)).toEqual(['D:\\CCAR'])
    expect(addScanFolders(['D:\\CCAR'], '')).toEqual(['D:\\CCAR'])
    expect(addScanFolders(['D:\\CCAR'], ['  '])).toEqual(['D:\\CCAR'])
  })

  it('removes a folder by path using the same normalization rules', () => {
    const folders = removeScanFolder(['D:\\CCAR', 'D:\\Manuals'], 'D:/CCAR/')

    expect(folders).toEqual(['D:\\Manuals'])
  })

  it('formats a compact folder name for display', () => {
    expect(formatFolderName('D:\\飞行手册\\局方\\CCAR规章')).toBe('CCAR规章')
    expect(formatFolderName('D:\\')).toBe('D:\\')
  })

  it('persists normalized scan folders and skips invalid stored data', () => {
    const storage = new Map<string, string>()
    const mockStorage = {
      getItem: (key: string) => storage.get(key) ?? null,
      setItem: (key: string, value: string) => storage.set(key, value),
      removeItem: (key: string) => storage.delete(key),
    } as unknown as Storage

    saveScanFolders(['D:/Manuals/', '  ', 'D:\\Manuals', 'E:\\Regs'], mockStorage)

    expect(loadScanFolders(mockStorage)).toEqual(['D:\\Manuals', 'E:\\Regs'])
  })

  it('detects files inside selected scan folders using path boundaries', () => {
    const folders = ['D:\\Regs', 'E:\\PDF\\局方']

    expect(isPathInScanFolders('D:\\Regs\\CCAR-121.pdf', folders)).toBe(true)
    expect(isPathInScanFolders('E:/PDF/局方/sub/check.pdf', folders)).toBe(true)
    expect(isPathInScanFolders('D:\\Regs-old\\CCAR-121.pdf', folders)).toBe(false)
    expect(isPathInScanFolders('', folders)).toBe(false)
  })
})
