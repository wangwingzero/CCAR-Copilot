import type { RegulationDocument } from '@/types'

const FALLBACK_SNIPPET_MAX_LENGTH = 240

function truncateText(text: string, maxLength: number): string {
  const normalized = text.trim()
  if (!normalized) return ''

  const chars = Array.from(normalized)
  if (chars.length <= maxLength) return normalized

  return `${chars.slice(0, maxLength).join('').trimEnd()}...`
}

function buildFallbackSnippet(document: RegulationDocument): string | undefined {
  const parts = [
    document.title.trim(),
    document.doc_number.trim() ? `文号 ${document.doc_number.trim()}` : '',
    document.publish_date.trim() ? `发布 ${document.publish_date.trim()}` : '',
    document.office_unit.trim(),
    document.validity.trim() ? `状态 ${document.validity.trim()}` : '',
  ].filter(Boolean)

  if (parts.length === 0) return undefined

  return truncateText(parts.join(' · '), FALLBACK_SNIPPET_MAX_LENGTH)
}

export function resolveResultSnippet(
  document: RegulationDocument,
  backendSnippet?: string | null
): string | undefined {
  const normalizedBackendSnippet = backendSnippet?.trim()
  if (normalizedBackendSnippet) {
    return normalizedBackendSnippet
  }

  return buildFallbackSnippet(document)
}
