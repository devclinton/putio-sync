//! # putio-sync-hooks
//!
//! Event hooks (feature 027): for each event on the bus
//! ([`putio_sync_core::events`]), run the configured commands and webhooks that want it.
//! Each hook has its own queue and runs one event at a time, so a slow hook never holds
//! up another or the sync itself. Runs are logged, counted in the metrics and kept (the
//! last 20 per hook) for the API and the doctor.

use chrono::{DateTime, Utc};
use serde::Serialize;

pub mod command;
mod service;
pub mod webhook;

pub use service::{HookStatus, HooksService, TestError};

/// Longest stdout, stderr or response body kept for a run, in bytes.
pub const MAX_OUTPUT: usize = 4096;

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RunResult {
    /// Exit code 0, or a 2xx response
    Success,
    /// Anything else
    Failure,
    /// The hook's timeout passed
    Timeout,
}

impl RunResult {
    /// The metric label value.
    pub fn as_str(&self) -> &'static str {
        match self {
            RunResult::Success => "success",
            RunResult::Failure => "failure",
            RunResult::Timeout => "timeout",
        }
    }
}

/// One finished run of one hook for one event.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HookRun {
    /// The event's type
    pub event_type: String,
    /// The event's id
    pub event_id: String,
    /// When the run started
    pub started_at: DateTime<Utc>,
    /// Retries included
    pub duration_ms: u64,
    /// How it ended
    pub result: RunResult,
    /// Webhook attempts made; 1 for commands
    pub attempts: u32,
    /// A command's exit code
    pub exit_code: Option<i32>,
    /// A webhook's last response status
    pub http_status: Option<u16>,
    /// What went wrong, without secrets
    pub error: Option<String>,
    /// A command's stdout, or a webhook's response body, cut to [`MAX_OUTPUT`]
    pub stdout: String,
    /// A command's stderr, cut to [`MAX_OUTPUT`]
    pub stderr: String,
}

/// Bytes kept up to [`MAX_OUTPUT`], with a count of the rest.
#[derive(Debug, Default)]
pub(crate) struct Capped {
    kept: Vec<u8>,
    cut: usize,
}

impl Capped {
    pub(crate) fn push(&mut self, bytes: &[u8]) {
        let room = MAX_OUTPUT.saturating_sub(self.kept.len());
        let take = room.min(bytes.len());
        self.kept.extend_from_slice(&bytes[..take]);
        self.cut += bytes.len() - take;
    }

    pub(crate) fn text(&self) -> String {
        let mut s = String::from_utf8_lossy(&self.kept).into_owned();
        if self.cut > 0 {
            s.push_str(&format!("… ({} bytes cut)", self.cut));
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capped_keeps_the_first_bytes_and_counts_the_rest() {
        let mut c = Capped::default();
        c.push(&[b'a'; 3000]);
        c.push(&[b'b'; 3000]);
        let text = c.text();
        assert!(text.starts_with("aaa"));
        assert!(text.ends_with("… (1904 bytes cut)"), "{}", &text[4000..]);
        assert_eq!(c.kept.iter().filter(|b| **b == b'b').count(), 1096);
    }
}
