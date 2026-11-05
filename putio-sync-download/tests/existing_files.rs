//! What the queue worker does when a file already exists at its local path (feature
//! 040): resume, skip, overwrite, keep both, fixed paths and a folder in the way.
//! Small payloads against mockito; nothing reaches Put.io.

use std::path::Path;
use std::sync::Arc;

use mockito::{Matcher, Mock, ServerGuard};
use putio_sync_client::PutioApiClient;
use putio_sync_core::config::ExistingFile;
use putio_sync_db::models::{DownloadHistory, QueueStatus};
use putio_sync_db::Database;
use putio_sync_download::{DownloadConfig, DownloadFile, DownloadManager};

const SIZE: usize = 8192;

fn crc(data: &[u8]) -> String {
    format!(
        "{:08x}",
        crc::Crc::<u32>::new(&crc::CRC_32_ISO_HDLC).checksum(data)
    )
}

/// Put.io's file 1, `movie.mkv`, with an optional CRC32
async fn file_details(server: &mut ServerGuard, id: i64, crc32: Option<&str>) -> Mock {
    let crc32 = crc32.map_or("null".to_string(), |c| format!("\"{c}\""));
    server
        .mock("GET", format!("/files/{id}").as_str())
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"file":{{"id":{id},"name":"movie.mkv","size":{SIZE},"file_type":"VIDEO","parent_id":0,"is_folder":false,"is_shared":false,"crc32":{crc32}}},"status":"OK"}}"#
        ))
        .create_async()
        .await
}

/// The whole file, for a request without a Range header
async fn full_body(server: &mut ServerGuard, id: i64, body: Vec<u8>, times: usize) -> Mock {
    server
        .mock("GET", format!("/files/{id}/download").as_str())
        .match_header("range", Matcher::Missing)
        .with_body(body)
        .expect(times)
        .create_async()
        .await
}

struct Setup {
    server: ServerGuard,
    dir: tempfile::TempDir,
    db: Arc<Database>,
    manager: DownloadManager,
}

