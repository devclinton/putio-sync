//! The download client's items: adding them to Put.io, following them to disk, removing
//! them, and asking the *arr to blocklist the ones that fail

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use putio_sync_client::{error_chain, PutioApiClient};
use putio_sync_core::config::{normalize_arr_category, ArrSettings, PrecheckConfig};
use putio_sync_core::rules::RuleSet;
use putio_sync_db::{
    ArrItem, ArrState, BlocklistState, CheckState, Database, Held, HeldSettle, NewArrItem,
    NewCheck, NewHeld, NewReport, PendingCheck, PutioProgress, ReportKind,
};
use putio_sync_download::{DownloadManager, TreeDecision};
use putio_sync_precheck::fetch::{self, Fetched};
use putio_sync_precheck::policy::Key;
use putio_sync_precheck::putio::{self, OnPutio};
use putio_sync_precheck::{
    Lookup, Metainfo, Outcome, Policy, PrecheckSource, Prechecker, Reason, Stage, Traits,
};
use putio_sync_telemetry::arr as metrics;
use putio_sync_telemetry::labels::{ArrItemState, BlocklistOutcome, HoldResult};
use putio_sync_telemetry::precheck as precheck_metrics;
use serde_json::Value;
use thiserror::Error;
use tokio::sync::Mutex;
use tokio::time::Instant;
use tracing::{debug, info, warn};

use crate::arr_api::ArrApi;
use crate::contact::Contacts;
use crate::infohash;
use crate::view::{self, LocalProgress};

/// How often a poll may refresh from Put.io (research R7)
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(20);

/// How long an add may take before the *arr's 15 s RPC timeout (042 research R6)
const ADD_BUDGET: Duration = Duration::from_secs(14);

/// Blocklist calls tried before giving up
const MAX_BLOCKLIST_ATTEMPTS: i64 = 5;

/// `putio_status` of a magnet held before Put.io (043)
pub const HELD: &str = "HELD";

/// Held magnets asked about in one pass, one at a time
const HOLD_BATCH: i64 = 20;

/// Seconds since the Unix epoch, the hold's clock
pub fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// What `torrent-add` was given
#[derive(Debug, Clone)]
pub enum AddSource {
    /// A magnet link
    Magnet(String),
    /// An http(s) URL of a .torrent file (or a magnet link sent as a URL)
    Url(String),
    /// The bytes of a .torrent file
    Metainfo(Vec<u8>),
}

/// Result of an add
#[derive(Debug, Clone)]
pub enum AddOutcome {
    /// A new item
    Added(ArrItem),
    /// The hash was already tracked; Put.io was not called
    Duplicate(ArrItem),
}

/// An id from `torrent-get` / `torrent-remove`
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemId {
    /// Info hash, any case
    Hash(String),
    /// The Transmission id
    Id(i64),
}

/// Errors whose text goes back to the *arr as the RPC `result`
#[derive(Debug, Error)]
pub enum ArrError {
    /// No valid btih in the magnet
    #[error("invalid magnet link")]
    InvalidMagnet,
    /// Not a .torrent, or it could not be fetched
    #[error("invalid torrent file")]
    InvalidTorrent,
    /// `download-dir` outside the target directory
    #[error("download-dir must be inside {0}")]
    OutsideTarget(String),
    /// Put.io refused to create the folder or transfer
    #[error("Put.io refused the transfer: {0}")]
    Refused(String),
    /// Database failure
    #[error("internal error: {0}")]
    Internal(String),
}

impl From<putio_sync_db::DbError> for ArrError {
    fn from(e: putio_sync_db::DbError) -> Self {
        ArrError::Internal(e.to_string())
    }
}

/// The error an item flagged fake by the operator reports to the *arr
const FLAGGED_FAKE: &str = "flagged fake by the operator";

/// A release being added, for [`ArrService::reject`]
struct Release<'a> {
    hash: &'a str,
    name: Option<String>,
    category: &'a str,
    source: PrecheckSource,
    /// Magnets: the lookup result and the cache that answered
    lookup: Option<&'static str>,
    cache: Option<&'a str>,
}

/// What the check decided for one add, and how
struct Checked {
    outcome: Outcome,
    policy: Policy,
    /// Magnets: the lookup result and the cache that answered
    lookup: Option<&'static str>,
    cache: Option<String>,
}

/// The torrent bytes or magnet to hand to Put.io
enum Payload {
    Magnet(String),
    Torrent(Vec<u8>),
}

/// The *arr download client
pub struct ArrService {
    client: Arc<PutioApiClient>,
    db: Arc<Database>,
    downloads: Arc<DownloadManager>,
    settings: ArrSettings,
    target: PathBuf,
    http: reqwest::Client,
    /// For .torrent URLs: stops at a redirect to `magnet:`
    fetch_http: reqwest::Client,
    /// The release pre-check (042); `None` leaves it off
    precheck: Option<Prechecker>,
    contacts: Contacts,
    /// Serialises adds so two of the same hash can't race
    add_lock: Mutex<()>,
    /// Last refresh; held during one, which makes refreshes single-flight
    last_refresh: Mutex<Option<Instant>>,
    /// Held while blocklist calls run, so a slow *arr never queues a second pass
    blocklist_lock: Mutex<()>,
}

impl ArrService {
    /// `target` is the sync's local target directory
    pub fn new(
        client: Arc<PutioApiClient>,
        db: Arc<Database>,
        downloads: Arc<DownloadManager>,
        settings: ArrSettings,
        target: PathBuf,
    ) -> Self {
        Self {
            client,
            db,
            downloads,
            settings,
            target,
            http: reqwest::Client::new(),
            fetch_http: fetch::client(),
            precheck: None,
            contacts: Contacts::default(),
            add_lock: Mutex::new(()),
            last_refresh: Mutex::new(None),
            blocklist_lock: Mutex::new(()),
        }
    }

    /// Check releases before they reach Put.io (042), with `[precheck]` and the 009 rules
    pub fn with_precheck(mut self, config: PrecheckConfig, rules: RuleSet) -> Self {
        self.precheck = Some(Prechecker::new(config, rules));
        self
    }

    /// The resolved `[arr]` settings
    pub fn settings(&self) -> &ArrSettings {
        &self.settings
    }

    /// The local target directory
    pub fn target(&self) -> &Path {
        &self.target
    }

    /// Last contact per *arr app
    pub fn contacts(&self) -> &Contacts {
        &self.contacts
    }

    /// The reqwest client for *arr API calls
    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// The download folder the *arr apps see in `session-get`
    pub fn download_dir(&self) -> String {
        view::download_dir(&self.target, "")
    }

