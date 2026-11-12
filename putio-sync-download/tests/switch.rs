//! The Pause local downloads switch (feature 039, R11, FR-020): on, running files stop and
//! go back to `pending` and nothing new starts; off, workers claim again and no row is
//! touched. It survives a restart.
//!
//! A local HTTP server stands in for Put.io and serves bodies slowly, so a download is
//! still running when the switch goes on.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use putio_sync_client::PutioApiClient;
use putio_sync_db::models::QueueStatus;
use putio_sync_db::Database;
use putio_sync_download::{DownloadConfig, DownloadManager};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

const SIZE: usize = 512 * 1024;
const CHUNK: usize = 8 * 1024;
/// 64 chunks of 100 ms: a whole file takes about 6.4 s, longer than the 5 s a stop may take
const CHUNK_DELAY: Duration = Duration::from_millis(100);

/// Byte `i` of every served file
fn byte(i: usize) -> u8 {
    (i % 251) as u8
}

/// One request the server saw: path and Range header
#[derive(Debug, Clone)]
struct Seen {
    path: String,
}

/// A slow Put.io stand-in: `GET /files/{id}` returns the file entry, and
/// `GET /files/{id}/download` streams `SIZE` bytes in small, delayed chunks, honouring
/// `Range: bytes=N-`.
async fn slow_server() -> (String, Arc<Mutex<Vec<Seen>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let log = log.clone();
            tokio::spawn(async move {
                let (read, mut write) = stream.into_split();
                let mut reader = BufReader::new(read);
                let mut line = String::new();
                if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                    return;
                }
                let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
                let path = path.split('?').next().unwrap_or("").to_string();
                let mut range = None;
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).await.unwrap_or(0) == 0 {
                        break;
                    }
                    let header = header.trim_end();
                    if header.is_empty() {
                        break;
                    }
                    if let Some((name, value)) = header.split_once(':') {
                        if name.eq_ignore_ascii_case("range") {
                            range = Some(value.trim().to_string());
                        }
                    }
                }
                log.lock().unwrap().push(Seen { path: path.clone() });

                let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
                match parts.as_slice() {
                    ["files", id] => {
                        let body = format!(
                            r#"{{"file":{{"id":{id},"name":"file{id}.mkv","size":{SIZE},"file_type":"VIDEO","parent_id":0,"is_folder":false,"is_shared":false}},"status":"OK"}}"#
                        );
                        let head = format!(
                            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                            body.len()
                        );
                        let _ = write.write_all(head.as_bytes()).await;
                        let _ = write.write_all(body.as_bytes()).await;
                    }
                    ["files", _, "download"] => {
                        let start = range
                            .as_deref()
                            .and_then(|r| r.strip_prefix("bytes="))
                            .and_then(|r| r.split('-').next())
                            .and_then(|n| n.parse::<usize>().ok())
                            .unwrap_or(0);
                        let head = if range.is_some() {
                            format!(
                                "HTTP/1.1 206 Partial Content\r\ncontent-length: {}\r\ncontent-range: bytes {}-{}/{}\r\nconnection: close\r\n\r\n",
                                SIZE - start,
                                start,
                                SIZE - 1,
                                SIZE
                            )
                        } else {
                            format!(
                                "HTTP/1.1 200 OK\r\ncontent-length: {SIZE}\r\nconnection: close\r\n\r\n"
                            )
                        };
                        if write.write_all(head.as_bytes()).await.is_err() {
                            return;
                        }
                        let mut at = start;
                        while at < SIZE {
                            let end = (at + CHUNK).min(SIZE);
                            let chunk: Vec<u8> = (at..end).map(byte).collect();
                            if write.write_all(&chunk).await.is_err() {
                                return;
                            }
                            let _ = write.flush().await;
                            at = end;
                            tokio::time::sleep(CHUNK_DELAY).await;
                        }
                    }
                    _ => {
                        let _ = write
                            .write_all(b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\n\r\n")
                            .await;
                    }
                }
                let _ = write.shutdown().await;
            });
        }
    });
    (url, seen)
}

async fn setup(url: &str, workers: usize) -> (tempfile::TempDir, Arc<Database>, DownloadManager) {
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(
        Database::new(dir.path().join(".putiosync.db"))
            .await
            .unwrap(),
    );
    let client = Arc::new(PutioApiClient::with_base_url("tok".into(), url.to_string()));
    let config = DownloadConfig {
        target_directory: dir.path().to_path_buf(),
        max_retries: 0,
        retry_initial_delay_secs: 0,
        workers,
        ..DownloadConfig::default()
    };
    let manager = DownloadManager::new(client, db.clone(), config);
    (dir, db, manager)
}

fn partial(dir: &Path, id: i64) -> std::path::PathBuf {
    dir.join(format!(".file{id}.mkv.downloading"))
}

