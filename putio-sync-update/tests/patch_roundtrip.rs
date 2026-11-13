//! A patch made by `examples/make-patch.rs` and compressed by `xz -9e` applies back to the
//! exact new binary (contracts/release-assets.md, Updater lookup step 2).

use std::process::Command;

/// Deterministic noise, so a failure reproduces
fn noise(len: usize, mut seed: u64) -> Vec<u8> {
    (0..len)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed as u8
        })
        .collect()
}

fn xz_on_path() -> bool {
    Command::new("xz")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
fn xz_compressed_patch_round_trips() {
    if !xz_on_path() {
        eprintln!("skipped: xz is not on PATH");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let old = noise(1 << 20, 0x9e37_79b9_7f4a_7c15);
    let mut new = old.clone();
    for (at, byte) in [(10, 1u8), (123_456, 2), (700_000, 3), (1_048_000, 4)] {
        new[at] ^= byte;
    }
    new.splice(500_000..500_000, b"a few new bytes".iter().copied());

    let old_path = dir.path().join("old");
    let new_path = dir.path().join("new");
    let patch_path = dir.path().join("patch");
    std::fs::write(&old_path, &old).unwrap();
    std::fs::write(&new_path, &new).unwrap();

    let status = Command::new(env!("CARGO"))
        .args(["run", "--quiet", "--example", "make-patch", "--"])
        .args([&old_path, &new_path, &patch_path])
        .status()
        .unwrap();
    assert!(status.success(), "make-patch failed");

    let status = Command::new("xz")
        .args(["-9e", "--check=crc64"])
        .arg(&patch_path)
        .status()
        .unwrap();
    assert!(status.success(), "xz failed");
    let compressed = std::fs::read(dir.path().join("patch.xz")).unwrap();
    assert!(
        compressed.len() < 64 * 1024,
        "patch is {} bytes",
        compressed.len()
    );

    let mut raw = Vec::new();
    lzma_rs::xz_decompress(&mut compressed.as_slice(), &mut raw).unwrap();
    let mut patched = Vec::new();
    bsdiff::patch(&old, &mut raw.as_slice(), &mut patched).unwrap();
    assert!(patched == new, "patched binary differs from the new one");
}

#[test]
fn make_patch_rejects_wrong_arguments() {
    let output = Command::new(env!("CARGO"))
        .args([
            "run",
            "--quiet",
            "--example",
            "make-patch",
            "--",
            "only-one",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
}