    /// The magnets held before Put.io (043), the oldest first, each with its item's name
    pub async fn held(&self) -> Result<Vec<(String, Held)>, ArrError> {
        let names: HashMap<String, String> = self
            .db
            .arr()
            .list()
            .await?
            .into_iter()
            .filter(is_held)
            .map(|i| (i.hash, i.name))
            .collect();
        Ok(self
            .db
            .held()
            .list()
            .await?
            .into_iter()
            .filter_map(|h| Some((names.get(&h.hash)?.clone(), h)))
            .collect())
    }

    /// Number of items in each state
    pub async fn counts(&self) -> Result<Vec<(ArrState, i64)>, ArrError> {
        Ok(self.db.arr().counts_by_state().await?)
    }

    /// The category a `download-dir` names: the path below the target directory
    fn category_for(&self, download_dir: Option<&str>) -> Result<String, ArrError> {
        let Some(dir) = download_dir.map(str::trim).filter(|d| !d.is_empty()) else {
            return Ok(String::new());
        };
        let target = self.download_dir();
        let outside = || ArrError::OutsideTarget(target.clone());
        let dir = dir.trim_end_matches('/');
        let rest = if dir == target {
            ""
        } else {
            dir.strip_prefix(&format!("{}/", target))
                .ok_or_else(outside)?
        };
        normalize_arr_category(rest).ok_or_else(outside)
    }

    /// Add a release. Duplicates (by info hash) don't reach Put.io.
    pub async fn add(
        &self,
        source: AddSource,
        download_dir: Option<&str>,
    ) -> Result<AddOutcome, ArrError> {
        let category = self.category_for(download_dir)?;
        let deadline = Instant::now() + ADD_BUDGET;
        let ((hash, name, payload), source) = match source {
            AddSource::Magnet(magnet) => (magnet_payload(magnet)?, PrecheckSource::Magnet),
            AddSource::Url(url) if url.starts_with("magnet:") => {
                (magnet_payload(url)?, PrecheckSource::Magnet)
            }
            AddSource::Url(url) => match fetch::fetch(&self.fetch_http, &url, deadline).await {
                Ok(Fetched::Torrent(bytes)) => (torrent_payload(bytes)?, PrecheckSource::Url),
                Ok(Fetched::Magnet(magnet)) => (magnet_payload(magnet)?, PrecheckSource::Magnet),
                Err(_) => return Err(ArrError::InvalidTorrent),
            },
            AddSource::Metainfo(bytes) => (torrent_payload(bytes)?, PrecheckSource::Torrent),
        };

        let _guard = self.add_lock.lock().await;
        let repo = self.db.arr();
        if let Some(existing) = repo.get_by_hash(&hash).await? {
            debug!(hash = %hash, "arr item already tracked");
            return Ok(AddOutcome::Duplicate(existing));
        }

        let checked = self
            .precheck(
                &payload,
                &hash,
                name.as_deref(),
                &category,
                source,
                deadline,
            )
            .await;
        let checked = match checked {
            Some(Checked {
                outcome: Outcome::Rejected(meta, reason),
                policy,
                lookup,
                cache,
            }) => {
                let release = Release {
                    hash: &hash,
                    name,
                    category: &category,
                    source,
                    lookup,
                    cache: cache.as_deref(),
                };
                return self.reject(release, meta, reason, &policy).await;
            }
            other => other,
        };
        if let (Some(checked), Payload::Magnet(magnet)) = (&checked, &payload) {
            if matches!(checked.outcome, Outcome::Unknown { .. }) && self.holding() {
                let release = Release {
                    hash: &hash,
                    name,
                    category: &category,
                    source,
                    lookup: checked.lookup,
                    cache: None,
                };
                return self.hold(release, magnet).await;
            }
        }

        let transfer = self.send_to_putio(&hash, &category, payload).await?;

        let name = name
            .or_else(|| Some(transfer.name.clone()).filter(|n| !n.trim().is_empty()))
            .unwrap_or_else(|| hash.clone());
        repo.insert(&NewArrItem {
            hash: &hash,
            name: &name,
            category: &category,
            transfer_id: Some(transfer.id),
            putio_status: &transfer.status,
            total_size: transfer.size.unwrap_or(0),
        })
        .await?;
        info!(hash = %hash, name = %name, transfer_id = transfer.id, "arr item added");
        if let Some(checked) = checked {
            // A magnet missed in the caches waits for its check on Put.io
            let (state, total) = match &checked.outcome {
                Outcome::Passed(meta) => (
                    Some(CheckState::Passed),
                    meta.as_ref().map_or(0, Metainfo::total_size),
                ),
                _ => (None, 0),
            };
            self.record_check(&NewCheck {
                hash: Some(&hash),
                name: &name,
                source: source.as_str(),
                category: Some(&category),
                stage: Stage::Before.as_str(),
                state,
                lookup: checked.lookup,
                lookup_cache: checked.cache.as_deref(),
                transfer_id: Some(transfer.id),
                total_size: total as i64,
                ..NewCheck::default()
            })
            .await;
        }
        self.update_gauge().await;

        let item = repo
            .get_by_hash(&hash)
            .await?
            .ok_or_else(|| ArrError::Internal("added item missing".to_string()))?;
        Ok(AddOutcome::Added(item))
    }

    /// Create the Put.io transfer in the category's folder
    async fn send_to_putio(
        &self,
        hash: &str,
        category: &str,
        payload: Payload,
    ) -> Result<putio_sync_client::Transfer, ArrError> {
        let parent = self.category_folder(category).await?;
        let added = match payload {
            Payload::Magnet(magnet) => {
                self.client
                    .add_transfer_url(&magnet, Some(parent), None)
                    .await
            }
            Payload::Torrent(bytes) => {
                self.client
                    .add_transfer_torrent(&format!("{}.torrent", hash), bytes, Some(parent))
                    .await
            }
        };
        added.map_err(|e| ArrError::Refused(error_chain(&e)))
    }

    /// Whether a magnet missed in the caches is held and asked about again (043)
    fn holding(&self) -> bool {
        self.precheck.as_ref().is_some_and(|p| p.config().holding())
    }

