# Cupcake C2 - 服务端与前端全量编译脚本 (Windows)
# 作用: 自动编译前端 frontend-v2 并嵌入/输出到 server/dist，然后编译 Go 服务端可执行文件

$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$OutputEncoding = [System.Text.Encoding]::UTF8

$BaseDir = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $BaseDir

Write-Host ""
Write-Host "  =========================================" -ForegroundColor Blue
Write-Host "     Cupcake C2 - 服务端与前端编译脚本     " -ForegroundColor Blue
Write-Host "  =========================================" -ForegroundColor Blue
Write-Host ""

# 1. 检查依赖
Write-Host "[*] 正在检查编译环境..." -ForegroundColor Yellow

if (-not (Get-Command npm -ErrorAction SilentlyContinue)) {
    Write-Host "[!] 错误: 未检测到 Node.js/NPM，请先安装 Node.js。" -ForegroundColor Red
    exit 1
}
$npmVer = npm --version
Write-Host "  [OK] Node.js/NPM 已就绪: $npmVer" -ForegroundColor Green

if (-not (Get-Command go -ErrorAction SilentlyContinue)) {
    Write-Host "[!] 错误: 未检测到 Go 环境，请先安装 Go 环境。" -ForegroundColor Red
    exit 1
}
$goVer = go version
Write-Host "  [OK] Go 已就绪: $goVer" -ForegroundColor Green
Write-Host ""

# 2. 初始化必需目录
Write-Host "[*] 正在检查并创建所需目录..." -ForegroundColor Yellow
$Dirs = @("server\storage\payloads", "server\storage\backups", "server\assets", "server\dist")
foreach ($dir in $Dirs) {
    $fullPath = Join-Path $BaseDir $dir
    if (-not (Test-Path $fullPath)) {
        New-Item -ItemType Directory -Path $fullPath | Out-Null
    }
}

# 3. 编译前端 (frontend-v2)
$FrontendDir = Join-Path $BaseDir "server\frontend-v2"
$DistDir     = Join-Path $BaseDir "server\dist"
$UiDir       = Join-Path $BaseDir "server\ui"

if (Test-Path $FrontendDir) {
    Write-Host "[*] [1/2] 正在编译前端 Vue 应用 (frontend-v2)..." -ForegroundColor Cyan
    Push-Location $FrontendDir
    try {
        if (-not (Test-Path "node_modules")) {
            Write-Host "  [*] 正在安装前端依赖 (npm install)..." -ForegroundColor Gray
            npm install
            if ($LASTEXITCODE -ne 0) { throw "npm install 失败" }
        }
        Write-Host "  [*] 正在构建前端静态文件 (npm run build)..." -ForegroundColor Gray
        npm run build
        if ($LASTEXITCODE -ne 0) { throw "npm run build 失败" }
    } finally {
        Pop-Location
    }

    $IndexHtml = Join-Path $DistDir "index.html"
    if (Test-Path $IndexHtml) {
        Write-Host "  [OK] 前端编译成功，产物存放在 server\dist" -ForegroundColor Green
        
        # 同步产物至 server\ui 备用
        Write-Host "  [*] 同步产物至 server\ui 备用..." -ForegroundColor Gray
        if (Test-Path $UiDir) { Remove-Item $UiDir -Recurse -Force }
        Copy-Item -Path "$DistDir\*" -Destination $UiDir -Recurse -Force
    } else {
        Write-Host "  [!] 错误: 未找到前端产物 $IndexHtml" -ForegroundColor Red
        exit 1
    }
} else {
    Write-Host "[!] 错误: 未找到前端源码目录 $FrontendDir" -ForegroundColor Red
    exit 1
}
Write-Host ""

# 4. 编译 Go 服务端 (server)
Write-Host "[*] [2/2] 正在编译 Go 服务端 (Cupcake Server)..." -ForegroundColor Cyan
$ServerDir = Join-Path $BaseDir "server"
$OutputFile = Join-Path $ServerDir "cupcake-server.exe"

Push-Location $ServerDir
try {
    Write-Host "  [*] 正在整理 Go 依赖 (go mod tidy)..." -ForegroundColor Gray
    go mod tidy

    Write-Host "  [*] 正在生成 Go 服务端二进制 (Hardened Build)..." -ForegroundColor Gray
    $env:CGO_ENABLED = "0"
    go build -v -ldflags="-s -w" -buildvcs=false -trimpath -o "cupcake-server.exe" .
    if ($LASTEXITCODE -ne 0) {
        Write-Host "  [!] 服务端编译失败！" -ForegroundColor Red
        exit 1
    }
} finally {
    Pop-Location
}

if (Test-Path $OutputFile) {
    $item = Get-Item $OutputFile
    $sizeMb = [math]::Round($item.Length / 1MB, 2)
    Write-Host ""
    Write-Host "  =========================================" -ForegroundColor Blue
    Write-Host "  [DONE] 服务端与前端编译完成！" -ForegroundColor Green
    Write-Host "  [+] 服务端产物: $OutputFile ($sizeMb MB)" -ForegroundColor Green
    Write-Host "  [+] 前端静态产物: $DistDir" -ForegroundColor Green
    Write-Host "  =========================================" -ForegroundColor Blue
    Write-Host ""
} else {
    Write-Host "  [!] 编译产物缺失: $OutputFile" -ForegroundColor Red
    exit 1
}
