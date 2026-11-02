//! Reading a .torrent's file list (T002)

mod common;

use common::*;
use putio_sync_precheck::metainfo::{self, ReleaseFile};
use sha1::{Digest, Sha1};

fn files(meta: &metainfo::Metainfo) -> Vec<(&str, u64)> {
    meta.files
        .iter()
        .map(|f| (f.path.as_str(), f.size))
        .collect()
}

#[test]
fn single_file() {
    let meta = metainfo::parse(&single("Movie.2024.mkv", 2048 * MB)).unwrap();
    assert_eq!(meta.name, "Movie.2024.mkv");
    assert_eq!(files(&meta), vec![("Movie.2024.mkv", 2048 * MB as u64)]);
    assert!(!meta.private);
}

#[test]
fn multi_file_paths_are_joined_under_the_name() {
    let bytes = multi(
        "Show.S01E01",
        vec![
            file(&["Show.S01E01.mkv"], 1500 * MB),
            file(&["Subs", "en.srt"], 40_000),
        ],
    );
    let meta = metainfo::parse(&bytes).unwrap();
    assert_eq!(
        files(&meta),
        vec![
            ("Show.S01E01/Show.S01E01.mkv", 1500 * MB as u64),
            ("Show.S01E01/Subs/en.srt", 40_000),
        ]
    );
}

#[test]
fn padding_files_are_ignored() {
    let padding = B::Dict(vec![
        ("attr", s("p")),
        ("length", B::Int(1000)),
        ("path", B::List(vec![s("x")])),
    ]);
    let bytes = multi(
        "Rel",
        vec![
            file(&["a.mkv"], 100 * MB),
            padding,
            file(&[".pad", "1234"], 1234),
            file(&["_____padding_file_0_"], 10),
        ],
    );
    let meta = metainfo::parse(&bytes).unwrap();
    assert_eq!(files(&meta), vec![("Rel/a.mkv", 100 * MB as u64)]);
}

#[test]
fn v2_file_tree() {
    let leaf = |len: i64| B::Dict(vec![("", B::Dict(vec![("length", B::Int(len))]))]);
    let info = B::Dict(vec![
        (
            "file tree",
            B::Dict(vec![
                ("Episode.exe", leaf(1400 * MB)),
                ("Extras", B::Dict(vec![("readme.txt", leaf(120))])),
            ]),
        ),
        ("meta version", B::Int(2)),
        ("name", s("Show.S01E02")),
        ("piece length", B::Int(16384)),
    ]);
    let meta = metainfo::parse(&torrent(info)).unwrap();
    assert_eq!(
        files(&meta),
        vec![
            ("Show.S01E02/Episode.exe", 1400 * MB as u64),
            ("Show.S01E02/Extras/readme.txt", 120),
        ]
    );
}

#[test]
fn hybrid_uses_the_v1_list() {
    let leaf = B::Dict(vec![("", B::Dict(vec![("length", B::Int(5))]))]);
    let info = B::Dict(vec![
        ("file tree", B::Dict(vec![("other.bin", leaf)])),
        ("files", B::List(vec![file(&["real.mkv"], 900 * MB)])),
        ("name", s("Hybrid")),
        ("piece length", B::Int(16384)),
        ("pieces", s("aaaaaaaaaaaaaaaaaaaa")),
    ]);
    let meta = metainfo::parse(&torrent(info)).unwrap();
    assert_eq!(files(&meta), vec![("Hybrid/real.mkv", 900 * MB as u64)]);
}

#[test]
fn hash_private_flag_and_tracker_hosts() {
    let info = B::Dict(vec![
        ("length", B::Int(10)),
        ("name", s("a.mkv")),
        ("private", B::Int(1)),
    ]);
    let info_bytes = encode(&info);
    let meta = metainfo::parse(&torrent(info)).unwrap();
    let expected: String = Sha1::digest(&info_bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(meta.hash, expected);
    assert!(meta.private);
    // Host names only: no scheme, port, path or passkey
    assert_eq!(
        meta.tracker_hosts,
        vec![
            "other.example.net".to_string(),
            "tracker.example.org".to_string()
        ]
    );
}

#[test]
fn info_hash_needs_no_file_list() {
    // 011 identifies a torrent by its hash even when it has no files
    let bytes = torrent(B::Dict(vec![("name", s("x"))]));
    let (hash, name) = metainfo::info_hash(&bytes).unwrap();
    assert_eq!(hash.len(), 40);
    assert_eq!(name.as_deref(), Some("x"));
    assert!(metainfo::parse(&bytes).is_none());
}

#[test]
fn garbage_and_truncated_input() {
    let good = single("a.mkv", 10);
    assert!(metainfo::parse(&good[..good.len() - 3]).is_none());
    assert!(metainfo::parse(b"").is_none());
    assert!(metainfo::parse(b"<html>404</html>").is_none());
    assert!(metainfo::parse(b"\x1f\x8b\x08\x00").is_none());
    let deep = format!("d4:info{}", "l".repeat(200));
    assert!(metainfo::parse(deep.as_bytes()).is_none());
    // A negative length is not a size
    assert!(metainfo::parse(&single("a.mkv", -5)).is_none());
}

#[test]
fn too_many_files_is_refused() {
    let many: Vec<B> = (0..100_001).map(|i| file(&[&i.to_string()], 1)).collect();
    assert!(metainfo::parse(&multi("Many", many)).is_none());
}

#[test]
fn release_file_extension() {
    let f = ReleaseFile {
        path: "Rel/Episode.mkv.exe".to_string(),
        size: 1,
    };
    assert_eq!(f.extension().as_deref(), Some("exe"));
}
