//! Sync-state checks: the queue and the download folder against Put.io

use super::{age, bytes, count, verb, Ctx, STALE_AGE};
use crate::report::{Finding, FixId, Item, Status};
use putio_sync_client::PutioApiClient;
use putio_sync_db::models::QueueItem;
use putio_sync_db::Database;
use putio_sync_download::files::{create_temp_path, is_partial_download};
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Downloads retried this often without finishing count as failing
pub(crate) const FAILING_RETRIES: i64 = 3;
/// Partial files deeper than this under the download folder are not looked at
const MAX_DEPTH: usize = 32;

/// Where a queue row downloads to, without `.` components
pub(crate) fn target_path(target_dir: &Path, row: &QueueItem) -> PathBuf {
    normalize(&target_dir.join(&row.relative_path).join(&row.filename))
}

fn normalize(path: &Path) -> PathBuf {
    path.components()
        .filter(|c| !matches!(c, Component::CurDir))
        .collect()
}

fn row_label(row: &QueueItem) -> String {
    let dir = row.relative_path.trim_start_matches("./").trim_matches('/');
    if dir.is_empty() || dir == "." {
        row.filename.clone()
    } else {
        format!("{dir}/{}", row.filename)
    }
}

/// Whether Put.io says file `id` is gone: `Some(true)` on a 404, `None` when unsure
pub(crate) async fn file_gone(client: &PutioApiClient, id: i64) -> Option<bool> {
    match client.get_file_details(id).await {
        Ok(_) => Some(false),
        Err(e) if e.is_not_found() => Some(true),
        Err(_) => None,
    }
}

/// Queue rows that are not finished
async fn open_rows(db: &Database) -> Result<Vec<QueueItem>, String> {
    db.queue()
        .get_all()
        .await
        .map(|rows| {
            rows.into_iter()
                .filter(|r| r.status != "completed")
                .collect()
        })
        .map_err(|e| e.to_string())
}

pub(crate) async fn dead_queue_rows(ctx: &Ctx) -> Finding {
    let Some(db) = &ctx.db else {
        return Finding::skipped("Needs the database.");
    };
    let rows = match open_rows(db).await {
        Ok(rows) => rows,
        Err(e) => return Finding::skipped(format!("Could not read the queue: {e}")),
    };
    if rows.is_empty() {
        return Finding::ok("The queue is empty.");
    }
    let walk = ctx.walk().await;
    if !walk.complete {
        return Finding::skipped(
            "The account listing did not finish, so missing files can't be told apart.",
        );
    }
    let seen: HashSet<i64> = walk.files.iter().map(|f| f.id).collect();
    let mut items = Vec::new();
    let mut unsure = 0;
    for row in rows.iter().filter(|r| !seen.contains(&r.file_id)) {
        match file_gone(&ctx.client, row.file_id).await {
            Some(true) => items.push(
                Item::new(row.file_id, row_label(row))
                    .detail(format!("{} in the queue", row.status))
                    .fix(FixId::DeadQueueRowsRemove),
            ),
            Some(false) => {}
            None => unsure += 1,
        }
    }
    let mut summary = if items.is_empty() {
        match rows.len() {
            1 => "The 1 queue entry points at a file on Put.io.".to_string(),
            n => format!("All {} point at files on Put.io.", count(n, "queue entry")),
        }
    } else {
        format!(
            "{} {} at files deleted from Put.io; {} never download.",
            count(items.len(), "queue entry"),
            verb(items.len(), "points", "point"),
            verb(items.len(), "it can", "they can")
        )
    };
    if unsure > 0 {
        summary.push_str(&format!(" {} could not be checked.", count(unsure, "file")));
    }
    let status = if items.is_empty() {
        Status::Ok
    } else {
        Status::Warning
    };
    Finding::new(status, summary)
        .suggest("Remove them from the queue.")
        .items(items)
}

