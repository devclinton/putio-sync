//! Database error metrics.

use crate::{global, DbOp, Telemetry};

impl Telemetry {
    /// Count one database error.
    pub fn db_error(&self, op: DbOp) {
        self.db_errors.with_label_values(&[op.as_str()]).inc();
    }
}

/// Count one database error on the global instance.
pub fn error(op: DbOp) {
    global().db_error(op);
}
