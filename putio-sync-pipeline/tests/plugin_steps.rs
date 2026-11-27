//! Feature 057: a plugin as a pipeline step, and the `pipeline.finished` event. The
//! plugins service is a fake here; the real one is tested in the plugins crate.

mod common;

use std::sync::{Arc, Mutex};

use common::*;
use putio_sync_core::events::{self, Payload, PipelineFinished};
use putio_sync_core::hooks::{HookConfig, HookStage};
use putio_sync_core::step_plugins::{StepAnswer, StepPlugins};
use putio_sync_core::sync_hooks::HookFuture;
use putio_sync_download::pipeline::{Stage, State};
use putio_sync_pipeline::model::ItemState;
use serde_json::Value;

/// Answers every call with one fixed answer and keeps what it was asked
struct Fake {
    answer: StepAnswer,
    calls: Mutex<Vec<(String, Value, String)>>,
}

impl Fake {
    fn new(answer: StepAnswer) -> Arc<Self> {
        Arc::new(Fake {
            answer,
            calls: Mutex::new(Vec::new()),
        })
    }
}

impl StepPlugins for Fake {
    fn run_item<'a>(
        &'a self,
        plugin: &'a str,
        item: &'a Value,
        stage: &'a str,
    ) -> HookFuture<'a, StepAnswer> {
        self.calls
            .lock()
            .unwrap()
            .push((plugin.to_string(), item.clone(), stage.to_string()));
        Box::pin(async move { self.answer.clone() })
    }

    fn problem(&self, _plugin: &str) -> Option<String> {
        None
    }
}

fn plugin_hook(name: &str, stage: HookStage, plugin: &str) -> HookConfig {
    HookConfig {
        name: name.to_string(),
        stage: Some(stage),
        plugin: Some(plugin.to_string()),
        ..Default::default()
    }
}

fn finished_for(
    rx: &mut tokio::sync::broadcast::Receiver<Arc<events::Event>>,
    key: &str,
) -> Vec<PipelineFinished> {
    let mut out = Vec::new();
    while let Ok(event) = rx.try_recv() {
        if let Payload::PipelineFinished(p) = &event.payload {
            if p.key == key {
                out.push(p.clone());
            }
        }
    }
    out
}

#[tokio::test]
async fn a_plugin_step_gets_the_item_and_the_stage_and_the_item_finishes() {
    let env = env(vec![plugin_hook(
        "sort",
        HookStage::PostProcessing,
        "tv-folders",
    )])
    .await;
    let fake = Fake::new(StepAnswer::Done);
    env.pipeline.set_step_plugins(fake.clone());
    let item_dir = env.dir.path().join("item/Show.S01E01");
    std::fs::create_dir_all(&item_dir).unwrap();
    std::fs::write(item_dir.join("a.mkv"), b"12345").unwrap();
    downloaded_item(&env, "transfer:571", &item_dir).await;
    let mut rx = events::subscribe();
    env.pipeline.run_due().await;

    let calls = fake.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    let (plugin, item, stage) = &calls[0];
    assert_eq!(plugin, "tv-folders");
    assert_eq!(stage, "post_processing");
    assert_eq!(item["key"], "transfer:571");
    assert_eq!(item["dir"], item_dir.to_string_lossy().as_ref());
    assert_eq!(item["files"][0]["relative_path"], "a.mkv");

    let t = env.pipeline.timeline("transfer:571").await.unwrap();
    assert_eq!(t.state, ItemState::Done);
    assert_eq!(t.stages[5].steps[0].state, "done");

    // One pipeline.finished, after every step, with the final folder and its files
    let finished = finished_for(&mut rx, "transfer:571");
    assert_eq!(finished.len(), 1);
    let f = &finished[0];
    assert_eq!(f.result, "success");
    assert_eq!(f.name, "name-of-transfer:571");
    assert_eq!(f.category.as_deref(), Some("sonarr"));
    assert_eq!(f.folder, item_dir.to_string_lossy());
    assert_eq!(f.files, ["a.mkv"]);
    assert!(f.warnings.is_empty());
    assert!(f.duration_seconds >= 0.0);
}

#[tokio::test]
async fn a_failing_required_plugin_step_stops_the_item_and_no_finished_event_goes_out() {
    let env = env(vec![plugin_hook(
        "sort",
        HookStage::PostProcessing,
        "tv-folders",
    )])
    .await;
    env.pipeline
        .set_step_plugins(Fake::new(StepAnswer::Failed("no room left".to_string())));
    downloaded_item(&env, "transfer:572", env.dir.path()).await;
    let mut rx = events::subscribe();
    env.pipeline.run_due().await;

    let t = env.pipeline.timeline("transfer:572").await.unwrap();
    assert_eq!(t.state, ItemState::WaitingForOperator);
    let step = &t.stages[5].steps[0];
    assert_eq!(step.state, "failed");
    assert_eq!(step.attempts[0].result, Some("failed"));
    assert!(step.attempts[0].output.contains("no room left"));
    assert!(finished_for(&mut rx, "transfer:572").is_empty());
}

#[tokio::test]
async fn an_optional_plugin_step_that_times_out_ends_with_warnings() {
    let mut hook = plugin_hook("sort", HookStage::PostProcessing, "tv-folders");
    hook.required = false;
    let env = env(vec![hook]).await;
    env.pipeline.set_step_plugins(Fake::new(StepAnswer::Timeout(
        "ran past its 5 s limit".to_string(),
    )));
    downloaded_item(&env, "transfer:573", env.dir.path()).await;
    let mut rx = events::subscribe();
    env.pipeline.run_due().await;

    let t = env.pipeline.timeline("transfer:573").await.unwrap();
    assert_eq!(t.state, ItemState::DoneWithWarnings);
    assert_eq!(t.stages[5].steps[0].attempts[0].result, Some("timeout"));
    let finished = finished_for(&mut rx, "transfer:573");
    assert_eq!(finished.len(), 1);
    assert_eq!(finished[0].result, "warnings");
    assert_eq!(finished[0].warnings, ["sort"]);
}

#[tokio::test]
async fn a_plugin_step_without_a_plugins_service_fails_and_names_the_plugin() {
    let env = env(vec![plugin_hook(
        "sort",
        HookStage::PostProcessing,
        "tv-folders",
    )])
    .await;
    downloaded_item(&env, "transfer:574", env.dir.path()).await;
    env.pipeline.run_due().await;
    let t = env.pipeline.timeline("transfer:574").await.unwrap();
    assert_eq!(t.state, ItemState::WaitingForOperator);
    assert!(t.stages[5].steps[0].attempts[0]
        .output
        .contains("tv-folders"));
}

#[tokio::test]
async fn a_pre_download_plugin_skip_ends_the_item_as_skipped() {
    let env = env(vec![plugin_hook("gate", HookStage::PreDownload, "gate")]).await;
    env.pipeline.set_step_plugins(Fake::new(StepAnswer::Skip));
    let list = view(vec![active(
        "transfer:575",
        Some(575),
        Stage::Putio,
        State::Downloading,
    )]);
    let mut rx = events::subscribe();
    env.pipeline.on_tick(&list).await;
    env.pipeline.run_due().await;
    let t = env.pipeline.timeline("transfer:575").await.unwrap();
    assert_eq!(t.state, ItemState::Skipped);
    assert_eq!(t.stages[0].steps[0].skip_reason, Some("hook"));
    let finished = finished_for(&mut rx, "transfer:575");
    assert_eq!(finished.len(), 1);
    assert_eq!(finished[0].result, "skipped");
    assert!(finished[0].files.is_empty());
}
