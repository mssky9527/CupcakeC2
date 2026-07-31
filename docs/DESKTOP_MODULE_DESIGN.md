# Design Probe: L2 Remote Desktop (`mod_desktop`)

Status: **PR0–PR6 MVP skeleton landed** (synthetic JPEG; DXGI real capture deferred)  
Branch context: `0.0.5`  
Date: 2026-07-31  
Updated: 2026-07-31 — CPXD codec, server busy/rate-limit, desktop_engine+bridge, L2 mod_desktop, RemoteDesktop UI

## 0. Goals & non-goals

### Goals (MVP)

- On-demand **GUI view + basic input** over existing C2 session (ToDesk-like *capability*, not product clone).
- **L2 only** — load via mem-map / module package; unload on STOP.
- **JPEG-first** streaming with dirty-rect optional; `encode=h264|jpeg` reserved, not implemented in phase 1.
- Central **Yamux stream type constants** (no scattered magic numbers).
- Bandwidth protection (server cap + client auto-degrade).
- Hard STOP: no residual capture/encode threads after unload.
- **Explicit two-step** memory window: Module panel load → then open Desktop view (no silent auto-stage).

### Non-goals (MVP)

- Full ToDesk/UU feature parity (relay, NAT, file drag, multi-monitor polish).
- Soft x264 in L2 (size blow-up).
- DXGI + hardware H.264 / MFT path (phase 2+).
- Winlogon / UAC secure desktop / Session-0 full interactivity as hard requirements (feasibility check only at MVP end).
- WebSocket desktop path / screenshot-over-JSON second path.
- Multi-viewer / multi-operator concurrent desktop on one agent.

---

## 1. Yamux stream type occupancy table

### 1.1 Current live usage (repo scan)

| Type | Name | Client dispatch | Server open site | Feature gate |
|------|------|-----------------|------------------|--------------|
| `0x01` | PTY / interactive shell | `transport/tcp.rs`, `tcp_bind.rs` → `pty::handle_stream` | `controllers/client_controller.go` `openPtyStream` | `pty` |
| `0x02` | SOCKS / tunnel data | → `socks::handle_stream` | `services/tunnel_service.go` | `socks` |
| `0x03` | File manager | → `fs::handle_stream` | `services/file_service.go` | `post-ex` |
| `0x04` | Process list/kill | → `process::handle_stream` | `services/process_service.go` | `post-ex` |
| `0x05`–`0x0C` | **free** | — | — | — |
| **`0x0D`** | **DESKTOP (locked)** | — | — | `desktop` (new; **never** default profile) |
| `0x0E`–`0xFE` | **free** (reserve policy below) | — | — | — |
| `0xFF` | **reserved** (reject / future ext) | — | — | — |

### 1.2 Important discovery: types are **not** in `transport/profile.rs`

`transport/profile.rs` is **HTTP malleable C2 profile** (URI/headers/JA3 hints only).  
Yamux type bytes today are **inline magic numbers** in:

- Client: `transport/tcp.rs`, `transport/tcp_bind.rs`
- Server: `client_controller.go`, `file_service.go`, `process_service.go`, `tunnel_service.go`

**Action (impl PR0):** introduce a single shared constant surface and stop writing bare `0x0D` everywhere.

Suggested client file: `Client/core/src/transport/stream_types.rs`  
Suggested server file: `server/pkg/utils/stream_types.go` (or next to `wire_ids.go`)

```text
// Canonical names (both sides must match)
YAMUX_STREAM_PTY      = 0x01
YAMUX_STREAM_SOCKS    = 0x02
YAMUX_STREAM_FS       = 0x03
YAMUX_STREAM_PROCESS  = 0x04
YAMUX_STREAM_DESKTOP  = 0x0D   // locked free
YAMUX_STREAM_RESERVED = 0xFF
```

Reserve policy:

- Prefer sequential low numbers for core post-ex (`0x05` next if something else ships first).
- Desktop uses **`0x0D`** as agreed (skips 0x05–0x0C for future FS/plugin streams if needed).
- Document any new type in **this table + constants file** in the same PR.

### 1.3 Transport constraint (MVP scope) — **WS deferred, locked**

| Transport | Yamux multiplex | Desktop MVP |
|-----------|-----------------|-------------|
| **TCP / TCP-bind** | Yes (`YamuxSession`) | **Supported** |
| **WebSocket** | Control JSON only; **no Yamux** | **Out of MVP** — UI grey-out only; **no** screenshot-over-JSON second path |
| DNS | No | No |

