//! Long-lived desktop RDP worker lifecycle + out-of-process relay handoff.
//!
//! Target:
//!   Stage0 ModuleSupervisor owns worker state, request id, start/stop, deadline, cleanup.
//!   `cupcake-desktop-worker` / `desktop_worker.exe` (Job Object child) owns RDP dial + byte relay.
//!
//! Product default:
//!   `transport::desktop_bridge` (in-process dial+pipe) when `CUPCAKE_DESKTOP_WORKER` is unset.
//!
//! Opt-in:
//!   `CUPCAKE_DESKTOP_WORKER=1` → [`DesktopRelayDecision::UseWorker`]; Stage0 spawns the worker
//!   PE, waits for `READY`/`ERR`, then duplex-pipes Yamux ↔ child stdin/stdout under a Job Object.
//!   Spawn/READY failure falls back to the in-process bridge (logged).

use super::job_object::JobObject;
use super::state::{WorkerState, WorkerStatus};
use log::info;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Env var: when set to `1` / `true` / `yes`, prefer the isolated worker path.
pub const ENV_DESKTOP_WORKER: &str = "CUPCAKE_DESKTOP_WORKER";

/// Optional absolute/relative path to the worker PE (overrides next-to-exe / cwd lookup).
pub const ENV_DESKTOP_WORKER_BIN: &str = "CUPCAKE_DESKTOP_WORKER_BIN";

/// How Stage0 should handle an inbound Yamux DESKTOP (0x0D) stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopRelayDecision {
    /// Existing path: dial + pipe inside the agent process.
    UseBridge,
    /// Hand off dial+relay to long-lived worker under Job Object limits.
    UseWorker,
}

/// Bookkeeping for one logical desktop relay session (target-side dial).
#[derive(Debug, Clone)]
pub struct DesktopSession {
    pub request_id: u64,
    pub target_host: String,
    pub target_port: u16,
    pub status: WorkerStatus,
}

#[derive(Default)]
struct DesktopWorkerInner {
    /// Next request id (monotonic within process).
    next_id: u64,
    /// Active / last session (at most one tracked).
    session: Option<DesktopSession>,
    /// True after an opt-in UseWorker decision was made.
    worker_path_attempted: bool,
}

/// Process-wide desktop worker lifecycle controller (Stage0 side only).
pub struct DesktopWorkerLifecycle {
    inner: Mutex<DesktopWorkerInner>,
    sessions_started: AtomicU64,
    sessions_stopped: AtomicU64,
}

