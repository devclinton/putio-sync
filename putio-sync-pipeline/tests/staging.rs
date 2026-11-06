//! Feature 038, story 3: watch-folder staging. An item is built in a hidden folder and
//! moved into place with one rename.

mod common;

use common::*;
use putio_sync_core::config::StagingMode;
use putio_sync_core::hooks::HookStage;
use putio_sync_db::models::{DownloadSource, DownloadStatus, Origin};
use putio_sync_download::PathRedirect;
use putio_sync_pipeline::model::ItemState;
use putio_sync_pipeline::staging::{check_same_device, final_path, staged_dir, STAGING_DIR};
use putio_sync_pipeline::store::NewItem;
use putio_sync_pipeline::Stage as PStage;

#[test]
fn the_staging_folder_goes_in_front_of_the_item() {
    assert_eq!(
        staged_dir("tv/Show/s1", "Show"),
        "tv/.putio-sync-staging/Show/s1"
    );
    assert_eq!(staged_dir("tv/Show", "Show"), "tv/.putio-sync-staging/Show");
    // A single file: the item is the file, so the folder is its category
    assert_eq!(
        staged_dir("movies", "film.mkv"),
        "movies/.putio-sync-staging"
    );
    assert_eq!(staged_dir("", "film.mkv"), ".putio-sync-staging");
}

#[test]
fn an_existing_target_becomes_a_numbered_name() {
    let dir = tempfile::tempdir().unwrap();
    let staged = dir.path().join("tv").join(STAGING_DIR).join("Show");
    std::fs::create_dir_all(&staged).unwrap();
    assert_eq!(final_path(&staged).unwrap(), dir.path().join("tv/Show"));
    std::fs::create_dir_all(dir.path().join("tv/Show")).unwrap();
    assert_eq!(final_path(&staged).unwrap(), dir.path().join("tv/Show (2)"));
    std::fs::create_dir_all(dir.path().join("tv/Show (2)")).unwrap();
    assert_eq!(final_path(&staged).unwrap(), dir.path().join("tv/Show (3)"));
    // Not under a staging folder: nothing to move
    assert!(final_path(&dir.path().join("tv/Show")).is_none());
}

#[test]
fn a_staging_folder_on_another_filesystem_is_refused() {
    use std::os::unix::fs::MetadataExt;
    let dir = tempfile::tempdir().unwrap();
    assert!(check_same_device(dir.path()).is_ok(), "nothing staged yet");
    std::fs::create_dir_all(dir.path().join("tv").join(STAGING_DIR)).unwrap();
    assert!(check_same_device(dir.path()).is_ok());

    // /dev is its own filesystem on macOS and Linux; a link to it stands in for a mount
    let other = tempfile::tempdir().unwrap();
    if std::fs::metadata("/dev").unwrap().dev() == std::fs::metadata(other.path()).unwrap().dev() {
        return;
    }
    std::os::unix::fs::symlink("/dev", other.path().join(STAGING_DIR)).unwrap();
    let err = check_same_device(other.path()).unwrap_err();
    assert!(err.contains("another filesystem"), "{err}");
}

/// A pipeline item for a watch-folder transfer named `Show`
async fn watch_item(env: &Env, transfer_id: i64, origin: Origin, dir: &std::path::Path) {
    env.store
        .insert_item(
            &NewItem {
                key: format!("transfer:{transfer_id}"),
                name: "Show".to_string(),
                category: Some("tv".to_string()),
                source: "watch".to_string(),
                dir: dir.to_string_lossy().into_owned(),
                transfer_id: Some(transfer_id),
                stage: PStage::Downloading,
                state: ItemState::Running,
            },
            &[],
        )
        .await
        .unwrap();
    watch_source(env, transfer_id, origin).await;
}

