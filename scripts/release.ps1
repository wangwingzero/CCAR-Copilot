#Requires -Version 7.0
<#
.SYNOPSIS
    CCAR Copilot 一键发布脚本

.DESCRIPTION
    对小团队/私用场景设计,目标是一条命令完成全部发布:
        pwsh -File scripts/release.ps1

    脚本启动时会自动 dot-source 同目录下的 release.env.ps1,
    把所有凭证(签名密钥路径、SSH、Cloudflare 认证)从一个本地文件里读进来,
    这样你不需要每次手动 $env:xxx=... 堆一堆。

    执行步骤:
      1. 加载 scripts/release.env.ps1(凭证)
      2. 校验签名密钥与必需环境变量
      3. tauri build 产出已签名的 NSIS 安装包
      4. 解析签名 + 生成 latest.json
      5. scp 上传到源站 ccar-dl.hudawang.cn
      6. Cloudflare purge_cache 让边缘缓存立即失效

.PARAMETER SkipBuild
    跳过 tauri build(只重新生成 manifest + 上传 + purge)。适合 manifest 改错重发。

.PARAMETER SkipUpload
    只生成本地 latest.json,不上传、不 purge。

.PARAMETER SkipPurge
    跳过 Cloudflare 缓存清理(等 60s 自然过期)。

.PARAMETER Notes
    发布说明(Markdown)。留空则读 RELEASE_NOTES.md,再没有就用空字符串。

.PARAMETER PublishPubDate
    发布时间(ISO 8601),默认当前 UTC。

.EXAMPLE
    # 首次使用:
    Copy-Item scripts/release.env.ps1.example scripts/release.env.ps1
    notepad scripts/release.env.ps1   # 填真实凭证,保存
    pwsh -File scripts/release.ps1    # 完成整条流水线
#>

[CmdletBinding()]
param(
    [switch]$SkipBuild,
    [switch]$SkipUpload,
    [switch]$SkipPurge,
    [string]$Notes,
    [string]$PublishPubDate
)

$ErrorActionPreference = 'Stop'
$ProgressPreference    = 'SilentlyContinue'

function Write-Step {
    param([string]$Message)
    Write-Host ''
    Write-Host "==> $Message" -ForegroundColor Cyan
}

function Fail {
    param([string]$Message)
    Write-Host "ERROR: $Message" -ForegroundColor Red
    exit 1
}

function Require-Env {
    param([string]$Name, [string]$Hint)
    $value = [Environment]::GetEnvironmentVariable($Name)
    if ([string]::IsNullOrWhiteSpace($value)) {
        Fail "环境变量 $Name 未设置。$Hint"
    }
    return $value
}

# -------------------------------------------------------------------------
# 0. 自动加载本地凭证文件
# -------------------------------------------------------------------------
$envFile = Join-Path $PSScriptRoot 'release.env.ps1'
if (Test-Path $envFile) {
    Write-Host "加载凭证: $envFile" -ForegroundColor DarkGray
    . $envFile
}
else {
    Write-Host ''
    Write-Host "未找到 $envFile" -ForegroundColor Yellow
    Write-Host "请先复制模板并填入真实凭证:" -ForegroundColor Yellow
    Write-Host "    Copy-Item scripts/release.env.ps1.example scripts/release.env.ps1" -ForegroundColor Yellow
    Write-Host "    notepad scripts/release.env.ps1" -ForegroundColor Yellow
    Fail "缺少发布凭证文件"
}

# 若用户历史上设过 TAURI_SIGNING_PRIVATE_KEY 为路径字符串,会让 tauri build
# 把路径本身当成密钥 base64 去 decode 而失败,这里统一清掉,稍后再以"文件内容"
# 的正确形式重新注入。
if ($env:TAURI_SIGNING_PRIVATE_KEY) {
    Write-Host '清除外部 TAURI_SIGNING_PRIVATE_KEY,稍后由脚本按文件内容重新注入' -ForegroundColor DarkGray
    Remove-Item Env:TAURI_SIGNING_PRIVATE_KEY -ErrorAction SilentlyContinue
}

