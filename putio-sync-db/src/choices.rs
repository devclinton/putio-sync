//! Which files of a transfer to download (feature 070): one choice per Put.io transfer,
//! with a wanted flag per file path under the transfer's root item

use super::error::{tracked, DbResult};
use putio_sync_telemetry::DbOp;
use sqlx::{Row, SqlitePool};
use std::collections::HashMap;

/// The choice made for one transfer
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Choice {
    /// Put.io transfer id
    pub transfer_id: i64,
    /// The file or folder the transfer made on Put.io, once known. Files are matched
    /// to the choice through it, so the choice outlives the transfer's list entry.
    pub root_file_id: Option<i64>,
    /// What happens to a file with no flag of its own (one that shows up later)
    pub others_wanted: bool,
    /// Nothing was chosen yet: the transfer waits for its files to be picked
    pub pending: bool,
    /// Wanted flag by path under the root item (`Season 1/e01.mkv`; the file name for
    /// a transfer that is one file)
    pub files: HashMap<String, bool>,
}

impl Choice {
    /// Whether the file at `path` is downloaded: its own flag, else the rule for others
    pub fn wants(&self, path: &str) -> bool {
        self.files.get(path).copied().unwrap_or(self.others_wanted)
    }
}

/// Repository for `transfer_choices` and `transfer_choice_files`
pub struct ChoicesRepository<'a> {
    pool: &'a SqlitePool,
}

impl<'a> ChoicesRepository<'a> {
    /// Create a new choices repository
    pub fn new(pool: &'a SqlitePool) -> Self {
        Self { pool }
    }

    /// Save (or replace) the choice of a transfer. A `None` root keeps the stored one.
    pub async fn set(&self, choice: &Choice) -> DbResult<()> {
        let failed = || tracked(DbOp::Other, "Failed to save transfer choice");
        let mut tx = self.pool.begin().await.map_err(failed())?;
        sqlx::query(
            r#"
            INSERT INTO transfer_choices (transfer_id, root_file_id, others, pending)
            VALUES (?, ?, ?, ?)
            ON CONFLICT(transfer_id) DO UPDATE SET
                root_file_id = COALESCE(excluded.root_file_id, transfer_choices.root_file_id),
                others = excluded.others,
                pending = excluded.pending,
                chosen_at = CURRENT_TIMESTAMP
            "#,
        )
        .bind(choice.transfer_id)
        .bind(choice.root_file_id)
        .bind(if choice.others_wanted {
            "download"
        } else {
            "skip"
        })
        .bind(choice.pending)
        .execute(&mut *tx)
        .await
        .map_err(failed())?;
        sqlx::query("DELETE FROM transfer_choice_files WHERE transfer_id = ?")
            .bind(choice.transfer_id)
            .execute(&mut *tx)
            .await
            .map_err(failed())?;
        for (path, wanted) in &choice.files {
            sqlx::query(
                "INSERT INTO transfer_choice_files (transfer_id, path, wanted) VALUES (?, ?, ?)",
            )
            .bind(choice.transfer_id)
            .bind(path)
            .bind(*wanted)
            .execute(&mut *tx)
            .await
            .map_err(failed())?;
        }
        tx.commit().await.map_err(failed())?;
        Ok(())
    }

    /// Record the root item of a transfer's choice, once Put.io has made it
    pub async fn set_root(&self, transfer_id: i64, root_file_id: i64) -> DbResult<()> {
        sqlx::query("UPDATE transfer_choices SET root_file_id = ? WHERE transfer_id = ?")
            .bind(root_file_id)
            .bind(transfer_id)
            .execute(self.pool)
            .await
            .map_err(tracked(DbOp::Other, "Failed to update transfer choice"))?;
        Ok(())
    }

    /// The choice of one transfer
    pub async fn get(&self, transfer_id: i64) -> DbResult<Option<Choice>> {
        Ok(self.read(Some(transfer_id)).await?.into_iter().next())
    }

    /// Every choice, by transfer id
    pub async fn all(&self) -> DbResult<Vec<Choice>> {
        self.read(None).await
    }

