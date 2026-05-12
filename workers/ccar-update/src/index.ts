/**
 * CCAR Copilot 更新分发 Cloudflare Worker
 *
 * 职责:
 * - 拦截 https://ccar-update.031986.xyz/* 请求
 * - 白名单路径:`/latest.json`(更新清单) 与 `/downloads/*`(NSIS 安装包/签名/更新包)
 * - 回源 ORIGIN (默认 https://ccar-dl.hudawang.cn) 并通过 Cloudflare 边缘
 *   缓存加速国内用户下载
 *
 * 安全:
 * - Worker 不校验下载体签名,由 Tauri 客户端使用 tauri.conf.json 里的 `pubkey`
 *   验证 `*.sig`,Worker 泄露不会导致任意代码下发
 * - 默认不允许跨路径穿越(比如不代理 `/..` 或任意路径)
 */

export interface Env {
  ORIGIN: string
  LATEST_TTL: string
  ARTIFACT_TTL: string
}

const ALLOWED_PREFIXES = ['/downloads/'] as const
const ALLOWED_EXACT = ['/latest.json'] as const

function isPathAllowed(pathname: string): boolean {
  if (ALLOWED_EXACT.includes(pathname as (typeof ALLOWED_EXACT)[number])) return true
  return ALLOWED_PREFIXES.some((p) => pathname.startsWith(p))
}

function pickTtl(pathname: string, env: Env): number {
  if (pathname === '/latest.json') {
    return parseInt(env.LATEST_TTL ?? '60', 10) || 60
  }
  return parseInt(env.ARTIFACT_TTL ?? '86400', 10) || 86400
}

export default {
  async fetch(request: Request, env: Env, ctx: ExecutionContext): Promise<Response> {
    const url = new URL(request.url)

    if (request.method !== 'GET' && request.method !== 'HEAD') {
      return new Response('Method Not Allowed', {
        status: 405,
        headers: { 'Allow': 'GET, HEAD' },
      })
    }

    if (!isPathAllowed(url.pathname)) {
      return new Response('Not Found', { status: 404 })
    }

    // 回源 URL 组装:保持路径与查询字符串,Host 换成源站
    const origin = env.ORIGIN.replace(/\/+$/, '')
    const originUrl = `${origin}${url.pathname}${url.search}`

    const cache = caches.default
    // 以原始请求 URL 作为缓存 key,保证不同查询串独立缓存
    const cacheKey = new Request(request.url, { method: 'GET' })

    // 先读缓存(HEAD 也用 GET 的缓存,命中时返回完整响应)
    const cached = await cache.match(cacheKey)
    if (cached) {
      const resp = new Response(cached.body, cached)
      resp.headers.set('X-CCar-Cache', 'HIT')
      return resp
    }

    // 回源
    let originResp: Response
    try {
      originResp = await fetch(originUrl, {
        method: 'GET',
        headers: {
          'User-Agent': 'ccar-update-worker/1.0',
          // 透传 If-None-Match/If-Modified-Since 让源站有机会返回 304
          ...(request.headers.get('if-none-match')
            ? { 'If-None-Match': request.headers.get('if-none-match')! }
            : {}),
          ...(request.headers.get('if-modified-since')
            ? { 'If-Modified-Since': request.headers.get('if-modified-since')! }
            : {}),
        },
        cf: {
          cacheEverything: true,
          cacheTtlByStatus: {
            '200-299': pickTtl(url.pathname, env),
            '304': pickTtl(url.pathname, env),
            '404': 30,
            '500-599': 0,
          },
        },
      })
    } catch (e) {
      return new Response(`Origin fetch failed: ${(e as Error).message}`, {
        status: 502,
      })
    }

    if (!originResp.ok && originResp.status !== 304) {
      return new Response(`Upstream error: ${originResp.status}`, {
        status: originResp.status === 404 ? 404 : 502,
      })
    }

    const ttl = pickTtl(url.pathname, env)

    // latest.json 需要改写内部的下载 URL:
    // release.ps1 发布时把 url 字段填的是源站域 (ccar-dl.hudawang.cn/downloads/...),
    // 保证源站直连 fallback 时客户端拿到的 URL 就是源站本地路径可以直连下载。
    // 但走 CF 通道时客户端应该用 CF 域下载以吃 CDN 加速,所以这里做替换。
    let responseToReturn: Response
    if (url.pathname === '/latest.json') {
      const rawText = await originResp.text()
      const originDownloadPrefix = `${origin}/downloads/`
      const selfDownloadPrefix = `https://${url.host}/downloads/`
      const rewritten = rawText.split(originDownloadPrefix).join(selfDownloadPrefix)

      responseToReturn = new Response(rewritten, {
        status: originResp.status,
        statusText: originResp.statusText,
        headers: originResp.headers,
      })
      responseToReturn.headers.set('Content-Type', 'application/json; charset=utf-8')
      responseToReturn.headers.set('X-CCar-Resource', 'manifest')
      responseToReturn.headers.set('X-CCar-Rewrote-Urls', '1')
      // 改写后长度变了,删掉可能过期的 Content-Length 避免下游误解析
      responseToReturn.headers.delete('Content-Length')
    } else {
      responseToReturn = new Response(originResp.body, originResp)
      const contentType =
        originResp.headers.get('Content-Type') ?? 'application/octet-stream'
      responseToReturn.headers.set('Content-Type', contentType)
      responseToReturn.headers.set('X-CCar-Resource', 'artifact')
    }
    responseToReturn.headers.set(
      'Cache-Control',
      `public, max-age=${ttl}, s-maxage=${ttl}`
    )
    responseToReturn.headers.set('X-CCar-Cache', 'MISS')

    // 写缓存(HEAD 不写,避免 body 为空)
    if (request.method === 'GET' && originResp.ok) {
      ctx.waitUntil(cache.put(cacheKey, responseToReturn.clone()))
    }
    return responseToReturn
  },
} satisfies ExportedHandler<Env>
