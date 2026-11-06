//! The four source modes through the engine (feature 050): a fake Put.io source that counts its
//! calls, a mock Whisper server and a fake ffmpeg. Unix only: the fakes are shell scripts.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use mockito::Matcher;
use putio_sync_client::{PutioSubtitle, SubtitleList};
use putio_sync_core::subtitles::{SubtitleMode, SubtitlesConfig, TranscriptionConfig};
use putio_sync_subtitles::engine::{self, Outcome, VideoRequest};
use putio_sync_subtitles::modes::ModePlan;
use putio_sync_subtitles::{BoxFut, SourceError, SubtitleSource};

const SRT: &str = "1\n00:00:01,000 --> 00:00:02,000\nHello\n\n";

/// Offers an English subtitle; counts calls and remembers the languages it was asked for
#[derive(Default)]
struct Source {
    offer: Vec<&'static str>,
    lists: Mutex<Vec<Vec<String>>>,
    downloads: Mutex<u32>,
}

impl Source {
    fn offering(codes: &[&'static str]) -> Self {
        Self {
            offer: codes.to_vec(),
            ..Default::default()
        }
    }
}

impl SubtitleSource for Source {
    fn list<'a>(
        &'a self,
        _: i64,
        languages: &'a [String],
    ) -> BoxFut<'a, Result<SubtitleList, SourceError>> {
        self.lists.lock().unwrap().push(languages.to_vec());
        let list = SubtitleList {
            default: None,
            subtitles: self
                .offer
                .iter()
                .map(|c| PutioSubtitle {
                    key: format!("k-{c}"),
                    format: "srt".into(),
                    language: c.to_string(),
                    language_code: c.to_string(),
                    name: c.to_string(),
                    source: "opensubtitles".into(),
                    url: String::new(),
                })
                .collect(),
        };
        Box::pin(async move { Ok(list) })
    }
    fn download<'a>(
        &'a self,
        _: i64,
        _: &'a PutioSubtitle,
    ) -> BoxFut<'a, Result<Vec<u8>, SourceError>> {
        *self.downloads.lock().unwrap() += 1;
        Box::pin(async { Ok(SRT.as_bytes().to_vec()) })
    }
}

fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn ffmpeg(dir: &Path) -> PathBuf {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    script(&bin, "ffprobe", "echo 100");
    script(
        &bin,
        "ffmpeg",
        "for last; do :; done\nhead -c 4000 /dev/zero > \"$last\"",
    )
}

fn transcription(url: &str, ffmpeg: &Path) -> TranscriptionConfig {
    TranscriptionConfig {
        enabled: true,
        url: url.to_string(),
        ffmpeg_path: ffmpeg.to_string_lossy().into_owned(),
        ..Default::default()
    }
}

