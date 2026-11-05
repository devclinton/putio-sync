//! The Active transfers pipeline (feature 039): one item per transfer, in the Put.io or
//! the NAS stage, built from the Put.io transfer list and the queue.
//!
//! [`build`] is pure, so every state rule is tested without Put.io (R4, R5, R6).
//! `DownloadManager::pipeline` gathers its input.

use chrono::{DateTime, NaiveDateTime, SecondsFormat, Utc};
use putio_sync_client::models::Transfer;
use putio_sync_core::stall::Stall;
use putio_sync_db::pipeline::{FileState, PipelineFile};
use serde::{Serialize, Serializer};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

/// Where an item is
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Putio,
    Nas,
}

/// An item's state within its stage
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Put.io or NAS: waiting its turn
    Queued,
    /// Put.io or NAS: bytes are moving
    Downloading,
    /// Put.io: no new bytes for the stall threshold (feature 007)
    Stalled,
    /// Put.io: `COMPLETING`
    Finishing,
    /// Put.io: `ERROR`
    Error,
    /// NAS: Put.io is done and the sync has not queued its files yet
    WaitingForSync,
    /// NAS: every file left is paused
    Paused,
    /// NAS: a file failed
    Failed,
}

/// Where an item's files are (NAS stage, R1)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct FileCounts {
    pub done: usize,
    pub downloading: usize,
    pub queued: usize,
    pub paused: usize,
    pub failed: usize,
    pub skipped: usize,
    /// Every file except skipped ones
    pub total: usize,
}

/// How an item reached Put.io (FR-009)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// Radarr or Sonarr, with its category
    Arr(String),
    /// A watch folder
    Watch,
    /// The web UI
    Manual,
    /// Added on Put.io directly
    Putio,
}

impl Origin {
    /// `download_sources.origin` or `transfer_origins.origin`; `unknown` is none
    pub fn from_db(origin: &str) -> Option<Origin> {
        match origin {
            "watch" => Some(Origin::Watch),
            "manual" => Some(Origin::Manual),
            "putio" => Some(Origin::Putio),
            other => other
                .strip_prefix("arr:")
                .filter(|category| !category.is_empty())
                .map(|category| Origin::Arr(category.to_string())),
        }
    }
}

impl Serialize for Origin {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Origin::Arr(category) => serializer.serialize_str(&format!("arr:{category}")),
            Origin::Watch => serializer.serialize_str("watch"),
            Origin::Manual => serializer.serialize_str("manual"),
            Origin::Putio => serializer.serialize_str("putio"),
        }
    }
}

/// A stalled transfer, as the Transfers page shows it (feature 007)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StallView {
    /// Last progress, Unix seconds
    pub since: u64,
    /// Seconds since the last progress
    pub seconds: u64,
    /// `availability N%`, `no peers` or `no progress`
    pub reason: String,
}

/// Links to the pages that act on an item
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Links {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transfers: Option<String>,
    pub queue: String,
}

/// One row of the Active page
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PipelineItem {
    /// `transfer:<id>`, `folder:<relative_path>` or `file:<file_id>`
    pub key: String,
    pub name: String,
    pub transfer_id: Option<i64>,
    pub stage: Stage,
    pub state: State,
    /// A hold row exists (paused while on Put.io)
    pub held: bool,
    /// Its own pause or hold, not the NAS switch
    pub paused: bool,
    pub size: i64,
    pub done_bytes: i64,
    pub percent: f64,
    pub speed_bps: Option<u64>,
    pub eta_seconds: Option<i64>,
    pub files: Option<FileCounts>,
    pub putio_status: Option<String>,
    pub seeding: bool,
    pub stall: Option<StallView>,
    pub origin: Option<Origin>,
    pub reason: Option<String>,
    #[serde(serialize_with = "rfc3339_opt")]
    pub next_sync_at: Option<SystemTime>,
    pub note: Option<String>,
    /// NAS: `checking` or `verifying` while every running file reads back from disk
    /// (features 040, 034); none otherwise
    pub phase: Option<&'static str>,
    pub links: Links,
}

/// Item counts in the Put.io stage, with zeros. `on_hold` is outside the sum.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct PutioCounts {
    pub queued: usize,
    pub downloading: usize,
    pub stalled: usize,
    pub finishing: usize,
    pub error: usize,
    pub on_hold: usize,
}

/// Item counts in the NAS stage, with zeros
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct NasCounts {
    pub waiting_for_sync: usize,
    pub queued: usize,
    pub downloading: usize,
    pub paused: usize,
    pub failed: usize,
}

/// Counts by state for each stage (US2)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    pub putio: PutioCounts,
    pub nas: NasCounts,
}

/// The Active page
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Pipeline {
    pub items: Vec<PipelineItem>,
    pub counts: Counts,
    /// When the Put.io figures were fetched (FR-029)
    #[serde(serialize_with = "rfc3339_opt")]
    pub putio_as_of: Option<SystemTime>,
    /// The last Put.io fetch failed, so its figures are older
    pub putio_stale: bool,
}

/// A running file's live progress
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct FileProgress {
    pub downloaded: u64,
    pub speed_bps: u64,
    /// `downloading`, `checking` or `verifying` (features 040, 034)
    pub phase: &'static str,
}

