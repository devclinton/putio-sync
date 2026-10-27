//! Fixes: each one detects again and acts only on the requested items that still qualify

use crate::checks::{self, sync, Ctx, STALE_AGE};
use crate::report::{FailedItem, FixId, FixRequest, Item, SkippedItem, Status};
use putio_sync_core::cleanup::{CleanupError, CleanupTrigger};
use putio_sync_db::models::QueueStatus;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use thiserror::Error;

/// Most items one apply request may name
pub const MAX_FIX_ITEMS: usize = 1000;

/// Why a fix request was refused before anything was done
#[derive(Debug, Error, PartialEq, Eq)]
pub enum FixError {
    #[error("unknown fix {0:?}")]
    UnknownFix(String),
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Conflict(String),
}

/// What an apply did, before the check is run again
#[derive(Debug, Default)]
pub(crate) struct Applied {
    pub done: Vec<Item>,
    pub skipped: Vec<SkippedItem>,
    pub failed: Vec<FailedItem>,
}

impl Applied {
    fn skip(&mut self, key: &str, reason: impl Into<String>) {
        self.skipped.push(SkippedItem {
            item: key.to_string(),
            reason: reason.into(),
        });
    }

    fn fail(&mut self, key: &str, error: impl ToString) {
        self.failed.push(FailedItem {
            item: key.to_string(),
            error: error.to_string(),
        });
    }
}

/// The request's keys, deduplicated in order, or why the request is refused
pub(crate) fn validate(fix: FixId, req: &FixRequest) -> Result<Vec<String>, FixError> {
    if req.items.is_empty() {
        return Err(FixError::Invalid("No items given.".into()));
    }
    if req.items.len() > MAX_FIX_ITEMS {
        return Err(FixError::Invalid(format!(
            "At most {MAX_FIX_ITEMS} items per request."
        )));
    }
    if fix.destructive() && !req.confirm {
        return Err(FixError::Invalid(format!(
            "{} deletes data, so it needs \"confirm\": true.",
            fix.as_str()
        )));
    }
    let mut seen = HashSet::new();
    Ok(req
        .items
        .iter()
        .filter(|k| seen.insert(k.as_str()))
        .cloned()
        .collect())
}

const NO_LONGER: &str = "No longer needs this fix.";

/// Apply `fix` to `keys`, which [`validate`] accepted
pub(crate) async fn apply(ctx: &Ctx, fix: FixId, keys: &[String]) -> Result<Applied, FixError> {
    match fix {
        FixId::EmptyFoldersClean => clean_empty_folders(ctx, keys).await,
        FixId::OrphanPartialsDelete => Ok(delete_partials(ctx, keys).await),
        _ => Ok(per_item(ctx, fix, keys).await),
    }
}

/// Fixes that act on one Put.io transfer or queue row at a time
async fn per_item(ctx: &Ctx, fix: FixId, keys: &[String]) -> Applied {
    let mut out = Applied::default();
    let finding = checks::finding(ctx, fix.check()).await;
    if finding.status == Status::Skipped {
        for key in keys {
            out.skip(key, &finding.summary);
        }
        return out;
    }
    let current: HashMap<String, Item> = finding
        .items_for(fix)
        .into_iter()
        .map(|i| (i.key.clone(), i))
        .collect();
    for key in keys {
        let Some(item) = current.get(key) else {
            out.skip(key, NO_LONGER);
            continue;
        };
        let Ok(id) = key.parse::<i64>() else {
            out.skip(key, NO_LONGER);
            continue;
        };
        let result = match fix {
            FixId::TransfersRetry => ctx
                .client
                .retry_transfer(id)
                .await
                .map_err(|e| e.to_string()),
            FixId::TransfersReadd => ctx
                .client
                .readd_transfer(id)
                .await
                .map(|_| ())
                .map_err(|e| e.to_string()),
            FixId::DeadQueueRowsRemove => match &ctx.db {
                Some(db) => db.queue().remove(id).await.map_err(|e| e.to_string()),
                None => Err("No database is open.".into()),
            },
            FixId::FailingDownloadsRequeue => match &ctx.db {
                Some(db) => db
                    .queue()
                    .update_status(id, QueueStatus::Pending)
                    .await
                    .map_err(|e| e.to_string()),
                None => Err("No database is open.".into()),
            },
            FixId::EmptyFoldersClean | FixId::OrphanPartialsDelete => unreachable!(),
        };
        match result {
            Ok(()) => out.done.push(item.clone()),
            Err(e) => out.fail(key, e),
        }
    }
    out
}