    /// Hold a magnet no cache had: listed as queued with no Put.io transfer until a later
    /// ask finds its file list or the hold runs out (043 US1)
    async fn hold(&self, release: Release<'_>, magnet: &str) -> Result<AddOutcome, ArrError> {
        let Release {
            hash,
            name,
            category,
            source,
            lookup,
            ..
        } = release;
        let config = self
            .precheck
            .as_ref()
            .map(Prechecker::config)
            .ok_or_else(|| ArrError::Internal("hold without precheck".to_string()))?;
        let name = name
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| hash.to_string());
        let repo = self.db.arr();
        repo.insert(&NewArrItem {
            hash,
            name: &name,
            category,
            transfer_id: None,
            putio_status: HELD,
            total_size: 0,
        })
        .await?;
        let check_id = self
            .record_check(&NewCheck {
                hash: Some(hash),
                name: &name,
                source: source.as_str(),
                category: Some(category),
                stage: Stage::Before.as_str(),
                lookup,
                ..NewCheck::default()
            })
            .await;
        let now = unix_now();
        let until = now + config.hold_magnet_secs as i64;
        let held = self.db.held();
        held.insert(&NewHeld {
            hash,
            magnet,
            source: source.as_str(),
            category: Some(category),
            watch_folder: None,
            check_id,
            last_lookup: lookup,
            held_at: now,
            next_at: (now + config.hold_retry_secs as i64).min(until),
            until_at: until,
        })
        .await?;
        info!(
            hash,
            name = %name,
            lookup = lookup.unwrap_or(""),
            hold_secs = config.hold_magnet_secs,
            "Magnet held until a cache has its file list"
        );
        self.update_held_gauge().await;
        self.update_gauge().await;

