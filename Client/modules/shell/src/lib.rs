//! L2 `mod_shell` — hybrid terminal module for Stage0.
//!
//! Exports C ABI expected by Stage0 `module_loader`:
//! - `mod_init`
//! - `mod_invoke(cmd_type, payload) -> JSON result buffer`
//! - `mod_free`
//! - `mod_shutdown`

use cupcake_core::executor::CommandExecutor;
use std::sync::OnceLock;
use tokio::runtime::Runtime;

fn runtime() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("mod_shell runtime")
    })
}

/// Called once after LoadLibrary / map.
#[no_mangle]
pub extern "C" fn mod_init() -> i32 {
    let _ = runtime();
    0
}

/// Invoke a command. `cmd_type` is typically `shell`; payload is UTF-8 command line.
/// On success writes a heap buffer `{"stdout":"...","stderr":"..."}` via out_ptr/out_len.
/// Caller must free with `mod_free`.
#[no_mangle]
pub unsafe extern "C" fn mod_invoke(
    cmd_type: *const u8,
    cmd_type_len: u32,
    payload: *const u8,
    payload_len: u32,
    out_ptr: *mut *mut u8,
    out_len: *mut u32,
) -> i32 {
    if out_ptr.is_null() || out_len.is_null() {
        return -1;
    }
    *out_ptr = std::ptr::null_mut();
    *out_len = 0;

    let ct = if cmd_type.is_null() || cmd_type_len == 0 {
        "shell"
    } else {
        match std::str::from_utf8(std::slice::from_raw_parts(cmd_type, cmd_type_len as usize)) {
            Ok(s) => s,
            Err(_) => return -2,
        }
    };

    let body = if payload.is_null() || payload_len == 0 {
        ""
    } else {
        match std::str::from_utf8(std::slice::from_raw_parts(payload, payload_len as usize)) {
            Ok(s) => s,
            Err(_) => return -3,
        }
    };

    let result = match ct {
        "shell" | "shell_interactive" => runtime().block_on(CommandExecutor::execute(body)),
        other => {
            let msg = format!("mod_shell: unsupported cmd_type '{other}'");
            return write_json_out(out_ptr, out_len, "", &msg);
        }
    };

    write_json_out(
        out_ptr,
        out_len,
        &result.stdout,
        &result.stderr,
    )
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
    0
}

unsafe fn write_json_out(
    out_ptr: *mut *mut u8,
    out_len: *mut u32,
    stdout: &str,
    stderr: &str,
) -> i32 {
    let v = serde_json::json!({
        "stdout": stdout,
        "stderr": stderr,
        "path": null,
    });
    let mut bytes = match serde_json::to_vec(&v) {
        Ok(b) => b,
        Err(_) => return -4,
    };
    // Cap must equal len for mod_free reconstruction
    bytes.shrink_to_fit();
    let len = bytes.len() as u32;
    let ptr = bytes.as_mut_ptr();
    std::mem::forget(bytes);
    *out_ptr = ptr;
    *out_len = len;
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invoke_echo_help() {
        assert_eq!(mod_init(), 0);
        let cmd = b"shell";
        let pay = b"help";
        let mut out_ptr: *mut u8 = std::ptr::null_mut();
        let mut out_len: u32 = 0;
        let rc = unsafe {
            mod_invoke(
                cmd.as_ptr(),
                cmd.len() as u32,
                pay.as_ptr(),
                pay.len() as u32,
                &mut out_ptr,
                &mut out_len,
            )
        };
        assert_eq!(rc, 0);
        assert!(!out_ptr.is_null());
        let s = unsafe {
            std::str::from_utf8(std::slice::from_raw_parts(out_ptr, out_len as usize)).unwrap()
        };
        assert!(s.contains("stdout") || s.contains("help") || s.contains("Built"));
        unsafe { mod_free(out_ptr, out_len) };
        assert_eq!(mod_shutdown(), 0);
    }
}
