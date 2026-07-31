# Safe unit tests for package services — avoids AV-quarantine of services.test.exe
#
# Root cause: linking github.com/Binject/go-donut into the package test binary
# produces a PE that Defender/AV commonly deletes mid-run (exit 0xffffffff).
#
# Defaults:
#   -tags nodonut     → stub ToShellcodeFromBytes, no go-donut in the test binary
#   binary under %TEMP% only if -Compile is set; never leave *.test.exe in server/
#
# Usage:
#   powershell -File scripts/test-services.ps1
#   powershell -File scripts/test-services.ps1 -WithDonut   # full Donut (may be killed by AV)
#   powershell -File scripts/test-services.ps1 -Compile     # write test binary to TEMP then run

param(
    [switch]$WithDonut,
    [switch]$Compile,
    [string]$Run = "",
    [int]$TimeoutSec = 120
)

$ErrorActionPreference = "Stop"
$ServerRoot = Split-Path -Parent $PSScriptRoot
Set-Location $ServerRoot

# Purge leftover test binaries under server/ (common AV magnet)
Get-ChildItem -Path $ServerRoot -Filter "*.test.exe" -File -ErrorAction SilentlyContinue |
    ForEach-Object {
        Write-Host "[-] Removing leftover $($_.Name)"
        Remove-Item -Force $_.FullName -ErrorAction SilentlyContinue
    }
Get-ChildItem -Path $ServerRoot -Filter "services.test.exe" -File -ErrorAction SilentlyContinue |
    ForEach-Object { Remove-Item -Force $_.FullName -ErrorAction SilentlyContinue }

$tags = @()
if (-not $WithDonut) {
    $tags += "nodonut"
    Write-Host "[*] Safe mode: -tags nodonut (no go-donut in test binary)"
} else {
    Write-Host "[!] WithDonut: test binary includes go-donut — AV may quarantine it"
}

$tagArgs = @()
if ($tags.Count -gt 0) {
    $tagArgs = @("-tags", ($tags -join ","))
}

$runArgs = @()
if ($Run -ne "") {
    $runArgs = @("-run", $Run)
}

if ($Compile) {
    $out = Join-Path $env:TEMP ("cupcake_services_test_{0}.exe" -f [guid]::NewGuid().ToString("N").Substring(0, 8))
    Write-Host "[*] Compiling test binary to $out"
    & go test ./services/ @tagArgs -c -o $out
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    try {
        $args = @("-test.count=1", "-test.timeout=${TimeoutSec}s")
        if ($Run -ne "") { $args += "-test.run=$Run" }
        & $out @args
        exit $LASTEXITCODE
    } finally {
        Remove-Item -Force $out -ErrorAction SilentlyContinue
        Write-Host "[*] Deleted temp test binary"
    }
} else {
    # go test keeps the binary in the module cache temp dir, not as server/services.test.exe
    & go test ./services/ @tagArgs @runArgs -count=1 -timeout "${TimeoutSec}s"
    exit $LASTEXITCODE
}
