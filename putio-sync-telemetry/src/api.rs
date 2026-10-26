//! Put.io API request metrics.

use std::time::Duration;

use crate::{global, ApiOp, ApiOutcome, RetryReason, Telemetry};

impl Telemetry {
    /// Record one HTTP attempt. The per-operation histogram is only fed at `detailed`.
    pub fn api_attempt(&self, op: ApiOp, outcome: ApiOutcome, elapsed: Duration) {
        self.api_requests
            .with_label_values(&[op.as_str(), outcome.as_str()])
            .inc();
        let secs = elapsed.as_secs_f64();
        self.api_duration.observe(secs);
        if self.detailed() {
            self.api_op_duration
                .with_label_values(&[op.as_str()])
                .observe(secs);
        }
    }

    /// Record one retry scheduled by the retry policy.
    pub fn api_retry(&self, reason: RetryReason) {
        self.api_retries.with_label_values(&[reason.as_str()]).inc();
    }
}

/// Record one HTTP attempt on the global instance.
pub fn record_attempt(op: ApiOp, outcome: ApiOutcome, elapsed: Duration) {
    global().api_attempt(op, outcome, elapsed);
}

/// Record one retry on the global instance.
pub fn record_retry(reason: RetryReason) {
    global().api_retry(reason);
}
