//! Feature 025: the Subtitles stage inside the pipeline. A fake Put.io source stands in for
//! the API; the engine itself is tested in `putio-sync-subtitles`.

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use common::*;
#[cfg(unix)]
use mockito::Matcher;
use putio_sync_client::{PutioSubtitle, SubtitleList};
use putio_sync_core::config::StagingMode;
use putio_sync_core::subtitles::SubtitlesConfig;
use putio_sync_db::models::{DownloadSource, DownloadStatus, Origin};
use putio_sync_download::PathRedirect;
use putio_sync_pipeline::model::{ItemState, StepKind, StepState};
use putio_sync_pipeline::store::NewItem;
use putio_sync_pipeline::Stage as PStage;
use putio_sync_subtitles::{BoxFut, SourceError, SubtitleSource};

const SRT: &str = "1\n00:00:01,000 --> 00:00:02,000\nHello\n\n";
const BIG: u64 = 101 * 1024 * 1024;

struct Fake {
    list: Mutex<Result<SubtitleList, SourceError>>,
    lists: Mutex<u32>,
}

impl Fake {
    fn offering(codes: &[&str]) -> Arc<Fake> {
        Arc::new(Fake {
            list: Mutex::new(Ok(SubtitleList {
                default: None,
                subtitles: codes
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
            })),
            lists: Mutex::new(0),
        })
    }
}

impl SubtitleSource for Fake {
    fn list<'a>(
        &'a self,
        _: i64,
        _: &'a [String],
    ) -> BoxFut<'a, Result<SubtitleList, SourceError>> {
        *self.lists.lock().unwrap() += 1;
        let r = self.list.lock().unwrap().clone();
        Box::pin(async move { r })
    }
    fn download<'a>(
        &'a self,
        _: i64,
        _: &'a PutioSubtitle,
    ) -> BoxFut<'a, Result<Vec<u8>, SourceError>> {
        Box::pin(async { Ok(SRT.as_bytes().to_vec()) })
    }
}

fn subs(languages: &[&str], excluded: &[&str]) -> SubtitlesConfig {
    SubtitlesConfig {
        languages: languages.iter().map(|s| s.to_string()).collect(),
        excluded_categories: excluded.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    }
}

async fn env_subs(cfg: SubtitlesConfig) -> Env {
    env_custom(|_| vec![], move |c| c.subtitles = cfg).await
}

fn big_file(path: &Path, size: u64) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let f = std::fs::File::create(path).unwrap();
    f.set_len(size).unwrap();
}

/// Record a completed download of `path` as Put.io file `id`
async fn downloaded(env: &Env, id: i64, path: &Path) {
    env.db
        .downloads()
        .insert(
            id,
            &path.file_name().unwrap().to_string_lossy(),
            10,
            &path.to_string_lossy(),
            DownloadStatus::Completed,
        )
        .await
        .unwrap();
}

async fn steps(env: &Env, key: &str) -> Vec<putio_sync_pipeline::StepRow> {
    env.store.steps(key).await.unwrap()
}

#[tokio::test]
async fn no_languages_leaves_the_stage_not_configured() {
    let env = env_subs(subs(&[], &[])).await;
    downloaded_item(&env, "transfer:1", env.dir.path()).await;
    env.pipeline.run_due().await;
    let t = env.pipeline.timeline("transfer:1").await.unwrap();
    assert_eq!(t.state, ItemState::Done);
    assert_eq!(t.stages[3].skipped, Some("not_configured"));
}

#[tokio::test]
async fn an_excluded_category_skips_the_scan_with_the_category_reason() {
    let env = env_subs(subs(&["en"], &["Sonarr"])).await;
    downloaded_item(&env, "transfer:1", env.dir.path()).await;
    env.pipeline.set_subtitle_source(Fake::offering(&["en"]));
    env.pipeline.run_due().await;
    let t = env.pipeline.timeline("transfer:1").await.unwrap();
    assert_eq!(t.state, ItemState::Done);
    let scan = &t.stages[3].steps[0];
    assert_eq!(scan.kind, "subtitle_scan");
    assert_eq!(scan.state, "skipped");
    assert_eq!(scan.skip_reason, Some("category"));
}

