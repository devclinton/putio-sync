//! Every new download is checked against Put.io's CRC32 before its rename, and
//! downloaded again on a mismatch (feature 034). Small payloads against mockito;
//! nothing reaches Put.io.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use mockito::{Matcher, Mock, ServerGuard};
use putio_sync_client::PutioApiClient;
use putio_sync_core::events::{self, Event, Payload};
use putio_sync_db::models::QueueStatus;
use putio_sync_db::Database;
use putio_sync_download::{DownloadConfig, DownloadManager, VerifySettings};
use tokio::sync::broadcast;

const SIZE: usize = 8192;
const TOKEN: &str = "secret-token-034";

fn crc(data: &[u8]) -> String {
    format!(
        "{:08x}",
        crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC).checksum(data)
    )
}

fn good() -> Vec<u8> {
    (0..SIZE).map(|i| (i % 251) as u8).collect()
}

fn bad() -> Vec<u8> {
    let mut data = good();
    data[100] ^= 0xff;
    data
}

struct Setup {
    server: ServerGuard,
    dir: tempfile::TempDir,
    db: Arc<Database>,
    manager: DownloadManager,
}

async fn setup(verify: VerifySettings, delete_after_download: bool) -> Setup {
    let server = mockito::Server::new_async().await;
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(
        Database::new(dir.path().join(".putiosync.db"))
            .await
            .unwrap(),
    );
    let client = Arc::new(PutioApiClient::with_base_url(TOKEN.into(), server.url()));
    let config = DownloadConfig {
        target_directory: dir.path().to_path_buf(),
        max_retries: 0,
        retry_initial_delay_secs: 0,
        delete_after_download,
        verify,
        ..DownloadConfig::default()
    };
    let manager = DownloadManager::new(client, db.clone(), config);
    Setup {
        server,
        dir,
        db,
        manager,
    }
}

fn verify_on(max_redownloads: u32) -> VerifySettings {
    VerifySettings {
        max_redownloads,
        ..VerifySettings::default()
    }
}

impl Setup {
    fn path(&self, name: &str) -> std::path::PathBuf {
        self.dir.path().join(name)
    }

    /// Put.io's file `id`, `movie.mkv`, with an optional CRC32
    async fn file_details(&mut self, id: i64, crc32: Option<&str>) -> Mock {
        let crc32 = crc32.map_or("null".to_string(), |c| format!("\"{c}\""));
        self.server
            .mock("GET", format!("/files/{id}").as_str())
            .with_header("content-type", "application/json")
            .with_body(format!(
                r#"{{"file":{{"id":{id},"name":"movie.mkv","size":{SIZE},"file_type":"VIDEO","parent_id":0,"is_folder":false,"is_shared":false,"crc32":{crc32}}},"status":"OK"}}"#
            ))
            .create_async()
            .await
    }

    /// The whole file: the n-th request (from 0) gets `bodies[min(n, last)]`
    async fn bodies(&mut self, id: i64, bodies: Vec<Vec<u8>>) -> (Mock, Arc<AtomicUsize>) {
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        let mock = self
            .server
            .mock("GET", format!("/files/{id}/download").as_str())
            .match_header("range", Matcher::Missing)
            .with_body_from_request(move |_| {
                let n = counter.fetch_add(1, Ordering::SeqCst);
                bodies[n.min(bodies.len() - 1)].clone()
            })
            .create_async()
            .await;
        (mock, hits)
    }

    async fn queue(&self, id: i64) {
        self.db
            .queue()
            .add_with_path_kept(
                id,
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
    }

    async fn queue_status(&self, id: i64) -> Option<String> {
        self.db
            .queue()
            .get_by_file_id(id)
            .await
            .unwrap()
            .map(|item| item.status)
    }
}

/// Every verify and download event for `file_id` until `download.finished` or
/// `download.failed`, or until a short wait ends
async fn events_for(rx: &mut broadcast::Receiver<Arc<Event>>, file_id: i64) -> Vec<Arc<Event>> {
    let mut seen = Vec::new();
    let wait = async {
        loop {
            let Ok(event) = rx.recv().await else {
                return;
            };
            let (id, last) = match &event.payload {
                Payload::VerifyPassed(e) => (e.file_id, false),
                Payload::VerifyFailed(e) => (e.file_id, false),
                Payload::VerifyRedownload(e) => (e.file_id, false),
                Payload::DownloadFinished(e) => (e.file_id, true),
                Payload::DownloadFailed(e) => (e.file_id, true),
                _ => continue,
            };
            if id == file_id {
                seen.push(event);
                if last {
                    return;
                }
            }
        }
    };
    let _ = tokio::time::timeout(Duration::from_millis(500), wait).await;
    seen
}

fn types(events: &[Arc<Event>]) -> Vec<&'static str> {
    events.iter().map(|e| e.event_type().as_str()).collect()
}

