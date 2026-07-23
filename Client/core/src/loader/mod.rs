// BOF-only loader surface (process injection / migrate / hollow removed).

#[cfg(all(feature = "bof", target_os = "windows"))]
pub mod bof;

#[cfg(all(feature = "bof", target_os = "windows"))]
pub mod error;

#[cfg(all(feature = "bof", target_os = "windows"))]
pub mod beacon_api;

#[cfg(all(feature = "bof", target_os = "windows"))]
pub mod safety;

#[cfg(all(feature = "bof", target_os = "windows"))]
pub use error::{BofError, BofResult};
