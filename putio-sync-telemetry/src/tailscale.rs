//! Tailscale identity sign-in metrics (feature 036).

use crate::{global, Telemetry};

/// How an identity was found: `serve`'s headers or tailscaled's `whois`
pub const VIAS: [&str; 2] = ["serve", "whois"];
/// Whether it was let in
pub const RESULTS: [&str; 2] = ["allowed", "refused"];

impl Telemetry {
    /// Count a request that claimed a Tailscale identity.
    pub fn tailscale_identity(&self, via: &str, allowed: bool) {
        let result = if allowed { "allowed" } else { "refused" };
        self.tailscale_identity
            .with_label_values(&[via, result])
            .inc();
    }
}

/// Count a Tailscale identity on the global instance.
pub fn record_identity(via: &str, allowed: bool) {
    global().tailscale_identity(via, allowed);
}

#[cfg(test)]
mod tests {
    use crate::Telemetry;

    #[test]
    fn identity_counter_is_exposed() {
        let t = Telemetry::new();
        t.tailscale_identity("serve", true);
        t.tailscale_identity("whois", false);
        let text = t.encode();
        assert!(text
            .contains("putio_sync_tailscale_identity_total{result=\"allowed\",via=\"serve\"} 1"));
        assert!(text
            .contains("putio_sync_tailscale_identity_total{result=\"refused\",via=\"whois\"} 1"));
        assert!(text
            .contains("putio_sync_tailscale_identity_total{result=\"refused\",via=\"serve\"} 0"));
    }
}
