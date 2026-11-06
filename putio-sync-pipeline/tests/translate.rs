//! Feature 026: the Translating stage inside the pipeline, against a mock chat server. No real
//! model, translation service or Put.io is used.

mod common;

use std::path::{Path, PathBuf};

use common::*;
use mockito::{Server, ServerGuard};
use putio_sync_core::config::StagingMode;
use putio_sync_core::subtitles::SubtitlesConfig;
use putio_sync_core::translation::{ChatConfig, TranslationConfig, TranslatorKind};
use putio_sync_db::models::DownloadStatus;
use putio_sync_pipeline::model::{ItemState, StepKind, StepState};
use putio_sync_pipeline::store::NewItem;
use putio_sync_pipeline::Stage as PStage;

const SRT: &str = "1\n00:00:01,000 --> 00:00:02,000\nHello there\n\n2\n00:00:03,000 --> 00:00:04,500\nHow are you?\n\n3\n00:00:05,000 --> 00:00:06,000\n\u{266a}\n\n";
const BIG: u64 = 101 * 1024 * 1024;
/// Stage index of Translating in a timeline
const T: usize = 4;

/// A chat server that translates every line to `<lang-code> <text>`, taking the code from the
/// system prompt; a target listed in `broken` gets a 401.
async fn chat_server(broken: &'static [&'static str]) -> ServerGuard {
    let mut s = Server::new_async().await;
    s.mock("POST", "/v1/chat/completions")
        .with_body_from_request(move |req| {
            let body: serde_json::Value = serde_json::from_slice(req.body().unwrap()).unwrap();
            let system = body.pointer("/messages/0/content").unwrap().as_str().unwrap();
            let user = body.pointer("/messages/1/content").unwrap().as_str().unwrap();
            let code = system
                .split(" to ")
                .nth(1)
                .and_then(|r| r.split('(').nth(1))
                .and_then(|r| r.split(')').next())
                .unwrap_or("??")
                .to_string();
            if broken.contains(&code.as_str()) {
                // mockito cannot change the status from here: a body the parser refuses
                return b"{\"error\":\"nope\"}".to_vec();
            }
            let tail = user.rsplit("Translate these lines:\n").next().unwrap();
            let lines: Vec<String> = tail
                .lines()
                .filter_map(|l| l.split_once("| "))
                .map(|(n, t)| format!("{n}| {code} {t}"))
                .collect();
            serde_json::json!({"choices":[{"finish_reason":"stop","message":{"content": lines.join("\n")}}]})
                .to_string()
                .into_bytes()
        })
        .expect_at_least(0)
        .create_async()
        .await;
    s
}

fn subs(languages: &[&str]) -> SubtitlesConfig {
    SubtitlesConfig {
        languages: languages.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    }
}

fn chat(url: &str) -> TranslationConfig {
    TranslationConfig {
        kind: TranslatorKind::Chat,
        chat: ChatConfig {
            base_url: url.to_string(),
            model: "m".to_string(),
            api_key: String::new(),
        },
        ..Default::default()
    }
}

async fn env_for(languages: &[&str], translation: TranslationConfig) -> Env {
    let subtitles = subs(languages);
    env_custom(
        |_| vec![],
        move |c| {
            c.subtitles = subtitles;
            c.translation = translation;
        },
    )
    .await
}

fn big_file(path: &Path, size: u64) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let f = std::fs::File::create(path).unwrap();
    f.set_len(size).unwrap();
}

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

/// A movie on disk with an English subtitle, registered as item `file:5`
async fn movie_with_en(env: &Env) -> PathBuf {
    let video = env.dir.path().join("Movie.2020.mkv");
    big_file(&video, BIG);
    std::fs::write(env.dir.path().join("Movie.2020.en.srt"), SRT).unwrap();
    downloaded(env, 5, &video).await;
    downloaded_item(env, "file:5", &video).await;
    video
}

async fn steps(env: &Env, key: &str) -> Vec<putio_sync_pipeline::StepRow> {
    env.store.steps(key).await.unwrap()
}

