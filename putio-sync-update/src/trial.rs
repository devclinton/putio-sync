//! The first start of a new version, and the rollback when it fails (research R8).
//!
//! Before the swap the old process writes `update-pending.json` in the config folder and
//! copies the database. The new binary reads the marker first thing at start: it has
//! [`DEADLINE`] to get the database open, the daemon initialized and the API bound, then
//! [`commit`]s. A setup error, a panic, the deadline, or a second start that finds the
//! marker unchanged puts the old binary and the database copy back and restarts the old
//! version, which shows why.

use crate::swap;
use crate::version::Version;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

/// How long a new version has to become healthy
pub const DEADLINE: Duration = Duration::from_secs(90);

/// The marker's name in the config folder
pub const MARKER: &str = "update-pending.json";

/// `update-pending.json` (data-model.md)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Marker {
    /// Written before the swap; `starts` counts the new version's starts
    Pending {
        from: String,
        to: String,
        /// `patch` or `full`, for `putio_sync_update_attempts_total`
        kind: String,
        exe: PathBuf,
        old_path: PathBuf,
        #[serde(default)]
        db: Option<PathBuf>,
        #[serde(default)]
        db_backup: Option<PathBuf>,
        started_at: DateTime<Utc>,
        starts: u32,
    },
    /// Written by a rollback, for the old version to show
    RolledBack { rolled_back: String, reason: String },
}

/// What [`on_start`] found
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    /// No update in progress
    Normal,
    /// This is the new version's first start: [`arm`], then [`commit`] once healthy
    Trial,
    /// The new version already had its start and didn't commit: roll back now
    RollBack(String),
}

fn marker_path(config_dir: &Path) -> PathBuf {
    config_dir.join(MARKER)
}

/// Read the marker, if there is a readable one
pub fn read(config_dir: &Path) -> Option<Marker> {
    let text = std::fs::read_to_string(marker_path(config_dir)).ok()?;
    serde_json::from_str(&text)
        .map_err(|e| tracing::warn!(error = %e, "Ignoring an unreadable {MARKER}"))
        .ok()
}

fn write(config_dir: &Path, marker: &Marker) -> std::io::Result<()> {
    let path = marker_path(config_dir);
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(marker)?)?;
    std::fs::rename(tmp, path)
}

fn remove(config_dir: &Path) {
    let _ = std::fs::remove_file(marker_path(config_dir));
}

/// The database copy taken before updating to `to`
pub fn backup_path(db: &Path, to: Version) -> PathBuf {
    let mut name = db.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".pre-{to}"));
    db.with_file_name(name)
}

/// Write the marker and copy the database, before the swap (T034)
pub async fn begin(
    config_dir: &Path,
    from: Version,
    to: Version,
    kind: &str,
    exe: &Path,
    db: Option<&putio_sync_db::Database>,
) -> Result<(), String> {
    let (db_path, db_backup) = match db {
        Some(db) => {
            let backup = backup_path(db.path(), to);
            let _ = std::fs::remove_file(&backup);
            db.snapshot_to(&backup)
                .await
                .map_err(|e| format!("could not copy the database: {e}"))?;
            (Some(db.path().to_path_buf()), Some(backup))
        }
        None => (None, None),
    };
    let marker = Marker::Pending {
        from: from.to_string(),
        to: to.to_string(),
        kind: kind.to_string(),
        exe: exe.to_path_buf(),
        old_path: swap::old_path(exe),
        db: db_path,
        db_backup,
        started_at: Utc::now(),
        starts: 0,
    };
    write(config_dir, &marker).map_err(|e| format!("could not write {MARKER}: {e}"))
}

/// Forget an update that never swapped (the swap failed after [`begin`])
pub fn abandon(config_dir: &Path) {
    if let Some(Marker::Pending { .. }) = read(config_dir) {
        remove(config_dir);
    }
}

/// Look at the marker, first thing at start
pub fn on_start(config_dir: &Path, own: Version) -> Start {
    let Some(Marker::Pending {
        from,
        to,
        kind,
        exe,
        old_path,
        db,
        db_backup,
        started_at,
        starts,
    }) = read(config_dir)
    else {
        return Start::Normal;
    };
    if to != own.to_string() {
        // The old version is running: the swap or the exec never happened
        tracing::warn!(%from, %to, "An update to {to} did not finish; carrying on with {own}");
        remove(config_dir);
        return Start::Normal;
    }
    if starts >= 1 {
        return Start::RollBack("did not start".into());
    }
    let marker = Marker::Pending {
        from,
        to,
        kind,
        exe,
        old_path,
        db,
        db_backup,
        started_at,
        starts: starts + 1,
    };
    match write(config_dir, &marker) {
        Ok(()) => Start::Trial,
        Err(e) => Start::RollBack(format!("could not write {MARKER}: {e}")),
    }
}

