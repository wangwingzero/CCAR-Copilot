import { describe, expect, it } from 'vitest'
import {
  addScanFolders,
  formatFolderName,
  removeScanFolder,
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
})