# -------------------------------------------------------------------------
# 1. 基础参数
# -------------------------------------------------------------------------
$repoRoot = Resolve-Path (Join-Path $PSScriptRoot '..')
Set-Location $repoRoot

$pkgJsonPath = Join-Path $repoRoot 'package.json'
if (-not (Test-Path $pkgJsonPath)) {
    Fail "找不到 package.json: $pkgJsonPath"
}
$pkg = Get-Content $pkgJsonPath -Raw | ConvertFrom-Json
$version = $pkg.version
if ([string]::IsNullOrWhiteSpace($version)) {
    Fail 'package.json 未定义 version 字段'
}
Write-Step "发布版本: $version"

if ([string]::IsNullOrWhiteSpace($PublishPubDate)) {
    $PublishPubDate = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ')
}

if ([string]::IsNullOrWhiteSpace($Notes)) {
    $notesPath = Join-Path $repoRoot 'RELEASE_NOTES.md'
    if (Test-Path $notesPath) {
        $Notes = (Get-Content $notesPath -Raw).Trim()
        Write-Host "发布说明来自 $notesPath ($($Notes.Length) 字符)"
    }
    else {
        $Notes = ''
    }
}

# -------------------------------------------------------------------------
# 2. 构建(含签名)
# -------------------------------------------------------------------------
if (-not $SkipBuild) {
    $privateKeyPath = Require-Env 'TAURI_SIGNING_PRIVATE_KEY_PATH' '指向 ed25519 私钥文件的绝对路径'
    if (-not (Test-Path $privateKeyPath)) {
        Fail "签名私钥不存在: $privateKeyPath"
    }

    # ⚠️ Tauri CLI 的坑: `tauri build` 在 bundle 签名阶段只认 TAURI_SIGNING_PRIVATE_KEY
    # (字符串模式),不识别 TAURI_SIGNING_PRIVATE_KEY_PATH。虽然 `tauri signer sign`
    # 独立子命令两者都认。所以这里必须把文件内容读出来注入到字符串变量。
    $env:TAURI_SIGNING_PRIVATE_KEY = Get-Content $privateKeyPath -Raw
    if ($null -eq $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) {
        # 即使密钥无密码,Tauri 仍会读此变量,显式给空串避免交互式 prompt。
        $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = ''
    }

    Write-Step '执行 tauri build (npm run build → cargo build → NSIS 打包 + 签名)'
    npm run tauri -- build
    if ($LASTEXITCODE -ne 0) {
        Fail 'tauri build 失败'
    }
}
else {
    Write-Step '跳过 tauri build (--SkipBuild)'
}

# -------------------------------------------------------------------------
# 3. 定位产物
# -------------------------------------------------------------------------
$bundleDir = Join-Path $repoRoot 'src-tauri\target\release\bundle\nsis'
if (-not (Test-Path $bundleDir)) {
    Fail "找不到 NSIS 产物目录: $bundleDir。请先构建或检查 tauri bundle targets。"
}

# Tauri v2 的 NSIS bundle 直接对 setup.exe 签名作为 updater artifact,
# 不再生成独立的 .nsis.zip 更新包。所以 setup.exe 同时扮演首次安装与更新包双重角色。
$setupExe = Get-ChildItem -Path $bundleDir -Filter "*$version*setup.exe"     -File | Select-Object -First 1
$setupSig = Get-ChildItem -Path $bundleDir -Filter "*$version*setup.exe.sig" -File | Select-Object -First 1

if (-not $setupExe) { Fail "找不到 setup.exe (*$version*setup.exe) in $bundleDir" }
if (-not $setupSig) { Fail "找不到签名文件 (*$version*setup.exe.sig)" }

$signature = (Get-Content $setupSig.FullName -Raw).Trim()
if ([string]::IsNullOrWhiteSpace($signature)) {
    Fail "签名文件为空: $($setupSig.FullName)"
}
Write-Host "setup.exe : $($setupExe.Name) ($([math]::Round($setupExe.Length/1MB, 2)) MB)"
Write-Host "签名长度   : $($signature.Length) 字符"