impl DesktopWorkerLifecycle {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(DesktopWorkerInner::default()),
            sessions_started: AtomicU64::new(0),
            sessions_stopped: AtomicU64::new(0),
        }
    }

    /// Whether the operator/env requested the isolated worker path.
    pub fn env_wants_worker() -> bool {
        match std::env::var(ENV_DESKTOP_WORKER) {
            Ok(v) => {
                let t = v.trim();
                t == "1" || t.eq_ignore_ascii_case("true") || t.eq_ignore_ascii_case("yes")
            }
            Err(_) => false,
        }
    }

    /// Decide relay mode for an inbound DESKTOP stream.
    ///
    /// Default (env unset): [`DesktopRelayDecision::UseBridge`].
    /// Opt-in env: [`DesktopRelayDecision::UseWorker`] (caller may fall back to bridge on spawn failure).
    pub fn decide_relay(&self) -> DesktopRelayDecision {
        if !Self::env_wants_worker() {
            return DesktopRelayDecision::UseBridge;
        }
        if let Ok(mut g) = self.inner.lock() {
            g.worker_path_attempted = true;
        }
        info!("[desktop_worker] {ENV_DESKTOP_WORKER} set — using out-of-process worker path");
        DesktopRelayDecision::UseWorker
    }

    /// Record a session start (bookkeeping — spawn is separate via [`run_desktop_worker_relay`]).
    pub fn start_session(&self, host: &str, port: u16) -> Result<u64, String> {
        let mut g = self
            .inner
            .lock()
            .map_err(|_| "desktop_worker lock".to_string())?;
        if let Some(ref s) = g.session {
            if matches!(
                s.status.state,
                WorkerState::Starting | WorkerState::Busy
            ) {
                return Err("desktop session already active".into());
            }
        }
        g.next_id = g.next_id.saturating_add(1);
        let id = g.next_id;
        g.session = Some(DesktopSession {
            request_id: id,
            target_host: host.to_string(),
            target_port: port,
            status: WorkerStatus {
                state: WorkerState::Ready,
                last_error: None,
                updated: Instant::now(),
            },
        });
        self.sessions_started.fetch_add(1, Ordering::Relaxed);
        info!("[desktop_worker] session start id={id} target={host}:{port}");
        Ok(id)
    }

    /// Mark tracked session Busy (relay in progress).
    pub fn mark_busy(&self, request_id: u64) {
        if let Ok(mut g) = self.inner.lock() {
            if let Some(ref mut s) = g.session {
                if s.request_id == request_id {
                    s.status.state = WorkerState::Busy;
                    s.status.updated = Instant::now();
                }
            }
        }
    }

    /// Mark tracked session Failed with an error string.
    pub fn mark_failed(&self, request_id: u64, err: &str) {
        if let Ok(mut g) = self.inner.lock() {
            if let Some(ref mut s) = g.session {
                if s.request_id == request_id {
                    s.status.state = WorkerState::Failed;
                    s.status.last_error = Some(err.to_string());
                    s.status.updated = Instant::now();
                }
            }
        }
    }

    /// Stop / clear tracked session (bookkeeping). Active Job child is killed by Job drop.
    pub fn stop_session(&self, request_id: Option<u64>) -> Result<(), String> {
        let mut g = self
            .inner
            .lock()
            .map_err(|_| "desktop_worker lock".to_string())?;
        match g.session.take() {
            None => Ok(()),
            Some(s) => {
                if let Some(want) = request_id {
                    if s.request_id != want {
                        g.session = Some(s);
                        return Err("desktop session id mismatch".into());
                    }
                }
                info!("[desktop_worker] session stop id={}", s.request_id);
                self.sessions_stopped.fetch_add(1, Ordering::Relaxed);
                Ok(())
            }
        }
    }

    /// Status of the tracked desktop session (or Stopped if none).
    pub fn status(&self) -> WorkerStatus {
        self.inner
            .lock()
            .ok()
            .and_then(|g| g.session.as_ref().map(|s| s.status.clone()))
            .unwrap_or(WorkerStatus {
                state: WorkerState::Stopped,
                last_error: None,
                updated: Instant::now(),
            })
    }

    pub fn active_request_id(&self) -> Option<u64> {
        self.inner
            .lock()
            .ok()
            .and_then(|g| g.session.as_ref().map(|s| s.request_id))
    }

    pub fn worker_path_attempted(&self) -> bool {
        self.inner
            .lock()
            .ok()
            .map(|g| g.worker_path_attempted)
            .unwrap_or(false)
    }

    pub fn sessions_started(&self) -> u64 {
        self.sessions_started.load(Ordering::Relaxed)
    }

    pub fn sessions_stopped(&self) -> u64 {
        self.sessions_stopped.load(Ordering::Relaxed)
    }

    /// Clear bookkeeping when Stage0 disconnects / `stop_all`.
    pub fn stop_all(&self) {
        if let Ok(mut g) = self.inner.lock() {
            g.session = None;
        }
    }
}

impl Default for DesktopWorkerLifecycle {
    fn default() -> Self {
        Self::new()
    }
}

static DESKTOP_LIFE: std::sync::OnceLock<DesktopWorkerLifecycle> = std::sync::OnceLock::new();

/// Process-wide desktop worker lifecycle (Stage0).
pub fn desktop_lifecycle() -> &'static DesktopWorkerLifecycle {
    DESKTOP_LIFE.get_or_init(DesktopWorkerLifecycle::new)
}

/// Convenience: env opt-in → UseWorker; otherwise UseBridge.
pub fn decide_desktop_relay() -> DesktopRelayDecision {
    desktop_lifecycle().decide_relay()
}