fn answer(language: &str) -> String {
    format!(r#"{{"language":"{language}","segments":[{{"start":1.0,"end":3.0,"text":"Hi"}}]}}"#)
}

fn langs(l: &[&str]) -> Vec<String> {
    l.iter().map(|s| s.to_string()).collect()
}

struct Run {
    dir: tempfile::TempDir,
    video: PathBuf,
    whisper_calls: mockito::Mock,
    source: Source,
    out: Outcome,
}

impl Run {
    fn has(&self, name: &str) -> bool {
        self.dir.path().join(name).exists()
    }
}

/// What the pipeline does for one video: the download passes, then the transcription when the
/// mode asks for it
async fn run(
    modes: impl Fn(&[String]) -> ModePlan,
    wanted: &[&str],
    source: Source,
    heard: &str,
    whisper_expected: usize,
    existing: &[&str],
) -> Run {
    let dir = tempfile::tempdir().unwrap();
    let tools = ffmpeg(dir.path());
    let video = dir.path().join("Movie.mkv");
    std::fs::write(&video, b"video").unwrap();
    for e in existing {
        std::fs::write(dir.path().join(e), "mine").unwrap();
    }
    let mut server = mockito::Server::new_async().await;
    let whisper_calls = server
        .mock("POST", Matcher::Any)
        .with_status(200)
        .with_body(answer(heard))
        .expect(whisper_expected)
        .create_async()
        .await;
    let config = transcription(&server.url(), &tools);
    let wanted = langs(wanted);
    let plan = modes(&wanted);
    let req = VideoRequest::new(&video, Some(1), &wanted, &plan);
    let mut out = engine::fetch_for_video(&source, &req).await.unwrap();
    if engine::wants_transcription(&config, &plan, &out, &wanted) {
        engine::transcribe_video(&req, &config, &|_| {}, &mut out)
            .await
            .unwrap();
    }
    whisper_calls.assert_async().await;
    // keep the mock server alive until the assertion above
    drop(server);
    Run {
        dir,
        video,
        whisper_calls,
        source,
        out,
    }
}

fn uniform(mode: SubtitleMode) -> impl Fn(&[String]) -> ModePlan {
    move |l| ModePlan::uniform(l, mode)
}

#[tokio::test]
async fn download_only_never_calls_whisper() {
    let r = run(
        uniform(SubtitleMode::DownloadOnly),
        &["en", "es"],
        Source::offering(&["en"]),
        "en",
        0,
        &[],
    )
    .await;
    assert!(r.has("Movie.en.srt"));
    assert!(!r.has("Movie.en.whisper.srt") && !r.has("Movie.es.srt"));
    assert_eq!(r.out.missing, ["es"]);
    let _ = (&r.video, &r.whisper_calls);
}

#[tokio::test]
async fn download_then_whisper_skips_whisper_when_a_download_exists() {
    let r = run(
        uniform(SubtitleMode::DownloadThenWhisper),
        &["en"],
        Source::offering(&["en"]),
        "en",
        0,
        &[],
    )
    .await;
    assert!(r.has("Movie.en.srt"));
    assert_eq!(r.out.message(), "en: opensubtitles");
}

#[tokio::test]
async fn download_then_whisper_transcribes_when_nothing_is_found() {
    let r = run(
        uniform(SubtitleMode::DownloadThenWhisper),
        &["en"],
        Source::offering(&[]),
        "en",
        1,
        &[],
    )
    .await;
    assert!(r.has("Movie.en.srt"));
    assert!(!r.has("Movie.en.whisper.srt"), "no token outside Both");
    assert_eq!(r.out.message(), "en: transcribed");
}

#[tokio::test]
async fn download_then_whisper_skips_whisper_when_the_file_was_already_there() {
    let r = run(
        uniform(SubtitleMode::DownloadThenWhisper),
        &["en"],
        Source::offering(&[]),
        "en",
        0,
        &["Movie.en.srt"],
    )
    .await;
    assert_eq!(r.out.message(), "en: already present");
}

#[tokio::test]
async fn whisper_only_never_asks_put_io_and_writes_the_plain_name() {
    let r = run(
        uniform(SubtitleMode::WhisperOnly),
        &["en"],
        Source::offering(&["en"]),
        "en",
        1,
        &[],
    )
    .await;
    assert!(r.source.lists.lock().unwrap().is_empty(), "no Put.io call");
    assert_eq!(*r.source.downloads.lock().unwrap(), 0);
    assert!(r.has("Movie.en.srt") && !r.has("Movie.en.whisper.srt"));
    assert_eq!(r.out.message(), "en: transcribed");
}

#[tokio::test]
async fn whisper_only_keeps_an_existing_file_and_reports_it_present() {
    let r = run(
        uniform(SubtitleMode::WhisperOnly),
        &["en"],
        Source::offering(&[]),
        "en",
        0,
        &["Movie.en.srt"],
    )
    .await;
    assert_eq!(
        std::fs::read_to_string(r.dir.path().join("Movie.en.srt")).unwrap(),
        "mine"
    );
    assert_eq!(r.out.message(), "en: already present");
    assert!(r.source.lists.lock().unwrap().is_empty());
}

#[tokio::test]
async fn both_keeps_the_download_and_adds_a_whisper_file() {
    let r = run(
        uniform(SubtitleMode::Both),
        &["en"],
        Source::offering(&["en"]),
        "en",
        1,
        &[],
    )
    .await;
    assert!(r.has("Movie.en.srt") && r.has("Movie.en.whisper.srt"));
    assert_eq!(
        r.out.message(),
        "en: opensubtitles; en: transcribed (whisper)"
    );
    assert!(r.out.missing.is_empty());
}

#[tokio::test]
async fn both_with_an_existing_download_keeps_it_and_still_transcribes() {
    let r = run(
        uniform(SubtitleMode::Both),
        &["en"],
        Source::offering(&["en"]),
        "en",
        1,
        &["Movie.en.srt"],
    )
    .await;
    assert_eq!(
        std::fs::read_to_string(r.dir.path().join("Movie.en.srt")).unwrap(),
        "mine"
    );
    assert!(r.has("Movie.en.whisper.srt"));
    assert_eq!(*r.source.downloads.lock().unwrap(), 0);
}

#[tokio::test]
async fn both_does_not_transcribe_twice() {
    let r = run(
        uniform(SubtitleMode::Both),
        &["en"],
        Source::offering(&[]),
        "en",
        0,
        &["Movie.en.srt", "Movie.en.whisper.srt"],
    )
    .await;
    assert_eq!(r.out.whisper_present, ["en"]);
}

#[tokio::test]
async fn a_language_nobody_wanted_follows_the_mode_for_other_languages() {
    // Both: heard Spanish, wanted English: `.es.whisper.srt`
    let r = run(
        uniform(SubtitleMode::Both),
        &["en"],
        Source::offering(&[]),
        "spanish",
        1,
        &[],
    )
    .await;
    assert!(r.has("Movie.es.whisper.srt") && !r.has("Movie.es.srt"));
    assert_eq!(
        r.out.missing,
        ["en"],
        "the wanted language is still missing"
    );
    // Whisper only: plain name
    let r = run(
        uniform(SubtitleMode::WhisperOnly),
        &["en"],
        Source::offering(&[]),
        "spanish",
        1,
        &[],
    )
    .await;
    assert!(r.has("Movie.es.srt") && !r.has("Movie.es.whisper.srt"));
}

#[tokio::test]
async fn a_language_override_beats_the_category_and_the_global_mode() {
    let mut cfg = SubtitlesConfig {
        mode: Some(SubtitleMode::DownloadOnly),
        ..Default::default()
    };
    cfg.category_modes
        .insert("anime".into(), SubtitleMode::WhisperOnly);
    cfg.language_modes.insert("en".into(), SubtitleMode::Both);
    let r = run(
        move |l| ModePlan::new(&cfg, Some("Anime"), l),
        &["en", "es"],
        Source::offering(&["en", "es"]),
        "en",
        1,
        &[],
    )
    .await;
    // en: Both (download and whisper token); es: Whisper only (never downloaded)
    assert_eq!(
        *r.source.lists.lock().unwrap(),
        vec![vec!["en".to_string()]]
    );
    assert!(r.has("Movie.en.srt") && r.has("Movie.en.whisper.srt"));
    assert!(!r.has("Movie.es.srt"));
}

#[tokio::test]
async fn no_mode_in_the_config_is_the_025_behaviour() {
    let cfg = SubtitlesConfig::default();
    let r = run(
        move |l| ModePlan::new(&cfg, None, l),
        &["en"],
        Source::offering(&[]),
        "en",
        1,
        &[],
    )
    .await;
    assert!(r.has("Movie.en.srt"));
}
