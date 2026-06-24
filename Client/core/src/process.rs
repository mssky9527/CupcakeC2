// 进程管理模块
//
// 通过 Yamux Stream 0x04 处理进程操作：列出进程和终止进程

use yamux::Stream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::compat::FuturesAsyncReadCompatExt;
use serde::{Deserialize, Serialize};
use log::{debug, error, info, warn};

/// 进程操作请求
#[derive(Serialize, Deserialize, Debug)]
struct ProcRequest {
    action: String, // "ps" 或 "kill"
    pid: Option<u32>,
}

/// 进程信息条目
#[derive(Serialize, Deserialize, Debug)]
struct ProcessEntry {
    pid: u32,
    ppid: u32,
    name: String,
}

/// 进程操作响应
#[derive(Serialize, Deserialize, Debug)]
struct ProcResponse {
    status: String,
    error: Option<String>,
    processes: Option<Vec<ProcessEntry>>,
}

/// 处理进程管理请求
/// 
/// # 协议格式
/// 
/// 请求：JSON 格式的 ProcRequest
/// 响应：JSON 格式的 ProcResponse
/// 
/// # 支持的操作
/// 
/// - "ps": 列出所有进程
/// - "kill": 终止指定 PID 的进程
pub async fn handle_stream(stream: Stream) {
    info!("[PROCESS] Starting process management session");
    
    let (mut reader, mut writer) = tokio::io::split(stream.compat());

    // 🛡️ FIX: Max request buffer (1MB) to prevent OOM from malicious data
    const MAX_BUF: usize = 1024 * 1024;

    // 1. 读取请求（处理分段）
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let req = loop {
        let n = match reader.read(&mut chunk).await {
            Ok(0) => {
                warn!("[PROCESS] Empty request received");
                break None;
            }
            Ok(n) => {
                debug!("[PROCESS] Received {} bytes", n);
                n
            }
            Err(e) => {
                error!("[PROCESS] Failed to read request: {}", e);
                break None;
            }
        };

        buf.extend_from_slice(&chunk[..n]);

        // 🛡️ FIX: Prevent unbounded memory growth — reject requests over 1MB
        if buf.len() > MAX_BUF {
            error!("[PROCESS] Request too large (>{})", MAX_BUF);
            break None;
        }
        match serde_json::from_slice::<ProcRequest>(&buf) {
            Ok(r) => break Some(r),
            Err(e) if e.is_eof() => continue,
            Err(e) => {
                error!("[PROCESS] Failed to parse request: {}", e);
                let error_response = ProcResponse {
                    status: "error".to_string(),
                    error: Some(format!("Invalid JSON: {}", e)),
                    processes: None,
                };
                let resp_str = serde_json::to_string(&error_response).unwrap_or_default();
                let _ = writer.write_all(resp_str.as_bytes()).await;
                // ⚡️ FIX: Flush and Shutdown explicitly
                let _ = writer.flush().await;
                let _ = writer.shutdown().await;
                break None;
            }
        }
    };

    let Some(req) = req else { return; };
    
    // 2. 执行操作
    let response = match req.action.as_str() {
        "ps" => {
            info!("[PROCESS] Listing processes");
            handle_ps()
        }
        "kill" => {
            let pid = req.pid.unwrap_or(0);
            info!("[PROCESS] Killing process PID: {}", pid);
            handle_kill(pid)
        }
        _ => {
            warn!("[PROCESS] Unknown action: {}", req.action);
            ProcResponse {
                status: "error".to_string(),
                error: Some(format!("Unknown action: {}", req.action)),
                processes: None,
            }
        }
    };
    
    // 3. 发送响应
    let resp_str = match serde_json::to_string(&response) {
        Ok(s) => s,
        Err(e) => {
            error!("[PROCESS] Failed to serialize response: {}", e);
            return;
        }
    };
    
    if let Err(e) = writer.write_all(resp_str.as_bytes()).await {
        error!("[PROCESS] Failed to send response: {}", e);
        return;
    }
    
    // ⚡️ FIX: Flush and Shutdown explicitly
    let _ = writer.flush().await;
    let _ = writer.shutdown().await; // Sends FIN, server sees EOF
    
    debug!("[PROCESS] Response sent successfully");
    info!("[PROCESS] Process management session completed");
}

