# 虎哥截图 Python Sidecar 自动化构建脚本

$ErrorActionPreference = "Stop"

$ProjectRoot = Resolve-Path "$PSScriptRoot/.."
$PythonDir = Join-Path $ProjectRoot "python"
$BinariesDir = Join-Path $ProjectRoot "src-tauri/binaries"
$DebugTargetDir = Join-Path $ProjectRoot "src-tauri/target/debug"

Write-Host "🚀 开始构建 Python Sidecar..." -ForegroundColor Cyan

# 1. 检查 Python 环境
Write-Host "📦 检查 Python 环境..."
Set-Location $PythonDir

# 2. 运行 PyInstaller
Write-Host "🔨 正在打包 (PyInstaller)..." -ForegroundColor Yellow
python -m PyInstaller huge_sidecar.spec --clean --noconfirm
if ($LASTEXITCODE -ne 0) {
    Write-Error "PyInstaller 打包失败！"
    exit 1
}

# 3. 准备目标目录
Write-Host "📂 清理旧文件..."
$SidecarExeName = "huge_sidecar-x86_64-pc-windows-msvc.exe"
$DistDir = Join-Path $PythonDir "dist/huge_sidecar"

# 清理 binaries 目录
if (Test-Path "$BinariesDir/_internal") { Remove-Item "$BinariesDir/_internal" -Recurse -Force }
if (Test-Path "$BinariesDir/$SidecarExeName") { Remove-Item "$BinariesDir/$SidecarExeName" -Force }

# 4. 复制到 binaries (Tauri 构建用)
Write-Host "✅ 部署到 binaries 目录..." -ForegroundColor Green
Copy-Item "$DistDir/_internal" "$BinariesDir" -Recurse
Copy-Item "$DistDir/huge_sidecar.exe" "$BinariesDir/$SidecarExeName"

# 5. 同步到开发环境 (Tauri Dev 用)
# 如果 target/debug 目录存在（说明运行过 dev），则同步更新，避免"找不到 DLL"错误
if (Test-Path $DebugTargetDir) {
    Write-Host "🔄 同步到开发环境 (target/debug)..." -ForegroundColor Magenta

    # 清理 debug 目录旧文件
    if (Test-Path "$DebugTargetDir/_internal") { Remove-Item "$DebugTargetDir/_internal" -Recurse -Force }
    if (Test-Path "$DebugTargetDir/huge_sidecar.exe") { Remove-Item "$DebugTargetDir/huge_sidecar.exe" -Force }

    # 复制新文件
    Copy-Item "$DistDir/_internal" "$DebugTargetDir" -Recurse
    # 注意：Tauri 在 dev 模式下运行时会查找 target/debug/huge_sidecar.exe (无后缀或带后缀取决于 Tauri 版本，保留原名更安全)
    Copy-Item "$DistDir/huge_sidecar.exe" "$DebugTargetDir/huge_sidecar.exe"

    Write-Host "   已更新开发环境依赖"
}

Write-Host "🎉 Sidecar 构建并部署完成！" -ForegroundColor Cyan
