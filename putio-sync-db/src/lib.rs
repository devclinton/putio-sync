//! # putio-sync-db
//!
//! SQLite database layer for putio-sync state management.
//!
//! This crate provides repository patterns for managing persistent state
//! including download tracking, history, statistics, and queue management.
//!
//! ## Features
//!
//! - **Download Tracking**: Track active downloads with progress and metadata
//! - **History Management**: Record download and delete operations
//! - **Statistics**: Aggregate statistics by date range and file extension
//! - **Queue Management**: Priority-based download queue
//! - **Torrent Tracking**: Checksum-based deduplication for processed torrents
//!
//! ## Usage
//!
//! ```no_run
//! use putio_sync_db::{Database, HistoryRepository};
//! use std::path::Path;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let db = Database::new(Path::new("sync.db")).await?;
//!     let history = HistoryRepository::new(db.pool());
//!
//!     // Query history
//!     let entries = history.query(None, 10, 0, Some("ubuntu")).await?;
//!     Ok(())
//! }
//! ```

pub mod analytics;
pub mod arr;
pub mod decisions;
pub mod downloads;
pub mod error;
pub mod held;
pub mod history;
pub mod models;
pub mod pipeline;
pub mod queue;
pub mod releases;
pub mod schema;
pub mod sources;
pub mod stats;
pub mod storage;
pub mod torrents;
pub mod verify;
pub mod whats_new;

pub use arr::{ArrItem, ArrRepository, ArrState, BlocklistState, NewArrItem, PutioProgress};
pub use decisions::{RuleDecisionRecord, RuleDecisionsRepository};
pub use downloads::DownloadRepository;
pub use error::{DbError, DbResult};
pub use held::{Held, HeldRepository, HoldStats, NewHeld};
pub use history::{DailyTotals, HistoryRepository};
pub use pipeline::{
    DeferredDelete, DeferredDeletesRepository, FinishedRepository, FlagsRepository, HoldsRepository,
};
pub use queue::{QueueRepository, QueueStats};
pub use releases::{
    CheckState, FlagUndo, HeldSettle, NewCheck, NewReport, PendingCheck, ReleaseStats,
    ReleasesRepository, ReportKind, ReportRow,
};
pub use sources::SourcesRepository;
pub use stats::{ExtensionStats, StatsRepository};
pub use storage::{NewRunFile, StorageRepository, StorageRun, StorageRunFile};
pub use torrents::ProcessedTorrentsRepository;
pub use verify::{NewProblem, RunCounts, VerifyProblem, VerifyRepository, VerifyRun};
pub use whats_new::WhatsNewRepository;

use crate::error::tracked;
use putio_sync_telemetry::DbOp;
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions, SqliteSynchronous,
};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::Duration;
use tracing::{debug, info};

/// Database connection pool and management
pub struct Database {
    pool: SqlitePool,
    path: PathBuf,
}

