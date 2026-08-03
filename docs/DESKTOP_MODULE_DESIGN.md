# Remote Desktop — Modular RDP Port Forward

Status: **active product path**  
Mode: L2 module `desktop` + Stage0 thin Yamux DESKTOP bridge  
Not: GDI/JPEG capture / CPXD canvas

## Architecture

```text
Operator
  1) Module panel → stage+load "desktop"
  2) Remote Desktop → Start RDP forward
  3) mstsc /v:<C2_IP>:<listen_port>

mstsc → C2 listen → Yamux DESKTOP 0x0D → Stage0 bridge (module gate)
      → Agent dials target:3389 → Windows RDP
```

| Layer | Component | Role |
|-------|-----------|------|
| UI | `RemoteDesktop.vue` + Module panel | Load module, start/stop, show mstsc |
| API | `/api/desktop/:uuid/{status,start,stop}` | Listener control |
| Server | `desktop_service.go` | Listen + `DialAgentDesktop` (type 0x0D) |
| Stage0 | `desktop_bridge` | Require module Loaded; dial + pipe |
| L2 | `modules/desktop` → `desktop.bin` | Capability package (`mod_*` ABI) |
| Target | RDP service | Default `127.0.0.1:3389` |

## Operator flow (locked)

1. TCP Yamux agent online (sole product tier **minimal** includes `module-loader`).
2. **模块** → load `desktop` → state Loaded.
3. **远程桌面** → target host/port (default 127.0.0.1:3389) → 启动.
4. `mstsc /v:<C2>:<port>` with Windows credentials.
5. STOP closes C2 listener; optional unload `desktop`.

If DESKTOP stream opens without module → agent NACKs `0x00` → server error mentions load module.

## Yamux types

| Byte | Name | Use |
|------|------|-----|
| `0x02` | SOCKS | General tunnels (independent of desktop module) |
| `0x0D` | DESKTOP | RDP port-forward only; module-gated |

## Build L2 package

```powershell
powershell -File server/scripts/build-desktop-module.ps1
# → server/storage/modules/desktop.bin
```

## Explicit non-goals

- Browser canvas / JPEG frames / GDI BitBlt in Stage0
- Auto-stage desktop on Remote Desktop click
- Embedding RDP stack in the agent (uses OS Terminal Services)