/// Locate the desktop worker PE.
///
/// Order: `CUPCAKE_DESKTOP_WORKER_BIN` → next to agent exe → cwd.
/// Names tried: `desktop_worker.exe`, `cupcake-desktop-worker.exe` (and non-`.exe` on non-Windows).
pub fn resolve_worker_binary() -> Option<PathBuf> {
    if let Ok(p) = std::env::var(ENV_DESKTOP_WORKER_BIN) {
        let pb = PathBuf::from(p.trim());
        if pb.is_file() {
            return Some(pb);
        }
    }

    let names: &[&str] = if cfg!(windows) {
        &["desktop_worker.exe", "cupcake-desktop-worker.exe"]
    } else {
        &[
            "desktop_worker",
            "cupcake-desktop-worker",
            "desktop_worker.exe",
            "cupcake-desktop-worker.exe",
        ]
    };

    let mut candidates = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in names {
                candidates.push(dir.join(name));
            }
        }
    }
    for name in names {
        candidates.push(PathBuf::from(name));
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// Skeleton-era alias kept for callers/tests that still name it "placeholder".
pub fn resolve_placeholder_binary() -> Option<PathBuf> {
    resolve_worker_binary()
}

/// Parse the worker's first status line (`READY` or `ERR …`).
pub fn parse_worker_status_line(line: &str) -> Result<(), String> {
    let t = line.trim();
    if t == "READY" {
        Ok(())
    } else if let Some(rest) = t.strip_prefix("ERR") {
        let msg = rest.trim();
        if msg.is_empty() {
            Err("worker ERR".into())
        } else {
            Err(msg.to_string())
        }
    } else if t.is_empty() {
        Err("empty worker status".into())
    } else {
        Err(format!("unexpected worker status: {t}"))
    }
}

const WORKER_READY_TIMEOUT: Duration = Duration::from_secs(20);
const RELAY_IDLE_SECS: u64 = 120;

/// Live worker child after successful dial (`READY`), ready for duplex pipe.
pub struct DesktopWorkerChild {
    child: tokio::process::Child,
    /// Taken by the pipe path; `Option` so fields can leave a `Drop` type safely.
    stdin: Option<tokio::process::ChildStdin>,
    stdout: Option<tokio::process::ChildStdout>,
    /// Kill-on-close Job Object (Windows). `Option` so cleanup can take it once.
    job: Option<JobObject>,
    session_id: u64,
    target: String,
    /// When true, Drop skips kill/session cleanup (already done explicitly).
    finished: bool,
}

impl DesktopWorkerChild {
    pub fn session_id(&self) -> u64 {
        self.session_id
    }

    pub fn target(&self) -> &str {
        &self.target
    }

    fn mark_finished(&mut self) {
        self.finished = true;
    }
}

impl Drop for DesktopWorkerChild {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        // Best-effort teardown if caller abandons without await cleanup.
        let _ = self.child.start_kill();
        if let Some(job) = self.job.take() {
            let _ = job.terminate(1);
        }
        let _ = desktop_lifecycle().stop_session(Some(self.session_id));
    }
}

/// Spawn `desktop_worker relay <host> <port>` under a Job Object and wait for `READY`.
///
/// Does **not** touch the Yamux stream — caller ACKs and pipes, or falls back to bridge.
pub async fn spawn_desktop_worker_ready(
    host: &str,
    port: u16,
) -> Result<DesktopWorkerChild, String> {
    let bin = resolve_worker_binary().ok_or_else(|| {
        format!(
            "desktop_worker binary not found (set {ENV_DESKTOP_WORKER_BIN} or place next to agent)"
        )
    })?;

    // Windows: fail-closed without Job Object so the child is always kill-contained.
    #[cfg(windows)]
    let job = Some(JobObject::create().ok_or_else(|| "job object unavailable".to_string())?);
    #[cfg(not(windows))]
    let job: Option<JobObject> = JobObject::create();

    let mut cmd = tokio::process::Command::new(&bin);
    cmd.arg("relay")
        .arg(host)
        .arg(port.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("spawn desktop_worker: {e}"))?;

    #[cfg(windows)]
    {
        // tokio::process::Child exposes raw_handle() on Windows.
        let h = child
            .raw_handle()
            .ok_or_else(|| "desktop_worker missing process handle".to_string())?
            as usize;
        let j = job.as_ref().expect("job present on windows");
        if j.assign_process(h).is_err() {
            let _ = child.start_kill();
            let _ = child.wait().await;
            return Err("AssignProcessToJobObject failed".into());
        }
    }

    let life = desktop_lifecycle();
    let session_id = match life.start_session(host, port) {
        Ok(id) => id,
        Err(e) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            if let Some(ref j) = job {
                let _ = j.terminate(1);
            }
            return Err(e);
        }
    };
    life.mark_busy(session_id);

    let mut child_stdout = match child.stdout.take() {
        Some(s) => s,
        None => {
            cleanup_failed_spawn(&mut child, &job, session_id, "desktop_worker missing stdout")
                .await;
            return Err("desktop_worker missing stdout".into());
        }
    };
    let child_stdin = match child.stdin.take() {
        Some(s) => s,
        None => {
            cleanup_failed_spawn(&mut child, &job, session_id, "desktop_worker missing stdin")
                .await;
            return Err("desktop_worker missing stdin".into());
        }
    };

    let status_line = match read_status_line(&mut child_stdout, WORKER_READY_TIMEOUT).await {
        Ok(l) => l,
        Err(e) => {
            life.mark_failed(session_id, &e);
            let _ = child.start_kill();
            let _ = child.wait().await;
            if let Some(ref j) = job {
                let _ = j.terminate(1);
            }
            let _ = life.stop_session(Some(session_id));
            return Err(e);
        }
    };

    if let Err(e) = parse_worker_status_line(&status_line) {
        life.mark_failed(session_id, &e);
        let _ = child.start_kill();
        let _ = child.wait().await;
        if let Some(ref j) = job {
            let _ = j.terminate(1);
        }
        let _ = life.stop_session(Some(session_id));
        return Err(e);
    }

    info!(
        "[desktop_worker] READY id={session_id} target={host}:{port} bin={}",
        bin.display()
    );

    Ok(DesktopWorkerChild {
        child,
        stdin: Some(child_stdin),
        stdout: Some(child_stdout),
        job,
        session_id,
        target: format!("{host}:{port}"),
        finished: false,
    })
}