impl Database {
    /// Create a new database connection
    ///
    /// This will create the database file if it doesn't exist and run migrations.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the SQLite database file
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use putio_sync_db::Database;
    /// use std::path::PathBuf;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let db = Database::new(&PathBuf::from("putio.db")).await.unwrap();
    /// }
    /// ```
    pub async fn new<P: AsRef<Path>>(path: P) -> DbResult<Self> {
        let path = path.as_ref();

        // Create parent directory if it doesn't exist
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent).map_err(tracked(
                    DbOp::Migrate,
                    "Failed to create database directory",
                ))?;
            }
        }

        debug!("Opening database at: {}", path.display());

        // Create connection options
        let options = SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))
            .map_err(tracked(DbOp::Migrate, "Invalid database path"))?
            .create_if_missing(true)
            // WAL lets readers and the claiming writer run side by side; the busy timeout
            // makes brief write contention wait instead of failing.
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(10));

        // Create connection pool
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .map_err(tracked(DbOp::Migrate, "Failed to connect to database"))?;

        let db = Database {
            pool,
            path: path.to_path_buf(),
        };

        // Run migrations
        db.migrate().await?;

        info!("Database initialized at: {}", path.display());

        Ok(db)
    }

    /// Open an existing database read-only, without migrating it or touching its files
    /// (the archive commands, feature 041). Writes through the handle fail.
    pub async fn open_read_only<P: AsRef<Path>>(path: P) -> DbResult<Self> {
        let path = path.as_ref();
        let options = SqliteConnectOptions::new()
            .filename(path)
            .read_only(true)
            .busy_timeout(Duration::from_secs(10));
        let pool = SqlitePoolOptions::new()
            .max_connections(2)
            .connect_with(options)
            .await
            .map_err(tracked(DbOp::Other, "Failed to open database read-only"))?;
        Ok(Database {
            pool,
            path: path.to_path_buf(),
        })
    }

    /// Write a consistent, compact copy of this database to `dest` with `VACUUM INTO` and
    /// return the number of download history rows in it. `dest` must not exist. Safe while
    /// other connections write; the source is not changed (feature 041).
    pub async fn snapshot_to(&self, dest: &Path) -> DbResult<i64> {
        let quoted = dest.to_string_lossy().replace('\'', "''");
        sqlx::query(&format!("VACUUM INTO '{quoted}'"))
            .execute(&self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to copy the database"))?;

        let copy = Database::open_read_only(dest).await?;
        let rows = sqlx::query_scalar("SELECT COUNT(*) FROM download_history")
            .fetch_one(copy.pool())
            .await
            .map_err(tracked(DbOp::Other, "Failed to count the copied history"));
        copy.close().await?;
        rows
    }

    /// Get the underlying connection pool
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// The database file, as given to [`Database::new`]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The database file and the journal files SQLite keeps beside it
    pub fn files(&self) -> Vec<PathBuf> {
        let mut files = vec![self.path.clone()];
        for suffix in ["-wal", "-shm", "-journal"] {
            let mut name = self.path.clone().into_os_string();
            name.push(suffix);
            files.push(name.into());
        }
        files
    }

    /// Get a download repository
    pub fn downloads(&self) -> DownloadRepository<'_> {
        DownloadRepository::new(&self.pool)
    }

    /// Get a history repository
    pub fn history(&self) -> HistoryRepository<'_> {
        HistoryRepository::new(&self.pool)
    }

    /// Get a stats repository
    pub fn stats(&self) -> StatsRepository<'_> {
        StatsRepository::new(&self.pool)
    }

    /// Get a queue repository
    pub fn queue(&self) -> QueueRepository<'_> {
        QueueRepository::new(&self.pool)
    }

    /// Get a download sources repository (feature 006)
    pub fn sources(&self) -> SourcesRepository<'_> {
        SourcesRepository::new(&self.pool)
    }

    /// Get the Radarr/Sonarr items repository (feature 011)
    pub fn arr(&self) -> ArrRepository<'_> {
        ArrRepository::new(&self.pool)
    }

    /// Get the release pre-check repository (feature 042)
    pub fn releases(&self) -> ReleasesRepository<'_> {
        ReleasesRepository::new(&self.pool)
    }

    /// Get the held magnets repository (feature 043)
    pub fn held(&self) -> HeldRepository<'_> {
        HeldRepository::new(&self.pool)
    }

    /// Get a rule decisions repository (feature 009)
    pub fn rule_decisions(&self) -> RuleDecisionsRepository<'_> {
        RuleDecisionsRepository::new(&self.pool)
    }

    /// Get a processed torrents repository
    pub fn processed_torrents(&self) -> ProcessedTorrentsRepository<'_> {
        ProcessedTorrentsRepository::new(&self.pool)
    }

    /// Get the transfer holds repository (feature 039)
    pub fn holds(&self) -> HoldsRepository<'_> {
        HoldsRepository::new(&self.pool)
    }

    /// Get the deferred remote deletes repository (feature 025)
    pub fn deferred_deletes(&self) -> DeferredDeletesRepository<'_> {
        DeferredDeletesRepository::new(&self.pool)
    }

    /// Get the daemon flags repository (feature 039)
    pub fn flags(&self) -> FlagsRepository<'_> {
        FlagsRepository::new(&self.pool)
    }

    /// Get the What's new seen-entries repository (feature 017)
    pub fn whats_new(&self) -> WhatsNewRepository<'_> {
        WhatsNewRepository::new(&self.pool)
    }

    /// Get the verify runs and problems repository (feature 034)
    pub fn verify(&self) -> VerifyRepository<'_> {
        VerifyRepository::new(&self.pool)
    }

    /// Get the Put.io storage runs repository (feature 033)
    pub fn storage(&self) -> StorageRepository<'_> {
        StorageRepository::new(&self.pool)
    }

    /// Get the Recently finished repository (feature 039)
    pub fn finished(&self) -> FinishedRepository<'_> {
        FinishedRepository::new(&self.pool)
    }

    /// The files the Active transfers pipeline is built from (feature 039, R1, R6): every
    /// queued file, the files of `transfer_ids`, and files done within `retention_hours`
    pub async fn pipeline_files(
        &self,
        transfer_ids: &[i64],
        retention_hours: u64,
    ) -> DbResult<Vec<pipeline::PipelineFile>> {
        pipeline::pipeline_files(&self.pool, transfer_ids, retention_hours).await
    }

    /// How putio-sync got each of `transfer_ids`, when it knows (feature 039):
    /// `arr:<category>` for Radarr/Sonarr releases, else `watch` or `manual`
    pub async fn transfer_origins(
        &self,
        transfer_ids: &[i64],
    ) -> DbResult<std::collections::HashMap<i64, String>> {
        pipeline::transfer_origins(&self.pool, transfer_ids).await
    }

    /// SQLite's `PRAGMA quick_check`; `["ok"]` means healthy (doctor, feature 010)
    pub async fn quick_check(&self) -> DbResult<Vec<String>> {
        sqlx::query_scalar("PRAGMA quick_check")
            .fetch_all(&self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to run quick_check"))
    }

    /// Run database migrations
    ///
    /// This creates all necessary tables if they don't exist.
    async fn migrate(&self) -> DbResult<()> {
        debug!("Running database migrations");

        // Create downloads table
        sqlx::query(schema::CREATE_DOWNLOADS_TABLE)
            .execute(&self.pool)
            .await
            .map_err(tracked(DbOp::Migrate, "Failed to create downloads table"))?;

        // Create download_history table
        sqlx::query(schema::CREATE_DOWNLOAD_HISTORY_TABLE)
            .execute(&self.pool)
            .await
            .map_err(tracked(
                DbOp::Migrate,
                "Failed to create download_history table",
            ))?;

        // Create download_stats table
        sqlx::query(schema::CREATE_DOWNLOAD_STATS_TABLE)
            .execute(&self.pool)
            .await
            .map_err(tracked(
                DbOp::Migrate,
                "Failed to create download_stats table",
            ))?;

        // Initialize stats table if empty
        sqlx::query(schema::INIT_DOWNLOAD_STATS)
            .execute(&self.pool)
            .await
            .map_err(tracked(DbOp::Migrate, "Failed to initialize stats"))?;

        // Create download_queue table
        sqlx::query(schema::CREATE_DOWNLOAD_QUEUE_TABLE)
            .execute(&self.pool)
            .await
            .map_err(tracked(
                DbOp::Migrate,
                "Failed to create download_queue table",
            ))?;

        // Migrate: Add relative_path column to download_queue if it doesn't exist
        self.migrate_add_relative_path_column().await?;

        // Existing-file handling (feature 040)
        for (table, column, declaration) in [
            ("download_queue", "save_as", "TEXT"),
            ("download_queue", "fixed_path", "INTEGER NOT NULL DEFAULT 0"),
            ("download_history", "local_path", "TEXT"),
            ("download_history", "outcome", "TEXT"),
            // Download verification (feature 034)
            ("downloads", "crc32", "TEXT"),
            ("downloads", "verify", "TEXT"),
            ("downloads", "verified_at", "TIMESTAMP"),
            ("download_history", "verify", "TEXT"),
            ("download_history", "redownloads", "INTEGER"),
            ("download_queue", "redownload", "INTEGER NOT NULL DEFAULT 0"),
        ] {
            self.add_column_if_missing(table, column, declaration)
                .await?;
        }

        // Create processed_torrents table
        sqlx::query(schema::CREATE_PROCESSED_TORRENTS_TABLE)
            .execute(&self.pool)
            .await
            .map_err(tracked(
                DbOp::Migrate,
                "Failed to create processed_torrents table",
            ))?;

        // Create the download source tables (feature 006)
        for (sql, context) in [
            (
                schema::CREATE_DOWNLOAD_SOURCES_TABLE,
                "Failed to create download_sources table",
            ),
            (
                schema::CREATE_TRANSFER_ORIGINS_TABLE,
                "Failed to create transfer_origins table",
            ),
            // Radarr/Sonarr items (feature 011)
            (
                schema::CREATE_ARR_ITEMS_TABLE,
                "Failed to create arr_items table",
            ),
            (
                schema::CREATE_ARR_ITEM_FILES_TABLE,
                "Failed to create arr_item_files table",
            ),
            (
                schema::CREATE_RULE_DECISIONS_TABLE,
                "Failed to create rule_decisions table",
            ),
            // Active transfers pipeline (feature 039)
            (
                schema::CREATE_PIPELINE_HOLDS_TABLE,
                "Failed to create pipeline_holds table",
            ),
            (
                schema::CREATE_DAEMON_FLAGS_TABLE,
                "Failed to create daemon_flags table",
            ),
            (
                schema::CREATE_PIPELINE_FINISHED_TABLE,
                "Failed to create pipeline_finished table",
            ),
            // Processing pipeline (feature 038)
            (
                schema::CREATE_PIPELINE_ITEMS_TABLE,
                "Failed to create pipeline_items table",
            ),
            (
                schema::CREATE_PIPELINE_STEPS_TABLE,
                "Failed to create pipeline_steps table",
            ),
            (
                schema::CREATE_PIPELINE_ATTEMPTS_TABLE,
                "Failed to create pipeline_attempts table",
            ),
            // Subtitles wait for the delete (feature 025)
            (
                schema::CREATE_PIPELINE_DEFERRED_DELETES_TABLE,
                "Failed to create pipeline_deferred_deletes table",
            ),
            // Release pre-check (feature 042)
            (
                schema::CREATE_RELEASE_CHECKS_TABLE,
                "Failed to create release_checks table",
            ),
            (
                schema::CREATE_RELEASE_REPORTS_TABLE,
                "Failed to create release_reports table",
            ),
            // Held magnets (feature 043)
            (
                schema::CREATE_HELD_MAGNETS_TABLE,
                "Failed to create held_magnets table",
            ),
            (
                schema::CREATE_HOLD_ENDS_TABLE,
                "Failed to create hold_ends table",
            ),
            // What's new (feature 017)
            (
                schema::CREATE_WHATS_NEW_SEEN_TABLE,
                "Failed to create whats_new_seen table",
            ),
            // Put.io storage (feature 033)
            (
                schema::CREATE_STORAGE_RUNS_TABLE,
                "Failed to create storage_runs table",
            ),
            (
                schema::CREATE_STORAGE_RUN_FILES_TABLE,
                "Failed to create storage_run_files table",
            ),
            // Download verification (feature 034)
            (
                schema::CREATE_VERIFY_RUNS_TABLE,
                "Failed to create verify_runs table",
            ),
            (
                schema::CREATE_VERIFY_PROBLEMS_TABLE,
                "Failed to create verify_problems table",
            ),
        ] {
            sqlx::query(sql)
                .execute(&self.pool)
                .await
                .map_err(tracked(DbOp::Migrate, context))?;
        }

        self.migrate_pipeline_steps_checks().await?;

        // Create performance indexes
        sqlx::query(schema::CREATE_INDEXES)
            .execute(&self.pool)
            .await
            .map_err(tracked(DbOp::Migrate, "Failed to create indexes"))?;

        info!("Database migrations completed");

        Ok(())
    }

    /// Feature 025 widens the CHECK lists of `pipeline_steps` (kinds `subtitle_scan` and
    /// `subtitles`, skip reason `no_video`). SQLite cannot alter a CHECK, so a table made by
    /// 038 is rebuilt: new table, copy, drop, rename, in one transaction with foreign keys
    /// off so `pipeline_attempts` is not cascade-deleted. Idempotent: a table that already
    /// allows `subtitles` is left alone.
    async fn migrate_pipeline_steps_checks(&self) -> DbResult<()> {
        let stored: Option<(String,)> = sqlx::query_as(
            "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = 'pipeline_steps'",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(tracked(
            DbOp::Migrate,
            "Failed to read pipeline_steps schema",
        ))?;
        let Some((sql,)) = stored else {
            return Ok(());
        };
        if sql.contains("'subtitles'") {
            return Ok(());
        }

        let create_new =
            schema::CREATE_PIPELINE_STEPS_TABLE.replacen("pipeline_steps", "pipeline_steps_new", 1);
        let cols = "id, item_key, stage, branch, position, kind, name, required, changes_files, \
                    state, skip_reason, next_attempt_at, progress";
        let ctx = "Failed to rebuild pipeline_steps";
        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(tracked(DbOp::Migrate, ctx))?;
        // foreign_keys cannot change inside a transaction
        sqlx::query("PRAGMA foreign_keys = OFF")
            .execute(&mut *conn)
            .await
            .map_err(tracked(DbOp::Migrate, ctx))?;
        let result: Result<(), sqlx::Error> = async {
            sqlx::query("BEGIN").execute(&mut *conn).await?;
            let steps: Result<(), sqlx::Error> = async {
                sqlx::query("DROP TABLE IF EXISTS pipeline_steps_new")
                    .execute(&mut *conn)
                    .await?;
                sqlx::query(&create_new).execute(&mut *conn).await?;
                sqlx::query(&format!(
                    "INSERT INTO pipeline_steps_new ({cols}) SELECT {cols} FROM pipeline_steps"
                ))
                .execute(&mut *conn)
                .await?;
                sqlx::query("DROP TABLE pipeline_steps")
                    .execute(&mut *conn)
                    .await?;
                sqlx::query("ALTER TABLE pipeline_steps_new RENAME TO pipeline_steps")
                    .execute(&mut *conn)
                    .await?;
                let broken = sqlx::query("PRAGMA foreign_key_check")
                    .fetch_all(&mut *conn)
                    .await?;
                if !broken.is_empty() {
                    return Err(sqlx::Error::Protocol(
                        "foreign_key_check failed after rebuilding pipeline_steps".into(),
                    ));
                }
                Ok(())
            }
            .await;
            match steps {
                Ok(()) => sqlx::query("COMMIT").execute(&mut *conn).await.map(|_| ()),
                Err(e) => {
                    let _ = sqlx::query("ROLLBACK").execute(&mut *conn).await;
                    Err(e)
                }
            }
        }
        .await;
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&mut *conn)
            .await
            .map_err(tracked(DbOp::Migrate, ctx))?;
        result.map_err(tracked(DbOp::Migrate, ctx))?;
        info!("Rebuilt pipeline_steps for the subtitle step kinds");
        Ok(())
    }

    /// Migrate: Add relative_path column to download_queue table if it doesn't exist
    async fn migrate_add_relative_path_column(&self) -> DbResult<()> {
        // Check if the column already exists
        let result: Result<(i64,), sqlx::Error> = sqlx::query_as(
            "SELECT COUNT(*) FROM pragma_table_info('download_queue') WHERE name = 'relative_path'",
        )
        .fetch_one(&self.pool)
        .await;

        match result {
            Ok((count,)) if count > 0 => {
                debug!("Column 'relative_path' already exists in download_queue table");
                Ok(())
            }
            _ => {
                debug!("Adding 'relative_path' column to download_queue table");
                sqlx::query(
                    "ALTER TABLE download_queue ADD COLUMN relative_path TEXT NOT NULL DEFAULT ''",
                )
                .execute(&self.pool)
                .await
                .map_err(tracked(DbOp::Migrate, "Failed to add relative_path column"))?;
                info!("Successfully added 'relative_path' column to download_queue table");
                Ok(())
            }
        }
    }

    /// `ALTER TABLE table ADD COLUMN column declaration`, unless the column exists
    async fn add_column_if_missing(
        &self,
        table: &str,
        column: &str,
        declaration: &str,
    ) -> DbResult<()> {
        let (count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM pragma_table_info(?) WHERE name = ?")
                .bind(table)
                .bind(column)
                .fetch_one(&self.pool)
                .await
                .map_err(tracked(DbOp::Migrate, "Failed to read table columns"))?;
        if count > 0 {
            return Ok(());
        }
        sqlx::query(&format!(
            "ALTER TABLE {table} ADD COLUMN {column} {declaration}"
        ))
        .execute(&self.pool)
        .await
        .map_err(tracked(DbOp::Migrate, "Failed to add column"))?;
        info!(table, column, "Added database column");
        Ok(())
    }

    /// Close the database connection
    pub async fn close(self) -> DbResult<()> {
        self.pool.close().await;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_database_creation() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let db = Database::new(&db_path).await.unwrap();
        assert!(db_path.exists());

        db.close().await.unwrap();
    }

    #[tokio::test]
    async fn snapshot_copies_history_while_another_connection_writes() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("live.db");
        let daemon = Database::new(&db_path).await.unwrap();
        for id in 1..=3 {
            daemon
                .history()
                .insert(
                    id,
                    "a.mkv",
                    10,
                    models::HistoryAction::Downloaded,
                    None,
                    None,
                )
                .await
                .unwrap();
        }

        // The archive command's own read-only handle, with the daemon's still open
        let reader = Database::open_read_only(&db_path).await.unwrap();
        daemon
            .history()
            .insert(
                4,
                "b.mkv",
                10,
                models::HistoryAction::Downloaded,
                None,
                None,
            )
            .await
            .unwrap();
        let copy_path = temp_dir.path().join("copy.sqlite");
        assert_eq!(reader.snapshot_to(&copy_path).await.unwrap(), 4);

        let copy = Database::open_read_only(&copy_path).await.unwrap();
        assert_eq!(copy.history().count(None, None).await.unwrap(), 4);
        assert_eq!(daemon.history().count(None, None).await.unwrap(), 4);
    }

    #[tokio::test]
    async fn read_only_handle_cannot_write_or_create() {
        let temp_dir = TempDir::new().unwrap();
        assert!(Database::open_read_only(temp_dir.path().join("missing.db"))
            .await
            .is_err());

        let db_path = temp_dir.path().join("live.db");
        Database::new(&db_path)
            .await
            .unwrap()
            .close()
            .await
            .unwrap();
        let reader = Database::open_read_only(&db_path).await.unwrap();
        let wrote = reader
            .history()
            .insert(
                1,
                "a.mkv",
                10,
                models::HistoryAction::Downloaded,
                None,
                None,
            )
            .await;
        assert!(wrote.is_err());
    }

    #[tokio::test]
    async fn test_database_creates_directory() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("nested").join("dir").join("test.db");

        let db = Database::new(&db_path).await.unwrap();
        assert!(db_path.exists());
        assert!(db_path.parent().unwrap().exists());

        db.close().await.unwrap();
    }

    /// Feature 040 columns reach a database made before them, and a re-run keeps them
    #[tokio::test]
    async fn existing_file_columns_are_added_once() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = Database::new(&db_path).await.unwrap();
        sqlx::query("ALTER TABLE download_queue DROP COLUMN save_as")
            .execute(db.pool())
            .await
            .unwrap();
        sqlx::query("ALTER TABLE download_history DROP COLUMN outcome")
            .execute(db.pool())
            .await
            .unwrap();
        db.close().await.unwrap();

        for _ in 0..2 {
            let db = Database::new(&db_path).await.unwrap();
            for (table, column) in [
                ("download_queue", "save_as"),
                ("download_queue", "fixed_path"),
                ("download_history", "local_path"),
                ("download_history", "outcome"),
            ] {
                let (count,): (i64,) =
                    sqlx::query_as("SELECT COUNT(*) FROM pragma_table_info(?) WHERE name = ?")
                        .bind(table)
                        .bind(column)
                        .fetch_one(db.pool())
                        .await
                        .unwrap();
                assert_eq!(count, 1, "{table}.{column}");
            }
            db.close().await.unwrap();
        }
    }

    #[tokio::test]
    async fn test_database_migrations_run() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        let db = Database::new(&db_path).await.unwrap();

        // Verify tables exist by querying them
        let result = sqlx::query("SELECT COUNT(*) FROM downloads")
            .fetch_one(db.pool())
            .await;
        assert!(result.is_ok());

        let result = sqlx::query("SELECT COUNT(*) FROM download_history")
            .fetch_one(db.pool())
            .await;
        assert!(result.is_ok());

        let result = sqlx::query("SELECT COUNT(*) FROM download_stats")
            .fetch_one(db.pool())
            .await;
        assert!(result.is_ok());

        db.close().await.unwrap();
    }

    #[tokio::test]
    async fn migrate_adds_the_pipeline_tables_to_an_existing_database() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");

        // A database from before feature 039, with a queued file
        let db = Database::new(&db_path).await.unwrap();
        for table in ["pipeline_holds", "daemon_flags", "pipeline_finished"] {
            sqlx::query(&format!("DROP TABLE {table}"))
                .execute(db.pool())
                .await
                .unwrap();
        }
        db.queue()
            .add(1, "a.mkv", 10, "", "", 0, models::QueueStatus::Pending)
            .await
            .unwrap();
        db.close().await.unwrap();

        let db = Database::new(&db_path).await.unwrap();
        for table in ["pipeline_holds", "daemon_flags", "pipeline_finished"] {
            let result = sqlx::query(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(db.pool())
                .await;
            assert!(result.is_ok(), "{table} exists");
        }
        assert!(db.queue().get_by_file_id(1).await.unwrap().is_some());
        let bad = sqlx::query(
            "INSERT INTO pipeline_finished (key, name, size, outcome, done_at) \
             VALUES ('k', 'n', 1, 'other', CURRENT_TIMESTAMP)",
        )
        .execute(db.pool())
        .await;
        assert!(bad.is_err(), "outcome is done or failed");
        db.close().await.unwrap();
    }

    /// Feature 038: the pipeline tables exist, check their states, and drop steps and
    /// attempts with their item
    #[tokio::test]
    async fn pipeline_tables_check_states_and_cascade() {
        let temp_dir = TempDir::new().unwrap();
        let db = Database::new(&temp_dir.path().join("test.db"))
            .await
            .unwrap();
        let pool = db.pool();
        sqlx::query("INSERT INTO pipeline_items (key, name, source, stage, state) VALUES ('transfer:1', 'n', 'arr', 'pre_processing', 'running')")
            .execute(pool).await.unwrap();
        let bad = sqlx::query("INSERT INTO pipeline_items (key, name, source, stage, state) VALUES ('transfer:2', 'n', 'arr', 'pre_processing', 'weird')")
            .execute(pool).await;
        assert!(bad.is_err(), "state is checked");
        let step = sqlx::query("INSERT INTO pipeline_steps (item_key, stage, kind, name, state) VALUES ('transfer:1', 'post_processing', 'hook', 'tidy', 'waiting')")
            .execute(pool).await.unwrap().last_insert_rowid();
        sqlx::query("INSERT INTO pipeline_attempts (step_id, n, output) VALUES (?, 1, 'x')")
            .bind(step)
            .execute(pool)
            .await
            .unwrap();
        sqlx::query("DELETE FROM pipeline_items WHERE key = 'transfer:1'")
            .execute(pool)
            .await
            .unwrap();
        for table in ["pipeline_steps", "pipeline_attempts"] {
            let n: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(pool)
                .await
                .unwrap();
            assert_eq!(n, 0, "{table} follows its item");
        }
        db.close().await.unwrap();
    }

    /// A database from before features 039, 040, 042 and 043 gets all their tables,
    /// columns and indexes in one run, keeping its rows
    #[tokio::test]
    async fn migrate_upgrades_a_database_from_before_039_and_040_to_043() {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let tables = [
            "pipeline_holds",
            "daemon_flags",
            "pipeline_finished",
            "release_checks",
            "release_reports",
            "held_magnets",
            "hold_ends",
            "whats_new_seen",
            "storage_run_files",
            "storage_runs",
        ];

        let db = Database::new(&db_path).await.unwrap();
        db.queue()
            .add(1, "a.mkv", 10, "", "", 0, models::QueueStatus::Pending)
            .await
            .unwrap();
        for table in tables {
            sqlx::query(&format!("DROP TABLE {table}"))
                .execute(db.pool())
                .await
                .unwrap();
        }
        for (table, column) in [
            ("download_queue", "save_as"),
            ("download_queue", "fixed_path"),
            ("download_history", "local_path"),
            ("download_history", "outcome"),
        ] {
            sqlx::query(&format!("ALTER TABLE {table} DROP COLUMN {column}"))
                .execute(db.pool())
                .await
                .unwrap();
        }
        db.close().await.unwrap();

        let db = Database::new(&db_path).await.unwrap();
        for table in tables {
            let result = sqlx::query(&format!("SELECT COUNT(*) FROM {table}"))
                .fetch_one(db.pool())
                .await;
            assert!(result.is_ok(), "{table} exists");
        }
        for index in [
            "idx_download_sources_transfer_id",
            "idx_release_checks_transfer_id",
            "idx_held_magnets_next_at",
        ] {
            let (count,): (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = ?",
            )
            .bind(index)
            .fetch_one(db.pool())
            .await
            .unwrap();
            assert_eq!(count, 1, "{index}");
        }
        let item = db.queue().get_by_file_id(1).await.unwrap().unwrap();
        assert!(!item.fixed_path);
        assert_eq!(item.save_as, None);
        db.close().await.unwrap();
    }
}
