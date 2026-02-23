/**
 * HTML 安全工具函数
 *
 * 提供 XSS 防护相关的工具函数，用于安全渲染用户输入或外部来源的内容。
 */

/**
 * 安全协议白名单
 * 只允许这些协议出现在 href/src 属性中
 */
const SAFE_URL_PROTOCOLS = ['http:', 'https:', 'mailto:']

/**
 * 验证并清理 URL，防止 javascript: 等危险协议注入
 *
 * @param url - 待验证的 URL（可能已经过 HTML 实体转义）
 * @returns 安全的 URL，危险 URL 返回 '#'
 *
 * @example
 * sanitizeUrl('https://example.com')          // 'https://example.com'
 * sanitizeUrl('javascript:alert(1)')          // '#'
 * sanitizeUrl('data:text/html,...')            // '#'
 * sanitizeUrl('/relative/path')               // '/relative/path'
 * sanitizeUrl('#anchor')                      // '#anchor'
 */
export function sanitizeUrl(url: string): string {
  // 还原 HTML 实体转义后再判断协议（parseMarkdown 会先转义 & < >）
  const decoded = url
    .replace(/&amp;/g, '&')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .trim()

  // 空字符串直接返回 #
  if (!decoded) return '#'

  // 相对路径和锚点允许通过
  if (decoded.startsWith('/') || decoded.startsWith('#') || decoded.startsWith('./')) {
    return url
  }

  // 检查是否为安全协议
  try {
    const urlObj = new URL(decoded, 'https://placeholder.local')
    if (SAFE_URL_PROTOCOLS.includes(urlObj.protocol)) {
      return url
    }
  } catch {
    // URL 解析失败，检查是否没有协议前缀（纯域名）
    if (!decoded.includes(':')) {
      return url
    }
  }

  // 不安全的协议（javascript:, data:, vbscript: 等）
  return '#'
}

/**
 * 验证图片 src URL 的安全性
 *
 * 比 sanitizeUrl 更严格：只允许 http/https 和相对路径，
 * 不允许 data: URI（可能包含恶意内容）。
 *
 * @param src - 图片 URL
 * @returns 安全的 URL，危险 URL 返回空字符串
 */
export function sanitizeImageSrc(src: string): string {
  const decoded = src
    .replace(/&amp;/g, '&')
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .trim()

  if (!decoded) return ''

  // 允许相对路径
  if (decoded.startsWith('/') || decoded.startsWith('./')) {
    return src
  }

  // 只允许 http/https 协议
  try {
    const urlObj = new URL(decoded, 'https://placeholder.local')
    if (urlObj.protocol === 'http:' || urlObj.protocol === 'https:') {
      return src
    }
  } catch {
    if (!decoded.includes(':')) {
      return src
    }
  }

  return ''
}