async fn cleanup_failed_spawn(
    child: &mut tokio::process::Child,
    job: &Option<JobObject>,
    session_id: u64,
    err: &str,
) {
    desktop_lifecycle().mark_failed(session_id, err);
    let _ = child.start_kill();
    let _ = child.wait().await;
    if let Some(j) = job {
        let _ = j.terminate(1);
    }
    let _ = desktop_lifecycle().stop_session(Some(session_id));
}

/// ACK Yamux (`0x01`) then duplex-pipe stream ↔ an already-READY worker child.
pub async fn pipe_desktop_worker_relay<S>(
    mut yamux: S,
    mut worker: DesktopWorkerChild,
) -> Result<(), String>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let session_id = worker.session_id;
    let target = worker.target.clone();

    let mut child_stdin = worker
        .stdin
        .take()
        .ok_or_else(|| "desktop_worker stdin already taken".to_string())?;
    let mut child_stdout = worker
        .stdout
        .take()
        .ok_or_else(|| "desktop_worker stdout already taken".to_string())?;

    if let Err(e) = yamux.write_all(&[0x01]).await {
        let msg = format!("yamux ACK write: {e}");
        desktop_lifecycle().mark_failed(session_id, &msg);
        let _ = worker.child.start_kill();
        let _ = worker.child.wait().await;
        if let Some(j) = worker.job.take() {
            let _ = j.terminate(1);
        }
        let _ = desktop_lifecycle().stop_session(Some(session_id));
        worker.mark_finished();
        return Err(msg);
    }

    let idle = Duration::from_secs(RELAY_IDLE_SECS);
    let (mut y_r, mut y_w) = tokio::io::split(yamux);

    let c2t = async {
        let _ = copy_with_idle(&mut y_r, &mut child_stdin, idle).await;
        drop(child_stdin);
    };
    let t2c = async {
        let _ = copy_with_idle(&mut child_stdout, &mut y_w, idle).await;
    };
    tokio::join!(c2t, t2c);

    let _ = worker.child.start_kill();
    let _ = worker.child.wait().await;
    if let Some(j) = worker.job.take() {
        let _ = j.terminate(1);
    }
    let _ = desktop_lifecycle().stop_session(Some(session_id));
    worker.mark_finished();

    info!("[desktop_worker] relay closed id={session_id} target={target}");
    Ok(())
}

/// Spawn worker, wait READY, ACK, and pipe. On spawn/READY failure returns `Err`
/// **without** writing to `yamux` so the caller can fall back to the in-process bridge.
pub async fn run_desktop_worker_relay<S>(
    host: &str,
    port: u16,
    yamux: S,
) -> Result<(), String>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let worker = spawn_desktop_worker_ready(host, port).await?;
    pipe_desktop_worker_relay(yamux, worker).await
}