/// Everything [`build`] reads
#[derive(Debug, Clone)]
pub struct BuildInput {
    /// The Put.io transfer list
    pub transfers: Vec<Transfer>,
    /// Queued files, files of listed transfers, and recently done files
    pub files: Vec<PipelineFile>,
    /// Held transfer ids
    pub holds: HashSet<i64>,
    /// Live progress of running files, by file id
    pub progress: HashMap<i64, FileProgress>,
    /// Stall verdicts, by transfer id
    pub stalls: HashMap<i64, Stall>,
    /// Origins known by transfer id: Radarr/Sonarr first, then `transfer_origins`
    pub transfer_origins: HashMap<i64, Origin>,
    /// In-memory notes, by file id
    pub notes: HashMap<i64, String>,
    /// When the last completed sync run started; `None` before the first
    pub last_sync_start: Option<SystemTime>,
    /// When the sync runs next
    pub next_sync_at: Option<SystemTime>,
    pub now: SystemTime,
    pub putio_as_of: Option<SystemTime>,
    pub putio_stale: bool,
}

impl Default for BuildInput {
    fn default() -> Self {
        Self {
            transfers: Vec::new(),
            files: Vec::new(),
            holds: HashSet::new(),
            progress: HashMap::new(),
            stalls: HashMap::new(),
            transfer_origins: HashMap::new(),
            notes: HashMap::new(),
            last_sync_start: None,
            next_sync_at: None,
            now: UNIX_EPOCH,
            putio_as_of: None,
            putio_stale: false,
        }
    }
}

/// Build the Active page from its input
pub fn build(input: &BuildInput) -> Pipeline {
    // Group the files by item key: their transfer, else their folder, else the file
    let mut groups: BTreeMap<String, Vec<&PipelineFile>> = BTreeMap::new();
    for file in &input.files {
        groups.entry(file_key(file)).or_default().push(file);
    }

    let mut items = Vec::new();
    for transfer in input.transfers.iter() {
        let key = format!("transfer:{}", transfer.id);
        let files = groups.remove(&key).unwrap_or_default();
        if let Some(item) = transfer_item(input, transfer, key, &files) {
            items.push(item);
        }
    }
    // Items whose transfer is gone from Put.io, and items with no transfer
    for (key, files) in groups {
        if let Some(item) = nas_item(input, key, None, &files) {
            items.push(item);
        }
    }

    items.sort_by(|a, b| {
        rank(a)
            .cmp(&rank(b))
            .then_with(|| match (a.state, b.state) {
                (State::Downloading, State::Downloading) => b.speed_bps.cmp(&a.speed_bps),
                _ => std::cmp::Ordering::Equal,
            })
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.key.cmp(&b.key))
    });

    Pipeline {
        counts: count(&items),
        items,
        putio_as_of: input.putio_as_of,
        putio_stale: input.putio_stale,
    }
}

/// Counts by state for each stage. Each item is counted once, in its current stage;
/// `on_hold` counts held items not yet on the NAS, outside the sum (FR-011, SC-004).
fn count(items: &[PipelineItem]) -> Counts {
    let mut counts = Counts::default();
    for item in items {
        let (putio, nas) = (&mut counts.putio, &mut counts.nas);
        match item.state {
            State::Queued if item.stage == Stage::Putio => putio.queued += 1,
            State::Downloading if item.stage == Stage::Putio => putio.downloading += 1,
            State::Stalled => putio.stalled += 1,
            State::Finishing => putio.finishing += 1,
            State::Error => putio.error += 1,
            State::WaitingForSync => nas.waiting_for_sync += 1,
            State::Queued => nas.queued += 1,
            State::Downloading => nas.downloading += 1,
            State::Paused => nas.paused += 1,
            State::Failed => nas.failed += 1,
        }
        if item.held && (item.stage == Stage::Putio || item.state == State::WaitingForSync) {
            counts.putio.on_hold += 1;
        }
    }
    counts
}

/// How many items the Status card lists
pub const SUMMARY_TOP: usize = 5;

/// The Status page's Active transfers card (`GET /status` `active`, FR-030)
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Summary {
    pub total: usize,
    pub putio: usize,
    pub nas: usize,
    /// `error` and `failed` items, also counted in `putio` or `nas`
    pub failed: usize,
    /// Up to [`SUMMARY_TOP`] items, busiest first
    pub top: Vec<TopItem>,
}

/// One line of the Status card
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TopItem {
    pub key: String,
    pub name: String,
    pub stage: Stage,
    pub state: State,
    pub percent: f64,
    pub eta_seconds: Option<i64>,
}

/// The Status card of `pipeline`: totals, and the busiest items, downloading before
/// queued, then by speed
pub fn summary(pipeline: &Pipeline) -> Summary {
    let items = &pipeline.items;
    let busy = |item: &PipelineItem| match item.state {
        State::Downloading | State::Finishing => 0,
        State::Queued => 1,
        _ => 2,
    };
    let mut top: Vec<&PipelineItem> = items.iter().collect();
    // Stable: equal items keep the page's order
    top.sort_by_key(|i| (busy(i), std::cmp::Reverse(i.speed_bps.unwrap_or(0))));
    Summary {
        total: items.len(),
        putio: items.iter().filter(|i| i.stage == Stage::Putio).count(),
        nas: items.iter().filter(|i| i.stage == Stage::Nas).count(),
        failed: items
            .iter()
            .filter(|i| matches!(i.state, State::Error | State::Failed))
            .count(),
        top: top
            .into_iter()
            .take(SUMMARY_TOP)
            .map(|i| TopItem {
                key: i.key.clone(),
                name: i.name.clone(),
                stage: i.stage,
                state: i.state,
                percent: i.percent,
                eta_seconds: i.eta_seconds,
            })
            .collect(),
    }
}

