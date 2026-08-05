# Module Worker Isolation

## Goal

Worker crash / leak / hang / output flood **must not** take down Stage0 Agent
(C2 session, heartbeat, command queue).

Core rule:

> Stage0 never loads product L2 DLL/PE and never calls module exports.
> Product modules run only in independent Worker processes.

```text
Main Agent (Stage0)
  ├─ transport / heartbeat / command queue
  ├─ ModuleSupervisor
  │    whitelist · hash/HMAC · Worker state · request id · start/stop · deadline · cleanup · circuit
  └─ IPC (length-prefixed job frame / JSON envelope)
       ├─ iso_host.exe         (BOF / .NET / KIND_INJECT; short-lived per job)
       ├─ inject via iso_host  (one-shot KIND=3; process exits after job)
       └─ desktop_worker.exe   (TARGET: long-lived; Job Object owns RDP dial+relay)
```

### Desktop isolation — target vs current

| | **Current (default, shipping)** | **Target (skeleton in tree)** |
|--|--------------------------------|-------------------------------|
| RDP dial + byte relay | Stage0 `desktop_bridge` in-process | `desktop_worker.exe` child under Job Object |
| Module role | Capability gate (`worker_ready`) | Same gate + long-lived worker lifecycle |
| Stage0 owns | Module state, gate, Yamux accept, dial+pipe | Worker state, request id, start/stop, deadline, cleanup |
| Worker owns | — | TCP dial to target:3389 + bidirectional relay |
| Activation | Always (module Loaded) | Future: full path; today opt-in env falls back |

**Stage0 keeps (target):** worker state map, request id bookkeeping, start/stop API,
deadline / idle kill, Job Object assign, cleanup on agent disconnect.

**desktop_worker owns (target):** RDP target dial, ACK/NACK to supervisor/IPC,
raw bidirectional byte relay (no GDI/JPEG, no C2 crypto).

**Default path today is unchanged:** Yamux `0x0D` → `desktop_bridge::handle_stream`
in-process. Opt-in `CUPCAKE_DESKTOP_WORKER=1` logs that the worker path is not fully
implemented and **falls back** to the bridge (no regression).
## Rules

1. Product modules (`desktop`, `iso_host`, `inject`) are **never** Manual-Mapped
   or `LoadLibrary`'d in Stage0.
2. Stage0 only: stage bytes, verify, track `WorkerState`, spawn worker, IPC,
   timeout, kill via Job Object.
3. Server / UI `loaded_on_agent` means **worker_ready / registered**, not
   "DLL mapped into agent".
4. Thread isolation, `catch_unwind`, or `FreeLibrary` alone is **not** isolation.
   Independent process is mandatory.

## Worker states (not Agent state)

```text
Stopped → Starting → Ready → Busy
             ↓         ↓
           Failed ← Timeout → (restart / circuit open)
```

String surface (module status, not agent online/offline):

| State     | Surface string     |
|-----------|--------------------|
| Stopped   | `stopped`          |
| Starting  | `worker_starting`  |
| Ready     | `worker_ready`     |
| Busy      | `executing`        |
| Failed    | `failed`           |
| Timeout   | `timeout`          |

## Current guarantees

The default `isolated-exec` path now applies the following lifecycle guarantees to one-shot `iso_host` and native workers:

- every spawned worker is assigned to a Windows Job Object before input is sent;
- a failed Job Object assignment is a hard startup failure and the child is terminated;
- synchronous pipe I/O runs outside the Tokio executor, while stdout/stderr lengths are bounded;
- deadlines are clamped to 1-300 seconds and timeout terminates the Job Object and child before cleanup;
- host staging files, inherited pipe handles, process handles, and staged PE copies are cleaned on success and failure.

`desktop` (default): capability registration + thin Stage0 RDP bridge. Each relay is
scoped to its own Yamux stream and gated by Supervisor `worker_ready`.

`desktop` (isolation skeleton): `module_supervisor::desktop_worker` exposes long-lived
lifecycle stubs (`start` / `stop` / `status`); crate `Client/desktop_worker/` is a
placeholder binary. Full RDP dial+relay ownership by the child is **not** wired yet —
see “Desktop isolation — target vs current” above.

Main Agent retains only:

- Module whitelist (`desktop` / `iso_host` / `inject`)
- Hash / HMAC package verify (existing CKMS)
- Worker status map
- Request forward to child process
- Deadline / timeout kill
- Crash auto-restart policy + consecutive-failure circuit breaker
- Pending request cleanup on worker exit
- Stop workers when Agent disconnects / process exits (Job Object KillOnJobClose)

**Forbidden in Stage0 for product modules:**