pub(crate) async fn failing_downloads(ctx: &Ctx) -> Finding {
    let Some(db) = &ctx.db else {
        return Finding::skipped("Needs the database.");
    };
    let failed = match db.queue().failed().await {
        Ok(rows) => rows,
        Err(e) => return Finding::skipped(format!("Could not read the queue: {e}")),
    };
    let failing = match db.downloads().failing(FAILING_RETRIES).await {
        Ok(rows) => rows,
        Err(e) => return Finding::skipped(format!("Could not read the downloads: {e}")),
    };
    // A file queued again is being retried right now
    let mut seen: HashSet<i64> = match db.queue().claimed().await {
        Ok(rows) => rows.iter().map(|r| r.file_id).collect(),
        Err(e) => return Finding::skipped(format!("Could not read the queue: {e}")),
    };
    let mut items = Vec::new();
    for row in &failed {
        seen.insert(row.file_id);
        items.push(
            Item::new(row.file_id, row_label(row))
                .detail("failed in the queue")
                .fix(FixId::FailingDownloadsRequeue),
        );
    }
    for d in failing {
        if !seen.insert(d.file_id) {
            continue;
        }
        let detail = if d.status == "failed" {
            format!(
                "failed after {}",
                count(d.retry_count.max(0) as usize, "retry")
            )
        } else {
            format!("retried {} times without finishing", d.retry_count)
        };
        items.push(Item::new(d.file_id, &d.filename).detail(detail));
    }
    let summary = if items.is_empty() {
        "No downloads keep failing.".to_string()
    } else {
        format!(
            "{} {} failing.",
            count(items.len(), "download"),
            verb(items.len(), "keeps", "keep")
        )
    };
    let status = if items.is_empty() {
        Status::Ok
    } else {
        Status::Warning
    };
    Finding::new(status, summary)
        .suggest("Requeue them for another try. If they fail again, check the logs for the cause.")
        .items(items)
}

/// A `.NAME.downloading` file under the download folder
#[derive(Debug, Clone)]
pub(crate) struct Partial {
    pub path: PathBuf,
    /// Path relative to the download folder; the fix's item key
    pub key: String,
    pub size: u64,
    pub age: Duration,
}

/// Every partial file under `root`, without following links
pub(crate) fn partials(root: &Path) -> Vec<Partial> {
    let now = SystemTime::now();
    let mut found = Vec::new();
    let mut dirs = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() {
                if depth < MAX_DEPTH {
                    dirs.push((path, depth + 1));
                }
            } else if kind.is_file() && is_partial_download(&path) {
                let Ok(meta) = entry.metadata() else { continue };
                let age = meta
                    .modified()
                    .ok()
                    .and_then(|m| now.duration_since(m).ok())
                    .unwrap_or_default();
                let key = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .into_owned();
                found.push(Partial {
                    path,
                    key,
                    size: meta.len(),
                    age,
                });
            }
        }
    }
    found.sort_by(|a, b| a.key.cmp(&b.key));
    found
}

/// Partial-file paths that queued, downloading or paused rows will write to
pub(crate) async fn claimed_partials(
    db: &Database,
    target_dir: &Path,
) -> Result<HashSet<PathBuf>, String> {
    let rows = db.queue().claimed().await.map_err(|e| e.to_string())?;
    Ok(rows
        .iter()
        .filter_map(|r| create_temp_path(&target_path(target_dir, r)).ok())
        .map(|p| normalize(&p))
        .collect())
}

/// Partial files no download owns, untouched for at least [`STALE_AGE`]
pub(crate) async fn orphans(db: &Database, target_dir: &Path) -> Result<Vec<Partial>, String> {
    let claimed = claimed_partials(db, target_dir).await?;
    let root = target_dir.to_path_buf();
    let all = tokio::task::spawn_blocking(move || partials(&root))
        .await
        .map_err(|e| e.to_string())?;
    Ok(all
        .into_iter()
        .filter(|p| p.age >= STALE_AGE && !claimed.contains(&normalize(&p.path)))
        .collect())
}