Rationale: a second path doubles maintenance and fights WS concurrency; MVP focuses on TCP Yamux only.

---

## 2. Fit with `0.0.5` module / fileless line

Existing path:

```text
stage CKMS → verify → mem-map (or short disk) → mod_init → mod_invoke → unload (mod_shutdown + unmap)
```

Desktop continues the same product story:

- Operator-visible capability window → explicit load → use → STOP / unload → no permanent bloat.
- Aligns with OPSEC pacing: operator knows **which step** pulled multi-MB L2 PE into target memory.

| Component | Location | Default |
|-----------|----------|---------|
| Capture / encode / input | `Client/modules/desktop` L2 PE | Not in Stage0 |
| Yamux `0x0D` thin bridge | `cupcake-core` `feature = "desktop"` | **off** everywhere by default — not in `default`, **not in `standard`**, not in `beacon` |
| Module id | `desktop` | Registered in `module_loader` + server `module_service` |
| Operator flow | **Two explicit steps** | (1) Module panel load `desktop` (2) open Desktop view |

### 2.1 Operator UX (locked — no auto-stage)

```text
1. Module Manager / ModulePanel → stage+load "desktop"
2. ClientDetail → Desktop (enabled only if module Loaded + TCP/Yamux)
3. STOP / close view → detach stream
4. Optional: unload desktop module (clears multi-MB mapping)
```

If Desktop clicked while module Absent → UI error **`module_not_loaded`** (do not silent stage).  
If agent lacks `desktop` feature bridge → **`bridge_unavailable`**.

---

## 3. `mod_desktop` ABI draft

### 3.1 Keep existing L2 C ABI (control plane)

Same as `mod_shell` / `mod_inject`:

| Export | Role |
|--------|------|
| `mod_init` | Create runtime, **do not** start DXGI yet |
| `mod_invoke(cmd_type, payload) → JSON` | Control / probe only (no frame blobs) |
| `mod_free` | Free invoke output buffers |
| `mod_shutdown` | **Hard teardown** (see §6) |

`mod_invoke` cmd_types (MVP):

| cmd_type | payload (JSON) | result |
|----------|----------------|--------|
| `desktop_probe` | `{}` | session reachability, monitors, OS session id, can_input, physical_w/h |
| `desktop_start` | `{fps, quality, max_w, encode, monitor}` | `{ok, session_id, encode_effective}` |
| `desktop_stop` | `{session_id?}` | `{ok}` |
| `desktop_config` | partial knobs | `{ok, applied}` |

Notes:

- `encode`: `"jpeg"` required MVP; `"h264"` returns error `encode_unsupported` until phase 2 (switch bit only).
- Frames **must not** go through `mod_invoke`.

### 3.2 Stream plane (Stage0 thin bridge)

```text
Server opens Yamux type DESKTOP (0x0D)
  → Stage0 desktop_bridge::handle_stream(stream)
      1) require module "desktop" already Loaded — fail closed with ERROR module_missing (no auto-stage)
      2) binary session on stream (frame protocol §4)
      3) mod_stream_* exports
```

**ABI extension** (PE self-contained; Stage0 stays thin):

```c
// Returns 0 ok; negative error codes (see §3.2.1).
int mod_stream_attach(uint32_t session_token);

// Blocking poll with timeout — see §3.2.1 (NOT busy-poll).
int mod_stream_poll_frame(
    uint32_t session_token,
    uint32_t timeout_ms,              // MVP default 100
    uint8_t* out_hdr, uint32_t hdr_cap, uint32_t* hdr_len,
    uint8_t** out_blob, uint32_t* blob_len
);

int mod_stream_push_input(uint32_t session_token, const uint8_t* msg, uint32_t len);

// Idempotent: second call returns 0 immediately (see §6.2.1).
int mod_stream_detach(uint32_t session_token);
```

Stage0 bridge owns:

- Read/write Yamux framing + length/magic enforcement (§4.1)
- Backpressure / local degrade
- Calling `mod_stream_*` on a worker that can wake on stop

If exports missing → send **one** `ERROR` `{code: module_missing|abi_missing}` then close (handshake path).  
If magic wrong mid-stream → **silent close** (§4.1.2) — do not amplify probing.

