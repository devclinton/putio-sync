//! DLNA media server metrics (feature 022).

use crate::{global, Telemetry};

/// Kinds of request the media server answers
pub const KINDS: [&str; 5] = ["ssdp", "description", "browse", "stream", "other"];
/// How a request ended
pub const RESULTS: [&str; 3] = ["ok", "refused", "error"];
/// Where streamed bytes come from
pub const ROOT_KINDS: [&str; 2] = ["local", "putio"];

impl Telemetry {
    /// Set whether the media server is running.
    pub fn dlna_up(&self, up: bool) {
        self.dlna_up.set(i64::from(up));
    }

    /// Count a request the media server answered.
    pub fn dlna_request(&self, kind: &str, result: &str) {
        self.dlna_requests.with_label_values(&[kind, result]).inc();
    }

    /// A stream started (`root_kind` is `local` or `putio`).
    pub fn dlna_stream_started(&self, root_kind: &str) {
        self.dlna_streams_active
            .with_label_values(&[root_kind])
            .inc();
    }

    /// A stream ended.
    pub fn dlna_stream_ended(&self, root_kind: &str) {
        self.dlna_streams_active
            .with_label_values(&[root_kind])
            .dec();
    }

    /// Count bytes sent to players.
    pub fn dlna_bytes(&self, root_kind: &str, bytes: u64) {
        self.dlna_bytes_sent
            .with_label_values(&[root_kind])
            .inc_by(bytes);
    }
}

/// Set whether the media server is running, on the global instance.
pub fn record_up(up: bool) {
    global().dlna_up(up);
}

/// Count a request on the global instance.
pub fn record_request(kind: &str, result: &str) {
    global().dlna_request(kind, result);
}

/// A stream started, on the global instance.
pub fn stream_started(root_kind: &str) {
    global().dlna_stream_started(root_kind);
}

/// A stream ended, on the global instance.
pub fn stream_ended(root_kind: &str) {
    global().dlna_stream_ended(root_kind);
}

/// Count bytes sent, on the global instance.
pub fn record_bytes(root_kind: &str, bytes: u64) {
    global().dlna_bytes(root_kind, bytes);
}

#[cfg(test)]
mod tests {
    use crate::Telemetry;

    #[test]
    fn families_are_exposed() {
        let t = Telemetry::new();
        t.dlna_up(true);
        t.dlna_request("browse", "ok");
        t.dlna_stream_started("local");
        t.dlna_bytes("local", 10);
        let text = t.encode();
        assert!(text.contains("putio_sync_dlna_up 1"));
        assert!(text.contains("putio_sync_dlna_requests_total{kind=\"browse\",result=\"ok\"} 1"));
        assert!(text.contains("putio_sync_dlna_streams_active{root_kind=\"local\"} 1"));
        assert!(text.contains("putio_sync_dlna_bytes_sent_total{root_kind=\"local\"} 10"));
        assert!(text.contains("putio_sync_dlna_streams_active{root_kind=\"putio\"} 0"));
        t.dlna_stream_ended("local");
        assert!(t
            .encode()
            .contains("putio_sync_dlna_streams_active{root_kind=\"local\"} 0"));
    }
}