fn translate_steps(all: &[putio_sync_pipeline::StepRow]) -> Vec<&putio_sync_pipeline::StepRow> {
    all.iter()
        .filter(|s| s.kind == StepKind::Translate)
        .collect()
}

#[tokio::test]
async fn kind_none_leaves_the_stage_not_configured() {
    let env = env_for(&["en", "es"], TranslationConfig::default()).await;
    movie_with_en(&env).await;
    env.pipeline.run_due().await;
    let t = env.pipeline.timeline("file:5").await.unwrap();
    assert_eq!(t.stages[T].skipped, Some("not_configured"));
    assert!(translate_steps(&steps(&env, "file:5").await).is_empty());
}

#[tokio::test]
async fn no_wanted_languages_leaves_the_stage_not_configured() {
    let s = chat_server(&[]).await;
    let env = env_for(&[], chat(&s.url())).await;
    movie_with_en(&env).await;
    env.pipeline.run_due().await;
    let t = env.pipeline.timeline("file:5").await.unwrap();
    assert_eq!(t.stages[T].skipped, Some("not_configured"));
}

#[tokio::test]
async fn the_scan_adds_one_step_per_video_and_missing_language_as_branches() {
    let s = chat_server(&[]).await;
    let env = env_for(&["en", "es", "fr"], chat(&s.url())).await;
    movie_with_en(&env).await;
    env.pipeline.run_due().await;
    let all = steps(&env, "file:5").await;
    let found: Vec<_> = translate_steps(&all)
        .iter()
        .map(|s| (s.branch.clone().unwrap(), s.name.clone()))
        .collect();
    assert_eq!(
        found,
        [
            ("es".to_string(), "Movie.2020.mkv".to_string()),
            ("fr".to_string(), "Movie.2020.mkv".to_string())
        ],
        "en exists, so only es and fr; one branch per language"
    );
    let t = env.pipeline.timeline("file:5").await.unwrap();
    assert_eq!(t.state, ItemState::Done);
    assert_eq!(
        t.stages[T].steps[0].attempts[0].output,
        "2 subtitles to translate"
    );
}

#[tokio::test]
async fn a_translation_keeps_cue_count_and_timestamps_and_records_provenance() {
    let s = chat_server(&[]).await;
    let env = env_for(&["en", "es"], chat(&s.url())).await;
    let video = movie_with_en(&env).await;
    env.pipeline.run_due().await;
    let out = env.dir.path().join("Movie.2020.es.srt");
    let text = std::fs::read_to_string(&out).unwrap();
    let cues = putio_sync_subtitles::srt::parse(&text);
    let src = putio_sync_subtitles::srt::parse(SRT);
    assert_eq!(cues.len(), src.len());
    for (a, b) in cues.iter().zip(&src) {
        assert_eq!((a.start_ms, a.end_ms), (b.start_ms, b.end_ms));
    }
    assert_eq!(cues[0].text, "es Hello there");
    assert_eq!(cues[2].text, "\u{266a}", "a cue with no words is copied");
    let size = std::fs::metadata(&out).unwrap().len() as i64;
    let p = env
        .store
        .provenance(&out.to_string_lossy(), Some(size))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (p.source.as_str(), p.language.as_str()),
        ("translated", "es")
    );
    assert!(video.exists());
    let t = env.pipeline.timeline("file:5").await.unwrap();
    assert!(t.stages[T].steps.iter().all(|s| s.state == "done"));
}

#[tokio::test]
async fn nothing_to_translate_passes_the_scan() {
    let s = chat_server(&[]).await;
    let env = env_for(&["en"], chat(&s.url())).await;
    movie_with_en(&env).await;
    env.pipeline.run_due().await;
    let t = env.pipeline.timeline("file:5").await.unwrap();
    assert_eq!(t.state, ItemState::Done);
    assert_eq!(
        t.stages[T].steps[0].attempts[0].output,
        "nothing to translate"
    );
}

