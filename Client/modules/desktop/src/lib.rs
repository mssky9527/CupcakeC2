//! L2 `mod_desktop` — C ABI surface for mem-map load.
//!
//! Capture/session engine is `cupcake_core::transport::desktop_engine` (feature desktop).
//! Stage0 with feature desktop can also run the engine in-process; this crate packages
//! the same lifecycle for explicit Module panel loading.

use std::os::raw::{c_int, c_uint};

#[no_mangle]
pub extern "C" fn mod_init() -> c_int {
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
) -> c_int {
    if out_ptr.is_null() || out_len.is_null() {
        return -1;
    }
    *out_ptr = std::ptr::null_mut();
    *out_len = 0;
    let ct = if cmd_type.is_null() || cmd_type_len == 0 {
        "desktop_probe"
    } else {
        match std::str::from_utf8(std::slice::from_raw_parts(cmd_type, cmd_type_len as usize)) {
            Ok(s) => s,
            Err(_) => return -2,
        }
    };
    let json = match ct {
        "desktop_probe" => cupcake_core::transport::desktop_engine::probe().to_string(),
        "desktop_stop" => {
            let _ = cupcake_core::transport::desktop_engine::stream_detach(0);
            r#"{"ok":true}"#.to_string()
        }
        _ => format!(r#"{{"ok":false,"stderr":"unsupported {ct}"}}"#),
    };
    write_out(out_ptr, out_len, &json)
}

#[no_mangle]
pub unsafe extern "C" fn mod_free(ptr: *mut u8, len: u32) {
    if ptr.is_null() || len == 0 {
        return;
    }
    let _ = Vec::from_raw_parts(ptr, len as usize, len as usize);
}

#[no_mangle]
pub extern "C" fn mod_shutdown() -> c_int {
    cupcake_core::transport::desktop_engine::stream_detach(0);
    0
}

#[no_mangle]
pub extern "C" fn mod_stream_attach(session_token: c_uint) -> c_int {
    // session_token reserved; use product defaults (bridge may pass 0).
    let _ = session_token;
    match cupcake_core::transport::desktop_engine::stream_attach(1280, 5, 75) {
        Ok(_tok) => 0, // subsequent calls use token 0 = current session
        Err(e) => e,
    }
}

#[no_mangle]
pub unsafe extern "C" fn mod_stream_poll_frame(
    session_token: c_uint,
    timeout_ms: c_uint,
    out_hdr: *mut u8,
    hdr_cap: c_uint,
    hdr_len: *mut c_uint,
    out_blob: *mut *mut u8,
    blob_len: *mut c_uint,
) -> c_int {
    if hdr_len.is_null() || out_blob.is_null() || blob_len.is_null() {
        return -2;
    }
    *hdr_len = 0;
    *out_blob = std::ptr::null_mut();
    *blob_len = 0;
    match cupcake_core::transport::desktop_engine::stream_poll_frame(session_token, timeout_ms) {
        Ok(None) => 0,
        Ok(Some(payload)) => {
            // pack: no separate hdr; full FRAME payload in blob
            let mut v = payload.into_boxed_slice();
            let len = v.len() as u32;
            *blob_len = len;
            *out_blob = v.as_mut_ptr();
            std::mem::forget(v);
            if !out_hdr.is_null() && hdr_cap >= 4 {
                // write seq hint
                let b = 0u32.to_le_bytes();
                std::ptr::copy_nonoverlapping(b.as_ptr(), out_hdr, 4);
                *hdr_len = 4;
            }
            0
        }
        Err(e) => e,
    }
}

#[no_mangle]
pub unsafe extern "C" fn mod_stream_push_input(
    session_token: c_uint,
    msg: *const u8,
    len: c_uint,
) -> c_int {
    if msg.is_null() || len == 0 {
        return -2;
    }
    let slice = std::slice::from_raw_parts(msg, len as usize);
    cupcake_core::transport::desktop_engine::stream_push_input(session_token, slice)
}

#[no_mangle]
pub extern "C" fn mod_stream_detach(session_token: c_uint) -> c_int {
    cupcake_core::transport::desktop_engine::stream_detach(session_token)
}

unsafe fn write_out(out_ptr: *mut *mut u8, out_len: *mut u32, s: &str) -> c_int {
    let mut v = s.as_bytes().to_vec().into_boxed_slice();
    *out_len = v.len() as u32;
    *out_ptr = v.as_mut_ptr();
    std::mem::forget(v);
    0
}

#[cfg(test)]
mod tests {
    #[test]
    fn c_abi_attach_poll_detach() {
        let _g = cupcake_core::transport::desktop_engine::desktop_test_lock();
        // token 0 = wildcard current session (C ABI attach does not return generation token)
        assert_eq!(super::mod_stream_attach(0), 0);
        let mut hdr = [0u8; 8];
        let mut hdr_len = 0u32;
        let mut blob: *mut u8 = std::ptr::null_mut();
        let mut blob_len = 0u32;
        let rc = unsafe {
            super::mod_stream_poll_frame(
                0,
                10,
                hdr.as_mut_ptr(),
                8,
                &mut hdr_len,
                &mut blob,
                &mut blob_len,
            )
        };
        assert_eq!(rc, 0);
        assert!(blob_len > 0);
        unsafe {
            super::mod_free(blob, blob_len);
        }
        assert_eq!(super::mod_stream_detach(0), 0);
        assert_eq!(super::mod_stream_detach(0), 0);
    }
}
