//! Feature 038, story 1: where an item is. The stages in order, restarts, failed
//! downloads and parallel branches.

mod common;

use common::*;
use putio_sync_core::hooks::HookStage;
use putio_sync_db::pipeline::FinishedOutcome;
use putio_sync_download::pipeline::{Stage, State};
use putio_sync_pipeline::model::{ItemState, StepKind, StepState};
use putio_sync_pipeline::plan::PlannedStep;
use putio_sync_pipeline::store::NewItem;
use putio_sync_pipeline::Stage as PStage;

#[tokio::test]
async fn with_no_hooks_the_timeline_is_download_done_notify_done_and_the_rest_not_configured() {
    let env = env(vec![]).await;
    env.pipeline
        .on_tick(&view(vec![active(
            "transfer:1",
            Some(1),
            Stage::Putio,
            State::Downloading,
        )]))
        .await;
    let t = env.pipeline.timeline("transfer:1").await.unwrap();
    assert_eq!(t.stages.len(), 7);
    let names: Vec<_> = t.stages.iter().map(|s| s.stage.as_str()).collect();
    assert_eq!(
        names,
        [
            "pre_download",
            "downloading",
            "pre_processing",
            "subtitles",
            "translating",
            "post_processing",
            "notifications"
        ],
        "FR-001 order"
    );
    assert_eq!(t.stages[1].steps[0].state, "waiting", "still downloading");

    // The download ends: it leaves the list and a finished row is written
    finished(&env.db, "transfer:1", FinishedOutcome::Done).await;
    env.pipeline.on_tick(&view(vec![])).await;
    env.pipeline.run_due().await;
    let t = env.pipeline.timeline("transfer:1").await.unwrap();
    assert_eq!(t.state, ItemState::Done);
    assert_eq!(t.stages[1].steps[0].state, "done");
    assert_eq!(t.stages[6].steps[0].state, "done");
    for i in [0, 2, 3, 4, 5] {
        assert_eq!(
            t.stages[i].skipped,
            Some("not_configured"),
            "{}",
            t.stages[i].stage.as_str()
        );
    }
}

#[tokio::test]
async fn stages_run_in_the_fr_001_order_whatever_the_config_order() {
    let env = env_with(|dir| {
        let log = dir.join("order.log");
        let append = |name: &str| script(dir, name, &format!("echo {name} >> {}", log.display()));
        vec![
            hook("post", HookStage::PostProcessing, append("post")),
            hook("pre-b", HookStage::PreProcessing, append("pre-b")),
            hook("pre-a", HookStage::PreProcessing, append("pre-a")),
        ]
    })
    .await;
    downloaded_item(&env, "transfer:1", env.dir.path()).await;
    env.pipeline.run_due().await;
    let log = std::fs::read_to_string(env.dir.path().join("order.log")).unwrap();
    assert_eq!(log.lines().collect::<Vec<_>>(), ["pre-b", "pre-a", "post"]);
    assert_eq!(
        env.pipeline.timeline("transfer:1").await.unwrap().state,
        ItemState::Done
    );
}

#[tokio::test]
async fn a_restart_runs_an_interrupted_step_again_and_never_a_done_one() {
    let env = env_with(|dir| {
        let count = |name: &str| {
            script(
                dir,
                name,
                &format!("echo x >> {}/{name}.count", dir.display()),
            )
        };
        vec![
            hook("first", HookStage::PreProcessing, count("first")),
            hook("second", HookStage::PostProcessing, count("second")),
        ]
    })
    .await;
    downloaded_item(&env, "transfer:1", env.dir.path()).await;
    let steps = env.store.steps("transfer:1").await.unwrap();
    let first = steps.iter().find(|s| s.name == "first").unwrap().id;
    let second = steps.iter().find(|s| s.name == "second").unwrap().id;
    // The daemon stopped while `first` ran: it was never finished
    env.store.start_attempt(first).await.unwrap();

    env.pipeline.startup().await;
    assert_eq!(
        env.store.step(first).await.unwrap().unwrap().state,
        StepState::Interrupted
    );
    env.pipeline.run_due().await;
    let count = |n: &str| {
        std::fs::read_to_string(env.dir.path().join(format!("{n}.count")))
            .unwrap()
            .lines()
            .count()
    };
    assert_eq!(count("first"), 1, "ran again");
    assert_eq!(count("second"), 1);
    let attempts = env.store.attempts(first).await.unwrap();
    assert_eq!(
        attempts.len(),
        2,
        "the interrupted attempt stays in the history"
    );

    // Another restart: done steps are not run again
    env.pipeline.startup().await;
    env.pipeline.run_due().await;
    assert_eq!(count("first"), 1);
    assert_eq!(count("second"), 1);
    assert_eq!(
        env.store.step(second).await.unwrap().unwrap().state,
        StepState::Done
    );
}