#[tokio::test]
async fn a_season_pack_gets_one_step_per_video_and_samples_and_small_files_are_left_out() {
    let env = env_subs(subs(&["en"], &[])).await;
    let pack = env.dir.path().join("Show.S01");
    for n in ["E01", "E02", "E03"] {
        big_file(&pack.join(format!("{n}.mkv")), BIG);
    }
    big_file(&pack.join("Sample/sample.mkv"), BIG);
    big_file(&pack.join("E04.mkv"), 1024);
    std::fs::write(pack.join("E01.nfo"), "x").unwrap();
    downloaded_item(&env, "transfer:1", &pack).await;
    let source = Fake::offering(&[]);
    env.pipeline.set_subtitle_source(source.clone());
    env.pipeline.run_due().await;

    let t = env.pipeline.timeline("transfer:1").await.unwrap();
    assert_eq!(t.state, ItemState::Done);
    let line = &t.stages[3].steps;
    assert_eq!(line.len(), 4, "the scan and three videos");
    assert_eq!(line[0].kind, "subtitle_scan");
    let names: Vec<_> = line[1..].iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["E01.mkv", "E02.mkv", "E03.mkv"]);
    assert!(line.iter().all(|s| s.state == "done"));
    assert_eq!(
        line[0].attempts[0].output,
        "3 videos, 2 sample or small skipped"
    );
}

#[tokio::test]
async fn no_video_skips_the_scan_with_no_video() {
    let env = env_subs(subs(&["en"], &[])).await;
    let dir = env.dir.path().join("Album");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.flac"), "x").unwrap();
    downloaded_item(&env, "transfer:1", &dir).await;
    env.pipeline.set_subtitle_source(Fake::offering(&["en"]));
    env.pipeline.run_due().await;
    let t = env.pipeline.timeline("transfer:1").await.unwrap();
    assert_eq!(t.state, ItemState::Done);
    assert_eq!(t.stages[3].steps[0].skip_reason, Some("no_video"));
    assert_eq!(
        steps(&env, "transfer:1")
            .await
            .iter()
            .filter(|s| s.kind == StepKind::Subtitles)
            .count(),
        0
    );
}

#[tokio::test]
async fn a_subtitle_is_saved_next_to_the_video() {
    let env = env_subs(subs(&["en", "es"], &[])).await;
    let video = env.dir.path().join("Movie.2020.mkv");
    big_file(&video, BIG);
    downloaded(&env, 5, &video).await;
    downloaded_item(&env, "file:5", &video).await;
    env.pipeline.set_subtitle_source(Fake::offering(&["en"]));
    env.pipeline.run_due().await;
    assert!(env.dir.path().join("Movie.2020.en.srt").exists());
    assert!(!env.dir.path().join("Movie.2020.es.srt").exists());
    let t = env.pipeline.timeline("file:5").await.unwrap();
    assert_eq!(
        t.state,
        ItemState::Done,
        "a missing language is not a failure"
    );
    let step = &t.stages[3].steps[1];
    assert_eq!(step.attempts[0].output, "en: opensubtitles; es: missing");
}

#[tokio::test]
async fn each_saved_subtitle_is_recorded_with_its_source() {
    let env = env_subs(subs(&["en"], &[])).await;
    let video = env.dir.path().join("Movie.2020.mkv");
    big_file(&video, BIG);
    downloaded(&env, 5, &video).await;
    downloaded_item(&env, "file:5", &video).await;
    env.pipeline.set_subtitle_source(Fake::offering(&["en"]));
    env.pipeline.run_due().await;
    let saved = env.dir.path().join("Movie.2020.en.srt");
    let size = std::fs::metadata(&saved).unwrap().len() as i64;
    let got = env
        .store
        .provenance(&saved.to_string_lossy(), Some(size))
        .await
        .unwrap()
        .expect("a provenance row");
    assert_eq!(got.language, "en");
    assert_eq!(got.source, "opensubtitles");
}

#[tokio::test]
async fn a_failing_step_is_retried_then_the_item_finishes_with_warnings() {
    let env = env_subs(subs(&["en"], &[])).await;
    let video = env.dir.path().join("Movie.mkv");
    big_file(&video, BIG);
    downloaded(&env, 5, &video).await;
    downloaded_item(&env, "file:5", &video).await;
    let source = Fake::offering(&["en"]);
    *source.list.lock().unwrap() = Err(SourceError::Failed("503".into()));
    env.pipeline.set_subtitle_source(source.clone());
    for _ in 0..4 {
        env.pipeline.run_due().await;
    }
    let t = env.pipeline.timeline("file:5").await.unwrap();
    assert_eq!(t.state, ItemState::DoneWithWarnings);
    let step = &t.stages[3].steps[1];
    assert_eq!(step.state, "failed");
    assert_eq!(step.attempts.len(), 3, "optional_retries is [0, 0]");
    assert_eq!(*source.lists.lock().unwrap(), 3);
}

