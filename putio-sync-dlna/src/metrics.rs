//! Metrics for the media server (feature 022). The families live in `putio-sync-telemetry`,
//! which owns the registry; these are the calls this crate makes. Labels are closed sets:
//! no name, path or address is ever a label.

use putio_sync_telemetry::dlna;

/// Whether the server is running
pub fn up(up: bool) {
    dlna::record_up(up);
}

/// The kinds of request, as label values
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Ssdp,
    Description,
    Browse,
    Stream,
    Other,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Ssdp => "ssdp",
            Kind::Description => "description",
            Kind::Browse => "browse",
            Kind::Stream => "stream",
            Kind::Other => "other",
        }
    }
}

/// How a request ended, as a label value
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    Refused,
    Error,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Ok => "ok",
            Outcome::Refused => "refused",
            Outcome::Error => "error",
        }
    }
}

/// Count a request
pub fn request(kind: Kind, outcome: Outcome) {
    dlna::record_request(kind.as_str(), outcome.as_str());
}

/// A stream started
pub fn stream_started(root_kind: &str) {
    dlna::stream_started(root_kind);
}

/// A stream ended
pub fn stream_ended(root_kind: &str) {
    dlna::stream_ended(root_kind);
}

/// Bytes sent to a player
pub fn bytes_sent(root_kind: &str, bytes: u64) {
    dlna::record_bytes(root_kind, bytes);
}
