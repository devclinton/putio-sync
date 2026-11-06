//! Feature 025: a downloaded file is not deleted from Put.io until the Subtitles stage ends.

mod common;

use common::*;
use putio_sync_core::subtitles::SubtitlesConfig;
use putio_sync_download::RemoteDeleteGate;
use putio_sync_pipeline::model::ItemState;
use putio_sync_pipeline::store::NewItem;
use putio_sync_pipeline::Stage as PStage;

fn subs() -> SubtitlesConfig {
    SubtitlesConfig {
        languages: vec!["en".to_string()],
        excluded_categories: vec!["radarr".to_string()],
        ..Default::default()
    }
}

async fn env_on() -> Env {
    env_custom(|_| vec![], |c| c.subtitles = subs()).await
}

async fn item(env: &Env, key: &str, stage: PStage, state: ItemState) {
    env.store
        .insert_item(
            &NewItem {
                key: key.to_string(),
                name: "n".to_string(),
                category: None,
                source: "sync".to_string(),
                dir: String::new(),
                transfer_id: None,
                stage,
                state,
            },
            &[],
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn the_gate_defers_videos_only_and_only_when_subtitles_are_on() {
    let env = env_on().await;
    let p = &env.pipeline;
    assert_eq!(
        p.defer(5, "tv/Show", "e01.mkv").await.as_deref(),
        Some("folder:tv/Show")
    );
    assert_eq!(p.defer(5, "", "e01.mkv").await.as_deref(), Some("file:5"));
    assert_eq!(
        p.defer(5, "tv/Show", "e01.en.srt").await.as_deref(),
        Some("folder:tv/Show")
    );
    assert_eq!(p.defer(5, "tv/Show", "cover.jpg").await, None);
    assert_eq!(
        p.defer(5, "Radarr/Film", "film.mkv").await,
        None,
        "an excluded category is not held"
    );

    let mut config = env.config.clone();
    config.subtitles = SubtitlesConfig::default();
    p.configure(&config);
    assert_eq!(p.defer(5, "tv/Show", "e01.mkv").await, None);
}

#[tokio::test]
async fn a_transfer_file_waits_for_its_transfer_item() {
    let env = env_on().await;
    env.db
        .sources()
        .record(
            50,
            &putio_sync_db::models::DownloadSource {
                transfer_id: Some(9),
                source_type: None,
                tracker_host: None,
                is_private: false,
                origin: putio_sync_db::models::Origin::Putio,
                watch_folder: None,
                putio_folder: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        env.pipeline
            .defer(50, "tv/Show", "e01.mkv")
            .await
            .as_deref(),
        Some("transfer:9")
    );
}

#[tokio::test]
async fn a_delete_is_not_released_before_the_stage_ends_and_is_after() {
    let env = env_on().await;
    item(
        &env,
        "transfer:1",
        PStage::PreProcessing,
        ItemState::Running,
    )
    .await;
    env.db
        .deferred_deletes()
        .insert(11, "transfer:1", now())
        .await
        .unwrap();
    assert!(env.pipeline.releasable_deletes().await.is_empty());

    env.store
        .set_item("transfer:1", PStage::Subtitles, ItemState::Running)
        .await
        .unwrap();
    assert!(
        env.pipeline.releasable_deletes().await.is_empty(),
        "still in the stage"
    );

    env.store
        .set_item("transfer:1", PStage::PostProcessing, ItemState::Running)
        .await
        .unwrap();
    assert_eq!(env.pipeline.releasable_deletes().await, [11]);

    env.pipeline.mark_deleted(&[11]).await;
    assert!(env.db.deferred_deletes().list().await.unwrap().is_empty());
}

#[tokio::test]
async fn a_finished_failed_or_vanished_item_releases_and_a_missing_one_waits() {
    let env = env_on().await;
    item(&env, "transfer:1", PStage::Subtitles, ItemState::Failed).await;
    item(&env, "transfer:2", PStage::Subtitles, ItemState::Cancelled).await;
    for (id, key) in [(1, "transfer:1"), (2, "transfer:2"), (3, "transfer:3")] {
        env.db
            .deferred_deletes()
            .insert(id, key, now())
            .await
            .unwrap();
    }
    let mut ready = env.pipeline.releasable_deletes().await;
    ready.sort();
    assert_eq!(ready, [1, 2], "an item the pipeline has not seen yet waits");
}

#[tokio::test]
async fn the_24_hour_failsafe_releases_a_stuck_item() {
    let env = env_on().await;
    item(
        &env,
        "transfer:1",
        PStage::PreProcessing,
        ItemState::WaitingForOperator,
    )
    .await;
    env.db
        .deferred_deletes()
        .insert(1, "transfer:1", now() - 24 * 3600 - 1)
        .await
        .unwrap();
    env.db
        .deferred_deletes()
        .insert(2, "transfer:1", now() - 3600)
        .await
        .unwrap();
    assert_eq!(env.pipeline.releasable_deletes().await, [1]);
}

#[tokio::test]
async fn turning_subtitles_off_releases_everything() {
    let env = env_on().await;
    item(
        &env,
        "transfer:1",
        PStage::PreProcessing,
        ItemState::Running,
    )
    .await;
    env.db
        .deferred_deletes()
        .insert(1, "transfer:1", now())
        .await
        .unwrap();
    let mut config = env.config.clone();
    config.subtitles = SubtitlesConfig::default();
    env.pipeline.configure(&config);
    assert_eq!(env.pipeline.releasable_deletes().await, [1]);
}

#[tokio::test]
async fn a_restart_keeps_the_held_rows() {
    let env = env_on().await;
    item(
        &env,
        "transfer:1",
        PStage::PreProcessing,
        ItemState::Running,
    )
    .await;
    env.db
        .deferred_deletes()
        .insert(1, "transfer:1", now())
        .await
        .unwrap();
    // A new service over the same database, as after a restart
    let again = putio_sync_pipeline::Pipeline::new(env.db.clone(), &env.config).await;
    again.startup().await;
    assert_eq!(env.db.deferred_deletes().list().await.unwrap().len(), 1);
    assert!(again.releasable_deletes().await.is_empty());
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