        let item = repo
            .get_by_hash(hash)
            .await?
            .ok_or_else(|| ArrError::Internal("held item missing".to_string()))?;
        Ok(AddOutcome::Added(item))
    }

    /// Ask the caches again about each held magnet that is due at `now` (Unix seconds),
    /// the longest-waiting first, one at a time. Each ends the hold or schedules the
    /// next ask.
    pub async fn release_held(&self, now: i64) {
        let due = match self.db.held().due(now, HOLD_BATCH).await {
            Ok(due) => due,
            Err(e) => {
                warn!(error = %e, "Failed to list held magnets");
                return;
            }
        };
        if due.is_empty() {
            return;
        }
        for held in &due {
            if let Err(e) = self.release_one(held, now).await {
                warn!(hash = %held.hash, error = %e, "Failed to release held magnet");
            }
        }
        self.update_held_gauge().await;
        self.update_gauge().await;
    }

    /// One held magnet's turn
    async fn release_one(&self, held: &Held, now: i64) -> Result<(), ArrError> {
        let Some(item) = self.held_item(&held.hash).await? else {
            self.db.held().delete(&held.hash).await?;
            return Ok(());
        };
        let source = PrecheckSource::ALL
            .iter()
            .copied()
            .find(|s| s.as_str() == held.source)
            .unwrap_or(PrecheckSource::Magnet);
        let category = held.category.clone().unwrap_or_default();
        let checker = self.precheck.as_ref().and_then(|p| {
            let policy = p.policy(&category, Key::Category(&category));
            policy.enabled.then_some((p, policy))
        });
        // The check is off for the category now: send it on as it is (FR-013)
        let Some((precheck, policy)) = checker else {
            return self.end_hold(held, &item, HoldEnd::Unchecked, now).await;
        };
        let config = precheck.config();
        if !config.holding() {
            return self.end_hold(held, &item, HoldEnd::RanOut, now).await;
        }

        let limit = Duration::from_secs(config.lookup_timeout_secs);
        match precheck.lookup(&held.hash, limit).await {
            Lookup::Found { meta, cache } => match precheck.check_meta(meta, &policy, source) {
                Outcome::Passed(meta) => {
                    let total = meta.as_ref().map_or(0, Metainfo::total_size) as i64;
                    let end = HoldEnd::Found { cache, total };
                    self.end_hold(held, &item, end, now).await
                }
                Outcome::Rejected(meta, reason) => {
                    self.reject_held(held, &item, source, meta, reason, &policy, cache, now)
                        .await
                }
                Outcome::Unknown { .. } => Ok(()),
            },
            Lookup::Missed(_) if now >= held.until_at => {
                self.end_hold(held, &item, HoldEnd::RanOut, now).await
            }
            Lookup::Missed(missed) => {
                let next = (now + config.hold_retry_secs as i64).min(held.until_at);
                debug!(hash = %held.hash, lookup = missed.as_str(), next, "Held magnet still missing");
                self.db
                    .held()
                    .reschedule(&held.hash, next, missed.as_str())
                    .await?;
                Ok(())
            }
        }
    }

    /// The item of a held magnet, while it is still held
    async fn held_item(&self, hash: &str) -> Result<Option<ArrItem>, ArrError> {
        let item = self.db.arr().get_by_hash(hash).await?;
        Ok(item.filter(is_held))
    }

    /// Send a held magnet to Put.io and settle its check. A refused add fails the item
    /// with Put.io's message.
    async fn end_hold(
        &self,
        held: &Held,
        item: &ArrItem,
        end: HoldEnd,
        now: i64,
    ) -> Result<(), ArrError> {
        // Removing a held item takes the same lock, so neither runs half-way
        let _guard = self.add_lock.lock().await;
        if self.held_item(&held.hash).await?.is_none() {
            self.db.held().delete(&held.hash).await?;
            return Ok(());
        }
        let category = held.category.as_deref().unwrap_or("");
        let payload = Payload::Magnet(held.magnet.clone());
        let repo = self.db.arr();
        let releases = self.db.releases();
        let result = match self.send_to_putio(&held.hash, category, payload).await {
            Ok(transfer) => {
                let progress = PutioProgress {
                    status: &transfer.status,
                    percent: 0,
                    eta: None,
                    size: transfer.size,
                };
                repo.set_transfer(&held.hash, transfer.id, &progress)
                    .await?;
                let settle = |state, lookup, cache, total, reason| HeldSettle {
                    state,
                    lookup,
                    lookup_cache: cache,
                    total_size: total,
                    reason,
                    transfer_id: Some(transfer.id),
                };
                let (result, settled) = match &end {
                    HoldEnd::Found { cache, total } => {
                        let found = settle(
                            CheckState::Passed,
                            Some("found"),
                            Some(cache.as_str()),
                            *total,
                            None,
                        );
                        (HoldResult::FoundPassed, Some(found))
                    }
                    HoldEnd::Unchecked => {
                        let off = settle(CheckState::Passed, None, None, 0, Some("unchecked"));
                        (HoldResult::Unchecked, Some(off))
                    }
                    HoldEnd::RanOut => (HoldResult::RanOut, None),
                };
                if let Some(id) = held.check_id {
                    let written = match settled {
                        Some(settle) => releases.settle_held(id, &settle).await,
                        // Checked on Put.io once its files are there (042 US3)
                        None => releases.set_check_transfer(id, transfer.id).await,
                    };
                    if let Err(e) = written {
                        warn!(hash = %held.hash, error = %e, "Failed to settle held release check");
                    }
                }
                if let (HoldEnd::RanOut, Some(precheck)) = (&end, &self.precheck) {
                    let lookup = held.last_lookup.as_deref().unwrap_or("");
                    precheck.unknown(&held.hash, &item.name, PrecheckSource::Magnet, lookup);
                }
                info!(
                    hash = %held.hash,
                    name = %item.name,
                    transfer_id = transfer.id,
                    result = result.as_str(),
                    attempts = held.attempts,
                    "Held magnet sent to Put.io"
                );
                result
            }
            Err(e) => {
                let message = e.to_string();
                repo.set_state(&held.hash, ArrState::Failed, Some(&message), false)
                    .await?;
                if let Err(e) = releases
                    .resolve_pending_by_hash(&held.hash, "putio_refused")
                    .await
                {
                    warn!(hash = %held.hash, error = %e, "Failed to settle held release check");
                }
                warn!(hash = %held.hash, name = %item.name, error = %message, "arr item failed");
                match end {
                    HoldEnd::Found { .. } => HoldResult::FoundPassed,
                    HoldEnd::Unchecked => HoldResult::Unchecked,
                    HoldEnd::RanOut => HoldResult::RanOut,
                }
            }
        };
        self.db
            .held()
            .finish(&held.hash, result.as_str(), now)
            .await?;
        precheck_metrics::hold_ended(result, held_for(held, now));
        Ok(())
    }

    /// A later ask found a file list that fails the check: fail the item as an add would
    /// have (042 reject), with no Put.io call
    #[allow(clippy::too_many_arguments)]
    async fn reject_held(
        &self,
        held: &Held,
        item: &ArrItem,
        source: PrecheckSource,
        meta: Option<Metainfo>,
        reason: Reason,
        policy: &Policy,
        cache: String,
        now: i64,
    ) -> Result<(), ArrError> {
        let _guard = self.add_lock.lock().await;
        if self.held_item(&held.hash).await?.is_none() {
            self.db.held().delete(&held.hash).await?;
            return Ok(());
        }
        let category = held.category.as_deref().unwrap_or("");
        let total = meta.as_ref().map_or(0, Metainfo::total_size) as i64;
        let message = reason.message(Stage::Before);
        let blocklist = self.blocklist_server(category).is_some();
        self.db
            .arr()
            .set_state(&held.hash, ArrState::Failed, Some(&message), blocklist)
            .await?;
        if let Some(id) = held.check_id {
            let settle = HeldSettle {
                state: CheckState::Rejected,
                lookup: Some("found"),
                lookup_cache: Some(&cache),
                total_size: total,
                reason: Some(reason.key()),
                transfer_id: None,
            };
            if let Err(e) = self.db.releases().settle_held(id, &settle).await {
                warn!(hash = %held.hash, error = %e, "Failed to settle held release check");
            }
        }
        let release = Release {
            hash: &held.hash,
            name: Some(item.name.clone()),
            category,
            source,
            lookup: None,
            cache: None,
        };
        self.record_report(
            &release,
            &item.name,
            held.check_id,
            meta.as_ref(),
            &reason,
            policy,
        )
        .await;
        warn!(hash = %held.hash, name = %item.name, error = %message, "arr item failed");
        let result = HoldResult::FoundRejected;
        self.db
            .held()
            .finish(&held.hash, result.as_str(), now)
            .await?;
        precheck_metrics::hold_ended(result, held_for(held, now));
        Ok(())
    }

    /// Set the held magnets gauge
    async fn update_held_gauge(&self) {
        match self.db.held().count().await {
            Ok(n) => precheck_metrics::set_held(n),
            Err(e) => debug!(error = %e, "Failed to count held magnets"),
        }
    }

    /// Check a release before Put.io. A magnet is looked up in the caches with the time
    /// left before `deadline`. `None` when the check is off for the category.
    async fn precheck(
        &self,
        payload: &Payload,
        hash: &str,
        name: Option<&str>,
        category: &str,
        source: PrecheckSource,
        deadline: Instant,
    ) -> Option<Checked> {
        let precheck = self.precheck.as_ref()?;
        let policy = precheck.policy(category, Key::Category(category));
        if !policy.enabled {
            return None;
        }
        let (outcome, lookup, cache) = match payload {
            Payload::Torrent(bytes) => (precheck.check(bytes, &policy, source), None, None),
            Payload::Magnet(_) => {
                let mut left = deadline.saturating_duration_since(Instant::now());
                let holding = precheck.config().holding();
                if holding {
                    // A miss is held and asked again, so the first ask is short (043 US2)
                    left = left.min(Duration::from_secs(precheck.config().first_lookup_secs));
                }
                let found = precheck.lookup(hash, left).await;
                let result = found.as_str();
                match found {
                    Lookup::Found { meta, cache } => (
                        precheck.check_meta(meta, &policy, source),
                        Some(result),
                        Some(cache),
                    ),
                    Lookup::Missed(missed) => {
                        if !holding {
                            precheck.unknown(hash, name.unwrap_or(hash), source, missed.as_str());
                        }
                        let outcome = Outcome::Unknown {
                            hash: hash.to_string(),
                            name: name.map(str::to_string),
                        };
                        (outcome, Some(result), None)
                    }
                }
            }
        };
        Some(Checked {
            outcome,
            policy,
            lookup,
            cache,
        })
    }

    /// A release that failed the check: listed as failed with no Put.io transfer, and
    /// blocklisted like any failed item (042 research R7)
    async fn reject(
        &self,
        release: Release<'_>,
        meta: Option<Metainfo>,
        reason: Reason,
        policy: &Policy,
    ) -> Result<AddOutcome, ArrError> {
        let Release {
            hash,
            name,
            category,
            source,
            lookup,
            cache,
        } = release;
        let name = name
            .or_else(|| meta.as_ref().map(|m| m.name.clone()))
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| hash.to_string());
        let total = meta.as_ref().map_or(0, Metainfo::total_size) as i64;
        let message = reason.message(Stage::Before);

        let repo = self.db.arr();
        repo.insert(&NewArrItem {
            hash,
            name: &name,
            category,
            transfer_id: None,
            putio_status: "",
            total_size: total,
        })
        .await?;
        let blocklist = self.blocklist_server(category).is_some();
        repo.set_state(hash, ArrState::Failed, Some(&message), blocklist)
            .await?;

        let check_id = self
            .record_check(&NewCheck {
                hash: Some(hash),
                name: &name,
                source: source.as_str(),
                category: Some(category),
                stage: Stage::Before.as_str(),
                state: Some(CheckState::Rejected),
                lookup,
                lookup_cache: cache,
                total_size: total,
                reason: Some(reason.key()),
                ..NewCheck::default()
            })
            .await;
        let release = Release {
            hash,
            name: None,
            category,
            source,
            lookup,
            cache,
        };
        self.record_report(&release, &name, check_id, meta.as_ref(), &reason, policy)
            .await;
        warn!(hash, name = %name, error = %message, "arr item failed");
        self.update_gauge().await;

        let item = repo
            .get_by_hash(hash)
            .await?
            .ok_or_else(|| ArrError::Internal("added item missing".to_string()))?;
        Ok(AddOutcome::Added(item))
    }

    /// Write the `release_reports` row of a rejection. A failure is logged.
    async fn record_report(
        &self,
        release: &Release<'_>,
        name: &str,
        check_id: Option<i64>,
        meta: Option<&Metainfo>,
        reason: &Reason,
        policy: &Policy,
    ) {
        let hash = release.hash;
        let traits = meta.map(|m| Traits::of(m, policy));
        let types = traits.as_ref().map_or("[]".to_string(), Traits::types_json);
        let hosts = traits
            .as_ref()
            .map_or("[]".to_string(), Traits::tracker_hosts_json);
        let report = NewReport {
            kind: ReportKind::Rejected,
            check_id,
            arr_hash: Some(hash),
            source: Some(release.source.as_str()),
            category: Some(release.category),
            stage: Some(Stage::Before.as_str()),
            name,
            hash: Some(hash),
            total_size: Some(meta.map_or(0, Metainfo::total_size) as i64),
            file_count: traits.as_ref().map(|t| t.file_count as i64),
            types: &types,
            largest_wanted: traits.as_ref().map(|t| t.largest_wanted as i64),
            min_size: Some(policy.min_size as i64),
            tracker_hosts: &hosts,
            is_private: meta.is_some_and(|m| m.private),
            reason: Some(reason.key()),
            ..NewReport::default()
        };
        if let Err(e) = self.db.releases().insert_report(&report).await {
            warn!(hash, error = %e, "Failed to record release report");
        }
    }

    /// Write a `release_checks` row. A failure is logged: the add goes on.
    async fn record_check(&self, check: &NewCheck<'_>) -> Option<i64> {
        match self.db.releases().insert_check(check).await {
            Ok(id) => Some(id),
            Err(e) => {
                warn!(hash = ?check.hash, error = %e, "Failed to record release check");
                None
            }
        }
    }

    /// The Put.io folder id for a category, creating missing segments under the root
    async fn category_folder(&self, category: &str) -> Result<i64, ArrError> {
        let refused = |e: putio_sync_client::ClientError| ArrError::Refused(error_chain(&e));
        let mut parent = 0;
        for segment in category.split('/').filter(|s| !s.is_empty()) {
            let listing = self.client.list_files(parent).await.map_err(refused)?;
            let existing = listing
                .files
                .iter()
                .find(|f| f.file_type == "FOLDER" && f.name == segment)
                .map(|f| f.id);
            parent = match existing {
                Some(id) => id,
                None => {
                    info!(
                        folder = segment,
                        parent, "Creating Put.io folder for arr category"
                    );
                    self.client
                        .create_folder(segment, parent)
                        .await
                        .map_err(refused)?
                        .id
                }
            };
        }
        Ok(parent)
    }

    /// Refresh from Put.io when the last refresh is older than [`REFRESH_INTERVAL`]
    pub async fn refresh_if_stale(&self) {
        {
            let mut last = self.last_refresh.lock().await;
            if last.is_some_and(|t| t.elapsed() < REFRESH_INTERVAL) {
                return;
            }
            self.refresh_states().await;
            *last = Some(Instant::now());
        }
        self.process_blocklist().await;
    }

    /// Refresh from Put.io now
    pub async fn refresh(&self) {
        {
            let mut last = self.last_refresh.lock().await;
            self.refresh_states().await;
            *last = Some(Instant::now());
        }
        self.process_blocklist().await;
    }

    /// Move each item along the state machine (data-model "State machine")
    async fn refresh_states(&self) {
        let repo = self.db.arr();
        let items = match repo.list().await {
            Ok(items) => items,
            Err(e) => {
                warn!(error = %e, "Failed to list arr items");
                return;
            }
        };

        let needs_putio = items.iter().any(follows_putio);
        if needs_putio {
            match self.client.list_transfers().await {
                Ok(listing) => {
                    let transfers: HashMap<i64, _> =
                        listing.transfers.into_iter().map(|t| (t.id, t)).collect();
                    let pending = self.pending_checks().await;
                    for item in items.iter().filter(|i| follows_putio(i)) {
                        let check = item.transfer_id.and_then(|id| pending.get(&id));
                        if let Err(e) = self.follow_transfer(item, &transfers, check).await {
                            warn!(hash = %item.hash, error = %e, "Failed to update arr item");
                        }
                    }
                }
                // Keep every state as it was; the next poll tries again
                Err(e) => warn!(error = %error_chain(&e), "Failed to list Put.io transfers"),
            }
        }

        // Items that just became local are checked too: their files may already be on disk
        if let Ok(items) = repo.list().await {
            for item in items.iter().filter(|i| i.state == ArrState::Local) {
                if let Err(e) = self.check_local(item).await {
                    warn!(hash = %item.hash, error = %e, "Failed to check arr item files");
                }
            }
        }
        self.update_gauge().await;
    }

    /// Checks waiting for Put.io, by transfer id. One query when there are none.
    async fn pending_checks(&self) -> HashMap<i64, PendingCheck> {
        let releases = self.db.releases();
        let pending = match releases.has_pending().await {
            Ok(false) => return HashMap::new(),
            Ok(true) => releases.pending_checks().await,
            Err(e) => Err(e),
        };
        match pending {
            Ok(checks) => checks
                .into_iter()
                .filter_map(|c| Some((c.transfer_id?, c)))
                .collect(),
            Err(e) => {
                warn!(error = %e, "Failed to list pending release checks");
                HashMap::new()
            }
        }
    }

    /// Follow one item's Put.io transfer. `check` is its release check still waiting for
    /// Put.io, when it is a magnet fallback (042 US3).
    async fn follow_transfer(
        &self,
        item: &ArrItem,
        transfers: &HashMap<i64, putio_sync_client::Transfer>,
        check: Option<&PendingCheck>,
    ) -> Result<(), ArrError> {
        let repo = self.db.arr();
        let Some(transfer) = item.transfer_id.and_then(|id| transfers.get(&id)) else {
            if let Some(check) = check {
                self.check_gone(item, check).await;
            }
            if item.state == ArrState::Putio {
                self.fail(item, "transfer no longer on Put.io").await?;
            }
            return Ok(());
        };

        if item.state == ArrState::Failed {
            if transfer.status == "ERROR" {
                return Ok(());
            }
            // A retried transfer is back on Put.io
            info!(hash = %item.hash, name = %item.name, "arr item recovered on Put.io");
            repo.set_state(&item.hash, ArrState::Putio, None, false)
                .await?;
        }

        repo.update_putio(
            &item.hash,
            &PutioProgress {
                status: &transfer.status,
                percent: i64::from(transfer.percent_done.unwrap_or(0)),
                eta: transfer.estimated_time,
                size: transfer.size.filter(|s| *s > 0),
            },
        )
        .await?;

        if transfer.status == "ERROR" {
            if let Some(check) = check {
                self.check_gone(item, check).await;
            }
            let reason = transfer
                .error_message
                .clone()
                .filter(|m| !m.trim().is_empty())
                .unwrap_or_else(|| "Put.io transfer failed".to_string());
            return self.fail(item, &reason).await;
        }
        if let Some(check) = check.filter(|_| putio::is_gone(transfer)) {
            self.check_gone(item, check).await;
            return self.fail(item, "transfer cancelled on Put.io").await;
        }

        match transfer.file_id {
            Some(file_id) if putio::is_finished(transfer) => {
                if let Some(check) = check {
                    if !self.check_on_putio(item, transfer, file_id, check).await? {
                        return Ok(());
                    }
                }
                self.move_to_local(item, file_id).await
            }
            _ => Ok(()),
        }
    }

    /// Check a magnet fallback's files now that Put.io has them, before any reach the NAS
    /// (042 US3). True when the item goes on to the NAS; false when it was rejected, or
    /// when its files couldn't be listed or deleted and the next pass tries again.
    async fn check_on_putio(
        &self,
        item: &ArrItem,
        transfer: &putio_sync_client::Transfer,
        file_id: i64,
        check: &PendingCheck,
    ) -> Result<bool, ArrError> {
        let releases = self.db.releases();
        let putio_bytes = putio::putio_bytes(transfer);
        let policy = self
            .precheck
            .as_ref()
            .map(|p| (p, p.policy(&item.category, Key::Category(&item.category))))
            .filter(|(_, policy)| policy.enabled);
        // The check was turned off since the add: let it through
        let Some((precheck, policy)) = policy else {
            releases
                .resolve_check(check.id, CheckState::Passed, None, putio_bytes)
                .await?;
            return Ok(true);
        };

        let hash = check.hash.as_deref().unwrap_or(&item.hash);
        let source = PrecheckSource::Magnet;
        let judged = precheck
            .judge_on_putio(
                &self.client,
                hash,
                &item.name,
                transfer,
                file_id,
                &policy,
                source,
            )
            .await;
        match judged {
            OnPutio::Retry => Ok(false),
            OnPutio::Rejected { meta, reason } => {
                releases
                    .resolve_check(
                        check.id,
                        CheckState::Rejected,
                        Some(reason.key()),
                        putio_bytes,
                    )
                    .await?;
                let total = meta.as_ref().map_or(0, Metainfo::total_size) as i64;
                let traits = meta.as_ref().map(|m| Traits::of(m, &policy));
                let types = traits.as_ref().map_or("[]".to_string(), Traits::types_json);
                let report = NewReport {
                    kind: ReportKind::Rejected,
                    check_id: Some(check.id),
                    arr_hash: Some(&item.hash),
                    file_id: Some(file_id),
                    transfer_id: Some(transfer.id),
                    source: Some(source.as_str()),
                    category: Some(&item.category),
                    stage: Some(Stage::Putio.as_str()),
                    name: &item.name,
                    hash: Some(hash),
                    total_size: Some(total),
                    file_count: traits.as_ref().map(|t| t.file_count as i64),
                    types: &types,
                    largest_wanted: traits.as_ref().map(|t| t.largest_wanted as i64),
                    min_size: Some(policy.min_size as i64),
                    reason: Some(reason.key()),
                    ..NewReport::default()
                };
                if let Err(e) = releases.insert_report(&report).await {
                    warn!(hash, error = %e, "Failed to record release report");
                }
                self.fail(item, &reason.message(Stage::Putio)).await?;
                Ok(false)
            }
            OnPutio::Passed => {
                releases
                    .resolve_check(check.id, CheckState::Passed, None, putio_bytes)
                    .await?;
                Ok(true)
            }
        }
    }

    /// Settle a fallback's check whose transfer can't be checked any more: rejected as
    /// `gone`, which no stat counts as fake
    async fn check_gone(&self, item: &ArrItem, check: &PendingCheck) {
        let releases = self.db.releases();
        match releases
            .resolve_check(check.id, CheckState::Rejected, Some(Reason::Gone.key()), 0)
            .await
        {
            Ok(_) => info!(
                hash = %item.hash,
                name = %item.name,
                "Release check on Put.io dropped: the transfer is gone"
            ),
            Err(e) => warn!(hash = %item.hash, error = %e, "Failed to resolve release check"),
        }
    }

    /// Queue a finished transfer's files and record which ones the item waits for
    async fn move_to_local(&self, item: &ArrItem, file_id: i64) -> Result<(), ArrError> {
        let report = match self.downloads.enqueue_tree_report(file_id, true).await {
            Ok(report) => report,
            Err(e) => {
                // Stays on Put.io; the next refresh tries again
                warn!(hash = %item.hash, error = %e, "Failed to queue arr item files");
                return Ok(());
            }
        };
        if report
            .files
            .iter()
            .any(|f| f.decision == TreeDecision::Failed)
        {
            warn!(hash = %item.hash, "Some arr item files could not be queued; retrying later");
            return Ok(());
        }

        let accepted: Vec<(i64, i64)> = report
            .files
            .iter()
            .filter(|f| f.decision.accepted())
            .map(|f| (f.file_id, f.size))
            .collect();
        if accepted.is_empty() {
            let mut blocked: Vec<String> = report
                .files
                .iter()
                .filter_map(|f| match &f.decision {
                    TreeDecision::Blocked(ext) if !ext.is_empty() => Some(format!(".{}", ext)),
                    _ => None,
                })
                .collect();
            blocked.sort();
            blocked.dedup();
            let reason = if blocked.is_empty() {
                "no files to import".to_string()
            } else {
                format!(
                    "no files to import: blocked extensions {}",
                    blocked.join(", ")
                )
            };
            return self.fail(item, &reason).await;
        }

        let output_name = report.root_name.unwrap_or_else(|| item.name.clone());
        self.db
            .arr()
            .set_local(&item.hash, file_id, &output_name, &accepted)
            .await?;
        info!(
            hash = %item.hash,
            name = %output_name,
            files = accepted.len(),
            "arr item on Put.io is complete; downloading"
        );
        Ok(())
    }

    async fn check_local(&self, item: &ArrItem) -> Result<(), ArrError> {
        let (_, all_done) = self.db.arr().local_progress(&item.hash).await?;
        if all_done {
            self.db
                .arr()
                .set_state(&item.hash, ArrState::Done, None, false)
                .await?;
            info!(hash = %item.hash, name = %item.name, "arr item downloaded; ready to import");
        }
        Ok(())
    }

    async fn fail(&self, item: &ArrItem, reason: &str) -> Result<(), ArrError> {
        let blocklist = self.blocklist_server(&item.category).is_some();
        self.db
            .arr()
            .set_state(&item.hash, ArrState::Failed, Some(reason), blocklist)
            .await?;
        warn!(hash = %item.hash, name = %item.name, error = reason, "arr item failed");
        Ok(())
    }

    /// The operator flagged an item fake and asked to fail and blocklist it (042 US6): fail
    /// it with "flagged fake by the operator", delete its Put.io files, and queue the
    /// blocklist call for the next refresh. Under `dry_run` only the delete is logged and
    /// the item is left as it is. True when the blocklist was queued.
    pub async fn flag_fake(&self, hash: &str, dry_run: bool) -> Result<bool, ArrError> {
        let Some(item) = self
            .db
            .arr()
            .get_by_hash(&hash.to_ascii_lowercase())
            .await?
        else {
            return Ok(false);
        };
        if dry_run {
            info!(
                hash = %item.hash,
                name = %item.name,
                transfer_id = ?item.transfer_id,
                file_id = ?item.file_id,
                "Dry run: would fail an arr item flagged fake and delete it from Put.io"
            );
            return Ok(false);
        }
        self.fail(&item, FLAGGED_FAKE).await?;
        if let Some(file_id) = item.file_id {
            // Constitution IV deviation (plan.md Complexity Tracking): the operator asked
            info!(
                hash = %item.hash,
                name = %item.name,
                transfer_id = ?item.transfer_id,
                file_id,
                "Deleting an arr item flagged fake from Put.io"
            );
            if let Err(e) = self.client.delete_files(vec![file_id]).await {
                warn!(hash = %item.hash, error = %error_chain(&e), "Failed to delete Put.io files");
            }
        }
        Ok(self.blocklist_server(&item.category).is_some())
    }

    /// The *arr API for a category, when one with an API key is configured
    fn blocklist_server(&self, category: &str) -> Option<ArrApi> {
        let server = self.settings.server_for(category)?;
        let key = server.api_key.as_deref()?;
        Some(ArrApi::new(
            server.kind,
            &server.url,
            key,
            self.http.clone(),
        ))
    }

    /// Ask the *arr to remove and blocklist each failed item still pending
    async fn process_blocklist(&self) {
        let Ok(_guard) = self.blocklist_lock.try_lock() else {
            return;
        };
        let items = match self.db.arr().list().await {
            Ok(items) => items,
            Err(e) => {
                warn!(error = %e, "Failed to list arr items");
                return;
            }
        };
        for item in items
            .iter()
            .filter(|i| i.blocklist == BlocklistState::Pending)
        {
            let Some(api) = self.blocklist_server(&item.category) else {
                continue;
            };
            let result = match api.find_queue_id(&item.hash).await {
                Ok(Some(queue_id)) => api
                    .remove_and_blocklist(queue_id)
                    .await
                    .map(|_| BlocklistOutcome::Ok),
                Ok(None) => Ok(BlocklistOutcome::NotFound),
                Err(e) => Err(e),
            };
            let (state, attempts) = match result {
                Ok(outcome) => {
                    metrics::blocklist(outcome);
                    info!(hash = %item.hash, name = %item.name, outcome = ?outcome, "arr item blocklisted");
                    (BlocklistState::Done, item.blocklist_attempts)
                }
                Err(e) => {
                    metrics::blocklist(BlocklistOutcome::Error);
                    let attempts = item.blocklist_attempts + 1;
                    let state = if attempts >= MAX_BLOCKLIST_ATTEMPTS {
                        BlocklistState::GaveUp
                    } else {
                        BlocklistState::Pending
                    };
                    warn!(hash = %item.hash, attempts, error = %e, "arr blocklist call failed");
                    (state, attempts)
                }
            };
            if let Err(e) = self
                .db
                .arr()
                .set_blocklist(&item.hash, state, attempts)
                .await
            {
                warn!(hash = %item.hash, error = %e, "Failed to record blocklist state");
            }
        }
    }

    /// Items as Transmission torrents, all of them or those `ids` name
    pub async fn torrents(&self, ids: Option<&[ItemId]>) -> Result<Vec<Value>, ArrError> {
        let repo = self.db.arr();
        let items = repo.list().await?;
        let speeds: HashMap<i64, f64> = self
            .downloads
            .get_active_downloads()
            .await
            .map(|list| list.into_iter().map(|d| (d.file_id, d.speed_bps)).collect())
            .unwrap_or_default();

        let mut out = Vec::new();
        for item in items
            .iter()
            .filter(|i| ids.is_none_or(|ids| matches(i, ids)))
        {
            let files = repo.files(&item.hash).await?;
            let (completed_bytes, _) = repo.local_progress(&item.hash).await?;
            let speed: f64 = files.iter().filter_map(|(id, _)| speeds.get(id)).sum();
            let local = LocalProgress {
                completed_bytes,
                file_count: files.len() as i64,
                speed_bps: speed.max(0.0) as u64,
            };
            out.push(view::torrent(item, &local, &self.target));
        }
        Ok(out)
    }

    /// Hashes of the items `ids` name
    pub async fn resolve(&self, ids: &[ItemId]) -> Result<Vec<String>, ArrError> {
        let items = self.db.arr().list().await?;
        Ok(items
            .into_iter()
            .filter(|i| matches(i, ids))
            .map(|i| i.hash)
            .collect())
    }

    /// Remove an item (research R8). Put.io failures are logged; the rows go regardless.
    pub async fn remove(&self, hash: &str, delete_local: bool) -> Result<(), ArrError> {
        let repo = self.db.arr();
        let Some(item) = repo.get_by_hash(&hash.to_ascii_lowercase()).await? else {
            return Ok(());
        };
        info!(
            hash = %item.hash,
            name = %item.name,
            transfer_id = ?item.transfer_id,
            delete_local,
            "arr item removed"
        );
        if is_held(&item) {
            // Never on Put.io: end the hold under the lock its release takes (043 US3)
            let _guard = self.add_lock.lock().await;
            let (result, now) = (HoldResult::Removed, unix_now());
            if let Some(held) = self
                .db
                .held()
                .finish(&item.hash, result.as_str(), now)
                .await?
            {
                precheck_metrics::hold_ended(result, held_for(&held, now));
                self.update_held_gauge().await;
            }
        }

        if let Some(transfer_id) = item.transfer_id {
            if let Err(e) = self.client.cancel_transfers(vec![transfer_id]).await {
                warn!(hash = %item.hash, error = %error_chain(&e), "Failed to cancel Put.io transfer");
            }
        }
        if let Some(file_id) = item.file_id {
            if item.state == ArrState::Done || delete_local {
                if let Err(e) = self.client.delete_files(vec![file_id]).await {
                    warn!(hash = %item.hash, error = %error_chain(&e), "Failed to delete Put.io files");
                }
            }
        }

        let queue = self.db.queue();
        for (file_id, _) in repo.files(&item.hash).await? {
            match queue.get_by_file_id(file_id).await {
                Ok(Some(row)) if row.status == "completed" => {}
                Ok(Some(row)) => {
                    if row.status == "downloading" {
                        if let Err(e) = self.downloads.cancel_download(file_id).await {
                            warn!(file_id, error = %e, "Failed to cancel download");
                        }
                    }
                    if let Err(e) = queue.remove(file_id).await {
                        warn!(file_id, error = %e, "Failed to drop queued download");
                    }
                }
                Ok(None) => {}
                Err(e) => warn!(file_id, error = %e, "Failed to look up queued download"),
            }
        }

        if delete_local {
            if let Some(name) = &item.output_name {
                self.delete_output(&item.category, name).await;
            }
        }

        // A check still waiting for Put.io would hold the sync gate (042 data-model "States")
        if let Err(e) = self
            .db
            .releases()
            .resolve_pending_by_hash(&item.hash, Reason::Gone.key())
            .await
        {
            warn!(hash = %item.hash, error = %e, "Failed to resolve release checks");
        }
        repo.delete(&item.hash).await?;
        self.update_gauge().await;
        Ok(())
    }

    /// Delete `<target>/<category>/<name>`, only when it stays inside the target
    async fn delete_output(&self, category: &str, name: &str) {
        let Some(path) = output_path(&self.target, category, name) else {
            warn!(
                name,
                "Refusing to delete an arr item outside the target directory"
            );
            return;
        };
        let result = match tokio::fs::symlink_metadata(&path).await {
            Ok(meta) if meta.is_dir() => tokio::fs::remove_dir_all(&path).await,
            Ok(_) => tokio::fs::remove_file(&path).await,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        };
        match result {
            Ok(()) => info!(path = %path.display(), "Deleted arr item data"),
            Err(e) => warn!(path = %path.display(), error = %e, "Failed to delete arr item data"),
        }
    }

    async fn update_gauge(&self) {
        match self.db.arr().counts_by_state().await {
            Ok(counts) => {
                for (state, n) in counts {
                    metrics::set_items(item_state(state), n);
                }
            }
            Err(e) => debug!(error = %e, "Failed to count arr items"),
        }
    }
}

