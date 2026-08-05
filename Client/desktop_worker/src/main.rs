//! `desktop_worker` — long-lived process for RDP dial + byte relay.
//!
//! Architecture (see docs/MODULE_WORKER_ISOLATION.md):
//!   Stage0 ModuleSupervisor  →  spawn this PE under a Job Object
//!   this process             →  owns TCP dial to target:3389 + bidirectional relay
//!   Stage0                   →  request id, start/stop, deadline, cleanup only
//!
//! **Not product default.** Enabled only when Stage0 sets `CUPCAKE_DESKTOP_WORKER=1`
//! and can resolve this binary. Default product path remains in-process `desktop_bridge`.
//!
//! ## Protocol (stdin/stdout pipes)
//! Parent spawns: `cupcake-desktop-worker relay <host> <port>`
//! 1. Worker dials TCP (`host:port`) with 15s connect timeout
//! 2. On success: write `READY\n` to stdout, then raw duplex:
//!    - stdin  → TCP
//!    - TCP    → stdout
//! 3. On dial failure: write `ERR <msg>\n` to stdout, exit 1
//! 4. Idle: optional 120s read timeout on the TCP socket

#![windows_subsystem = "windows"]

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::process;
use std::thread;
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const IDLE_READ_TIMEOUT: Duration = Duration::from_secs(120);

fn main() {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("relay") => {
            let host = match args.next() {
                Some(h) if !h.is_empty() => h,
                _ => {
                    write_err("missing host");
                    process::exit(1);
                }
            };
            let port = match args.next() {
                Some(p) if !p.is_empty() => p,
                _ => {
                    write_err("missing port");
                    process::exit(1);
                }
            };
            if args.next().is_some() {
                write_err("too many args");
                process::exit(1);
            }
            run_relay(&host, &port);
        }
        // Bare invoke (no mode): stable exit 2 = "no mode / not a default product entry".
        _ => process::exit(2),
    }
}

fn run_relay(host: &str, port: &str) {
    let addr = match resolve_addr(host, port) {
        Ok(a) => a,
        Err(e) => {
            write_err(&e);
            process::exit(1);
        }
    };

    let stream = match TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT) {
        Ok(s) => s,
        Err(e) => {
            write_err(&format!("connect {addr}: {e}"));
            process::exit(1);
        }
    };
    let _ = stream.set_nodelay(true);
    let _ = stream.set_read_timeout(Some(IDLE_READ_TIMEOUT));
    // Writes should not hang forever if the peer stalls mid-frame.
    let _ = stream.set_write_timeout(Some(IDLE_READ_TIMEOUT));

    // Handshake complete — switch stdout to raw byte relay after READY line.
    {
        let mut out = std::io::stdout().lock();
        if out.write_all(b"READY\n").is_err() || out.flush().is_err() {
            process::exit(1);
        }
    }

    let mut tcp_read = match stream.try_clone() {
        Ok(s) => s,
        Err(e) => {
            // Already sent READY; best-effort exit — parent will see EOF on pipes.
            let _ = e;
            process::exit(1);
        }
    };
    let mut tcp_write = stream;

    let up = thread::spawn(move || {
        let mut stdin = std::io::stdin();
        let _ = copy_all(&mut stdin, &mut tcp_write);
        let _ = tcp_write.shutdown(std::net::Shutdown::Write);
    });
    let down = thread::spawn(move || {
        let mut stdout = std::io::stdout();
        let _ = copy_all(&mut tcp_read, &mut stdout);
        let _ = stdout.flush();
    });

    let _ = up.join();
    let _ = down.join();
}

fn resolve_addr(host: &str, port: &str) -> Result<SocketAddr, String> {
    let port: u16 = port
        .parse()
        .map_err(|_| format!("invalid port: {port}"))?;
    let mut iter = (host, port)
        .to_socket_addrs()
        .map_err(|e| format!("resolve {host}:{port}: {e}"))?;
    iter.next()
        .ok_or_else(|| format!("resolve {host}:{port}: no addresses"))
}

fn write_err(msg: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "ERR {msg}");
    let _ = out.flush();
}

/// Copy until EOF or error (idle timeouts surface as errors on the TCP side).
fn copy_all(reader: &mut impl Read, writer: &mut impl Write) -> std::io::Result<u64> {
    let mut buf = [0u8; 16 * 1024];
    let mut total = 0u64;
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => return Ok(total),
            Ok(n) => n,
            // Windows: timed-out reads often return WouldBlock/TimedOut — treat as idle end.
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                return Ok(total);
            }
            Err(e) => return Err(e),
        };
        writer.write_all(&buf[..n])?;
        writer.flush()?;
        total += n as u64;
    }
}
