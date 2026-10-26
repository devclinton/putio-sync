//! Where downloads came from (feature 006): the source of each queued Put.io file and
//! the origin of transfers putio-sync created itself

use super::error::{tracked, DbResult};
use super::models::{DownloadSource, Origin, SourceType, TransferOrigin};
use putio_sync_telemetry::DbOp;
use sqlx::{Row, SqlitePool};

/// Repository for `download_sources` and `transfer_origins`
pub struct SourcesRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> SourcesRepository<'a> {
    /// Create a new sources repository
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Record (or replace) where a queued file came from
    pub async fn record(&self, file_id: i64, source: &DownloadSource) -> DbResult<()> {
        sqlx::query(
            r#"
            INSERT INTO download_sources
                (file_id, transfer_id, source_type, tracker_host, is_private, origin, watch_folder, putio_folder)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(file_id) DO UPDATE SET
                transfer_id = excluded.transfer_id,
                source_type = excluded.source_type,
                tracker_host = excluded.tracker_host,
                is_private = excluded.is_private,
                origin = excluded.origin,
                watch_folder = excluded.watch_folder,
                putio_folder = excluded.putio_folder,
                recorded_at = CURRENT_TIMESTAMP
            "#,
        )
        .bind(file_id)
        .bind(source.transfer_id)
        .bind(source.source_type.map(SourceType::as_str))
        .bind(source.tracker_host.as_deref())
        .bind(source.is_private)
        .bind(source.origin.as_str())
        .bind(source.watch_folder.as_deref())
        .bind(source.putio_folder.as_deref())
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to record download source"))?;
        Ok(())
    }

    /// The recorded source of a file, if any
    pub async fn get(&self, file_id: i64) -> DbResult<Option<DownloadSource>> {
        let row = sqlx::query(
            r#"
            SELECT transfer_id, source_type, tracker_host, is_private, origin, watch_folder, putio_folder
            FROM download_sources WHERE file_id = ?
            "#,
        )
        .bind(file_id)
        .fetch_optional(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to get download source"))?;

        Ok(row.map(|row| DownloadSource {
            transfer_id: row.get("transfer_id"),
            source_type: row
                .get::<Option<String>, _>("source_type")
                .and_then(|s| s.parse().ok()),
            tracker_host: row.get("tracker_host"),
            is_private: row.get("is_private"),
            origin: row
                .get::<String, _>("origin")
                .parse()
                .unwrap_or(Origin::Unknown),
            watch_folder: row.get("watch_folder"),
            putio_folder: row.get("putio_folder"),
        }))
    }

    /// Record that putio-sync created a transfer, from a watch folder or the web UI
    pub async fn record_transfer_origin(
        &self,
        transfer_id: i64,
        origin: &TransferOrigin,
    ) -> DbResult<()> {
        sqlx::query(
            r#"
            INSERT INTO transfer_origins (transfer_id, origin, watch_folder)
            VALUES (?, ?, ?)
            ON CONFLICT(transfer_id) DO UPDATE SET
                origin = excluded.origin,
                watch_folder = excluded.watch_folder
            "#,
        )
        .bind(transfer_id)
        .bind(origin.origin.as_str())
        .bind(origin.watch_folder.as_deref())
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to record transfer origin"))?;
        Ok(())
    }

    /// How putio-sync created a transfer, if it did
    pub async fn transfer_origin(&self, transfer_id: i64) -> DbResult<Option<TransferOrigin>> {
        let row =
            sqlx::query("SELECT origin, watch_folder FROM transfer_origins WHERE transfer_id = ?")
                .bind(transfer_id)
                .fetch_optional(self.pool)
                .await
                .map_err(tracked(DbOp::Other, "Failed to get transfer origin"))?;

        Ok(row.and_then(|row| {
            let origin = row.get::<String, _>("origin").parse().ok()?;
            Some(TransferOrigin {
                origin,
                watch_folder: row.get("watch_folder"),
            })
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;
    use tempfile::TempDir;

    async fn setup_test_db() -> (TempDir, Database) {
        let temp_dir = TempDir::new().unwrap();
        let db = Database::new(&temp_dir.path().join("test.db"))
            .await
            .unwrap();
        (temp_dir, db)
    }

    fn torrent() -> DownloadSource {
        DownloadSource {
            transfer_id: Some(9),
            source_type: Some(SourceType::Torrent),
            tracker_host: Some("tracker.example.org".into()),
            is_private: true,
            origin: Origin::Watch,
            watch_folder: Some("/watch/tv".into()),
            putio_folder: Some("TV".into()),
        }
    }

    #[tokio::test]
    async fn test_record_and_get() {
        let (_dir, db) = setup_test_db().await;
        let repo = db.sources();

        assert_eq!(repo.get(1).await.unwrap(), None);
        repo.record(1, &torrent()).await.unwrap();
        assert_eq!(repo.get(1).await.unwrap(), Some(torrent()));
    }

    #[tokio::test]
    async fn test_record_replaces() {
        let (_dir, db) = setup_test_db().await;
        let repo = db.sources();
        repo.record(1, &torrent()).await.unwrap();

        let unknown = DownloadSource {
            transfer_id: None,
            source_type: None,
            tracker_host: None,
            is_private: false,
            origin: Origin::Unknown,
            watch_folder: None,
            putio_folder: Some("(root)".into()),
        };
        repo.record(1, &unknown).await.unwrap();
        assert_eq!(repo.get(1).await.unwrap(), Some(unknown));
    }

    #[tokio::test]
    async fn test_transfer_origin() {
        let (_dir, db) = setup_test_db().await;
        let repo = db.sources();
        assert_eq!(repo.transfer_origin(5).await.unwrap(), None);

        let manual = TransferOrigin {
            origin: Origin::Manual,
            watch_folder: None,
        };
        repo.record_transfer_origin(5, &manual).await.unwrap();
        assert_eq!(repo.transfer_origin(5).await.unwrap(), Some(manual));
    }
}
