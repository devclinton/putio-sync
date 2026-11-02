//! The pure rule: wanted types, minimum size and the reason strings (T003)

use putio_sync_precheck::rule::{self, Policy, Reason, Stage, Verdict};
use putio_sync_precheck::ReleaseFile;

const MB: u64 = 1024 * 1024;

fn video() -> Policy {
    Policy {
        enabled: true,
        wanted: ["mkv", "mp4", "avi", "srt"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        wanted_from: None,
        min_size: 50 * MB,
        allow_archives: false,
        kind: "video".to_string(),
    }
}

fn f(path: &str, size: u64) -> ReleaseFile {
    ReleaseFile {
        path: path.to_string(),
        size,
    }
}

fn reject(policy: &Policy, files: &[ReleaseFile]) -> String {
    match rule::evaluate(policy, files) {
        Verdict::Reject(reason) => reason.message(Stage::Before),
        Verdict::Pass { .. } => panic!("expected a rejection"),
    }
}

#[test]
fn exe_only_is_rejected() {
    let files = [
        f("Rel/Show.S01E03.exe", 1400 * MB),
        f("Rel/readme.txt", 120),
    ];
    assert_eq!(
        reject(&video(), &files),
        "rejected before Put.io: no video file (found .exe, .txt)"
    );
}

#[test]
fn archive_only_is_rejected() {
    let files = [f("Rel/Movie.7z", 4000 * MB), f("Rel/Movie.exe", 2 * MB)];
    assert_eq!(
        reject(&video(), &files),
        "rejected before Put.io: no video file (found .7z, .exe)"
    );
}

#[test]
fn split_rar_parts_are_rejected() {
    let files = [
        f("Rel/a.rar", 100 * MB),
        f("Rel/a.r00", 100 * MB),
        f("Rel/a.r01", 90 * MB),
    ];
    assert!(matches!(
        rule::evaluate(&video(), &files),
        Verdict::Reject(Reason::NoWanted { .. })
    ));
}

#[test]
fn small_video_alone_is_rejected() {
    let files = [f("Rel/sample.mkv", 12 * MB)];
    assert_eq!(
        reject(&video(), &files),
        "rejected before Put.io: only a small video (12 MB, minimum 50 MB)"
    );
}

#[test]
fn double_extension_uses_the_last() {
    let files = [f("Rel/Episode.mkv.exe", 1400 * MB)];
    assert_eq!(
        reject(&video(), &files),
        "rejected before Put.io: no video file (found .exe)"
    );
}

#[test]
fn big_video_with_an_exe_passes() {
    let files = [f("Rel/Movie.mkv", 2048 * MB), f("Rel/codec.exe", 3 * MB)];
    assert_eq!(
        rule::evaluate(&video(), &files),
        Verdict::Pass {
            largest_wanted: 2048 * MB
        }
    );
}

#[test]
fn season_pack_with_one_big_episode_passes() {
    let files = [
        f("S01/E01.mkv", 900 * MB),
        f("S01/E02.mkv", 10 * MB),
        f("S01/E01.srt", 40_000),
    ];
    assert!(matches!(
        rule::evaluate(&video(), &files),
        Verdict::Pass { largest_wanted } if largest_wanted == 900 * MB
    ));
}

#[test]
fn audio_policy() {
    let audio = Policy {
        wanted: ["flac", "mp3"].iter().map(|s| s.to_string()).collect(),
        wanted_from: Some("lidarr".to_string()),
        kind: "audio".to_string(),
        ..video()
    };
    assert!(matches!(
        rule::evaluate(&audio, &[f("Album/01.flac", 60 * MB)]),
        Verdict::Pass { .. }
    ));
    assert_eq!(
        reject(&audio, &[f("Album/setup.exe", 60 * MB)]),
        "rejected before Put.io: no audio file (found .exe)"
    );
}

#[test]
fn archives_pass_when_allowed() {
    let policy = Policy {
        allow_archives: true,
        ..video()
    };
    let files = [
        f("Rel/a.part1.rar", 100 * MB),
        f("Rel/a.part2.rar", 100 * MB),
        f("Rel/b.r00", 100 * MB),
        f("Rel/c.001", 100 * MB),
    ];
    assert!(matches!(
        rule::evaluate(&policy, &files),
        Verdict::Pass { .. }
    ));
    assert!(matches!(
        rule::evaluate(&policy, &[f("Rel/c.001", 100 * MB)]),
        Verdict::Pass { .. }
    ));
}

#[test]
fn disabled_policy_passes_everything() {
    let policy = Policy {
        enabled: false,
        ..video()
    };
    assert!(matches!(
        rule::evaluate(&policy, &[f("Rel/x.exe", 1)]),
        Verdict::Pass { .. }
    ));
}

#[test]
fn messages_and_keys() {
    let reason = Reason::NoWanted {
        kind: "video".to_string(),
        found: vec!["exe".to_string()],
    };
    assert_eq!(
        reason.message(Stage::Putio),
        "rejected on Put.io: no video file (found .exe) (checked on Put.io)"
    );
    assert_eq!(reason.key(), "no_wanted");
    assert_eq!(
        Reason::Invalid.message(Stage::Before),
        "rejected before Put.io: not a valid .torrent"
    );
    assert_eq!(Reason::Invalid.key(), "invalid");
    assert_eq!(Reason::Gone.key(), "gone");
    assert_eq!(
        Reason::Gone.message(Stage::Putio),
        "transfer no longer on Put.io"
    );
    let small = Reason::SmallWanted {
        kind: "video".to_string(),
        largest: 12 * MB,
        min: 50 * MB,
    };
    assert_eq!(small.key(), "small_wanted");
    // Nothing at all: no list in brackets
    assert_eq!(
        reject(&video(), &[]),
        "rejected before Put.io: no video file"
    );
}
