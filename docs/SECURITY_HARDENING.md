# Security Hardening — Production Boundary

## Overview

This document tracks the first batch of production security hardening applied
to Server, MCP, and Client. The goal is fail-closed boundaries: missing
credentials, missing keys, or unknown endpoints must never silently degrade to
an open or cleartext path.

## 1. Server Origin validation

CORS and WebSocket Origin checks now use strict URL parsing instead of string
prefix/contains matching.

- `server/main.go` CORS `AllowOriginFunc` parses the Origin URL and compares
  scheme, hostname, and port explicitly.
- `server/pkg/globals/globals.go` `originAllowed()` is shared by both CORS and
  WebSocket upgraders. It rejects origins with paths, queries, fragments, or
  userinfo.
- Malicious subdomains like `http://localhost.attacker.example` are rejected.
- Tests: `server/pkg/globals/origin_test.go` covers same-host, port mismatch,
  malicious subdomain, IPv6, empty, and malformed origins.

## 2. MCP Server endpoint allowlist

MCP authorization changed from "HTTP method + path blacklist" to an explicit
endpoint allowlist with read/write capability.

- `server/pkg/middleware/auth.go` defines `mcpAllowlist` — every MCP-accessible
  route is declared with its method, prefix, and write flag.
- Read-only mode (default) rejects all write endpoints, not just non-GET
  methods.
- Unknown endpoints are denied by default — there is no "any GET is fine" path.
- Management paths (`/api/settings`, `/api/maintenance`, `/api/auth`,
  `/api/generate`, `/api/stager`, `/api/agents/connect`) are never in the MCP
  allowlist, even when read-only is off.
- Denials return a structured `error_code` (`mcp_disabled`, `mcp_read_only`,
  `mcp_endpoint_denied`) instead of bare 403 text.
- Tests: `server/pkg/middleware/mcp_policy_test.go` covers read-only allowed,
  read-only denied, write-allowed, management-path denied, and unknown denied.

## 3. MCP client fail-closed

- `MCPClient/client.py` removed the hardcoded default token. `C2_API_TOKEN`
  is required; missing token causes immediate startup failure.
- `c2_request()` now checks HTTP status codes and returns structured
  `{ok, status, error_code, message, data}` for 401, 403, 404, 5xx, timeout,
  and connection errors.
- `MCPClient/command_guard.py` can no longer be disabled via config. A config
  setting `"enabled": false` is ignored. Corrupt or unreadable config falls
  back to built-in rules with an error, not silent fail-open.

## 4. MCP token migration

- `server/pkg/store/db.go` `initDefaultAdmin()` no longer reuses the legacy
  `system_api_token` as the MCP token. On upgrade, a fresh random
  `mcp_api_token` is generated and the legacy token is cleared.
- Default policy remains fail-closed: `system_mcp_enabled=false`,
  `mcp_allowed_cidrs=127.0.0.1/32,::1/128`, `mcp_read_only=true`.

## 5. Panel session invalidation on password change

- `server/controllers/admin_controller.go` `HandleChangeMyPassword()` now
  rotates the bearer token on password change. The old session token is
  invalidated immediately and a new token is returned to the caller.
- A leaked token cannot survive a password change.

## 6. Client transport fail-closed

- `Client/core/src/transport/session_crypto.rs` rejects empty keys in
  `traffic_key()`, `seal_for_wire()`, `FragReassembler::push()`, and
  `open_wire_frame()`. Production builds never send or accept cleartext.
- `Client/core/src/transport/ws.rs` refuses to establish a WebSocket session
  when the Noise PSK is missing. The old "warn and continue" path is removed.
- `Client/core/src/transport/tcp.rs` refuses to establish a TCP session when
  the Noise PSK is missing.
- `Client/core/src/transport/tcp_bind.rs` refuses to accept a bind connection
  when the Noise PSK is missing.
- Tests: 4 new `session_crypto` tests verify empty-key rejection for seal,
  traffic_key, open_wire_frame, and reassembler push.

## 7. TCP bind address preservation

- `Client/core/src/main.rs` `run_bind_mode()` no longer rewrites the bind
  address to `0.0.0.0`. The configured host is preserved; when only a port is
  given, the default is `127.0.0.1` (loopback). Explicit `0.0.0.0` must be
  chosen by the operator.

## Verification (batch 1)

```text
Server:
  go test ./pkg/middleware/... ./pkg/globals/... ./controllers/...
  → ok (3 packages)

Client:
  cargo test --features minimal session_crypto --lib
  → 9 passed (5 existing + 4 new empty-key rejection tests)
```

