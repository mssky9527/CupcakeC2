// SOCKS5 代理处理模块
//
// 当服务器通过 Yamux 发起 SOCKS 代理请求时（YAMUX_STREAM_SOCKS），
// 客户端读取目标地址并建立到目标的 TCP 连接，然后进行双向数据转发

use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use yamux::Stream;
use tokio_util::compat::FuturesAsyncReadCompatExt;
use log::{debug, error, info, warn};
use std::time::Duration;

/// 处理 SOCKS 代理请求
/// 
/// # 协议格式
/// 
/// 服务器发送：[Host_Len(u8)] [Host_Bytes] [Port(u16_be)]
/// 注意：Type Byte 0x02 已被调度器消费
/// 
/// # 参数
/// 
/// * `stream` - Yamux 流，用于与服务器通信
/// 
/// # 工作流程
/// 
/// 1. 从 Yamux 流读取目标地址（主机名/IP + 端口）
/// 2. 建立到目标的 TCP 连接
/// 3. 双向转发数据：Server ↔ Agent ↔ Target
pub async fn handle_stream(stream: Stream) {
    debug!("[SOCKS] Starting SOCKS proxy session");
    
    // 1. 读取目标地址
    // 协议格式: [Host_Len(u8)] [Host_Bytes] [Port(u16_be)]
    let mut stream_compat = stream.compat();
    
    // 读取主机名长度
    let mut len_buf = [0u8; 1];
    if let Err(e) = stream_compat.read_exact(&mut len_buf).await {
        error!("[SOCKS] Failed to read host length: {}", e);
        return;
    }
    let host_len = len_buf[0] as usize;
    
    if host_len == 0 || host_len > 255 {
        error!("[SOCKS] Invalid host length: {}", host_len);
        return;
    }
    
    // 读取主机名
    let mut host_buf = vec![0u8; host_len];
    if let Err(e) = stream_compat.read_exact(&mut host_buf).await {
        error!("[SOCKS] Failed to read host: {}", e);
        return;
    }
    let host_str = String::from_utf8_lossy(&host_buf).to_string();
    
    // 读取端口（大端序）
    let mut port_buf = [0u8; 2];
    if let Err(e) = stream_compat.read_exact(&mut port_buf).await {
        error!("[SOCKS] Failed to read port: {}", e);
        return;
    }
    let port = u16::from_be_bytes(port_buf);
    
    let target_addr = format!("{}:{}", host_str, port);
    info!("[SOCKS] Connect request to: {}", target_addr);
    
    // 2. 连接到目标服务器（带超时保护）
    let target_stream = match connect_to_target(&target_addr).await {
        Ok(s) => s,
        Err(_) => {
            // Agent 发送 0x00 表示连接失败 → Server 回复 SOCKS5 拒绝
            let _ = stream_compat.write_all(&[0x00]).await;
            return;
        }
    };

    // 发送成功响应给服务器
    if let Err(e) = stream_compat.write_all(&[0x01]).await {
        error!("[SOCKS] Failed to send success response: {}", e);
        return;
    }
    
    // 3. 建立双向数据管道
    // Server (Yamux) ↔ Agent ↔ Target (TCP)
    let (mut client_r, mut client_w) = tokio::io::split(stream_compat);
    let (mut target_r, mut target_w) = target_stream.into_split();
    
    // 任务 1: Client → Target
    let client_to_target = async {
        match tokio::io::copy(&mut client_r, &mut target_w).await {
            Ok(n) => {
                debug!("[SOCKS] Client→Target: {} bytes transferred", n);
            }
            Err(e) => {
                warn!("[SOCKS] Client→Target error: {}", e);
            }
        }
    };
    
    // 任务 2: Target → Client
    let target_to_client = async {
        match tokio::io::copy(&mut target_r, &mut client_w).await {
            Ok(n) => {
                debug!("[SOCKS] Target→Client: {} bytes transferred", n);
            }
            Err(e) => {
                warn!("[SOCKS] Target→Client error: {}", e);
            }
        }
    };
    
    // 并发执行双向转发，任一方向结束则终止
    tokio::join!(client_to_target, target_to_client);
    
    info!("[SOCKS] Connection closed: {}", target_addr);
}

/// 🛡️ FIX: 连接到目标主机（带 15 秒超时 + TCP_NODELAY）
///
/// 防止因目标主机不可达导致 Agent 长时间阻塞（OS TCP 超时可长达 120 秒），
/// 同时设置 NODELAY 降低隧道延迟。
async fn connect_to_target(addr: &str) -> Result<TcpStream, ()> {
    match tokio::time::timeout(Duration::from_secs(15), TcpStream::connect(addr)).await {
        Ok(Ok(s)) => {
            info!("[SOCKS] Successfully connected to {}", addr);
            // 启用 TCP_NODELAY 减少隧道延迟（小包不等待合并）
            let _ = s.set_nodelay(true);
            Ok(s)
        }
        Ok(Err(e)) => {
            error!("[SOCKS] Failed to connect to {}: {}", addr, e);
            Err(())
        }
        Err(_) => {
            error!("[SOCKS] Timeout connecting to {} (15s)", addr);
            Err(())
        }
    }
}
