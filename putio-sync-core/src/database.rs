//! Where the state database (queue, history, stats) lives, and the one-time move out of
//! the download folder.
//!
//! Up to 0.2.x the database was `<download target_directory>/.putiosync.db`. Download
//! folders are often NFS or SMB shares, where SQLite locking is unreliable, and changing
//! the folder started a fresh, empty database. It now lives next to the config file
//! (`/config` in containers) unless `[database] path` says otherwise.

use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions, Connection};

use crate::config::Config;

/// File name of the database, in both the old and the new location
pub const FILE_NAME: &str = ".putiosync.db";

/// SQLite files that sit next to the database while it is open
const SIDECARS: [&str; 3] = ["-wal", "-shm", "-journal"];

/// `[database]` settings
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, Default)]
#[serde(default)]
pub struct DatabaseConfig {
    /// Database file; relative paths are relative to the config file's folder. Default
    /// `.putiosync.db` next to the config file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
}

impl DatabaseConfig {
    /// Nothing set, so the table is left out when the config is saved
    pub fn is_empty(&self) -> bool {
        self.path.is_none()
    }
}

/// The database file: `[database] path` when set, else `.putiosync.db` next to the config
/// file. Without a config file path it falls back to the default config path, then to
/// the old place in the download folder.
pub fn path_for(config: &Config, config_path: Option<&Path>) -> PathBuf {
    let default_config = Config::default_path().ok();
    let config_dir = config_path
        .or(default_config.as_deref())
        .and_then(Path::parent);
    match (&config.database.path, config_dir) {
        (Some(path), Some(dir)) if path.is_relative() => dir.join(path),
        (Some(path), _) => path.clone(),
        (None, Some(dir)) => dir.join(FILE_NAME),
        (None, None) => legacy_path(config),
    }
}

/// Where 0.2.x and earlier kept the database
pub fn legacy_path(config: &Config) -> PathBuf {
    config.download.target_directory.join(FILE_NAME)
}

/// What [`migrate`] did
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Migration {
    /// No database in the old place (or the old and new places are the same)
    NotNeeded,
    /// Moved from this old path
    Moved(PathBuf),
    /// A database is in both places; the new one is used and the old one left alone
    BothPresent(PathBuf),
}

/// Resolve the database path and move a 0.2.x database there first. Every command that
/// opens the database goes through this, so whichever runs first after an update does
/// the move. If the move fails the old database is used for this run, with a warning,
/// rather than starting an empty one.
pub async fn prepare(config: &Config, config_path: Option<&Path>) -> PathBuf {
    let path = path_for(config, config_path);
    let legacy = legacy_path(config);
    match migrate(&legacy, &path).await {
        Ok(Migration::NotNeeded) => path,
        Ok(Migration::Moved(from)) => {
            tracing::info!(
                "Moved the database from {} to {}",
                from.display(),
                path.display()
            );
            path
        }
        Ok(Migration::BothPresent(old)) => {
            tracing::warn!(
                "A database exists at {} and at the old location {}; using {}. Delete the \
                 old one once you no longer need it.",
                path.display(),
                old.display(),
                path.display()
            );
            path
        }
        Err(e) => {
            tracing::warn!(
                "Could not move the database from {} to {}: {e}. Using {} for now.",
                legacy.display(),
                path.display(),
                legacy.display()
            );
            legacy
        }
    }
}

/// Move the database at `legacy` to `new` if `new` doesn't exist yet. The WAL is
/// checkpointed first, so the main file holds everything. A rename is tried first; across
/// filesystems the file is copied to a temporary name, synced, renamed into place, and
/// only then is the old one removed. Running it again after an interruption picks up
/// where it left off.
pub async fn migrate(legacy: &Path, new: &Path) -> io::Result<Migration> {
    migrate_with(legacy, new, true).await
}

async fn migrate_with(legacy: &Path, new: &Path, allow_rename: bool) -> io::Result<Migration> {
    if same_file(legacy, new) || !legacy.is_file() {
        return Ok(Migration::NotNeeded);
    }
    if new.exists() {
        return Ok(Migration::BothPresent(legacy.to_path_buf()));
    }
    checkpoint(legacy).await?;
    // After a full checkpoint and close the WAL is empty and the shared-memory index is
    // stale; neither belongs with the moved file
    for suffix in SIDECARS {
        remove_if_present(&sidecar(legacy, suffix))?;
    }
    if let Some(parent) = new.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    if !(allow_rename && fs::rename(legacy, new).is_ok()) {
        copy_into_place(legacy, new)?;
        fs::remove_file(legacy)?;
    }
    sync_dir(new);
    Ok(Migration::Moved(legacy.to_path_buf()))
}