/// The item key of a file (R2)
fn file_key(file: &PipelineFile) -> String {
    match (file.transfer_id, file.relative_path.as_deref()) {
        (Some(id), _) => format!("transfer:{id}"),
        (None, Some(path)) if !path.is_empty() => format!("folder:{path}"),
        (None, _) => format!("file:{}", file.file_id),
    }
}

/// Sort rank: failed and error first, then downloading, then the rest
fn rank(item: &PipelineItem) -> u8 {
    match item.state {
        State::Error | State::Failed => 0,
        State::Downloading => 1,
        _ => 2,
    }
}

/// The item of a transfer in the Put.io list, or `None` when it is not active (R5, R6)
fn transfer_item(
    input: &BuildInput,
    transfer: &Transfer,
    key: String,
    files: &[&PipelineFile],
) -> Option<PipelineItem> {
    let status = transfer.status.to_ascii_uppercase();
    let state = match status.as_str() {
        "DOWNLOADING" if input.stalls.contains_key(&transfer.id) => State::Stalled,
        "DOWNLOADING" => State::Downloading,
        "COMPLETING" => State::Finishing,
        "ERROR" => State::Error,
        "COMPLETED" | "SEEDING" => {
            if files.is_empty() {
                return waiting_item(input, transfer, key);
            }
            return nas_item(input, key, Some(transfer), files);
        }
        // IN_QUEUE, WAITING, PREPARING_DOWNLOAD, and anything Put.io adds later
        _ => State::Queued,
    };

    let held = input.holds.contains(&transfer.id);
    let size = transfer.size.unwrap_or(0);
    Some(PipelineItem {
        name: transfer.name.clone(),
        transfer_id: Some(transfer.id),
        stage: Stage::Putio,
        state,
        held,
        paused: held,
        size,
        done_bytes: transfer.downloaded.unwrap_or(0),
        percent: transfer.percent_done.unwrap_or(0).clamp(0, 100) as f64,
        // Speed and time left only while Put.io is downloading it
        speed_bps: transfer
            .down_speed
            .filter(|_| state == State::Downloading)
            .map(|s| s.max(0) as u64),
        eta_seconds: transfer
            .estimated_time
            .filter(|_| state == State::Downloading),
        files: None,
        putio_status: Some(transfer.status.clone()),
        seeding: false,
        stall: (state == State::Stalled)
            .then(|| {
                input
                    .stalls
                    .get(&transfer.id)
                    .map(|s| stall_view(s, input.now))
            })
            .flatten(),
        origin: origin(input, Some(transfer.id), files),
        reason: (state == State::Error).then(|| {
            transfer
                .error_message
                .clone()
                .unwrap_or_else(|| "Put.io reported an error".to_string())
        }),
        next_sync_at: None,
        note: None,
        phase: None,
        links: links(Some(transfer.id)),
        key,
    })
}

/// A finished transfer with no files queued yet: Waiting for sync if it finished after
/// the last sync run started (or no sync has run), else not active (R6)
fn waiting_item(input: &BuildInput, transfer: &Transfer, key: String) -> Option<PipelineItem> {
    let finished = transfer.finished_at.as_deref().and_then(parse_putio_time);
    let waiting = match (input.last_sync_start, finished) {
        (None, _) => true,
        (Some(start), Some(finished)) => finished > start,
        (Some(_), None) => false,
    };
    if !waiting {
        return None;
    }
    let held = input.holds.contains(&transfer.id);
    let size = transfer.size.unwrap_or(0);
    Some(PipelineItem {
        name: transfer.name.clone(),
        transfer_id: Some(transfer.id),
        stage: Stage::Nas,
        state: State::WaitingForSync,
        held,
        paused: held,
        size,
        done_bytes: 0,
        percent: 0.0,
        speed_bps: None,
        eta_seconds: None,
        files: None,
        putio_status: Some(transfer.status.clone()),
        seeding: transfer.status.eq_ignore_ascii_case("SEEDING"),
        stall: None,
        origin: origin(input, Some(transfer.id), &[]),
        reason: None,
        next_sync_at: input.next_sync_at,
        note: None,
        phase: None,
        links: links(Some(transfer.id)),
        key,
    })
}

