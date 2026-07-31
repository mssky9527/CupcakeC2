# Build L2 process-inject module and install as storage/modules/inject.bin
# Usage:
#   powershell -File scripts/build-inject-module.ps1

$ErrorActionPreference = "Stop"
$ServerRoot = Split-Path -Parent $PSScriptRoot
$ClientRoot = Join-Path (Split-Path -Parent $ServerRoot) "Client"
$OutDir = Join-Path $ServerRoot "storage\modules"
$DllName = "cupcake_mod_inject.dll"

if (-not (Test-Path $ClientRoot)) {
    throw "Client tree not found: $ClientRoot"
}

Write-Host "[*] cargo build -p cupcake-mod-inject --release"
Set-Location $ClientRoot
cargo build -p cupcake-mod-inject --release
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$src = Join-Path $ClientRoot "target\release\$DllName"
if (-not (Test-Path $src)) {
    # MSVC may produce .dll next to deps
    $alt = Get-ChildItem -Path (Join-Path $ClientRoot "target\release") -Filter $DllName -Recurse -ErrorAction SilentlyContinue |
        Select-Object -First 1 -ExpandProperty FullName
    if ($alt) { $src = $alt }
}
if (-not (Test-Path $src)) {
    throw "built DLL not found: $DllName under target/release"
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null
$dst = Join-Path $OutDir "inject.bin"
Copy-Item -Force $src $dst
Write-Host "[+] Installed $dst ($((Get-Item $dst).Length) bytes)"
Write-Host "    Push via Modules UI or auto on module_required:inject"
