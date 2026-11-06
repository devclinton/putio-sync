//! Helpers for the pipeline tests (feature 038): a seeded database, hooks that are tiny
//! shell scripts in a temp dir, and Active-list views built by hand.

#![allow(dead_code)]

use std::path::Path;
use std::sync::Arc;

use putio_sync_core::config::Config;
use putio_sync_core::hooks::{HookConfig, HookStage};
use putio_sync_db::pipeline::{FinishedOutcome, FinishedRow};
use putio_sync_db::Database;
use putio_sync_download::pipeline::{
    Counts, FileCounts, Links, Origin, Pipeline as ActiveView, PipelineItem, Stage, State,
};
use putio_sync_pipeline::store::{NewItem, Store};
use putio_sync_pipeline::{ItemState, Pipeline, Stage as PStage};
use tempfile::TempDir;

/// A script in `dir` that runs `body` with `sh`; returns the command for a hook
pub fn script(dir: &Path, name: &str, body: &str) -> Vec<String> {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    vec![path.to_string_lossy().into_owned()]
}

/// A pipeline hook running `command`
pub fn hook(name: &str, stage: HookStage, command: Vec<String>) -> HookConfig {
    HookConfig {
        name: name.to_string(),
        stage: Some(stage),
        command: Some(command),
        timeout_secs: 10,
        ..Default::default()
    }
}

pub struct Env {
    pub pipeline: Pipeline,
    pub db: Arc<Database>,
    pub store: Store,
    pub dir: TempDir,
    pub config: Config,
}

/// A pipeline over a fresh database with `hooks` configured and no retry delay
pub async fn env(hooks: Vec<HookConfig>) -> Env {
    env_with(|_| hooks).await
}

/// Like [`env`], with hooks made from the temp dir, where their scripts live
pub async fn env_with(make: impl FnOnce(&Path) -> Vec<HookConfig>) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let hooks = make(dir.path());
    let db = Arc::new(Database::new(&dir.path().join("t.db")).await.unwrap());
    let mut config = Config {
        hooks,
        ..Default::default()
    };
    config.pipeline.optional_retries = vec![0, 0];
    config.download.target_directory = dir.path().join("downloads");
    std::fs::create_dir_all(&config.download.target_directory).unwrap();
    let pipeline = Pipeline::new(db.clone(), &config).await;
    let store = Store::new(db.pool().clone());
    Env {
        pipeline,
        db,
        store,
        dir,
        config,
    }
}

/// A row of the Active list
pub fn active(key: &str, transfer_id: Option<i64>, stage: Stage, state: State) -> PipelineItem {
    PipelineItem {
        key: key.to_string(),
        name: format!("name-of-{key}"),
        transfer_id,
        stage,
        state,
        held: false,
        paused: false,
        size: 100,
        done_bytes: 0,
        percent: 0.0,
        speed_bps: None,
        eta_seconds: None,
        files: Some(FileCounts::default()),
        putio_status: None,
        seeding: false,
        stall: None,
        origin: Some(Origin::Arr("sonarr".to_string())),
        reason: None,
        next_sync_at: None,
        note: None,
        phase: None,
        links: Links {
            transfers: None,
            queue: String::new(),
        },
    }
}

pub fn view(items: Vec<PipelineItem>) -> ActiveView {
    ActiveView {
        items,
        counts: Counts::default(),
        putio_as_of: None,
        putio_stale: false,
    }
}

/// A finished row, as `record_finished` writes it
pub async fn finished(db: &Database, key: &str, outcome: FinishedOutcome) {
    db.finished()
        .upsert(&FinishedRow {
            key: key.to_string(),
            name: format!("name-of-{key}"),
            size: 100,
            outcome,
            reason: None,
            added_at: None,
            putio_done_at: None,
            queued_at: None,
            done_at: "2999-01-01 00:00:00".to_string(),
        })
        .await
        .unwrap();
}

/// An item already downloaded and waiting for its processing steps, with its plan from the
/// pipeline's own configuration
pub async fn downloaded_item(env: &Env, key: &str, dir: &Path) {
    let steps = putio_sync_pipeline::plan::plan(
        putio_sync_pipeline::plan::PlanInput {
            category: Some("sonarr"),
            has_transfer: true,
            downloaded: true,
        },
        &env.config.hooks,
    );
    env.store
        .insert_item(
            &NewItem {
                key: key.to_string(),
                name: format!("name-of-{key}"),
                category: Some("sonarr".to_string()),
                source: "arr".to_string(),
                dir: dir.to_string_lossy().into_owned(),
                transfer_id: key.strip_prefix("transfer:").and_then(|s| s.parse().ok()),
                stage: PStage::PreProcessing,
                state: ItemState::Running,
            },
            &steps,
        )
        .await
        .unwrap();
}
