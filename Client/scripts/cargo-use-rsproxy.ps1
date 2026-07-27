# Optional: point crates-io at rsproxy sparse (online China).
$cfg = Join-Path $env:USERPROFILE ".cargo\config.toml"
$bak = "$cfg.bak-before-rsproxy"
if (Test-Path $cfg) { Copy-Item $cfg $bak -Force }
@"
[source.crates-io]
replace-with = "rsproxy-sparse"

[source.rsproxy-sparse]
registry = "sparse+https://rsproxy.cn/index/"

[net]
git-fetch-with-cli = true
"@ | Set-Content -Encoding utf8 $cfg
Write-Host "Configured rsproxy-sparse (backup: $bak)"
