//! The download client's items: adding them to Put.io, following them to disk, removing
//! them, and asking the *arr to blocklist the ones that fail

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use putio_sync_client::{error_chain, PutioApiClient};
use putio_sync_core::config::{normalize_arr_category, ArrServer, ArrSettings};
use putio_sync_db::{ArrItem, ArrState, BlocklistState, Database, NewArrItem, PutioProgress};
use putio_sync_download::{DownloadManager, TreeDecision};
use putio_sync_telemetry::arr as metrics;
use putio_sync_telemetry::labels::{ArrItemState, BlocklistOutcome};
use serde_json::Value;
use thiserror::Error;
use tokio::sync::Mutex;
use tokio::time::Instant;
use tracing::{debug, info, warn};

use crate::arr_api::ArrApi;
use crate::clients::{self, ClientInfo};
use crate::contact::Contacts;
use crate::infohash;
use crate::path_check::{self, PathCheck, PathStatus};
use crate::view::{self, LocalProgress};

/// How often a poll may refresh from Put.io (research R7)
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(20);

/// Largest .torrent fetched from a URL
const MAX_TORRENT_BYTES: usize = 10 * 1024 * 1024;

/// Blocklist calls tried before giving up
const MAX_BLOCKLIST_ATTEMPTS: i64 = 5;

/// Put.io statuses after which the transfer's files exist
const FINISHED: [&str; 2] = ["COMPLETED", "SEEDING"];

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
    /// Swapped by [`ArrService::apply`] when the settings are saved (feature 018)
    settings: std::sync::RwLock<Arc<ArrSettings>>,
    target: PathBuf,
    http: reqwest::Client,
    contacts: Contacts,
    /// Serialises adds so two of the same hash can't race
    add_lock: Mutex<()>,
    /// Last refresh; held during one, which makes refreshes single-flight
    last_refresh: Mutex<Option<Instant>>,
    /// Held while blocklist calls run, so a slow *arr never queues a second pass
    blocklist_lock: Mutex<()>,
    /// One discovery at a time (feature 018, R9)
    discovery: Mutex<()>,
    /// What each saved application has for putio-sync, by [`url_key`] (feature 018, R4)
    clients: std::sync::RwLock<HashMap<String, ClientInfo>>,
}

