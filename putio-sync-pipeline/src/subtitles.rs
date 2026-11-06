//! The Subtitles stage (feature 025): a scan step that finds an item's videos and adds one
//! step per video, and the per-video step that fetches (or makes) the subtitles.
//!
//! The steps are ordinary pipeline steps, so retries, cancel, the gate, staging and the
//! timeline come with them. The engine in `putio-sync-subtitles` does the work for one video.

use std::path::{Path, PathBuf};
use std::time::Instant;

use putio_sync_core::log_redact;
use putio_sync_subtitles::engine::{self, EngineError, VideoRequest};
use tracing::{info, warn};

use crate::locate::{self, ItemFile};
use crate::model::{AttemptResult, SkipReason, Stage, StepKind, StepState};
use crate::plan::{PlannedStep, VIDEO_POSITION};
use crate::runner::{Ended, Outcome};
use crate::store::{ItemRow, StepRow};
use crate::Pipeline;

/// Files smaller than this are not subtitle targets (samples, extras)
pub const MIN_VIDEO_BYTES: u64 = 100 * 1024 * 1024;

/// Extensions of videos
const VIDEO_EXTS: [&str; 10] = [
    "mkv", "mp4", "m4v", "avi", "mov", "wmv", "ts", "m2ts", "webm", "mpg",
];

/// Whether a file name has a video extension
pub fn is_video(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.ends_with(".mpeg")
        || Path::new(&lower)
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| VIDEO_EXTS.contains(&e))
}

/// Whether the file or a folder above it (relative to the item) is named like a sample
pub fn is_sample(relative_path: &str) -> bool {
    relative_path.to_lowercase().contains("sample")
}

/// What a scan found
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Scan {
    /// Relative paths of the videos to fetch for, in a stable order
    pub videos: Vec<String>,
    /// Videos left out as samples or as too small
    pub skipped: usize,
}

/// Pick the subtitle targets among an item's files (research R4)
pub fn scan_files(files: &[ItemFile]) -> Scan {
    let mut scan = Scan::default();
    for f in files.iter().filter(|f| is_video(&f.relative_path)) {
        if is_sample(&f.relative_path) || f.size < MIN_VIDEO_BYTES {
            scan.skipped += 1;
        } else {
            scan.videos.push(f.relative_path.clone());
        }
    }
    scan.videos.sort();
    scan
}

/// Where a step's video is: the item's file itself for a single-file item, else under the folder
fn video_path(item: &ItemRow, relative: &str) -> PathBuf {
    let dir = PathBuf::from(&item.dir);
    if dir.is_file() {
        dir
    } else {
        dir.join(relative)
    }
}

impl Pipeline {
    /// Whether subtitles are off for this item now (settings changed since the plan)
    fn subtitles_off(&self, item: &ItemRow) -> Option<SkipReason> {
        let cfg = self.settings().subtitles;
        if !cfg.is_configured() {
            Some(SkipReason::NotConfigured)
        } else if item.category.as_deref().is_some_and(|c| cfg.excludes(c)) {
            Some(SkipReason::Category)
        } else {
            None
        }
    }

    async fn skip_step(&self, step: &StepRow, why: SkipReason) -> Outcome {
        let _ = self
            .inner
            .store
            .set_step(step.id, StepState::Skipped, Some(why), None)
            .await;
        Outcome::Continue
    }

    /// The scan step: find the videos and add a step for each
    pub(crate) async fn run_subtitle_scan(&self, item: &ItemRow, step: &StepRow) -> Outcome {
        if let Some(why) = self.subtitles_off(item) {
            return self.skip_step(step, why).await;
        }
        let store = &self.inner.store;
        let Ok((attempt_id, n)) = store.start_attempt(step.id).await else {
            return Outcome::Stopped;
        };
        let started = Instant::now();
        let files = self.item_files(item).await;
        let scan = scan_files(&files);
        let message = match (scan.videos.len(), scan.skipped) {
            (v, 0) => format!("{v} videos"),
            (v, s) => format!("{v} videos, {s} sample or small skipped"),
        };
        if scan.videos.is_empty() {
            let _ = store
                .end_attempt(attempt_id, AttemptResult::Skip, None, &message)
                .await;
            putio_sync_telemetry::steps::step(
                step.stage.as_str(),
                "subtitle_scan",
                "skip",
                started.elapsed(),
            );
            return self.skip_step(step, SkipReason::NoVideo).await;
        }
        let planned: Vec<PlannedStep> = scan
            .videos
            .iter()
            .map(|v| PlannedStep {
                stage: Stage::Subtitles,
                branch: None,
                position: VIDEO_POSITION,
                kind: StepKind::Subtitles,
                name: v.clone(),
                required: false,
                changes_files: true,
                state: StepState::Waiting,
                skip_reason: None,
            })
            .collect();
        if let Err(e) = store.insert_steps(&item.key, &planned).await {
            return self
                .conclude(
                    item,
                    step,
                    Ended {
                        attempt_id,
                        n,
                        kind: "subtitle_scan",
                        result: AttemptResult::Failed,
                        code: None,
                        output: e.to_string(),
                        error: Some(e.to_string()),
                        took: started.elapsed(),
                    },
                )
                .await;
        }
        self.conclude(
            item,
            step,
            Ended {
                attempt_id,
                n,
                kind: "subtitle_scan",
                result: AttemptResult::Done,
                code: None,
                output: message,
                error: None,
                took: started.elapsed(),
            },
        )
        .await
    }

