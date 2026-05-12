# `ccar-update` Cloudflare Worker

CCAR Copilot 私有更新通道的 CDN 层。

```
+----------+        +----------------------------+        +----------------------+
|  客户端   |  GET   | ccar-update.031986.xyz/*   |  回源  | ccar-dl.hudawang.cn  |
| (Tauri)  +-------->  (Cloudflare Worker + 边缘 +-------->  (Nginx / 宝塔)      |
|          |        |   节点缓存 LATEST_TTL/...) |        |  /www/wwwroot/...    |
+----------+        +----------------------------+        +----------------------+
```

## 部署

1. 安装 Node.js 18+ 与 wrangler:

   ```powershell
   cd workers/ccar-update
   npm install
   npx wrangler login         # 用浏览器跳转完成 OAuth
   ```

2. 确认 `wrangler.toml` 中 `[vars]`、`zone_name` 正确后:

   ```powershell
   npm run deploy
   ```

3. 在 Cloudflare 控制台 `031986.xyz` → DNS,添加一条代理开启(橙云)的
   `CNAME ccar-update → ccar-update.<account>.workers.dev`
   或任意代理开启的占位记录,让 Worker 路由 `ccar-update.031986.xyz/*` 生效。

## 变量说明

| 变量           | 默认值                       | 说明                                        |
| -------------- | ---------------------------- | ------------------------------------------- |
| `ORIGIN`       | `https://ccar-dl.hudawang.cn` | 源站基址,国内宝塔服务器                     |
| `LATEST_TTL`   | `60`                         | `/latest.json` 边缘缓存秒数                 |
| `ARTIFACT_TTL` | `86400`                      | `/downloads/*` 边缘缓存秒数                 |

发布新版本后需要 Purge 掉 `/latest.json` 或等它到 TTL 失效。
`scripts/release.ps1` 已包含自动 Purge 步骤。

## URL 改写

`release.ps1` 在 `latest.json` 里写入的下载 URL 是 **源站** 域
(`https://ccar-dl.hudawang.cn/downloads/...`)。当请求经过本 Worker 时,
会用字符串替换把它改成 CF 域 (`https://ccar-update.031986.xyz/downloads/...`),
确保客户端后续的下载请求也会命中 CF 边缘缓存。

这样做的好处:**源站只维护一份 manifest**,但两条通道(Worker 主 / 源站
fallback)都能正确工作:

- 客户端走 Worker 主通道 → 拿到 CF 域 URL → 下载继续吃 CDN 加速
- 客户端 CF 失败回落源站 → 直接拉源站 manifest(不经过 Worker)→ 拿到源站
  域 URL → 下载直连源站,绕开 CF

## 本地调试

```powershell
npm run dev
# 访问 http://127.0.0.1:8787/latest.json
```

`wrangler dev` 会把 ORIGIN 当成远程源站回源,便于验证路由。

## 日志

```powershell
npm run tail
```

实时查看生产日志,排查回源失败/缓存命中率。
