//! Recently finished (feature 039, R6, R12): which items to write to `pipeline_finished`
//! on each pipeline tick
//!
//! Put.io keeps finished transfers and `download_sources` keeps every file ever queued, so
//! "done" alone cannot decide. A row is written only on a transition: a transfer item that
//! was active on the previous tick and is not now. After a restart (no previous tick) only
//! recent completions are written, and a transfer whose files were all blocked by a rule is
//! written once.

use crate::error::{DownloadError, DownloadResult};
use crate::manager::DownloadManager;
use crate::pipeline::{Pipeline, State};
use chrono::{Duration, NaiveDateTime, Utc};
use putio_sync_client::models::Transfer;
use putio_sync_db::pipeline::{FileState, FinishedOutcome, FinishedRow, PipelineFile};
use std::collections::{HashMap, HashSet};
use tracing::info;

/// What the previous tick knew of an active transfer item
#[derive(Debug, Clone)]
pub struct Seen {
    name: String,
    size: i64,
    state: State,
    reason: Option<String>,
}

/// Active transfer items by transfer id
pub type SeenItems = HashMap<i64, Seen>;

/// The format SQLite stores times in
const TIME_FORMAT: &str = "%Y-%m-%d %H:%M:%S";

fn now_text() -> String {
    Utc::now().format(TIME_FORMAT).to_string()
}

fn parse_time(text: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(&text.get(..19)?.replace('T', " "), TIME_FORMAT).ok()
}

/// A Put.io or SQLite time as SQLite's `YYYY-MM-DD HH:MM:SS`
fn normalize(text: &str) -> Option<String> {
    parse_time(text).map(|t| t.format(TIME_FORMAT).to_string())
}

fn within_retention(text: &str, retention_hours: u64) -> bool {
    let limit = Utc::now().naive_utc() - Duration::hours(retention_hours.min(100_000) as i64);
    parse_time(text).is_some_and(|t| t >= limit)
}

fn files_skipped(n: usize) -> String {
    format!("{n} file{} skipped", if n == 1 { "" } else { "s" })
}

/// The times a transfer gives a row
fn transfer_times(transfer: Option<&Transfer>) -> (Option<String>, Option<String>) {
    (
        transfer.and_then(|t| normalize(&t.created_at)),
        transfer
            .and_then(|t| t.finished_at.as_deref())
            .and_then(normalize),
    )
}

fn earliest_queued(files: &[&PipelineFile]) -> Option<String> {
    files.iter().filter_map(|f| f.recorded_at.clone()).min()
}

fn last_download(files: &[&PipelineFile]) -> Option<String> {
    files.iter().filter_map(|f| f.downloaded_at.clone()).max()
}

/// The row for a transfer item that left the active list, from the files it has now
fn departed_row(
    transfer_id: i64,
    seen: &Seen,
    transfer: Option<&Transfer>,
    files: &[&PipelineFile],
) -> Option<FinishedRow> {
    if files
        .iter()
        .any(|f| matches!(f.state, FileState::Queued(_)))
    {
        return None;
    }
    let done = files.iter().filter(|f| f.state == FileState::Done).count();
    let skipped = files.len() - done;
    let (outcome, reason) = if files.is_empty() {
        // Gone from Put.io in error: nothing can be retried. Gone otherwise: the
        // operator cancelled or cleared it, which is not a result.
        if seen.state != State::Error {
            return None;
        }
        (
            FinishedOutcome::Failed,
            Some(
                seen.reason
                    .clone()
                    .unwrap_or_else(|| "Put.io reported an error".to_string()),
            ),
        )
    } else if done == 0 {
        (
            FinishedOutcome::Failed,
            Some("nothing downloaded".to_string()),
        )
    } else {
        (
            FinishedOutcome::Done,
            (skipped > 0).then(|| files_skipped(skipped)),
        )
    };
    let (added_at, putio_done_at) = transfer_times(transfer);
    let done_at = match outcome {
        FinishedOutcome::Done => last_download(files),
        FinishedOutcome::Failed => None,
    }
    .unwrap_or_else(now_text);
    Some(FinishedRow {
        key: format!("transfer:{transfer_id}"),
        name: seen.name.clone(),
        size: seen.size,
        outcome,
        reason,
        added_at,
        putio_done_at,
        queued_at: earliest_queued(files),
        done_at,
    })
}

/// `.ext` of a file name, or the name when it has none
fn extension_of(name: &str) -> String {
    name.rsplit_once('.').map_or_else(
        || name.to_string(),
        |(_, e)| format!(".{}", e.to_lowercase()),
    )
}

