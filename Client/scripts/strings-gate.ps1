# Fail if high-signal brand / protocol strings remain in a release binary.
param(
    [Parameter(Mandatory = $true)]
    [string]$Path
)
$ErrorActionPreference = "Stop"
if (-not (Test-Path $Path)) {
    Write-Error "file not found: $Path"
    exit 2
}
$bytes = [IO.File]::ReadAllBytes((Resolve-Path $Path))
$text = [Text.Encoding]::ASCII.GetString($bytes)
$banned = @(
    'cupcake-noise-v2',
    'cupcake-mod-key-v1',
    'CUPCAKE_MODULE_KEY',
    'CupcakeC2',
    '[Cupcake]',
    # Legacy ASCII protocol brands (must be seed-derived now)
    'CKMS',
    'CKF1',
    'CIS1'
)
$hits = @()
foreach ($s in $banned) {
    if ($text.Contains($s)) { $hits += $s }
}
if ($hits.Count -gt 0) {
    Write-Host "STRINGS GATE FAIL: $Path" -ForegroundColor Red
    $hits | ForEach-Object { Write-Host "  HIT: $_" }
    exit 1
}
Write-Host "STRINGS GATE OK: $Path" -ForegroundColor Green
exit 0
