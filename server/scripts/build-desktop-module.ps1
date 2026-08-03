# Build L2 remote-desktop module and install as storage/modules/desktop.bin
#
#   powershell -File scripts/build-desktop-module.ps1
#
# Module is a capability package: Stage0 desktop_bridge only relays RDP while
# this DLL is Loaded (no GDI capture).

$ErrorActionPreference = "Stop"
$Root = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not (Test-Path (Join-Path $Root "Client\Cargo.toml"))) {
    $Root = Split-Path -Parent $PSScriptRoot
}
$Client = Join-Path $Root "Client"
$OutDir = Join-Path $Root "server\storage\modules"
$DllName = "cupcake_mod_desktop.dll"

if (-not (Test-Path $Client)) {
    throw "Client workspace not found: $Client"
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

Push-Location $Client
try {
    Write-Host "[*] cargo build -p cupcake-mod-desktop --release"
    cargo build -p cupcake-mod-desktop --release
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

    $src = Join-Path $Client "target\release\$DllName"
    if (-not (Test-Path $src)) {
        throw "DLL not found: $src"
    }
    $dst = Join-Path $OutDir "desktop.bin"
    Copy-Item -Force $src $dst
    $len = (Get-Item $dst).Length
    Write-Host "[+] Installed $dst ($len bytes)"
    Write-Host "    Load from Module panel as id=desktop before opening Remote Desktop."
}
finally {
    Pop-Location
}
