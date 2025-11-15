//! Database module for local state management
//!
//! This module provides SQLite-based storage for tracking downloads,
//! history, statistics, and chunk state for resumable downloads.

pub mod downloads;
pub mod history;
pub mod models;
pub mod schema;
pub mod stats;

pub use downloads::DownloadRepository;
pub use history::HistoryRepository;
pub use stats::{ExtensionStats, StatsRepository};

use crate::error::{CoreError, CoreResult};
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
    /// use putio_sync_core::db::Database;
    /// use std::path::PathBuf;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let db = Database::new(&PathBuf::from("putio.db")).await.unwrap();
    /// }
    /// ```
    pub async fn new<P: AsRef<Path>>(path: P) -> CoreResult<Self> {
        let path = path.as_ref();

        // Create parent directory if it doesn't exist
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    CoreError::DatabaseError(format!("Failed to create database directory: {}", e))
                })?;
            }
        }

        debug!("Opening database at: {}", path.display());

        // Create connection options
        let options = SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))
            .map_err(|e| CoreError::DatabaseError(format!("Invalid database path: {}", e)))?
            .create_if_missing(true);

        // Create connection pool
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await
            .map_err(|e| CoreError::DatabaseError(format!("Failed to connect to database: {}", e)))?;

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

    /// Run database migrations
    ///
    /// This creates all necessary tables if they don't exist.
    async fn migrate(&self) -> CoreResult<()> {
        debug!("Running database migrations");

        // Create downloads table
        sqlx::query(schema::CREATE_DOWNLOADS_TABLE)
            .execute(&self.pool)
            .await
            .map_err(|e| CoreError::DatabaseError(format!("Failed to create downloads table: {}", e)))?;

        // Create download_chunks table
        sqlx::query(schema::CREATE_DOWNLOAD_CHUNKS_TABLE)
            .execute(&self.pool)
            .await
            .map_err(|e| CoreError::DatabaseError(format!("Failed to create download_chunks table: {}", e)))?;

        // Create download_history table
        sqlx::query(schema::CREATE_DOWNLOAD_HISTORY_TABLE)
            .execute(&self.pool)
            .await
            .map_err(|e| CoreError::DatabaseError(format!("Failed to create download_history table: {}", e)))?;

        // Create download_stats table
        sqlx::query(schema::CREATE_DOWNLOAD_STATS_TABLE)
            .execute(&self.pool)
            .await
            .map_err(|e| CoreError::DatabaseError(format!("Failed to create download_stats table: {}", e)))?;

        // Initialize stats table if empty
        sqlx::query(schema::INIT_DOWNLOAD_STATS)
            .execute(&self.pool)
            .await
            .map_err(|e| CoreError::DatabaseError(format!("Failed to initialize stats: {}", e)))?;

        // Create performance indexes
        sqlx::query(schema::CREATE_INDEXES)
            .execute(&self.pool)
            .await
            .map_err(|e| CoreError::DatabaseError(format!("Failed to create indexes: {}", e)))?;

        info!("Database migrations completed");

        Ok(())
    }

    /// Close the database connection
    pub async fn close(self) -> CoreResult<()> {
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

        let result = sqlx::query("SELECT COUNT(*) FROM download_chunks")
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
