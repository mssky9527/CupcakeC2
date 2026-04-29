# Cupcake C2 - Windows Agent Template Compiler
# Compiles all Windows Agent variants and places them in server/assets/

$ErrorActionPreference = "Stop"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$OutputEncoding = [System.Text.Encoding]::UTF8

$BaseDir   = Split-Path -Parent $MyInvocation.MyCommand.Path
$ClientDir = Join-Path $BaseDir "Client"
$AssetsDir = Join-Path $BaseDir "server\assets"

Write-Host ""
Write-Host "  =========================================" -ForegroundColor Blue
Write-Host "    Cupcake C2 - Windows Template Compiler" -ForegroundColor Blue
Write-Host "  =========================================" -ForegroundColor Blue
Write-Host ""

# Ensure assets directory exists
if (-not (Test-Path $AssetsDir)) {
    New-Item -ItemType Directory -Path $AssetsDir | Out-Null
}

# Check Rust environment
Write-Host "[*] Checking build environment..." -ForegroundColor Yellow
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Host "[!] Error: Rust (cargo) not found. Please install Rust first." -ForegroundColor Red
    exit 1
}
Write-Host "  [OK] Rust/cargo found" -ForegroundColor Green
Write-Host ""

# Build function
function Build-Template {
    param (
        [string]$Arch,
        [string]$Feature,
        [string]$OutputName
    )

    $Target = if ($Arch -eq "x64") { "x86_64-pc-windows-msvc" } else { "i686-pc-windows-msvc" }

    Write-Host "[*] Building: $OutputName  (arch=$Arch, feature=$Feature)" -ForegroundColor Yellow

    # Ensure rustup target is installed
    # (temporarily set Continue to suppress rustup's info-level stderr output)
    $prev = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    rustup target add $Target 2>&1 | Out-Null
    $ErrorActionPreference = $prev

    Push-Location $ClientDir
    try {
        # STEALTH: strip local build paths from binary
        $env:RUSTFLAGS = "--remap-path-prefix `"$ClientDir`"=/cupcake"

        cargo build -p cupcake-core --release --target $Target --no-default-features --features $Feature
        if ($LASTEXITCODE -ne 0) {
            Write-Host "  [!] Build failed for $OutputName" -ForegroundColor Red
            exit 1
        }

        $SrcPath  = Join-Path $ClientDir "target\$Target\release\cupcake-core.exe"
        $DestPath = Join-Path $AssetsDir $OutputName

        if (Test-Path $SrcPath) {
            if (Test-Path $DestPath) { Remove-Item $DestPath -Force }
            Copy-Item -Path $SrcPath -Destination $DestPath -Force
            $mb = [math]::Round((Get-Item $DestPath).Length / 1MB, 1)
            Write-Host "  [OK] $OutputName  ($mb MB)" -ForegroundColor Green
        } else {
            Write-Host "  [!] Binary not found at: $SrcPath" -ForegroundColor Red
            exit 1
        }
    } finally {
        Pop-Location
    }
    Write-Host ""
}

# --- Build all Windows variants ---
Write-Host "[*] Starting Windows Agent template builds..." -ForegroundColor Cyan
Write-Host ""

# WebSocket
Build-Template -Arch "x64" -Feature "ws"       -OutputName "client_template_windows.exe"
Build-Template -Arch "x86" -Feature "ws"       -OutputName "client_template_windows_x86.exe"

# Reverse TCP
Build-Template -Arch "x64" -Feature "tcp"      -OutputName "client_template_windows_tcp.exe"

# Bind TCP (forward connect)
Build-Template -Arch "x64" -Feature "tcp_bind" -OutputName "client_template_windows_bind.exe"

# DNS
Build-Template -Arch "x64" -Feature "dns"      -OutputName "client_template_windows_dns.exe"

# Summary
Write-Host "  =========================================" -ForegroundColor Blue
Write-Host "  [DONE] All Windows templates compiled." -ForegroundColor Green
Write-Host "  Output dir: $AssetsDir" -ForegroundColor White
Write-Host "  =========================================" -ForegroundColor Blue
Write-Host ""
