export type FolderSelection = string | string[] | null | undefined

const SCAN_FOLDERS_STORAGE_KEY = 'regulation-scan-folders'

export function normalizeFolderPath(folderPath: string): string {
  let normalized = folderPath.trim().replace(/\//g, '\\')

  while (normalized.length > 3 && normalized.endsWith('\\')) {
    normalized = normalized.slice(0, -1)
  }

  return normalized
}

function folderKey(folderPath: string): string {
  return normalizeFolderPath(folderPath).toLocaleLowerCase()
}

function resolveStorage(storage?: Storage): Storage | null {
  if (storage) return storage

  try {
    return typeof localStorage === 'undefined' ? null : localStorage
  } catch {
    return null
  }
}

export function addScanFolders(existing: string[], selection: FolderSelection): string[] {
  const selectedFolders = Array.isArray(selection) ? selection : selection ? [selection] : []
  const nextFolders = [...existing]
  const existingKeys = new Set(nextFolders.map(folderKey).filter(Boolean))

  for (const selected of selectedFolders) {
    const normalized = normalizeFolderPath(selected)
    const key = folderKey(normalized)

    if (!normalized || existingKeys.has(key)) {
      continue
    }

    nextFolders.push(normalized)
    existingKeys.add(key)
  }

  return nextFolders
}

export function loadScanFolders(storage?: Storage): string[] {
  const target = resolveStorage(storage)
  if (!target) return []

  try {
    const raw = target.getItem(SCAN_FOLDERS_STORAGE_KEY)
    const parsed = raw ? JSON.parse(raw) : []
    return Array.isArray(parsed)
      ? addScanFolders(
          [],
          parsed.filter(item => typeof item === 'string')
        )
      : []
  } catch {
    return []
  }
}

export function saveScanFolders(folders: string[], storage?: Storage): void {
  const target = resolveStorage(storage)
  if (!target) return

  const normalized = addScanFolders([], folders)

  try {
    if (normalized.length === 0) {
      target.removeItem(SCAN_FOLDERS_STORAGE_KEY)
    } else {
      target.setItem(SCAN_FOLDERS_STORAGE_KEY, JSON.stringify(normalized))
    }
  } catch {
    /* ignore storage failures */
  }
}

export function removeScanFolder(existing: string[], folderPath: string): string[] {
  const keyToRemove = folderKey(folderPath)
  return existing.filter(folder => folderKey(folder) !== keyToRemove)
}

export function isPathInScanFolders(filePath: string, folders: string[]): boolean {
  const pathKey = folderKey(filePath)
  const folderKeys = addScanFolders([], folders).map(folderKey)

  if (folderKeys.length === 0) return true
  if (!pathKey) return false

  return folderKeys.some(folder => {
    if (pathKey === folder) return true
    return pathKey.startsWith(folder.endsWith('\\') ? folder : `${folder}\\`)
  })
}

export function formatFolderName(folderPath: string): string {
  const normalized = normalizeFolderPath(folderPath)

  if (!normalized) {
    return ''
  }

  if (/^[A-Za-z]:\\$/.test(normalized)) {
    return normalized
  }

  const parts = normalized.split('\\').filter(Boolean)
  return parts[parts.length - 1] ?? normalized
}
