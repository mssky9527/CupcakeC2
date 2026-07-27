# Memory load + modular OPSEC plan (implementation spine)

> Status: active · Windows x64 first  
> Full design session: plan mode `plan.md` (mem-map / sleep / fileless)

## Product goals

1. **Mem-map L2 modules** — default Manual-Map; `LoadLibrary` + temp file only as fallback  
2. **Modular heavy ops** — BOF/.NET via `iso_host` isolation; same-process L2 DLL when needed  
3. **SleepCrypto** — optional sleep-time encryption (no full-heap XOR)  
4. **Fileless Stage0** — panel `fileless` profile (required this iteration)

## Confirmed decisions

| Item | Decision |
|------|----------|
| Priority | pe_map → modular UX → Sleep → Fileless |
| socks | **Not** split this iteration (standard keeps built-in) |
| Fileless | **Must-ship** panel profile |

## Features / runtime

| Gate | Default | Meaning |
|------|---------|---------|
| cargo `mem-map` | on with `minimal`/`standard` | Compile Manual-Map path |
| cargo `mem-map-strict` | off | No disk fallback |
| env `CUPCAKE_MEM_MAP=0` | — | Force legacy LoadLibrary |
| env `CUPCAKE_MEM_MAP_STRICT=1` | — | Runtime strict (no fallback) |
| cargo `sleep-mask` | off | Sleep PE/region mask |
| panel `fileless` | off | PIC + Stage2 delivery |

## CKMS flags (u16)

| Bit | Name | Meaning |
|-----|------|---------|
| 0 | `PREF_MEM_MAP` | Prefer Manual-Map |
| 1 | `REQUIRE_MEM_MAP` | Fail if disk map required |
| 2 | reserved (compress) |

## Load path (Windows)

```text
module_stage → verify HMAC →
  iso_host? keep PE in RAM
  else mem-map? pe_map::map → exports → mod_init
  else (fallback) temp + LoadLibrary → delete
```

## File index

| Component | Path |
|-----------|------|
| Manual-Map | `Client/core/src/pe_map.rs` |
| Integration | `Client/core/src/module_loader.rs` |
| Package flags | `Client/core/src/module_package.rs` |
| Sleep mask | `Client/core/src/stealth/mask.rs` |
| iso_host | `Client/core/src/native/ghost_host.rs` |
| Fileless | `fileless_service.go`, `/api/stage2/:id`, `generate_controller?delivery=fileless`, stager |

## Shipped status

| Item | Status |
|------|--------|
| Manual-Map `pe_map` + loader | ✅ (LoadLibrary fallback) |
| CKMS flags + Go packer | ✅ |
| Catalog `load_mode` | ✅ |
| module_required auto-push + retry | ✅ |
| SleepCrypto (no default heap XOR) | ✅ `sleep-mask` feature |
| Fileless panel + Stage2 API | ✅ |
| socks L2 split | ❌ out of scope |

## Out of scope (this iteration)

- Permanent AV/EDR evasion claims  
- Re-adding generic process injection product line  
- socks as staged L2 module  
- Linux parity for pe_map (short-lived `.so` remains)