#[tokio::test]
async fn the_gate_keeps_radarr_and_sonarr_waiting_until_the_stage_ends() {
    let env = env_subs(subs(&["en"], &[])).await;
    let video = env.dir.path().join("Movie.mkv");
    big_file(&video, BIG);
    downloaded_item(&env, "transfer:3", &video).await;
    env.pipeline.set_subtitle_source(Fake::offering(&[]));
    let gate = env.pipeline.gate();
    env.pipeline.on_tick(&view(vec![])).await;
    assert_eq!(
        gate.pending("transfer:3").as_deref(),
        Some("subtitles: videos"),
        "the scan is due"
    );
    env.pipeline.run_due().await;
    assert_eq!(gate.pending("transfer:3"), None);
}

#[tokio::test]
async fn auto_staging_is_on_when_subtitles_are_configured() {
    let env = env_subs(subs(&["en"], &[])).await;
    env.store
        .insert_item(
            &NewItem {
                key: "transfer:1".to_string(),
                name: "Show".to_string(),
                category: Some("tv".to_string()),
                source: "watch".to_string(),
                dir: String::new(),
                transfer_id: Some(1),
                stage: PStage::Downloading,
                state: ItemState::Running,
            },
            &[],
        )
        .await
        .unwrap();
    env.db
        .sources()
        .record(
            10,
            &DownloadSource {
                transfer_id: Some(1),
                source_type: None,
                tracker_host: None,
                is_private: false,
                origin: Origin::Watch,
                watch_folder: None,
                putio_folder: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        env.pipeline.redirect(10, "tv/Show").await.as_deref(),
        Some("tv/.putio-sync-staging/Show")
    );
    let mut config = env.config.clone();
    config.subtitles = subs(&[], &[]);
    config.pipeline.staging = StagingMode::Auto;
    env.pipeline.configure(&config);
    assert_eq!(env.pipeline.redirect(10, "tv/Show").await, None);
}

#[tokio::test]
async fn a_staged_item_gets_its_subtitle_inside_the_staging_folder_and_moves_with_it() {
    let env = env_subs(subs(&["en"], &[])).await;
    let target = env.dir.path().join("downloads");
    let staged = target.join("tv/.putio-sync-staging/Show");
    let video = staged.join("Show.S01E01.mkv");
    big_file(&video, BIG);
    downloaded(&env, 7, &video).await;
    env.store
        .insert_item(
            &NewItem {
                key: "folder:tv/Show".to_string(),
                name: "Show".to_string(),
                category: Some("tv".to_string()),
                source: "watch".to_string(),
                dir: staged.to_string_lossy().into_owned(),
                transfer_id: None,
                stage: PStage::PreProcessing,
                state: ItemState::Running,
            },
            &putio_sync_pipeline::plan::plan_with(
                putio_sync_pipeline::plan::PlanInput {
                    category: Some("tv"),
                    has_transfer: false,
                    downloaded: true,
                },
                &[],
                Some(&env.config.subtitles),
                Some(&env.config.translation),
            ),
        )
        .await
        .unwrap();
    env.pipeline.set_subtitle_source(Fake::offering(&["en"]));
    env.pipeline.run_due().await;
    assert!(
        target.join("tv/Show/Show.S01E01.en.srt").exists(),
        "written beside the video, then moved with the item"
    );
    assert!(!staged.exists());
}

#[tokio::test]
async fn video_steps_keep_their_order_and_a_repeat_scan_adds_nothing() {
    let env = env_subs(subs(&["en"], &[])).await;
    let pack: PathBuf = env.dir.path().join("Pack");
    big_file(&pack.join("b.mkv"), BIG);
    big_file(&pack.join("a.mkv"), BIG);
    downloaded_item(&env, "transfer:1", &pack).await;
    env.pipeline.set_subtitle_source(Fake::offering(&[]));
    env.pipeline.run_due().await;
    let before = steps(&env, "transfer:1").await;
    // The scan step comes back (interrupted, say) and runs again
    let scan = before
        .iter()
        .find(|s| s.kind == StepKind::SubtitleScan)
        .unwrap();
    env.store
        .set_step(scan.id, StepState::Waiting, None, None)
        .await
        .unwrap();
    env.store
        .set_item("transfer:1", PStage::Subtitles, ItemState::Running)
        .await
        .unwrap();
    env.pipeline.run_due().await;
    let after = steps(&env, "transfer:1").await;
    assert_eq!(before.len(), after.len());
}

// ---- Transcription (US2): a fake ffmpeg and a mock server, never the real ones -----------

#[cfg(unix)]
mod transcription {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }

    /// ffprobe says 1500 s (three 600 s chunks); each ffmpeg run takes `pause` seconds
    fn tools(dir: &Path, pause: &str) -> PathBuf {
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        script(&bin, "ffprobe", "echo 1500");
        script(
            &bin,
            "ffmpeg",
            &format!("sleep {pause}\nfor last; do :; done\nhead -c 4000 /dev/zero > \"$last\""),
        )
    }

    fn config(url: &str, ffmpeg: &Path) -> SubtitlesConfig {
        let mut c = subs(&["en"], &[]);
        c.transcription.enabled = true;
        c.transcription.url = url.to_string();
        c.transcription.ffmpeg_path = ffmpeg.to_string_lossy().into_owned();
        c
    }

    const SPEECH: &str = r#"{"language":"en","segments":[{"start":1.0,"end":2.0,"text":"Hi"}]}"#;

    async fn with_video(env: &Env, key: &str) -> PathBuf {
        let video = env.dir.path().join("Movie.mkv");
        big_file(&video, BIG);
        downloaded(env, 5, &video).await;
        downloaded_item(env, key, &video).await;
        env.pipeline.set_subtitle_source(Fake::offering(&[]));
        video
    }

    #[tokio::test]
    async fn transcription_fills_in_when_put_io_has_nothing_and_progress_is_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = tools(dir.path(), "0");
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("POST", "/v1/audio/transcriptions")
            .with_status(200)
            .with_body(SPEECH)
            .expect(3)
            .create_async()
            .await;
        let cfg = config(&server.url(), &ffmpeg);
        let env = env_subs(cfg).await;
        with_video(&env, "file:5").await;
        env.pipeline.run_due().await;
        m.assert_async().await;
        assert!(env.dir.path().join("Movie.en.srt").exists());
        let t = env.pipeline.timeline("file:5").await.unwrap();
        assert_eq!(t.state, ItemState::Done);
        let step = &t.stages[3].steps[1];
        assert_eq!(step.attempts[0].output, "en: transcribed");
        assert_eq!(step.progress, None, "progress is cleared at the end");
    }

    #[tokio::test]
    async fn progress_is_written_after_each_chunk() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = tools(dir.path(), "0.3");
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", Matcher::Any)
            .with_status(200)
            .with_body(SPEECH)
            .create_async()
            .await;
        let env = env_subs(config(&server.url(), &ffmpeg)).await;
        with_video(&env, "file:5").await;
        let seen: Arc<Mutex<Vec<f64>>> = Arc::default();
        let poll = {
            let (seen, store) = (seen.clone(), env.store.clone());
            tokio::spawn(async move {
                loop {
                    if let Ok(steps) = store.steps("file:5").await {
                        for s in steps.iter().filter(|s| s.kind == StepKind::Subtitles) {
                            if let Some(p) = s.progress {
                                let mut v = seen.lock().unwrap();
                                if !v.iter().any(|x| (x - p).abs() < 1e-6) {
                                    v.push(p);
                                }
                            }
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                }
            })
        };
        env.pipeline.run_due().await;
        poll.abort();
        let v = seen.lock().unwrap().clone();
        assert!(
            v.iter().any(|p| (p - 1.0 / 3.0).abs() < 1e-3)
                && v.iter().any(|p| (p - 2.0 / 3.0).abs() < 1e-3),
            "progress per chunk: {v:?}"
        );
    }

    #[tokio::test]
    async fn a_server_that_is_down_retries_then_the_item_finishes_with_warnings() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = tools(dir.path(), "0");
        let mut server = mockito::Server::new_async().await;
        let m = server
            .mock("POST", Matcher::Any)
            .with_status(503)
            .with_body("model loading")
            .expect(3)
            .create_async()
            .await;
        let env = env_subs(config(&server.url(), &ffmpeg)).await;
        with_video(&env, "file:5").await;
        for _ in 0..4 {
            env.pipeline.run_due().await;
        }
        m.assert_async().await;
        let t = env.pipeline.timeline("file:5").await.unwrap();
        assert_eq!(t.state, ItemState::DoneWithWarnings);
        let step = &t.stages[3].steps[1];
        assert_eq!(step.attempts.len(), 3);
        assert!(
            step.attempts[0].output.contains("HTTP 503: model loading"),
            "{:?}",
            step.attempts[0].output
        );
        assert!(!env.dir.path().join("Movie.en.srt").exists());
    }

    #[tokio::test]
    async fn transcription_runs_one_video_at_a_time_across_items() {
        // Two items run their steps together (concurrency 2). Each fake ffmpeg run records
        // how many runs are active when it starts; the permit must keep that at one.
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        script(&bin, "ffprobe", "echo 600");
        let marks = dir.path().join("marks");
        std::fs::create_dir_all(&marks).unwrap();
        let counts = dir.path().join("counts");
        let ffmpeg = script(
            &bin,
            "ffmpeg",
            &format!(
                "touch {m}/run.$$\nls {m} | wc -l | tr -d ' ' >> {c}\nsleep 0.3\nrm {m}/run.$$\nfor last; do :; done\nhead -c 4000 /dev/zero > \"$last\"",
                m = marks.display(),
                c = counts.display()
            )
            .replace("\\n", "\n"),
        );
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", Matcher::Any)
            .with_status(200)
            .with_body(SPEECH)
            .create_async()
            .await;
        let env = env_subs(config(&server.url(), &ffmpeg)).await;
        for (id, name) in [(5, "A.mkv"), (6, "B.mkv")] {
            let video = env.dir.path().join(name);
            big_file(&video, BIG);
            downloaded(&env, id, &video).await;
            downloaded_item(&env, &format!("file:{id}"), &video).await;
        }
        env.pipeline.set_subtitle_source(Fake::offering(&[]));
        // The scans first, then both video steps at once
        env.pipeline.run_due().await;
        env.pipeline.run_due().await;
        assert!(
            env.dir.path().join("A.en.srt").exists() && env.dir.path().join("B.en.srt").exists()
        );
        let seen: Vec<String> = std::fs::read_to_string(&counts)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect();
        assert_eq!(seen.len(), 2, "{seen:?}");
        assert!(
            seen.iter().all(|n| n == "1"),
            "two ffmpeg runs overlapped: {seen:?}"
        );
    }

    #[tokio::test]
    async fn cancelling_the_item_kills_ffmpeg_and_leaves_the_step_interrupted() {
        let dir = tempfile::tempdir().unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        script(&bin, "ffprobe", "echo 1500");
        let pidfile = dir.path().join("pid");
        let ffmpeg = script(
            &bin,
            "ffmpeg",
            &format!("echo $$ > {}\nexec sleep 30", pidfile.display()),
        );
        let server = mockito::Server::new_async().await;
        let env = env_subs(config(&server.url(), &ffmpeg)).await;
        let video = env.dir.path().join("Movie.mkv");
        big_file(&video, BIG);
        downloaded(&env, 1, &video).await;
        downloaded_item(&env, "transfer:1", &video).await;
        env.pipeline.set_subtitle_source(Fake::offering(&[]));
        let run = {
            let pipeline = env.pipeline.clone();
            tokio::spawn(async move { pipeline.run_due().await })
        };
        for _ in 0..100 {
            if pidfile.exists() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        let pid = std::fs::read_to_string(&pidfile)
            .expect("ffmpeg started")
            .trim()
            .to_string();
        // Gone from the Active list for long enough: the item is cancelled
        for _ in 0..3 {
            env.pipeline.on_tick(&view(vec![])).await;
        }
        tokio::time::timeout(std::time::Duration::from_secs(10), run)
            .await
            .expect("the step stopped")
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        let alive = std::process::Command::new("kill")
            .args(["-0", &pid])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success();
        assert!(!alive, "ffmpeg {pid} still runs");
        let t = env.pipeline.timeline("transfer:1").await.unwrap();
        assert_eq!(t.state, ItemState::Cancelled);
        assert_eq!(t.stages[3].steps[1].state, "interrupted");
    }
}