- `LoadLibrary` / Manual Map
- `mod_init` / `mod_invoke` / `mod_shutdown`
- Sharing pointers, threads, or heaps with workers

## IPC protocol bounds

Request (logical; product inject uses binary job frame over stdin today):

```json
{
  "request_id": "...",
  "module_id": "inject",
  "operation": "execute",
  "payload_b64": "...",
  "deadline_ms": 30000
}
```

Response:

```json
{
  "request_id": "...",
  "status": "ok|error|timeout",
  "stdout": "...",
  "stderr": "...",
  "error_code": ""
}
```

Hard limits (Stage0 enforces):

| Limit              | Default   |
|--------------------|-----------|
| Max payload        | 8 MiB     |
| Max stdout/stderr  | 2 MiB ea  |
| Deadline           | 1s–300s   |
| Max concurrent     | 4         |
| Circuit open after | 5 fails   |

Worker no-response → timeout kill → fail pending → optional restart.

## Windows Job Object

Workers are assigned to a Job Object with:

- `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` — Agent exit cleans workers
- No orphan child processes after Agent restart
- Future: process / memory / handle limits

## Product module paths

| Module    | Worker model                                      | Stage0 role                          |
|-----------|---------------------------------------------------|--------------------------------------|
| `iso_host`| Sacrificial EXE; BOF/.NET jobs; host PE for inject| Stage PE bytes; spawn only           |
| `inject`  | One-shot: spawn iso_host with KIND_INJECT=3       | Register capability; IPC forward     |
| `desktop` | **Default:** gate + Stage0 `desktop_bridge` dial/relay. **Target:** long-lived `desktop_worker.exe` under Job Object | Register capability; lifecycle stubs; no DLL map |

Phase later (desktop isolation steps after this skeleton):

1. Supervisor spawns staged `desktop` / `desktop_worker` PE into Job Object
2. Hand off accepted Yamux side or duplicate socket / framed IPC for relay
3. Child dials target:3389 and pipes bytes; Stage0 only tracks request + deadline
4. Stop / agent disconnect terminates Job Object (kill-on-close)
5. Remove in-process dial from `desktop_bridge` once worker path is proven
6. Dedicated `inject_worker.exe` binary if iso_host host is undesirable
## Implementation order

1. Main Agent `ModuleSupervisor` (state, limits, Job Object helpers) — **done**
2. `iso_host` as universal sacrificial worker (BOF/.NET/INJECT kinds) — **done**
3. `module_loader` product path → register only (no map); inject invoke → supervisor — **done**
4. Crash / timeout / restart / circuit tests — **done / ongoing**
5. desktop isolation skeleton (lifecycle stubs + placeholder crate + design) — **this step**
6. desktop long-lived relay worker (full dial+relay in child; default off until proven)
7. inject one-shot dedicated PE (optional; currently iso_host KIND 3)
8. Delete Stage0 product LoadLibrary / Manual-Map paths for whitelist IDs
## Acceptance criteria

```text
Worker crash          → Agent still heartbeats
Worker infinite loop  → Agent still handles new commands (deadline kill)
Worker output flood   → Agent transport not blocked (output cap + drop counter)
Worker force-killed   → Agent does not crash
Agent restart         → no residual Worker (Job Object kill-on-close)
```

## Non-goals (this skeleton step)

- Full desktop RDP byte-relay inside long-lived `desktop_worker` (stubs only)
- Changing default product path away from `desktop_bridge`
- Cross-platform Job Object (Windows first)
- Shared-memory zero-copy IPC
- Policy-locked server-side module delete

## Opt-in env (skeleton)

| Env / flag | Behavior |
|------------|----------|
| *(unset)* | Default: Stage0 `desktop_bridge` RDP dial+relay |
| `CUPCAKE_DESKTOP_WORKER=1` | Log “desktop worker path not fully implemented”; fall back to bridge. May record lifecycle bookkeeping / optional placeholder spawn later |

## Code map

| Area                         | Path                                      |
|------------------------------|-------------------------------------------|
| Supervisor                   | `Client/core/src/module_supervisor/`      |
| Desktop worker lifecycle stubs | `Client/core/src/module_supervisor/desktop_worker.rs` |
| Product load / invoke bridge | `Client/core/src/module_loader.rs`        |
| Sacrificial host             | `Client/iso_host/`                        |
| Desktop worker placeholder   | `Client/desktop_worker/`                  |
| Isolated BOF/.NET spawn      | `Client/core/src/isolated_exec.rs`        |
| RDP gate + dial (default)    | `Client/core/src/transport/desktop_bridge.rs` |
| Design                       | this file + `docs/DESKTOP_MODULE_DESIGN.md` |
