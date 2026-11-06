//! Radarr/Sonarr download-client metrics (feature 011).

use crate::{
    global, ArrItemState, ArrMappingResult, ArrMethod, ArrOutcome, ArrSetupKind, ArrSetupResult,
    BlocklistOutcome, Telemetry,
};

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

    /// Count one auto-setup (feature 018).
    pub fn arr_setup(&self, kind: ArrSetupKind, result: ArrSetupResult) {
        self.arr_setup
            .with_label_values(&[kind.as_str(), result.as_str()])
            .inc();
    }

    /// Count one Remote Path Mapping change (feature 049).
    pub fn arr_path_mapping(&self, result: ArrMappingResult) {
        self.arr_path_mapping
            .with_label_values(&[result.as_str()])
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

/// Count an auto-setup on the global instance.
pub fn setup(kind: ArrSetupKind, result: ArrSetupResult) {
    global().arr_setup(kind, result);
}

/// Count a Remote Path Mapping change on the global instance.
pub fn path_mapping(result: ArrMappingResult) {
    global().arr_path_mapping(result);
}
