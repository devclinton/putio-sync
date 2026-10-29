//! Releases Radarr and Sonarr added through the Transmission RPC (feature 011), and the
//! Put.io files of each that the sync accepted

use super::error::{tracked, DbResult};
use putio_sync_telemetry::DbOp;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqlitePool};
use std::str::FromStr;

/// Where an item is in its life
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArrState {
    /// The Put.io transfer is queued or downloading
    Putio,
    /// The transfer finished; its files are downloading to disk
    Local,
    /// Every accepted file is on disk
    Done,
    /// Put.io failed, the transfer vanished, or the filters accepted no file
    Failed,
}

impl ArrState {
    /// Every state, in life order
    pub const ALL: [ArrState; 4] = [
        ArrState::Putio,
        ArrState::Local,
        ArrState::Done,
        ArrState::Failed,
    ];

    /// The `arr_items.state` value
    pub fn as_str(self) -> &'static str {
        match self {
            ArrState::Putio => "putio",
            ArrState::Local => "local",
            ArrState::Done => "done",
            ArrState::Failed => "failed",
        }
    }
}

impl FromStr for ArrState {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ArrState::ALL
            .into_iter()
            .find(|state| state.as_str() == s)
            .ok_or_else(|| format!("unknown arr item state '{}'", s))
    }
}

/// Whether putio-sync has asked the *arr to blocklist a failed item
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlocklistState {
    /// Nothing to do
    None,
    /// The call is due on the next refresh
    Pending,
    /// The *arr removed and blocklisted it, or no longer had it queued
    Done,
    /// Five calls failed
    GaveUp,
}

impl BlocklistState {
    /// The `arr_items.blocklist` value
    pub fn as_str(self) -> &'static str {
        match self {
            BlocklistState::None => "none",
            BlocklistState::Pending => "pending",
            BlocklistState::Done => "done",
            BlocklistState::GaveUp => "gave_up",
        }
    }
}

impl FromStr for BlocklistState {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "none" => Ok(BlocklistState::None),
            "pending" => Ok(BlocklistState::Pending),
            "done" => Ok(BlocklistState::Done),
            "gave_up" => Ok(BlocklistState::GaveUp),
            other => Err(format!("unknown blocklist state '{}'", other)),
        }
    }
}

/// One `arr_items` row
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArrItem {
    /// Transmission torrent id
    pub id: i64,
    /// Info hash, 40 lowercase hex characters
    pub hash: String,
    /// Display name
    pub name: String,
    /// Folder below the download folder (`sonarr`, `movies/4k` or empty)
    pub category: String,
    /// Put.io transfer id
    pub transfer_id: Option<i64>,
    /// Put.io file or folder the transfer produced
    pub file_id: Option<i64>,
    /// Its name on disk, under `<target>/<category>`
    pub output_name: Option<String>,
    /// Bytes
    pub total_size: i64,
    /// Last Put.io transfer status seen
    pub putio_status: Option<String>,
    /// Put.io progress, 0–100
    pub putio_percent: i64,
    /// Put.io ETA in seconds
    pub putio_eta: Option<i64>,
    /// Where the item is
    pub state: ArrState,
    /// Reported to the *arr as `errorString` when failed
    pub error: Option<String>,
    /// *arr blocklist call state
    pub blocklist: BlocklistState,
    /// Failed blocklist calls so far
    pub blocklist_attempts: i64,
}

/// What an add records
#[derive(Debug, Clone)]
pub struct NewArrItem<'a> {
    /// Info hash, lowercase hex
    pub hash: &'a str,
    /// Transfer name
    pub name: &'a str,
    /// Normalised category
    pub category: &'a str,
    /// Put.io transfer id; `None` for an item rejected before Put.io (042)
    pub transfer_id: Option<i64>,
    /// Put.io status at creation
    pub putio_status: &'a str,
    /// Size Put.io reported, 0 while unknown
    pub total_size: i64,
}

/// Put.io's view of a transfer, written on each refresh
#[derive(Debug, Clone, Default)]
pub struct PutioProgress<'a> {
    /// Transfer status (`IN_QUEUE`, `DOWNLOADING`, ...)
    pub status: &'a str,
    /// 0–100
    pub percent: i64,
    /// Seconds
    pub eta: Option<i64>,
    /// Bytes, when Put.io knows
    pub size: Option<i64>,
}