# -------------------------------------------------------------------------
# 4. 生成 latest.json
# -------------------------------------------------------------------------
Write-Step '生成 latest.json'

# latest.json 里写源站 URL;CF Worker 会在返回 manifest 时把域名改写成
# ccar-update.031986.xyz,所以 CF 通道依然能吃 CDN 加速。
# 客户端走源站直连 fallback 时,拿到的就是未改写的源站 URL。
$originDownloadBase = 'https://ccar-dl.hudawang.cn/downloads'
$cfDownloadBase     = 'https://ccar-update.031986.xyz/downloads'
$pkgUrl             = "$originDownloadBase/$($setupExe.Name)"
$cfPkgUrl           = "$cfDownloadBase/$($setupExe.Name)"

$manifest = [ordered]@{
    version   = $version
    notes     = $Notes
    pub_date  = $PublishPubDate
    platforms = [ordered]@{
        'windows-x86_64' = [ordered]@{
            signature = $signature
            url       = $pkgUrl
        }
    }
}

$latestJsonPath = Join-Path $bundleDir 'latest.json'
$manifest | ConvertTo-Json -Depth 6 | Out-File -FilePath $latestJsonPath -Encoding utf8 -NoNewline
Write-Host "写入 $latestJsonPath"

# -------------------------------------------------------------------------
# 5. 上传到源站
# -------------------------------------------------------------------------
if (-not $SkipUpload) {
    $sshUser = if ($env:RELEASE_SSH_USER) { $env:RELEASE_SSH_USER } else { 'root' }
    $sshHost = if ($env:RELEASE_SSH_HOST) { $env:RELEASE_SSH_HOST } else { '154.9.27.44' }
    $sshPort = if ($env:RELEASE_SSH_PORT) { $env:RELEASE_SSH_PORT } else { '7668' }
    $sshKey  = Require-Env 'RELEASE_SSH_KEY' '指向可登录源站的 SSH 私钥路径'
    $remoteDir = if ($env:RELEASE_REMOTE_DIR) { $env:RELEASE_REMOTE_DIR } else { '/www/wwwroot/ccar-release' }

    if (-not (Test-Path $sshKey)) { Fail "SSH 私钥不存在: $sshKey" }

    Write-Step "上传到 $sshUser@${sshHost}:$remoteDir"

    # 确保远程 downloads/ 目录存在
    & ssh -i "$sshKey" -p $sshPort "$sshUser@$sshHost" "mkdir -p '$remoteDir/downloads'"
    if ($LASTEXITCODE -ne 0) { Fail '创建远程 downloads/ 目录失败' }

    # scp 上传 artifacts 与 manifest
    # Tauri 2: setup.exe 同时是首次安装与 updater 包,setup.exe.sig 供校验参考
    $targets = @(
        @{ Local = $setupExe.FullName; Remote = "$remoteDir/downloads/" },
        @{ Local = $setupSig.FullName; Remote = "$remoteDir/downloads/" },
        @{ Local = $latestJsonPath;    Remote = "$remoteDir/latest.json" }
    )
    foreach ($t in $targets) {
        Write-Host "  scp $($t.Local) -> $($t.Remote)"
        & scp -i "$sshKey" -P $sshPort "$($t.Local)" "$sshUser@${sshHost}:$($t.Remote)"
        if ($LASTEXITCODE -ne 0) { Fail "scp 失败: $($t.Local)" }
    }
    Write-Host '上传完成'
}
else {
    Write-Step '跳过上传 (--SkipUpload)'
}