#### 3.2.1 `mod_stream_poll_frame` semantics (**locked**)

| Situation | Return | `hdr_len` / `blob_len` | Notes |
|-----------|--------|------------------------|-------|
| Frame ready | `0` | set; blob via module alloc → bridge `mod_free` after write | Normal |
| Timeout, no frame | `0` | `hdr_len=0`, `blob_len=0`, `out_blob=null` | **Not** an error — bridge loops / checks stop |
| Session detaching / detached | `-1` (`E_DETACHED`) | zeroed | Bridge exits loop |
| Invalid token / internal | `< -1` | zeroed | Bridge sends ERROR once then teardown |

**MVP: blocking with `timeout_ms=100` (default).**  

- Bridge thread can observe stop flag between polls.  
- **Forbidden:** busy-spin `poll` with 0 timeout in a tight loop (CPU burn = OPSEC signal).  
- Suggested negative constants (document in module header):

```text
E_DETACHED     = -1
E_AGAIN        = not used for timeout (timeout is 0/empty success)
E_INVAL        = -2
E_NOMEM        = -3
E_CAPTURE      = -4
```

### 3.3 Feature gates & CI matrix (**locked**)

```toml
# cupcake-core
desktop = []   # Yamux 0x0D bridge ONLY — NO DXGI / d3d11 in core

# NOT listed under default = [...]
# NOT added to standard = [...]
# NOT added to full unless product explicitly defines full-gui later
# modules/desktop crate
default = ["encode-jpeg"]
encode-jpeg = []
encode-h264 = []   # phase 2 stub
```

**Shipped profiles:**

| Profile | `feature = "desktop"` (bridge) | L2 PE on disk/server |
|---------|--------------------------------|----------------------|
| `beacon` | **off** | optional package only |
| `minimal` | **off** | optional package only |
| `standard` | **off** (OPSEC hard line) | optional package only |
| `full` | **off** unless product renames/adds `full-gui` | optional |
| custom / **`full-gui`** (operator-selected) | **on** | yes |

OPSEC hard line: if bridge is baked into `standard`, every online agent exposes a permanent `0x0D` entry surface and loses “on-demand capability” positioning.

#### CI matrix (must run both — B5)

| Check | Command / rule | Pass criteria |
|-------|----------------|---------------|
| C1 | `cargo build -p cupcake-core --no-default-features` (+ platform target as CI already uses) | Builds; **must not** link desktop L2 PE; no DXGI path required |
| C2 | `cargo build -p cupcake-core --features "ws,desktop"` (or project-equivalent) | Builds bridge only; **artifact must not contain DXGI/D3D11 imports** — e.g. `dumpbin /imports` / `llvm-objdump` / strings grep fail closed on `dxgi.dll` `d3d11.dll` `D3D11CreateDevice` as **linked** imports from core |
| C3 | Unit: `desktop_not_in_default_profiles` | `cfg!(feature="desktop")` false under default CI product features used for Stage0 |
| C4 | L2 package build (Windows) | `modules/desktop` may link DXGI; isolated from Stage0 |

> Note: beacon-only tests are **insufficient**. Core could drag DXGI under `feature=desktop` and still pass a beacon-without-desktop build. C2 catches that.

---

## 4. Frame protocol draft (binary on Yamux `0x0D`)

### 4.1 Envelope (all messages)

```text
offset  size  field
0       4     magic = "CPXD" (0x43505844 LE bytes 'C','P','X','D')  // fixed; NOT wire seed
4       1     version = 1
5       1     msg_type
6       2     flags (LE)
8       4     payload_len (LE, bytes after 12-byte header)
12      N     payload
```

**Locked magic policy (§10.3):** fixed `CPXD`. Outer transport already encrypts. Deriving magic from `CUPCAKE_WIRE_SEED` would couple framing to crypto init and hurt debug/pcap.

#### 4.1.1 Length bounds (**B1 — dual enforce**)

| Rule | Value | Enforcer |
|------|-------|----------|
| Max `payload_len` | **2 MiB** (`2 * 1024 * 1024`) | **Both** Server `desktop_service` **and** Agent bridge — independent rebind, not trust-one-side |
| Max FRAME image blob (within payload) | recommend ≤ 512 KiB soft; hard still 2 MiB envelope | Agent encode path + server drop |
| Undersize header | read must fill 12 bytes or treat as EOF | both |