const COLUMNS: &str = "id, hash, name, category, transfer_id, file_id, output_name, total_size, \
     putio_status, putio_percent, putio_eta, state, error, blocklist, blocklist_attempts";

fn item_from_row(row: &SqliteRow) -> ArrItem {
    ArrItem {
        id: row.get("id"),
        hash: row.get("hash"),
        name: row.get("name"),
        category: row.get("category"),
        transfer_id: row.get("transfer_id"),
        file_id: row.get("file_id"),
        output_name: row.get("output_name"),
        total_size: row.get("total_size"),
        putio_status: row.get("putio_status"),
        putio_percent: row.get("putio_percent"),
        putio_eta: row.get("putio_eta"),
        // The CHECK constraints keep both columns to known values
        state: row
            .get::<String, _>("state")
            .parse()
            .unwrap_or(ArrState::Failed),
        error: row.get("error"),
        blocklist: row
            .get::<String, _>("blocklist")
            .parse()
            .unwrap_or(BlocklistState::None),
        blocklist_attempts: row.get("blocklist_attempts"),
    }
}

/// Repository for `arr_items` and `arr_item_files`
pub struct ArrRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> ArrRepository<'a> {
    /// Create a new arr repository
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Record a new item in state `putio`; returns its id
    pub async fn insert(&self, item: &NewArrItem<'_>) -> DbResult<i64> {
        let result = sqlx::query(
            r#"
            INSERT INTO arr_items (hash, name, category, transfer_id, putio_status, total_size, state)
            VALUES (?, ?, ?, ?, ?, ?, 'putio')
            "#,
        )
        .bind(item.hash)
        .bind(item.name)
        .bind(item.category)
        .bind(item.transfer_id)
        .bind(item.putio_status)
        .bind(item.total_size)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to insert arr item"))?;
        Ok(result.last_insert_rowid())
    }

    /// The item with this hash (lowercase hex)
    pub async fn get_by_hash(&self, hash: &str) -> DbResult<Option<ArrItem>> {
        let row = sqlx::query(&format!("SELECT {COLUMNS} FROM arr_items WHERE hash = ?"))
            .bind(hash)
            .fetch_optional(self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to get arr item"))?;
        Ok(row.as_ref().map(item_from_row))
    }

    /// The item whose Put.io folder or accepted files include `file_id`
    pub async fn get_by_file_id(&self, file_id: i64) -> DbResult<Option<ArrItem>> {
        let sql = format!(
            "SELECT {COLUMNS} FROM arr_items WHERE file_id = ? \
             OR hash IN (SELECT hash FROM arr_item_files WHERE file_id = ?) LIMIT 1"
        );
        let row = sqlx::query(&sql)
            .bind(file_id)
            .bind(file_id)
            .fetch_optional(self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to get arr item by file"))?;
        Ok(row.as_ref().map(item_from_row))
    }

    /// Which of `file_ids` belong to an item, for the UI's "fail and blocklist" (042 US6)
    pub async fn arr_file_ids(&self, file_ids: &[i64]) -> DbResult<Vec<i64>> {
        if file_ids.is_empty() {
            return Ok(Vec::new());
        }
        let list = vec!["?"; file_ids.len()].join(",");
        let sql = format!(
            "SELECT file_id FROM arr_items WHERE file_id IN ({list}) \
             UNION SELECT file_id FROM arr_item_files WHERE file_id IN ({list})"
        );
        let mut query = sqlx::query_scalar(&sql);
        for _ in 0..2 {
            for id in file_ids {
                query = query.bind(id);
            }
        }
        query
            .fetch_all(self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to look up arr files"))
    }

    /// The item with this Transmission id
    pub async fn get_by_id(&self, id: i64) -> DbResult<Option<ArrItem>> {
        let row = sqlx::query(&format!("SELECT {COLUMNS} FROM arr_items WHERE id = ?"))
            .bind(id)
            .fetch_optional(self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to get arr item"))?;
        Ok(row.as_ref().map(item_from_row))
    }

    /// Every item, oldest first
    pub async fn list(&self) -> DbResult<Vec<ArrItem>> {
        let rows = sqlx::query(&format!("SELECT {COLUMNS} FROM arr_items ORDER BY id"))
            .fetch_all(self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to list arr items"))?;
        Ok(rows.iter().map(item_from_row).collect())
    }

    /// The categories items have used, sorted
    pub async fn categories(&self) -> DbResult<Vec<String>> {
        sqlx::query_scalar("SELECT DISTINCT category FROM arr_items ORDER BY category")
            .fetch_all(self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to list arr categories"))
    }

    /// Store Put.io's view of the transfer. A known size replaces `total_size`.
    pub async fn update_putio(&self, hash: &str, progress: &PutioProgress<'_>) -> DbResult<()> {
        sqlx::query(
            r#"
            UPDATE arr_items SET
                putio_status = ?, putio_percent = ?, putio_eta = ?,
                total_size = COALESCE(?, total_size), updated_at = CURRENT_TIMESTAMP
            WHERE hash = ?
            "#,
        )
        .bind(progress.status)
        .bind(progress.percent.clamp(0, 100))
        .bind(progress.eta)
        .bind(progress.size)
        .bind(hash)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to update arr item"))?;
        Ok(())
    }

    /// The transfer finished: store its Put.io file, its name on disk, the accepted
    /// files and their total size, and move the item to `local`
    pub async fn set_local(
        &self,
        hash: &str,
        file_id: i64,
        output_name: &str,
        files: &[(i64, i64)],
    ) -> DbResult<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(tracked(DbOp::Other, "Failed to start arr transaction"))?;
        sqlx::query("DELETE FROM arr_item_files WHERE hash = ?")
            .bind(hash)
            .execute(&mut *tx)
            .await
            .map_err(tracked(DbOp::Other, "Failed to clear arr item files"))?;
        for (file_id, size) in files {
            sqlx::query("INSERT INTO arr_item_files (hash, file_id, size) VALUES (?, ?, ?)")
                .bind(hash)
                .bind(file_id)
                .bind(size)
                .execute(&mut *tx)
                .await
                .map_err(tracked(DbOp::Other, "Failed to insert arr item file"))?;
        }
        let total: i64 = files.iter().map(|(_, size)| size).sum();
        sqlx::query(
            r#"
            UPDATE arr_items SET
                file_id = ?, output_name = ?, name = ?, total_size = ?,
                state = 'local', error = NULL, updated_at = CURRENT_TIMESTAMP
            WHERE hash = ?
            "#,
        )
        .bind(file_id)
        .bind(output_name)
        .bind(output_name)
        .bind(total)
        .bind(hash)
        .execute(&mut *tx)
        .await
        .map_err(tracked(DbOp::Other, "Failed to update arr item"))?;
        tx.commit()
            .await
            .map_err(tracked(DbOp::Other, "Failed to commit arr item"))?;
        Ok(())
    }

    /// The accepted files of an item: `(file_id, size)`
    pub async fn files(&self, hash: &str) -> DbResult<Vec<(i64, i64)>> {
        let rows =
            sqlx::query("SELECT file_id, size FROM arr_item_files WHERE hash = ? ORDER BY file_id")
                .bind(hash)
                .fetch_all(self.pool)
                .await
                .map_err(tracked(DbOp::Other, "Failed to list arr item files"))?;
        Ok(rows
            .iter()
            .map(|row| (row.get("file_id"), row.get("size")))
            .collect())
    }

    /// Move an item to `state`, setting or clearing its error. Entering `failed` with
    /// `blocklist` set marks the *arr blocklist call as due.
    pub async fn set_state(
        &self,
        hash: &str,
        state: ArrState,
        error: Option<&str>,
        blocklist: bool,
    ) -> DbResult<()> {
        sqlx::query(
            r#"
            UPDATE arr_items SET
                state = ?, error = ?,
                blocklist = CASE WHEN ? AND blocklist = 'none' THEN 'pending' ELSE blocklist END,
                updated_at = CURRENT_TIMESTAMP
            WHERE hash = ?
            "#,
        )
        .bind(state.as_str())
        .bind(error)
        .bind(blocklist && state == ArrState::Failed)
        .bind(hash)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to update arr item state"))?;
        Ok(())
    }

    /// Store the outcome of a blocklist call
    pub async fn set_blocklist(
        &self,
        hash: &str,
        blocklist: BlocklistState,
        attempts: i64,
    ) -> DbResult<()> {
        sqlx::query("UPDATE arr_items SET blocklist = ?, blocklist_attempts = ? WHERE hash = ?")
            .bind(blocklist.as_str())
            .bind(attempts)
            .bind(hash)
            .execute(self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to update arr blocklist state"))?;
        Ok(())
    }

    /// Forget an item and its files
    pub async fn delete(&self, hash: &str) -> DbResult<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(tracked(DbOp::Other, "Failed to start arr transaction"))?;
        for sql in [
            "DELETE FROM arr_item_files WHERE hash = ?",
            "DELETE FROM arr_items WHERE hash = ?",
        ] {
            sqlx::query(sql)
                .bind(hash)
                .execute(&mut *tx)
                .await
                .map_err(tracked(DbOp::Other, "Failed to delete arr item"))?;
        }
        tx.commit()
            .await
            .map_err(tracked(DbOp::Other, "Failed to commit arr item delete"))?;
        Ok(())
    }

    /// Bytes of the item's files already completed on disk, and whether all of them
    /// are (true for an item with no files)
    pub async fn local_progress(&self, hash: &str) -> DbResult<(i64, bool)> {
        let row = sqlx::query(
            r#"
            SELECT
                COALESCE(SUM(CASE WHEN done THEN size ELSE 0 END), 0) AS completed_bytes,
                COALESCE(MIN(done), 1) AS all_done
            FROM (
                SELECT f.size, EXISTS (
                    SELECT 1 FROM downloads d
                    WHERE d.file_id = f.file_id AND d.status = 'completed'
                ) AS done
                FROM arr_item_files f WHERE f.hash = ?
            )
            "#,
        )
        .bind(hash)
        .fetch_one(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to read arr item progress"))?;
        Ok((
            row.get("completed_bytes"),
            row.get::<i64, _>("all_done") != 0,
        ))
    }

    /// Number of items in each state
    pub async fn counts_by_state(&self) -> DbResult<Vec<(ArrState, i64)>> {
        let rows = sqlx::query("SELECT state, COUNT(*) AS n FROM arr_items GROUP BY state")
            .fetch_all(self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to count arr items"))?;
        let mut counts: Vec<(ArrState, i64)> = ArrState::ALL.into_iter().map(|s| (s, 0)).collect();
        for row in rows {
            if let Ok(state) = row.get::<String, _>("state").parse::<ArrState>() {
                if let Some(entry) = counts.iter_mut().find(|(s, _)| *s == state) {
                    entry.1 = row.get("n");
                }
            }
        }
        Ok(counts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::DownloadStatus;
    use crate::Database;
    use tempfile::TempDir;

    async fn db() -> (TempDir, Database) {
        let dir = TempDir::new().unwrap();
        let db = Database::new(dir.path().join("arr.db")).await.unwrap();
        (dir, db)
    }

    fn new_item(hash: &str) -> NewArrItem<'_> {
        NewArrItem {
            hash,
            name: "Show.S01E01",
            category: "sonarr",
            transfer_id: Some(77),
            putio_status: "IN_QUEUE",
            total_size: 0,
        }
    }

    const HASH: &str = "0123456789abcdef0123456789abcdef01234567";

    #[tokio::test]
    async fn insert_get_and_list() {
        let (_dir, db) = db().await;
        let repo = db.arr();
        let id = repo.insert(&new_item(HASH)).await.unwrap();

        let item = repo.get_by_hash(HASH).await.unwrap().unwrap();
        assert_eq!(item.id, id);
        assert_eq!(item.state, ArrState::Putio);
        assert_eq!(item.blocklist, BlocklistState::None);
        assert_eq!(item.transfer_id, Some(77));
        assert_eq!(repo.get_by_id(id).await.unwrap(), Some(item.clone()));
        assert_eq!(repo.list().await.unwrap(), vec![item]);
        assert!(repo.get_by_hash("nope").await.unwrap().is_none());

        // The hash is unique
        assert!(repo.insert(&new_item(HASH)).await.is_err());
    }

    #[tokio::test]
    async fn putio_progress_and_local_files() {
        let (_dir, db) = db().await;
        let repo = db.arr();
        repo.insert(&new_item(HASH)).await.unwrap();

        repo.update_putio(
            HASH,
            &PutioProgress {
                status: "DOWNLOADING",
                percent: 140,
                eta: Some(60),
                size: Some(1000),
            },
        )
        .await
        .unwrap();
        let item = repo.get_by_hash(HASH).await.unwrap().unwrap();
        assert_eq!(item.putio_percent, 100);
        assert_eq!(item.total_size, 1000);
        repo.update_putio(
            HASH,
            &PutioProgress {
                status: "DOWNLOADING",
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(
            repo.get_by_hash(HASH).await.unwrap().unwrap().total_size,
            1000
        );

        repo.set_local(HASH, 500, "Show.S01E01", &[(11, 300), (12, 600)])
            .await
            .unwrap();
        let item = repo.get_by_hash(HASH).await.unwrap().unwrap();
        assert_eq!(item.state, ArrState::Local);
        assert_eq!(item.file_id, Some(500));
        assert_eq!(item.output_name.as_deref(), Some("Show.S01E01"));
        assert_eq!(item.total_size, 900);
        assert_eq!(repo.files(HASH).await.unwrap(), vec![(11, 300), (12, 600)]);

        assert_eq!(repo.local_progress(HASH).await.unwrap(), (0, false));
        let downloads = db.downloads();
        downloads
            .insert(11, "a.mkv", 300, "/t/a.mkv", DownloadStatus::Completed)
            .await
            .unwrap();
        downloads
            .insert(12, "b.nfo", 600, "/t/b.nfo", DownloadStatus::InProgress)
            .await
            .unwrap();
        assert_eq!(repo.local_progress(HASH).await.unwrap(), (300, false));
        downloads
            .insert(12, "b.nfo", 601, "/t/b.nfo", DownloadStatus::Completed)
            .await
            .unwrap();
        assert_eq!(repo.local_progress(HASH).await.unwrap(), (900, true));
    }

    #[tokio::test]
    async fn failure_marks_blocklist_once() {
        let (_dir, db) = db().await;
        let repo = db.arr();
        repo.insert(&new_item(HASH)).await.unwrap();

        repo.set_state(HASH, ArrState::Failed, Some("Put.io: dead torrent"), true)
            .await
            .unwrap();
        let item = repo.get_by_hash(HASH).await.unwrap().unwrap();
        assert_eq!(item.state, ArrState::Failed);
        assert_eq!(item.error.as_deref(), Some("Put.io: dead torrent"));
        assert_eq!(item.blocklist, BlocklistState::Pending);

        repo.set_blocklist(HASH, BlocklistState::Done, 1)
            .await
            .unwrap();
        // A later failure doesn't queue a second call
        repo.set_state(HASH, ArrState::Failed, Some("again"), true)
            .await
            .unwrap();
        let item = repo.get_by_hash(HASH).await.unwrap().unwrap();
        assert_eq!(item.blocklist, BlocklistState::Done);
        assert_eq!(item.blocklist_attempts, 1);

        // Recovery clears the error
        repo.set_state(HASH, ArrState::Putio, None, false)
            .await
            .unwrap();
        assert_eq!(repo.get_by_hash(HASH).await.unwrap().unwrap().error, None);
    }

    #[tokio::test]
    async fn delete_and_counts() {
        let (_dir, db) = db().await;
        let repo = db.arr();
        let other = "fedcba9876543210fedcba9876543210fedcba98";
        repo.insert(&new_item(HASH)).await.unwrap();
        repo.insert(&new_item(other)).await.unwrap();
        repo.set_local(other, 5, "x", &[(1, 1)]).await.unwrap();

        let counts = repo.counts_by_state().await.unwrap();
        assert_eq!(
            counts,
            vec![
                (ArrState::Putio, 1),
                (ArrState::Local, 1),
                (ArrState::Done, 0),
                (ArrState::Failed, 0)
            ]
        );

        repo.delete(other).await.unwrap();
        assert!(repo.get_by_hash(other).await.unwrap().is_none());
        assert!(repo.files(other).await.unwrap().is_empty());
        // No files: nothing left to wait for
        assert_eq!(repo.local_progress(other).await.unwrap(), (0, true));
    }
}
