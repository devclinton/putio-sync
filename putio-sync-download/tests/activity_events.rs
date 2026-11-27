//! The activity events of feature 057 against a mock Put.io: `sync.started`,
//! `download.started`, `download.progress` and `cleanup.deleted`. The event bus is
//! process-wide, so each test looks only at its own file ids.

use std::sync::Arc;
use std::time::Duration;

use putio_sync_client::{PutioApiClient, RetryPolicy};
use putio_sync_core::events::{self, Event, EventType, Payload};
use putio_sync_db::models::{DownloadStatus, QueueStatus};
use putio_sync_db::Database;
use putio_sync_download::{DownloadConfig, DownloadFile, DownloadManager};
use tokio::sync::broadcast::Receiver;

const SIZE: usize = 2048;
const ROOT: &str = r#"{"id":0,"name":"Root","size":0,"file_type":"FOLDER","parent_id":0,"is_folder":true,"is_shared":false}"#;

fn file(id: i64, name: &str, dir: &str, url: String) -> DownloadFile {
    DownloadFile {
        file_id: id,
        filename: name.to_string(),
        relative_path: dir.to_string(),
        file_size: SIZE as u64,
        download_url: url,
        crc32: None,
        save_as: None,
        fixed_path: false,
        overwrite: false,
    }
}

/// Everything on the bus so far, checked against the schema
fn seen(rx: &mut Receiver<Arc<Event>>) -> Vec<Arc<Event>> {
    let mut out = Vec::new();
    while let Ok(event) = rx.try_recv() {
        events::validate_against_schema(&serde_json::to_value(&*event).unwrap()).unwrap();
        out.push(event);
    }
    out
}

/// The download events of `file_id`, in order
fn of_file(events: &[Arc<Event>], file_id: i64) -> Vec<Payload> {
    events
        .iter()
        .filter(|e| match &e.payload {
            Payload::DownloadStarted(d) => d.file_id == file_id,
            Payload::DownloadProgress(d) => d.file_id == file_id,
            Payload::DownloadFinished(d) => d.file_id == file_id,
            Payload::DownloadFailed(d) => d.file_id == file_id,
            _ => false,
        })
        .map(|e| e.payload.clone())
        .collect()
}

struct World {
    server: mockito::ServerGuard,
    dir: tempfile::TempDir,
    db: Arc<Database>,
    manager: DownloadManager,
}

async fn world(delete_after_download: bool) -> World {
    let server = mockito::Server::new_async().await;
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(
        Database::new(dir.path().join(".putiosync.db"))
            .await
            .unwrap(),
    );
    let client = Arc::new(
        PutioApiClient::with_base_url("tok_SECRET".into(), server.url()).with_retry_policy(
            RetryPolicy {
                max_attempts: 1,
                ..RetryPolicy::default()
            },
        ),
    );
    let manager = DownloadManager::new(
        client,
        db.clone(),
        DownloadConfig {
            target_directory: dir.path().to_path_buf(),
            max_retries: 0,
            retry_initial_delay_secs: 0,
            delete_after_download,
            ..DownloadConfig::default()
        },
    );
    World {
        server,
        dir,
        db,
        manager,
    }
}

#[tokio::test]
async fn every_download_has_a_start_before_its_end() {
    let mut w = world(false).await;
    w.server
        .mock("GET", "/files/8101/download")
        .with_body(vec![1u8; SIZE])
        .create_async()
        .await;
    w.server
        .mock("GET", "/files/8102/download")
        .with_status(500)
        .create_async()
        .await;
    let mut rx = events::subscribe();

    let url = w.server.url();
    w.manager
        .download_file(
            file(
                8101,
                "ep.mkv",
                "Shows/S1",
                format!("{url}/files/8101/download"),
            ),
            None,
        )
        .await
        .unwrap();
    w.manager
        .download_file(
            file(8102, "bad.mkv", "", format!("{url}/files/8102/download")),
            None,
        )
        .await
        .unwrap();
    // Already there and the same size: skipped, so no start either
    w.manager
        .download_file(
            file(
                8101,
                "ep.mkv",
                "Shows/S1",
                format!("{url}/files/8101/download"),
            ),
            None,
        )
        .await
        .unwrap();
    let events = seen(&mut rx);

    let good = of_file(&events, 8101);
    assert_eq!(good.len(), 2, "{good:?}");
    let Payload::DownloadStarted(d) = &good[0] else {
        panic!("{:?}", good[0])
    };
    assert_eq!(d.name, "ep.mkv");
    assert_eq!(d.relative_path, "Shows/S1/ep.mkv");
    assert_eq!(d.size_bytes, SIZE as u64);
    assert!(!d.resumed);
    assert_eq!(d.resumed_from_bytes, 0);
    assert_eq!(good[1].event_type(), EventType::DownloadFinished);

    // A download that fails before its first byte still has a start
    let bad = of_file(&events, 8102);
    let types: Vec<EventType> = bad.iter().map(Payload::event_type).collect();
    assert_eq!(
        types,
        vec![EventType::DownloadStarted, EventType::DownloadFailed]
    );
}

#[tokio::test]
async fn a_resumed_download_says_so_in_its_start() {
    let mut w = world(false).await;
    w.server
        .mock("GET", "/files/8201/download")
        .match_header("range", "bytes=1024-")
        .with_status(206)
        .with_header("content-range", "bytes 1024-2047/2048")
        .with_body(vec![1u8; SIZE / 2])
        .create_async()
        .await;
    std::fs::write(
        w.dir.path().join(".part.mkv.downloading"),
        vec![1u8; SIZE / 2],
    )
    .unwrap();
    let mut rx = events::subscribe();
    let url = format!("{}/files/8201/download", w.server.url());
    w.manager
        .download_file(file(8201, "part.mkv", "", url), None)
        .await
        .unwrap();
    let mine = of_file(&seen(&mut rx), 8201);
    let Some(Payload::DownloadStarted(d)) = mine.first() else {
        panic!("{mine:?}")
    };
    assert!(d.resumed);
    assert_eq!(d.resumed_from_bytes, (SIZE / 2) as u64);
    // One start, then the end
    assert_eq!(mine.len(), 2, "{mine:?}");
}