/// The cache key for a server URL
pub fn url_key(url: &str) -> String {
    url.trim().trim_end_matches('/').to_ascii_lowercase()
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
            settings: std::sync::RwLock::new(Arc::new(settings)),
            target,
            http: reqwest::Client::new(),
            contacts: Contacts::default(),
            add_lock: Mutex::new(()),
            last_refresh: Mutex::new(None),
            blocklist_lock: Mutex::new(()),
            discovery: Mutex::new(()),
            clients: std::sync::RwLock::new(HashMap::new()),
        }
    }

    /// The last [`ClientInfo`] read for the application at `url`
    pub fn client_info(&self, url: &str) -> Option<ClientInfo> {
        self.clients
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .get(&url_key(url))
            .cloned()
    }

    /// Re-read one application's putio-sync clients. `port` and `hosts` say what points
    /// at putio-sync (R6). Without a key there's nothing to read.
    pub async fn refresh_client_info(
        &self,
        server: &ArrServer,
        port: u16,
        hosts: &[String],
    ) -> Option<ClientInfo> {
        let key = server.api_key.as_deref()?;
        let api = ArrApi::new(server.kind, &server.url, key, self.http.clone());
        let info = clients::read_clients(&api, port, hosts, self.client_info(&server.url)).await;
        if let Some(error) = info.last_error {
            warn!(url = %server.url, error, "Could not read the categories from the arr");
        }
        self.clients
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(url_key(&server.url), info.clone());
        Some(info)
    }

    /// Check that the application at `server` can see the download folder, which its
    /// putio-sync client at `client_host` reports (R10). The result is kept with its
    /// [`ClientInfo`].
    pub async fn check_path(&self, server: &ArrServer, client_host: &str) -> Option<PathCheck> {
        let key = server.api_key.as_deref()?;
        let api = ArrApi::new(server.kind, &server.url, key, self.http.clone());
        let result = path_check::check(&api, &self.download_dir(), client_host).await;
        if result.status != PathStatus::Visible {
            warn!(url = %server.url, status = ?result.status, arr_path = %result.arr_path,
                "The arr may not see the download folder");
        }
        self.clients
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .entry(url_key(&server.url))
            .or_default()
            .path_check = Some(result.clone());
        Some(result)
    }

    /// Re-read every saved application (R4)
    pub async fn refresh_all_client_info(&self, port: u16, hosts: &[String]) {
        let settings = self.settings();
        for server in &settings.servers {
            self.refresh_client_info(server, port, hosts).await;
        }
    }

    /// Re-read every saved application now, then every 15 minutes
    pub fn spawn_category_refresh(
        self: &Arc<Self>,
        port: u16,
        hosts: Vec<String>,
    ) -> tokio::task::JoinHandle<()> {
        let service = Arc::downgrade(self);
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(clients::CATEGORY_REFRESH);
            loop {
                ticker.tick().await;
                let Some(service) = service.upgrade() else {
                    return;
                };
                service.refresh_all_client_info(port, &hosts).await;
            }
        })
    }

    /// Held while a discovery runs (feature 018)
    pub fn discovery_lock(&self) -> &Mutex<()> {
        &self.discovery
    }

    /// The resolved `[arr]` settings
    pub fn settings(&self) -> Arc<ArrSettings> {
        self.settings
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Use new `[arr]` settings from now on. Tracked items, refreshes and downloads carry
    /// on; only the next request sees the change.
    pub fn apply(&self, settings: ArrSettings) {
        *self.settings.write().unwrap_or_else(|e| e.into_inner()) = Arc::new(settings);
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
        let (hash, name, payload) = match source {
            AddSource::Magnet(magnet) => magnet_payload(magnet)?,
            AddSource::Url(url) if url.starts_with("magnet:") => magnet_payload(url)?,
            AddSource::Url(url) => torrent_payload(self.fetch_torrent(&url).await?)?,
            AddSource::Metainfo(bytes) => torrent_payload(bytes)?,
        };

        let _guard = self.add_lock.lock().await;
        let repo = self.db.arr();
        if let Some(existing) = repo.get_by_hash(&hash).await? {
            debug!(hash = %hash, "arr item already tracked");
            return Ok(AddOutcome::Duplicate(existing));
        }

        let parent = self.category_folder(&category).await?;
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
        let transfer = added.map_err(|e| ArrError::Refused(error_chain(&e)))?;

        let name = name
            .or_else(|| Some(transfer.name.clone()).filter(|n| !n.trim().is_empty()))
            .unwrap_or_else(|| hash.clone());
        repo.insert(&NewArrItem {
            hash: &hash,
            name: &name,
            category: &category,
            transfer_id: transfer.id,
            putio_status: &transfer.status,
            total_size: transfer.size.unwrap_or(0),
        })
        .await?;
        info!(hash = %hash, name = %name, transfer_id = transfer.id, "arr item added");
        self.update_gauge().await;

        let item = repo
            .get_by_hash(&hash)
            .await?
            .ok_or_else(|| ArrError::Internal("added item missing".to_string()))?;
        Ok(AddOutcome::Added(item))
    }

    /// Fetch a .torrent from an http(s) URL, capped at 10 MB. The URL is never logged:
    /// indexer URLs carry API keys.
    async fn fetch_torrent(&self, url: &str) -> Result<Vec<u8>, ArrError> {
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(ArrError::InvalidTorrent);
        }
        let failed = |what: &str| {
            warn!(reason = what, "Could not fetch the .torrent URL");
            ArrError::InvalidTorrent
        };
        let mut response = self
            .http
            .get(url)
            .timeout(Duration::from_secs(30))
            .send()
            .await
            .map_err(|_| failed("request failed"))?;
        if !response.status().is_success() {
            return Err(failed("bad status"));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| failed("read failed"))? {
            if bytes.len() + chunk.len() > MAX_TORRENT_BYTES {
                return Err(failed("too large"));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
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

        let needs_putio = items
            .iter()
            .any(|i| i.state == ArrState::Putio || recoverable(i));
        if needs_putio {
            match self.client.list_transfers().await {
                Ok(listing) => {
                    let transfers: HashMap<i64, _> =
                        listing.transfers.into_iter().map(|t| (t.id, t)).collect();
                    for item in items
                        .iter()
                        .filter(|i| i.state == ArrState::Putio || recoverable(i))
                    {
                        if let Err(e) = self.follow_transfer(item, &transfers).await {
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

    /// Follow one item's Put.io transfer
    async fn follow_transfer(
        &self,
        item: &ArrItem,
        transfers: &HashMap<i64, putio_sync_client::Transfer>,
    ) -> Result<(), ArrError> {
        let repo = self.db.arr();
        let Some(transfer) = item.transfer_id.and_then(|id| transfers.get(&id)) else {
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
            let reason = transfer
                .error_message
                .clone()
                .filter(|m| !m.trim().is_empty())
                .unwrap_or_else(|| "Put.io transfer failed".to_string());
            return self.fail(item, &reason).await;
        }

        match transfer.file_id {
            Some(file_id) if FINISHED.contains(&transfer.status.as_str()) => {
                self.move_to_local(item, file_id).await
            }
            _ => Ok(()),
        }
    }

    /// Queue a finished transfer's files and record which ones the item waits for
    async fn move_to_local(&self, item: &ArrItem, file_id: i64) -> Result<(), ArrError> {
        let report = match self.downloads.enqueue_tree_report(file_id).await {
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

    /// The *arr API for a category, when one with an API key is configured
    /// The application whose putio-sync client has `category` (R4). Before the first read,
    /// a legacy typed category is used.
    fn blocklist_server(&self, category: &str) -> Option<ArrApi> {
        let settings = self.settings();
        let wanted = normalize_arr_category(category)?;
        let read = {
            let clients = self.clients.read().unwrap_or_else(|e| e.into_inner());
            settings.servers.iter().find(|s| {
                clients
                    .get(&url_key(&s.url))
                    .is_some_and(|i| i.categories.contains(&wanted))
            })
        };
        let Some(server) = read.or_else(|| settings.server_for(&wanted)) else {
            debug!(category = %wanted, "No application has this category; not blocklisted yet");
            return None;
        };
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
