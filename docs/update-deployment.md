# CCAR Copilot 自动更新部署指南

本文档说明如何搭建 **私有、签名、CDN 加速** 的 Tauri 自动更新通道。

## 整体架构

客户端按 **优先级顺序** 尝试两条独立通道:先走 CF 加速,失败自动回落源站直连。

```
                           优先通道 (主)
        +------------------------------------------------+
        |                                                v
+-------+-------+       GET /latest.json        +---------------------+
|   CCAR        |    (Tauri updater 插件,        |  Cloudflare Worker  |
|   Copilot     |     多 endpoint 顺序尝试)      |  ccar-update.       |
|   桌面客户端   |                                |  031986.xyz/*       |
|   (Windows)   |                                +----------+----------+
+-------+-------+                                           |
        |                                     回源 / 缓存    v
        |                                                +--+--------+
        |                                                |           |
        |       备通道 (fallback,CF 不可达时直接连源站)  |  源站     |
        +----------------------------------------------->+  ccar-dl  |
                                                         |  .hudawang|
                                                         |  .cn      |
                                                         |  (宝塔    |
                                                         |   Nginx)  |
                                                         +-----------+
                                                               ^
                                                  scp 上传    |
                                                +------+------+
                                                |scripts/release.ps1|
                                                +-------------------+
```

**关键原则**

- **第一优先走 CF 加速,失败才回落源站直连**:`tauri.conf.json` 里
  `endpoints` 是按顺序排列的数组,updater 会依次尝试,前一个响应失败或不可
  解析才尝试下一个。国内用户日常命中 CF 边缘节点(国内 POP);CF Worker
  宕机或被墙时,客户端透明切换到源站直连,升级流程不中断。
- 客户端只信任 `tauri.conf.json` 里写死的 ed25519 公钥;Cloudflare 或源站被
  入侵也无法替换可执行文件。
- Worker 不校验签名,只负责回源与 CDN 缓存,减少源站带宽压力。
- 两条通道 **共享同一份产物** —— `scripts/release.ps1` 只上传源站,CF Worker
  通过回源拉取,无需双写。
- 域名分工: `031986.xyz` 的子域名用于 Worker(该域名托管在 CF 且有历史,
  不易被墙);源站用 `hudawang.cn`,DNS 灰云(不经过 CF 代理)以便直连。

## 一次性基础设施初始化

按以下 **7 步** 顺序执行,每步独立可回滚。

### 1. 生成更新签名密钥

```powershell
# 在您的主开发机,不在服务器上执行
cd d:\CCAR-Copilot
npx tauri signer generate -w "$HOME\.tauri\ccar-copilot-updater.key" --no-password
```

- 命令会产生两份内容:
  - `%USERPROFILE%\.tauri\ccar-copilot-updater.key`:**私钥**,用于构建时签名,**绝不上传 Git 或服务器**
  - stdout 中的 `Public key`:**公钥**,填到 `src-tauri/tauri.conf.json` 的 `plugins.updater.pubkey`

将公钥替换 `tauri.conf.json` 中的占位:

```jsonc
"plugins": {
  "updater": {
    "endpoints": [
      // 1. 第一优先: CF Worker 边缘加速
      "https://ccar-update.031986.xyz/latest.json",
      // 2. 备用: 源站直连 (CF 不可达时透明回落)
      "https://ccar-dl.hudawang.cn/latest.json"
    ],
    "pubkey": "<把上面生成的 Public key 粘贴到这里>",
    "windows": { "installMode": "passive" }
  }
}
```

> **重要:** 只要公钥一旦发布给用户,**永远不要更换**。换了公钥会导致所有老客户端无法验证更新,只能重新下载安装。
> 备份 `ccar-copilot-updater.key` 到离线位置(硬件加密盘 / 1Password 附件)。

### 2. 宝塔:新建源站 `ccar-dl.hudawang.cn`

1. 宝塔面板 → **网站** → **添加站点**
   - 域名: `ccar-dl.hudawang.cn`
   - 根目录: `/www/wwwroot/ccar-release`
   - 不创建数据库,不创建 FTP
2. 站点配置 → **SSL** → Let's Encrypt,强制 HTTPS
3. 站点配置 → **伪静态** → 粘贴:

    ```nginx
    # 允许 Tauri updater 直接访问 latest.json 和 downloads/*
    location = /latest.json {
        default_type application/json;
        add_header Cache-Control "public, max-age=60";
    }

    location ^~ /downloads/ {
        # 二进制文件可长缓存,文件名含版本号所以无冲突
        add_header Cache-Control "public, max-age=3600";
        # 常见 MIME 兜底
        types {
            application/octet-stream exe zip sig;
            application/json json;
        }
    }

    # 其他路径全部 404,拒绝遍历
    location / {
        return 404;
    }
    ```

