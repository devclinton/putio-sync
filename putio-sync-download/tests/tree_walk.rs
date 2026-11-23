//! The sync's Put.io tree walk lists a few folders at once, and the files come out in
//! the order a one-at-a-time walk gives

use putio_sync_client::PutioApiClient;
use putio_sync_db::Database;
use putio_sync_download::{DownloadConfig, DownloadManager};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn entry(id: i64, name: &str, folder: bool, parent_id: i64) -> String {
    format!(
        r#"{{"id":{id},"name":"{name}","size":100,"file_type":"{}","parent_id":{parent_id},"is_folder":{folder},"is_shared":false}}"#,
        if folder { "FOLDER" } else { "VIDEO" }
    )
}

/// Folder 50 holds folders 60..=65 (and file 1); 61 also holds folder 70
fn listing(id: i64) -> Vec<String> {
    match id {
        50 => (60..=65)
            .map(|f| entry(f, &format!("F{f}"), true, 50))
            .chain([entry(1, "root.mkv", false, 50)])
            .collect(),
        70 => vec![entry(700, "deep.mkv", false, 70)],
        61 => vec![entry(610, "a.mkv", false, 61), entry(70, "Sub", true, 61)],
        f @ 60..=65 => vec![entry(f * 10, "a.mkv", false, f)],
        _ => vec![],
    }
}

async fn server(active: Arc<AtomicUsize>, peak: Arc<AtomicUsize>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let (active, peak) = (active.clone(), peak.clone());
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = request.split_whitespace().nth(1).unwrap_or("").to_string();
                let body = if let Some(id) = path.strip_prefix("/files/list?parent_id=") {
                    let id: i64 = id.parse().unwrap();
                    let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(now, Ordering::SeqCst);
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    active.fetch_sub(1, Ordering::SeqCst);
                    format!(
                        r#"{{"files":[{}],"parent":{},"status":"OK"}}"#,
                        listing(id).join(","),
                        entry(id, "x", true, 0)
                    )
                } else if path.starts_with("/files/") {
                    format!(r#"{{"file":{},"status":"OK"}}"#, entry(50, "Show", true, 0))
                } else {
                    r#"{"transfers":[],"status":"OK"}"#.to_string()
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });
    url
}

#[tokio::test]
async fn folders_are_listed_concurrently_but_assembled_in_walk_order() {
    let (active, peak) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let url = server(active, peak.clone()).await;
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(
        Database::new(dir.path().join(".putiosync.db"))
            .await
            .unwrap(),
    );
    let client = Arc::new(PutioApiClient::with_base_url("tok".into(), url));
    let config = DownloadConfig {
        target_directory: dir.path().to_path_buf(),
        ..DownloadConfig::default()
    };
    let manager = DownloadManager::new(client, db, config);

    let report = manager.enqueue_tree_report(50, false).await.unwrap();
    let ids: Vec<i64> = report.files.iter().map(|f| f.file_id).collect();
    // Subfolders in reverse listing order, each with its own subtree straight after it
    assert_eq!(ids, [1, 650, 640, 630, 620, 610, 700, 600]);
    let peak = peak.load(Ordering::SeqCst);
    assert!(peak > 1, "folders were listed one at a time");
    assert!(peak <= 4, "{peak} listings at once");
}