mod manual {
    use super::*;
    use putio_sync_pipeline::subtitles::ManualError;

    #[tokio::test]
    async fn a_manual_item_has_one_subtitles_step_and_ignores_size_and_sample_rules() {
        let env = env_subs(subs(&["en"], &[])).await;
        // Small, and named like a sample: a scan would leave it out
        let video = env.dir.path().join("sample-clip.mkv");
        big_file(&video, 1024);
        downloaded(&env, 7, &video).await;
        env.pipeline.set_subtitle_source(Fake::offering(&["en"]));
        let key = env.pipeline.start_manual_subtitles(&video).await.unwrap();
        assert_eq!(key, "manual:1");
        let item = env.store.item(&key).await.unwrap().unwrap();
        assert_eq!(item.source, "manual");
        assert_eq!(item.dir, video.to_string_lossy());
        let rows = steps(&env, &key).await;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, StepKind::Subtitles);

        env.pipeline.run_due().await;
        assert!(env.dir.path().join("sample-clip.en.srt").exists());
        let t = env.pipeline.timeline(&key).await.unwrap();
        assert_eq!(t.state, ItemState::Done);
        // The tick must not mistake it for a download that left the Active list
        env.pipeline.on_tick(&view(vec![])).await;
        let t = env.pipeline.timeline(&key).await.unwrap();
        assert_eq!(t.state, ItemState::Done);
    }

    #[tokio::test]
    async fn a_second_request_for_a_running_item_is_a_duplicate_and_a_finished_one_is_not() {
        let env = env_subs(subs(&["en"], &[])).await;
        let video = env.dir.path().join("Movie.mkv");
        big_file(&video, BIG);
        env.pipeline.set_subtitle_source(Fake::offering(&[]));
        let first = env.pipeline.start_manual_subtitles(&video).await.unwrap();
        assert_eq!(
            env.pipeline.start_manual_subtitles(&video).await,
            Err(ManualError::Duplicate(first.clone()))
        );
        env.pipeline.run_due().await;
        let second = env.pipeline.start_manual_subtitles(&video).await.unwrap();
        assert_eq!(second, "manual:2");
    }

    #[tokio::test]
    async fn only_videos_are_accepted() {
        let env = env_subs(subs(&["en"], &[])).await;
        let note = env.dir.path().join("notes.txt");
        std::fs::write(&note, "x").unwrap();
        assert_eq!(
            env.pipeline.start_manual_subtitles(&note).await,
            Err(ManualError::NotAVideo)
        );
    }
}

