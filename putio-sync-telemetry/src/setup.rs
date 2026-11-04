//! Setup mode metric (feature 013).

use crate::{global, Telemetry};

impl Telemetry {
    /// Set whether the daemon is waiting in setup mode.
    pub fn set_setup_required(&self, waiting: bool) {
        self.setup_required.set(i64::from(waiting));
    }
}

/// Set the setup-mode gauge on the global instance.
pub fn set_required(waiting: bool) {
    global().set_setup_required(waiting);
}