async fn read_status_line<R: AsyncRead + Unpin>(
    reader: &mut R,
    timeout: Duration,
) -> Result<String, String> {
    let fut = async {
        let mut buf = Vec::with_capacity(128);
        let mut byte = [0u8; 1];
        loop {
            let n = reader
                .read(&mut byte)
                .await
                .map_err(|e| format!("read worker status: {e}"))?;
            if n == 0 {
                return Err("worker closed before READY".into());
            }
            if byte[0] == b'\n' {
                break;
            }
            if byte[0] == b'\r' {
                continue;
            }
            if buf.len() >= 4096 {
                return Err("worker status line too long".into());
            }
            buf.push(byte[0]);
        }
        String::from_utf8(buf).map_err(|_| "worker status not utf-8".to_string())
    };
    match tokio::time::timeout(timeout, fut).await {
        Ok(r) => r,
        Err(_) => Err("worker READY timeout".into()),
    }
}

/// Bidirectional half-copy with per-read idle timeout (matches desktop_bridge).
async fn copy_with_idle<R, W>(
    reader: &mut R,
    writer: &mut W,
    idle: Duration,
) -> std::io::Result<u64>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
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
                    "desktop worker relay idle timeout",
                ));
            }
        };
        writer.write_all(&buf[..n]).await?;
        writer.flush().await?;
        total += n as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex as StdMutex, OnceLock};

    /// Serialize tests that touch process-global env vars.
    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<StdMutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| StdMutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn default_decision_is_bridge() {
        let _g = env_lock();
        // Ensure env does not force worker for this assertion.
        let prev = std::env::var(ENV_DESKTOP_WORKER).ok();
        std::env::remove_var(ENV_DESKTOP_WORKER);
        let life = DesktopWorkerLifecycle::new();
        assert_eq!(life.decide_relay(), DesktopRelayDecision::UseBridge);
        if let Some(v) = prev {
            std::env::set_var(ENV_DESKTOP_WORKER, v);
        }
    }

    #[test]
    fn decide_relay_returns_use_worker_when_env_set() {
        let _g = env_lock();
        let prev = std::env::var(ENV_DESKTOP_WORKER).ok();
        std::env::set_var(ENV_DESKTOP_WORKER, "1");
        let life = DesktopWorkerLifecycle::new();
        assert_eq!(life.decide_relay(), DesktopRelayDecision::UseWorker);
        assert!(life.worker_path_attempted());
        // true / yes also count
        std::env::set_var(ENV_DESKTOP_WORKER, "true");
        assert_eq!(
            DesktopWorkerLifecycle::new().decide_relay(),
            DesktopRelayDecision::UseWorker
        );
        std::env::set_var(ENV_DESKTOP_WORKER, "yes");
        assert_eq!(
            DesktopWorkerLifecycle::new().decide_relay(),
            DesktopRelayDecision::UseWorker
        );
        std::env::set_var(ENV_DESKTOP_WORKER, "0");
        assert_eq!(
            DesktopWorkerLifecycle::new().decide_relay(),
            DesktopRelayDecision::UseBridge
        );
        if let Some(v) = prev {
            std::env::set_var(ENV_DESKTOP_WORKER, v);
        } else {
            std::env::remove_var(ENV_DESKTOP_WORKER);
        }
    }

    #[test]
    fn session_start_stop_bookkeeping() {
        let life = DesktopWorkerLifecycle::new();
        assert_eq!(life.status().state, WorkerState::Stopped);
        let id = life.start_session("127.0.0.1", 3389).unwrap();
        assert!(id >= 1);
        assert_eq!(life.active_request_id(), Some(id));
        assert_eq!(life.status().state, WorkerState::Ready);
        assert_eq!(life.sessions_started(), 1);
        life.mark_busy(id);
        assert_eq!(life.status().state, WorkerState::Busy);
        life.stop_session(Some(id)).unwrap();
        assert_eq!(life.active_request_id(), None);
        assert_eq!(life.status().state, WorkerState::Stopped);
        assert_eq!(life.sessions_stopped(), 1);
    }

    #[test]
    fn stop_all_clears_session() {
        let life = DesktopWorkerLifecycle::new();
        life.start_session("10.0.0.1", 3389).unwrap();
        life.stop_all();
        assert!(life.active_request_id().is_none());
        assert_eq!(life.status().state, WorkerState::Stopped);
    }

    #[test]
    fn busy_session_blocks_second_start() {
        let life = DesktopWorkerLifecycle::new();
        let id = life.start_session("127.0.0.1", 3389).unwrap();
        {
            let mut g = life.inner.lock().unwrap();
            if let Some(ref mut s) = g.session {
                s.status.state = WorkerState::Busy;
            }
        }
        let err = life.start_session("127.0.0.1", 3390).unwrap_err();
        assert!(err.contains("already active"));
        assert_eq!(life.active_request_id(), Some(id));
    }

    #[test]
    fn ready_session_can_be_replaced() {
        let life = DesktopWorkerLifecycle::new();
        let id1 = life.start_session("127.0.0.1", 3389).unwrap();
        let id2 = life.start_session("10.0.0.2", 3389).unwrap();
        assert_ne!(id1, id2);
        assert_eq!(life.active_request_id(), Some(id2));
    }

    #[test]
    fn env_name_is_stable() {
        assert_eq!(ENV_DESKTOP_WORKER, "CUPCAKE_DESKTOP_WORKER");
        assert_eq!(ENV_DESKTOP_WORKER_BIN, "CUPCAKE_DESKTOP_WORKER_BIN");
    }

    #[test]
    fn parse_worker_status_line_ready_and_err() {
        assert!(parse_worker_status_line("READY").is_ok());
        assert!(parse_worker_status_line("READY\r").is_ok());
        assert!(parse_worker_status_line("  READY  ").is_ok());
        let e = parse_worker_status_line("ERR connect failed").unwrap_err();
        assert!(e.contains("connect failed"));
        let e2 = parse_worker_status_line("ERR").unwrap_err();
        assert!(e2.contains("ERR") || e2.contains("worker"));
        assert!(parse_worker_status_line("BOGUS").is_err());
        assert!(parse_worker_status_line("").is_err());
    }

    #[test]
    fn resolve_worker_binary_respects_env_bin() {
        let _g = env_lock();
        let prev = std::env::var(ENV_DESKTOP_WORKER_BIN).ok();
        // Point at this source file so is_file() is true without building the PE.
        let fake = PathBuf::from(file!());
        // file!() may be relative; canonicalize if possible.
        let path = fake
            .canonicalize()
            .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src").join("module_supervisor").join("desktop_worker.rs"));
        if path.is_file() {
            std::env::set_var(ENV_DESKTOP_WORKER_BIN, &path);
            let got = resolve_worker_binary();
            assert_eq!(got.as_ref(), Some(&path));
        }
        if let Some(v) = prev {
            std::env::set_var(ENV_DESKTOP_WORKER_BIN, v);
        } else {
            std::env::remove_var(ENV_DESKTOP_WORKER_BIN);
        }
    }

    #[tokio::test]
    async fn run_worker_relay_errors_when_binary_missing() {
        let _g = env_lock();
        let prev_bin = std::env::var(ENV_DESKTOP_WORKER_BIN).ok();
        // Force a non-existent binary path.
        std::env::set_var(
            ENV_DESKTOP_WORKER_BIN,
            "Z:\\nonexistent\\cupcake-desktop-worker-missing.exe",
        );
        // Even with a bogus ENV path that is not a file, resolve falls through —
        // set to an existing directory path that is NOT a file:
        std::env::set_var(ENV_DESKTOP_WORKER_BIN, env!("CARGO_MANIFEST_DIR"));
        // Directory is not is_file(), so resolve may still find cwd candidates.
        // Temporarily also ensure we don't pick up a real binary by using a unique name:
        std::env::set_var(
            ENV_DESKTOP_WORKER_BIN,
            format!(
                "{}\\___no_such_desktop_worker_{}.exe",
                env!("CARGO_MANIFEST_DIR"),
                std::process::id()
            ),
        );

        let (client, server) = tokio::io::duplex(64);
        drop(client);
        let err = run_desktop_worker_relay("127.0.0.1", 1, server)
            .await
            .unwrap_err();
        assert!(
            err.contains("not found") || err.contains("spawn") || err.contains("job"),
            "unexpected err: {err}"
        );

        if let Some(v) = prev_bin {
            std::env::set_var(ENV_DESKTOP_WORKER_BIN, v);
        } else {
            std::env::remove_var(ENV_DESKTOP_WORKER_BIN);
        }
    }
}