#[tokio::test]
async fn no_source_subtitle_means_no_steps() {
    let s = chat_server(&[]).await;
    let env = env_for(&["en", "es"], chat(&s.url())).await;
    let video = env.dir.path().join("Movie.mkv");
    big_file(&video, BIG);
    downloaded(&env, 5, &video).await;
    downloaded_item(&env, "file:5", &video).await;
    env.pipeline.run_due().await;
    assert!(translate_steps(&steps(&env, "file:5").await).is_empty());
    assert_eq!(
        env.pipeline.timeline("file:5").await.unwrap().state,
        ItemState::Done
    );
}

#[tokio::test]
async fn a_whisper_file_beside_the_download_is_a_translation_source() {
    let s = chat_server(&[]).await;
    let env = env_for(&["en", "es"], chat(&s.url())).await;
    let video = env.dir.path().join("Movie.mkv");
    big_file(&video, BIG);
    // Only the transcription exists (Both mode kept it apart from the main slot)
    std::fs::write(env.dir.path().join("Movie.en.whisper.srt"), SRT).unwrap();
    downloaded(&env, 5, &video).await;
    downloaded_item(&env, "file:5", &video).await;
    env.pipeline.run_due().await;
    let all = steps(&env, "file:5").await;
    let found: Vec<_> = translate_steps(&all)
        .iter()
        .map(|s| s.branch.clone().unwrap())
        .collect();
    assert_eq!(
        found,
        ["es"],
        "en is not saved in the main slot, so it is the source"
    );
}

#[tokio::test]
async fn a_season_pack_gets_steps_per_video_and_no_video_skips_the_scan() {
    let s = chat_server(&[]).await;
    let env = env_for(&["en", "es"], chat(&s.url())).await;
    let pack = env.dir.path().join("Show.S01");
    for n in ["E01", "E02"] {
        big_file(&pack.join(format!("{n}.mkv")), BIG);
        std::fs::write(pack.join(format!("{n}.en.srt")), SRT).unwrap();
    }
    downloaded_item(&env, "transfer:1", &pack).await;
    let album = env.dir.path().join("Album");
    std::fs::create_dir_all(&album).unwrap();
    std::fs::write(album.join("a.flac"), "x").unwrap();
    downloaded_item(&env, "transfer:2", &album).await;
    env.pipeline.run_due().await;
    assert!(pack.join("E01.es.srt").exists() && pack.join("E02.es.srt").exists());
    let t = env.pipeline.timeline("transfer:2").await.unwrap();
    assert_eq!(t.stages[T].steps[0].skip_reason, Some("no_video"));
}

#[tokio::test]
async fn an_excluded_category_skips_the_scan() {
    let s = chat_server(&[]).await;
    let mut translation = chat(&s.url());
    translation.excluded_categories = vec!["Sonarr".to_string()];
    let env = env_for(&["en", "es"], translation).await;
    movie_with_en(&env).await;
    env.pipeline.run_due().await;
    let t = env.pipeline.timeline("file:5").await.unwrap();
    assert_eq!(t.stages[T].steps[0].skip_reason, Some("category"));
    assert!(!env.dir.path().join("Movie.2020.es.srt").exists());
}

#[tokio::test]
async fn one_language_failing_does_not_stop_the_other_and_the_item_finishes_with_warnings() {
    let s = chat_server(&["fr"]).await;
    let env = env_for(&["en", "es", "fr"], chat(&s.url())).await;
    movie_with_en(&env).await;
    for _ in 0..4 {
        env.pipeline.run_due().await;
    }
    assert!(env.dir.path().join("Movie.2020.es.srt").exists());
    assert!(!env.dir.path().join("Movie.2020.fr.srt").exists());
    let t = env.pipeline.timeline("file:5").await.unwrap();
    assert_eq!(t.state, ItemState::DoneWithWarnings);
    let all = steps(&env, "file:5").await;
    let fr = all
        .iter()
        .find(|s| s.branch.as_deref() == Some("fr"))
        .unwrap();
    assert_eq!(fr.state, StepState::Failed);
    let attempts = env.store.attempts(fr.id).await.unwrap();
    assert_eq!(attempts.len(), 3, "optional_retries is [0, 0]");
}

