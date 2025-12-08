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
//!     let entries = history.query(None, 10, 0).await?;
//!     Ok(())
//! }
//! ```

pub mod downloads;
pub mod error;
pub mod history;
pub mod models;
pub mod queue;
pub mod schema;
pub mod stats;
pub mod torrents;

pub use downloads::DownloadRepository;
pub use error::{DbError, DbResult};
pub use history::HistoryRepository;
pub use queue::{QueueRepository, QueueStats};
pub use stats::{ExtensionStats, StatsRepository};
pub use torrents::ProcessedTorrentsRepository;

use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use std::path::Path;
use std::str::FromStr;
use tracing::{debug, info};

/// Database connection pool and management
pub struct Database {
    pool: SqlitePool,
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
                std::fs::create_dir_all(parent).map_err(|e| {
                    DbError::DatabaseError(format!("Failed to create database directory: {}", e))
                })?;
            }
        }

        debug!("Opening database at: {}", path.display());

        // Create connection options
        let options = SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))
            .map_err(|e| DbError::DatabaseError(format!("Invalid database path: {}", e)))?
            .create_if_missing(true);

        // Create connection pool
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .map_err(|e| DbError::DatabaseError(format!("Failed to connect to database: {}", e)))?;

        let db = Database { pool };

        // Run migrations
        db.migrate().await?;

        info!("Database initialized at: {}", path.display());

        Ok(db)
    }

    /// Get the underlying connection pool
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Get a download repository
    pub fn downloads(&self) -> DownloadRepository {
        DownloadRepository::new(&self.pool)
    }

    /// Get a history repository
    pub fn history(&self) -> HistoryRepository {
        HistoryRepository::new(&self.pool)
    }

    /// Get a stats repository
    pub fn stats(&self) -> StatsRepository {
        StatsRepository::new(&self.pool)
    }

    /// Get a queue repository
    pub fn queue(&self) -> QueueRepository {
        QueueRepository::new(&self.pool)
    }

    /// Get a processed torrents repository
    pub fn processed_torrents(&self) -> ProcessedTorrentsRepository {
        ProcessedTorrentsRepository::new(&self.pool)
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
            .map_err(|e| {
                DbError::DatabaseError(format!("Failed to create downloads table: {}", e))
            })?;

        // Create download_history table
        sqlx::query(schema::CREATE_DOWNLOAD_HISTORY_TABLE)
            .execute(&self.pool)
            .await
            .map_err(|e| {
                DbError::DatabaseError(format!("Failed to create download_history table: {}", e))
            })?;

        // Create download_stats table
        sqlx::query(schema::CREATE_DOWNLOAD_STATS_TABLE)
            .execute(&self.pool)
            .await
            .map_err(|e| {
                DbError::DatabaseError(format!("Failed to create download_stats table: {}", e))
            })?;

        // Initialize stats table if empty
        sqlx::query(schema::INIT_DOWNLOAD_STATS)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::DatabaseError(format!("Failed to initialize stats: {}", e)))?;

        // Create download_queue table
        sqlx::query(schema::CREATE_DOWNLOAD_QUEUE_TABLE)
            .execute(&self.pool)
            .await
            .map_err(|e| {
                DbError::DatabaseError(format!("Failed to create download_queue table: {}", e))
            })?;

        // Create processed_torrents table
        sqlx::query(schema::CREATE_PROCESSED_TORRENTS_TABLE)
            .execute(&self.pool)
            .await
            .map_err(|e| {
                DbError::DatabaseError(format!("Failed to create processed_torrents table: {}", e))
            })?;

        // Create performance indexes
        sqlx::query(schema::CREATE_INDEXES)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::DatabaseError(format!("Failed to create indexes: {}", e)))?;

        info!("Database migrations completed");

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
