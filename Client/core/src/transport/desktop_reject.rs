//! Always-available reject path for Yamux DESKTOP when `feature = "desktop"` is off.
//! Ensures the server receives a CPXD ERROR instead of a silent stream drop.

use crate::transport::desktop_proto::{encode_message, parse_header, DESKTOP_HEADER_LEN, MSG_ERROR};
use futures_util::{AsyncReadExt, AsyncWriteExt};

/// Drain optional first envelope (HELLO), then send ERROR `bridge_unavailable` and close.
pub async fn reject_desktop_stream(mut stream: yamux::Stream) {
    // Best-effort: consume HELLO so server's write completes; then reply ERROR.
    let mut hdr = [0u8; DESKTOP_HEADER_LEN];
    if stream.read_exact(&mut hdr).await.is_ok() {
        if let Ok(env) = parse_header(&hdr) {
            if env.payload_len > 0 && env.payload_len <= 2 * 1024 * 1024 {
                let mut discard = vec![0u8; env.payload_len as usize];
                let _ = stream.read_exact(&mut discard).await;
            }
        }
    }
    let body = br#"{"code":"module_not_loaded","msg":"load L2 module desktop from Module panel first (or rebuild agent with feature=desktop)"}"#;
    if let Some(msg) = encode_message(MSG_ERROR, 0, body) {
        let _ = stream.write_all(&msg).await;
    }
    let _ = stream.close().await;
}