fn read_bytes_metric() -> u64 {
    putio_sync_telemetry::global()
        .encode()
        .lines()
        .find_map(|l| l.strip_prefix("putio_sync_verify_read_bytes_total "))
        .and_then(|v| v.trim().parse::<f64>().ok())
        .unwrap_or(0.0) as u64
}

#[tokio::test]
async fn mismatch_then_match_downloads_twice_and_finishes_after_verify_passed() {
    let mut s = setup(verify_on(2), false).await;
    let _file = s.file_details(3401, Some(&crc(&good()))).await;
    let (_mock, hits) = s.bodies(3401, vec![bad(), good()]).await;
    s.queue(3401).await;
    let mut rx = events::subscribe();

    assert!(s.manager.process_queue().await.unwrap());

    assert_eq!(hits.load(Ordering::SeqCst), 2);
    assert_eq!(std::fs::read(s.path("movie.mkv")).unwrap(), good());
    assert!(!s.path(".movie.mkv.downloading").exists());
    let seen = events_for(&mut rx, 3401).await;
    assert_eq!(
        types(&seen),
        [
            "verify.failed",
            "verify.redownload",
            "verify.passed",
            "download.finished"
        ]
    );
    let Payload::VerifyFailed(f) = &seen[0].payload else {
        unreachable!()
    };
    assert_eq!(f.expected_crc32.as_deref(), Some(crc(&good()).as_str()));
    assert_eq!(f.actual_crc32.as_deref(), Some(crc(&bad()).as_str()));
    assert_eq!((f.attempt, f.will_redownload), (1, true));
    assert_eq!(f.trigger, "download");
    let Payload::VerifyPassed(p) = &seen[2].payload else {
        unreachable!()
    };
    assert_eq!((p.method.as_str(), p.redownloads), ("crc32", 1));

    let history = s.db.history().query(None, 50, 0, None).await.unwrap();
    assert_eq!(history[0].verify.as_deref(), Some("crc32"));
    assert_eq!(history[0].redownloads, Some(1));
    let row =
        s.db.downloads()
            .find_by_file_id(3401)
            .await
            .unwrap()
            .unwrap();
    assert_eq!(row.crc32.as_deref(), Some(crc(&good()).as_str()));
    assert_eq!(row.verify.as_deref(), Some("crc32"));
    assert!(row.verified_at.is_some());
}

/// T024: a bus subscriber gets `verify.failed` with the fields EVENTS.md documents, and
/// /metrics counts the check and the re-download
#[tokio::test]
async fn subscribers_and_metrics_see_a_mismatch() {
    let counter = |result: &str| {
        putio_sync_telemetry::global()
            .sample(
                "putio_sync_verify_total",
                &[
                    ("trigger", "download"),
                    ("method", "crc32"),
                    ("result", result),
                ],
            )
            .unwrap_or(0.0)
    };
    let redownloads = || {
        putio_sync_telemetry::global()
            .sample(
                "putio_sync_verify_redownloads_total",
                &[("trigger", "download")],
            )
            .unwrap_or(0.0)
    };
    let (failed_before, redownloads_before) = (counter("failed"), redownloads());
    let mut s = setup(verify_on(1), false).await;
    let _file = s.file_details(3409, Some(&crc(&good()))).await;
    let _bodies = s.bodies(3409, vec![bad(), good()]).await;
    s.queue(3409).await;
    let mut rx = events::subscribe();

    assert!(s.manager.process_queue().await.unwrap());

    let seen = events_for(&mut rx, 3409).await;
    let failed = seen
        .iter()
        .find(|e| e.event_type().as_str() == "verify.failed")
        .expect("verify.failed");
    let json = serde_json::to_value(&**failed).unwrap();
    assert_eq!(json["schema_version"], 1);
    let data = json["data"].as_object().unwrap();
    let mut keys: Vec<_> = data.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "actual_crc32",
            "attempt",
            "expected_crc32",
            "file_id",
            "local_path",
            "name",
            "relative_path",
            "size_bytes",
            "trigger",
            "will_redownload"
        ]
    );
    assert_eq!(data["relative_path"], "movie.mkv");

    // Tests share the global registry, so only growth is checked
    assert!(counter("failed") > failed_before);
    assert!(redownloads() > redownloads_before);
    let text = putio_sync_telemetry::global().encode();
    assert!(text.contains("putio_sync_verify_total{"), "{text}");
    assert!(
        text.contains("putio_sync_verify_read_bytes_total"),
        "{text}"
    );
}

