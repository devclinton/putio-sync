//! Moves the organiser plans for files kept on Put.io (feature 023), and what it needs to
//! undo them. One row per Put.io file id; a move keeps the id, so the row follows the file.

use super::error::{tracked, DbResult};
use putio_sync_telemetry::DbOp;
use sqlx::{Row, SqlitePool};
use std::collections::HashSet;

/// Where a move stands
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveStatus {
    /// Waiting for the operator
    Planned,
    /// Made on Put.io
    Applied,
    /// Made, then put back
    Undone,
    /// Tried and refused or failed; `error` says why
    Failed,
}

impl MoveStatus {
    /// The stored value
    pub fn as_str(&self) -> &'static str {
        match self {
            MoveStatus::Planned => "planned",
            MoveStatus::Applied => "applied",
            MoveStatus::Undone => "undone",
            MoveStatus::Failed => "failed",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "applied" => MoveStatus::Applied,
            "undone" => MoveStatus::Undone,
            "failed" => MoveStatus::Failed,
            _ => MoveStatus::Planned,
        }
    }
}

/// A move to record
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewMove {
    pub file_id: i64,
    /// `tv` (024 adds `movie`)
    pub kind: String,
    pub from_folder: String,
    pub from_name: String,
    pub to_folder: String,
    pub to_name: String,
    /// A plugin set the folder or name
    pub by_plugin: bool,
}

/// One recorded move
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrganiseMove {
    pub file_id: i64,
    pub kind: String,
    pub from_folder: String,
    pub from_name: String,
    pub to_folder: String,
    pub to_name: String,
    pub by_plugin: bool,
    pub status: MoveStatus,
    /// The folder id the file was in when applied, for undo
    pub from_parent_id: Option<i64>,
    pub error: Option<String>,
    pub updated_at: String,
}

impl OrganiseMove {
    fn from_row(row: &sqlx::sqlite::SqliteRow) -> Self {
        Self {
            file_id: row.get("file_id"),
            kind: row.get("kind"),
            from_folder: row.get("from_folder"),
            from_name: row.get("from_name"),
            to_folder: row.get("to_folder"),
            to_name: row.get("to_name"),
            by_plugin: row.get::<i64, _>("by_plugin") != 0,
            status: MoveStatus::parse(row.get("status")),
            from_parent_id: row.get("from_parent_id"),
            error: row.get("error"),
            updated_at: row.get("updated_at"),
        }
    }
}

const COLUMNS: &str = "file_id, kind, from_folder, from_name, to_folder, to_name, by_plugin, \
    status, from_parent_id, error, CAST(updated_at AS TEXT) AS updated_at";

