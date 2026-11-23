//! A kept watch file is not read and hashed on every scan, and the folder listing only
//! stats candidates

use putio_sync_client::PutioApiClient;
use putio_sync_db::Database;
use putio_sync_file_manager::{ProcessOutcome, TorrentWatchCore};

#[test]
fn only_torrent_and_magnet_files_are_listed() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.torrent"), b"x").unwrap();
    std::fs::write(dir.path().join("b.MAGNET"), b"magnet:?xt=urn:btih:abc").unwrap();
    std::fs::write(dir.path().join("c.txt"), b"x").unwrap();
    std::fs::create_dir(dir.path().join("d.torrent")).unwrap();
    let mut found = TorrentWatchCore::new()
        .find_torrent_files(dir.path())
        .unwrap();
    found.sort();
    assert_eq!(
        found,
        [dir.path().join("a.torrent"), dir.path().join("b.MAGNET")]
    );
}

#[tokio::test]
async fn an_unchanged_kept_file_is_answered_from_memory_after_the_first_scan() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.torrent");
    std::fs::write(&path, b"torrent bytes").unwrap();
    let db = Database::new(dir.path().join(".putiosync.db"))
        .await
        .unwrap();
    let sum = putio_sync_file_manager::calculate_file_checksum(&path)
        .await
        .unwrap();
    db.processed_torrents()
        .record_processed(path.to_str().unwrap(), &sum, 13, None)
        .await
        .unwrap();
    let client = PutioApiClient::with_base_url("t".into(), "http://127.0.0.1:1".into());
    let core = TorrentWatchCore::new();

    for _ in 0..2 {
        let outcome = core
            .process_torrent_file(&path, &client, None, Some(&db))
            .await
            .unwrap();
        assert!(matches!(outcome, ProcessOutcome::AlreadyUploaded));
    }
}