4. SSH 建立发布目录骨架:

    ```bash
    ssh -p 7668 root@154.9.27.44
    mkdir -p /www/wwwroot/ccar-release/downloads
    chown -R www:www /www/wwwroot/ccar-release
    ```

### 3. Cloudflare DNS

两个域名都要配,分别支撑两条通道。

**a. `031986.xyz` → CF Worker(主通道,第一优先)**

| 类型  | 名称          | 目标                               | 代理    |
| ----- | ------------- | ---------------------------------- | ------- |
| CNAME | `ccar-update` | `<Worker 部署后给的域名>`          | 已代理  |

> 第一次还没有 Worker,可以先随便填一个已代理记录占位(如 `workers.dev`)。部署 Worker 后改回来。

**b. `hudawang.cn` → 源站(备通道,CF 失败时直连)**

在 `hudawang.cn` 所在的 DNS 控制台(CF / DNSPod / 阿里云 DNS 任一)添加:

| 类型  | 名称       | 目标            | 代理                 |
| ----- | ---------- | --------------- | -------------------- |
| A     | `ccar-dl`  | `154.9.27.44`   | **关闭 / 仅 DNS**    |

**关键:**

- 如果 `hudawang.cn` 托管在 Cloudflare,必须 **灰云**(仅 DNS,不代理)。
  走 CF 代理的话,CF 被墙时这条备路也一起断,双 endpoint 就失去意义。
