//! Watch-folder staging (feature 038, R5): a watch-folder download whose steps change files
//! is built in a hidden `.putio-sync-staging` folder next to where it will live, then moved
//! into place with one `rename(2)`, so Radarr and Sonarr never see it half done.

use std::path::{Path, PathBuf};

use putio_sync_core::config::StagingMode;
use putio_sync_db::models::Origin;
use putio_sync_download::{PathRedirect, RedirectFuture};
use tracing::{info, warn};

use crate::store::ItemRow;
use crate::{locate, Pipeline, Settings};

/// The hidden folder that holds items while their steps run
pub const STAGING_DIR: &str = ".putio-sync-staging";

/// `relative_dir` with the staging folder put in front of the item named `name`:
/// `tv/Show` with item `Show` becomes `tv/.putio-sync-staging/Show`, and a single file
/// `movies` becomes `movies/.putio-sync-staging`
pub fn staged_dir(relative_dir: &str, name: &str) -> String {
    let parts: Vec<&str> = relative_dir.split('/').filter(|p| !p.is_empty()).collect();
    let at = parts.iter().position(|p| *p == name).unwrap_or(parts.len());
    let mut out: Vec<&str> = parts[..at].to_vec();
    out.push(STAGING_DIR);
    out.extend_from_slice(&parts[at..]);
    out.join("/")
}

/// Where a staged item goes: next to its staging folder, as `name` or, when that exists,
/// `name (2)`, `name (3)` and on
pub fn final_path(staged: &Path) -> Option<PathBuf> {
    let name = staged.file_name()?.to_string_lossy().into_owned();
    let parent = staged.parent()?;
    if parent.file_name()? != STAGING_DIR {
        return None;
    }
    let home = parent.parent()?;
    let mut candidate = home.join(&name);
    let mut n = 2;
    while candidate.exists() {
        candidate = home.join(format!("{name} ({n})"));
        n += 1;
    }
    Some(candidate)
}

/// Whether every staging folder under `target` (at the top or one category down) is on the
/// same filesystem as the folder it sits in, so the final rename is atomic
pub fn check_same_device(target: &Path) -> Result<(), String> {
    let mut homes = vec![target.to_path_buf()];
    if let Ok(entries) = std::fs::read_dir(target) {
        homes.extend(
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir() && p.file_name().is_some_and(|n| n != STAGING_DIR)),
        );
    }
    for home in homes {
        let staging = home.join(STAGING_DIR);
        let (Ok(a), Ok(b)) = (std::fs::metadata(&staging), std::fs::metadata(&home)) else {
            continue;
        };
        if !same_device(&a, &b) {
            return Err(format!(
                "{} is on another filesystem than {}, so an item cannot be moved into place in one rename",
                staging.display(),
                home.display()
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn same_device(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    a.dev() == b.dev()
}

/// Windows has no stable volume id in `std`. A staging folder inside its home folder is
/// only on another volume through a mount point, so it counts as the same.
#[cfg(not(unix))]
fn same_device(_: &std::fs::Metadata, _: &std::fs::Metadata) -> bool {
    true
}

impl Settings {
    /// Whether watch-folder downloads are staged now
    pub(crate) fn staging_on(&self) -> bool {
        match self.pipeline.staging {
            StagingMode::Off => false,
            StagingMode::On => true,
            // Every pipeline hook changes files, so any enabled one counts
            StagingMode::Auto => {
                self.hooks.iter().any(|h| h.enabled)
                    || self.subtitles.is_configured()
                    || (self.translation.is_configured() && self.subtitles.is_configured())
            }
        }
    }
}

impl PathRedirect for Pipeline {
    fn redirect<'a>(&'a self, file_id: i64, relative_path: &'a str) -> RedirectFuture<'a> {
        Box::pin(async move {
            if !self.inner.settings.read().unwrap().staging_on()
                || self
                    .inner
                    .staging_off
                    .load(std::sync::atomic::Ordering::Relaxed)
            {
                return None;
            }
            let source = self.inner.db.sources().get(file_id).await.ok()??;
            if source.origin != Origin::Watch {
                return None;
            }
            let key = format!("transfer:{}", source.transfer_id?);
            // Without an item row yet there is no name to stage by: the file lands in place
            let item = self.inner.store.item(&key).await.ok()??;
            Some(staged_dir(relative_path, &item.name))
        })
    }
}

impl Pipeline {
    /// Check the staging folders at startup; on a mismatch staging is turned off
    pub(crate) fn check_staging(&self) {
        let target = self.inner.settings.read().unwrap().target.clone();
        if let Err(e) = check_same_device(&target) {
            warn!(error = %e, "Watch-folder staging is off");
            self.inner
                .staging_off
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Move a staged item to its final place; nothing to do when it was not staged
    pub(crate) async fn finish_staging(&self, item: &ItemRow) -> Result<(), String> {
        let staged = PathBuf::from(&item.dir);
        let Some(to) = final_path(&staged) else {
            return Ok(());
        };
        if !staged.exists() {
            return Ok(());
        }
        std::fs::rename(&staged, &to).map_err(|e| format!("rename failed: {e}"))?;
        let retention = self.inner.settings.read().unwrap().retention_hours;
        // Downloads still point into the staging folder
        for (file_id, path) in locate::local_paths(&self.inner.db, &item.key, retention).await {
            if let Ok(rest) = path.strip_prefix(&staged) {
                let new = to.join(rest);
                let _ = self
                    .inner
                    .db
                    .downloads()
                    .set_local_path(file_id, &new.to_string_lossy())
                    .await;
            } else if path == staged {
                let _ = self
                    .inner
                    .db
                    .downloads()
                    .set_local_path(file_id, &to.to_string_lossy())
                    .await;
            }
        }
        // Where each saved subtitle came from follows the rename (feature 026)
        let _ = self
            .inner
            .store
            .rekey_provenance(&staged.to_string_lossy(), &to.to_string_lossy())
            .await;
        let _ = self
            .inner
            .store
            .set_dir(&item.key, &to.to_string_lossy())
            .await;
        info!(key = %item.key, to = %to.display(), "Staged item moved into place");
        Ok(())
    }
}
