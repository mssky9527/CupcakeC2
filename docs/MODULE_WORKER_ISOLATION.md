# Module Worker Isolation

## Goal

Worker crash / leak / hang / output flood **must not** take down Stage0 Agent
(C2 session, heartbeat, command queue).

Core rule:

> Stage0 never loads product L2 DLL/PE and never calls module exports.
> Product modules run only in independent Worker processes.

```text
Main Agent
  ├─ transport / heartbeat / command queue
  ├─ ModuleSupervisor
  │    whitelist · hash/HMAC · Worker state · request forward · timeout · restart · circuit
  └─ IPC (length-prefixed job frame / JSON envelope)
       ├─ iso_host.exe       (BOF / .NET / KIND_INJECT; short-lived per job)
       ├─ inject_worker      (one-shot via iso_host KIND=3; process exits after job)
       └─ desktop            (capability gate; RDP dial stays out of product DLL map)
```

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

`desktop` remains a capability registration plus thin Stage0 RDP bridge in this phase. A long-lived desktop relay worker is still a later change; the bridge is gated by Supervisor `worker_ready` state and each relay is scoped to its own stream.


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
| `desktop` | Gate only (phase 1); RDP dial in thin bridge      | Register capability; no DLL map      |

Phase later:

- Long-lived `desktop_worker` for RDP byte relay outside Agent
- Dedicated `inject_worker.exe` binary if iso_host host is undesirable

## Implementation order

1. Main Agent `ModuleSupervisor` (state, limits, Job Object helpers)
2. `iso_host` as universal sacrificial worker (BOF/.NET/INJECT kinds)
3. `module_loader` product path → register only (no map); inject invoke → supervisor
4. Crash / timeout / restart / circuit tests
5. desktop long-lived relay worker (optional)
6. inject one-shot dedicated PE (optional; currently iso_host KIND 3)
7. Delete Stage0 product LoadLibrary / Manual-Map paths for whitelist IDs

## Acceptance criteria

```text
Worker crash          → Agent still heartbeats
Worker infinite loop  → Agent still handles new commands (deadline kill)
Worker output flood   → Agent transport not blocked (output cap + drop counter)
Worker force-killed   → Agent does not crash
Agent restart         → no residual Worker (Job Object kill-on-close)
```

## Non-goals (phase 1)

- Full desktop RDP byte-relay inside long-lived desktop_worker
- Cross-platform Job Object (Windows first)
- Shared-memory zero-copy IPC
- Policy-locked server-side module delete

## Code map

| Area                         | Path                                      |
|------------------------------|-------------------------------------------|
| Supervisor                   | `Client/core/src/module_supervisor/`      |
| Product load / invoke bridge | `Client/core/src/module_loader.rs`        |
| Sacrificial host             | `Client/iso_host/`                        |
| Isolated BOF/.NET spawn      | `Client/core/src/isolated_exec.rs`        |
| RDP gate + dial              | `Client/core/src/transport/desktop_bridge.rs` |
| Design                       | this file                                 |