/// 列出所有进程 (本机版本 - 极致体积优化)
fn handle_ps() -> ProcResponse {
    #[cfg(target_os = "windows")]
    {
        use winapi::um::tlhelp32::{CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS};
        use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
        use winapi::shared::minwindef::TRUE;

        let mut list = Vec::new();
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snapshot != INVALID_HANDLE_VALUE {
                let mut entry: PROCESSENTRY32W = std::mem::zeroed();
                entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

                if Process32FirstW(snapshot, &mut entry) == TRUE {
                    loop {
                        let name = String::from_utf16_lossy(&entry.szExeFile).trim_matches('\0').to_string();
                        list.push(ProcessEntry {
                            pid: entry.th32ProcessID,
                            ppid: entry.th32ParentProcessID,
                            name,
                        });
                        if Process32NextW(snapshot, &mut entry) != TRUE { break; }
                    }
                }
                CloseHandle(snapshot);
            }
        }
        
        info!("[PROCESS] Found {} processes", list.len());
        ProcResponse {
            status: "ok".to_string(),
            error: None,
            processes: Some(list),
        }
    }
    
    #[cfg(not(target_os = "windows"))]
    {
        let mut list = Vec::new();
        if let Ok(entries) = std::fs::read_dir("/proc") {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(pid_str) = path.file_name().and_then(|s| s.to_str()) {
                    if pid_str.chars().all(|c| c.is_digit(10)) {
                        // 1. 获取进程名 (comm)
                        let name = std::fs::read_to_string(path.join("comm"))
                            .unwrap_or_else(|_| "unknown".to_string())
                            .trim()
                            .to_string();
                        
                        // 2. 获取父进程 PID (status)
                        let ppid = if let Ok(status) = std::fs::read_to_string(path.join("status")) {
                            status.lines()
                                .find(|l| l.starts_with("PPid:"))
                                .and_then(|l| l.split_whitespace().nth(1))
                                .and_then(|s| s.parse::<u32>().ok())
                                .unwrap_or(0)
                        } else {
                            0
                        };

                        list.push(ProcessEntry {
                            pid: pid_str.parse::<u32>().unwrap_or(0),
                            ppid,
                            name,
                        });
                    }
                }
            }
        }
        
        info!("[PROCESS] [Linux] Found {} processes", list.len());
        ProcResponse {
            status: "ok".to_string(),
            error: None,
            processes: Some(list),
        }
    }
}

/// 终止指定进程 (本机版本)
fn handle_kill(pid_u32: u32) -> ProcResponse {
    if pid_u32 == 0 {
        return ProcResponse { status: "error".to_string(), error: Some("Invalid PID".to_string()), processes: None };
    }
    
    #[cfg(target_os = "windows")]
    {
        use winapi::um::processthreadsapi::{OpenProcess, TerminateProcess};
        use winapi::um::winnt::PROCESS_TERMINATE;
        use winapi::um::handleapi::CloseHandle;
        use winapi::shared::minwindef::FALSE;

        unsafe {
            let h = OpenProcess(PROCESS_TERMINATE, FALSE, pid_u32);
            if !h.is_null() {
                let res = TerminateProcess(h, 1);
                CloseHandle(h);
                if res != FALSE {
                    info!("[PROCESS] Successfully killed process PID: {}", pid_u32);
                    ProcResponse { status: "ok".to_string(), error: None, processes: None }
                } else {
                    ProcResponse { status: "error".to_string(), error: Some("Failed to kill process".to_string()), processes: None }
                }
            } else {
                ProcResponse { status: "error".to_string(), error: Some("Access denied or process not found".to_string()), processes: None }
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        ProcResponse { status: "error".to_string(), error: Some("Kill not implemented".to_string()), processes: None }
    }
}