#[tokio::test]
async fn always_mismatching_fails_with_integrity_and_is_never_deleted_from_putio() {
    let mut s = setup(verify_on(2), true).await;
    let _file = s.file_details(3402, Some(&crc(&good()))).await;
    let (_mock, hits) = s.bodies(3402, vec![bad()]).await;
    let delete = s
        .server
        .mock("POST", "/files/delete")
        .expect(0)
        .create_async()
        .await;
    s.queue(3402).await;
    let mut rx = events::subscribe();

    assert!(s.manager.process_queue().await.unwrap());

    assert_eq!(hits.load(Ordering::SeqCst), 3);
    assert!(!s.path("movie.mkv").exists());
    assert!(!s.path(".movie.mkv.downloading").exists());
    delete.assert_async().await;
    assert_eq!(s.queue_status(3402).await.as_deref(), Some("failed"));

    let seen = events_for(&mut rx, 3402).await;
    assert_eq!(
        types(&seen),
        [
            "verify.failed",
            "verify.redownload",
            "verify.failed",
            "verify.redownload",
            "verify.failed",
            "download.failed"
        ]
    );
    let Payload::VerifyFailed(last) = &seen[4].payload else {
        unreachable!()
    };
    assert_eq!((last.attempt, last.will_redownload), (3, false));
    let Payload::DownloadFailed(failed) = &seen[5].payload else {
        unreachable!()
    };
    assert_eq!(failed.reason, "integrity");
    let error = &failed.error;
    assert!(error.contains("Checksum mismatch"), "{error}");
    assert!(error.contains(&crc(&good())), "{error}");
    assert!(error.contains(&crc(&bad())), "{error}");
    assert!(error.contains("after 3 downloads"), "{error}");

    let problems = s.db.verify().open_problems(10).await.unwrap();
    assert_eq!(problems.len(), 1);
    assert_eq!(
        (problems[0].source.as_str(), problems[0].result.as_str()),
        ("download", "failed")
    );

    // FR-016: no token in event or problem text
    for event in &seen {
        assert!(!serde_json::to_string(&**event).unwrap().contains(TOKEN));
    }
    assert!(!problems[0].detail.as_deref().unwrap_or("").contains(TOKEN));
}

#[tokio::test]
async fn no_redownloads_fails_on_the_first_mismatch() {
    let mut s = setup(verify_on(0), false).await;
    let _file = s.file_details(3403, Some(&crc(&good()))).await;
    let (_mock, hits) = s.bodies(3403, vec![bad(), good()]).await;
    s.queue(3403).await;
    let mut rx = events::subscribe();

    assert!(s.manager.process_queue().await.unwrap());

    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert!(!s.path("movie.mkv").exists());
    let seen = events_for(&mut rx, 3403).await;
    assert_eq!(types(&seen), ["verify.failed", "download.failed"]);
    let Payload::DownloadFailed(failed) = &seen[1].payload else {
        unreachable!()
    };
    assert!(
        failed.error.contains("after 1 downloads"),
        "{}",
        failed.error
    );
}

