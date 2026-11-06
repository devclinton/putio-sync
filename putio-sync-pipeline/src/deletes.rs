//! Held-back Put.io deletes (feature 025, research R10).
//!
//! With "delete after download" on, a downloaded file would be deleted from Put.io at once,
//! and Put.io's subtitles go with it. The download crate asks [`RemoteDeleteGate::defer`]
//! first; an answer holds the delete in `pipeline_deferred_deletes`. Each tick the daemon
//! asks [`Pipeline::releasable_deletes`], deletes those files through the download manager
//! and tells [`Pipeline::mark_deleted`].

use std::time::{SystemTime, UNIX_EPOCH};

use putio_sync_download::{DeferFuture, RemoteDeleteGate};
use tracing::warn;

use crate::model::Stage;
use crate::subtitles::is_video;
use crate::Pipeline;

/// A delete that has waited this long is released whatever the pipeline is doing, so a stuck
/// item cannot keep a file on Put.io forever
pub const FAILSAFE_SECS: i64 = 24 * 60 * 60;

const SUBTITLE_EXTS: [&str; 5] = ["srt", "ass", "ssa", "vtt", "sub"];

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Whether a file's name is one a subtitle step may need Put.io for
fn waits(relative_path: &str) -> bool {
    is_video(relative_path)
        || std::path::Path::new(relative_path)
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| SUBTITLE_EXTS.contains(&e.to_lowercase().as_str()))
}

impl RemoteDeleteGate for Pipeline {
    fn defer<'a>(
        &'a self,
        file_id: i64,
        relative_path: &'a str,
        filename: &'a str,
    ) -> DeferFuture<'a> {
        Box::pin(async move {
            let cfg = self.settings().subtitles;
            if !cfg.is_configured() {
                return None;
            }
            // `relative_path` is the file's folder; the category is its first folder
            let mut parts = relative_path.split('/').filter(|p| !p.is_empty());
            let category = parts.next();
            if category.is_some_and(|c| cfg.excludes(c)) {
                return None;
            }
            let source = self.inner.db.sources().get(file_id).await.ok().flatten();
            let key = match source.and_then(|s| s.transfer_id) {
                Some(id) => format!("transfer:{id}"),
                None if !relative_path.is_empty() => format!("folder:{relative_path}"),
                None => format!("file:{file_id}"),
            };
            if !waits(filename) {
                return None;
            }
            Some(key)
        })
    }
}

impl Pipeline {
    /// The deferred deletes that may go now: their item ended or is past the Subtitles
    /// stage, subtitles were switched off since, or the 24 hour failsafe passed
    pub async fn releasable_deletes(&self) -> Vec<i64> {
        let rows = match self.inner.db.deferred_deletes().list().await {
            Ok(rows) => rows,
            Err(e) => {
                warn!(error = %e, "Could not read the held-back deletes");
                return Vec::new();
            }
        };
        let off = !self.settings().subtitles.is_configured();
        let now = now();
        let mut out = Vec::new();
        for row in rows {
            let release = off
                || now - row.created_at >= FAILSAFE_SECS
                || match self.inner.store.item(&row.item_key).await {
                    Ok(Some(item)) => item.state.is_terminal() || item.stage > Stage::Subtitles,
                    _ => false,
                };
            if release {
                out.push(row.file_id);
            }
        }
        out
    }

    /// The files were deleted from Put.io: forget the held-back rows
    pub async fn mark_deleted(&self, file_ids: &[i64]) {
        for id in file_ids {
            if let Err(e) = self.inner.db.deferred_deletes().delete(*id).await {
                warn!(file_id = id, error = %e, "Could not forget a held-back delete");
            }
        }
    }
}