    /// The files of an item, with their Put.io ids
    async fn item_files(&self, item: &ItemRow) -> Vec<ItemFile> {
        if item.dir.is_empty() {
            return Vec::new();
        }
        let retention = self.settings().retention_hours;
        let ids = locate::local_paths(&self.inner.db, &item.key, retention).await;
        let dir = PathBuf::from(&item.dir);
        tokio::task::spawn_blocking(move || locate::list_files(&dir, &ids))
            .await
            .unwrap_or_default()
    }

    /// The Put.io id of the file at `path`
    async fn putio_id_of(&self, item: &ItemRow, path: &Path) -> Option<i64> {
        let retention = self.settings().retention_hours;
        let ids = locate::local_paths(&self.inner.db, &item.key, retention).await;
        if let Some((id, _)) = ids.iter().find(|(_, p)| p == path) {
            return Some(*id);
        }
        self.inner
            .db
            .downloads()
            .find_by_local_path(&path.to_string_lossy())
            .await
            .ok()
            .flatten()
            .map(|d| d.file_id)
    }

    /// One video's step
    pub(crate) async fn run_subtitles(&self, item: &ItemRow, step: &StepRow) -> Outcome {
        if let Some(why) = self.subtitles_off(item) {
            return self.skip_step(step, why).await;
        }
        let Some(source) = self.inner.source.read().unwrap().clone() else {
            return self.skip_step(step, SkipReason::NotConfigured).await;
        };
        let store = &self.inner.store;
        let Ok((attempt_id, n)) = store.start_attempt(step.id).await else {
            return Outcome::Stopped;
        };
        let cfg = self.settings().subtitles;
        let languages = cfg.wanted();
        let video = video_path(item, &step.name);
        let cancel = self.cancel_of(&item.key);
        let started = Instant::now();
        let run = async {
            if !video.is_file() {
                return Err(EngineError::Failed(format!(
                    "the video is not there: {}",
                    step.name
                )));
            }
            let file_id = self.putio_id_of(item, &video).await;
            engine::fetch_for_video(
                source.as_ref(),
                &VideoRequest {
                    video: &video,
                    file_id,
                    languages: &languages,
                },
            )
            .await
        };
        let run = tokio::select! {
            run = run => Some(run),
            _ = cancel.notified() => None,
        };
        let took = started.elapsed();
        let Some(run) = run else {
            let _ = store
                .end_attempt(attempt_id, AttemptResult::Interrupted, None, "")
                .await;
            let _ = store
                .set_step(step.id, StepState::Interrupted, None, None)
                .await;
            putio_sync_telemetry::steps::step(
                step.stage.as_str(),
                "subtitles",
                "interrupted",
                took,
            );
            return Outcome::Stopped;
        };
        let (result, output, error) = match run {
            Ok(outcome) => {
                let mut message = outcome.message();
                if !outcome.warnings.is_empty() {
                    message.push_str(&format!(" ({} passed over)", outcome.warnings.len()));
                }
                info!(key = %item.key, video = %step.name, result = %message, "Subtitles step done");
                (AttemptResult::Done, log_redact::redact(&message), None)
            }
            Err(EngineError::Failed(why)) => {
                let why = log_redact::redact(&why);
                warn!(key = %item.key, video = %step.name, error = %why, "Subtitles step failed");
                (AttemptResult::Failed, why.clone(), Some(why))
            }
        };
        self.conclude(
            item,
            step,
            Ended {
                attempt_id,
                n,
                kind: "subtitles",
                result,
                code: None,
                output,
                error,
                took,
            },
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, size: u64) -> ItemFile {
        ItemFile {
            relative_path: name.to_string(),
            size,
            putio_id: None,
        }
    }

    #[test]
    fn videos_are_picked_by_extension_size_and_sample_rules() {
        let big = MIN_VIDEO_BYTES;
        let scan = scan_files(&[
            file("S01/E02.mkv", big),
            file("S01/E01.MP4", big + 1),
            file("S01/E01.srt", 10),
            file("Sample/clip.mkv", big),
            file("S01/small.mkv", big - 1),
            file("S01/sample-x.mkv", big),
            file("notes.txt", big),
        ]);
        assert_eq!(scan.videos, ["S01/E01.MP4", "S01/E02.mkv"]);
        assert_eq!(scan.skipped, 3);
    }

    #[test]
    fn extensions() {
        for n in ["a.mkv", "a.M2TS", "a.mpeg", "a.webm", "a.ts"] {
            assert!(is_video(n), "{n}");
        }
        for n in ["a.srt", "a", "a.nfo", "mkv"] {
            assert!(!is_video(n), "{n}");
        }
    }
}