#[tokio::test]
async fn without_a_crc32_the_size_is_the_check() {
    let mut s = setup(verify_on(2), false).await;
    let _file = s.file_details(3404, None).await;
    let (_mock, hits) = s.bodies(3404, vec![bad()]).await;
    s.queue(3404).await;
    let mut rx = events::subscribe();

    assert!(s.manager.process_queue().await.unwrap());

    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert_eq!(std::fs::read(s.path("movie.mkv")).unwrap(), bad());
    let seen = events_for(&mut rx, 3404).await;
    assert_eq!(types(&seen), ["verify.passed", "download.finished"]);
    let Payload::VerifyPassed(p) = &seen[0].payload else {
        unreachable!()
    };
    assert_eq!((p.method.as_str(), p.crc32.as_deref()), ("size", None));
    let history = s.db.history().query(None, 50, 0, None).await.unwrap();
    assert_eq!(history[0].verify.as_deref(), Some("size"));
}

#[tokio::test]
async fn verification_off_keeps_a_bad_fresh_download_and_sends_no_verify_events() {
    let off = VerifySettings {
        enabled: false,
        ..VerifySettings::default()
    };
    let mut s = setup(off, false).await;
    let _file = s.file_details(3405, Some(&crc(&good()))).await;
    let (_mock, hits) = s.bodies(3405, vec![bad()]).await;
    s.queue(3405).await;
    let mut rx = events::subscribe();

    assert!(s.manager.process_queue().await.unwrap());

    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert!(s.path("movie.mkv").exists());
    let seen = events_for(&mut rx, 3405).await;
    assert_eq!(types(&seen), ["download.finished"]);
    let history = s.db.history().query(None, 50, 0, None).await.unwrap();
    assert_eq!(history[0].verify.as_deref(), Some("none"));
}

#[tokio::test]
async fn only_a_resumed_file_reads_back_its_prefix() {
    // Fresh download: nothing read back (SC-002)
    let mut s = setup(verify_on(2), false).await;
    let _file = s.file_details(3406, Some(&crc(&good()))).await;
    let (_mock, _) = s.bodies(3406, vec![good()]).await;
    s.queue(3406).await;
    let before = read_bytes_metric();
    assert!(s.manager.process_queue().await.unwrap());
    assert_eq!(read_bytes_metric(), before);
    assert_eq!(std::fs::read(s.path("movie.mkv")).unwrap(), good());

    // Resumed download: the half already on disk is hashed, the rest as it streams
    let mut s = setup(verify_on(2), false).await;
    let whole = good();
    let half = SIZE / 2;
    let _file = s.file_details(3407, Some(&crc(&whole))).await;
    let range = s
        .server
        .mock("GET", "/files/3407/download")
        .match_header("range", format!("bytes={half}-{}", SIZE - 1).as_str())
        .with_status(206)
        .with_body(&whole[half..])
        .expect(1)
        .create_async()
        .await;
    let (_full, full_hits) = s.bodies(3407, vec![whole.clone()]).await;
    std::fs::write(s.path(".movie.mkv.downloading"), &whole[..half]).unwrap();
    s.queue(3407).await;
    let mut rx = events::subscribe();
    let before = read_bytes_metric();

    assert!(s.manager.process_queue().await.unwrap());

    range.assert_async().await;
    assert_eq!(full_hits.load(Ordering::SeqCst), 0);
    assert!(read_bytes_metric() >= before + half as u64);
    assert_eq!(std::fs::read(s.path("movie.mkv")).unwrap(), whole);
    let seen = events_for(&mut rx, 3407).await;
    assert_eq!(types(&seen), ["verify.passed", "download.finished"]);
}

#[tokio::test]
async fn redownload_flag_overwrites_a_bad_local_copy_without_reading_it() {
    let mut s = setup(verify_on(2), false).await;
    let _file = s.file_details(3408, Some(&crc(&good()))).await;
    let (_mock, hits) = s.bodies(3408, vec![good()]).await;
    std::fs::write(s.path("movie.mkv"), bad()).unwrap();
    s.db.queue()
        .add_redownload(3408, "movie.mkv", SIZE as i64, "", "", None)
        .await
        .unwrap();

    assert!(s.manager.process_queue().await.unwrap());

    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert_eq!(std::fs::read(s.path("movie.mkv")).unwrap(), good());
    let history = s.db.history().query(None, 50, 0, None).await.unwrap();
    assert_eq!(history[0].outcome.as_deref(), Some("overwritten"));
}
