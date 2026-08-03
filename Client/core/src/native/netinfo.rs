// Network adapter enumeration via IP Helper (no ipconfig / shell).

use crate::stealth;

#[derive(Debug, Clone)]
pub struct AdapterInfo {
    pub name: String,
    pub description: String,
    pub friendly: String,
    pub ipv4: Vec<String>,
    pub ipv6: Vec<String>,
    pub mac: String,
    pub oper_status: u32,
    pub if_type: u32,
}

/// List adapters + unicast addresses (GetAdaptersAddresses).
pub fn list_adapters() -> Result<Vec<AdapterInfo>, String> {
    #[cfg(windows)]
    {
        unsafe { list_adapters_win() }
    }
    #[cfg(not(windows))]
    {
        list_adapters_unix()
    }
}

/// Classic `ipconfig`-style human text (not JSON).
pub fn format_adapters_text() -> Result<String, String> {
    let list = list_adapters()?;
    let mut out = String::from("\r\nWindows IP Configuration\r\n\r\n");
    if list.is_empty() {
        out.push_str("No adapters found.\r\n");
        return Ok(out);
    }
    for a in &list {
        let title = if !a.friendly.is_empty() {
            a.friendly.clone()
        } else if !a.description.is_empty() {
            a.description.clone()
        } else {
            a.name.clone()
        };
        let media = if_type_str(a.if_type);
        out.push_str(&format!("{} adapter {}:\r\n\r\n", media, title));
        out.push_str(&format!(
            "   Media State . . . . . . . . . . . : {}\r\n",
            oper_status_str(a.oper_status)
        ));
        if !a.description.is_empty() && a.description != title {
            out.push_str(&format!(
                "   Description . . . . . . . . . . . : {}\r\n",
                a.description
            ));
        }
        if !a.mac.is_empty() {
            out.push_str(&format!(
                "   Physical Address. . . . . . . . . : {}\r\n",
                a.mac
            ));
        }
        if a.ipv4.is_empty() && a.ipv6.is_empty() {
            out.push_str("   Autoconfiguration Enabled. . . . : Yes\r\n");
        }
        for ip in &a.ipv4 {
            out.push_str(&format!(
                "   IPv4 Address. . . . . . . . . . . : {}\r\n",
                ip
            ));
        }
        for ip in &a.ipv6 {
            out.push_str(&format!(
                "   IPv6 Address. . . . . . . . . . . : {}\r\n",
                ip
            ));
        }
        out.push_str("\r\n");
    }
    Ok(out)
}

fn oper_status_str(s: u32) -> &'static str {
    match s {
        1 => "Media connected",
        2 => "Media disconnected",
        3 => "Testing",
        4 => "Unknown",
        5 => "Dormant",
        6 => "Not present",
        7 => "Lower layer down",
        _ => "Other",
    }
}

fn if_type_str(t: u32) -> &'static str {
    // Common IF_TYPE_* values
    match t {
        6 => "Ethernet",
        71 => "Wireless LAN",
        24 => "Tunnel",
        23 => "PPP",
        131 => "Tunnel",
        53 => "Proprietary virtual",
        _ => "Network",
    }
}

#[cfg(windows)]
unsafe fn list_adapters_win() -> Result<Vec<AdapterInfo>, String> {
    // Load iphlpapi
    let mut iphlp = stealth::get_module_base(stealth::hash_module_name(b"iphlpapi.dll"));
    if iphlp == 0 {
        let k32 = stealth::get_module_base(stealth::hash_module_name(b"kernel32.dll"));
        if let Some(load) = stealth::get_api_addr(k32, stealth::hash_api_name(b"LoadLibraryA")) {
            let load_library: unsafe extern "system" fn(*const i8) -> usize =
                std::mem::transmute(load);
            iphlp = load_library(b"iphlpapi.dll\0".as_ptr() as *const i8);
        }
    }
    if iphlp == 0 {
        return Err("iphlpapi.dll not found".into());
    }

    type GetAdaptersAddressesFn = unsafe extern "system" fn(
        u32, // Family
        u32, // Flags
        *mut u8,
        *mut u8, // buffer
        *mut u32,
    ) -> u32;

    let gaa: GetAdaptersAddressesFn = std::mem::transmute(
        stealth::get_api_addr(iphlp, stealth::hash_api_name(b"GetAdaptersAddresses"))
            .ok_or("GetAdaptersAddresses unresolved")?,
    );

    const AF_UNSPEC: u32 = 0;
    // GAA_FLAG_INCLUDE_PREFIX | SKIP_ANYCAST | SKIP_MULTICAST | SKIP_DNS
    const FLAGS: u32 = 0x0010 | 0x0002 | 0x0004 | 0x0008;

    let mut size: u32 = 16 * 1024;
    let mut buf = vec![0u8; size as usize];
    let mut ret = gaa(
        AF_UNSPEC,
        FLAGS,
        std::ptr::null_mut(),
        buf.as_mut_ptr(),
        &mut size,
    );
    if ret == 111
    /* ERROR_BUFFER_OVERFLOW */
    {
        buf.resize(size as usize, 0);
        ret = gaa(
            AF_UNSPEC,
            FLAGS,
            std::ptr::null_mut(),
            buf.as_mut_ptr(),
            &mut size,
        );
    }
    if ret != 0 {
        return Err(format!("GetAdaptersAddresses failed: {}", ret));
    }

    parse_adapters_buffer(&buf)
}

