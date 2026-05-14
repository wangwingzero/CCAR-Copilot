import { describe, expect, it } from 'vitest'
import { buildSearchFeedback } from '../searchFeedback'

describe('searchFeedback', () => {
  it('explains zero local results when OCR still has pending documents', () => {
    const feedback = buildSearchFeedback({
      elapsedMs: 0,
      hasSearched: true,
      indexedCount: 525,
      isLoading: false,
      isLocalSearching: false,
      keyword: '检查员',
      pendingOcr: 2438,
      resultCount: 0,
      source: 'local',
    })

    expect(feedback).toEqual({
      summary: '本地搜索完成：0 条，0ms（已索引 525 个）',
      hint: '还有 2438 个待 OCR，未完成正文暂时搜不到。',
    })
  })

  it('reports local searching while the backend request is still running', () => {
    const feedback = buildSearchFeedback({
      elapsedMs: null,
      hasSearched: true,
      indexedCount: 525,
      isLoading: false,
      isLocalSearching: true,
      keyword: '检查员',
      pendingOcr: 0,
      resultCount: 0,
      source: 'local',
    })

    expect(feedback).toEqual({
      summary: '本地搜索中...',
      hint: '',
    })
  })
})
