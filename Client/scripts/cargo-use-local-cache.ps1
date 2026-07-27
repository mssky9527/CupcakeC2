# Temporarily disable global crates-io replace-with so offline builds use
# %USERPROFILE%\.cargo\registry\src\index.crates.io-*\aes-gcm-*
param(
    [switch]$Test
)
$ErrorActionPreference = "Stop"
$cfg = Join-Path $env:USERPROFILE ".cargo\config.toml"
$bak = "$cfg.bak-cupcake"
$root = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
if (-not (Test-Path (Join-Path $PSScriptRoot "..\core\Cargo.toml"))) {
    $core = Join-Path $PSScriptRoot "..\core"
} else {
    $core = Join-Path $PSScriptRoot "..\core"
}
$core = (Resolve-Path $core).Path

if (Test-Path $cfg) {
    Copy-Item $cfg $bak -Force
    @"
[net]
git-fetch-with-cli = true
"@ | Set-Content -Encoding utf8 $cfg
    Write-Host "Disabled crates-io replace-with (backup: $bak)"
} else {
    Write-Host "No user cargo config; using default crates.io"
}

Write-Host "Core path: $core"
Write-Host "Run: cargo test --manifest-path core/Cargo.toml --lib --offline"
if ($Test) {
    Push-Location $core
    try {
        cargo test --lib --offline
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    } finally {
        Pop-Location
        if (Test-Path $bak) {
            Move-Item -Force $bak $cfg
            Write-Host "Restored user cargo config"
        }
    }
} else {
    Write-Host "Restore later: Move-Item -Force `"$bak`" `"$cfg`""
    Write-Host "Or re-run with -Test to auto-restore after cargo test"
}
