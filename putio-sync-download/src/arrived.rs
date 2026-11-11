//! Steps that run once for each file stream mode keeps on Put.io (feature 022).
//!
//! The sync pass records every newly streamed file in `stream_files` with the name of
//! each registered step, then runs a bounded number of pending steps. A step that asks
//! for a retry keeps its place and stops that file's later steps until the next pass.
//! 022 has one step, [`Mp4Step`]; organising TV shows and movies on Put.io (023, 024)
//! add theirs to the same list.

use crate::manager::DownloadConfig;
use futures_util::future::BoxFuture;
use putio_sync_client::PutioApiClient;
use putio_sync_db::Database;
use putio_sync_telemetry::{self as telemetry, Mp4Request};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Step runs per sync pass, oldest files first (FR-012)
pub const STEPS_PER_PASS: usize = 20;

/// A file stream mode keeps on Put.io
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArrivedFile {
    pub file_id: i64,
    pub name: String,
    /// Put.io folder path relative to the synced root
    pub folder: String,
    pub size: i64,
}

/// What a step did
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepResult {
    /// Finished, or nothing to do: never run again for this file
    Done,
    /// Try again next pass, with why
    Retry(String),
}

/// Something to do once with each newly streamed file
pub trait ArrivedStep: Send + Sync {
    /// Stable name, stored in `stream_files.pending`; no spaces
    fn name(&self) -> &'static str;
    /// Whether the step is on under `config`; read each pass, so a settings change applies
    /// without a restart. Pending runs of a step that is off are dropped.
    fn enabled(&self, _config: &DownloadConfig) -> bool {
        true
    }
    fn run<'a>(&'a self, file: &'a ArrivedFile) -> BoxFuture<'a, StepResult>;
}

/// Run up to `limit` pending steps, oldest files first. Returns how many ran.
pub async fn run_pending(db: &Database, steps: &[Arc<dyn ArrivedStep>], limit: usize) -> usize {
    if steps.is_empty() || limit == 0 {
        return 0;
    }
    let repo = db.stream_files();
    let files = match repo.pending(limit as i64).await {
        Ok(files) => files,
        Err(e) => {
            warn!(error = %e, "Failed to list streamed files with steps to run");
            return 0;
        }
    };
    let mut ran = 0;
    'files: for row in files {
        let file = ArrivedFile {
            file_id: row.file_id,
            name: row.filename,
            folder: row.folder_path,
            size: row.size,
        };
        for name in &row.pending {
            if ran >= limit {
                break 'files;
            }
            let Some(step) = steps.iter().find(|s| s.name() == name) else {
                // A step no longer registered or turned off: drop it
                debug!(file_id = file.file_id, step = %name, "Step not registered; dropped");
                if let Err(e) = repo.finish_step(file.file_id, name).await {
                    warn!(file_id = file.file_id, error = %e, "Failed to update streamed file");
                }
                continue;
            };
            ran += 1;
            match step.run(&file).await {
                StepResult::Done => {
                    if let Err(e) = repo.finish_step(file.file_id, name).await {
                        warn!(file_id = file.file_id, error = %e, "Failed to update streamed file");
                    }
                }
                StepResult::Retry(reason) => {
                    warn!(file_id = file.file_id, step = %name, reason = %reason, "Step failed; retried next sync");
                    if let Err(e) = repo.retry_step(file.file_id, &reason).await {
                        warn!(file_id = file.file_id, error = %e, "Failed to update streamed file");
                    }
                    continue 'files;
                }
            }
        }
    }
    ran
}

/// Video extensions Put.io can convert; `.mp4` and `.m4v` already play everywhere
const CONVERTIBLE: &[&str] = &[
    "mkv", "avi", "wmv", "mov", "flv", "webm", "ts", "m2ts", "mts", "mpg", "mpeg", "vob", "ogv",
    "3gp", "divx", "xvid", "asf", "rm", "rmvb",
];

/// Asks Put.io for an MP4 version of each streamed video that isn't MP4, so players that
/// can't play the original get one (`download.stream_mp4`)
pub struct Mp4Step {
    client: Arc<PutioApiClient>,
}

impl Mp4Step {
    pub fn new(client: Arc<PutioApiClient>) -> Self {
        Self { client }
    }

    /// Whether `name` is a video Put.io should convert
    pub fn wants(name: &str) -> bool {
        name.rsplit_once('.')
            .map(|(_, ext)| CONVERTIBLE.contains(&ext.to_ascii_lowercase().as_str()))
            .unwrap_or(false)
    }

    async fn request(&self, file: &ArrivedFile) -> StepResult {
        if !Self::wants(&file.name) {
            return StepResult::Done;
        }
        let status = match self.client.mp4_status(file.file_id).await {
            Ok(status) => status,
            Err(e) if e.is_not_found() => return gone(file),
            Err(e) => {
                telemetry::sync::stream_mp4_request(Mp4Request::Failed);
                return StepResult::Retry(format!("MP4 status: {e}"));
            }
        };
        if status.is_ready() || status.in_progress() {
            telemetry::sync::stream_mp4_request(Mp4Request::Ready);
            debug!(file_id = file.file_id, status = %status.status, "MP4 version exists or is on its way");
            return StepResult::Done;
        }
        match self.client.request_mp4(file.file_id).await {
            Ok(()) => {
                telemetry::sync::stream_mp4_request(Mp4Request::Requested);
                info!(file_id = file.file_id, name = %file.name, "Asked Put.io for an MP4 version");
                StepResult::Done
            }
            Err(e) if e.is_not_found() => gone(file),
            Err(e) => {
                telemetry::sync::stream_mp4_request(Mp4Request::Failed);
                StepResult::Retry(format!("MP4 request: {e}"))
            }
        }
    }
}

fn gone(file: &ArrivedFile) -> StepResult {
    telemetry::sync::stream_mp4_request(Mp4Request::Gone);
    info!(file_id = file.file_id, name = %file.name, "File gone from Put.io; no MP4 version asked for");
    StepResult::Done
}

impl ArrivedStep for Mp4Step {
    fn name(&self) -> &'static str {
        "mp4"
    }

    fn enabled(&self, config: &DownloadConfig) -> bool {
        config.stream_mp4
    }

    fn run<'a>(&'a self, file: &'a ArrivedFile) -> BoxFuture<'a, StepResult> {
        Box::pin(self.request(file))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_videos_that_are_not_mp4_are_converted() {
        assert!(Mp4Step::wants("Show.S01E01.mkv"));
        assert!(Mp4Step::wants("film.AVI"));
        assert!(!Mp4Step::wants("film.mp4"));
        assert!(!Mp4Step::wants("film.m4v"));
        assert!(!Mp4Step::wants("film.srt"));
        assert!(!Mp4Step::wants("README"));
    }
}
