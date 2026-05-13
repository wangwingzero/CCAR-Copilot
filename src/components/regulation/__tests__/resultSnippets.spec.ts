import { describe, expect, it } from 'vitest'
import type { RegulationDocument } from '@/types'
import { resolveResultSnippet } from '../resultSnippets'

function doc(overrides: Partial<RegulationDocument> = {}): RegulationDocument {
  return {
    title: '民用航空空中交通管理检查员管理办法',
    doc_number: 'AP-66I-TM-2010-02',
    validity: '有效',
    doc_type: 'normative',
    office_unit: '空管行业管理办公室',
    sign_date: '',
    publish_date: '2010-11-01',
    url: 'https://example.com/check',
    pdf_url: '',
    file_path: '',
    content: '',
    ...overrides,
  }
}

describe('resultSnippets', () => {
  it('prefers backend snippet when available', () => {
    const snippet = resolveResultSnippet(doc(), '...<mark>检查员</mark>职责要求...')

    expect(snippet).toBe('...<mark>检查员</mark>职责要求...')
  })

  it('builds a fallback snippet from document context when backend snippet is missing', () => {
    const snippet = resolveResultSnippet(doc())

    expect(snippet).toContain('民用航空空中交通管理检查员管理办法')
    expect(snippet).toContain('AP-66I-TM-2010-02')
    expect(snippet).toContain('2010-11-01')
    expect(snippet).toContain('空管行业管理办公室')
  })

  it('returns undefined when neither snippet nor document context exists', () => {
    const snippet = resolveResultSnippet(
      doc({
        title: '',
        doc_number: '',
        validity: '',
        office_unit: '',
        publish_date: '',
        file_path: '',
      })
    )

    expect(snippet).toBeUndefined()
  })
})
