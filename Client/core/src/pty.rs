use portable_pty::{CommandBuilder, NativePtySystem, PtySize, Child, PtySystem};
use std::io::{Read, Write};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::compat::FuturesAsyncReadCompatExt;
use std::panic::catch_unwind;
use std::process::Stdio;
use encoding_rs::GBK;

/// 工业级自适应 PTY 处理器 (支持 GBK 转码与 Windows 8.1 兼容)
pub async fn handle_stream(stream: yamux::Stream) {
    crate::utils::db_print("[PTY] Initiating Cross-Generation Hybrid Shell...");
    
    let (mut net_r, mut net_w) = tokio::io::split(stream.compat());
    let (tx_to_net, mut rx_from_pty) = tokio::sync::mpsc::channel::<Vec<u8>>(4096);
    let (tx_to_pty, rx_from_net) = tokio::sync::mpsc::channel::<Vec<u8>>(4096);

    // 1. 尝试 ConPty (仅 Win10+ 支持)
    let pty_setup = tokio::task::spawn_blocking(move || {
        catch_unwind(|| -> Result<(Box<dyn Child + Send>, Box<dyn Read + Send>, Box<dyn Write + Send>, Box<dyn PtySystem + Send>), String> {
            let pty_system = NativePtySystem::default();
            let pair = pty_system.openpty(PtySize { rows: 24, cols: 80, ..Default::default() })
                .map_err(|e| format!("{:?}", e))?;
            
            let shell = if cfg!(windows) { "cmd.exe" } else { "/bin/bash" };
            let mut cmd = CommandBuilder::new(shell);
            if cfg!(windows) {
                if let Ok(val) = std::env::var("SystemRoot") { cmd.env("SystemRoot", val); }
            }
            cmd.env("TERM", "xterm-256color");
            let child = pair.slave.spawn_command(cmd).map_err(|e| format!("{:?}", e))?;
            let reader = pair.master.try_clone_reader().map_err(|e| format!("{:?}", e))?;
            let writer = pair.master.take_writer().map_err(|e| format!("{:?}", e))?;
            
            // 将 pty_system 传出，以支持后续可能的 Resize (需要 Master)
            Ok((child, reader, writer, Box::new(pty_system)))
        })
    }).await;

    // 2. 核心分发逻辑
    let (child_guard, pty_read_task, pty_write_task) = match pty_setup {
        Ok(Ok(Ok((child, reader, writer, _system)))) => {
            crate::utils::db_print("[PTY] ConPty mode active (Industrial Shell).");
            // 发送 PTY 就绪信号（可选）
            let _ = tx_to_net.send(b"\r\n\x1b[1;32m[+] Industrial PTY Session Started.\x1b[0m\r\n\r\n".to_vec()).await;
            
            struct ConPtyGuard(Box<dyn Child + Send>);
            impl Drop for ConPtyGuard {
                fn drop(&mut self) {
                    let _ = self.0.kill();
                }
            }
            
            let (r, w) = spawn_io_tasks(reader, writer, tx_to_net, rx_from_net);
            (Box::new(ConPtyGuard(child)) as Box<dyn std::any::Any + Send>, r, w)
        },
        _ => {
            crate::utils::db_print("[PTY] ConPty FAILED (Normal on Win7/8 or Linux). Falling back to PipeShell...");
            let (c, r, w) = spawn_pipe_shell(tx_to_net, rx_from_net).await;
            (Box::new(c) as Box<dyn std::any::Any + Send>, r, w)
        }
    };

    // 3. 网络传输循环
    let net_read = async {
        let mut buf = [0u8; 8192];
        while let Ok(n) = net_r.read(&mut buf).await {
            if n == 0 { break; }
            if tx_to_pty.send(buf[..n].to_vec()).await.is_err() { break; }
        }
    };

    let net_write = async {
        while let Some(data) = rx_from_pty.recv().await {
            if net_w.write_all(&data).await.is_err() { break; }
            let _ = net_w.flush().await;
        }
    };

    tokio::select! {
        _ = net_read => {},
        _ = net_write => {},
        _ = pty_read_task => {},
        _ = pty_write_task => {},
    }
    
    // Drop guard naturally, which kills the child process.
    drop(child_guard);
}

