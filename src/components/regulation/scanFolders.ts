export type FolderSelection = string | string[] | null | undefined

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

export function removeScanFolder(existing: string[], folderPath: string): string[] {
  const keyToRemove = folderKey(folderPath)
  return existing.filter(folder => folderKey(folder) !== keyToRemove)
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