/// Fold the WAL into the main file. Fails if another process holds the database, so a
/// still-running old version keeps its file.
async fn checkpoint(path: &Path) -> io::Result<()> {
    let to_io = |e: sqlx::Error| io::Error::other(e.to_string());
    let mut conn = SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))
        .map_err(to_io)?
        .create_if_missing(false)
        .busy_timeout(std::time::Duration::from_secs(1))
        .connect()
        .await
        .map_err(to_io)?;
    let (busy, _, _): (i64, i64, i64) = sqlx::query_as("PRAGMA wal_checkpoint(TRUNCATE)")
        .fetch_one(&mut conn)
        .await
        .map_err(to_io)?;
    conn.close().await.map_err(to_io)?;
    if busy != 0 {
        return Err(io::Error::other(
            "the database is in use by another process",
        ));
    }
    Ok(())
}

/// Copy to `<new>.migrating`, sync it, then rename it into place
fn copy_into_place(from: &Path, to: &Path) -> io::Result<()> {
    let tmp = sidecar(to, ".migrating");
    fs::copy(from, &tmp)?;
    File::open(&tmp)?.sync_all()?;
    fs::rename(&tmp, to)
}

/// Best effort: make the rename durable. Not every platform can open a directory.
fn sync_dir(path: &Path) {
    if let Some(dir) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        if let Ok(dir) = File::open(dir) {
            let _ = dir.sync_all();
        }
    }
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    a == b
        || matches!(
            (fs::canonicalize(a), fs::canonicalize(b)),
            (Ok(a), Ok(b)) if a == b
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqliteJournalMode;

    /// A 0.2.x-style database: WAL mode, with a row still only in the WAL. The
    /// connection is returned so the caller decides when it closes.
    async fn wal_db(path: &Path) -> sqlx::SqliteConnection {
        let mut conn = SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))
            .unwrap()
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .connect()
            .await
            .unwrap();
        sqlx::query("PRAGMA wal_autocheckpoint = 0")
            .execute(&mut conn)
            .await
            .unwrap();
        sqlx::query("CREATE TABLE t (v TEXT)")
            .execute(&mut conn)
            .await
            .unwrap();
        sqlx::query("INSERT INTO t VALUES ('kept')")
            .execute(&mut conn)
            .await
            .unwrap();
        conn
    }

    async fn rows(path: &Path) -> Vec<String> {
        let mut conn = SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))
            .unwrap()
            .connect()
            .await
            .unwrap();
        sqlx::query_scalar("SELECT v FROM t")
            .fetch_all(&mut conn)
            .await
            .unwrap()
    }

    /// A WAL database as a killed process leaves it: the last row only in the `-wal` file
    async fn crashed_wal_db(path: &Path) {
        let source = sidecar(path, ".source");
        let conn = wal_db(&source).await;
        fs::copy(&source, path).unwrap();
        fs::copy(sidecar(&source, "-wal"), sidecar(path, "-wal")).unwrap();
        conn.close().await.unwrap();
        fs::remove_file(&source).unwrap();
    }

    #[tokio::test]
    async fn moves_on_the_same_filesystem() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("downloads").join(FILE_NAME);
        let new = dir.path().join("config").join(FILE_NAME);
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        crashed_wal_db(&legacy).await;
        fs::write(sidecar(&legacy, "-shm"), b"stale").unwrap();

        assert_eq!(
            migrate(&legacy, &new).await.unwrap(),
            Migration::Moved(legacy.clone())
        );
        assert!(!legacy.exists());
        for suffix in SIDECARS {
            assert!(!sidecar(&legacy, suffix).exists(), "{suffix} left behind");
        }
        assert_eq!(rows(&new).await, ["kept"]);

        // Idempotent: nothing left to move
        assert_eq!(migrate(&legacy, &new).await.unwrap(), Migration::NotNeeded);
        assert_eq!(rows(&new).await, ["kept"]);
    }

    #[tokio::test]
    #[cfg_attr(
        windows,
        ignore = "copy_into_place can't sync_all a read-only handle on Windows"
    )]
    async fn copies_when_rename_is_not_possible() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("old").join(FILE_NAME);
        let new = dir.path().join("new").join(FILE_NAME);
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        crashed_wal_db(&legacy).await;

        assert_eq!(
            migrate_with(&legacy, &new, false).await.unwrap(),
            Migration::Moved(legacy.clone())
        );
        assert!(!legacy.exists());
        assert!(!sidecar(&new, ".migrating").exists());
        assert_eq!(rows(&new).await, ["kept"]);
    }

    #[tokio::test]
    #[cfg_attr(
        windows,
        ignore = "copy_into_place can't sync_all a read-only handle on Windows"
    )]
    async fn an_interrupted_copy_is_redone() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("old").join(FILE_NAME);
        let new = dir.path().join("new").join(FILE_NAME);
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        fs::create_dir_all(new.parent().unwrap()).unwrap();
        wal_db(&legacy).await.close().await.unwrap();
        fs::write(sidecar(&new, ".migrating"), b"half a copy").unwrap();

        migrate_with(&legacy, &new, false).await.unwrap();
        assert_eq!(rows(&new).await, ["kept"]);
    }

    #[tokio::test]
    async fn already_migrated() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("downloads").join(FILE_NAME);
        let new = dir.path().join(FILE_NAME);
        wal_db(&new).await.close().await.unwrap();

        assert_eq!(migrate(&legacy, &new).await.unwrap(), Migration::NotNeeded);
        assert_eq!(rows(&new).await, ["kept"]);
    }

    #[tokio::test]
    async fn same_place_is_not_a_migration() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        wal_db(&path).await.close().await.unwrap();
        assert_eq!(migrate(&path, &path).await.unwrap(), Migration::NotNeeded);
        assert!(path.exists());
    }

    #[tokio::test]
    async fn both_present_keeps_both_and_uses_the_new_one() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config").join("config.toml");
        let mut config = Config::default();
        config.download.target_directory = dir.path().join("downloads");
        let legacy = legacy_path(&config);
        let new = path_for(&config, Some(&config_path));
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        fs::create_dir_all(new.parent().unwrap()).unwrap();
        wal_db(&legacy).await.close().await.unwrap();
        wal_db(&new).await.close().await.unwrap();

        assert_eq!(
            migrate(&legacy, &new).await.unwrap(),
            Migration::BothPresent(legacy.clone())
        );
        assert_eq!(prepare(&config, Some(&config_path)).await, new);
        assert!(legacy.exists());
    }

    #[tokio::test]
    async fn prepare_moves_a_0_2_layout_into_the_config_folder() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config").join("config.toml");
        let mut config = Config::default();
        config.download.target_directory = dir.path().join("downloads");
        fs::create_dir_all(&config.download.target_directory).unwrap();
        wal_db(&legacy_path(&config)).await.close().await.unwrap();

        let path = prepare(&config, Some(&config_path)).await;
        assert_eq!(path, dir.path().join("config").join(FILE_NAME));
        assert_eq!(rows(&path).await, ["kept"]);
        assert!(!legacy_path(&config).exists());
    }

    #[tokio::test]
    async fn a_database_in_use_stays_where_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = dir.path().join("old").join(FILE_NAME);
        let new = dir.path().join("new").join(FILE_NAME);
        fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        let mut reader = wal_db(&legacy).await;
        // An open read transaction pins the WAL, so a TRUNCATE checkpoint reports busy
        sqlx::query("BEGIN").execute(&mut reader).await.unwrap();
        sqlx::query("SELECT * FROM t")
            .fetch_all(&mut reader)
            .await
            .unwrap();

        assert!(migrate(&legacy, &new).await.is_err());
        assert!(legacy.exists());
        assert!(!new.exists());
        reader.close().await.unwrap();
    }

    #[test]
    fn path_for_defaults_next_to_the_config_file() {
        let mut config = Config::default();
        config.download.target_directory = PathBuf::from("/downloads");
        let config_path = Path::new("/config/config.toml");
        assert_eq!(
            path_for(&config, Some(config_path)),
            PathBuf::from("/config/.putiosync.db")
        );

        config.database.path = Some(PathBuf::from("state/putio.db"));
        assert_eq!(
            path_for(&config, Some(config_path)),
            PathBuf::from("/config/state/putio.db")
        );

        config.database.path = Some(PathBuf::from("/var/lib/putio.db"));
        assert_eq!(
            path_for(&config, Some(config_path)),
            PathBuf::from("/var/lib/putio.db")
        );
        assert_eq!(
            legacy_path(&config),
            PathBuf::from("/downloads/.putiosync.db")
        );
    }
}