/// Repository for `organise_moves`
pub struct OrganiseMovesRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> OrganiseMovesRepository<'a> {
    /// Create a new organise moves repository
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Record a planned move; a file that already has one is left as it is. Whether it was
    /// recorded.
    pub async fn plan(&self, m: &NewMove) -> DbResult<bool> {
        let done = sqlx::query(
            r#"
            INSERT INTO organise_moves
                (file_id, kind, from_folder, from_name, to_folder, to_name, by_plugin, status)
            VALUES (?, ?, ?, ?, ?, ?, ?, 'planned')
            ON CONFLICT(file_id) DO NOTHING
            "#,
        )
        .bind(m.file_id)
        .bind(&m.kind)
        .bind(&m.from_folder)
        .bind(&m.from_name)
        .bind(&m.to_folder)
        .bind(&m.to_name)
        .bind(m.by_plugin as i64)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to plan organise move"))?;
        Ok(done.rows_affected() > 0)
    }

    /// One move
    pub async fn get(&self, file_id: i64) -> DbResult<Option<OrganiseMove>> {
        let sql = format!("SELECT {COLUMNS} FROM organise_moves WHERE file_id = ?");
        let row = sqlx::query(&sql)
            .bind(file_id)
            .fetch_optional(self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to get organise move"))?;
        Ok(row.as_ref().map(OrganiseMove::from_row))
    }

    /// Moves, newest change first
    pub async fn list(&self, limit: i64) -> DbResult<Vec<OrganiseMove>> {
        let sql = format!(
            "SELECT {COLUMNS} FROM organise_moves ORDER BY updated_at DESC, file_id DESC LIMIT ?"
        );
        let rows = sqlx::query(&sql)
            .bind(limit)
            .fetch_all(self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to list organise moves"))?;
        Ok(rows.iter().map(OrganiseMove::from_row).collect())
    }

    /// Ids of files with a move recorded, whatever its state
    pub async fn ids(&self) -> DbResult<HashSet<i64>> {
        let rows = sqlx::query("SELECT file_id FROM organise_moves")
            .fetch_all(self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to list organise moves"))?;
        Ok(rows.iter().map(|r| r.get("file_id")).collect())
    }

    /// The move was made. `from_name` and `from_parent_id` are what Put.io said just before,
    /// so undo puts the file back where it really was.
    pub async fn applied(
        &self,
        file_id: i64,
        from_name: &str,
        from_parent_id: i64,
    ) -> DbResult<()> {
        sqlx::query(
            r#"
            UPDATE organise_moves SET status = 'applied', from_name = ?, from_parent_id = ?,
                error = NULL, updated_at = CURRENT_TIMESTAMP
            WHERE file_id = ?
            "#,
        )
        .bind(from_name)
        .bind(from_parent_id)
        .bind(file_id)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to update organise move"))?;
        Ok(())
    }

    /// The move was put back
    pub async fn undone(&self, file_id: i64) -> DbResult<()> {
        self.set(file_id, MoveStatus::Undone, None).await
    }

    /// The move was refused or failed
    pub async fn failed(&self, file_id: i64, error: &str) -> DbResult<()> {
        self.set(file_id, MoveStatus::Failed, Some(error)).await
    }

    /// Note an error without changing the state (an undo that could not be made)
    pub async fn note(&self, file_id: i64, error: &str) -> DbResult<()> {
        sqlx::query(
            "UPDATE organise_moves SET error = ?, updated_at = CURRENT_TIMESTAMP WHERE file_id = ?",
        )
        .bind(error)
        .bind(file_id)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to update organise move"))?;
        Ok(())
    }

    async fn set(&self, file_id: i64, status: MoveStatus, error: Option<&str>) -> DbResult<()> {
        sqlx::query(
            "UPDATE organise_moves SET status = ?, error = ?, updated_at = CURRENT_TIMESTAMP \
             WHERE file_id = ?",
        )
        .bind(status.as_str())
        .bind(error)
        .bind(file_id)
        .execute(self.pool)
        .await
        .map_err(tracked(DbOp::Other, "Failed to update organise move"))?;
        Ok(())
    }

    /// Forget moves
    pub async fn remove(&self, file_ids: &[i64]) -> DbResult<u64> {
        let mut removed = 0;
        for chunk in file_ids.chunks(500) {
            let marks = vec!["?"; chunk.len()].join(",");
            let sql = format!("DELETE FROM organise_moves WHERE file_id IN ({marks})");
            let mut query = sqlx::query(&sql);
            for id in chunk {
                query = query.bind(id);
            }
            removed += query
                .execute(self.pool)
                .await
                .map_err(tracked(DbOp::Other, "Failed to remove organise moves"))?
                .rows_affected();
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;
    use tempfile::TempDir;

    fn new_move(file_id: i64) -> NewMove {
        NewMove {
            file_id,
            kind: "tv".to_string(),
            from_folder: "Show.S01.1080p".to_string(),
            from_name: "Show.S01E02.mkv".to_string(),
            to_folder: "TV Shows/Show/Season 01".to_string(),
            to_name: "Show - S01E02.mkv".to_string(),
            by_plugin: false,
        }
    }

    #[tokio::test]
    async fn plan_once_then_apply_undo_and_fail() {
        let dir = TempDir::new().unwrap();
        let db = Database::new(&dir.path().join("test.db")).await.unwrap();
        let repo = db.organise_moves();
        assert!(repo.plan(&new_move(7)).await.unwrap());
        // A second plan for the same file changes nothing
        let mut other = new_move(7);
        other.to_name = "Other.mkv".to_string();
        assert!(!repo.plan(&other).await.unwrap());
        let m = repo.get(7).await.unwrap().unwrap();
        assert_eq!(m.status, MoveStatus::Planned);
        assert_eq!(m.to_name, "Show - S01E02.mkv");
        assert_eq!(m.from_parent_id, None);

        repo.applied(7, "Show.S01E02.REPACK.mkv", 40).await.unwrap();
        let m = repo.get(7).await.unwrap().unwrap();
        assert_eq!(m.status, MoveStatus::Applied);
        assert_eq!(m.from_name, "Show.S01E02.REPACK.mkv");
        assert_eq!(m.from_parent_id, Some(40));

        repo.note(7, "old folder gone").await.unwrap();
        let m = repo.get(7).await.unwrap().unwrap();
        assert_eq!(m.status, MoveStatus::Applied);
        assert_eq!(m.error.as_deref(), Some("old folder gone"));

        repo.undone(7).await.unwrap();
        let m = repo.get(7).await.unwrap().unwrap();
        assert_eq!((m.status, m.error), (MoveStatus::Undone, None));

        repo.failed(7, "name taken").await.unwrap();
        assert_eq!(
            repo.get(7).await.unwrap().unwrap().status,
            MoveStatus::Failed
        );
    }

    #[tokio::test]
    async fn list_ids_and_remove() {
        let dir = TempDir::new().unwrap();
        let db = Database::new(&dir.path().join("test.db")).await.unwrap();
        let repo = db.organise_moves();
        for id in 1..=3 {
            repo.plan(&new_move(id)).await.unwrap();
        }
        let mut plugin = new_move(4);
        plugin.by_plugin = true;
        repo.plan(&plugin).await.unwrap();
        assert!(repo.get(4).await.unwrap().unwrap().by_plugin);
        assert_eq!(repo.list(2).await.unwrap().len(), 2);
        assert_eq!(repo.ids().await.unwrap().len(), 4);
        assert_eq!(repo.remove(&[1, 2, 99]).await.unwrap(), 2);
        assert_eq!(repo.ids().await.unwrap(), HashSet::from([3, 4]));
        assert!(repo.get(1).await.unwrap().is_none());
    }
}