# -------------------------------------------------------------------------
# 6. 清 Cloudflare 缓存
# -------------------------------------------------------------------------
if (-not $SkipPurge -and -not $SkipUpload) {
    $cfZone = Require-Env 'CF_ZONE_ID' '031986.xyz 的 zone id'

    # 认证二选一: Bearer Token 优先, 否则 Email + Global API Key.
    if ($env:CF_API_TOKEN) {
        $cfHeaders = @{
            'Authorization' = "Bearer $($env:CF_API_TOKEN)"
            'Content-Type'  = 'application/json'
        }
        $cfAuthMode = 'Bearer Token'
    }
    elseif ($env:CF_EMAIL -and $env:CF_API_KEY) {
        $cfHeaders = @{
            'X-Auth-Email' = $env:CF_EMAIL
            'X-Auth-Key'   = $env:CF_API_KEY
            'Content-Type' = 'application/json'
        }
        $cfAuthMode = 'Global API Key'
    }
    else {
        Fail '未配置 CF 认证: 需要设置 CF_API_TOKEN 或 (CF_EMAIL + CF_API_KEY)'
    }

    Write-Step "Purge Cloudflare 缓存 (auth: $cfAuthMode)"
    # Purge 的是 CF 边缘缓存,所以清的是 ccar-update.031986.xyz 这个域。
    # 源站直连不走 CF,无需 purge。
    $body = @{
        files = @(
            'https://ccar-update.031986.xyz/latest.json',
            $cfPkgUrl
        )
    } | ConvertTo-Json -Depth 4

    $resp = Invoke-RestMethod `
        -Uri "https://api.cloudflare.com/client/v4/zones/$cfZone/purge_cache" `
        -Method POST `
        -Headers $cfHeaders `
        -Body $body

    if (-not $resp.success) {
        Fail "Purge 失败: $(($resp.errors | ConvertTo-Json -Depth 4))"
    }
    Write-Host "Purge 成功: $($resp.result | ConvertTo-Json -Compress)"
}
elseif ($SkipPurge) {
    Write-Step '跳过 Cloudflare Purge (--SkipPurge)'
}
else {
    Write-Step '跳过 Cloudflare Purge (因为跳过了上传)'
}

