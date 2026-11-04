//! Service tests: a mock Put.io, a temp database and temp local files. Nothing here
//! talks to a real account.

use super::*;
use mockito::{Matcher, Mock, Server, ServerGuard};
use putio_sync_client::RetryPolicy;
use putio_sync_db::models::{DownloadStatus, QueueStatus};
use tempfile::TempDir;

const GB: i64 = 1 << 30;

struct World {
    server: ServerGuard,
    dir: TempDir,
    db: Arc<Database>,
    svc: StorageService,
}

fn entry(id: i64, parent: i64, name: &str, size: i64, folder: bool, created: &str) -> String {
    format!(
        r#"{{"id":{id},"name":"{name}","size":{size},"file_type":"{}","parent_id":{parent},
           "is_folder":{folder},"is_shared":false,"created_at":"{created}"}}"#,
        if folder { "FOLDER" } else { "VIDEO" }
    )
}

fn listing(entries: &[String]) -> String {
    format!(
        r#"{{"status":"OK","files":[{}],"parent":{{"id":0,"name":"root","size":0,
           "file_type":"FOLDER","parent_id":null,"is_folder":true}}}}"#,
        entries.join(",")
    )
}

fn transfer(id: i64, status: &str, save_parent_id: i64) -> String {
    format!(
        r#"{{"id":{id},"name":"t{id}","status":"{status}","created_at":"2026-01-01T00:00:00",
           "save_parent_id":{save_parent_id}}}"#
    )
}

impl World {
    async fn new(config: Config) -> Self {
        let server = Server::new_async().await;
        let dir = TempDir::new().unwrap();
        let db = Arc::new(Database::new(dir.path().join("test.db")).await.unwrap());
        let client = Arc::new(
            PutioApiClient::with_base_url("t".into(), server.url()).with_retry_policy(
                RetryPolicy {
                    max_attempts: 1,
                    ..RetryPolicy::default()
                },
            ),
        );
        let svc = StorageService::from_config(client, Some(db.clone()), &config);
        Self {
            server,
            dir,
            db,
            svc,
        }
    }

    async fn account(&mut self, size: i64, used: i64) -> Mock {
        self.server
            .mock("GET", "/account/info")
            .with_body(format!(
                r#"{{"status":"OK","info":{{"user_id":1,"username":"u","mail":"m",
                   "account_active":true,"disk":{{"size":{size},"used":{used},"avail":{}}}}}}}"#,
                size - used
            ))
            .create_async()
            .await
    }

    async fn root(&mut self, entries: &[String]) -> Mock {
        self.server
            .mock("GET", "/files/list?parent_id=0")
            .with_body(listing(entries))
            .create_async()
            .await
    }

    async fn transfers(&mut self, transfers: &[String]) -> Mock {
        self.server
            .mock("GET", "/transfers/list")
            .with_body(format!(
                r#"{{"status":"OK","transfers":[{}]}}"#,
                transfers.join(",")
            ))
            .create_async()
            .await
    }

    async fn details(&mut self, id: i64, size: i64) -> Mock {
        self.server
            .mock("GET", format!("/files/{id}").as_str())
            .with_body(format!(
                r#"{{"status":"OK","file":{}}}"#,
                entry(
                    id,
                    0,
                    &format!("f{id}.mkv"),
                    size,
                    false,
                    "2026-01-01T00:00:00"
                )
            ))
            .create_async()
            .await
    }

    async fn delete(&mut self, ids: &str) -> Mock {
        self.server
            .mock("POST", "/files/delete")
            .match_body(if ids.is_empty() {
                Matcher::Any
            } else {
                Matcher::Json(serde_json::json!({ "file_ids": ids }))
            })
            .with_body(r#"{"status":"OK"}"#)
            .create_async()
            .await
    }

    /// A completed download of `id` with a local file of `size` bytes (sparse)
    async fn downloaded(&self, id: i64, size: i64) -> String {
        let path = self.dir.path().join(format!("f{id}.mkv"));
        std::fs::File::create(&path)
            .unwrap()
            .set_len(size as u64)
            .unwrap();
        let path = path.to_string_lossy().to_string();
        self.db
            .downloads()
            .insert(
                id,
                &format!("f{id}.mkv"),
                size,
                &path,
                DownloadStatus::Completed,
            )
            .await
            .unwrap();
        path
    }
}