/// Feature 050: source modes chosen per category, and Whisper output beside the downloaded file
#[cfg(unix)]
mod modes {
    use super::*;
    use putio_sync_core::subtitles::SubtitleMode;
    use std::os::unix::fs::PermissionsExt;

    fn tools(dir: &Path) -> PathBuf {
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        for (name, body) in [
            ("ffprobe", "echo 100".to_string()),
            (
                "ffmpeg",
                "for last; do :; done\nhead -c 4000 /dev/zero > \"$last\"".to_string(),
            ),
        ] {
            let p = bin.join(name);
            std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        bin.join("ffmpeg")
    }

    const SPEECH: &str = r#"{"language":"en","segments":[{"start":1.0,"end":2.0,"text":"Hi"}]}"#;

    fn config(url: &str, ffmpeg: &Path, excluded: &[&str]) -> SubtitlesConfig {
        let mut c = subs(&["en"], excluded);
        c.transcription.enabled = true;
        c.transcription.url = url.to_string();
        c.transcription.ffmpeg_path = ffmpeg.to_string_lossy().into_owned();
        c
    }

    async fn item(env: &Env, fake: Arc<Fake>) -> PathBuf {
        let video = env.dir.path().join("Movie.mkv");
        big_file(&video, BIG);
        downloaded(env, 5, &video).await;
        downloaded_item(env, "file:5", &video).await;
        env.pipeline.set_subtitle_source(fake);
        video
    }

    #[tokio::test]
    async fn a_category_in_whisper_only_never_asks_put_io_and_fills_the_main_slot() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = tools(dir.path());
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", "/v1/audio/transcriptions")
            .with_status(200)
            .with_body(SPEECH)
            .create_async()
            .await;
        let mut cfg = config(&server.url(), &ffmpeg, &[]);
        cfg.category_modes
            .insert("sonarr".into(), SubtitleMode::WhisperOnly);
        let env = env_subs(cfg).await;
        let fake = Fake::offering(&["en"]);
        item(&env, fake.clone()).await;
        env.pipeline.run_due().await;
        assert_eq!(*fake.lists.lock().unwrap(), 0, "Put.io was not asked");
        assert!(env.dir.path().join("Movie.en.srt").exists());
        assert!(!env.dir.path().join("Movie.en.whisper.srt").exists());
        let t = env.pipeline.timeline("file:5").await.unwrap();
        assert_eq!(t.state, ItemState::Done);
        let out = &t.stages[3].steps[1].attempts[0].output;
        assert!(out.contains("en: transcribed"), "{out}");
        assert!(out.contains("[whisper_only]"), "{out}");
        // Every transcription is recorded where it came from (feature 026 provenance)
        let saved = env.dir.path().join("Movie.en.srt");
        let size = std::fs::metadata(&saved).unwrap().len() as i64;
        let p = env
            .store
            .provenance(&saved.to_string_lossy(), Some(size))
            .await
            .unwrap()
            .expect("provenance recorded");
        assert_eq!(p.source, "transcribed");
    }