fn spawn_io_tasks(
    mut reader: Box<dyn Read + Send>,
    mut writer: Box<dyn Write + Send>,
    tx_to_net: tokio::sync::mpsc::Sender<Vec<u8>>,
    mut rx_from_net: tokio::sync::mpsc::Receiver<Vec<u8>>
) -> (tokio::task::JoinHandle<()>, tokio::task::JoinHandle<()>) {
    let r_task = tokio::task::spawn_blocking(move || {
        let mut buf = [0u8; 16384];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 { break; }
            if tx_to_net.blocking_send(buf[..n].to_vec()).is_err() { break; }
        }
    });
    let w_task = tokio::task::spawn_blocking(move || {
        while let Some(data) = rx_from_net.blocking_recv() {
            if writer.write_all(&data).is_err() { break; }
            let _ = writer.flush();
        }
    });
    (r_task, w_task)
}

/// 🚀 增强版 Windows 管道 Shell：支持 GBK -> UTF8 实时转码
async fn spawn_pipe_shell(
    tx_to_net: tokio::sync::mpsc::Sender<Vec<u8>>,
    mut rx_from_net: tokio::sync::mpsc::Receiver<Vec<u8>>
) -> (tokio::process::Child, tokio::task::JoinHandle<()>, tokio::task::JoinHandle<()>) {
    use tokio::process::Command;
    
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd.exe");
        #[cfg(windows)]
        c.creation_flags(0x08000000); // CREATE_NO_WINDOW
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
        .expect("Failed to spawn shell");

    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();

    let _ = tx_to_net.send(b"{\"type\": \"PTY_MODE\", \"content\": \"fallback\"}".to_vec()).await;
    let _ = tx_to_net.send(b"\r\n\x1b[32m[+] Legacy Pipe Stream Started.\x1b[0m\r\n\r\n".to_vec()).await;

    // 1. Stdout 读取
    let tx_out = tx_to_net.clone();
    let r_out = tokio::spawn(async move {
        let mut buf = [0u8; 16384];
        while let Ok(n) = stdout.read(&mut buf).await {
            if n == 0 { break; }
            let data = &buf[..n];
            
            #[cfg(windows)]
            let payload = {
                let (res, _, has_error) = GBK.decode(data);
                if !has_error { res.as_bytes().to_vec() } else { data.to_vec() }
            };
            #[cfg(not(windows))]
            let payload = data.to_vec();

            if tx_out.send(payload).await.is_err() { break; }
        }
    });

    // 2. Stderr 读取
    let tx_err = tx_to_net.clone();
    let r_err = tokio::spawn(async move {
        let mut buf = [0u8; 16384];
        while let Ok(n) = stderr.read(&mut buf).await {
            if n == 0 { break; }
            let data = &buf[..n];

            #[cfg(windows)]
            let payload = {
                let (res, _, has_error) = GBK.decode(data);
                if !has_error { res.as_bytes().to_vec() } else { data.to_vec() }
            };
            #[cfg(not(windows))]
            let payload = data.to_vec();

            if tx_err.send(payload).await.is_err() { break; }
        }
    });

    // 3. Stdin 处理
    let w_task = tokio::spawn(async move {
        while let Some(data) = rx_from_net.recv().await {
            #[cfg(windows)]
            let final_data = {
                let utf8_str = String::from_utf8_lossy(&data);
                let (gbk_data, _, _) = GBK.encode(&utf8_str);
                gbk_data
            };
            #[cfg(not(windows))]
            let final_data = data;

            if stdin.write_all(&final_data).await.is_err() { break; }
            let _ = stdin.flush().await;
        }
    });

    let combined_r = tokio::spawn(async move { let _ = tokio::join!(r_out, r_err); });

    (child, combined_r, w_task)
}