/// 100 GB, 95 used; five old files of 5 GB each in the root
async fn full_world() -> World {
    let mut w = World::new(Config::default()).await;
    w.account(100 * GB, 95 * GB).await;
    let files: Vec<String> = (1..=5)
        .map(|i| {
            entry(
                i,
                0,
                &format!("f{i}.mkv"),
                5 * GB,
                false,
                &format!("2026-01-0{i}T00:00:00"),
            )
        })
        .collect();
    w.root(&files).await;
    w.transfers(&[]).await;
    w
}

fn ids(files: &[Candidate]) -> Vec<i64> {
    files.iter().map(|f| f.id).collect()
}

#[tokio::test]
async fn summary_lists_largest_first_and_flags_low() {
    let mut w = World::new(Config::default()).await;
    w.account(100 * GB, 95 * GB).await;
    w.root(&[
        entry(1, 0, "Small", 5 * GB, true, "2026-01-01T00:00:00"),
        entry(2, 0, "Big", 60 * GB, true, "2026-01-01T00:00:00"),
        entry(3, 0, "Mid", 30 * GB, false, "2026-01-01T00:00:00"),
    ])
    .await;
    let s = w.svc.summary().await.unwrap();
    assert_eq!(s.free, 5 * GB);
    assert_eq!(s.used_percent, 95.0);
    assert!(s.low);
    assert_eq!(
        s.top.iter().map(|t| t.id).collect::<Vec<_>>(),
        vec![2, 3, 1]
    );
    assert_eq!(s.top[0].share_percent, 63.2);

    let mut w = World::new(Config::default()).await;
    w.account(100 * GB, 50 * GB).await;
    w.root(&[]).await;
    assert!(!w.svc.summary().await.unwrap().low);
}

#[tokio::test]
async fn plan_picks_verified_downloads_oldest_first_to_the_target() {
    let w = full_world().await;
    // 1 and 2 not downloaded; 3, 4, 5 downloaded
    for id in 3..=5 {
        w.downloaded(id, 5 * GB).await;
    }
    let plan = w.svc.plan().await.unwrap();
    // 5 GB free, target 20 GB: 15 GB needed, three files
    assert_eq!(plan.needed_bytes, 15 * GB);
    assert_eq!(ids(&plan.files), vec![3, 4, 5]);
    assert_eq!(plan.total_bytes, 15 * GB);
    assert_eq!(plan.free_after, 20 * GB);
    assert_eq!(plan.shortfall_bytes, 0);
    assert!(plan.complete);
}

#[tokio::test]
async fn plan_reports_a_shortfall_and_keeps_queued_files() {
    let w = full_world().await;
    w.downloaded(1, 5 * GB).await;
    w.downloaded(2, 5 * GB).await;
    w.db.queue()
        .add(2, "f2.mkv", 5 * GB, "u", "f2.mkv", 0, QueueStatus::Pending)
        .await
        .unwrap();
    let plan = w.svc.plan().await.unwrap();
    assert_eq!(ids(&plan.files), vec![1]);
    assert_eq!(plan.shortfall_bytes, 10 * GB);
}

#[tokio::test]
async fn plan_is_empty_when_the_target_is_met() {
    let mut w = World::new(Config::default()).await;
    w.account(100 * GB, 50 * GB).await;
    let plan = w.svc.plan().await.unwrap();
    assert_eq!(plan.needed_bytes, 0);
    assert!(plan.files.is_empty());
}

#[tokio::test]
async fn free_deletes_exactly_the_plan_files_and_records_the_run() {
    let mut w = full_world().await;
    for id in 3..=5 {
        w.downloaded(id, 5 * GB).await;
    }
    let plan = w.svc.plan().await.unwrap();
    for id in 3..=5 {
        w.details(id, 5 * GB).await;
    }
    let delete = w.delete("3,4,5").await;
    let result = w
        .svc
        .free(Trigger::Manual, &plan.id, &[3, 4, 5])
        .await
        .unwrap();
    delete.assert_async().await;
    assert_eq!(ids(&result.deleted), vec![3, 4, 5]);
    assert_eq!(result.bytes_deleted, 15 * GB);
    assert!(result.skipped.is_empty() && result.failed.is_empty());

    let runs = w.svc.runs(RECENT_RUNS).await.unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(Some(runs[0].id), result.run_id);
    assert_eq!(runs[0].trigger, "manual");
    assert_eq!(runs[0].deleted.len(), 3);
    let deleted =
        w.db.history()
            .count(Some(HistoryAction::Deleted), None)
            .await
            .unwrap();
    assert_eq!(deleted, 3);

    // The plan is used up
    assert!(matches!(
        w.svc.free(Trigger::Manual, &plan.id, &[3]).await,
        Err(StorageError::PlanExpired)
    ));
}