---

# Batch 2 — Worker limits, Desktop relay, HTTP/file quotas

## 8. Worker output and resource limits (Client Rust)

### 8a. Inject worker reader thread (deadlock fix)

- `module_supervisor/mod.rs` `run_inject_via_iso_host` spawns a stdout reader
  thread **before** `WaitForSingleObject`, matching `run_job_blocking`.
- Prevents pipe-buffer deadlock when worker output exceeds ~64 KiB.

### 8b. Native worker bounded output

- `isolated_exec.rs` `run_native_job` uses `pipe_read_to_end_bounded(..., MAX_OUTPUT_BYTES)`
  (2 MiB) instead of reading up to 32 MiB then rejecting.
- Truncation terminates the Job Object / process and returns
  `worker output too large`.
- `native/spawn.rs` `apply_output_bound` is the pure cap used by the read loop.

### 8c–8d. Job Object fail-closed + resource limits

- `job_object.rs` `create()` returns `None` if limit configuration fails
  (no more `let _ = set_kill_on_close()`).
- Limits applied together with kill-on-close:
  - `active_process_limit` = 32
  - `job_memory_limit` = 512 MiB
  - `per_process_user_time_limit` = 60s CPU

### 8e. Agent exit cleanup

- `main.rs` each `run_*_mode` session end calls
  `module_supervisor::supervisor().stop_all()`.
- `utils::self_destruct` calls `stop_all()` before `process::exit`.

## 9. Desktop relay connection-level protection

### 9a. Agent idle timeout

- `desktop_bridge.rs` bidirectional copy uses `copy_with_idle` with 120s
  per-read idle timeout (resets on data).

### 9b. Server idle timeout + per-agent conn cap

- `desktop_service.go` wraps both ends of the RDP pipe with
  `idleDeadlineConn` (120s deadline reset on I/O).
- Concurrent client pipes per agent capped at 8
  (`desktopMaxConnsPerAgent`).

### 9c. RDP listener default loopback

- `StartDesktopRDP` binds `127.0.0.1` by default.
- Override with env `CUPCAKE_DESKTOP_LISTEN_HOST` (e.g. `0.0.0.0`).

## 10. Server HTTP timeouts and file/plugin quotas

### 10a. Admin HTTP Server timeouts (P0)

- `main.go` `newAdminHTTPServer`:
  - `ReadHeaderTimeout` 10s
  - `ReadTimeout` 60s
  - `WriteTimeout` 300s
  - `IdleTimeout` 120s
  - `MaxHeaderBytes` 1 MiB

### 10b. Agent upload limits (P0)

- `transfer_service.go`: max file 256 MiB; RFC-4122 UUID required
  (`ValidateAgentUpload` / `ValidAgentUUID`).

### 10c. Plugin upload size + admin auth (P0)

- `plugin_controller.go`: max plugin file 64 MiB; SHA-256 stored on upload.
- `main.go` `/plugins/upload`, `/plugins/run`, `/plugins/delete` gated with
  `RequireAdmin()` (same as module delete / generate).

### 10d. Plugin hash trust chain (P1)

- `PluginMetadata.Hash` = lowercase hex SHA-256 of file bytes.
- `DeployPlugin` calls `VerifyPluginHash` before staging to the agent;
  mismatch refuses deploy and drops cache.

### 10e. Task output retention (P1)

- `command_store.go` `PurgeExpiredTaskLogs` removes `logs/task_*.txt` and
  matching DB rows older than N days (default 7;
  env `CUPCAKE_TASK_LOG_RETENTION_DAYS`).
- `StartTaskLogRetentionWorker` runs hourly from `main`.

## Verification (batch 2)

```text
Server:
  go test ./...
  → admin HTTP timeouts, transfer gates, plugin hash, retention,
    desktop listen host / idle constants, plugin RequireAdmin routes

Client:
  cargo test --features minimal --lib
  → job_object fail-closed, stop_all, MAX_OUTPUT_BYTES, apply_output_bound,
    desktop_bridge idle copy
```

## Not covered (later batches)

- Desktop relay migration to long-lived worker process (phase 2)
- Full plugin signature (non hash-only) / anti-rollback
- Structured audit log and metrics/healthz
- CI, SBOM, reproducible release pipeline
- Full route RBAC matrix (operator vs admin capability per route)
- Session table with expiry, device binding, and multi-device revocation
