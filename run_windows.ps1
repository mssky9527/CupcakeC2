# Cupcake C2 - Windows 一键启动脚本
# 用于在 Windows 环境下快速部署管理面板与服务端

$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$OutputEncoding = [System.Text.Encoding]::UTF8

$BaseDir = Split-Path -Parent $MyInvocation.MyCommand.Path
Set-Location $BaseDir

Write-Host ""
Write-Host "  =========================================" -ForegroundColor Blue
Write-Host "      Cupcake C2 - 服务端启动脚本 (Windows)  " -ForegroundColor Blue
Write-Host "  =========================================" -ForegroundColor Blue
Write-Host ""

# 1. 环境检查
Write-Host "[*] 正在检查开发环境..." -ForegroundColor Yellow

# Check Go
if (-not (Get-Command go -ErrorAction SilentlyContinue)) {
    Write-Host "[!] 错误: 未检测到 Go 环境。请从 https://go.dev/dl/ 安装。" -ForegroundColor Red
    exit 1
}
Write-Host "  [OK] Go 已就绪: $(go version)" -ForegroundColor Green

# Check Rust
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Host "[!] 错误: 未检测到 Rust 环境。请从 https://rustup.rs/ 安装。" -ForegroundColor Red
    exit 1
}
Write-Host "  [OK] Rust 已就绪: $(rustc --version)" -ForegroundColor Green

# Check Node.js
if (-not (Get-Command npm -ErrorAction SilentlyContinue)) {
    Write-Host "[!] 错误: 未检测到 Node.js 环境。请从 https://nodejs.org/ 安装。" -ForegroundColor Red
    exit 1
}
Write-Host "  [OK] Node.js/NPM 已就绪: $(npm --version)" -ForegroundColor Green

# 2. 初始化目录
Write-Host "[*] 正在初始化存储目录..." -ForegroundColor Yellow
$Dirs = @("server\storage\payloads", "server\storage\backups", "server\assets")
foreach ($dir in $Dirs) {
    if (-not (Test-Path (Join-Path $BaseDir $dir))) {
        New-Item -ItemType Directory -Path (Join-Path $BaseDir $dir) | Out-Null
    }
}

# 3. 编译前端
if (-not (Test-Path "server\ui")) {
    Write-Host "[*] 未检测到前端 UI 目录，尝试从源码构建..." -ForegroundColor Yellow
    $FrontendDir = "server\frontend-v2"
    if (Test-Path $FrontendDir) {
        Push-Location $FrontendDir
        Write-Host "[*] 正在安装前端依赖 (npm install)..." -ForegroundColor Gray
        npm install
        Write-Host "[*] 正在构建前端 (npm run build)..." -ForegroundColor Gray
        npm run build
        
        $DistDir = "dist"
        if (Test-Path $DistDir) {
            Write-Host "[*] 同步构建产物至 server\ui..." -ForegroundColor Green
            if (Test-Path "..\ui") { Remove-Item "..\ui" -Recurse -Force }
            Copy-Item -Path "$DistDir\*" -Destination "..\ui" -Recurse -Force
        } else {
            Write-Host "[!] 错误: 未能找到编译产物目录 (dist)。" -ForegroundColor Red
        }
        Pop-Location
    } else {
        Write-Host "[!] 警告: 未找到前端源码，请确保 server\ui 目录已预编译。" -ForegroundColor Red
    }
}

# 4. 生成 Agent 模板
if (-not (Test-Path "server\assets\client_template_windows.exe")) {
    Write-Host "[*] 未检测到 Agent 模板，建议执行首次生成..." -ForegroundColor Yellow
    $choice = Read-Host "是否现在编译 Agent 模板? (y/N)"
    if ($choice -eq "y") {
        & .\compile_windows.ps1
    }
} else {
    Write-Host "  [OK] Agent 模板已存在。" -ForegroundColor Green
    $choice = Read-Host "是否重新编译模板? (y/N)"
    if ($choice -eq "y") {
        & .\compile_windows.ps1
    }
}

# 5. 启动服务端
Write-Host ""
Write-Host "[+] 环境初始化完成，准备启动程序..." -ForegroundColor Green
Write-Host "[+] 控制终端: http://127.0.0.1:9999" -ForegroundColor Green
Write-Host ""

Set-Location "server"
Write-Host "[*] 正在编译并运行服务端..." -ForegroundColor Yellow
go run main.go