pub(crate) async fn orphan_partials(ctx: &Ctx) -> Finding {
    let Some(db) = &ctx.db else {
        return Finding::skipped("Needs the database to know which downloads are running.");
    };
    let target = &ctx.config.download.target_directory;
    if !target.is_dir() {
        return Finding::skipped(
            "The download folder does not exist; see the download folder check.",
        );
    }
    let orphans = match orphans(db, target).await {
        Ok(o) => o,
        Err(e) => return Finding::skipped(format!("Could not read the queue: {e}")),
    };
    let total: u64 = orphans.iter().map(|p| p.size).sum();
    let items: Vec<Item> = orphans
        .iter()
        .map(|p| {
            Item::new(&p.key, &p.key)
                .detail(format!("{}, {} old", bytes(p.size), age(p.age)))
                .fix(FixId::OrphanPartialsDelete)
        })
        .collect();
    let summary = if items.is_empty() {
        "No leftover partial downloads.".to_string()
    } else {
        format!(
            "{} using {} {} to no download.",
            count(items.len(), "partial file"),
            bytes(total),
            verb(items.len(), "belongs", "belong")
        )
    };
    let status = if items.is_empty() {
        Status::Ok
    } else {
        Status::Warning
    };
    Finding::new(status, summary)
        .suggest("Delete them to free the space; a later sync downloads anything still needed.")
        .items(items)
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use mockito::Server;
    use putio_sync_db::models::{DownloadStatus, QueueStatus};

    #[tokio::test]
    async fn dead_rows_need_a_confirming_404() {
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/files/list?parent_id=0")
            .with_body(listing(&[entry(1, "here.mkv", 10, false)]))
            .create_async()
            .await;
        server
            .mock("GET", "/files/2")
            .with_status(404)
            .create_async()
            .await;
        server
            .mock("GET", "/files/3")
            .with_body(
                r#"{"status":"OK","file":{"id":3,"name":"shared.mkv","size":1,
                   "file_type":"VIDEO","parent_id":null}}"#,
            )
            .create_async()
            .await;
        let dir = tempfile::tempdir().unwrap();
        let db = db(dir.path()).await;
        for id in 1..=3 {
            db.queue()
                .add(id, "f.mkv", 1, "", "Movies", 0, QueueStatus::Pending)
                .await
                .unwrap();
        }
        let f = dead_queue_rows(&ctx(&server.url(), dir.path(), Some(db))).await;
        assert_eq!(f.status, Status::Warning);
        let keys: Vec<&str> = f.items.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(keys, vec!["2"]);
        assert_eq!(f.items[0].label, "Movies/f.mkv");
    }

    #[tokio::test]
    async fn failing_downloads_merge_queue_and_history() {
        let dir = tempfile::tempdir().unwrap();
        let db = db(dir.path()).await;
        db.queue()
            .add(1, "a.mkv", 1, "", "", 0, QueueStatus::Pending)
            .await
            .unwrap();
        db.queue()
            .update_status(1, QueueStatus::Failed)
            .await
            .unwrap();
        db.downloads()
            .insert_with_retry_count(1, "a.mkv", 1, "/x", DownloadStatus::Failed, 4)
            .await
            .unwrap();
        db.downloads()
            .insert_with_retry_count(2, "b.mkv", 1, "/y", DownloadStatus::Failed, 1)
            .await
            .unwrap();
        let f = failing_downloads(&ctx("http://127.0.0.1:9", dir.path(), Some(db))).await;
        let keys: Vec<&str> = f.items.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(keys, vec!["1", "2"]);
        assert_eq!(f.items[1].detail.as_deref(), Some("failed after 1 retry"));
    }

    #[tokio::test]
    async fn orphan_partials_skip_claimed_and_recent() {
        let dir = tempfile::tempdir().unwrap();
        let db = db(dir.path()).await;
        let c = ctx("http://127.0.0.1:9", dir.path(), Some(db.clone()));
        let target = c.config.download.target_directory.clone();
        std::fs::create_dir_all(target.join("Movies")).unwrap();
        let old = filetime::FileTime::from_unix_time(1_600_000_000, 0);
        for name in [
            "Movies/.claimed.mkv.downloading",
            "Movies/.orphan.mkv.downloading",
            "done.mkv",
        ] {
            std::fs::write(target.join(name), "abc").unwrap();
            filetime::set_file_mtime(target.join(name), old).unwrap();
        }
        std::fs::write(target.join(".fresh.mkv.downloading"), "x").unwrap();
        db.queue()
            .add(1, "claimed.mkv", 3, "", "./Movies", 0, QueueStatus::Pending)
            .await
            .unwrap();

        let f = orphan_partials(&c).await;
        let keys: Vec<&str> = f.items.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(keys, vec!["Movies/.orphan.mkv.downloading"]);
        assert_eq!(f.status, Status::Warning);
    }
}
