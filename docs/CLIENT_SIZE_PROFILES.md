# Client product model — sole tier `minimal`

## Product rule

**Only one Stage0 aggregate: `minimal`.**  
Deleted product tiers: `standard`, `full`, `beacon`.

```text
Stage0 (minimal)
  transport + crypto + module-loader + isolated-exec + Layer-A stealth
  + shell / fs / process / pty / socks

L2 (on demand)
  desktop.bin   → RDP Yamux 0x0D
  iso_host.bin  → BOF / .NET
  inject.bin    → process inject
```

## Cargo

```toml
default = ["ws", "minimal"]
minimal = [
  "post-ex", "pty", "socks", "encoding-support",
  "module-loader", "isolated-exec", "mem-map",
]
```

Builder **always** passes `--features <transport>,minimal`.  
Legacy API values `standard` / `full` / `beacon` are ignored with a log line.

Optional non-product features (manual only): `plugin`, `stealth-adv`, `logging`, `rt-multi`, `bof`, `dotnet`, `inject`, `sleep-mask`.

## Measured size (Windows release, `tcp`, panic=abort, LTO fat)

| Build | Approx. |
|-------|---------|
| `tcp,minimal` | ~0.9 MB |

## Release profile

```toml
[profile.release]
opt-level = "z"
lto = "fat"
codegen-units = 1
panic = "abort"
strip = "symbols"
```