/// A magnet held before Put.io (043)
fn is_held(item: &ArrItem) -> bool {
    item.transfer_id.is_none()
        && item.state == ArrState::Putio
        && item.putio_status.as_deref() == Some(HELD)
}

/// How long a magnet was held
fn held_for(held: &Held, now: i64) -> Duration {
    Duration::from_secs((now - held.held_at).max(0) as u64)
}

/// How a hold ends with a Put.io transfer
enum HoldEnd {
    /// A later ask found a passing file list
    Found { cache: String, total: i64 },
    /// The hold ran out, or holding was turned off: checked on Put.io (042 US3)
    RanOut,
    /// The check is off for the category
    Unchecked,
}

/// An item a refresh follows on Put.io. Items rejected before Put.io have no transfer.
fn follows_putio(item: &ArrItem) -> bool {
    item.transfer_id.is_some() && (item.state == ArrState::Putio || recoverable(item))
}

/// A failed item whose transfer may come back (it failed on Put.io, not locally)
fn recoverable(item: &ArrItem) -> bool {
    item.state == ArrState::Failed && item.putio_status.as_deref() == Some("ERROR")
}

fn matches(item: &ArrItem, ids: &[ItemId]) -> bool {
    ids.iter().any(|id| match id {
        ItemId::Hash(h) => h.eq_ignore_ascii_case(&item.hash),
        ItemId::Id(n) => *n == item.id,
    })
}

