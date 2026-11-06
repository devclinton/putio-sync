//! History repository for managing download and delete history

use super::error::{tracked, DbResult};
use super::models::{DownloadHistory, HistoryAction};
use putio_sync_telemetry::DbOp;
use sqlx::SqlitePool;
use tracing::debug;

/// What was downloaded on one UTC day (feature 041)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyTotals {
    /// The UTC day
    pub day: chrono::NaiveDate,
    /// Downloads finished that day
    pub downloads: i64,
    /// Bytes of those downloads
    pub bytes: i64,
    /// Seconds spent downloading them
    pub download_seconds: i64,
    /// `bytes` over `download_seconds`, 0 when no time was recorded
    pub avg_speed_bytes_per_s: i64,
}

/// Repository for managing history records
pub struct HistoryRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> HistoryRepository<'a> {
    /// Create a new history repository
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Insert a new history record
    ///
    /// # Arguments
    ///
    /// * `file_id` - Put.io file ID
    /// * `filename` - Original filename
    /// * `file_size` - File size in bytes
    /// * `action` - Action performed (downloaded or deleted)
    /// * `duration_seconds` - Optional duration in seconds (for downloads)
    /// * `average_speed` - Optional average speed in bytes/sec (for downloads)
    pub async fn insert(
        &self,
        file_id: i64,
        filename: &str,
        file_size: i64,
        action: HistoryAction,
        duration_seconds: Option<i64>,
        average_speed: Option<i64>,
    ) -> DbResult<i64> {
        // Extract file extension (normalized to lowercase)
        let extension = std::path::Path::new(filename)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_lowercase());

        debug!(
            "Inserting history: file_id={}, filename={}, action={}",
            file_id, filename, action
        );

        let result = sqlx::query(
            r#"
            INSERT INTO download_history (file_id, filename, file_size, extension, action, duration_seconds, average_speed)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(file_id)
        .bind(filename)
        .bind(file_size)
        .bind(extension)
        .bind(action.to_string())
        .bind(duration_seconds)
        .bind(average_speed)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::History, "Failed to insert history"))?;

        Ok(result.last_insert_rowid())
    }

    /// Record where a history row's download was written and what happened to a file
    /// that already existed there (feature 040)
    pub async fn set_outcome(
        &self,
        id: i64,
        local_path: Option<&str>,
        outcome: Option<&str>,
    ) -> DbResult<()> {
        sqlx::query("UPDATE download_history SET local_path = ?, outcome = ? WHERE id = ?")
            .bind(local_path)
            .bind(outcome)
            .bind(id)
            .execute(self.pool)
            .await
            .map_err(tracked(DbOp::History, "Failed to set history outcome"))?;
        Ok(())
    }

    /// Record how a history row's download was checked and how many re-downloads it
    /// took (feature 034)
    pub async fn set_verify(&self, id: i64, verify: &str, redownloads: u32) -> DbResult<()> {
        sqlx::query("UPDATE download_history SET verify = ?, redownloads = ? WHERE id = ?")
            .bind(verify)
            .bind(redownloads as i64)
            .bind(id)
            .execute(self.pool)
            .await
            .map_err(tracked(DbOp::History, "Failed to set history verify"))?;
        Ok(())
    }

    /// Insert a finished download with an explicit finish time (UTC). Used to seed
    /// tests and the mock UI with history spread over time (feature 006, R10).
    pub async fn insert_download_at(
        &self,
        file_id: i64,
        filename: &str,
        file_size: i64,
        duration_seconds: Option<i64>,
        average_speed: Option<i64>,
        timestamp: chrono::DateTime<chrono::Utc>,
    ) -> DbResult<i64> {
        let extension = std::path::Path::new(filename)
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_lowercase());

        let result = sqlx::query(
            r#"
            INSERT INTO download_history (file_id, filename, file_size, extension, action, duration_seconds, average_speed, timestamp)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(file_id)
        .bind(filename)
        .bind(file_size)
        .bind(extension)
        .bind(HistoryAction::Downloaded.to_string())
        .bind(duration_seconds)
        .bind(average_speed)
        .bind(timestamp.format("%Y-%m-%d %H:%M:%S").to_string())
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::History, "Failed to insert history"))?;

        Ok(result.last_insert_rowid())
    }

    /// Query history with optional filters, newest first
    ///
    /// # Arguments
    ///
    /// * `action` - Optional action filter (downloaded or deleted)
    /// * `limit` - Maximum number of results (default: 50)
    /// * `offset` - Number of results to skip (for pagination)
    /// * `name` - Optional case-insensitive filename fragment; `%`, `_` and `\` match
    ///   literally, and an empty fragment is no filter
    pub async fn query(
        &self,
        action: Option<HistoryAction>,
        limit: i64,
        offset: i64,
        name: Option<&str>,
    ) -> DbResult<Vec<DownloadHistory>> {
        let (filter, bindings) = filters(action, name);
        let sql = format!(
            "SELECT * FROM download_history WHERE 1=1{filter} \
             ORDER BY timestamp DESC, id DESC LIMIT ? OFFSET ?"
        );
        let mut q = sqlx::query_as::<_, DownloadHistory>(&sql);
        for binding in bindings {
            q = q.bind(binding);
        }
        let history = q
            .bind(limit)
            .bind(offset)
            .fetch_all(self.pool)
            .await
            .map_err(tracked(DbOp::History, "Failed to query history"))?;

        Ok(history)
    }

    /// One history row
    pub async fn get(&self, id: i64) -> DbResult<Option<DownloadHistory>> {
        sqlx::query_as::<_, DownloadHistory>("SELECT * FROM download_history WHERE id = ?")
            .bind(id)
            .fetch_optional(self.pool)
            .await
            .map_err(tracked(DbOp::History, "Failed to get history row"))
    }

    /// The latest history row of a Put.io file
    pub async fn latest_for_file(&self, file_id: i64) -> DbResult<Option<DownloadHistory>> {
        sqlx::query_as::<_, DownloadHistory>(
            "SELECT * FROM download_history WHERE file_id = ? ORDER BY timestamp DESC, id DESC LIMIT 1",
        )
        .bind(file_id)
        .fetch_optional(self.pool)
        .await
        .map_err(tracked(DbOp::History, "Failed to get history row"))
    }

    /// Query history with date range filter
    ///
    /// # Arguments
    ///
    /// * `since` - Optional start date (ISO 8601 format)
    /// * `until` - Optional end date (ISO 8601 format)
    /// * `action` - Optional action filter
    /// * `limit` - Maximum number of results
    /// * `offset` - Number of results to skip
    pub async fn query_by_date(
        &self,
        since: Option<&str>,
        until: Option<&str>,
        action: Option<HistoryAction>,
        limit: i64,
        offset: i64,
    ) -> DbResult<Vec<DownloadHistory>> {
        let mut query = String::from("SELECT * FROM download_history WHERE 1=1");
        let mut bindings: Vec<String> = Vec::new();

        if let Some(since_date) = since {
            query.push_str(" AND timestamp >= ?");
            bindings.push(since_date.to_string());
        }

        if let Some(until_date) = until {
            query.push_str(" AND timestamp <= ?");
            bindings.push(until_date.to_string());
        }

        if let Some(action_filter) = &action {
            query.push_str(" AND action = ?");
            bindings.push(action_filter.to_string());
        }

        query.push_str(" ORDER BY timestamp DESC LIMIT ? OFFSET ?");

        let mut q = sqlx::query_as::<_, DownloadHistory>(&query);
        for binding in &bindings {
            q = q.bind(binding);
        }
        q = q.bind(limit).bind(offset);

        let history = q
            .fetch_all(self.pool)
            .await
            .map_err(tracked(DbOp::History, "Failed to query history by date"))?;

        Ok(history)
    }

    /// Count history entries matching the same filters as [`Self::query`]
    ///
    /// # Arguments
    ///
    /// * `action` - Optional action filter
    /// * `name` - Optional filename fragment, as in [`Self::query`]
    pub async fn count(&self, action: Option<HistoryAction>, name: Option<&str>) -> DbResult<i64> {
        let (filter, bindings) = filters(action, name);
        let sql = format!("SELECT COUNT(*) FROM download_history WHERE 1=1{filter}");
        let mut q = sqlx::query_as::<_, (i64,)>(&sql);
        for binding in bindings {
            q = q.bind(binding);
        }
        let count = q
            .fetch_one(self.pool)
            .await
            .map_err(tracked(DbOp::History, "Failed to count history"))?;

        Ok(count.0)
    }

    /// Downloads per UTC day between `from` and `to`, both inclusive and both optional,
    /// oldest first. Days without downloads are left out. History timestamps are UTC.
    pub async fn daily_totals(
        &self,
        from: Option<chrono::NaiveDate>,
        to: Option<chrono::NaiveDate>,
    ) -> DbResult<Vec<DailyTotals>> {
        let rows: Vec<(String, i64, i64, i64)> = sqlx::query_as(
            r#"
            SELECT date(timestamp) AS day,
                   COUNT(*),
                   COALESCE(SUM(file_size), 0),
                   COALESCE(SUM(duration_seconds), 0)
            FROM download_history
            WHERE action = 'downloaded'
              AND (?1 IS NULL OR date(timestamp) >= ?1)
              AND (?2 IS NULL OR date(timestamp) <= ?2)
            GROUP BY day
            ORDER BY day
            "#,
        )
        .bind(from.map(|d| d.to_string()))
        .bind(to.map(|d| d.to_string()))
        .fetch_all(self.pool)
        .await
        .map_err(tracked(DbOp::History, "Failed to total history by day"))?;

        Ok(rows
            .into_iter()
            .filter_map(|(day, downloads, bytes, download_seconds)| {
                Some(DailyTotals {
                    day: day.parse().ok()?,
                    downloads,
                    bytes,
                    download_seconds,
                    avg_speed_bytes_per_s: if download_seconds > 0 {
                        bytes / download_seconds
                    } else {
                        0
                    },
                })
            })
            .collect())
    }
}