/// An item in the NAS stage, from its files, or `None` when no file is left in the
/// queue: it is then done, or failed when every file was skipped (R5)
fn nas_item(
    input: &BuildInput,
    key: String,
    transfer: Option<&Transfer>,
    files: &[&PipelineFile],
) -> Option<PipelineItem> {
    let mut counts = FileCounts::default();
    let mut size = 0;
    let mut done_bytes = 0;
    let mut speed = None::<u64>;
    let mut phases = Vec::new();
    for file in files {
        match &file.state {
            // A queue row left with status completed (older versions) is done too
            FileState::Done => {
                counts.done += 1;
                done_bytes += file.size;
            }
            FileState::Queued(status) if status == "completed" => {
                counts.done += 1;
                done_bytes += file.size;
            }
            FileState::Skipped => {
                counts.skipped += 1;
                continue;
            }
            FileState::Queued(status) => {
                match status.as_str() {
                    "downloading" => counts.downloading += 1,
                    "paused" => counts.paused += 1,
                    "failed" => counts.failed += 1,
                    _ => counts.queued += 1,
                }
                if let Some(progress) = input.progress.get(&file.file_id) {
                    done_bytes += progress.downloaded.min(file.size.max(0) as u64) as i64;
                    if status == "downloading" {
                        *speed.get_or_insert(0) += progress.speed_bps;
                        phases.push(progress.phase);
                    }
                }
            }
        }
        size += file.size;
    }
    counts.total = files.len() - counts.skipped;

    let state = if counts.failed > 0 {
        State::Failed
    } else if counts.downloading > 0 {
        State::Downloading
    } else if counts.queued > 0 {
        State::Queued
    } else if counts.paused > 0 {
        State::Paused
    } else {
        return None;
    };

    let transfer_id = transfer
        .map(|t| t.id)
        .or_else(|| files.first().and_then(|f| f.transfer_id));
    let held = transfer_id.is_some_and(|id| input.holds.contains(&id));
    let name = match transfer {
        Some(transfer) => transfer.name.clone(),
        None => local_name(&key, files),
    };
    let note = files
        .iter()
        .find_map(|f| input.notes.get(&f.file_id).cloned());
    let remaining = (size - done_bytes).max(0);
    Some(PipelineItem {
        name,
        transfer_id,
        stage: Stage::Nas,
        state,
        held,
        paused: held || state == State::Paused,
        size,
        done_bytes,
        percent: if size > 0 {
            (done_bytes as f64 * 100.0 / size as f64).min(100.0)
        } else {
            0.0
        },
        speed_bps: speed,
        eta_seconds: speed.filter(|s| *s > 0).map(|s| remaining / s as i64),
        files: Some(counts),
        putio_status: transfer.map(|t| t.status.clone()),
        seeding: transfer.is_some_and(|t| t.status.eq_ignore_ascii_case("SEEDING")),
        stall: None,
        origin: origin(input, transfer_id, files),
        reason: (state == State::Failed).then(|| match counts.failed {
            1 => "1 file failed".to_string(),
            n => format!("{n} files failed"),
        }),
        next_sync_at: None,
        note,
        phase: phases
            .first()
            .copied()
            .filter(|p| *p != "downloading" && phases.iter().all(|q| q == p)),
        links: links(transfer_id),
        key,
    })
}

/// The name of an item with no transfer in the list: the folder for a folder row; for a
/// transfer gone from Put.io, the deepest folder its files share, or the file name (R6)
fn local_name(key: &str, files: &[&PipelineFile]) -> String {
    if let Some(folder) = key.strip_prefix("folder:") {
        return folder.to_string();
    }
    let listed: Vec<&&PipelineFile> = files
        .iter()
        .filter(|f| f.state != FileState::Skipped)
        .collect();
    let first = listed.first().copied().or(files.first());
    if listed.len() <= 1 {
        return first.map(|f| f.filename.clone()).unwrap_or_default();
    }
    let paths: Vec<Vec<&str>> = listed
        .iter()
        .map(|f| {
            f.relative_path
                .as_deref()
                .unwrap_or("")
                .split('/')
                .filter(|s| !s.is_empty())
                .collect()
        })
        .collect();
    let mut common = paths[0].clone();
    for path in &paths[1..] {
        let shared = common.iter().zip(path).take_while(|(a, b)| a == b).count();
        common.truncate(shared);
    }
    // The top folder the files share; a file name when they share none
    match paths
        .iter()
        .map(|p| p.first())
        .collect::<HashSet<_>>()
        .len()
    {
        1 if !common.is_empty() => common[0].to_string(),
        _ => first.map(|f| f.filename.clone()).unwrap_or_default(),
    }
}

/// An item's origin: the transfer's, else its files' `download_sources.origin`
fn origin(input: &BuildInput, transfer_id: Option<i64>, files: &[&PipelineFile]) -> Option<Origin> {
    transfer_id
        .and_then(|id| input.transfer_origins.get(&id).cloned())
        .or_else(|| {
            files
                .iter()
                .find_map(|f| f.origin.as_deref())
                .and_then(Origin::from_db)
        })
}

fn links(transfer_id: Option<i64>) -> Links {
    match transfer_id {
        Some(id) => Links {
            transfers: Some(format!("#/transfers?id={id}")),
            queue: format!("#/queue?transfer={id}"),
        },
        None => Links {
            transfers: None,
            queue: "#/queue".to_string(),
        },
    }
}

fn stall_view(stall: &Stall, now: SystemTime) -> StallView {
    StallView {
        since: stall
            .since
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
        seconds: now.duration_since(stall.since).map_or(0, |d| d.as_secs()),
        reason: stall.reason.label(),
    }
}

/// A Put.io timestamp (`2026-01-01T00:00:00`, UTC) or RFC 3339
pub fn parse_putio_time(text: &str) -> Option<SystemTime> {
    NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S")
        .map(|t| t.and_utc())
        .or_else(|_| DateTime::parse_from_rfc3339(text).map(|t| t.with_timezone(&Utc)))
        .ok()
        .map(SystemTime::from)
}

