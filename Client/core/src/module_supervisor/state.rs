use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerState {
    Stopped,
    Starting,
    Ready,
    Busy,
    Failed,
    Timeout,
}

#[derive(Debug, Clone)]
pub struct WorkerStatus {
    pub state: WorkerState,
    pub last_error: Option<String>,
    pub updated: Instant,
}

impl WorkerStatus {
    pub fn as_str(&self) -> &'static str {
        match self.state {
            WorkerState::Stopped => "stopped",
            WorkerState::Starting => "worker_starting",
            WorkerState::Ready => "worker_ready",
            WorkerState::Busy => "executing",
            WorkerState::Failed => "failed",
            WorkerState::Timeout => "timeout",
        }
    }
}
