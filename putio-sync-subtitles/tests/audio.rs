//! ffmpeg and ffprobe handling with fake scripts (feature 025). Unix only: the fakes are
//! shell scripts. No real ffmpeg is needed.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use putio_sync_subtitles::audio::{self, AudioError, Chunk};

fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

/// A fake ffmpeg that records its arguments and writes a WAV-sized file to its last argument
fn fake_ffmpeg(dir: &Path, log: &Path) -> PathBuf {
    script(
        dir,
        "ffmpeg",
        &format!(
            "echo \"$@\" >> {log}\n\
             for last; do :; done\n\
             case \"$1\" in -version) echo 'ffmpeg version 6.1.1 Copyright (c) 2000'; exit 0;; esac\n\
             head -c 4000 /dev/zero > \"$last\"",
            log = log.display()
        ),
    )
}

#[test]
fn discovery_uses_the_configured_path_then_path() {
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let ffmpeg = script(&bin, "ffmpeg", "exit 0");
    script(&bin, "ffprobe", "exit 0");

    let tools = audio::discover(ffmpeg.to_str().unwrap()).unwrap();
    assert_eq!(tools.ffmpeg, ffmpeg);
    assert_eq!(
        tools.ffprobe,
        bin.join("ffprobe"),
        "ffprobe sits next to ffmpeg"
    );

    // A configured path that is not there is not replaced by a search
    let err = audio::discover("/no/such/ffmpeg").unwrap_err();
    assert!(matches!(err, AudioError::NotFound(_)));
    assert!(err.to_string().contains("ffmpeg not found"), "{err}");
}

#[test]
fn a_missing_ffprobe_is_named() {
    let dir = tempfile::tempdir().unwrap();
    let ffmpeg = script(dir.path(), "ffmpeg", "exit 0");
    // Either the host has an ffprobe on PATH (then discovery works) or it names ffprobe
    match audio::discover(ffmpeg.to_str().unwrap()) {
        Ok(t) => assert_ne!(t.ffprobe.parent(), Some(dir.path())),
        Err(e) => assert!(e.to_string().contains("ffprobe not found"), "{e}"),
    }
}

#[tokio::test]
async fn version_and_duration_come_from_the_tools() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("log");
    let ffmpeg = fake_ffmpeg(dir.path(), &log);
    let ffprobe = script(dir.path(), "ffprobe", "echo 1500.25");
    assert_eq!(audio::version(&ffmpeg).await.as_deref(), Some("6.1.1"));
    let d = audio::duration_secs(&ffprobe, Path::new("/v.mkv"))
        .await
        .unwrap();
    assert!((d - 1500.25).abs() < 1e-9);

    let bad = script(dir.path(), "badprobe", "echo 'N/A'");
    assert!(audio::duration_secs(&bad, Path::new("/v.mkv"))
        .await
        .is_err());
    let failing = script(
        dir.path(),
        "failprobe",
        "echo 'moov atom not found' >&2; exit 1",
    );
    let e = audio::duration_secs(&failing, Path::new("/v.mkv"))
        .await
        .unwrap_err();
    assert!(e.to_string().contains("moov atom not found"), "{e}");
}

#[tokio::test]
async fn a_chunk_is_cut_and_its_file_goes_away_with_the_guard() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("log");
    let ffmpeg = fake_ffmpeg(dir.path(), &log);
    let chunk = Chunk {
        index: 1,
        start_secs: 600.0,
        len_secs: 300.0,
    };
    let work = tempfile::tempdir().unwrap();
    let wav = audio::extract_chunk(&ffmpeg, Path::new("/m/Movie.mkv"), &chunk, work.path())
        .await
        .unwrap();
    let path = wav.path().to_path_buf();
    assert!(path.exists());
    let args = std::fs::read_to_string(&log).unwrap();
    assert!(
        args.contains("-ss 600.000 -t 300.000 -i /m/Movie.mkv"),
        "{args}"
    );
    assert!(args.contains("-map 0:a:0"), "{args}");
    drop(wav);
    assert!(!path.exists(), "removed on drop");
}

#[tokio::test]
async fn a_failing_ffmpeg_reports_its_message_and_leaves_no_file() {
    let dir = tempfile::tempdir().unwrap();
    let ffmpeg = script(
        dir.path(),
        "ffmpeg",
        "for last; do :; done\necho partial > \"$last\"\necho 'Stream map matches no streams' >&2\nexit 1",
    );
    let chunk = Chunk {
        index: 0,
        start_secs: 0.0,
        len_secs: 60.0,
    };
    let work = tempfile::tempdir().unwrap();
    let err = audio::extract_chunk(&ffmpeg, Path::new("/m/a.mkv"), &chunk, work.path())
        .await
        .unwrap_err();
    assert!(
        err.to_string().contains("Stream map matches no streams"),
        "{err}"
    );
    // The guard dropped inside the function: nothing is left behind
    assert_eq!(std::fs::read_dir(work.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn an_ffmpeg_that_makes_nothing_is_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let ffmpeg = script(dir.path(), "ffmpeg", "exit 0");
    let chunk = Chunk {
        index: 0,
        start_secs: 0.0,
        len_secs: 60.0,
    };
    let work = tempfile::tempdir().unwrap();
    let err = audio::extract_chunk(&ffmpeg, Path::new("/m/a.mkv"), &chunk, work.path())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("no audio"), "{err}");
}

#[tokio::test]
async fn dropping_the_future_kills_ffmpeg() {
    let dir = tempfile::tempdir().unwrap();
    let pidfile = dir.path().join("pid");
    let ffmpeg = script(
        dir.path(),
        "ffmpeg",
        &format!("echo $$ > {}\nexec sleep 30", pidfile.display()),
    );
    let chunk = Chunk {
        index: 0,
        start_secs: 0.0,
        len_secs: 60.0,
    };
    let work = tempfile::tempdir().unwrap();
    let mut run = Box::pin(audio::extract_chunk(
        &ffmpeg,
        Path::new("/m/a.mkv"),
        &chunk,
        work.path(),
    ));
    // Drop it once the fake ffmpeg has written its pid, however slow the machine
    let pid = async {
        loop {
            if let Ok(pid) = std::fs::read_to_string(&pidfile)
                .unwrap_or_default()
                .trim()
                .parse::<i32>()
            {
                return pid;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    };
    let pid = tokio::select! {
        _ = &mut run => panic!("ffmpeg finished before it was dropped"),
        pid = tokio::time::timeout(std::time::Duration::from_secs(10), pid) => pid.expect("ffmpeg started"),
    };
    drop(run);
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
    let alive = std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap()
        .success();
    assert!(!alive, "ffmpeg (pid {pid}) is still running after cancel");
}