impl DownloadManager {
    /// Write the Recently finished rows due after a pipeline tick, then prune rows older
    /// than `retention_hours` (feature 039, R6, R12). Returns how many rows were written.
    ///
    /// `pipeline` is the one this tick built. The previous tick's active transfer items
    /// are kept in memory; with none (the first tick after a restart) only recent
    /// completions and fully blocked transfers are written.
    pub async fn record_finished(
        &self,
        pipeline: &Pipeline,
        retention_hours: u64,
    ) -> DownloadResult<usize> {
        let db = self.database.as_ref().ok_or_else(|| {
            DownloadError::InternalError("Database required for Recently finished".to_string())
        })?;
        let internal = |e: putio_sync_db::DbError| DownloadError::InternalError(e.to_string());

        let current: SeenItems = pipeline
            .items
            .iter()
            .filter_map(|item| {
                Some((
                    item.transfer_id
                        .filter(|_| item.key.starts_with("transfer:"))?,
                    Seen {
                        name: item.name.clone(),
                        size: item.size,
                        state: item.state,
                        reason: item.reason.clone(),
                    },
                ))
            })
            .collect();
        let transfers = self.cached_transfers().await.unwrap_or_default();
        let transfer_of = |id: i64| transfers.iter().find(|t| t.id == id);

        db.finished()
            .prune_older_than(retention_hours)
            .await
            .map_err(internal)?;
        let existing: HashSet<String> = db
            .finished()
            .list()
            .await
            .map_err(internal)?
            .into_iter()
            .map(|r| r.key)
            .collect();

        let previous = self.finish_seen.lock().await.clone();
        let departed: Vec<(i64, &Seen)> = previous
            .as_ref()
            .map(|p| {
                p.iter()
                    .filter(|(id, _)| !current.contains_key(id))
                    .map(|(id, seen)| (*id, seen))
                    .collect()
            })
            .unwrap_or_default();

        // Finished on Put.io within the retention period, with nothing active and no row
        // yet: they may have had every file blocked
        let blocked_candidates: Vec<&Transfer> = transfers
            .iter()
            .filter(|t| {
                !current.contains_key(&t.id)
                    && !t.status.eq_ignore_ascii_case("ERROR")
                    && !existing.contains(&format!("transfer:{}", t.id))
                    && t.finished_at
                        .as_deref()
                        .is_some_and(|f| within_retention(f, retention_hours))
            })
            .collect();

        let mut ids: Vec<i64> = departed.iter().map(|(id, _)| *id).collect();
        ids.extend(blocked_candidates.iter().map(|t| t.id));
        let all_files = db
            .pipeline_files(&ids, retention_hours)
            .await
            .map_err(internal)?;
        let mut by_transfer: HashMap<i64, Vec<&PipelineFile>> = HashMap::new();
        for file in &all_files {
            if let Some(id) = file.transfer_id {
                by_transfer.entry(id).or_default().push(file);
            }
        }
        let files_of = |id: i64| by_transfer.get(&id).cloned().unwrap_or_default();

        let mut rows: Vec<FinishedRow> = departed
            .iter()
            .filter_map(|(id, seen)| departed_row(*id, seen, transfer_of(*id), &files_of(*id)))
            .collect();

        // First tick after a restart: items that finished within the retention period
        // and have no row yet
        if previous.is_none() {
            for (id, files) in &by_transfer {
                let key = format!("transfer:{id}");
                let done: Vec<&&PipelineFile> = files
                    .iter()
                    .filter(|f| f.state == FileState::Done)
                    .collect();
                let waiting = files
                    .iter()
                    .any(|f| matches!(f.state, FileState::Queued(_)));
                if current.contains_key(id) || existing.contains(&key) || waiting || done.is_empty()
                {
                    continue;
                }
                let transfer = transfer_of(*id);
                let (added_at, putio_done_at) = transfer_times(transfer);
                let skipped = files.len() - done.len();
                rows.push(FinishedRow {
                    key,
                    name: transfer
                        .map(|t| t.name.clone())
                        .unwrap_or_else(|| done[0].filename.clone()),
                    size: done.iter().map(|f| f.size).sum(),
                    outcome: FinishedOutcome::Done,
                    reason: (skipped > 0).then(|| files_skipped(skipped)),
                    added_at,
                    putio_done_at,
                    queued_at: earliest_queued(files),
                    done_at: last_download(files).unwrap_or_else(now_text),
                });
            }
        }

        // Every file of a finished transfer blocked by a rule: nothing to download
        let candidates: Vec<&Transfer> = blocked_candidates
            .into_iter()
            .filter(|t| !by_transfer.contains_key(&t.id))
            .collect();
        if !candidates.is_empty() {
            let decisions = db.rule_decisions().recent(5000).await.map_err(internal)?;
            for transfer in candidates {
                let mut extensions: Vec<String> = decisions
                    .iter()
                    .filter(|d| {
                        d.decision == "blocked"
                            && (Some(d.file_id) == transfer.file_id
                                || d.path == transfer.name
                                || d.path.starts_with(&format!("{}/", transfer.name)))
                    })
                    .map(|d| extension_of(&d.name))
                    .collect();
                if extensions.is_empty() {
                    continue;
                }
                extensions.sort();
                extensions.dedup();
                let (added_at, putio_done_at) = transfer_times(Some(transfer));
                rows.push(FinishedRow {
                    key: format!("transfer:{}", transfer.id),
                    name: transfer.name.clone(),
                    size: transfer.size.unwrap_or(0),
                    outcome: FinishedOutcome::Failed,
                    reason: Some(format!(
                        "nothing to download (blocked: {})",
                        extensions.join(", ")
                    )),
                    done_at: putio_done_at.clone().unwrap_or_else(now_text),
                    added_at,
                    putio_done_at,
                    queued_at: None,
                });
            }
        }

        for row in &rows {
            db.finished().upsert(row).await.map_err(internal)?;
            info!(
                key = %row.key,
                name = %row.name,
                outcome = row.outcome.as_str(),
                "Recorded a finished item"
            );
        }
        *self.finish_seen.lock().await = Some(current);
        Ok(rows.len())
    }
}