#[tokio::test]
async fn the_gate_keeps_radarr_and_sonarr_waiting_until_the_stage_ends() {
    let s = chat_server(&[]).await;
    let env = env_for(&["en", "es"], chat(&s.url())).await;
    let video = env.dir.path().join("Movie.mkv");
    big_file(&video, BIG);
    std::fs::write(env.dir.path().join("Movie.en.srt"), SRT).unwrap();
    downloaded_item(&env, "transfer:3", &video).await;
    let gate = env.pipeline.gate();
    env.pipeline.on_tick(&view(vec![])).await;
    assert!(gate.pending("transfer:3").is_some());
    env.pipeline.run_due().await;
    assert_eq!(gate.pending("transfer:3"), None);
}

#[tokio::test]
async fn a_target_that_appeared_meanwhile_passes_already_exists() {
    let s = chat_server(&[]).await;
    let env = env_for(&["en", "es"], chat(&s.url())).await;
    let video = movie_with_en(&env).await;
    // Run the scan only, then let another tool write the file
    env.pipeline.run_due().await;
    let es = env.dir.path().join("Movie.2020.es.srt");
    std::fs::remove_file(&es).unwrap();
    std::fs::write(&es, "1\n00:00:01,000 --> 00:00:02,000\nmine\n\n").unwrap();
    let step = steps(&env, "file:5")
        .await
        .into_iter()
        .find(|s| s.kind == StepKind::Translate)
        .unwrap();
    env.store
        .set_step(step.id, StepState::Waiting, None, None)
        .await
        .unwrap();
    env.store
        .set_item("file:5", PStage::Translating, ItemState::Running)
        .await
        .unwrap();
    env.pipeline.run_due().await;
    assert!(std::fs::read_to_string(&es).unwrap().contains("mine"));
    let a = env.store.attempts(step.id).await.unwrap();
    assert!(a.last().unwrap().output.contains("already exists"));
    assert!(video.exists());
}

#[tokio::test]
async fn staging_writes_inside_the_staging_folder_and_provenance_follows_the_rename() {
    let s = chat_server(&[]).await;
    let env = env_for(&["en", "es"], chat(&s.url())).await;
    let target = env.dir.path().join("downloads");
    let staged = target.join("tv/.putio-sync-staging/Show");
    let video = staged.join("Show.S01E01.mkv");
    big_file(&video, BIG);
    std::fs::write(staged.join("Show.S01E01.en.srt"), SRT).unwrap();
    downloaded(&env, 7, &video).await;
    let mut config = env.config.clone();
    config.pipeline.staging = StagingMode::Auto;
    env.pipeline.configure(&config);
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
    env.pipeline.run_due().await;
    let final_srt = target.join("tv/Show/Show.S01E01.es.srt");
    assert!(final_srt.exists(), "moved with the item");
    assert!(!staged.exists());
    let p = env
        .store
        .provenance(&final_srt.to_string_lossy(), None)
        .await
        .unwrap();
    assert_eq!(p.map(|p| p.source), Some("translated".to_string()));
}

#[tokio::test]
async fn a_manual_translation_runs_one_branch_per_language() {
    let s = chat_server(&[]).await;
    let env = env_for(&["en"], chat(&s.url())).await;
    let video = env.dir.path().join("Movie.2020.mkv");
    big_file(&video, 10);
    std::fs::write(env.dir.path().join("Movie.2020.en.srt"), SRT).unwrap();
    let key = env
        .pipeline
        .start_manual_translation(&video, &["es".to_string(), "fr".to_string()])
        .await
        .unwrap();
    assert!(key.starts_with("manual:"));
    env.pipeline.run_due().await;
    for code in ["es", "fr"] {
        let text =
            std::fs::read_to_string(env.dir.path().join(format!("Movie.2020.{code}.srt"))).unwrap();
        assert!(text.contains(&format!("{code} Hello there")), "{text}");
    }
    let t = env.pipeline.timeline(&key).await.unwrap();
    assert_eq!(t.state, ItemState::Done);
    assert!(env
        .pipeline
        .start_manual_translation(&video, &[])
        .await
        .is_err());
}