# -------------------------------------------------------------------------
# 7. CF 边缘缓存预热(让用户第一次下载就走 HIT)
# -------------------------------------------------------------------------
#
# 背景: 源站 156 在香港,大陆中国移动晚高峰跨境出口只有 70 KB/s,直连源站慢得
# 让人没耐心。Cloudflare 边缘节点会缓存 setup.exe,但首次必须 MISS 回源,**就
# 是这次回源把用户卡在 70 KB/s**。
#
# 思路: release 上传完成 + purge 之后,我们主动让 CF 在多个常用 PoP 都拉一份
# 到缓存里。下次用户下载时直接命中边缘,跳过 156 -> CF 这一段跨境链路。
#
# - 7.1 从 156 香港服务器 GET 一次 setup.exe + manifest:156 -> CF HK 是同区
#       链路,几秒内就能预热 HK 节点(大陆移动用户最可能命中 HK)
# - 7.2 从本地(release 机器) HEAD 一次 manifest 验证 CF 是否真的 HIT
#
# 不在 SkipUpload 时跳过(因为没新内容)。
if (-not $SkipUpload) {
    Write-Step 'CF 边缘缓存预热(让用户首次下载就走 HIT)'

    # 7.1 从 156 服务器拉一次 CF URL,预热 CF HK PoP
    #     156 -> CF 通常走 HK 数据中心,~50-200 MB/s 回源,30s 内就能预热好。
    Write-Host '  [1/2] 156 -> CF (预热 HK / 亚洲 PoP)' -ForegroundColor DarkGray
    $sshUser = if ($env:RELEASE_SSH_USER) { $env:RELEASE_SSH_USER } else { 'root' }
    $sshHost = if ($env:RELEASE_SSH_HOST) { $env:RELEASE_SSH_HOST } else { '154.9.27.44' }
    $sshPort = if ($env:RELEASE_SSH_PORT) { $env:RELEASE_SSH_PORT } else { '7668' }
    $sshKeyPath = $env:RELEASE_SSH_KEY

    # 用 single-quote here-string 避免 PowerShell 转义干扰 bash 反引号 / $()。
    # CF HK 出现 cf-cache-status: HIT 之前可能要多 GET 几次,因为 CF 有时会
    # 在第一次 GET 后才把对象写入 cache。这里 GET 三次,中间 1 秒间隔。
    $prefetchScript = @'
for i in 1 2 3; do
  curl -sk --max-time 90 -o /dev/null \
    -D /tmp/cf-prefetch-headers.txt \
    -w "    GET #$i: http=%{http_code} size=%{size_download}B time=%{time_total}s speed=%{speed_download}B/s\n" \
    "__CF_PKG_URL__"
  status=$(grep -i '^cf-cache-status:' /tmp/cf-prefetch-headers.txt | tr -d '\r' | awk '{print $2}')
  ray=$(grep -i '^cf-ray:' /tmp/cf-prefetch-headers.txt | tr -d '\r' | awk '{print $2}')
  echo "       cf-cache-status=$status cf-ray=$ray"
  if [ "$status" = "HIT" ]; then
    echo "    CF HK PoP 已缓存 (CF-Cache-Status: HIT)"
    break
  fi
  sleep 1
done
curl -sk --max-time 30 -o /dev/null \
  -D /tmp/cf-prefetch-headers.txt \
  "__CF_MANIFEST_URL__"
manifest_status=$(grep -i '^cf-cache-status:' /tmp/cf-prefetch-headers.txt | tr -d '\r' | awk '{print $2}')
echo "    manifest cache=$manifest_status"
rm -f /tmp/cf-prefetch-headers.txt
'@
    # Tauri 产物文件名带空格,curl 在 bash 双引号里能处理但保险起见提前 URL
    # encode 空格为 %20,避免任何 shell 转义意外
    $encodedCfPkgUrl = $cfPkgUrl -replace ' ', '%20'
    $prefetchScript = $prefetchScript.Replace('__CF_PKG_URL__', $encodedCfPkgUrl)
    $prefetchScript = $prefetchScript.Replace('__CF_MANIFEST_URL__', 'https://ccar-update.031986.xyz/latest.json')

    if ($sshKeyPath -and (Test-Path $sshKeyPath)) {
        # 通过 stdin 传脚本避免 -c 单行命令的引号问题
        $prefetchScript | & ssh -i "$sshKeyPath" -p $sshPort -o ConnectTimeout=15 "$sshUser@$sshHost" 'bash -s'
        if ($LASTEXITCODE -ne 0) {
            Write-Host '  WARN: SSH 预热失败(非致命),用户首次下载可能走 MISS' -ForegroundColor Yellow
        }
    }
    else {
        Write-Host '  WARN: 跳过 156 预热(RELEASE_SSH_KEY 不可用)' -ForegroundColor Yellow
    }

    # 7.2 本地验证当前 CF 状态(本地命中的 PoP 可能不是 HK,但能侧面反映 CF 全
    #     局缓存是否填充)
    Write-Host '  [2/2] 本地 -> CF (验证当前 cache 状态)' -ForegroundColor DarkGray
    $localHead = & curl.exe -sIk --max-time 15 "$cfPkgUrl" 2>$null
    $localHead | Select-String -Pattern '^(HTTP|CF-Cache-Status|CF-Ray|Age|Content-Length):' | ForEach-Object {
        Write-Host "    $($_.Line.Trim())"
    }
}
else {
    Write-Step '跳过 CF 预热 (因为跳过了上传)'
}

# -------------------------------------------------------------------------
# 8. 完成
# -------------------------------------------------------------------------
Write-Step '发布完成'
Write-Host ''
Write-Host "版本: $version"
Write-Host "时间: $PublishPubDate"
Write-Host ''
Write-Host '验证命令(两条通道都应返回 200):'
Write-Host '  # 主通道 (CF Worker, 第一优先)'
Write-Host '  curl.exe -sk https://ccar-update.031986.xyz/latest.json'
Write-Host "  curl.exe -sIk $cfPkgUrl"
Write-Host ''
Write-Host '  # 备通道 (源站直连, CF 不可达时 fallback)'
Write-Host '  curl.exe -sk https://ccar-dl.hudawang.cn/latest.json'
Write-Host "  curl.exe -sIk $pkgUrl"
Write-Host ''