    #[tokio::test]
    async fn both_saves_the_download_and_the_transcription() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = tools(dir.path());
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", "/v1/audio/transcriptions")
            .with_status(200)
            .with_body(SPEECH)
            .create_async()
            .await;
        let mut cfg = config(&server.url(), &ffmpeg, &[]);
        cfg.mode = Some(SubtitleMode::Both);
        let env = env_subs(cfg).await;
        item(&env, Fake::offering(&["en"])).await;
        env.pipeline.run_due().await;
        assert!(env.dir.path().join("Movie.en.srt").exists());
        assert!(env.dir.path().join("Movie.en.whisper.srt").exists());
        let t = env.pipeline.timeline("file:5").await.unwrap();
        let out = &t.stages[3].steps[1].attempts[0].output;
        assert!(out.contains("en: transcribed (whisper)"), "{out}");
    }

    #[tokio::test]
    async fn an_excluded_category_beats_its_mode() {
        let dir = tempfile::tempdir().unwrap();
        let ffmpeg = tools(dir.path());
        let mut cfg = config("http://127.0.0.1:9", &ffmpeg, &["Sonarr"]);
        cfg.category_modes
            .insert("sonarr".into(), SubtitleMode::WhisperOnly);
        let env = env_subs(cfg).await;
        let fake = Fake::offering(&["en"]);
        item(&env, fake.clone()).await;
        env.pipeline.run_due().await;
        let t = env.pipeline.timeline("file:5").await.unwrap();
        assert_eq!(t.stages[3].steps[0].skip_reason, Some("category"));
        assert_eq!(*fake.lists.lock().unwrap(), 0);
        assert!(!env.dir.path().join("Movie.en.whisper.srt").exists());
    }

    #[tokio::test]
    async fn a_config_without_modes_keeps_the_025_behaviour_and_message() {
        let env = env_subs(subs(&["en"], &[])).await;
        item(&env, Fake::offering(&["en"])).await;
        env.pipeline.run_due().await;
        assert!(env.dir.path().join("Movie.en.srt").exists());
        let t = env.pipeline.timeline("file:5").await.unwrap();
        let out = &t.stages[3].steps[1].attempts[0].output;
        assert!(!out.contains('['), "no mode label for the default: {out}");
    }
}