/// `AND …` clauses and their bindings for the action and name filters (R7)
fn filters(action: Option<HistoryAction>, name: Option<&str>) -> (String, Vec<String>) {
    let mut sql = String::new();
    let mut bindings = Vec::new();
    if let Some(action) = action {
        sql.push_str(" AND action = ?");
        bindings.push(action.to_string());
    }
    if let Some(name) = name.filter(|n| !n.is_empty()) {
        sql.push_str(" AND LOWER(filename) LIKE ? ESCAPE '\\'");
        bindings.push(like_pattern(name));
    }
    (sql, bindings)
}

/// `%fragment%`, lower-cased, with the `LIKE` wildcards and the escape character escaped
fn like_pattern(fragment: &str) -> String {
    let mut pattern = String::with_capacity(fragment.len() + 2);
    pattern.push('%');
    for c in fragment.to_lowercase().chars() {
        if matches!(c, '\\' | '%' | '_') {
            pattern.push('\\');
        }
        pattern.push(c);
    }
    pattern.push('%');
    pattern
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;
    use tempfile::TempDir;

    async fn setup_test_db() -> (TempDir, Database) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = Database::new(&db_path).await.unwrap();
        (temp_dir, db)
    }

    #[tokio::test]
    async fn test_insert_download_at_keeps_timestamp() {
        let (_temp_dir, db) = setup_test_db().await;
        let repo = HistoryRepository::new(db.pool());
        let at = chrono::DateTime::parse_from_rfc3339("2026-03-04T05:06:07Z")
            .unwrap()
            .with_timezone(&chrono::Utc);

        repo.insert_download_at(7, "Show.S01E01.MKV", 1000, Some(10), Some(100), at)
            .await
            .unwrap();

        let rows = repo.query(None, 10, 0, None).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].timestamp, "2026-03-04 05:06:07");
        assert_eq!(rows[0].extension.as_deref(), Some("mkv"));
        assert_eq!(rows[0].action, "downloaded");
    }

    fn at(text: &str) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&chrono::Utc)
    }

    fn day(text: &str) -> chrono::NaiveDate {
        text.parse().unwrap()
    }

    #[tokio::test]
    async fn daily_totals_split_at_utc_midnight() {
        let (_temp_dir, db) = setup_test_db().await;
        let repo = HistoryRepository::new(db.pool());
        repo.insert_download_at(1, "a.mkv", 1000, Some(10), None, at("2026-03-04T23:59:59Z"))
            .await
            .unwrap();
        repo.insert_download_at(2, "b.mkv", 3000, Some(20), None, at("2026-03-05T00:00:00Z"))
            .await
            .unwrap();
        repo.insert_download_at(3, "c.mkv", 500, Some(5), None, at("2026-03-05T12:00:00Z"))
            .await
            .unwrap();
        // Deleted rows and days far away do not count
        repo.insert(4, "d.mkv", 9999, HistoryAction::Deleted, None, None)
            .await
            .unwrap();
        repo.insert_download_at(5, "e.mkv", 1, Some(1), None, at("2026-04-01T00:00:00Z"))
            .await
            .unwrap();

        let totals = repo
            .daily_totals(Some(day("2026-03-04")), Some(day("2026-03-05")))
            .await
            .unwrap();
        assert_eq!(
            totals,
            vec![
                DailyTotals {
                    day: day("2026-03-04"),
                    downloads: 1,
                    bytes: 1000,
                    download_seconds: 10,
                    avg_speed_bytes_per_s: 100,
                },
                DailyTotals {
                    day: day("2026-03-05"),
                    downloads: 2,
                    bytes: 3500,
                    download_seconds: 25,
                    avg_speed_bytes_per_s: 140,
                },
            ]
        );
    }

    #[tokio::test]
    async fn daily_totals_without_bounds_or_duration() {
        let (_temp_dir, db) = setup_test_db().await;
        let repo = HistoryRepository::new(db.pool());
        assert!(repo.daily_totals(None, None).await.unwrap().is_empty());

        repo.insert_download_at(1, "a.mkv", 1000, None, None, at("2026-03-04T10:00:00Z"))
            .await
            .unwrap();
        repo.insert_download_at(2, "b.mkv", 1, Some(1), None, at("2026-05-01T10:00:00Z"))
            .await
            .unwrap();

        let all = repo.daily_totals(None, None).await.unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].day, day("2026-03-04"));
        assert_eq!(all[0].download_seconds, 0);
        assert_eq!(all[0].avg_speed_bytes_per_s, 0);

        let from = repo
            .daily_totals(Some(day("2026-04-01")), None)
            .await
            .unwrap();
        assert_eq!(from.len(), 1);
        assert_eq!(from[0].day, day("2026-05-01"));
    }

    #[tokio::test]
    async fn outcome_and_local_path_round_trip() {
        let (_temp_dir, db) = setup_test_db().await;
        let repo = HistoryRepository::new(db.pool());
        let old = repo
            .insert(1, "Old.mkv", 10, HistoryAction::Downloaded, None, None)
            .await
            .unwrap();
        let new = repo
            .insert(2, "Movie.mkv", 10, HistoryAction::Downloaded, None, None)
            .await
            .unwrap();
        repo.set_outcome(new, Some("/dl/Movie (1).mkv"), Some("saved_as"))
            .await
            .unwrap();

        let rows = repo.query(None, 10, 0, None).await.unwrap();
        let row = |id| rows.iter().find(|r| r.id == id).unwrap();
        assert_eq!(row(new).local_path.as_deref(), Some("/dl/Movie (1).mkv"));
        assert_eq!(row(new).outcome.as_deref(), Some("saved_as"));
        assert_eq!(row(old).local_path, None);
        assert_eq!(row(old).outcome, None);
    }

    #[tokio::test]
    async fn test_insert_history() {
        let (_temp_dir, db) = setup_test_db().await;
        let repo = HistoryRepository::new(db.pool());

        let id = repo
            .insert(
                12345,
                "test.mkv",
                1024 * 1024 * 100,
                HistoryAction::Downloaded,
                Some(120),
                Some(850000),
            )
            .await
            .unwrap();

        assert!(id > 0);
    }

    #[tokio::test]
    async fn test_query_history() {
        let (_temp_dir, db) = setup_test_db().await;
        let repo = HistoryRepository::new(db.pool());

        repo.insert(
            1,
            "file1.mkv",
            1024,
            HistoryAction::Downloaded,
            Some(10),
            Some(100),
        )
        .await
        .unwrap();
        repo.insert(
            2,
            "file2.mp4",
            2048,
            HistoryAction::Downloaded,
            Some(20),
            Some(100),
        )
        .await
        .unwrap();
        repo.insert(3, "file3.txt", 512, HistoryAction::Deleted, None, None)
            .await
            .unwrap();

        let all = repo.query(None, 50, 0, None).await.unwrap();
        assert_eq!(all.len(), 3);

        let downloaded = repo
            .query(Some(HistoryAction::Downloaded), 50, 0, None)
            .await
            .unwrap();
        assert_eq!(downloaded.len(), 2);

        let deleted = repo
            .query(Some(HistoryAction::Deleted), 50, 0, None)
            .await
            .unwrap();
        assert_eq!(deleted.len(), 1);
        assert_eq!(deleted[0].file_id, 3);
    }

    #[tokio::test]
    async fn test_query_pagination() {
        let (_temp_dir, db) = setup_test_db().await;
        let repo = HistoryRepository::new(db.pool());

        for i in 1..=10 {
            repo.insert(
                i,
                &format!("file{}.mkv", i),
                1024,
                HistoryAction::Downloaded,
                None,
                None,
            )
            .await
            .unwrap();
        }

        let page1 = repo.query(None, 5, 0, None).await.unwrap();
        assert_eq!(page1.len(), 5);

        let page2 = repo.query(None, 5, 5, None).await.unwrap();
        assert_eq!(page2.len(), 5);

        // Ensure pages are different
        assert_ne!(page1[0].file_id, page2[0].file_id);
    }

    #[tokio::test]
    async fn test_count() {
        let (_temp_dir, db) = setup_test_db().await;
        let repo = HistoryRepository::new(db.pool());

        repo.insert(1, "file1.mkv", 1024, HistoryAction::Downloaded, None, None)
            .await
            .unwrap();
        repo.insert(2, "file2.mkv", 2048, HistoryAction::Downloaded, None, None)
            .await
            .unwrap();
        repo.insert(3, "file3.txt", 512, HistoryAction::Deleted, None, None)
            .await
            .unwrap();

        let total = repo.count(None, None).await.unwrap();
        assert_eq!(total, 3);

        let downloaded = repo
            .count(Some(HistoryAction::Downloaded), None)
            .await
            .unwrap();
        assert_eq!(downloaded, 2);

        let deleted = repo
            .count(Some(HistoryAction::Deleted), None)
            .await
            .unwrap();
        assert_eq!(deleted, 1);
    }

    async fn seed_name_filter(repo: &HistoryRepository<'_>) {
        for (id, name, action) in [
            (1, "Ubuntu.iso", HistoryAction::Downloaded),
            (2, "debian.iso", HistoryAction::Deleted),
            (3, "100%_done.mkv", HistoryAction::Downloaded),
            (4, r"back\slash.txt", HistoryAction::Downloaded),
        ] {
            repo.insert(id, name, 1024, action, None, None)
                .await
                .unwrap();
        }
    }

    #[tokio::test]
    async fn test_name_filter_case_insensitive() {
        let (_temp_dir, db) = setup_test_db().await;
        let repo = HistoryRepository::new(db.pool());
        seed_name_filter(&repo).await;

        let rows = repo.query(None, 10, 0, Some("ubuntu")).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].filename, "Ubuntu.iso");

        assert_eq!(repo.count(None, Some("iso")).await.unwrap(), 2);
        assert_eq!(repo.count(None, Some("ISO")).await.unwrap(), 2);
        assert_eq!(
            repo.count(Some(HistoryAction::Deleted), Some("iso"))
                .await
                .unwrap(),
            1
        );
        // Empty fragment is no filter
        assert_eq!(repo.count(None, Some("")).await.unwrap(), 4);
        assert_eq!(repo.query(None, 10, 0, Some("")).await.unwrap().len(), 4);
    }

    #[tokio::test]
    async fn test_name_filter_wildcards_are_literal() {
        let (_temp_dir, db) = setup_test_db().await;
        let repo = HistoryRepository::new(db.pool());
        seed_name_filter(&repo).await;

        let percent = repo.query(None, 10, 0, Some("%")).await.unwrap();
        assert_eq!(percent.len(), 1);
        assert_eq!(percent[0].filename, "100%_done.mkv");

        let underscore = repo.query(None, 10, 0, Some("_done")).await.unwrap();
        assert_eq!(underscore.len(), 1);
        assert_eq!(underscore[0].filename, "100%_done.mkv");

        // Unescaped, `_` would match the `b` in "Ubuntu"
        assert_eq!(repo.count(None, Some("u_untu")).await.unwrap(), 0);

        let backslash = repo.query(None, 10, 0, Some(r"\")).await.unwrap();
        assert_eq!(backslash.len(), 1);
        assert_eq!(backslash[0].filename, r"back\slash.txt");
    }

    #[tokio::test]
    async fn test_extension_extraction() {
        let (_temp_dir, db) = setup_test_db().await;
        let repo = HistoryRepository::new(db.pool());

        repo.insert(1, "file.MKV", 1024, HistoryAction::Downloaded, None, None)
            .await
            .unwrap();

        let history = repo.query(None, 1, 0, None).await.unwrap();
        assert_eq!(history[0].extension, Some("mkv".to_string()));
    }
}
