# Build Vue frontend-v2 into server/dist (go:embed path).
# Usage:
#   powershell -File scripts/build-frontend.ps1
#   powershell -File scripts/build-frontend.ps1 -SkipInstall

param(
    [switch]$SkipInstall
)

$ErrorActionPreference = "Stop"
$ServerRoot = Split-Path -Parent $PSScriptRoot
$Fe = Join-Path $ServerRoot "frontend-v2"
$Dist = Join-Path $ServerRoot "dist"

if (-not (Test-Path $Fe)) {
    throw "frontend-v2 not found: $Fe"
}

Set-Location $Fe
if (-not $SkipInstall) {
    if (-not (Test-Path "node_modules")) {
        Write-Host "[*] npm ci ..."
        npm ci
    }
}

Write-Host "[*] npm run build → ../dist"
npm run build

if (-not (Test-Path (Join-Path $Dist "index.html"))) {
    throw "build failed: dist/index.html missing"
}

Write-Host "[+] Frontend ready: $Dist"
Write-Host "    Embed: //go:embed dist/* in main.go"
Write-Host "    Legacy server/ui/ is obsolete — do not use."
