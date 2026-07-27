# OPSEC deploy notes (agent + control plane)

## iso_host disk reality

- **BOF / .NET payload body**: always on the **pipe** only (job frame) — never a payload file.
- **Host EXE (x64 preferred path)** — `native/ghost_host.rs`:
  1. **Section ghost + true PPID**: delete-pending file → `NtCreateSection(SEC_IMAGE)` → close file (path gone) → open parent from pool (`RuntimeBroker` / `sihost` / …) → **`NtCreateProcessEx(ParentProcess=parent, Section=host)`** → spoofed ImagePath in process params → `NtCreateThreadEx`. Kernel parent is the spoofed process (not the agent).
  2. **Fallback**: delete-on-close `CreateProcessW` + `PROC_THREAD_ATTRIBUTE_PARENT_PROCESS` (same parent pool).
  3. **Last resort**: classic temp stage under INetCache, burned after exit.
- Non-x64 / failure: falls back to temp stage automatically.

## Wire seed (P0)

- Client `build.rs` and server `utils.WireIDs` share algorithm + default seed `wire-v1-default-2026`.
- Override: set `CUPCAKE_WIRE_SEED` on the **server process** and ensure panel builds inject the same (builder exports env to cargo).
- Setting `wire_seed` in DB is written on first start if empty.

## Profiles

- **minimal / standard**: never enable `stealth-adv` (ETW/AMSI unhook = louder).
- **full**: only when you explicitly accept ETW/AMSI hunting rules.

## Why AV/EDR kills “during heavy ops” (not only at first beacon)

Agent **online/heartbeat alone** is usually quieter. **Kills spike when operators burst post-ex**:

| Action | High-signal behavior | What AV/EDR sees |
|--------|----------------------|------------------|
| BOF / execute-assembly | `iso_host` process create + PPID spoof + pipes | Process tree anomaly, short-lived children of RuntimeBroker/sihost |
| Module stage (L2 DLL) | Often **temp write + LoadLibrary** (Rust CRT/TLS cannot Manual-Map DllMain safely) | Image load from user cache / short-lived DLL |
| native_exec (fscan 等) | Temp PE + CreateProcess | New unsigned EXE under INetCache/TEMP |
| Fileless one-click | PS `VirtualAlloc` + `CreateThread` / stager debug APIs | Classic shellcode loader signatures |
| Profile `full` / `stealth-adv` | ETW/AMSI patch + ntdll unhook at **startup** | Immediate memory-patch alerts (often worse than not patching) |

### Operator rules (must)

1. **Build `minimal`**, not `full`. Never ship `stealth-adv` unless you accept instant hunting.
2. **Do not burst**: one BOF / one assembly / one plugin at a time; wait for result before next.
3. Prefer **iso_host path** for BOF/.NET (already product default) — do **not** also stage legacy in-process `bof`/`dotnet` DLLs into the beacon.
4. Avoid **fileless PS** on modern Defender; use disk Stage0 + careful ops, or hardened L0.
5. Avoid **fscan-class native_exec** early — treat as last resort (temp PE is loud).
6. Do not enable diag (`RUST_LOG` / `AGENT_ALLOW_DIAG`) on target.

### Agent pacing (built-in)

Env on the **agent process** (or set at build/run wrapper):

| Value | Effect |
|-------|--------|
| *(unset)* | Random **300–1200 ms** pause before BOF/module-load/native spawn |
| `CUPCAKE_OPSEC_PACE_MS=auto` | Same as default |
| `CUPCAKE_OPSEC_PACE_MS=2000` | Fixed 2s between heavy ops |
| `CUPCAKE_OPSEC_PACE_MS=off` | No delay (lab only) |

Temp stage files use `LOCALAPPDATA\Microsoft\Windows\INetCache\~DF*.tmp|dll` (not `cpx_*.dll` under `%TEMP%`).

## Control plane (P2)

- New `config.json` defaults: `admin_bind=127.0.0.1`, empty password → random on first user create.
- Lab convenience: `CUPCAKE_FORCE_DEV_PASS=1` forces `admin` / `cupcake123`.
- Prefer reverse proxy / redirector so agents never see the real panel IP.
- Production agents: do not set `AGENT_ALLOW_DIAG=1` / do not rely on `RUST_LOG`.

## Strings gate

```powershell
powershell -File Client/scripts/strings-gate.ps1 -Path path\to\agent.exe
```

## Lab: disable pacing

```text
set CUPCAKE_OPSEC_PACE_MS=off
```

## Lab hygiene: do not leave Go package test binaries

`go test -c ./services` writes **`server/services.test.exe`** (~30MB). That binary links Donut/PE packing/fileless tests and is **commonly quarantined by AV** as a PE/shellcode toolset — not because the live panel is running.

**Do this instead:**

```powershell
# Preferred: run tests without leaving a named test.exe in the tree
cd server
go test ./services/ -count=1 -timeout 120s

# If you must compile: put output under TEMP, then delete
go test -c -o $env:TEMP\cupcake_svc_unit.exe ./services/
& $env:TEMP\cupcake_svc_unit.exe -test.count=1
Remove-Item -Force $env:TEMP\cupcake_svc_unit.exe
```

Never commit or keep `services.test.exe` / `*.test.exe` under `server/`. Root `.gitignore` already has `/server/*.exe`.
