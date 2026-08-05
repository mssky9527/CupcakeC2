# Lab start: allow unsigned modules/plugins (local testing only).
# Usage:  powershell -File .\start-lab.ps1
# Stop:   Get-Process cupcake-server | Stop-Process

$ErrorActionPreference = "Stop"
$dir = Split-Path -Parent $MyInvocation.MyCommand.Path
$exe = Join-Path $dir "cupcake-server.exe"
if (-not (Test-Path $exe)) {
    Write-Error "Missing $exe — build first (go build -o cupcake-server.exe .)"
}

Get-Process -Name "cupcake-server" -ErrorAction SilentlyContinue | ForEach-Object {
    Write-Host "Stopping existing PID $($_.Id)"
    Stop-Process -Id $_.Id -Force
}
Start-Sleep -Milliseconds 500

$env:CUPCAKE_ALLOW_UNSIGNED_MODULE = "1"
$env:CUPCAKE_ALLOW_UNSIGNED_PLUGIN = "1"
# Optional: also disable all trust-sig requirements
# $env:CUPCAKE_TRUST_REQUIRE_SIG = "0"

Write-Host "Starting $exe"
Write-Host "  CUPCAKE_ALLOW_UNSIGNED_MODULE=1"
Write-Host "  CUPCAKE_ALLOW_UNSIGNED_PLUGIN=1"
Set-Location $dir
& $exe