On `payload_len > MAX` or truncated body:

1. Log locally (agent: `db_print` / server: log)  
2. Run unified teardown (§6.2.1)  
3. **Do not** send ERROR for oversize from untrusted peer if magic was already wrong path; if magic OK and session established, optional single ERROR `payload_too_large` then close — **MVP simplify: always teardown without requiring peer ack**

#### 4.1.2 Magic / version mishandling (**B1 — anti-probe**)

| Condition | Action |
|-----------|--------|
| First 4 bytes ≠ `CPXD` | **Silent close stream** + local log only. **No** ERROR frame, **no** bytes back |
| `version` unsupported (≠ 1 for MVP) | Silent close + log (same as bad magic for unknown scanners) |
| Mid-session corrupted magic | Silent close + teardown |

Rationale: avoid probe amplification / protocol oracle.

Handshake errors **after** valid CPXD (e.g. module missing) may use one `ERROR` then close — that path is only reachable with correct magic from a real server.

### 4.2 msg_type

| Code | Name | Dir | Payload |
|------|------|-----|---------|
| `0x01` | `HELLO` | S→A | JSON: `{fps, quality, max_w, encode, monitor, max_bps}` |
| `0x02` | `HELLO_ACK` | A→S | JSON: `{ok, w, h, physical_w, physical_h, encode, can_input, session, monitors[]}` |
| `0x03` | `FRAME` | A→S | binary header + JPEG blob |
| `0x04` | `INPUT` | S→A | binary input events |
| `0x05` | `CONFIG` | S→A | JSON partial knobs |
| `0x06` | `PING` | either | `{t_ms}` |
| `0x07` | `PONG` | either | `{t_ms}` |
| `0x08` | `ERROR` | either | JSON `{code, msg}` — only on valid-magic sessions |
| `0x09` | `STOP` | S→A | `{}` — agent detach + close |
| `0x0A` | `KEYFRAME_REQ` | S→A | force full frame |
| `0x10` | `STATS` | A→S | JSON `{fps_out, bps, drops, queue}` (optional) |

`ERROR.code` values (MVP): `module_missing`, `abi_missing`, `busy` (**server-side only**, see §5.1.1), `encode_unsupported`, `capture_failed`, `can_input_false` (warn), `payload_too_large`.

### 4.3 `FRAME` payload (version 1, JPEG)

```text
0   2  w (LE)           // frame pixel width AFTER resize
2   2  h (LE)           // frame pixel height AFTER resize
4   1  encode           // 1=jpeg, 2=h264(reserved)
5   1  frame_flags      // bit0=keyframe, bit1=dirty_rect_set
6   2  rect_count       // 0 = full frame
8   4  ts_ms
12  4  seq
16  rect_count * 8: x,y,w,h as u16 LE each (in frame pixel space)
…  jpeg_bytes
```

HELLO_ACK / FRAME must stay consistent: UI renders `w×h`; input uses same space (§4.4).

MVP may always send full-frame JPEG (`rect_count=0`, `keyframe=1`).

### 4.4 `INPUT` payload & coordinate mapping (**B3 — locked**)

```text
0  1  ev_type  // 1=mouse_move 2=mouse_btn 3=wheel 4=key
…  type-specific
```

Mouse: `x_u16, y_u16, button_u8, down_u8` — coordinates in **frame pixel space** (`0..w-1`, `0..h-1` as sent in FRAME / HELLO_ACK).  
Key: `vk_u16, scan_u16, flags_u8`.

**Mapping responsibility (agent only):**

```text
UI / Server:  always in frame pixel space (post-resize image)
Agent:        physical_x = round(x * physical_w / frame_w)
              physical_y = round(y * physical_h / frame_h)
              then SendInput / equivalent in physical desktop coords
```

- `physical_w/h` from capture source (true desktop metrics for selected monitor).  
- `frame_w/h` = encoded frame dimensions after `max_w` scale.  
- If `frame_w==0` treat as error / drop input.  
- Multi-monitor: offsets applied in agent using selected monitor origin.

**MVP acceptance A3 sub-item:** high-DPI / 4K panel with `max_w=1280` — click lands within tolerance (e.g. ≤ 4 physical px of target center on a known UI control). Default-resolution-only click tests are **insufficient**.

### 4.5 Encode switch (future-proof)

HELLO negotiates `encode: "jpeg"`; `h264` → agent fallback to jpeg or `encode_unsupported` without crash. Prefer fallback so UI still works.

