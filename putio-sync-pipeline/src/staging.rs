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
    while candidate.symlink_metadata().is_ok() {
        candidate = home.join(format!("{name} ({n})"));
        n += 1;
    }
    Some(candidate)
}

/// Rename `from` to `to` without ever replacing something already at `to`: fails with
/// `AlreadyExists` instead. The check and the move are one step where the OS has it
/// (renameat2 on Linux, renamex_np on macOS); elsewhere, or on a filesystem without it,
/// they are two and a file created in between is still at risk.
pub fn rename_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
    {
        use std::os::unix::ffi::OsStrExt;
        let c = |p: &Path| std::ffi::CString::new(p.as_os_str().as_bytes());
        let (Ok(a), Ok(b)) = (c(from), c(to)) else {
            return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput));
        };
        #[cfg(any(target_os = "linux", target_os = "android"))]
        // SAFETY: both pointers are valid NUL-terminated strings for the call
        let rc = unsafe {
            libc::syscall(
                libc::SYS_renameat2,
                libc::AT_FDCWD,
                a.as_ptr(),
                libc::AT_FDCWD,
                b.as_ptr(),
                1u32, // RENAME_NOREPLACE
            ) as i64
        };
        #[cfg(target_os = "macos")]
        // SAFETY: both pointers are valid NUL-terminated strings for the call
        let rc = unsafe { libc::renamex_np(a.as_ptr(), b.as_ptr(), libc::RENAME_EXCL) as i64 };
        if rc == 0 {
            return Ok(());
        }
        let err = std::io::Error::last_os_error();
        // Not supported here (old kernel, or a filesystem without the flag): the two steps
        if !matches!(
            err.raw_os_error(),
            Some(libc::ENOSYS) | Some(libc::EINVAL) | Some(libc::ENOTSUP)
        ) {
            return Err(err);
        }
    }
    if to.symlink_metadata().is_ok() {
        return Err(std::io::ErrorKind::AlreadyExists.into());
    }
    std::fs::rename(from, to)
}

/// Move `staged` to its final place, under a free name; a name taken meanwhile gets the
/// next `(n)`. Returns the path it ended up at.
pub fn move_to_final(staged: &Path) -> Result<Option<PathBuf>, String> {
    for _ in 0..1000 {
        let Some(to) = final_path(staged) else {
            return Ok(None);
        };
        match rename_no_replace(staged, &to) {
            Ok(()) => return Ok(Some(to)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("rename failed: {e}")),
        }
    }
    Err("rename failed: no free name".to_string())
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
            if !self
                .inner
                .settings
                .read()
                .unwrap_or_else(|e| e.into_inner())
                .staging_on()
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
        let target = self
            .inner
            .settings
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .target
            .clone();
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
        if final_path(&staged).is_none() || !staged.exists() {
            return Ok(());
        }
        let Some(to) = move_to_final(&staged)? else {
            return Ok(());
        };
        let retention = self
            .inner
            .settings
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .retention_hours;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_never_replaces_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("new.mkv");
        let to = dir.path().join("old.mkv");
        std::fs::write(&from, b"new").unwrap();
        std::fs::write(&to, b"old").unwrap();
        let err = rename_no_replace(&from, &to).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&to).unwrap(), b"old");
        assert!(from.exists());

        let free = dir.path().join("free.mkv");
        rename_no_replace(&from, &free).unwrap();
        assert_eq!(std::fs::read(&free).unwrap(), b"new");
        assert!(!from.exists());
    }

    #[test]
    fn rename_does_not_replace_an_empty_directory() {
        let dir = tempfile::tempdir().unwrap();
        let from = dir.path().join("a");
        let to = dir.path().join("b");
        std::fs::create_dir(&from).unwrap();
        std::fs::create_dir(&to).unwrap();
        let err = rename_no_replace(&from, &to).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
    }

    #[test]
    fn finishing_picks_a_free_name_and_keeps_the_library_file() {
        let home = tempfile::tempdir().unwrap();
        let staging = home.path().join(STAGING_DIR);
        std::fs::create_dir(&staging).unwrap();
        let staged = staging.join("movie.mkv");
        std::fs::write(&staged, b"new").unwrap();
        std::fs::write(home.path().join("movie.mkv"), b"library").unwrap();
        std::fs::write(home.path().join("movie.mkv (2)"), b"library 2").unwrap();

        let to = move_to_final(&staged).unwrap().unwrap();
        assert_eq!(to, home.path().join("movie.mkv (3)"));
        assert_eq!(std::fs::read(&to).unwrap(), b"new");
        assert_eq!(
            std::fs::read(home.path().join("movie.mkv")).unwrap(),
            b"library"
        );
        assert!(!staged.exists());
    }

    #[test]
    fn a_path_outside_staging_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("x");
        std::fs::write(&f, b"x").unwrap();
        assert_eq!(move_to_final(&f).unwrap(), None);
        assert!(f.exists());
    }
}