#[tokio::test]
async fn free_skips_files_that_changed_since_the_preview() {
    let mut w = full_world().await;
    let mut paths = HashMap::new();
    for id in 1..=5 {
        paths.insert(id, w.downloaded(id, 5 * GB).await);
    }
    let plan = w.svc.plan().await.unwrap();
    assert_eq!(ids(&plan.files), vec![1, 2, 3]);

    // 1 is gone from Put.io, 2 changed size, 3's local copy was removed
    w.server
        .mock("GET", "/files/1")
        .with_status(404)
        .with_body(r#"{"status":"ERROR","error_type":"NotFound"}"#)
        .create_async()
        .await;
    w.details(2, 6 * GB).await;
    w.details(3, 5 * GB).await;
    std::fs::remove_file(&paths[&3]).unwrap();
    let delete = w.delete("").await.expect(0);

    let result = w
        .svc
        .free(Trigger::Manual, &plan.id, &[1, 2, 3, 99])
        .await
        .unwrap();
    delete.assert_async().await;
    assert!(result.deleted.is_empty());
    assert_eq!(result.run_id, None);
    let reasons: HashMap<i64, &str> = result
        .skipped
        .iter()
        .map(|s| (s.id, s.reason.as_str()))
        .collect();
    assert_eq!(reasons[&99], "not in the preview");
    assert_eq!(reasons[&1], "no longer on Put.io");
    assert_eq!(reasons[&2], "size changed on Put.io");
    assert_eq!(reasons[&3], "local copy missing or changed");
    assert!(w.svc.runs(RECENT_RUNS).await.unwrap().is_empty());
}

#[tokio::test]
async fn free_skips_newly_queued_and_transfer_files() {
    let mut w = World::new(Config::default()).await;
    w.account(100 * GB, 95 * GB).await;
    w.root(&[
        entry(1, 0, "f1.mkv", 10 * GB, false, "2026-01-01T00:00:00"),
        entry(2, 0, "f2.mkv", 10 * GB, false, "2026-01-02T00:00:00"),
    ])
    .await;
    let empty = w.transfers(&[]).await;
    w.downloaded(1, 10 * GB).await;
    w.downloaded(2, 10 * GB).await;
    let plan = w.svc.plan().await.unwrap();
    assert_eq!(ids(&plan.files), vec![1, 2]);

    // Since the preview: 1 was queued again; 2 moved into a folder a transfer now saves to
    empty.remove_async().await;
    w.transfers(&[transfer(9, "DOWNLOADING", 50)]).await;
    w.server
        .mock("GET", "/files/2")
        .with_body(format!(
            r#"{{"status":"OK","file":{}}}"#,
            entry(2, 50, "f2.mkv", 10 * GB, false, "2026-01-02T00:00:00")
        ))
        .create_async()
        .await;
    w.details(1, 10 * GB).await;
    w.db.queue()
        .add(1, "f1.mkv", 10 * GB, "u", "f1.mkv", 0, QueueStatus::Pending)
        .await
        .unwrap();
    let delete = w.delete("").await.expect(0);

    let result = w
        .svc
        .free(Trigger::Manual, &plan.id, &[1, 2])
        .await
        .unwrap();
    delete.assert_async().await;
    let reasons: HashMap<i64, &str> = result
        .skipped
        .iter()
        .map(|s| (s.id, s.reason.as_str()))
        .collect();
    assert_eq!(reasons[&1], "queued for download");
    assert_eq!(reasons[&2], "part of an unfinished transfer");
}

#[tokio::test]
async fn a_failed_delete_batch_is_reported() {
    let mut w = full_world().await;
    for id in 1..=3 {
        w.downloaded(id, 5 * GB).await;
        w.details(id, 5 * GB).await;
    }
    let plan = w.svc.plan().await.unwrap();
    w.server
        .mock("POST", "/files/delete")
        .with_status(500)
        .with_body(r#"{"status":"ERROR"}"#)
        .create_async()
        .await;
    let result = w
        .svc
        .free(Trigger::Manual, &plan.id, &[1, 2, 3])
        .await
        .unwrap();
    assert!(result.deleted.is_empty());
    assert_eq!(result.failed.len(), 3);
    assert_eq!(result.run_id, None);
}

#[tokio::test]
async fn unknown_plans_and_concurrent_runs_are_refused() {
    let w = full_world().await;
    assert!(matches!(
        w.svc.free(Trigger::Manual, "nope", &[1]).await,
        Err(StorageError::PlanExpired)
    ));
    assert!(matches!(
        w.svc.free(Trigger::Manual, "nope", &[]).await,
        Err(StorageError::NothingChosen)
    ));
    let _guard = w.svc.running.try_lock().unwrap();
    assert!(matches!(
        w.svc.plan().await,
        Err(StorageError::AlreadyRunning)
    ));
}

#[tokio::test]
async fn no_database_means_no_plan_and_no_runs() {
    let mut server = Server::new_async().await;
    server
        .mock("GET", "/account/info")
        .with_body(r#"{"status":"OK","info":{"user_id":1,"username":"u","mail":"m","account_active":true,"disk":{"size":10,"used":9,"avail":1}}}"#)
        .create_async()
        .await;
    let client = Arc::new(PutioApiClient::with_base_url("t".into(), server.url()));
    let svc = StorageService::from_config(client, None, &Config::default());
    assert!(matches!(svc.plan().await, Err(StorageError::NoDatabase)));
    assert!(svc.runs(20).await.unwrap().is_empty());
}

#[tokio::test]
async fn auto_run_only_when_on_and_below_the_warning_level() {
    // Off: nothing, not even an account read
    let w = full_world().await;
    assert!(w.svc.auto_run().await.unwrap().is_none());

    // On, but 50% free
    let mut config = Config::default();
    config.storage.auto_free = true;
    let mut w = World::new(config.clone()).await;
    w.account(100 * GB, 50 * GB).await;
    assert!(w.svc.auto_run().await.unwrap().is_none());

    // On and 5% free: a schedule run deletes the plan
    let mut w = World::new(config).await;
    w.account(100 * GB, 95 * GB).await;
    w.root(&[entry(1, 0, "f1.mkv", 20 * GB, false, "2026-01-01T00:00:00")])
        .await;
    w.transfers(&[]).await;
    w.downloaded(1, 20 * GB).await;
    w.details(1, 20 * GB).await;
    let delete = w.delete("1").await;
    let result = w.svc.auto_run().await.unwrap().unwrap();
    delete.assert_async().await;
    assert_eq!(ids(&result.deleted), vec![1]);
    assert_eq!(w.svc.runs(5).await.unwrap()[0].trigger, "schedule");
}

#[test]
fn low_is_strictly_below_the_level() {
    assert!(is_low(100, 9, 10));
    assert!(!is_low(100, 10, 10));
    assert!(!is_low(0, 0, 10));
}

#[tokio::test]
async fn storage_low_is_sent_on_the_way_down_only() {
    // A size no other test uses, since the event bus is shared
    const SIZE: i64 = 777 * GB;
    let mut rx = events::subscribe();
    let mut w = World::new(Config::default()).await;
    let warn = w.svc.config().warn_free_percent as i64;
    let low_used = SIZE - SIZE * (warn - 1) / 100;
    let lows = |rx: &mut tokio::sync::broadcast::Receiver<Arc<events::Event>>| {
        let mut n = Vec::new();
        while let Ok(event) = rx.try_recv() {
            if let Payload::StorageLow(l) = &event.payload {
                if l.total_bytes == SIZE {
                    n.push(l.clone());
                }
            }
        }
        n
    };

    let m = w.account(SIZE, SIZE / 2).await;
    w.svc.refresh().await.unwrap();
    assert!(lows(&mut rx).is_empty(), "not low");
    m.remove_async().await;

    let m = w.account(SIZE, low_used).await;
    w.svc.refresh().await.unwrap();
    let sent = lows(&mut rx);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].used_bytes, low_used);
    assert_eq!(sent[0].free_bytes, SIZE - low_used);
    assert_eq!(sent[0].warn_free_percent as i64, warn);
    assert!(sent[0].free_percent < warn as f64);
    w.svc.refresh().await.unwrap();
    assert!(lows(&mut rx).is_empty(), "still low sends nothing");
    m.remove_async().await;

    let m = w.account(SIZE, SIZE / 2).await;
    w.svc.refresh().await.unwrap();
    m.remove_async().await;
    w.account(SIZE, low_used).await;
    w.svc.refresh().await.unwrap();
    assert_eq!(lows(&mut rx).len(), 1, "low again after recovering");
}
