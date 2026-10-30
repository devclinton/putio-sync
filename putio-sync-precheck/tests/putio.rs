//! The check on Put.io of a magnet fallback's files (T016)

use putio_sync_client::{FileWalk, WalkError, WalkedFile};
use putio_sync_precheck::rule::{Policy, Reason, Stage, Verdict};
use putio_sync_precheck::Prechecker;

const MB: i64 = 1024 * 1024;
const HASH: &str = "0123456789ABCDEF0123456789ABCDEF01234567";

fn video() -> Policy {
    Policy {
        enabled: true,
        wanted: ["mkv", "mp4"].iter().map(|s| s.to_string()).collect(),
        wanted_from: None,
        min_size: 50 * MB as u64,
        allow_archives: false,
        kind: "video".to_string(),
    }
}

fn walked(path: &str, size: i64, is_folder: bool) -> WalkedFile {
    let name = path.rsplit('/').next().unwrap().to_string();
    WalkedFile {
        id: 1,
        extension: (!is_folder)
            .then(|| name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()))
            .flatten(),
        name,
        size,
        parent_id: 0,
        path: path.to_string(),
        is_folder,
        created_at: None,
    }
}

fn walk(files: Vec<WalkedFile>) -> FileWalk {
    FileWalk {
        files,
        complete: true,
        folders_listed: 1,
        errors: Vec::new(),
    }
}

#[test]
fn a_fake_folder_is_rejected_on_putio() {
    let tree = walk(vec![
        walked("/Show.S01E03", 0, true),
        walked("/Show.S01E03/Show.S01E03.exe", 1400 * MB, false),
        walked("/Show.S01E03/readme.txt", 120, false),
    ]);
    let checked = Prechecker::check_on_putio(HASH, "Show.S01E03", &tree, &video()).unwrap();
    let Verdict::Reject(reason) = checked.verdict else {
        panic!("expected a rejection");
    };
    assert_eq!(
        reason.message(Stage::Putio),
        "rejected on Put.io: no video file (found .exe, .txt) (checked on Put.io)"
    );
    let meta = checked.meta.unwrap();
    assert_eq!(meta.hash, HASH.to_ascii_lowercase());
    assert_eq!(meta.files.len(), 2, "folders are left out");
    assert_eq!(meta.files[0].path, "Show.S01E03/Show.S01E03.exe");
    assert_eq!(meta.total_size(), (1400 * MB + 120) as u64);
}

#[test]
fn a_real_file_passes() {
    let tree = walk(vec![walked("/Film.mkv", 2048 * MB, false)]);
    let checked = Prechecker::check_on_putio(HASH, "Film", &tree, &video()).unwrap();
    assert!(matches!(checked.verdict, Verdict::Pass { .. }));
}

#[test]
fn a_small_video_is_rejected() {
    let tree = walk(vec![walked("/Show/sample.mkv", 10 * MB, false)]);
    let checked = Prechecker::check_on_putio(HASH, "Show", &tree, &video()).unwrap();
    assert!(matches!(
        checked.verdict,
        Verdict::Reject(Reason::SmallWanted { .. })
    ));
}

#[test]
fn an_incomplete_walk_is_not_judged() {
    let mut tree = walk(vec![walked("/Show", 0, true)]);
    tree.complete = false;
    assert_eq!(
        Prechecker::check_on_putio(HASH, "Show", &tree, &video()),
        None
    );
    let mut tree = walk(vec![walked("/Show", 0, true)]);
    tree.errors.push(WalkError {
        folder_id: 1,
        path: "/Show".to_string(),
        error: "500".to_string(),
    });
    assert_eq!(
        Prechecker::check_on_putio(HASH, "Show", &tree, &video()),
        None
    );
}