#[tokio::test]
async fn an_item_whose_download_failed_has_its_later_steps_not_reached() {
    let env = env_with(|dir| {
        vec![hook(
            "tidy",
            HookStage::PostProcessing,
            script(dir, "tidy", "exit 0"),
        )]
    })
    .await;
    env.pipeline
        .on_tick(&view(vec![active(
            "transfer:1",
            Some(1),
            Stage::Nas,
            State::Downloading,
        )]))
        .await;
    env.pipeline
        .on_tick(&view(vec![active(
            "transfer:1",
            Some(1),
            Stage::Nas,
            State::Failed,
        )]))
        .await;
    let t = env.pipeline.timeline("transfer:1").await.unwrap();
    assert_eq!(t.state, ItemState::Failed);
    let tidy = &t.stages[5].steps[0];
    assert_eq!(tidy.state, "not_reached");
    env.pipeline.run_due().await;
    assert_eq!(
        env.pipeline.timeline("transfer:1").await.unwrap().stages[5].steps[0].state,
        "not_reached",
        "nothing ran"
    );

    // It recovers: back to running
    env.pipeline
        .on_tick(&view(vec![active(
            "transfer:1",
            Some(1),
            Stage::Nas,
            State::Downloading,
        )]))
        .await;
    assert_eq!(
        env.pipeline.timeline("transfer:1").await.unwrap().state,
        ItemState::Running
    );
}

#[tokio::test]
async fn an_item_that_vanishes_without_a_finished_row_is_cancelled() {
    let env = env(vec![]).await;
    env.pipeline
        .on_tick(&view(vec![active(
            "transfer:1",
            Some(1),
            Stage::Putio,
            State::Downloading,
        )]))
        .await;
    env.pipeline.on_tick(&view(vec![])).await;
    assert_eq!(
        env.pipeline.timeline("transfer:1").await.unwrap().state,
        ItemState::Running,
        "once is not enough"
    );
    env.pipeline.on_tick(&view(vec![])).await;
    assert_eq!(
        env.pipeline.timeline("transfer:1").await.unwrap().state,
        ItemState::Cancelled
    );
}

/// The branch API (026 will use it): two branches in one stage run at the same time, and
/// the stage is done when both are
#[tokio::test]
async fn two_branches_in_one_stage_run_at_the_same_time() {
    let env = env_with(|dir| {
        // Each waits for the other's marker, so they can only finish together
        let wait_for = |me: &str, other: &str| {
            script(
                dir,
                me,
                &format!(
                    "touch {d}/{me}.started; i=0; while [ ! -f {d}/{other}.started ]; do i=$((i+1)); [ $i -gt 100 ] && exit 1; sleep 0.05; done",
                    d = dir.display()
                ),
            )
        };
        vec![
            hook("en", HookStage::PostProcessing, wait_for("en", "es")),
            hook("es", HookStage::PostProcessing, wait_for("es", "en")),
        ]
    })
    .await;
    // Plan the two hooks as branches of the same stage
    let mut steps = putio_sync_pipeline::plan::plan(
        putio_sync_pipeline::plan::PlanInput {
            category: None,
            has_transfer: true,
            downloaded: true,
        },
        &env.config.hooks,
    );
    for s in steps.iter_mut().filter(|s| s.kind == StepKind::Hook) {
        s.branch = Some(s.name.clone());
    }
    let steps: Vec<PlannedStep> = steps;
    env.store
        .insert_item(
            &NewItem {
                key: "transfer:9".to_string(),
                name: "n".to_string(),
                category: None,
                source: "sync".to_string(),
                dir: String::new(),
                transfer_id: Some(9),
                stage: PStage::PreProcessing,
                state: ItemState::Running,
            },
            &steps,
        )
        .await
        .unwrap();
    env.pipeline.run_due().await;
    let t = env.pipeline.timeline("transfer:9").await.unwrap();
    assert_eq!(
        t.state,
        ItemState::Done,
        "both finished, so they ran together: {t:#?}"
    );
    let post = &t.stages[5];
    assert_eq!(post.branches.as_ref().unwrap().len(), 2);
    assert!(post.steps.is_empty());
}

