//! Feature 025: the Subtitles stage inside the pipeline. A fake Put.io source stands in for
//! the API; the engine itself is tested in `putio-sync-subtitles`.

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use common::*;
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
