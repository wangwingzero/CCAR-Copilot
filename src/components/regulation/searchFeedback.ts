export type SearchFeedbackSource = 'local' | 'online' | null

export interface SearchFeedbackInput {
  elapsedMs: number | null
  hasSearched: boolean
  indexedCount: number
  isLoading: boolean
  isLocalSearching: boolean
  keyword: string
  pendingOcr: number
  resultCount: number
  source: SearchFeedbackSource
}

export interface SearchFeedback {
  summary: string
  hint: string
}

export function buildSearchFeedback(input: SearchFeedbackInput): SearchFeedback | null {
  if (input.isLocalSearching) {
    return { summary: '本地搜索中...', hint: '' }
  }

  if (input.isLoading) {
    return { summary: '在线搜索中...', hint: '' }
  }

  if (!input.hasSearched || !input.keyword.trim() || !input.source) {
    return null
  }

  const sourceLabel = input.source === 'online' ? '在线搜索' : '本地搜索'
  const elapsedText =
    input.source === 'local' && input.elapsedMs !== null
      ? `，${Math.max(0, Math.round(input.elapsedMs))}ms`
      : ''
  const indexedText =
    input.source === 'local' && input.indexedCount > 0 ? `（已索引 ${input.indexedCount} 个）` : ''
  const hint =
    input.source === 'local' && input.resultCount === 0 && input.pendingOcr > 0
      ? `还有 ${input.pendingOcr} 个待 OCR，未完成正文暂时搜不到。`
      : ''

  return {
    summary: `${sourceLabel}完成：${input.resultCount} 条${elapsedText}${indexedText}`,
    hint,
  }
}