/// Feature 050: the OpenSubtitles pass inside the pipeline, with a fake search and a fake *arr
mod opensubtitles {
    use super::*;
    use putio_sync_core::media::{MediaInfo, MediaKind, MediaLookup, MediaRequest};
    use putio_sync_subtitles::opensubtitles::{Hit, Quota, SearchError, SubtitleSearch};

    struct Search {
        hits: Vec<Hit>,
        calls: Mutex<Vec<Vec<(String, String)>>>,
    }

    impl SubtitleSearch for Search {
        fn search<'a>(
            &'a self,
            params: &'a [(String, String)],
        ) -> BoxFut<'a, Result<Vec<Hit>, SearchError>> {
            self.calls.lock().unwrap().push(params.to_vec());
            let hits = self.hits.clone();
            Box::pin(async move { Ok(hits) })
        }
        fn download<'a>(&'a self, _: i64) -> BoxFut<'a, Result<Vec<u8>, SearchError>> {
            Box::pin(async { Ok(SRT.as_bytes().to_vec()) })
        }
        fn quota(&self) -> Quota {
            Quota::default()
        }
    }

    struct Arr(Option<MediaInfo>);

    impl MediaLookup for Arr {
        fn lookup<'a>(&'a self, _: MediaRequest<'a>) -> BoxFut<'a, Option<MediaInfo>> {
            let r = self.0.clone();
            Box::pin(async move { r })
        }
    }

    fn search(release: &str) -> Arc<Search> {
        Arc::new(Search {
            hits: vec![Hit {
                file_id: 1,
                language: "en".into(),
                release: release.into(),
                title: "Movie".into(),
                year: Some(2024),
                ..Default::default()
            }],
            calls: Mutex::default(),
        })
    }

    const RELEASE: &str = "Movie.2024.1080p.WEB-DL.x264-GRP";

    #[tokio::test]
    async fn a_download_with_ids_in_the_arr_is_matched_by_them_when_put_io_has_nothing() {
        let env = env_subs(subs(&["en"], &[])).await;
        let video = env.dir.path().join(format!("{RELEASE}.mkv"));
        big_file(&video, BIG);
        downloaded(&env, 5, &video).await;
        downloaded_item(&env, "file:5", &video).await;
        env.pipeline.set_subtitle_source(Fake::offering(&[]));
        let s = search(RELEASE);
        env.pipeline.set_subtitle_search(s.clone());
        env.pipeline.set_media_lookup(Arc::new(Arr(Some(MediaInfo {
            kind: Some(MediaKind::Movie),
            imdb_id: Some("903747".into()),
            ..Default::default()
        }))));
        env.pipeline.run_due().await;
        assert!(env.dir.path().join(format!("{RELEASE}.en.srt")).exists());
        let t = env.pipeline.timeline("file:5").await.unwrap();
        let out = &t.stages[3].steps[1].attempts[0].output;
        assert!(out.contains("en: os_ids"), "{out}");
        assert!(s
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|p| p.iter().any(|(k, v)| k == "imdb_id" && v == "903747")));
        // Recorded where it came from
        let saved = env.dir.path().join(format!("{RELEASE}.en.srt"));
        let size = std::fs::metadata(&saved).unwrap().len() as i64;
        let p = env
            .store
            .provenance(&saved.to_string_lossy(), Some(size))
            .await
            .unwrap()
            .expect("provenance recorded");
        // The table keeps the origin (OpenSubtitles), not the matching level
        assert_eq!(p.source, "opensubtitles");
    }

    #[tokio::test]
    async fn put_io_comes_first_and_stops_the_pass() {
        let env = env_subs(subs(&["en"], &[])).await;
        let video = env.dir.path().join("Movie.mkv");
        big_file(&video, BIG);
        downloaded(&env, 5, &video).await;
        downloaded_item(&env, "file:5", &video).await;
        env.pipeline.set_subtitle_source(Fake::offering(&["en"]));
        let s = search(RELEASE);
        env.pipeline.set_subtitle_search(s.clone());
        env.pipeline.run_due().await;
        assert!(env.dir.path().join("Movie.en.srt").exists());
        assert!(s.calls.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_manual_item_with_no_ids_is_matched_by_its_file_name() {
        let env = env_subs(subs(&["en"], &[])).await;
        let video = env.dir.path().join(format!("{RELEASE}.mkv"));
        big_file(&video, BIG);
        env.pipeline.set_subtitle_source(Fake::offering(&[]));
        let s = search(RELEASE);
        env.pipeline.set_subtitle_search(s.clone());
        let key = env.pipeline.start_manual_subtitles(&video).await.unwrap();
        env.pipeline.run_due().await;
        assert!(env.dir.path().join(format!("{RELEASE}.en.srt")).exists());
        let t = env.pipeline.timeline(&key).await.unwrap();
        assert_eq!(t.state, ItemState::Done);
        let out = t
            .stages
            .iter()
            .flat_map(|s| &s.steps)
            .flat_map(|s| &s.attempts)
            .map(|a| a.output.as_str())
            .collect::<String>();
        assert!(out.contains("os_name"), "{out}");
    }

    #[tokio::test]
    async fn without_a_key_nothing_is_searched_and_the_language_stays_missing() {
        let env = env_subs(subs(&["en"], &[])).await;
        let video = env.dir.path().join("Movie.mkv");
        big_file(&video, BIG);
        downloaded(&env, 5, &video).await;
        downloaded_item(&env, "file:5", &video).await;
        env.pipeline.set_subtitle_source(Fake::offering(&[]));
        env.pipeline.run_due().await;
        assert!(!env.dir.path().join("Movie.en.srt").exists());
        let t = env.pipeline.timeline("file:5").await.unwrap();
        let out = &t.stages[3].steps[1].attempts[0].output;
        assert!(out.contains("en: missing"), "{out}");
    }
}