fn len(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// Wait until `cond` holds, failing after `limit`
async fn wait_for(limit: Duration, what: &str, mut cond: impl FnMut() -> bool) {
    let start = Instant::now();
    while !cond() {
        assert!(start.elapsed() < limit, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Whether the server has seen a download request for file `id`
fn requested(seen: &Mutex<Vec<Seen>>, id: i64) -> bool {
    let path = format!("/files/{id}/download");
    seen.lock().unwrap().iter().any(|s| s.path == path)
}

fn manager_for(dir: &Path, db: &Arc<Database>, url: &str) -> DownloadManager {
    let client = Arc::new(PutioApiClient::with_base_url("tok".into(), url.to_string()));
    let config = DownloadConfig {
        target_directory: dir.to_path_buf(),
        max_retries: 0,
        retry_initial_delay_secs: 0,
        workers: 1,
        ..DownloadConfig::default()
    };
    DownloadManager::new(client, db.clone(), config)
}

async fn status_of(db: &Database, file_id: i64) -> String {
    db.queue()
        .get_by_file_id(file_id)
        .await
        .unwrap()
        .unwrap()
        .status
}

#[tokio::test]
async fn switch_on_stops_running_files_and_requeues_them_pending() {
    let (url, seen) = slow_server().await;
    let (dir, db, manager) = setup(&url, 1).await;
    for (id, priority) in [(1, 5), (2, 4), (3, 3)] {
        db.queue()
            .add(
                id,
                &format!("file{id}.mkv"),
                SIZE as i64,
                "",
                "",
                priority,
                QueueStatus::Pending,
            )
            .await
            .unwrap();
    }
    // File 3 was paused on its own before the switch
    db.queue().pause_files(&[3]).await.unwrap();

    let worker = manager.clone();
    let run = tokio::spawn(async move { worker.process_queue().await });
    wait_for(Duration::from_secs(5), "file 1 to start", || {
        requested(&seen, 1)
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;

    manager.set_nas_paused(true).await.unwrap();
    assert!(manager.nas_paused().await);
    let switched_at = Instant::now();
    tokio::time::timeout(Duration::from_secs(5), run)
        .await
        .expect("the download stops within 5 s")
        .unwrap()
        .unwrap();
    assert!(switched_at.elapsed() < Duration::from_secs(5));

    // The stopped file is queued again, not paused, and keeps its partial
    assert_eq!(status_of(&db, 1).await, "pending");
    assert_eq!(status_of(&db, 2).await, "pending");
    assert_eq!(status_of(&db, 3).await, "paused");
    assert!(len(&partial(dir.path(), 1)) > 0);

    // Nothing is claimed while the switch is on, and nothing is requested
    assert!(!manager.process_queue().await.unwrap());
    assert!(!requested(&seen, 2));
    assert_eq!(status_of(&db, 2).await, "pending");

    // A file a sync queues meanwhile stays queued
    db.queue()
        .add(4, "file4.mkv", SIZE as i64, "", "", 1, QueueStatus::Pending)
        .await
        .unwrap();
    assert!(!manager.process_queue().await.unwrap());
    assert_eq!(status_of(&db, 4).await, "pending");
    assert!(!requested(&seen, 4));
}

#[tokio::test]
async fn switch_off_lets_workers_claim_again_without_touching_rows() {
    let (url, seen) = slow_server().await;
    let (_dir, db, manager) = setup(&url, 1).await;
    db.queue()
        .add(1, "file1.mkv", SIZE as i64, "", "", 2, QueueStatus::Pending)
        .await
        .unwrap();
    db.queue()
        .add(2, "file2.mkv", SIZE as i64, "", "", 1, QueueStatus::Pending)
        .await
        .unwrap();
    db.queue().pause_files(&[2]).await.unwrap();
    manager.set_nas_paused(true).await.unwrap();
    assert!(!manager.process_queue().await.unwrap());

    let before = db.queue().get_all().await.unwrap();
    manager.set_nas_paused(false).await.unwrap();
    assert!(!manager.nas_paused().await);
    let after = db.queue().get_all().await.unwrap();
    let rows = |list: &[putio_sync_db::models::QueueItem]| {
        list.iter()
            .map(|r| (r.file_id, r.status.clone(), r.priority))
            .collect::<Vec<_>>()
    };
    assert_eq!(rows(&before), rows(&after), "switching off touches no row");

    // Workers claim file 1 again; file 2 stays paused
    let worker = manager.clone();
    let run = tokio::spawn(async move { worker.process_queue().await });
    wait_for(Duration::from_secs(5), "file 1 to start", || {
        requested(&seen, 1)
    })
    .await;
    assert_eq!(status_of(&db, 2).await, "paused");
    db.queue().pause_files(&[1]).await.unwrap();
    manager.stop_files(&[1]).await;
    tokio::time::timeout(Duration::from_secs(5), run)
        .await
        .expect("the download stops")
        .unwrap()
        .unwrap();
    assert!(!requested(&seen, 2));
}

#[tokio::test]
async fn the_switch_survives_reopening_the_database() {
    let (url, _seen) = slow_server().await;
    let (dir, db, manager) = setup(&url, 1).await;
    assert!(!manager.nas_paused().await, "a missing flag means off");
    manager.set_nas_paused(true).await.unwrap();
    drop(manager);
    drop(db);

    let db = Arc::new(
        Database::new(dir.path().join(".putiosync.db"))
            .await
            .unwrap(),
    );
    let manager = manager_for(dir.path(), &db, &url);
    assert!(manager.nas_paused().await);
    db.queue()
        .add(1, "file1.mkv", SIZE as i64, "", "", 0, QueueStatus::Pending)
        .await
        .unwrap();
    assert!(!manager.process_queue().await.unwrap());
    manager.set_nas_paused(false).await.unwrap();
    assert!(!manager.nas_paused().await);
}