---

## 5. Bandwidth protection

### 5.1 Server per-stream caps

| Knob | Default MVP | Notes |
|------|-------------|-------|
| `max_bps` | 2_000_000 (2 Mbps) | Per desktop stream |
| `max_fps` | 5 | Hard clamp |
| `max_frame_bytes` | 512_000 | Drop + KEYFRAME_REQ at lower quality |
| `max_concurrent_desktop` | **1 per agent** | See §5.1.1 |

Implementation sketch (`desktop_service.go`):

- Token bucket on Admin←Agent frame forward path.
- If bucket empty: drop non-keyframes; every N drops send `CONFIG` lowering quality/fps.

#### 5.1.1 Multi-operator / multi-viewer (**locked — D**)

| Rule | Behavior |
|------|----------|
| Owner map | Server holds `agentUUID → activeDesktopSession` |
| Second open | **Reject at Server** before `session.Open()` to agent |
| Rejection | Admin WS/API returns **`ERROR busy`** (or HTTP 409); **no Yamux stream opened**, **no bytes to agent** |
| Who wins | First successful open holds until STOP / EOF / server drop |
| Multi-viewer | **Never in MVP** — dual mouse is catastrophic; agent implements single session only |

Agent-side need not implement “busy” for a second stream if server is correct; still safe to refuse second `mod_stream_attach` if a session_token already active.

### 5.2 Client auto-degrade

Agent tracks Yamux write backlog / PING RTT / queue depth.

Degrade ladder:

```text
fps 5 → 3 → 2 → 1
quality 75 → 60 → 45 → 30
max_w 1280 → 1024 → 800
```

Recover with hysteresis. Optional: dedicated desktop stream permit pool of 1 (MVP documents risk under shared max-16 semaphore).

### 5.3 Operator knobs (UI)

- FPS, quality, max width, STOP  
- `can_input` banner  
- Live STATS optional  
- Desktop disabled unless: TCP/Yamux + module Loaded + not busy  

---

## 6. OPSEC window & STOP hard requirements

### 6.1 Lifecycle

```text
[Absent] --explicit Module panel load--> [Loaded idle]
    --open Desktop + HELLO--> [Capturing]
    --STOP frame OR stream EOF OR unload--> [Idle or Absent]
```

### 6.2 `mod_shutdown` / `mod_stream_detach` MUST

1. Signal encode + capture threads to exit  
2. **Join** threads (timeout e.g. 3s, then log + best-effort)  
3. Release DXGI `IDXGIOutputDuplication` / GDI DCs  
4. Free frame buffers; wipe pointers  
5. No HWND / message-only window left registered  
6. No process-wide hooks left installed  

#### 6.2.1 Unified teardown path & idempotency (**B4 — locked**)

Two command sources **must** share one path:

| Source | Trigger | Agent action |
|--------|---------|--------------|
| A | Server sends `STOP` frame | `mod_stream_detach(token)` → close stream |
| B | Browser close / admin WS drop / network cut | Server closes Yamux → agent read **EOF** → **same** `mod_stream_detach(token)` |

Requirements:

- `mod_stream_detach` is **idempotent**: second call returns `0` immediately (atomic state: `Active → Detaching → Detached`).  
- No double-free of DXGI/objects/buffers.  
- Fast open→close→open must not crash (operator spam).  
- `mod_shutdown` (module unload) calls detach-all then tears down runtime; also idempotent.  
- Bridge on any fatal parse path also calls detach once then drops stream.

```text
                    ┌── STOP frame ──┐
stream loop ────────┤                ├──► detach_once() ──► join/capture free ──► stream close
                    └── read EOF ────┘
```

### 6.3 MVP acceptance (dev / hard)

| # | Criterion |
|---|-----------|
| A1 | First frame ≤ 3s after HELLO on healthy TCP agent (module pre-loaded) |
| A2 | JPEG path only; h264 request does not crash |
| A3 | Mouse + keyboard on interactive session; **includes high-DPI / max_w scale click accuracy** (§4.4) |
| A4 | STOP **or** EOF → **Process Explorer: no desktop encode/capture threads** |
| A5 | `unload desktop` → mem-map wiped; re-load works; double-detach safe |
| A6 | Default / standard / beacon Stage0 **do not** enable `desktop` feature; core+desktop has **no DXGI imports** (CI C1–C3) |
| A7 | Server max_bps prevents single stream from wedging control channel in lab |
| A8 | Second operator open → busy at server; agent never sees second stream |
| A9 | Bad magic → silent close (no ERROR oracle) |

