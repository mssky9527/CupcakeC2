// stealth-adv subsystem — version-sensitive enhancements with graceful fallback.
//
// Layer B only. Default path (layer A) must not depend on this module.
//
// Planned / placeholder:
// - nt_user_process: NtCreateUserProcess PPID (implemented MVP)
// - manual map ntdll: future
// - temporary unhook: future
// - aggressive stack spoof: future

#[cfg(windows)]
pub mod nt_user_process;

#[cfg(windows)]
pub use nt_user_process::try_nt_create_user_process_ppid;
