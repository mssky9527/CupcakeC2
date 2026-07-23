// Windows version detection — layer A (version-agnostic implementation).
// Used by stealth-adv gates; safe to call from default path.

use std::sync::OnceLock;

/// Minimum OS build for attempting NtCreateUserProcess PPID path (Win10 1809).
pub const NT_CREATE_USER_PROCESS_MIN_BUILD: u32 = 17763;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowsVersion {
    pub major: u32,
    pub minor: u32,
    pub build: u32,
}

impl WindowsVersion {
    pub const UNKNOWN: Self = Self {
        major: 0,
        minor: 0,
        build: 0,
    };

    /// Pure gate logic (unit-testable without OS).
    pub fn supports_nt_create_user_process(self) -> bool {
        self.major >= 10 && self.build >= NT_CREATE_USER_PROCESS_MIN_BUILD
    }
}

/// Cached OS version for the process lifetime.
pub fn get_windows_version() -> WindowsVersion {
    static CACHED: OnceLock<WindowsVersion> = OnceLock::new();
    *CACHED.get_or_init(|| {
        #[cfg(windows)]
        {
            unsafe { detect_windows_version() }
        }
        #[cfg(not(windows))]
        {
            WindowsVersion::UNKNOWN
        }
    })
}

/// Runtime gate for version-sensitive NtCreateUserProcess path.
pub fn is_supported_for_nt_create_user_process() -> bool {
    get_windows_version().supports_nt_create_user_process()
}

#[cfg(windows)]
unsafe fn detect_windows_version() -> WindowsVersion {
    if let Some(v) = version_from_peb() {
        if v.major != 0 {
            return v;
        }
    }
    if let Some(v) = version_from_rtl_get_version() {
        return v;
    }
    WindowsVersion::UNKNOWN
}

/// PEB OSMajorVersion / OSMinorVersion / OSBuildNumber.
/// x64: +0x118 / +0x11C / +0x120
/// x86: +0xA4 / +0xA8 / +0xAC
#[cfg(windows)]
unsafe fn version_from_peb() -> Option<WindowsVersion> {
    let peb: *const u8;
    #[cfg(target_arch = "x86_64")]
    {
        let p: *const usize;
        std::arch::asm!("mov {}, gs:[0x60]", out(reg) p);
        peb = p as *const u8;
    }
    #[cfg(target_arch = "x86")]
    {
        let p: *const usize;
        std::arch::asm!("mov {}, fs:[0x30]", out(reg) p);
        peb = p as *const u8;
    }

    if peb.is_null() {
        return None;
    }

    #[cfg(target_arch = "x86_64")]
    let (maj_off, min_off, build_off) = (0x118usize, 0x11Cusize, 0x120usize);
    #[cfg(target_arch = "x86")]
    let (maj_off, min_off, build_off) = (0xA4usize, 0xA8usize, 0xACusize);

    let major = std::ptr::read_unaligned(peb.add(maj_off) as *const u32);
    let minor = std::ptr::read_unaligned(peb.add(min_off) as *const u32);
    let build = std::ptr::read_unaligned(peb.add(build_off) as *const u32);

    // Sanity: modern Windows major is 6 or 10; build non-zero on Win10+
    if major == 0 || major > 20 {
        return None;
    }
    Some(WindowsVersion {
        major,
        minor,
        build,
    })
}

#[cfg(windows)]
#[repr(C)]
struct OsVersionInfoW {
    os_version_info_size: u32,
    major_version: u32,
    minor_version: u32,
    build_number: u32,
    platform_id: u32,
    csd_version: [u16; 128],
}

#[cfg(windows)]
unsafe fn version_from_rtl_get_version() -> Option<WindowsVersion> {
    let ntdll = crate::stealth::get_module_base(crate::stealth::hash_module_name(b"ntdll.dll"));
    if ntdll == 0 {
        return None;
    }
    let addr = crate::stealth::get_api_addr(
        ntdll,
        crate::stealth::hash_api_name(b"RtlGetVersion"),
    )?;
    type RtlGetVersionFn = unsafe extern "system" fn(*mut OsVersionInfoW) -> i32;
    let rtl: RtlGetVersionFn = std::mem::transmute(addr);

    let mut info: OsVersionInfoW = std::mem::zeroed();
    info.os_version_info_size = std::mem::size_of::<OsVersionInfoW>() as u32;
    let status = rtl(&mut info);
    if status < 0 {
        return None;
    }
    Some(WindowsVersion {
        major: info.major_version,
        minor: info.minor_version,
        build: info.build_number,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_rejects_unknown_and_legacy() {
        assert!(!WindowsVersion::UNKNOWN.supports_nt_create_user_process());
        assert!(
            !WindowsVersion {
                major: 6,
                minor: 3,
                build: 9600
            }
            .supports_nt_create_user_process()
        );
        assert!(
            !WindowsVersion {
                major: 10,
                minor: 0,
                build: 17134
            }
            .supports_nt_create_user_process()
        ); // 1803
    }

    #[test]
    fn gate_accepts_1809_and_win11() {
        assert!(
            WindowsVersion {
                major: 10,
                minor: 0,
                build: 17763
            }
            .supports_nt_create_user_process()
        );
        assert!(
            WindowsVersion {
                major: 10,
                minor: 0,
                build: 19045
            }
            .supports_nt_create_user_process()
        );
        assert!(
            WindowsVersion {
                major: 10,
                minor: 0,
                build: 22631
            }
            .supports_nt_create_user_process()
        );
    }

    #[test]
    fn min_build_constant() {
        assert_eq!(NT_CREATE_USER_PROCESS_MIN_BUILD, 17763);
    }
}