#[cfg(windows)]
unsafe fn parse_adapters_buffer(buf: &[u8]) -> Result<Vec<AdapterInfo>, String> {
    // IP_ADAPTER_ADDRESSES (Vista+ / Win10) pointer packing:
    // x64: Next@0x08 AdapterName@0x10 FirstUnicast@0x18
    //      DnsSuffix@0x38 Description@0x40 FriendlyName@0x48
    //      PhysicalAddress@0x50 PhysicalAddressLength@0x58 IfType@0x64 OperStatus@0x68
    // x86: scaled for 32-bit pointers (approx half between pointer fields).

    #[cfg(target_arch = "x86_64")]
    let (desc_off, friendly_off, phys_off, phys_len_off, if_type_off, oper_off) = (
        0x40usize, 0x48usize, 0x50usize, 0x58usize, 0x64usize, 0x68usize,
    );
    #[cfg(target_arch = "x86")]
    let (desc_off, friendly_off, phys_off, phys_len_off, if_type_off, oper_off) = (
        0x24usize, 0x28usize, 0x2Cusize, 0x34usize, 0x3Cusize, 0x40usize,
    );

    let mut out = Vec::new();
    let mut cur = buf.as_ptr() as usize;
    let end = buf.as_ptr() as usize + buf.len();
    let mut guard = 0;

    while cur != 0 && cur >= buf.as_ptr() as usize && cur < end && guard < 256 {
        guard += 1;
        let base = cur as *const u8;
        let length = read_u32(base, 0) as usize;
        if length < 0x48 {
            break;
        }

        let next = read_usize(base, 0x08);
        let adapter_name_ptr = read_usize(base, 0x10) as *const i8;
        let first_uni = read_usize(base, 0x18);

        let name = cstr(adapter_name_ptr);
        let description = wstr(read_usize(base, desc_off) as *const u16);
        let friendly = wstr(read_usize(base, friendly_off) as *const u16);
        let phys_len = read_u32(base, phys_len_off).min(8) as usize;
        let mac = if phys_len > 0 && phys_off + phys_len <= length {
            let bytes = std::slice::from_raw_parts(base.add(phys_off), phys_len);
            bytes
                .iter()
                .map(|b| format!("{:02X}", b))
                .collect::<Vec<_>>()
                .join("-")
        } else {
            String::new()
        };
        let if_type = if if_type_off + 4 <= length {
            read_u32(base, if_type_off)
        } else {
            0
        };
        let oper_status = if oper_off + 4 <= length {
            read_u32(base, oper_off)
        } else {
            0
        };

        let (ipv4, ipv6) = collect_unicast(first_uni);

        out.push(AdapterInfo {
            name,
            description,
            friendly,
            ipv4,
            ipv6,
            mac,
            oper_status,
            if_type,
        });

        if next == 0 {
            break;
        }
        cur = next;
    }

    Ok(out)
}

#[cfg(windows)]
unsafe fn collect_unicast(first: usize) -> (Vec<String>, Vec<String>) {
    let mut v4 = Vec::new();
    let mut v6 = Vec::new();
    if first == 0 {
        return (v4, v6);
    }
    // IP_ADAPTER_UNICAST_ADDRESS: Length, Flags, Next, Address (SOCKET_ADDRESS)
    // SOCKET_ADDRESS: lpSockaddr, iSockaddrLength
    let mut cur = first;
    let mut guard = 0;
    while cur != 0 && guard < 64 {
        guard += 1;
        let base = cur as *const u8;
        let next = read_usize(base, 0x08);
        let sa = read_usize(base, 0x10) as *const u8; // lpSockaddr
        if !sa.is_null() {
            let family = read_u16(sa, 0);
            if family == 2 {
                // AF_INET
                let b = std::slice::from_raw_parts(sa.add(4), 4);
                v4.push(format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3]));
            } else if family == 23 {
                // AF_INET6
                let b = std::slice::from_raw_parts(sa.add(8), 16);
                v6.push(format_ipv6(b));
            }
        }
        cur = next;
    }
    (v4, v6)
}

#[cfg(windows)]
fn format_ipv6(b: &[u8]) -> String {
    let mut parts = Vec::new();
    for i in (0..16).step_by(2) {
        parts.push(format!("{:x}", u16::from_be_bytes([b[i], b[i + 1]])));
    }
    parts.join(":")
}

#[cfg(windows)]
unsafe fn cstr(p: *const i8) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    while *p.add(len) != 0 && len < 512 {
        len += 1;
    }
    String::from_utf8_lossy(std::slice::from_raw_parts(p as *const u8, len)).into_owned()
}

#[cfg(windows)]
unsafe fn wstr(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    while *p.add(len) != 0 && len < 512 {
        len += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
}

#[cfg(windows)]
unsafe fn read_u16(base: *const u8, off: usize) -> u16 {
    std::ptr::read_unaligned(base.add(off) as *const u16)
}
#[cfg(windows)]
unsafe fn read_u32(base: *const u8, off: usize) -> u32 {
    std::ptr::read_unaligned(base.add(off) as *const u32)
}
#[cfg(windows)]
unsafe fn read_usize(base: *const u8, off: usize) -> usize {
    std::ptr::read_unaligned(base.add(off) as *const usize)
}

#[cfg(not(windows))]
fn list_adapters_unix() -> Result<Vec<AdapterInfo>, String> {
    // Lightweight: parse /proc/net or use std only — list interface names from /sys/class/net
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir("/sys/class/net") {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let oper = std::fs::read_to_string(e.path().join("operstate"))
                .unwrap_or_default()
                .trim()
                .to_string();
            let status = if oper == "up" { 1 } else { 2 };
            let mac = std::fs::read_to_string(e.path().join("address"))
                .unwrap_or_default()
                .trim()
                .to_uppercase();
            out.push(AdapterInfo {
                name: name.clone(),
                description: String::new(),
                friendly: name,
                ipv4: Vec::new(),
                ipv6: Vec::new(),
                mac,
                oper_status: status,
                if_type: 0,
            });
        }
    }
    Ok(out)
}
