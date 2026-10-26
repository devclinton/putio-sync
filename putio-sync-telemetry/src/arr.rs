//! Radarr/Sonarr download-client metrics (feature 011).

use crate::{global, ArrItemState, ArrMethod, ArrOutcome, BlocklistOutcome, Telemetry};

impl Telemetry {
    /// Count one Transmission RPC request.
    pub fn arr_rpc_request(&self, method: ArrMethod, outcome: ArrOutcome) {
        self.arr_rpc_requests
            .with_label_values(&[method.as_str(), outcome.as_str()])
            .inc();
    }

    /// Set the number of items in `state`.
    pub fn set_arr_items(&self, state: ArrItemState, n: i64) {
        self.arr_items.with_label_values(&[state.as_str()]).set(n);
    }

    /// Count one blocklist call.
    pub fn arr_blocklist(&self, outcome: BlocklistOutcome) {
        self.arr_blocklist
            .with_label_values(&[outcome.as_str()])
            .inc();
    }
}

/// Count an RPC request on the global instance.
pub fn rpc_request(method: ArrMethod, outcome: ArrOutcome) {
    global().arr_rpc_request(method, outcome);
}

/// Set an item gauge on the global instance.
pub fn set_items(state: ArrItemState, n: i64) {
    global().set_arr_items(state, n);
}

/// Count a blocklist call on the global instance.
pub fn blocklist(outcome: BlocklistOutcome) {
    global().arr_blocklist(outcome);
}