### 6.4 Input reachability (MVP **end** gate)

| Context | Expectation MVP |
|---------|-----------------|
| Interactive user session, unlocked | capture + SendInput OK |
| Locked workstation | capture maybe; input may fail → `can_input=false` |
| Session 0 / service | often no desktop → probe fails cleanly |
| UAC consent UI | input may not reach → document; no crash |
| RDP session | usually OK |

HELLO_ACK must surface `can_input` and `session_name`. UI warns view-only when false.

---

## 7. Capture / encode plan

| Phase | Capture | Encode | Notes |
|-------|---------|--------|-------|
| MVP | DXGI Desktop Duplication; GDI `BitBlt` fallback | JPEG | Full frame; keep physical metrics for input map |
| 1.5 | Dirty rect from DXGI | JPEG | Bandwidth win |
| 2 | DXGI | HW H.264 MFT **or** refuse | `encode-h264` feature |
| — | — | Soft x264 | **Avoid** in L2 |

---

## 8. Server / Client / Frontend change list

### 8.1 Client (Rust)

| Item | File / crate | Notes |
|------|--------------|-------|
| Stream type constants | **new** `transport/stream_types.rs` | Replace 0x01–0x04 magics |
| Dispatch `YAMUX_STREAM_DESKTOP` | `tcp.rs`, `tcp_bind.rs` | `#[cfg(feature="desktop")]` only |
| Bridge | **new** bridge + PR0.5 stubs | No auto-stage; fail `module_missing` |
| Module id | `module_loader.rs` | `MOD_DESKTOP = "desktop"` |
| Feature | `Cargo.toml` | `desktop = []`; **not** in default/standard/full/beacon |
| L2 crate | `Client/modules/desktop` | DXGI/JPEG only here |
| Gates / CI | `feature_gates_test.rs` + CI scripts | C1–C4 §3.3 |
| Docs residual | `OPSEC_WINDOWS_RESIDUAL.md` | residual only while module live |

### 8.2 Server (Go)

| Item | File | Notes |
|------|------|-------|
| Constants | **new** `pkg/utils/stream_types.go` | Sync with client |
| Desktop service | **new** `services/desktop_service.go` | Open 0x0D, rate limit, **single-session map**, busy reject |
| Controller / WS | admin WS `/api/desktop/:uuid` | Binary to browser |
| Module catalog | `module_service.go` | `desktop` package; no auto-push |
| Agent check | helpers | `YamuxSession != nil`; module Loaded checked in UI/API |

### 8.3 Frontend (Vue)

| Item | File | Notes |
|------|------|-------|
| View | `views/client/RemoteDesktop.vue` | canvas + input; frame-space coords |
| Entry | `ClientDetail.vue` | Grey if WS / no Yamux / module not Loaded / busy |
| Flow copy | ModulePanel | “Load desktop module before opening Desktop” |
| UX | STOP + `can_input` + knobs | No silent stage button |

### 8.4 Build / packaging

- Builder: `desktop` CKMS like inject  
- **Never** auto-enable bridge on `standard`  
- CI: C1–C4  

---

## 9. Implementation order (PRs) — with **PR0.5**

| PR | Title | Deliverable | Parallelism |
|----|-------|-------------|-------------|
| **0** | Stream type constants | Client + server single source; refactor 0x01–0x04; declare `0x0D` | — |
| **0.5** | `mod_stream_*` stub + bridge skeleton | Empty L2 or host stub exports; bridge resolves symbols; without real desktop → valid CPXD session ends with **`ERROR module_missing`** (or abi_missing). **Unblocks PR1 ∥ PR2** | after PR0 |
| **1** | Protocol + server skeleton | HELLO/STOP/busy map/rate limit; can lab against stub agent | ∥ PR2 |
| **2** | L2 `mod_desktop` JPEG | DXGI/GDI + probe + poll_frame timeout + input scale | ∥ PR1 |
| **3** | Wire bridge to real module + feature gate | full-gui profile only; CI C1–C4 | after 0.5+1+2 |
| **4** | Frontend RemoteDesktop | canvas, two-step UX, grey WS | ∥ after PR1 API stable |
| **5** | Bandwidth + degrade | server bucket + agent ladder | after frames flow |
| **6** | MVP lab gate | §6.3 + §6.4 + §12 QA sign-off | last |

