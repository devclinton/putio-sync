//! Release pre-check metrics (feature 042, research R12). No release name or hash is
//! ever a label; the `cache` label is a host name from the bounded config list.

use std::time::Duration;

use crate::{
    global, FallbackResult, FlagKind, HoldResult, LookupResult, PrecheckOutcome, PrecheckSource,
    PrecheckStage, Telemetry,
};

impl Telemetry {
    /// Count one check.
    pub fn precheck(&self, source: PrecheckSource, stage: PrecheckStage, outcome: PrecheckOutcome) {
        self.precheck
            .with_label_values(&[source.as_str(), stage.as_str(), outcome.as_str()])
            .inc();
    }

    /// Add the file-list bytes of a rejected release.
    pub fn precheck_avoided(&self, stage: PrecheckStage, bytes: u64) {
        self.precheck_avoided_bytes
            .with_label_values(&[stage.as_str()])
            .inc_by(bytes);
    }

    /// Start the series of the configured caches at 0, so they show before a lookup.
    pub fn precheck_caches(&self, hosts: &[String]) {
        for host in hosts {
            for r in LookupResult::ALL {
                self.precheck_cache_lookups
                    .with_label_values(&[host.as_str(), r.as_str()]);
            }
        }
    }

    /// Count one cache request.
    pub fn precheck_cache_lookup(&self, host: &str, result: LookupResult) {
        self.precheck_cache_lookups
            .with_label_values(&[host, result.as_str()])
            .inc();
    }

    /// Observe the time one magnet lookup took across the caches.
    pub fn precheck_lookup_time(&self, elapsed: Duration) {
        self.precheck_cache_lookup_seconds
            .observe(elapsed.as_secs_f64());
    }

    /// Count a fallback's check on Put.io; `putio_bytes` counts only for rejections.
    pub fn precheck_fallback(&self, result: FallbackResult, putio_bytes: u64) {
        self.precheck_fallback
            .with_label_values(&[result.as_str()])
            .inc();
        if result == FallbackResult::Rejected {
            self.precheck_fallback_putio_bytes.inc_by(putio_bytes);
        }
    }

    /// Count a hold that ended, and observe how long it took.
    pub fn precheck_hold_ended(&self, result: HoldResult, held: Duration) {
        self.precheck_held
            .with_label_values(&[result.as_str()])
            .inc();
        self.precheck_held_seconds
            .with_label_values(&[result.as_str()])
            .observe(held.as_secs_f64());
    }

    /// Set the number of magnets held now.
    pub fn set_precheck_held(&self, count: i64) {
        self.precheck_held_now.set(count);
    }

    /// Count one operator flag.
    pub fn release_flag(&self, kind: FlagKind) {
        self.release_flags.with_label_values(&[kind.as_str()]).inc();
    }
}

/// Count a check on the global instance.
pub fn check(source: PrecheckSource, stage: PrecheckStage, outcome: PrecheckOutcome) {
    global().precheck(source, stage, outcome);
}

/// Add avoided bytes on the global instance.
pub fn avoided(stage: PrecheckStage, bytes: u64) {
    global().precheck_avoided(stage, bytes);
}

/// Start the cache series on the global instance.
pub fn caches(hosts: &[String]) {
    global().precheck_caches(hosts);
}

/// Count a cache request on the global instance.
pub fn cache_lookup(host: &str, result: LookupResult) {
    global().precheck_cache_lookup(host, result);
}

/// Observe a lookup's time on the global instance.
pub fn lookup_time(elapsed: Duration) {
    global().precheck_lookup_time(elapsed);
}

/// Count a fallback's check on Put.io on the global instance.
pub fn fallback(result: FallbackResult, putio_bytes: u64) {
    global().precheck_fallback(result, putio_bytes);
}

/// Count an ended hold on the global instance.
pub fn hold_ended(result: HoldResult, held: Duration) {
    global().precheck_hold_ended(result, held);
}

/// Set the held gauge on the global instance.
pub fn set_held(count: i64) {
    global().set_precheck_held(count);
}

/// Count a flag on the global instance.
pub fn flag(kind: FlagKind) {
    global().release_flag(kind);
}