async fn setup(on_existing: ExistingFile) -> Setup {
    let server = mockito::Server::new_async().await;
    let dir = tempfile::tempdir().unwrap();
    let db = Arc::new(
        Database::new(dir.path().join(".putiosync.db"))
            .await
            .unwrap(),
    );
    let client = Arc::new(PutioApiClient::with_base_url("tok".into(), server.url()));
    let config = DownloadConfig {
        target_directory: dir.path().to_path_buf(),
        on_existing,
        max_retries: 0,
        retry_initial_delay_secs: 0,
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
    fn path(&self, name: &str) -> std::path::PathBuf {
        self.dir.path().join(name)
    }

    async fn queue(&self, id: i64, fixed_path: bool) {
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
                fixed_path,
            )
            .await
            .unwrap();
    }

    async fn history(&self) -> Vec<DownloadHistory> {
        self.db.history().query(None, 50, 0, None).await.unwrap()
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

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap()
}

#[tokio::test]
async fn partial_file_at_the_real_name_resumes_from_where_it_stops() {
    let mut s = setup(ExistingFile::Resume).await;
    let whole = [vec![7u8; SIZE / 2], vec![9u8; SIZE / 2]].concat();
    let _file = file_details(&mut s.server, 1, Some(&crc(&whole))).await;
    let range = s
        .server
        .mock("GET", "/files/1/download")
        .match_header("range", "bytes=4096-8191")
        .with_status(206)
        .with_body(vec![9u8; SIZE / 2])
        .expect(1)
        .create_async()
        .await;
    let full = full_body(&mut s.server, 1, vec![0u8; SIZE], 0).await;
    std::fs::write(s.path("movie.mkv"), vec![7u8; SIZE / 2]).unwrap();
    s.queue(1, false).await;

    assert!(s.manager.process_queue().await.unwrap());
    range.assert_async().await;
    full.assert_async().await;
    assert_eq!(read(&s.path("movie.mkv")), whole);
    assert!(!s.path(".movie.mkv.downloading").exists());
    let history = s.history().await;
    assert_eq!(history[0].outcome.as_deref(), Some("resumed"));
    assert_eq!(
        history[0].local_path.as_deref(),
        s.path("movie.mkv").to_str()
    );
    assert_eq!(s.queue_status(1).await, None);
}

#[tokio::test]
async fn resumed_file_with_a_bad_checksum_is_downloaded_once_more() {
    let mut s = setup(ExistingFile::Resume).await;
    // Put.io's file is all 9s; the local half is 7s, so the resumed file is wrong
    let _file = file_details(&mut s.server, 1, Some(&crc(&[9u8; SIZE]))).await;
    let _range = s
        .server
        .mock("GET", "/files/1/download")
        .match_header("range", "bytes=4096-8191")
        .with_status(206)
        .with_body(vec![9u8; SIZE / 2])
        .create_async()
        .await;
    let full = full_body(&mut s.server, 1, vec![9u8; SIZE], 1).await;
    std::fs::write(s.path("movie.mkv"), vec![7u8; SIZE / 2]).unwrap();
    s.queue(1, false).await;

    assert!(s.manager.process_queue().await.unwrap());
    full.assert_async().await;
    assert_eq!(read(&s.path("movie.mkv")), vec![9u8; SIZE]);
    assert_eq!(s.history().await[0].outcome.as_deref(), Some("restarted"));
}

#[tokio::test]
async fn a_second_bad_checksum_fails_the_file() {
    let mut s = setup(ExistingFile::Resume).await;
    let _file = file_details(&mut s.server, 1, Some("deadbeef")).await;
    let _range = s
        .server
        .mock("GET", "/files/1/download")
        .match_header("range", "bytes=4096-8191")
        .with_status(206)
        .with_body(vec![9u8; SIZE / 2])
        .create_async()
        .await;
    let _full = full_body(&mut s.server, 1, vec![9u8; SIZE], 1).await;
    std::fs::write(s.path("movie.mkv"), vec![7u8; SIZE / 2]).unwrap();
    s.queue(1, false).await;

    assert!(s.manager.process_queue().await.unwrap());
    assert_eq!(s.queue_status(1).await.as_deref(), Some("failed"));
    assert!(!s.path("movie.mkv").exists());
    assert!(!s.path(".movie.mkv.downloading").exists());
    assert!(s.history().await.is_empty());
}

#[tokio::test]
async fn complete_file_is_skipped_and_recorded_as_downloaded() {
    let mut s = setup(ExistingFile::Resume).await;
    let _file = file_details(&mut s.server, 1, Some(&crc(&[5u8; SIZE]))).await;
    let full = full_body(&mut s.server, 1, vec![5u8; SIZE], 0).await;
    std::fs::write(s.path("movie.mkv"), vec![5u8; SIZE]).unwrap();
    s.queue(1, false).await;

    assert!(s.manager.process_queue().await.unwrap());
    full.assert_async().await;
    let row = s.db.downloads().find_by_file_id(1).await.unwrap().unwrap();
    assert_eq!(row.status, "completed");
    assert_eq!(s.history().await[0].outcome.as_deref(), Some("skipped"));
    assert_eq!(s.queue_status(1).await, None);
}

#[tokio::test]
async fn larger_local_file_is_kept_and_the_new_one_gets_a_number() {
    let mut s = setup(ExistingFile::Resume).await;
    let _file = file_details(&mut s.server, 1, None).await;
    let _full = full_body(&mut s.server, 1, vec![9u8; SIZE], 1).await;
    std::fs::write(s.path("movie.mkv"), vec![1u8; SIZE * 2]).unwrap();
    s.queue(1, false).await;

    assert!(s.manager.process_queue().await.unwrap());
    assert_eq!(read(&s.path("movie.mkv")), vec![1u8; SIZE * 2]);
    assert_eq!(read(&s.path("movie (1).mkv")), vec![9u8; SIZE]);
    let history = s.history().await;
    assert_eq!(history[0].outcome.as_deref(), Some("saved_as"));
    assert_eq!(
        history[0].local_path.as_deref(),
        s.path("movie (1).mkv").to_str()
    );
}

#[tokio::test]
async fn keep_both_takes_the_next_free_number_and_keeps_it_after_a_failure() {
    let mut s = setup(ExistingFile::KeepBoth).await;
    let _file = file_details(&mut s.server, 1, None).await;
    let fail = s
        .server
        .mock("GET", "/files/1/download")
        .with_status(500)
        .expect(1)
        .create_async()
        .await;
    std::fs::write(s.path("movie.mkv"), b"old").unwrap();
    std::fs::write(s.path("movie (1).mkv"), b"older").unwrap();
    s.queue(1, false).await;

    assert!(s.manager.process_queue().await.unwrap());
    fail.assert_async().await;
    let row = s.db.queue().get_by_file_id(1).await.unwrap().unwrap();
    assert_eq!(row.status, "failed");
    assert_eq!(row.save_as.as_deref(), Some("movie (2).mkv"));

    // Restarted with "(1)" free again: the stored name is kept, not a new pick
    fail.remove_async().await;
    let _full = full_body(&mut s.server, 1, vec![9u8; SIZE], 1).await;
    std::fs::remove_file(s.path("movie (1).mkv")).unwrap();
    s.db.queue()
        .update_status(1, putio_sync_db::models::QueueStatus::Pending)
        .await
        .unwrap();
    assert!(s.manager.process_queue().await.unwrap());
    assert_eq!(read(&s.path("movie (2).mkv")), vec![9u8; SIZE]);
    assert!(!s.path("movie (1).mkv").exists());
    assert_eq!(read(&s.path("movie.mkv")), b"old");
    assert_eq!(s.history().await[0].outcome.as_deref(), Some("saved_as"));
}

#[tokio::test]
async fn failed_overwrite_leaves_the_old_file_as_it_was() {
    let mut s = setup(ExistingFile::Overwrite).await;
    let _file = file_details(&mut s.server, 1, None).await;
    // Short body: a size mismatch, as if the connection dropped midway
    let _short = full_body(&mut s.server, 1, vec![9u8; SIZE / 4], 1).await;
    std::fs::write(s.path("movie.mkv"), b"the old file").unwrap();
    s.queue(1, false).await;

    assert!(s.manager.process_queue().await.unwrap());
    assert_eq!(s.queue_status(1).await.as_deref(), Some("failed"));
    assert_eq!(read(&s.path("movie.mkv")), b"the old file");
    assert!(!s.path(".movie.mkv.downloading").exists());
}

#[tokio::test]
async fn overwrite_replaces_the_old_file_once_complete() {
    let mut s = setup(ExistingFile::Overwrite).await;
    let _file = file_details(&mut s.server, 1, Some(&crc(&[9u8; SIZE]))).await;
    let _full = full_body(&mut s.server, 1, vec![9u8; SIZE], 1).await;
    std::fs::write(s.path("movie.mkv"), vec![9u8; SIZE]).unwrap();
    s.queue(1, false).await;

    assert!(s.manager.process_queue().await.unwrap());
    assert_eq!(read(&s.path("movie.mkv")), vec![9u8; SIZE]);
    assert_eq!(s.history().await[0].outcome.as_deref(), Some("overwritten"));
}

#[tokio::test]
async fn fixed_path_overwrites_instead_of_a_numbered_name() {
    let mut s = setup(ExistingFile::KeepBoth).await;
    let _file = file_details(&mut s.server, 1, None).await;
    let _full = full_body(&mut s.server, 1, vec![9u8; SIZE], 1).await;
    std::fs::write(s.path("movie.mkv"), b"old").unwrap();
    s.queue(1, true).await;

    assert!(s.manager.process_queue().await.unwrap());
    assert_eq!(read(&s.path("movie.mkv")), vec![9u8; SIZE]);
    assert!(!s.path("movie (1).mkv").exists());
    assert_eq!(s.history().await[0].outcome.as_deref(), Some("overwritten"));
}

#[tokio::test]
async fn folder_in_the_way_is_refused_and_left_alone() {
    let mut s = setup(ExistingFile::Overwrite).await;
    let _file = file_details(&mut s.server, 1, None).await;
    let full = full_body(&mut s.server, 1, vec![9u8; SIZE], 0).await;
    std::fs::create_dir(s.path("movie.mkv")).unwrap();
    std::fs::write(s.path("movie.mkv").join("inside.txt"), b"keep").unwrap();
    s.queue(1, false).await;

    assert!(s.manager.process_queue().await.unwrap());
    full.assert_async().await;
    assert_eq!(s.queue_status(1).await.as_deref(), Some("failed"));
    assert_eq!(read(&s.path("movie.mkv").join("inside.txt")), b"keep");
}

#[tokio::test]
async fn resume_off_downloads_a_partial_file_again() {
    let mut s = setup(ExistingFile::Resume).await;
    let mut config = s.manager.get_config().await;
    config.resume_enabled = false;
    s.manager.update_config(config).await;
    let _file = file_details(&mut s.server, 1, None).await;
    let full = full_body(&mut s.server, 1, vec![9u8; SIZE], 1).await;
    std::fs::write(s.path("movie.mkv"), vec![7u8; SIZE / 2]).unwrap();
    s.queue(1, false).await;

    assert!(s.manager.process_queue().await.unwrap());
    full.assert_async().await;
    assert_eq!(read(&s.path("movie.mkv")), vec![9u8; SIZE]);
    assert_eq!(s.history().await[0].outcome.as_deref(), Some("overwritten"));
}

/// Two Put.io files with one local path never write it at the same time: the second
/// waits, then finds the first and keeps both
#[tokio::test]
async fn two_files_for_one_path_are_written_one_after_the_other() {
    let mut s = setup(ExistingFile::KeepBoth).await;
    let _one = full_body(&mut s.server, 1, vec![1u8; SIZE], 1).await;
    let _two = full_body(&mut s.server, 2, vec![2u8; SIZE], 1).await;
    let file = |id: i64| DownloadFile {
        file_id: id,
        filename: "movie.mkv".to_string(),
        relative_path: String::new(),
        file_size: SIZE as u64,
        download_url: format!("{}/files/{id}/download", s.server.url()),
        // Each file's own checksum, so the second differs from the first on disk
        crc32: Some(crc(&[id as u8; SIZE])),
        save_as: None,
        fixed_path: false,
        overwrite: false,
    };

    let (a, b) = tokio::join!(
        s.manager.download_file(file(1), None),
        s.manager.download_file(file(2), None)
    );
    assert!(a.unwrap().success && b.unwrap().success);
    let mut contents = [read(&s.path("movie.mkv")), read(&s.path("movie (1).mkv"))];
    contents.sort();
    assert_eq!(contents, [vec![1u8; SIZE], vec![2u8; SIZE]]);
}

#[tokio::test]
async fn same_size_with_another_checksum_is_kept_and_the_new_one_gets_a_number() {
    let mut s = setup(ExistingFile::Resume).await;
    let _file = file_details(&mut s.server, 1, Some(&crc(&[9u8; SIZE]))).await;
    let _full = full_body(&mut s.server, 1, vec![9u8; SIZE], 1).await;
    std::fs::write(s.path("movie.mkv"), vec![3u8; SIZE]).unwrap();
    s.queue(1, false).await;

    assert!(s.manager.process_queue().await.unwrap());
    assert_eq!(read(&s.path("movie.mkv")), vec![3u8; SIZE]);
    assert_eq!(read(&s.path("movie (1).mkv")), vec![9u8; SIZE]);
}

#[tokio::test]
async fn keep_both_skips_a_complete_copy_with_no_request() {
    let mut s = setup(ExistingFile::KeepBoth).await;
    let _file = file_details(&mut s.server, 1, Some(&crc(&[9u8; SIZE]))).await;
    let full = full_body(&mut s.server, 1, vec![9u8; SIZE], 0).await;
    std::fs::write(s.path("movie.mkv"), vec![9u8; SIZE]).unwrap();
    s.queue(1, false).await;

    assert!(s.manager.process_queue().await.unwrap());
    full.assert_async().await;
    assert!(!s.path("movie (1).mkv").exists());
    assert_eq!(s.history().await[0].outcome.as_deref(), Some("skipped"));
}

/// Radarr/Sonarr rows: what "resume" would save under a number is overwritten instead
#[tokio::test]
async fn fixed_path_overwrites_a_larger_or_mismatching_file_in_resume() {
    for local in [vec![1u8; SIZE * 2], vec![3u8; SIZE]] {
        let mut s = setup(ExistingFile::Resume).await;
        let _file = file_details(&mut s.server, 1, Some(&crc(&[9u8; SIZE]))).await;
        let _full = full_body(&mut s.server, 1, vec![9u8; SIZE], 1).await;
        std::fs::write(s.path("movie.mkv"), local).unwrap();
        s.queue(1, true).await;

        assert!(s.manager.process_queue().await.unwrap());
        assert_eq!(read(&s.path("movie.mkv")), vec![9u8; SIZE]);
        assert!(!s.path("movie (1).mkv").exists());
        assert_eq!(s.history().await[0].outcome.as_deref(), Some("overwritten"));
    }
}

/// `enqueue_tree_report(_, true)` (the *arr service) marks its rows fixed
#[tokio::test]
async fn arr_queued_rows_keep_their_path() {
    let mut s = setup(ExistingFile::KeepBoth).await;
    let _file = file_details(&mut s.server, 1, None).await;
    let _full = full_body(&mut s.server, 1, vec![9u8; SIZE], 1).await;
    std::fs::write(s.path("movie.mkv"), b"old").unwrap();

    let report = s.manager.enqueue_tree_report(1, true).await.unwrap();
    assert_eq!(report.stats.files_downloaded, 1);
    assert!(
        s.db.queue()
            .get_by_file_id(1)
            .await
            .unwrap()
            .unwrap()
            .fixed_path
    );

    assert!(s.manager.process_queue().await.unwrap());
    assert_eq!(read(&s.path("movie.mkv")), vec![9u8; SIZE]);
    assert!(!s.path("movie (1).mkv").exists());
}