/// The download source of file `transfer_id * 10`
async fn watch_source(env: &Env, transfer_id: i64, origin: Origin) {
    env.db
        .sources()
        .record(
            transfer_id * 10,
            &DownloadSource {
                transfer_id: Some(transfer_id),
                source_type: None,
                tracker_host: None,
                is_private: false,
                origin,
                watch_folder: None,
                putio_folder: None,
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn only_watch_folder_files_are_redirected_and_only_while_staging_is_on() {
    let env = env_with(|dir| {
        vec![hook(
            "tidy",
            HookStage::PostProcessing,
            script(dir, "tidy", "true"),
        )]
    })
    .await;
    watch_item(&env, 1, Origin::Watch, std::path::Path::new("")).await;
    watch_item(&env, 2, Origin::Unknown, std::path::Path::new("")).await;

    assert_eq!(
        env.pipeline.redirect(10, "tv/Show/s1").await.as_deref(),
        Some("tv/.putio-sync-staging/Show/s1"),
        "a watch-folder file is staged (auto: a pipeline hook exists)"
    );
    assert_eq!(env.pipeline.redirect(20, "tv/Show/s1").await, None);
    assert_eq!(
        env.pipeline.redirect(99, "tv/Show").await,
        None,
        "no source"
    );

    let mut config = env.config.clone();
    config.pipeline.staging = StagingMode::Off;
    env.pipeline.configure(&config);
    assert_eq!(env.pipeline.redirect(10, "tv/Show/s1").await, None);

    config.pipeline.staging = StagingMode::Auto;
    config.hooks.clear();
    env.pipeline.configure(&config);
    assert_eq!(
        env.pipeline.redirect(10, "tv/Show/s1").await,
        None,
        "auto with no steps places files as today (FR-016)"
    );
    config.pipeline.staging = StagingMode::On;
    env.pipeline.configure(&config);
    assert!(env.pipeline.redirect(10, "tv/Show/s1").await.is_some());
}

#[tokio::test]
async fn the_item_appears_in_its_category_only_with_the_final_rename() {
    let env = env_with(|dir| {
        let target = dir.join("downloads");
        // Runs in the staging folder: edits the item, and records whether the category
        // listing showed it meanwhile
        let body = format!(
            "ls {t}/tv > {d}/listing; echo subs > {t}/tv/.putio-sync-staging/Show/extra.srt",
            t = target.display(),
            d = dir.display()
        );
        vec![hook(
            "tidy",
            HookStage::PostProcessing,
            script(dir, "tidy", &body),
        )]
    })
    .await;
    let target = env.dir.path().join("downloads");
    let staged = target.join("tv").join(STAGING_DIR).join("Show");
    std::fs::create_dir_all(&staged).unwrap();
    std::fs::write(staged.join("ep.mkv"), b"video").unwrap();
    env.db
        .downloads()
        .insert(
            10,
            "ep.mkv",
            5,
            &staged.join("ep.mkv").to_string_lossy(),
            DownloadStatus::Completed,
        )
        .await
        .unwrap();
    watch_source(&env, 1, Origin::Watch).await;
    downloaded_item_named(&env, "transfer:1", "Show", &staged).await;

    env.pipeline.run_due().await;

    let listing = std::fs::read_to_string(env.dir.path().join("listing")).unwrap();
    assert!(
        !listing.contains("Show"),
        "the category showed the item before the rename: {listing}"
    );
    assert!(target.join("tv/Show/ep.mkv").exists());
    assert_eq!(
        std::fs::read_to_string(target.join("tv/Show/extra.srt"))
            .unwrap()
            .trim(),
        "subs"
    );
    assert!(!staged.exists());
    assert_eq!(
        env.pipeline.timeline("transfer:1").await.unwrap().state,
        ItemState::Done
    );
    let row = env
        .db
        .downloads()
        .find_by_file_id(10)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        row.local_path,
        target.join("tv/Show/ep.mkv").to_string_lossy()
    );
    let item = env.store.item("transfer:1").await.unwrap().unwrap();
    assert_eq!(item.dir, target.join("tv/Show").to_string_lossy());
}