fn item_state(state: ArrState) -> ArrItemState {
    match state {
        ArrState::Putio => ArrItemState::Putio,
        ArrState::Local => ArrItemState::Local,
        ArrState::Done => ArrItemState::Done,
        ArrState::Failed => ArrItemState::Failed,
    }
}

fn magnet_payload(magnet: String) -> Result<(String, Option<String>, Payload), ArrError> {
    let (hash, name) = infohash::from_magnet(&magnet).ok_or(ArrError::InvalidMagnet)?;
    Ok((hash, name, Payload::Magnet(magnet)))
}

fn torrent_payload(bytes: Vec<u8>) -> Result<(String, Option<String>, Payload), ArrError> {
    let (hash, name) = infohash::from_torrent(&bytes).ok_or(ArrError::InvalidTorrent)?;
    Ok((hash, name, Payload::Torrent(bytes)))
}

/// `<target>/<category>/<name>` when the name is a single safe component and the
/// category has no `..`
fn output_path(target: &Path, category: &str, name: &str) -> Option<PathBuf> {
    let safe_name = !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains('\0');
    let category = normalize_arr_category(category)?;
    if !safe_name {
        return None;
    }
    let mut path = target.to_path_buf();
    for segment in category.split('/').filter(|s| !s.is_empty()) {
        path.push(segment);
    }
    path.push(name);
    path.starts_with(target).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_paths_stay_inside() {
        let target = Path::new("/downloads");
        assert_eq!(
            output_path(target, "sonarr", "Show.S01"),
            Some(PathBuf::from("/downloads/sonarr/Show.S01"))
        );
        assert_eq!(
            output_path(target, "", "Movie"),
            Some(PathBuf::from("/downloads/Movie"))
        );
        assert_eq!(output_path(target, "sonarr", ".."), None);
        assert_eq!(output_path(target, "sonarr", "a/b"), None);
        assert_eq!(output_path(target, "../etc", "x"), None);
        assert_eq!(output_path(target, "sonarr", ""), None);
    }
}