/// The new version is healthy: drop the marker and the old binary. The database copy stays
/// until the next update replaces it.
pub fn commit(config_dir: &Path) {
    COMMITTED.store(true, Ordering::SeqCst);
    let Some(Marker::Pending {
        from,
        to,
        kind,
        old_path,
        exe,
        ..
    }) = read(config_dir)
    else {
        return;
    };
    let _ = std::fs::remove_file(&old_path);
    let _ = std::fs::remove_file(exe.with_extension("failed.exe"));
    remove(config_dir);
    putio_sync_telemetry::update::record_attempt(&kind, "done");
    tracing::info!(%from, %to, kind = %kind, "Update finished");
}

/// Put the old binary and the database copy back and leave a [`Marker::RolledBack`].
/// Returns the binary to restart.
pub fn roll_back(config_dir: &Path, reason: &str) -> Result<PathBuf, String> {
    let Some(Marker::Pending {
        from,
        to,
        kind,
        exe,
        old_path,
        db,
        db_backup,
        ..
    }) = read(config_dir)
    else {
        return Err(format!("no update in {MARKER}"));
    };
    tracing::error!(%from, %to, reason, "The new version failed; rolling back");
    swap::restore(&old_path, &exe).map_err(|e| format!("could not restore the old binary: {e}"))?;
    if let (Some(db), Some(backup)) = (db, db_backup) {
        if backup.exists() {
            for suffix in ["-wal", "-shm"] {
                let mut side = db.clone().into_os_string();
                side.push(suffix);
                let _ = std::fs::remove_file(side);
            }
            std::fs::copy(&backup, &db)
                .map_err(|e| format!("could not restore the database: {e}"))?;
        }
    }
    putio_sync_telemetry::update::record_attempt(&kind, "rolled_back");
    write(
        config_dir,
        &Marker::RolledBack {
            rolled_back: to,
            reason: reason.to_string(),
        },
    )
    .map_err(|e| format!("could not write {MARKER}: {e}"))?;
    Ok(exe)
}

/// The rollback the last start left, for the old version to store and show. Deletes the
/// marker.
pub fn take_rolled_back(config_dir: &Path) -> Option<(String, String)> {
    match read(config_dir)? {
        Marker::RolledBack {
            rolled_back,
            reason,
        } => {
            remove(config_dir);
            Some((rolled_back, reason))
        }
        Marker::Pending { .. } => None,
    }
}

static COMMITTED: AtomicBool = AtomicBool::new(false);
static ARMED: OnceLock<PathBuf> = OnceLock::new();

/// Roll back and restart the old binary; returns only if that failed
pub fn roll_back_and_restart(config_dir: &Path, reason: &str) {
    match roll_back(config_dir, reason) {
        Ok(exe) => {
            let e = swap::restart(&exe, std::env::args_os().skip(1).collect());
            tracing::error!(error = %e, "Could not restart the old version");
        }
        Err(e) => tracing::error!(error = %e, "Rollback failed"),
    }
}

/// Start the trial: a panic, or no [`commit`] within `deadline`, rolls back and restarts
/// the old version
pub fn arm(config_dir: &Path, deadline: Duration) {
    if ARMED.set(config_dir.to_path_buf()).is_err() {
        return;
    }
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        previous(info);
        if let Some(dir) = ARMED.get() {
            if !COMMITTED.swap(true, Ordering::SeqCst) {
                roll_back_and_restart(dir, &format!("crashed at start: {info}"));
            }
        }
    }));
    std::thread::spawn(move || {
        std::thread::sleep(deadline);
        if let Some(dir) = ARMED.get() {
            if !COMMITTED.swap(true, Ordering::SeqCst) {
                roll_back_and_restart(dir, "did not become healthy in time");
                std::process::exit(1);
            }
        }
    });
}