/// A fresh dry run must not remove anything outside `keys`; then the real cleanup runs
async fn clean_empty_folders(ctx: &Ctx, keys: &[String]) -> Result<Applied, FixError> {
    let busy = || FixError::Conflict("An empty-folder cleanup is already running.".into());
    let dry = match ctx.cleanup.run(CleanupTrigger::Manual, true).await {
        Ok(report) => report,
        Err(CleanupError::AlreadyRunning) => return Err(busy()),
        Err(e) => return Err(FixError::Conflict(format!("The dry run failed: {e}"))),
    };
    let wanted: HashSet<&str> = keys.iter().map(String::as_str).collect();
    if dry
        .removed
        .iter()
        .any(|f| !wanted.contains(f.id.to_string().as_str()))
    {
        return Err(FixError::Conflict(
            "The list of empty folders changed; preview again.".into(),
        ));
    }
    let report = match ctx.cleanup.run(CleanupTrigger::Manual, false).await {
        Ok(report) => report,
        Err(CleanupError::AlreadyRunning) => return Err(busy()),
        Err(e) => return Err(FixError::Conflict(format!("The cleanup failed: {e}"))),
    };
    let mut out = Applied::default();
    let mut handled = HashSet::new();
    for f in &report.removed {
        let key = f.id.to_string();
        handled.insert(key.clone());
        out.done
            .push(Item::new(&key, &f.path).fix(FixId::EmptyFoldersClean));
    }
    for f in &report.failures {
        let key = f.id.to_string();
        if wanted.contains(key.as_str()) && handled.insert(key.clone()) {
            out.fail(&key, &f.error);
        }
    }
    for key in keys.iter().filter(|k| !handled.contains(*k)) {
        out.skip(key, NO_LONGER);
    }
    Ok(out)
}

/// Delete partial files that are still unclaimed, unchanged and stale, and inside the
/// download folder
async fn delete_partials(ctx: &Ctx, keys: &[String]) -> Applied {
    let mut out = Applied::default();
    let target = &ctx.config.download.target_directory;
    let Some(db) = &ctx.db else {
        for key in keys {
            out.skip(key, "No database is open.");
        }
        return out;
    };
    let orphans = match sync::orphans(db, target).await {
        Ok(o) => o,
        Err(e) => {
            for key in keys {
                out.fail(key, &e);
            }
            return out;
        }
    };
    let by_key: HashMap<&str, &sync::Partial> =
        orphans.iter().map(|p| (p.key.as_str(), p)).collect();
    let Ok(root) = target.canonicalize() else {
        for key in keys {
            out.skip(key, "The download folder does not exist.");
        }
        return out;
    };
    for key in keys {
        let Some(p) = by_key.get(key.as_str()) else {
            out.skip(key, NO_LONGER);
            continue;
        };
        if !inside(&root, &p.path) {
            out.skip(key, "Outside the download folder.");
            continue;
        }
        // Last look right before the delete: still there, same size, still stale
        let still = std::fs::symlink_metadata(&p.path).ok().filter(|m| {
            m.is_file()
                && m.len() == p.size
                && m.modified()
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .is_some_and(|a| a >= STALE_AGE)
        });
        if still.is_none() {
            out.skip(key, "The file changed since the preview.");
            continue;
        }
        match std::fs::remove_file(&p.path) {
            Ok(()) => out.done.push(
                Item::new(key, key)
                    .detail(crate::checks::bytes(p.size))
                    .fix(FixId::OrphanPartialsDelete),
            ),
            Err(e) => out.fail(key, e),
        }
    }
    out
}

/// Whether `path`'s real parent folder sits under `root` (already canonical)
fn inside(root: &Path, path: &Path) -> bool {
    path.parent()
        .and_then(|d| d.canonicalize().ok())
        .is_some_and(|d| d.starts_with(root))
        && path.file_name().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(items: &[&str], confirm: bool) -> FixRequest {
        FixRequest {
            items: items.iter().map(|s| s.to_string()).collect(),
            confirm,
        }
    }

    #[test]
    fn validation() {
        assert!(matches!(
            validate(FixId::TransfersRetry, &req(&[], false)),
            Err(FixError::Invalid(_))
        ));
        assert!(matches!(
            validate(FixId::DeadQueueRowsRemove, &req(&["1"], false)),
            Err(FixError::Invalid(_))
        ));
        assert_eq!(
            validate(FixId::DeadQueueRowsRemove, &req(&["1", "2", "1"], true)).unwrap(),
            vec!["1", "2"]
        );
        assert!(validate(FixId::TransfersRetry, &req(&["1"], false)).is_ok());
        let many: Vec<String> = (0..=MAX_FIX_ITEMS).map(|i| i.to_string()).collect();
        let r = FixRequest {
            items: many,
            confirm: true,
        };
        assert!(validate(FixId::TransfersRetry, &r).is_err());
    }

    #[test]
    fn inside_rejects_escapes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir(root.join("a")).unwrap();
        assert!(inside(&root, &root.join("a/.x.downloading")));
        assert!(!inside(&root, &root.join("../.x.downloading")));
        assert!(!inside(&root, Path::new("/etc/.x.downloading")));
    }
}
