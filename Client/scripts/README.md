# Client build helpers

## Offline tests (local crate cache)

If `~/.cargo/config.toml` forces a broken mirror (`ustc` via dead proxy):

```powershell
powershell -File scripts/cargo-use-local-cache.ps1
cd core
cargo test --lib --offline
# restore user config when done (script prints path)
```

`cargo-use-local-cache.ps1` strips `replace-with` so Cargo uses the default
`index.crates.io` layout under `%USERPROFILE%\.cargo\registry\src\`.

## Online China mirror (optional)

```powershell
powershell -File scripts/cargo-use-rsproxy.ps1
cargo test --manifest-path core/Cargo.toml --lib
```

Project `Client/.cargo/config.toml` does **not** force a remote replace-with
(so offline cache remains usable).
