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
pub mod queue;
pub mod releases;
pub mod schema;
pub mod sources;
pub mod stats;
pub mod torrents;

pub use arr::{ArrItem, ArrRepository, ArrState, BlocklistState, NewArrItem, PutioProgress};
pub use decisions::{RuleDecisionRecord, RuleDecisionsRepository};
pub use downloads::DownloadRepository;
pub use error::{DbError, DbResult};
pub use held::{Held, HeldRepository, HoldStats, NewHeld};
pub use history::HistoryRepository;
pub use queue::{QueueRepository, QueueStats};
pub use releases::{
    CheckState, FlagUndo, HeldSettle, NewCheck, NewReport, PendingCheck, ReleaseStats,
    ReleasesRepository, ReportKind, ReportRow,
};
pub use sources::SourcesRepository;
pub use stats::{ExtensionStats, StatsRepository};
pub use torrents::ProcessedTorrentsRepository;

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
        ] {
            sqlx::query(sql)
                .execute(&self.pool)
                .await
                .map_err(tracked(DbOp::Migrate, context))?;
        }

        // Create performance indexes
        sqlx::query(schema::CREATE_INDEXES)
            .execute(&self.pool)
            .await
            .map_err(tracked(DbOp::Migrate, "Failed to create indexes"))?;

        info!("Database migrations completed");

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
}
