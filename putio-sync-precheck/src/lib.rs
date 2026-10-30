//! Release pre-check (feature 042): reads a release's file list before it reaches Put.io
//! and rejects it when no file is a wanted type of at least the minimum size.

pub mod fetch;
pub mod lookup;
pub mod magnet;
pub mod metainfo;
pub mod policy;
pub mod putio;
pub mod report;
pub mod rule;

pub use lookup::{Lookup, Missed};
pub use metainfo::{Metainfo, ReleaseFile};
pub use putio::OnPutio;
pub use putio_sync_telemetry::{PrecheckSource, PrecheckStage};
pub use report::Traits;
pub use rule::{Policy, Reason, Stage, Verdict};

use std::time::Duration;

use putio_sync_client::FileWalk;
use putio_sync_core::config::PrecheckConfig;
use putio_sync_core::rules::RuleSet;
use putio_sync_telemetry::precheck as metrics;
use putio_sync_telemetry::PrecheckOutcome;
use tracing::{info, warn};

use crate::policy::Key;

/// What a check decided
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Go on to Put.io. `None` when the check was off and the bytes didn't parse.
    Passed(Option<Metainfo>),
    /// Don't send it to Put.io. The file list is `None` for [`Reason::Invalid`].
    Rejected(Option<Metainfo>, Reason),
    /// No file list could be had (a magnet missed in every cache)
    Unknown { hash: String, name: Option<String> },
}

/// The file list and the rule's verdict on it
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    pub meta: Option<Metainfo>,
    pub verdict: Verdict,
}

/// Reads .torrent files and applies `[precheck]` with the 009 folder rules
pub struct Prechecker {
    config: PrecheckConfig,
    rules: RuleSet,
    lookup_http: reqwest::Client,
}

impl Prechecker {
    pub fn new(config: PrecheckConfig, rules: RuleSet) -> Self {
        metrics::caches(&cache_hosts(&config.caches));
        Self {
            config,
            rules,
            lookup_http: lookup::client(),
        }
    }

    pub fn config(&self) -> &PrecheckConfig {
        &self.config
    }

    /// The policy for releases landing in `putio_folder`
    pub fn policy(&self, putio_folder: &str, key: Key<'_>) -> Policy {
        Policy::for_folder(&self.config, &self.rules, putio_folder, key)
    }

    /// Parse and judge, with no logging or counting
    pub fn check_bytes(bytes: &[u8], policy: &Policy) -> Checked {
        match metainfo::parse(bytes) {
            Some(meta) => {
                let verdict = rule::evaluate(policy, &meta.files);
                Checked {
                    meta: Some(meta),
                    verdict,
                }
            }
            None => Checked {
                meta: None,
                verdict: Verdict::Reject(Reason::Invalid),
            },
        }
    }

    /// Check a .torrent before Put.io: log it, count it, and under `dry_run` let a
    /// rejection through. A policy that is off passes without a check.
    pub fn check(&self, bytes: &[u8], policy: &Policy, source: PrecheckSource) -> Outcome {
        if !policy.enabled {
            return Outcome::Passed(metainfo::parse(bytes));
        }
        let checked = Self::check_bytes(bytes, policy);
        self.decide(checked, source, Stage::Before)
    }

    /// Check a file list found in a cache before Put.io, as [`Prechecker::check`] does
    pub fn check_meta(&self, meta: Metainfo, policy: &Policy, source: PrecheckSource) -> Outcome {
        let verdict = rule::evaluate(policy, &meta.files);
        let checked = Checked {
            meta: Some(meta),
            verdict,
        };
        self.decide(checked, source, Stage::Before)
    }