**Why PR0.5:** without a stub ABI, Server PR1 cannot handshake and L2 PR2 cannot integrate without serial blocking. Stub returns `module_missing` until PE is loaded; once Loaded, real exports replace stub behavior.

---

## 10. Decisions — **LOCKED** (operator 2026-07-31)

| # | Question | Decision | Rationale |
|---|----------|----------|-----------|
| 1 | Bridge on which profile? | **Cargo feature only**; **not** in any default profile including **`standard`**. Operator opts into **`full-gui`** or custom feature set. | OPSEC hard line: default `0x0D` surface on all online agents kills on-demand positioning |
| 2 | Auto-stage on Desktop click? | **No.** Explicit two steps: Module panel load `desktop` → then open Desktop view | Operator must know which step pulls multi-MB L2 into memory; matches OPSEC pacing |
| 3 | Frame magic? | **Fixed `CPXD`** | Transport encrypts; wire-seed magic couples parse to crypto init; hurts debug/pcap |
| 4 | WS path? | **Fully deferred.** UI grey-out only. No screenshot-over-JSON | Second path doubles cost; WS concurrency conflict |

---

## 11. Probe conclusions

| Question | Answer |
|----------|--------|
| Is `0x0D` free? | **Yes** — only 0x01–0x04 used |
| Put constants in `profile.rs`? | **No** — HTTP profiles; use `stream_types` |
| JPEG first? | **Yes**; h264 switch bit only |
| L2 mem-map fit? | **Yes** — fileless / inject pattern |
| Auto-stage? | **No** — two-step explicit |
| Multi-viewer? | **No** — server busy reject |
| Biggest product risk? | Input reachability + bandwidth + scale click accuracy |
| Biggest eng risk? | Stage0↔L2 stream ABI + teardown races (mitigated by PR0.5 + idempotent detach) |

**Ready for PR0** (constants) then **PR0.5** (stub bridge).

---

## 12. QA / operator acceptance checklist

Dev criteria: §6.3. This section is **operator/QA sign-off** (human path).

### Preconditions

- [ ] Agent is **TCP** (or TCP-bind) with live Yamux — not WebSocket  
- [ ] Agent binary built with **`desktop` bridge** (`full-gui` / custom) — not stock `standard`  
- [ ] Server has `desktop` module package available  
- [ ] Target has interactive user session for input tests  

### Flow

1. [ ] Select TCP agent → open ClientDetail  
2. [ ] Desktop control is **disabled** or shows “load module first” before Module load  
3. [ ] Module panel → load **desktop** → state Loaded (operator aware of memory window)  
4. [ ] Open Desktop → HELLO → **first frame ≤ 3s**  
5. [ ] Drag mouse → motion lag subjectively acceptable on lab LAN  
6. [ ] Click a known UI control → **action fires on target**  
7. [ ] On **4K / high-DPI** (or simulated `max_w=1280` on large desktop) → click still hits control (scale mapping)  
8. [ ] Set fps → **1** → visible frame rate drop  
9. [ ] STOP → picture freezes / session ends; Desktop can be reopened after  
10. [ ] Unload **desktop** module → Process Explorer: **no** residual encode/capture threads from module  
11. [ ] Second browser/operator tries Desktop on same agent → **busy** (no dual control)  
12. [ ] (Optional MVP) Network blip recover → can load/open again without agent crash  

### Fail / reject signs

- Desktop works on WS agent without Yamux (should not)  
- Clicking Desktop auto-stages multi-MB PE without Module panel  
- Double open-close crashes agent  
- `standard` beacon shows working Desktop without rebuild  

---

## Appendix A — Error code quick ref

| Code | Where | Meaning |
|------|-------|---------|
| `module_not_loaded` | UI / API | Operator skipped Module panel |
| `module_missing` | Agent ERROR frame | Bridge up but PE not Loaded / exports missing |
| `bridge_unavailable` | UI | Agent built without `feature=desktop` |
| `busy` | Server only | Second viewer |
| `encode_unsupported` | Agent | h264 requested, not built |
| `capture_failed` | Agent | DXGI/GDI failure |
| `payload_too_large` | optional | envelope > 2 MiB after valid magic |
