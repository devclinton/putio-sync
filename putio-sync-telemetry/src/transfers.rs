//! Put.io transfer metrics (feature 007).

use crate::{global, Telemetry};

impl Telemetry {
    /// Set the number of stalled Put.io transfers found by the last check.
    pub fn set_transfers_stalled(&self, n: usize) {
        self.transfers_stalled.set(n as i64);
    }
}

/// Set the stalled-transfer count on the global instance.
pub fn set_stalled(n: usize) {
    global().set_transfers_stalled(n);
}