    /// Judge a fallback's files once Put.io has them, with no logging or counting; pass
    /// the result to [`Prechecker::decide`] with [`Stage::Putio`]. `tree` is the walk of
    /// the transfer's `file_id`. `None` when the walk is incomplete or hit an error: the
    /// next pass tries again.
    pub fn check_on_putio(
        hash: &str,
        name: &str,
        tree: &FileWalk,
        policy: &Policy,
    ) -> Option<Checked> {
        if !tree.complete || !tree.errors.is_empty() {
            return None;
        }
        let files = tree
            .files
            .iter()
            .filter(|f| !f.is_folder)
            .map(|f| ReleaseFile {
                path: f.path.trim_start_matches('/').to_string(),
                size: f.size.max(0) as u64,
            })
            .collect();
        let meta = Metainfo {
            hash: hash.to_ascii_lowercase(),
            name: name.to_string(),
            files,
            private: false,
            tracker_hosts: Vec::new(),
        };
        let verdict = rule::evaluate(policy, &meta.files);
        Some(Checked {
            meta: Some(meta),
            verdict,
        })
    }

    /// Look a magnet up in the caches within `limit`, capped at `lookup_timeout_secs`.
    /// With `magnet_lookup` off no request is made. Under 1 s gives a timeout unasked.
    pub async fn lookup(&self, hash: &str, limit: Duration) -> Lookup {
        if !self.config.magnet_lookup {
            return Lookup::Missed(Missed::Off);
        }
        let limit = limit.min(Duration::from_secs(self.config.lookup_timeout_secs));
        if limit < Duration::from_secs(1) {
            return Lookup::Missed(Missed::Failed(putio_sync_telemetry::LookupResult::Timeout));
        }
        lookup::lookup(&self.lookup_http, hash, &self.config.caches, limit).await
    }

    /// Log and count a magnet that goes to Put.io unchecked
    pub fn unknown(&self, hash: &str, name: &str, source: PrecheckSource, missed: Missed) {
        info!(
            hash,
            name,
            source = source.as_str(),
            stage = Stage::Before.as_str(),
            outcome = "unknown",
            lookup = missed.as_str(),
            "Release not checked before Put.io"
        );
        metrics::check(source, PrecheckStage::Before, PrecheckOutcome::Unknown);
    }

    /// Log, count and apply `dry_run` to a verdict
    pub fn decide(&self, checked: Checked, source: PrecheckSource, stage: Stage) -> Outcome {
        let Checked { meta, verdict } = checked;
        let hash = meta.as_ref().map(|m| m.hash.as_str()).unwrap_or("");
        let name = meta.as_ref().map(|m| m.name.as_str()).unwrap_or("");
        let metric_stage = match stage {
            Stage::Before => PrecheckStage::Before,
            Stage::Putio => PrecheckStage::Putio,
        };
        match verdict {
            Verdict::Pass { .. } => {
                info!(
                    hash,
                    name,
                    source = source.as_str(),
                    stage = stage.as_str(),
                    outcome = "passed",
                    "Release check passed"
                );
                metrics::check(source, metric_stage, PrecheckOutcome::Passed);
                Outcome::Passed(meta)
            }
            Verdict::Reject(reason) => {
                let dry_run = self.config.dry_run;
                warn!(
                    hash,
                    name,
                    source = source.as_str(),
                    stage = stage.as_str(),
                    outcome = "rejected",
                    reason = reason.key(),
                    dry_run,
                    "{}",
                    reason.message(stage)
                );
                metrics::check(source, metric_stage, PrecheckOutcome::Rejected);
                if dry_run {
                    return Outcome::Passed(meta);
                }
                if let Some(m) = &meta {
                    metrics::avoided(metric_stage, m.total_size());
                }
                Outcome::Rejected(meta, reason)
            }
        }
    }
}

/// The host names of the cache URLs, for the bounded `cache` metric label
pub fn cache_hosts(caches: &[String]) -> Vec<String> {
    caches
        .iter()
        .filter_map(|c| {
            let url = c.replace("{HASH}", "0").replace("{hash}", "0");
            reqwest::Url::parse(&url)
                .ok()?
                .host_str()
                .map(str::to_string)
        })
        .collect()
}