/// Serialize a time as RFC 3339 UTC, to the second
fn rfc3339_opt<S: Serializer>(time: &Option<SystemTime>, serializer: S) -> Result<S::Ok, S::Error> {
    match time {
        Some(time) => serializer.serialize_str(
            &DateTime::<Utc>::from(*time).to_rfc3339_opts(SecondsFormat::Secs, true),
        ),
        None => serializer.serialize_none(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use putio_sync_core::stall::StallReason;
    use std::time::Duration;

    const T0: u64 = 1_767_225_600; // 2026-01-01T00:00:00Z

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(T0 + secs)
    }

    /// A Put.io transfer; `finished` is seconds after T0
    fn transfer(id: i64, status: &str, finished: Option<u64>) -> Transfer {
        let finished_at = finished.map(|s| {
            DateTime::<Utc>::from(at(s))
                .format("%Y-%m-%dT%H:%M:%S")
                .to_string()
        });
        serde_json::from_value(serde_json::json!({
            "id": id,
            "name": format!("T{id}"),
            "size": 1000,
            "status": status,
            "downloaded": 250,
            "down_speed": 50,
            "percent_done": 25,
            "estimated_time": 15,
            "error_message": if status == "ERROR" { Some("tracker gone") } else { None },
            "created_at": "2025-12-31T00:00:00",
            "finished_at": finished_at,
            "save_parent_id": 0,
        }))
        .unwrap()
    }

    fn file(
        id: i64,
        transfer: Option<i64>,
        state: FileState,
        size: i64,
        path: &str,
    ) -> PipelineFile {
        PipelineFile {
            file_id: id,
            transfer_id: transfer,
            state,
            filename: format!("f{id}.mkv"),
            size,
            relative_path: Some(path.to_string()),
            origin: Some("putio".into()),
            recorded_at: None,
            downloaded_at: None,
        }
    }

    fn queued(id: i64, transfer: Option<i64>, status: &str) -> PipelineFile {
        file(id, transfer, FileState::Queued(status.into()), 100, "Show")
    }

    fn input(transfers: Vec<Transfer>, files: Vec<PipelineFile>) -> BuildInput {
        BuildInput {
            transfers,
            files,
            last_sync_start: Some(at(100)),
            now: at(200),
            ..BuildInput::default()
        }
    }

    fn only(pipeline: &Pipeline) -> &PipelineItem {
        assert_eq!(pipeline.items.len(), 1, "{:#?}", pipeline.items);
        &pipeline.items[0]
    }

    fn stage_state(transfer: Transfer, files: Vec<PipelineFile>) -> (Stage, State) {
        let pipeline = build(&input(vec![transfer], files));
        let item = only(&pipeline);
        (item.stage, item.state)
    }

    #[test]
    fn put_io_waiting_statuses_are_queued() {
        for status in ["IN_QUEUE", "WAITING", "PREPARING_DOWNLOAD"] {
            assert_eq!(
                stage_state(transfer(1, status, None), vec![]),
                (Stage::Putio, State::Queued),
                "{status}"
            );
        }
    }

    #[test]
    fn downloading_is_stalled_with_a_verdict() {
        assert_eq!(
            stage_state(transfer(1, "DOWNLOADING", None), vec![]),
            (Stage::Putio, State::Downloading)
        );
        let mut input = input(vec![transfer(1, "DOWNLOADING", None)], vec![]);
        input.stalls.insert(
            1,
            Stall {
                since: at(50),
                reason: StallReason::NoPeers,
            },
        );
        let pipeline = build(&input);
        let item = only(&pipeline);
        assert_eq!((item.stage, item.state), (Stage::Putio, State::Stalled));
        assert_eq!(
            item.stall,
            Some(StallView {
                since: T0 + 50,
                seconds: 150,
                reason: "no peers".into()
            })
        );
    }

    #[test]
    fn completing_is_finishing_and_error_has_its_reason() {
        assert_eq!(
            stage_state(transfer(1, "COMPLETING", None), vec![]),
            (Stage::Putio, State::Finishing)
        );
        let pipeline = build(&input(vec![transfer(1, "ERROR", None)], vec![]));
        let item = only(&pipeline);
        assert_eq!((item.stage, item.state), (Stage::Putio, State::Error));
        assert_eq!(item.reason.as_deref(), Some("tracker gone"));
        // Put.io's last speed and estimate mean nothing once it stopped
        assert_eq!((item.speed_bps, item.eta_seconds), (None, None));
    }

    #[test]
    fn an_unknown_status_is_queued_with_the_raw_status() {
        let pipeline = build(&input(vec![transfer(1, "SOMETHING_NEW", None)], vec![]));
        let item = only(&pipeline);
        assert_eq!((item.stage, item.state), (Stage::Putio, State::Queued));
        assert_eq!(item.putio_status.as_deref(), Some("SOMETHING_NEW"));
    }

    #[test]
    fn put_io_figures_in_the_put_io_stage() {
        let pipeline = build(&input(vec![transfer(1, "DOWNLOADING", None)], vec![]));
        let item = only(&pipeline);
        assert_eq!(item.key, "transfer:1");
        assert_eq!(item.name, "T1");
        assert_eq!((item.size, item.done_bytes), (1000, 250));
        assert_eq!(item.percent, 25.0);
        assert_eq!((item.speed_bps, item.eta_seconds), (Some(50), Some(15)));
        assert_eq!(item.files, None);
        assert_eq!(item.links.transfers.as_deref(), Some("#/transfers?id=1"));
        assert_eq!(item.links.queue, "#/queue?transfer=1");
    }

    #[test]
    fn completed_and_seeding_move_to_the_nas_stage() {
        for (status, seeding) in [("COMPLETED", false), ("SEEDING", true)] {
            let pipeline = build(&input(
                vec![transfer(1, status, Some(10))],
                vec![queued(11, Some(1), "pending")],
            ));
            let item = only(&pipeline);
            assert_eq!((item.stage, item.state), (Stage::Nas, State::Queued));
            assert_eq!(item.seeding, seeding, "{status}");
            assert_eq!(item.putio_status.as_deref(), Some(status));
        }
    }

    #[test]
    fn nas_state_is_failed_then_downloading_then_queued_then_paused() {
        let t = || transfer(1, "COMPLETED", Some(10));
        let cases = [
            (
                vec!["paused", "pending", "downloading", "failed"],
                State::Failed,
            ),
            (vec!["paused", "pending", "downloading"], State::Downloading),
            (vec!["paused", "pending"], State::Queued),
            (vec!["paused"], State::Paused),
        ];
        for (statuses, want) in cases {
            let files = statuses
                .iter()
                .enumerate()
                .map(|(i, s)| queued(10 + i as i64, Some(1), s))
                .collect();
            assert_eq!(stage_state(t(), files), (Stage::Nas, want), "{statuses:?}");
        }
    }

    #[test]
    fn finished_after_the_last_sync_start_is_waiting_for_sync() {
        let mut waiting = input(vec![transfer(1, "COMPLETED", Some(150))], vec![]);
        waiting.next_sync_at = Some(at(400));
        let pipeline = build(&waiting);
        let item = only(&pipeline);
        assert_eq!(
            (item.stage, item.state),
            (Stage::Nas, State::WaitingForSync)
        );
        assert_eq!(item.next_sync_at, Some(at(400)));

        // No sync has run yet
        let mut first = input(vec![transfer(1, "COMPLETED", Some(50))], vec![]);
        first.last_sync_start = None;
        assert_eq!(only(&build(&first)).state, State::WaitingForSync);

        // Finished before the last sync started, with nothing queued: not active
        let old = input(vec![transfer(1, "SEEDING", Some(50))], vec![]);
        assert!(build(&old).items.is_empty());
    }

    #[test]
    fn file_counts_and_done_bytes() {
        let mut input = input(
            vec![transfer(1, "SEEDING", Some(10))],
            vec![
                file(10, Some(1), FileState::Done, 400, "Show"),
                file(11, Some(1), FileState::Skipped, 0, "Show"),
                file(
                    12,
                    Some(1),
                    FileState::Queued("downloading".into()),
                    300,
                    "Show",
                ),
                file(
                    13,
                    Some(1),
                    FileState::Queued("pending".into()),
                    300,
                    "Show",
                ),
                file(14, Some(1), FileState::Queued("paused".into()), 100, "Show"),
                file(15, Some(1), FileState::Queued("failed".into()), 100, "Show"),
            ],
        );
        input.progress.insert(
            12,
            FileProgress {
                downloaded: 120,
                speed_bps: 40,
                phase: "downloading",
            },
        );
        let pipeline = build(&input);
        let item = only(&pipeline);
        assert_eq!(
            item.files,
            Some(FileCounts {
                done: 1,
                downloading: 1,
                queued: 1,
                paused: 1,
                failed: 1,
                skipped: 1,
                total: 5,
            })
        );
        assert_eq!(item.size, 1200);
        assert_eq!(item.done_bytes, 520);
        assert!((item.percent - 520.0 / 12.0).abs() < 1e-9);
        assert_eq!(item.speed_bps, Some(40));
        assert_eq!(item.eta_seconds, Some((1200 - 520) / 40));
        assert_eq!(item.phase, None);

        // Feature 034: the running file reads back from disk
        input.progress.get_mut(&12).unwrap().phase = "verifying";
        assert_eq!(only(&build(&input)).phase, Some("verifying"));
    }

    #[test]
    fn nothing_left_in_the_queue_is_not_active() {
        // At least one done: done
        let done = input(
            vec![transfer(1, "SEEDING", Some(150))],
            vec![
                file(10, Some(1), FileState::Done, 400, "Show"),
                file(11, Some(1), FileState::Skipped, 0, "Show"),
            ],
        );
        assert!(build(&done).items.is_empty());
        // Every file skipped: failed, "nothing downloaded", not active either
        let skipped = input(
            vec![transfer(1, "SEEDING", Some(150))],
            vec![file(11, Some(1), FileState::Skipped, 0, "Show")],
        );
        assert!(build(&skipped).items.is_empty());
        // A queue row left with status completed (older versions) counts as done
        let completed = input(vec![], vec![queued(12, None, "completed")]);
        assert!(build(&completed).items.is_empty());
    }

    #[test]
    fn failed_rows_stay_active() {
        let pipeline = build(&input(
            vec![
                transfer(1, "ERROR", None),
                transfer(2, "COMPLETED", Some(10)),
            ],
            vec![queued(20, Some(2), "failed")],
        ));
        let states: Vec<_> = pipeline.items.iter().map(|i| i.state).collect();
        assert_eq!(states, vec![State::Error, State::Failed]);
        assert!(pipeline.items[1].reason.is_some());
    }

    #[test]
    fn a_transfer_gone_from_put_io_keeps_its_key_and_nas_stage() {
        let pipeline = build(&input(
            vec![],
            vec![
                file(10, Some(7), FileState::Done, 400, "Show S01/Season 1"),
                file(11, Some(7), FileState::Skipped, 0, ""),
                file(
                    12,
                    Some(7),
                    FileState::Queued("pending".into()),
                    300,
                    "Show S01/Season 1",
                ),
                file(
                    13,
                    Some(7),
                    FileState::Queued("pending".into()),
                    300,
                    "Show S01/Season 2",
                ),
            ],
        ));
        let item = only(&pipeline);
        assert_eq!(item.key, "transfer:7");
        assert_eq!(item.transfer_id, Some(7));
        assert_eq!((item.stage, item.state), (Stage::Nas, State::Queued));
        assert_eq!(item.name, "Show S01");
        assert_eq!(item.size, 1000);
        assert_eq!(item.putio_status, None);
    }

    #[test]
    fn a_single_file_transfer_gone_is_named_after_its_file() {
        let pipeline = build(&input(
            vec![],
            vec![file(
                10,
                Some(7),
                FileState::Queued("pending".into()),
                300,
                "sonarr",
            )],
        ));
        assert_eq!(only(&pipeline).name, "f10.mkv");
    }

    #[test]
    fn rows_without_a_transfer_group_by_folder_or_file() {
        let mut files = vec![
            queued(10, None, "pending"),
            queued(11, None, "downloading"),
            file(12, None, FileState::Queued("pending".into()), 5, ""),
        ];
        files[0].relative_path = Some("Photos/2026".into());
        files[1].relative_path = Some("Photos/2026".into());
        let pipeline = build(&input(vec![], files));
        let mut keys: Vec<_> = pipeline
            .items
            .iter()
            .map(|i| {
                (
                    i.key.as_str(),
                    i.name.as_str(),
                    i.stage,
                    i.files.unwrap().total,
                )
            })
            .collect();
        keys.sort();
        assert_eq!(
            keys,
            vec![
                ("file:12", "f12.mkv", Stage::Nas, 1),
                ("folder:Photos/2026", "Photos/2026", Stage::Nas, 2),
            ]
        );
        let folder = pipeline
            .items
            .iter()
            .find(|i| i.key.starts_with("folder"))
            .unwrap();
        assert_eq!(folder.links.transfers, None);
        assert_eq!(folder.links.queue, "#/queue");
    }

    #[test]
    fn origin_prefers_the_transfer_then_the_files() {
        let mut input = input(
            vec![
                transfer(1, "DOWNLOADING", None),
                transfer(2, "COMPLETED", Some(10)),
                transfer(3, "COMPLETED", Some(10)),
            ],
            vec![
                queued(20, Some(2), "pending"),
                queued(30, Some(3), "pending"),
            ],
        );
        input.files[1].origin = Some("unknown".into());
        input
            .transfer_origins
            .insert(1, Origin::Arr("sonarr".into()));
        input.transfer_origins.insert(2, Origin::Watch);
        let pipeline = build(&input);
        let origin = |key: &str| {
            pipeline
                .items
                .iter()
                .find(|i| i.key == key)
                .unwrap()
                .origin
                .clone()
        };
        assert_eq!(origin("transfer:1"), Some(Origin::Arr("sonarr".into())));
        assert_eq!(origin("transfer:2"), Some(Origin::Watch));
        assert_eq!(origin("transfer:3"), None);

        let mut putio = super::tests::input(vec![], vec![queued(40, None, "pending")]);
        putio.files[0].relative_path = Some(String::new());
        assert_eq!(build(&putio).items[0].origin, Some(Origin::Putio));
    }

    #[test]
    fn held_transfers_keep_their_state() {
        let mut input = input(vec![transfer(1, "DOWNLOADING", None)], vec![]);
        input.holds.insert(1);
        let pipeline = build(&input);
        let item = only(&pipeline);
        assert_eq!(item.state, State::Downloading);
        assert!(item.held && item.paused);
    }

    #[test]
    fn restart_notes_show_on_the_item() {
        let mut input = input(
            vec![transfer(1, "COMPLETED", Some(10))],
            vec![queued(11, Some(1), "downloading")],
        );
        input
            .notes
            .insert(11, "Started again: the partial download was gone".into());
        assert_eq!(
            only(&build(&input)).note.as_deref(),
            Some("Started again: the partial download was gone")
        );
    }

    #[test]
    fn sorted_failed_first_then_downloading_by_speed_then_name() {
        let mut slow = transfer(1, "DOWNLOADING", None);
        slow.down_speed = Some(10);
        slow.name = "A".into();
        let mut fast = transfer(2, "DOWNLOADING", None);
        fast.down_speed = Some(99);
        fast.name = "B".into();
        let mut queued_b = transfer(3, "IN_QUEUE", None);
        queued_b.name = "b queued".into();
        let mut queued_a = transfer(4, "IN_QUEUE", None);
        queued_a.name = "a queued".into();
        let error = transfer(5, "ERROR", None);
        let pipeline = build(&input(vec![slow, fast, queued_b, queued_a, error], vec![]));
        let keys: Vec<_> = pipeline.items.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "transfer:5",
                "transfer:2",
                "transfer:1",
                "transfer:4",
                "transfer:3"
            ]
        );
    }

    #[test]
    fn one_item_per_transfer_through_every_stage() {
        // A transfer still on Put.io whose first files are already queued shows once
        let pipeline = build(&input(
            vec![transfer(1, "DOWNLOADING", None)],
            vec![queued(11, Some(1), "pending")],
        ));
        assert_eq!(only(&pipeline).stage, Stage::Putio);
    }

    #[test]
    fn serde_names_match_the_contract() {
        let json = serde_json::to_value(State::WaitingForSync).unwrap();
        assert_eq!(json, "waiting_for_sync");
        assert_eq!(serde_json::to_value(Stage::Nas).unwrap(), "nas");
        assert_eq!(
            serde_json::to_value(Origin::Arr("radarr".into())).unwrap(),
            "arr:radarr"
        );
        let counts = serde_json::to_value(Counts::default()).unwrap();
        assert_eq!(counts["putio"]["on_hold"], 0);
        assert_eq!(counts["nas"]["waiting_for_sync"], 0);
        let mut waiting = input(vec![transfer(1, "COMPLETED", Some(150))], vec![]);
        waiting.next_sync_at = Some(at(400));
        waiting.putio_as_of = Some(at(0));
        let json = serde_json::to_value(build(&waiting)).unwrap();
        assert_eq!(json["putio_as_of"], "2026-01-01T00:00:00Z");
        assert_eq!(json["items"][0]["next_sync_at"], "2026-01-01T00:06:40Z");
    }

    #[test]
    fn origin_labels_from_the_database() {
        assert_eq!(
            Origin::from_db("arr:sonarr"),
            Some(Origin::Arr("sonarr".into()))
        );
        assert_eq!(Origin::from_db("watch"), Some(Origin::Watch));
        assert_eq!(Origin::from_db("arr:"), None);
        assert_eq!(Origin::from_db("unknown"), None);
    }

    #[test]
    fn counts_show_every_state_with_zeros() {
        let json = serde_json::to_value(build(&input(vec![], vec![])).counts).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "putio": { "queued": 0, "downloading": 0, "stalled": 0, "finishing": 0,
                           "error": 0, "on_hold": 0 },
                "nas": { "waiting_for_sync": 0, "queued": 0, "downloading": 0, "paused": 0,
                         "failed": 0 }
            })
        );
    }

    #[test]
    fn counts_add_up_to_the_items_once_each_in_their_stage() {
        let mut input = input(
            vec![
                transfer(1, "IN_QUEUE", None),
                transfer(2, "DOWNLOADING", None),
                transfer(3, "DOWNLOADING", None),
                transfer(4, "COMPLETING", None),
                transfer(5, "ERROR", None),
                transfer(6, "COMPLETED", Some(150)),
                // In both lists: counted once, in the NAS stage (FR-011)
                transfer(7, "SEEDING", Some(10)),
            ],
            vec![
                queued(70, Some(7), "downloading"),
                queued(71, Some(7), "pending"),
                file(80, None, FileState::Queued("pending".into()), 100, "A"),
                file(81, None, FileState::Queued("paused".into()), 100, "B"),
                file(82, None, FileState::Queued("failed".into()), 100, "C"),
            ],
        );
        input.stalls.insert(
            3,
            Stall {
                since: at(50),
                reason: StallReason::NoPeers,
            },
        );
        input.holds.extend([1, 6]);
        let pipeline = build(&input);
        let c = pipeline.counts;
        assert_eq!(
            c.putio,
            PutioCounts {
                queued: 1,
                downloading: 1,
                stalled: 1,
                finishing: 1,
                error: 1,
                on_hold: 2,
            }
        );
        assert_eq!(
            c.nas,
            NasCounts {
                waiting_for_sync: 1,
                queued: 1,
                downloading: 1,
                paused: 1,
                failed: 1,
            }
        );
        let putio = c.putio.queued
            + c.putio.downloading
            + c.putio.stalled
            + c.putio.finishing
            + c.putio.error;
        let nas =
            c.nas.waiting_for_sync + c.nas.queued + c.nas.downloading + c.nas.paused + c.nas.failed;
        assert_eq!(putio + nas, pipeline.items.len(), "SC-004");
    }

    #[test]
    fn summary_totals_by_stage_with_failures_in_both() {
        let input = input(
            vec![
                transfer(1, "IN_QUEUE", None),
                transfer(2, "ERROR", None),
                transfer(3, "COMPLETED", Some(10)),
            ],
            vec![
                queued(30, Some(3), "pending"),
                file(80, None, FileState::Queued("failed".into()), 100, "A"),
            ],
        );
        let s = summary(&build(&input));
        assert_eq!((s.total, s.putio, s.nas, s.failed), (4, 2, 2, 2));
    }

    #[test]
    fn summary_top_is_five_busiest_downloading_before_queued_by_speed() {
        let mut transfers: Vec<Transfer> =
            (1..=4).map(|id| transfer(id, "IN_QUEUE", None)).collect();
        for (id, speed) in [(5, 10), (6, 300), (7, 20)] {
            let mut t = transfer(id, "DOWNLOADING", None);
            t.down_speed = Some(speed);
            transfers.push(t);
        }
        let s = summary(&build(&input(transfers, Vec::new())));
        assert_eq!(s.total, 7);
        let keys: Vec<&str> = s.top.iter().map(|t| t.key.as_str()).collect();
        assert_eq!(keys[..3], ["transfer:6", "transfer:7", "transfer:5"]);
        assert_eq!(s.top.len(), SUMMARY_TOP);
        assert!(s.top[3..].iter().all(|t| t.state == State::Queued));
    }
}
