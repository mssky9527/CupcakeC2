use serde::{Deserialize, Serialize};

/// Max payload accepted by Stage0 for a single worker job (8 MiB).
pub const MAX_PAYLOAD_BYTES: usize = 8 * 1024 * 1024;
/// Max stdout/stderr returned from worker (2 MiB each).
pub const MAX_OUTPUT_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerRequest {
    pub request_id: String,
    pub module_id: String,
    pub operation: String,
    #[serde(default)]
    pub payload_b64: String,
    #[serde(default = "default_deadline")]
    pub deadline_ms: u64,
}

fn default_deadline() -> u64 {
    30_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerResponse {
    #[serde(default)]
    pub request_id: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub stdout: String,
    #[serde(default)]
    pub stderr: String,
    #[serde(default)]
    pub error_code: String,
}
