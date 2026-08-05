# Remote Desktop — Modular RDP Port Forward

Status: **active product path**  
Mode: L2 module `desktop` + Stage0 thin Yamux DESKTOP bridge  
Not: GDI/JPEG capture / CPXD canvas  
Isolation: long-lived `desktop_worker` is **skeleton only** — default still Stage0 bridge

## Architecture

### Current (default, shipping)

```text
Operator
  1) Module panel → stage+load "desktop"
       ↳ Stage0 auto-enables local RDP (fDenyTSConnections=0, TermService, firewall, probe :3389)
       ↳ Opt-out: CUPCAKE_DESKTOP_NO_AUTO_RDP=1
  2) Remote Desktop → Start RDP forward
  3) mstsc /v:<C2_IP>:<listen_port>

mstsc → C2 listen → Yamux DESKTOP 0x0D → Stage0 desktop_bridge (module gate)
      → Agent process dials target:3389 → pipes RDP bytes in-process
```

### Target (isolation — not default yet)

```text
mstsc → C2 → Yamux DESKTOP 0x0D → Stage0 ModuleSupervisor
      → start desktop_worker.exe (Job Object)
      → worker dials target:3389 + owns byte relay
      → Stage0: state / request id / stop / deadline / cleanup only
```

| Layer | Component | Role |
|-------|-----------|------|
| UI | `RemoteDesktop.vue` + Module panel | Load module, start/stop, show mstsc |
| API | `/api/desktop/:uuid/{status,start,stop}` | Listener control |
| Server | `desktop_service.go` | Listen + `DialAgentDesktop` (type 0x0D) |
| Stage0 | `desktop_bridge` | **Default:** module gate + dial + pipe |
| Stage0 | `rdp_enable` | On desktop load: enable local TermService / 3389 (Windows) |
| Stage0 | `module_supervisor::desktop_worker` | Lifecycle stubs (start/stop/status); opt-in env falls back to bridge |
| Worker (target) | `Client/desktop_worker/` | Placeholder PE; future: Job Object RDP dial+relay |
| L2 | `modules/desktop` → `desktop.bin` | Capability package / staged PE identity (not mapped; no mod_init) |
| Target | RDP service | Default `127.0.0.1:3389` (auto-enabled on load) |

See also: `docs/MODULE_WORKER_ISOLATION.md` (desktop target vs current table).
## Operator flow (locked)

1. TCP Yamux agent online (sole product tier **minimal** includes `module-loader`).
2. **模块** → load `desktop` → state Loaded; Agent 侧自动开 RDP（需管理员权限更稳）。
   - module_stage 回执含 `rdp_enable=reg=ok svc=ok fw=ok listen3389=ok|fail …`
3. **远程桌面** → target host/port (default 127.0.0.1:3389) → 启动 C2 端口转发。
4. `mstsc /v:<C2>:<port>` with Windows credentials.
5. STOP closes C2 listener; optional unload `desktop`.

If DESKTOP stream opens without module → agent NACKs `0x00` → server error mentions load module.

Auto-enable is **best-effort**: GPO / no admin / Server Core without RDP feature may leave `listen3389=fail`.

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

## Isolation skeleton (current step)

| Piece | Status |
|-------|--------|
| Design: Stage0 supervisor vs desktop_worker boundary | Documented |
| `module_supervisor` start/stop/status stubs | Present; no production handoff |
| `Client/desktop_worker` placeholder binary | Present; not used by default path |
| `CUPCAKE_DESKTOP_WORKER=1` | Logs not-fully-implemented; **falls back to bridge** |
| Default product path | Unchanged: `desktop_bridge` in-process relay |

**Next steps for full isolation**

1. Stage/spawn `desktop_worker` PE under Job Object when DESKTOP stream opens
2. Define IPC or socket handoff for Yamux half ↔ worker TCP half
3. Move dial + `copy_with_idle` into worker; Stage0 only watches deadline
4. Wire stop / unload / agent disconnect → Job Object terminate
5. Integration tests: worker crash must not kill Stage0 C2 session
6. Flip default only after acceptance criteria in MODULE_WORKER_ISOLATION.md

## Explicit non-goals

- Browser canvas / JPEG frames / GDI BitBlt in Stage0
- Auto-stage desktop on Remote Desktop click
- Embedding RDP stack in the agent (uses OS Terminal Services)
- Defaulting product traffic onto unfinished desktop_worker path
