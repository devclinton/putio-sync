//! "Verify everything" (feature 034, US2): files already downloaded are read back and
//! compared with Put.io's CRC32. Put.io is a mockito server; nothing reaches it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use mockito::{Mock, ServerGuard};
use putio_sync_client::PutioApiClient;
use putio_sync_db::models::{DownloadStatus, QueueStatus};
use putio_sync_db::{Database, VerifyRun};
use putio_sync_download::{
    DownloadConfig, DownloadManager, VerifyControlError, VerifyRequest, VerifySettings,
    VerifyStartError,
};

const SIZE: usize = 8192;

fn crc(data: &[u8]) -> String {
    format!(
        "{:08x}",
        crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC).checksum(data)
    )
}

fn data(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

const ROOT: &str = r#"{"id":0,"name":"Root","size":0,"file_type":"FOLDER","parent_id":0,"is_folder":true,"is_shared":false}"#;

fn entry(id: i64, name: &str, size: usize, crc32: &str) -> String {
    format!(
        r#"{{"id":{id},"name":"{name}","size":{size},"file_type":"VIDEO","parent_id":0,"is_folder":false,"is_shared":false,"crc32":"{crc32}"}}"#
    )
}

struct Setup {
    server: ServerGuard,
    dir: tempfile::TempDir,
    db: Arc<Database>,
    manager: DownloadManager,
}

async fn setup(read_limit_bytes_per_sec: u64) -> Setup {
    let server = mockito::Server::new_async().await;
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(
        Database::new(dir.path().join(".putiosync.db"))
            .await
            .unwrap(),
    );
    let client = Arc::new(PutioApiClient::with_base_url("token".into(), server.url()));
    let config = DownloadConfig {
        target_directory: dir.path().to_path_buf(),
        verify: VerifySettings {
            read_limit_bytes_per_sec,
            ..VerifySettings::default()
        },
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

impl Setup {
    /// A downloaded file: `bytes` on disk (none when `None`) and a completed row
    async fn downloaded(&self, id: i64, name: &str, size: usize, bytes: Option<&[u8]>) -> String {
        let path = self.dir.path().join(name);
        if let Some(bytes) = bytes {
            std::fs::write(&path, bytes).unwrap();
        }
        let path = path.to_string_lossy().into_owned();
        self.db
            .downloads()
            .insert(id, name, size as i64, &path, DownloadStatus::Completed)
            .await
            .unwrap();
        path
    }

    /// The account root lists `entries`
    async fn listing(&mut self, entries: &[String]) -> Mock {
        self.server
            .mock("GET", "/files/list?parent_id=0")
            .with_header("content-type", "application/json")
            .with_body(format!(
                r#"{{"files":[{}],"parent":{ROOT},"status":"OK"}}"#,
                entries.join(",")
            ))
            .create_async()
            .await
    }

    async fn details(&mut self, entry: String) -> Mock {
        let id: i64 = entry[6..entry.find(',').unwrap()].parse().unwrap();
        self.server
            .mock("GET", format!("/files/{id}").as_str())
            .with_header("content-type", "application/json")
            .with_body(format!(r#"{{"file":{entry},"status":"OK"}}"#))
            .create_async()
            .await
    }

    async fn start(&self, request: VerifyRequest) {
        self.manager.verify().start(request).await.unwrap();
    }

    /// Wait for the run to end and return its row
    async fn finished(&self) -> VerifyRun {
        let deadline = Instant::now() + Duration::from_secs(10);
        while self.manager.verify().status().state != "idle" {
            assert!(Instant::now() < deadline, "the run did not end");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        self.db.verify().latest_run().await.unwrap().unwrap()
    }
}

#[tokio::test]
async fn good_corrupted_and_deleted_files() {
    let mut s = setup(0).await;
    let good = data(SIZE);
    let mut bad = good.clone();
    bad[10] ^= 0xff;
    s.downloaded(1, "good.mkv", SIZE, Some(&good)).await;
    s.downloaded(2, "bad.mkv", SIZE, Some(&bad)).await;
    s.downloaded(3, "gone.mkv", SIZE, None).await;
    let sum = crc(&good);
    s.listing(&[
        entry(1, "good.mkv", SIZE, &sum),
        entry(2, "bad.mkv", SIZE, &sum),
        entry(3, "gone.mkv", SIZE, &sum),
    ])
    .await;

    s.start(VerifyRequest::default()).await;
    let run = s.finished().await;

    assert_eq!(run.state, "finished");
    assert_eq!(run.scope, "everything");
    assert_eq!((run.files_total, run.files_done), (3, 3));
    assert_eq!(run.bytes_done, 3 * SIZE as i64);
    assert_eq!(
        (run.counts.verified, run.counts.mismatch, run.counts.missing),
        (1, 1, 1)
    );

    let queued = s.db.queue().get_by_file_id(2).await.unwrap().unwrap();
    assert!(queued.redownload);
    assert_eq!(queued.status, "pending");
    assert_eq!(queued.relative_path, "");
    assert_eq!(queued.save_as, None);
    assert!(s.db.queue().get_by_file_id(1).await.unwrap().is_none());

    let problems = s.db.verify().open_problems(10).await.unwrap();
    let mut results: Vec<_> = problems
        .iter()
        .map(|p| (p.file_id, p.result.as_str(), p.queued))
        .collect();
    results.sort();
    assert_eq!(results, [(2, "mismatch", true), (3, "missing", false)]);

    let row = s.db.downloads().find_by_file_id(1).await.unwrap().unwrap();
    assert_eq!(row.verify.as_deref(), Some("crc32"));
    assert!(row.verified_at.is_some());
}

#[tokio::test]
async fn report_only_queues_nothing() {
    let mut s = setup(0).await;
    let good = data(SIZE);
    s.downloaded(2, "bad.mkv", SIZE, Some(&good[..SIZE - 1]))
        .await;
    s.listing(&[entry(2, "bad.mkv", SIZE, &crc(&good))]).await;

    s.start(VerifyRequest {
        paths: None,
        report_only: true,
    })
    .await;
    let run = s.finished().await;

    assert!(run.report_only);
    assert_eq!(run.counts.mismatch, 1);
    assert!(s.db.queue().get_by_file_id(2).await.unwrap().is_none());
    let problems = s.db.verify().open_problems(10).await.unwrap();
    assert_eq!(problems.len(), 1);
    assert!(!problems[0].queued);
    assert!(problems[0].detail.as_deref().unwrap().contains("size"));
}

#[tokio::test]
async fn a_saved_name_and_folder_are_kept_on_requeue() {
    let mut s = setup(0).await;
    std::fs::create_dir_all(s.dir.path().join("Movies")).unwrap();
    let good = data(SIZE);
    let mut bad = good.clone();
    bad[0] ^= 1;
    s.downloaded(4, "Movies/movie (1).mkv", SIZE, Some(&bad))
        .await;
    s.listing(&[entry(4, "movie.mkv", SIZE, &crc(&good))]).await;

    s.start(VerifyRequest::default()).await;
    s.finished().await;

    let queued = s.db.queue().get_by_file_id(4).await.unwrap().unwrap();
    assert_eq!(queued.relative_path, "Movies");
    assert_eq!(queued.save_as.as_deref(), Some("movie (1).mkv"));
}

#[tokio::test]
async fn a_file_gone_from_putio_is_not_on_putio() {
    let mut s = setup(0).await;
    s.downloaded(5, "old.mkv", SIZE, Some(&data(SIZE))).await;
    s.listing(&[]).await;

    s.start(VerifyRequest::default()).await;
    let run = s.finished().await;

    assert_eq!(run.counts.not_on_putio, 1);
    assert!(s.db.queue().get_by_file_id(5).await.unwrap().is_none());
}

#[tokio::test]
async fn a_file_being_downloaded_is_busy() {
    let mut s = setup(0).await;
    let good = data(SIZE);
    s.downloaded(6, "movie.mkv", SIZE, Some(&good)).await;
    s.db.queue()
        .add_with_path_kept(
            6,
            "movie.mkv",
            SIZE as i64,
            "",
            "",
            0,
            QueueStatus::Downloading,
            false,
        )
        .await
        .unwrap();
    s.listing(&[entry(6, "movie.mkv", SIZE, &crc(&good))]).await;

    s.start(VerifyRequest::default()).await;
    let run = s.finished().await;

    assert_eq!((run.counts.busy, run.counts.verified), (1, 0));
}

#[tokio::test]
async fn a_paths_run_asks_putio_per_file() {
    let mut s = setup(0).await;
    let good = data(SIZE);
    let wanted = s.downloaded(7, "a.mkv", SIZE, Some(&good)).await;
    s.downloaded(8, "b.mkv", SIZE, Some(&good)).await;
    let details = s.details(entry(7, "a.mkv", SIZE, &crc(&good))).await;
    let listing = s.listing(&[]).await.expect(0);

    s.start(VerifyRequest {
        paths: Some(vec![wanted]),
        report_only: false,
    })
    .await;
    let run = s.finished().await;

    assert_eq!(run.scope, "paths");
    assert_eq!((run.files_total, run.counts.verified), (1, 1));
    details.assert_async().await;
    listing.assert_async().await;
}

#[tokio::test]
async fn paths_that_were_not_downloaded_are_refused() {
    let s = setup(0).await;
    let refused = s
        .manager
        .verify()
        .start(VerifyRequest {
            paths: Some(vec!["/nowhere/x.mkv".into()]),
            report_only: false,
        })
        .await;
    assert_eq!(refused.unwrap_err(), VerifyStartError::NoFiles);
    assert_eq!(s.manager.verify().status().state, "idle");
}

/// A file large enough that a run at 64 KB/s takes about 4 s
async fn slow_run(s: &mut Setup) {
    let big = data(256 * 1024);
    s.downloaded(9, "big.mkv", big.len(), Some(&big)).await;
    s.listing(&[entry(9, "big.mkv", big.len(), &crc(&big))])
        .await;
    s.start(VerifyRequest::default()).await;
}

#[tokio::test]
async fn a_second_start_is_refused() {
    let mut s = setup(64 * 1024).await;
    slow_run(&mut s).await;

    let again = s.manager.verify().start(VerifyRequest::default()).await;
    assert_eq!(again.unwrap_err(), VerifyStartError::Running);

    s.manager.verify().stop().unwrap();
    s.finished().await;
}

#[tokio::test]
async fn pause_holds_reads_until_resumed() {
    let mut s = setup(64 * 1024).await;
    slow_run(&mut s).await;
    tokio::time::sleep(Duration::from_millis(300)).await;

    assert_eq!(s.manager.verify().pause().unwrap().state, "paused");
    // The block being read may finish
    tokio::time::sleep(Duration::from_millis(400)).await;
    let held = s.manager.verify().status().bytes_done;
    tokio::time::sleep(Duration::from_millis(600)).await;
    let status = s.manager.verify().status();
    assert_eq!(status.state, "paused");
    assert_eq!(status.bytes_done, held);
    assert!(held < 256 * 1024);

    assert_eq!(s.manager.verify().resume().unwrap().state, "running");
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(s.manager.verify().status().bytes_done > held);

    s.manager.verify().stop().unwrap();
    s.finished().await;
}

#[tokio::test]
async fn stop_ends_the_run_stopped() {
    let mut s = setup(64 * 1024).await;
    slow_run(&mut s).await;
    tokio::time::sleep(Duration::from_millis(200)).await;

    assert_eq!(s.manager.verify().stop().unwrap().state, "stopping");
    let run = s.finished().await;

    assert_eq!(run.state, "stopped");
    assert!(run.ended_at.is_some());
    assert_eq!(
        s.manager.verify().stop().unwrap_err(),
        VerifyControlError::NotRunning
    );
    assert_eq!(
        s.manager.verify().resume().unwrap_err(),
        VerifyControlError::NotRunning
    );
}

#[tokio::test]
async fn resume_needs_a_pause() {
    let mut s = setup(64 * 1024).await;
    slow_run(&mut s).await;

    assert_eq!(
        s.manager.verify().resume().unwrap_err(),
        VerifyControlError::NotPaused
    );

    s.manager.verify().stop().unwrap();
    s.finished().await;
}

/// SC-003: reads keep within 10% of the limit
#[tokio::test]
async fn reads_keep_to_the_limit() {
    let limit = 200_000u64;
    let mut s = setup(limit).await;
    let file = data(400_000);
    s.downloaded(10, "rate.mkv", file.len(), Some(&file)).await;
    s.listing(&[entry(10, "rate.mkv", file.len(), &crc(&file))])
        .await;

    let started = Instant::now();
    s.start(VerifyRequest::default()).await;
    let run = s.finished().await;
    let took = started.elapsed().as_secs_f64();

    assert_eq!(run.counts.verified, 1);
    let rate = file.len() as f64 / took;
    assert!(
        rate <= limit as f64 * 1.1 && rate >= limit as f64 * 0.9,
        "{rate:.0} B/s against a limit of {limit} B/s ({took:.2} s)"
    );
}

#[tokio::test]
async fn startup_marks_an_interrupted_run_stopped() {
    let s = setup(0).await;
    let id =
        s.db.verify()
            .start_run("everything", false, 1, 1)
            .await
            .unwrap();

    s.manager.recover_interrupted().await.unwrap();

    let run = s.db.verify().run(id).await.unwrap().unwrap();
    assert_eq!(run.state, "stopped");
}
