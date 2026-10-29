//! Pausing a running download stops it within 5 s and keeps its partial file; resuming
//! continues from the partial with a Range request (feature 039, R7, R8, FR-014, SC-007).
//!
//! A local HTTP server stands in for Put.io and serves the body slowly, so the download is
//! still running when it is paused.

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
    range: Option<String>,
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
                log.lock().unwrap().push(Seen {
                    path: path.clone(),
                    range: range.clone(),
                });

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

fn failures() -> f64 {
    putio_sync_telemetry::FailReason::ALL
        .iter()
        .filter_map(|r| {
            putio_sync_telemetry::global().sample(
                "putio_sync_downloads_failed_total",
                &[("reason", r.as_str())],
            )
        })
        .sum()
}

#[tokio::test]
async fn pause_stops_a_running_download_and_resume_continues_from_the_partial() {
    let (url, seen) = slow_server().await;
    let (dir, db, manager) = setup(&url, 1).await;
    db.queue()
        .add(1, "file1.mkv", SIZE as i64, "", "", 0, QueueStatus::Pending)
        .await
        .unwrap();
    let failed_before = failures();

    let worker = manager.clone();
    let run = tokio::spawn(async move { worker.process_queue().await });

    // The writer buffers in memory, so wait for the request and some bytes on the wire
    let part = partial(dir.path(), 1);
    wait_for(Duration::from_secs(5), "the download to start", || {
        requested(&seen, 1)
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;

    // Pause: the caller sets the row's status, then stops the running file
    db.queue()
        .update_status(1, QueueStatus::Paused)
        .await
        .unwrap();
    let paused_at = Instant::now();
    manager.stop_files(&[1]).await;

    let processed = tokio::time::timeout(Duration::from_secs(5), run)
        .await
        .expect("the download stops within 5 s")
        .unwrap()
        .unwrap();
    assert!(processed);
    assert!(paused_at.elapsed() < Duration::from_secs(5));

    // No more bytes arrive, the partial is kept and nothing is marked done or failed
    let kept = len(&part);
    assert!(kept > 0 && kept < SIZE as u64, "partial has {kept} bytes");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(len(&part), kept, "no bytes arrive after the stop");
    assert!(!dir.path().join("file1.mkv").exists());
    let row = db
        .queue()
        .get_by_file_id(1)
        .await
        .unwrap()
        .expect("row kept");
    assert_eq!(row.status, "paused");
    assert!(db
        .downloads()
        .find_by_file_id(1)
        .await
        .unwrap()
        .is_none_or(|d| d.status != "completed"));
    assert_eq!(failures(), failed_before, "a stop is not a failure");

    // Resume: a Range request from the partial size, then a correct final file
    db.queue()
        .update_status(1, QueueStatus::Pending)
        .await
        .unwrap();
    assert!(manager.process_queue().await.unwrap());
    let ranges: Vec<Option<String>> = seen
        .lock()
        .unwrap()
        .iter()
        .filter(|s| s.path == "/files/1/download")
        .map(|s| s.range.clone())
        .collect();
    assert_eq!(ranges.len(), 2);
    assert_eq!(ranges[0], None);
    assert_eq!(
        ranges[1].as_deref(),
        Some(format!("bytes={}-{}", kept, SIZE - 1).as_str())
    );
    let data = std::fs::read(dir.path().join("file1.mkv")).unwrap();
    assert_eq!(data.len(), SIZE);
    assert!(data.iter().enumerate().all(|(i, b)| *b == byte(i)));
    assert!(!part.exists());
    assert!(db.queue().get_by_file_id(1).await.unwrap().is_none());
}

#[tokio::test]
async fn a_stopped_download_frees_its_slot_for_the_next_file() {
    let (url, seen) = slow_server().await;
    let (dir, db, manager) = setup(&url, 1).await;
    // File 1 first (higher priority); file 2, of another transfer, waits behind it
    db.queue()
        .add(1, "file1.mkv", SIZE as i64, "", "", 5, QueueStatus::Pending)
        .await
        .unwrap();
    db.queue()
        .add(2, "file2.mkv", SIZE as i64, "", "", 1, QueueStatus::Pending)
        .await
        .unwrap();

    manager.start_workers().await;
    let part = partial(dir.path(), 1);
    wait_for(Duration::from_secs(5), "file 1 to start", || {
        requested(&seen, 1)
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!requested(&seen, 2));

    db.queue()
        .update_status(1, QueueStatus::Paused)
        .await
        .unwrap();
    manager.stop_files(&[1]).await;

    wait_for(Duration::from_secs(5), "file 2 to start", || {
        requested(&seen, 2)
    })
    .await;
    // Stop file 2 too, so the worker shuts down without waiting for it
    db.queue()
        .update_status(2, QueueStatus::Paused)
        .await
        .unwrap();
    manager.stop_files(&[2]).await;
    manager.shutdown_all_workers().await;
    assert_eq!(
        db.queue().get_by_file_id(1).await.unwrap().unwrap().status,
        "paused"
    );
    assert!(part.exists());
}