/// The trial is armed and not yet committed
pub fn in_trial() -> bool {
    ARMED.get().is_some() && !COMMITTED.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        s.parse().unwrap()
    }

    struct Setup {
        dir: tempfile::TempDir,
        exe: PathBuf,
        db: putio_sync_db::Database,
    }

    /// A config folder with a database, and an installed 0.4.0 whose 0.3.0 waits as `.old`
    async fn setup() -> Setup {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("putio-sync");
        std::fs::write(&exe, "0.3.0").unwrap();
        let db = putio_sync_db::Database::new(dir.path().join("putio-sync.db"))
            .await
            .unwrap();
        db.updates().dismiss("0.2.0").await.unwrap();
        begin(dir.path(), v("0.3.0"), v("0.4.0"), "patch", &exe, Some(&db))
            .await
            .unwrap();
        swap::install(b"0.4.0", &exe).unwrap();
        Setup { dir, exe, db }
    }

    #[tokio::test]
    async fn begin_writes_the_marker_and_a_database_copy() {
        let s = setup().await;
        let Some(Marker::Pending {
            from,
            to,
            old_path,
            db_backup,
            starts,
            ..
        }) = read(s.dir.path())
        else {
            panic!("no marker");
        };
        assert_eq!((from.as_str(), to.as_str(), starts), ("0.3.0", "0.4.0", 0));
        assert_eq!(old_path, s.dir.path().join("putio-sync.old"));
        let backup = db_backup.unwrap();
        assert_eq!(backup, s.dir.path().join("putio-sync.db.pre-0.4.0"));
        assert!(backup.exists());
        let json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(s.dir.path().join(MARKER)).unwrap()).unwrap();
        for key in [
            "from",
            "to",
            "old_path",
            "db_backup",
            "started_at",
            "starts",
        ] {
            assert!(json.get(key).is_some(), "{key} missing: {json}");
        }
    }

    #[test]
    fn no_marker_is_a_normal_start() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(on_start(dir.path(), v("0.4.0")), Start::Normal);
    }

    #[tokio::test]
    async fn first_start_is_a_trial_and_the_second_rolls_back() {
        let s = setup().await;
        assert_eq!(on_start(s.dir.path(), v("0.4.0")), Start::Trial);
        assert!(matches!(
            read(s.dir.path()),
            Some(Marker::Pending { starts: 1, .. })
        ));
        assert_eq!(
            on_start(s.dir.path(), v("0.4.0")),
            Start::RollBack("did not start".into())
        );
    }

    #[tokio::test]
    async fn the_old_version_drops_an_unfinished_marker() {
        let s = setup().await;
        assert_eq!(on_start(s.dir.path(), v("0.3.0")), Start::Normal);
        assert_eq!(read(s.dir.path()), None);
    }

    #[tokio::test]
    async fn commit_removes_the_marker_and_the_old_binary() {
        let s = setup().await;
        on_start(s.dir.path(), v("0.4.0"));
        commit(s.dir.path());
        assert_eq!(read(s.dir.path()), None);
        assert!(!s.dir.path().join("putio-sync.old").exists());
        assert_eq!(std::fs::read(&s.exe).unwrap(), b"0.4.0");
        // the database copy stays until the next update
        assert!(s.dir.path().join("putio-sync.db.pre-0.4.0").exists());
    }

    #[tokio::test]
    async fn roll_back_restores_the_binary_and_the_database() {
        let s = setup().await;
        // the new version changes the database before it fails
        s.db.updates().dismiss("9.9.9").await.unwrap();
        let path = s.db.path().to_path_buf();
        s.db.close().await.unwrap();

        let exe = roll_back(s.dir.path(), "did not start").unwrap();
        assert_eq!(exe, s.exe);
        assert_eq!(std::fs::read(&s.exe).unwrap(), b"0.3.0");
        assert!(!s.dir.path().join("putio-sync.old").exists());
        assert_eq!(
            read(s.dir.path()),
            Some(Marker::RolledBack {
                rolled_back: "0.4.0".into(),
                reason: "did not start".into(),
            })
        );
        let db = putio_sync_db::Database::new(&path).await.unwrap();
        assert_eq!(
            db.updates()
                .get()
                .await
                .unwrap()
                .dismissed_version
                .as_deref(),
            Some("0.2.0")
        );

        // the old version picks the reason up once
        assert_eq!(on_start(s.dir.path(), v("0.3.0")), Start::Normal);
        assert_eq!(
            take_rolled_back(s.dir.path()),
            Some(("0.4.0".into(), "did not start".into()))
        );
        assert_eq!(read(s.dir.path()), None);
    }
}