- 灰云让 DNS 解析直接返回源站 IP `154.9.27.44`,绕开 CF 边缘。
- 对应地,**源站 Nginx 必须开启公网可信 HTTPS 证书**
  (第 2 步宝塔已配 Let's Encrypt,客户端校验证书通过)。

### 4. 部署 Cloudflare Worker

```powershell
cd d:\CCAR-Copilot\workers\ccar-update
npm install
npx wrangler login   # 会打开浏览器,用 86250887@qq.com 对应的账户登录
npm run deploy
```

部署输出类似:

```
Published ccar-update (1.23 sec)
  https://ccar-update.<account>.workers.dev
  ccar-update.031986.xyz/*
```

回到 CF DNS,把上一步 `ccar-update` 的 CNAME 目标改成 `ccar-update.<account>.workers.dev`,保持橙云代理。

### 5. Cloudflare API Token(发布脚本用)

CF 控制台 → **My Profile** → **API Tokens** → **Create Token**:

- 选模板 *Custom token*
- Permissions: `Zone` → `Cache Purge` → `Purge`
- Zone Resources: `Include` → `Specific zone` → `031986.xyz`

创建后复制 **Token** 和 **Zone ID**(后者在 `031986.xyz` Overview 右下角)。

### 6. 本地 PowerShell Profile:持久化环境变量

把以下内容追加到 `$PROFILE`(即 `%USERPROFILE%\Documents\PowerShell\Microsoft.PowerShell_profile.ps1`):

```powershell
# CCAR Copilot 发布所需
$env:TAURI_SIGNING_PRIVATE_KEY = "$HOME\.tauri\ccar-copilot-updater.key"
$env:RELEASE_SSH_USER          = 'root'
$env:RELEASE_SSH_HOST          = '154.9.27.44'
$env:RELEASE_SSH_PORT          = '7668'
$env:RELEASE_SSH_KEY           = "$HOME\.ssh\154.9.27.44_id_ed25519"
$env:RELEASE_REMOTE_DIR        = '/www/wwwroot/ccar-release'
$env:CF_API_TOKEN              = '...<从 CF 控制台复制>...'
$env:CF_ZONE_ID                = '...<031986.xyz 的 zone id>...'
```

重开 PowerShell 窗口使其生效,或直接 `. $PROFILE`。

### 7. 打一次 "版本 0.1.0" 作为基线

```powershell
cd d:\CCAR-Copilot
pwsh -File scripts/release.ps1
```

脚本会:

1. `tauri build` 产出 NSIS 安装器和 `.nsis.zip` updater 包 + `.sig`
2. 写 `latest.json`(pub_date / version / 签名 / 下载 URL)
3. `scp` 上传到 `/www/wwwroot/ccar-release/`
4. `Invoke-RestMethod` 调 CF API Purge 掉 `latest.json` 缓存

验证(**两条通道都必须 200**):

```powershell
# 主通道 - CF Worker 边缘加速
curl https://ccar-update.031986.xyz/latest.json
# 注意返回里 platforms.windows-x86_64.url 已被 Worker 改写为 ccar-update.* 域
curl -I https://ccar-update.031986.xyz/downloads/CCAR%20Copilot_0.1.0_x64-setup.nsis.zip

# 备通道 - 源站直连, 客户端在 CF 不可达时会走这里
curl https://ccar-dl.hudawang.cn/latest.json
# 注意这里返回的 url 字段是 ccar-dl.* 域 (未经 Worker 改写)
curl -I https://ccar-dl.hudawang.cn/downloads/CCAR%20Copilot_0.1.0_x64-setup.nsis.zip
```

第一次访问 CF 通道 `X-CCar-Cache: MISS`,第二次起 `HIT`。

> **URL 改写的工作原理**
>
> `release.ps1` 上传到源站的 `latest.json` 里写的是 `https://ccar-dl.hudawang.cn/downloads/...`
> (源站绝对路径)。
>
> - 当客户端经 CF Worker 命中主通道时,Worker 用字符串替换把所有
>   `ccar-dl.hudawang.cn/downloads/` 改写成 `ccar-update.031986.xyz/downloads/`,
>   这样下载请求继续走 CF 加速。
> - 当客户端 CF 失败回落到备通道,直接拉源站的 latest.json 不经过 Worker,
>   拿到的 URL 未被改写,指向源站自己,下载直连。
>
> 这样源站只维护一份 manifest,两条通道分别用合适的下载路径。

## 日常发布流程

每次上线新版本:

1. 修改 `package.json` + `src-tauri/Cargo.toml` + `src-tauri/tauri.conf.json` 的 version 字段(三处必须一致)
2. 可选:写 `RELEASE_NOTES.md`(Markdown,会写入 `latest.json.notes`)
3. 执行:

    ```powershell
    pwsh -File scripts/release.ps1
    ```

4. 在老版本客户端点 **设置 → 更新 → 立即检查** 测试升级流程

## 故障排查

### 客户端提示 "下载或安装更新失败: signature error"

- 公钥不匹配:`tauri.conf.json` 的 `pubkey` 与签名用的私钥不是同一对。回到第 1 步重新对齐。
- 旧客户端(比如 0.0.x)没有公钥,升级到 0.1.0 后才有。历史用户需要重装一次 setup.exe。

### 客户端提示 "检查更新失败: …"

- `curl https://ccar-update.031986.xyz/latest.json` 看:
  - 403/404 → Worker 路由或回源失败,检查 CF Worker → Logs(`npm run tail`)
  - ECONNRESET → Cloudflare 没代理,检查 DNS 橙云
- 本地先走代理试试:设置里打开 "使用代理",填 `https://ghproxy.net/` 看是否恢复(可判断是否是 CDN 端问题)

### 客户端进度条卡在 0%

- 后端 emit 事件失败:打开 DevTools(`Ctrl+Shift+I`)→ Console,搜 `update://`
- Rust 日志:`%APPDATA%\com.wangh.ccarcopilot\logs\`,看 `updater` 关键字

### `cargo check` 报 `tauri-plugin-updater` 找不到

- `src-tauri/Cargo.lock` 没更新,执行一次 `cargo update -p tauri-plugin-updater`
- 也可能是网络:`cargo build` 需要从 crates.io 拉取

### Cloudflare Worker 部署失败 `wrangler: command not found`

```powershell
cd workers/ccar-update
npm install        # 安装 devDependencies 里的 wrangler
npx wrangler deploy
```

## 关键文件速查

| 文件                                                      | 作用                           |
| --------------------------------------------------------- | ------------------------------ |
| `src-tauri/tauri.conf.json`                               | updater endpoint + pubkey      |
| `src-tauri/src/commands/update_cmd.rs`                    | check / download / restart     |
| `src/composables/useUpdate.ts`                            | 前端状态 + 事件监听            |
| `src/components/settings/sections/UpdateSection.vue`      | 更新设置 UI                    |
| `workers/ccar-update/src/index.ts`                        | CF Worker 路由 + 缓存          |
| `workers/ccar-update/wrangler.toml`                       | Worker 配置、zone、环境变量    |
| `scripts/release.ps1`                                     | 一键构建 + 上传 + CF Purge     |
| `%USERPROFILE%\.tauri\ccar-copilot-updater.key`           | **私钥**,永远保密             |