    async fn read(&self, transfer_id: Option<i64>) -> DbResult<Vec<Choice>> {
        let failed = || tracked(DbOp::Other, "Failed to read transfer choices");
        let rows = sqlx::query(
            r#"
            SELECT transfer_id, root_file_id, others, pending FROM transfer_choices
            WHERE ?1 IS NULL OR transfer_id = ?1
            ORDER BY transfer_id
            "#,
        )
        .bind(transfer_id)
        .fetch_all(self.pool)
        .await
        .map_err(failed())?;
        let mut choices: Vec<Choice> = rows
            .into_iter()
            .map(|row| Choice {
                transfer_id: row.get("transfer_id"),
                root_file_id: row.get("root_file_id"),
                others_wanted: row.get::<String, _>("others") == "download",
                pending: row.get("pending"),
                files: HashMap::new(),
            })
            .collect();
        if choices.is_empty() {
            return Ok(choices);
        }
        let files = sqlx::query(
            r#"
            SELECT transfer_id, path, wanted FROM transfer_choice_files
            WHERE ?1 IS NULL OR transfer_id = ?1
            "#,
        )
        .bind(transfer_id)
        .fetch_all(self.pool)
        .await
        .map_err(failed())?;
        let mut by_transfer: HashMap<i64, HashMap<String, bool>> = HashMap::new();
        for row in files {
            by_transfer
                .entry(row.get("transfer_id"))
                .or_default()
                .insert(row.get("path"), row.get("wanted"));
        }
        for choice in &mut choices {
            choice.files = by_transfer.remove(&choice.transfer_id).unwrap_or_default();
        }
        Ok(choices)
    }

    /// Forget a transfer's choice; `true` when it had one
    pub async fn remove(&self, transfer_id: i64) -> DbResult<bool> {
        let failed = || tracked(DbOp::Other, "Failed to remove transfer choice");
        let mut tx = self.pool.begin().await.map_err(failed())?;
        sqlx::query("DELETE FROM transfer_choice_files WHERE transfer_id = ?")
            .bind(transfer_id)
            .execute(&mut *tx)
            .await
            .map_err(failed())?;
        let result = sqlx::query("DELETE FROM transfer_choices WHERE transfer_id = ?")
            .bind(transfer_id)
            .execute(&mut *tx)
            .await
            .map_err(failed())?;
        tx.commit().await.map_err(failed())?;
        Ok(result.rows_affected() > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Database;

    fn choice(transfer_id: i64, others_wanted: bool, files: &[(&str, bool)]) -> Choice {
        Choice {
            transfer_id,
            root_file_id: None,
            others_wanted,
            pending: false,
            files: files.iter().map(|(p, w)| (p.to_string(), *w)).collect(),
        }
    }

    #[test]
    fn a_listed_path_wins_and_the_rest_follow_others() {
        let only = choice(1, false, &[("a.mkv", true)]);
        assert!(only.wants("a.mkv"));
        assert!(!only.wants("later.mkv"));
        let all_but = choice(1, true, &[("sample.mkv", false)]);
        assert!(!all_but.wants("sample.mkv"));
        assert!(all_but.wants("later.mkv"));
    }

    #[tokio::test]
    async fn a_choice_is_saved_replaced_and_removed() {
        let dir = tempfile::TempDir::new().unwrap();
        let db = Database::new(dir.path().join("choices.db")).await.unwrap();
        let repo = db.choices();
        assert_eq!(repo.get(7).await.unwrap(), None);

        let first = choice(7, false, &[("S1/e01.mkv", true), ("S1/e02.mkv", false)]);
        repo.set(&first).await.unwrap();
        assert_eq!(repo.get(7).await.unwrap(), Some(first));

        // The root is kept when a later save does not name it
        repo.set_root(7, 70).await.unwrap();
        let second = choice(7, true, &[("S1/e02.mkv", true)]);
        repo.set(&second).await.unwrap();
        let stored = repo.get(7).await.unwrap().unwrap();
        assert_eq!(stored.root_file_id, Some(70));
        assert!(stored.others_wanted);
        assert_eq!(stored.files, second.files);

        repo.set(&Choice {
            pending: true,
            ..choice(8, false, &[])
        })
        .await
        .unwrap();
        let all = repo.all().await.unwrap();
        assert_eq!(all.len(), 2);
        assert!(all[1].pending && all[1].files.is_empty());

        assert!(repo.remove(7).await.unwrap());
        assert!(!repo.remove(7).await.unwrap());
        assert_eq!(repo.all().await.unwrap().len(), 1);
    }
}