#[tokio::test]
async fn progress_is_sent_while_a_file_downloads_and_no_faster_than_the_period() {
    let mut w = world(false).await;
    // Half the file, a pause, then the rest
    w.server
        .mock("GET", "/files/8301/download")
        .with_chunked_body(|out| {
            out.write_all(&[1u8; SIZE / 2])?;
            out.flush()?;
            std::thread::sleep(Duration::from_millis(900));
            out.write_all(&[1u8; SIZE / 2])
        })
        .create_async()
        .await;
    let period = Duration::from_millis(200);
    w.manager.set_progress_event_period(period);
    let mut rx = events::subscribe();
    let url = format!("{}/files/8301/download", w.server.url());
    let started = std::time::Instant::now();
    w.manager
        .download_file(file(8301, "slow.mkv", "Shows", url), None)
        .await
        .unwrap();
    let took = started.elapsed();
    let all = seen(&mut rx);
    let mine = of_file(&all, 8301);
    let progress: Vec<&events::DownloadProgress> = mine
        .iter()
        .filter_map(|p| match p {
            Payload::DownloadProgress(d) => Some(d),
            _ => None,
        })
        .collect();
    assert!(!progress.is_empty(), "{mine:?}");
    // At most one per period
    let most = (took.as_millis() / period.as_millis()) as usize + 1;
    assert!(progress.len() <= most, "{} in {took:?}", progress.len());
    for p in &progress {
        assert_eq!(p.name, "slow.mkv");
        assert_eq!(p.relative_path, "Shows/slow.mkv");
        assert_eq!(p.size_bytes, SIZE as u64);
        assert!(p.downloaded_bytes <= p.size_bytes);
        assert!((0.0..=100.0).contains(&p.percent));
    }
    // Between the start and the end
    assert_eq!(mine[0].event_type(), EventType::DownloadStarted);
    assert_eq!(
        mine.last().unwrap().event_type(),
        EventType::DownloadFinished
    );
    // Only a subscriber that names it gets it
    assert!(EventType::DownloadProgress.matched_by("download.progress"));
    assert!(!EventType::DownloadProgress.matched_by("*"));
}

#[tokio::test]
async fn a_sync_run_starts_with_sync_started() {
    let mut w = world(false).await;
    w.server
        .mock("GET", "/files/list?parent_id=0")
        .with_header("content-type", "application/json")
        .with_body(format!(r#"{{"files":[],"parent":{ROOT},"status":"OK"}}"#))
        .create_async()
        .await;
    let mut rx = events::subscribe();
    // Nothing to do: the pair is still sent
    w.manager.sync_downloads().await.unwrap();
    let runs: Vec<Payload> = seen(&mut rx)
        .iter()
        .filter(|e| {
            matches!(
                e.payload,
                Payload::SyncStarted(_) | Payload::SyncCompleted(_)
            )
        })
        .map(|e| e.payload.clone())
        .collect();
    assert_eq!(runs.len(), 2, "{runs:?}");
    let (Payload::SyncStarted(start), Payload::SyncCompleted(end)) = (&runs[0], &runs[1]) else {
        panic!("{runs:?}")
    };
    assert_eq!(start.trigger, "schedule");
    assert_eq!(start.trigger, end.trigger);
    assert_eq!(start.started_at, end.started_at);
}

#[tokio::test]
async fn delete_after_download_sends_cleanup_deleted() {
    let mut w = world(true).await;
    w.server
        .mock("GET", "/files/8401")
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"file":{{"id":8401,"name":"movie.mkv","size":{SIZE},"file_type":"VIDEO","parent_id":0,"is_folder":false,"is_shared":false}},"status":"OK"}}"#
        ))
        .create_async()
        .await;
    w.server
        .mock("GET", "/files/8401/download")
        .with_body(vec![1u8; SIZE])
        .create_async()
        .await;
    let delete = w
        .server
        .mock("POST", "/files/delete")
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"OK"}"#)
        .expect(2)
        .create_async()
        .await;
    w.db.queue()
        .add_with_path_kept(
            8401,
            "movie.mkv",
            SIZE as i64,
            "",
            "",
            0,
            QueueStatus::Pending,
            false,
        )
        .await
        .unwrap();
    let mut rx = events::subscribe();
    let deleted = |rx: &mut Receiver<Arc<Event>>, id: i64| -> Vec<events::CleanupDeleted> {
        seen(rx)
            .iter()
            .filter_map(|e| match &e.payload {
                Payload::CleanupDeleted(d) if d.file_ids.contains(&id) => Some(d.clone()),
                _ => None,
            })
            .collect()
    };

    assert!(w.manager.process_queue().await.unwrap());
    let sent = deleted(&mut rx, 8401);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].file_ids, vec![8401]);
    assert_eq!(sent[0].count, 1);
    assert_eq!(sent[0].reason, "downloaded");

    // A delete the pipeline held back and lets go later
    w.db.downloads()
        .insert(8402, "a.mkv", 10, "/d/a.mkv", DownloadStatus::Completed)
        .await
        .unwrap();
    w.manager.delete_remote(&[8402, 8403]).await;
    let sent = deleted(&mut rx, 8402);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].file_ids, vec![8402]);
    delete.assert_async().await;
}