#[tokio::test]
async fn steps_in_one_branch_run_in_position_order() {
    let env = env_with(|dir| {
        let log = dir.join("o.log");
        let a = |n: &str| script(dir, n, &format!("echo {n} >> {}", log.display()));
        vec![
            hook("one", HookStage::PostProcessing, a("one")),
            hook("two", HookStage::PostProcessing, a("two")),
        ]
    })
    .await;
    downloaded_item(&env, "transfer:1", env.dir.path()).await;
    env.pipeline.run_due().await;
    let log = std::fs::read_to_string(env.dir.path().join("o.log")).unwrap();
    assert_eq!(log.lines().collect::<Vec<_>>(), ["one", "two"]);
}

#[tokio::test]
async fn a_hold_pauses_the_item_between_steps_and_release_goes_on() {
    let env = env_with(|dir| {
        vec![hook(
            "tidy",
            HookStage::PostProcessing,
            script(dir, "tidy", "true"),
        )]
    })
    .await;
    downloaded_item(&env, "transfer:3", env.dir.path()).await;
    env.db.holds().add(3, "name-of-transfer:3").await.unwrap();
    env.pipeline.on_tick(&view(vec![])).await;
    let t = env.pipeline.timeline("transfer:3").await.unwrap();
    assert_eq!(t.state, ItemState::Held);

    env.pipeline.run_due().await;
    let t = env.pipeline.timeline("transfer:3").await.unwrap();
    assert_eq!(t.state, ItemState::Held, "no step starts while held");
    let steps = env.store.steps("transfer:3").await.unwrap();
    assert!(steps
        .iter()
        .filter(|s| s.name == "tidy")
        .all(|s| s.state == StepState::Waiting));

    env.db.holds().remove(3).await.unwrap();
    env.pipeline.on_tick(&view(vec![])).await;
    assert_eq!(
        env.pipeline.timeline("transfer:3").await.unwrap().state,
        ItemState::Running,
        "release returns it to running"
    );
    env.pipeline.run_due().await;
    assert_eq!(
        env.pipeline.timeline("transfer:3").await.unwrap().state,
        ItemState::Done
    );
}

#[tokio::test]
async fn the_gate_holds_a_downloaded_item_from_the_moment_its_steps_start() {
    let env = env_with(|dir| {
        vec![hook(
            "slow",
            HookStage::PostProcessing,
            script(dir, "slow", "sleep 1"),
        )]
    })
    .await;
    downloaded_item(&env, "transfer:4", env.dir.path()).await;
    let gate = env.pipeline.gate();
    assert_eq!(gate.pending("transfer:4"), None, "nothing refreshed it yet");
    let running = tokio::spawn({
        let pipeline = env.pipeline.clone();
        async move { pipeline.run_due().await }
    });
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    assert_eq!(
        gate.pending("transfer:4").as_deref(),
        Some("post_processing: slow")
    );
    running.await.unwrap();
    assert_eq!(
        gate.pending("transfer:4"),
        None,
        "clear once the step is done"
    );
}

fn subtitle_step(branch: &str, position: i64) -> PlannedStep {
    PlannedStep {
        stage: PStage::Subtitles,
        branch: Some(branch.to_string()),
        position,
        kind: StepKind::Subtitles,
        name: format!("subtitles {branch}"),
        required: false,
        changes_files: false,
        state: StepState::Waiting,
        skip_reason: None,
    }
}

#[tokio::test]
async fn insert_steps_adds_in_order_with_branches_and_is_idempotent() {
    let env = env(vec![]).await;
    let dir = tempfile::tempdir().unwrap();
    downloaded_item(&env, "transfer:7", dir.path()).await;
    let before = env.store.steps("transfer:7").await.unwrap().len();
    let steps = vec![subtitle_step("a.mkv", 0), subtitle_step("b.mkv", 1)];
    assert_eq!(
        env.store.insert_steps("transfer:7", &steps).await.unwrap(),
        2
    );
    let rows = env.store.steps("transfer:7").await.unwrap();
    assert_eq!(rows.len(), before + 2);
    let added: Vec<_> = rows
        .iter()
        .filter(|r| r.kind == StepKind::Subtitles)
        .collect();
    assert_eq!(added[0].branch.as_deref(), Some("a.mkv"));
    assert_eq!(added[1].branch.as_deref(), Some("b.mkv"));
    assert!(added[0].id < added[1].id);
    assert_eq!(added[0].state, StepState::Waiting);
    // A second scan adds nothing
    assert_eq!(
        env.store.insert_steps("transfer:7", &steps).await.unwrap(),
        0
    );
    assert_eq!(
        env.store.steps("transfer:7").await.unwrap().len(),
        before + 2
    );
}
