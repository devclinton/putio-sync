//! Feature 038, story 2: the operator's own steps. What a hook is given, how its result is
//! read, retries for optional steps, the pre-download gate, and the operator's retry and
//! skip. Unix only: the hooks are shell scripts.
#![cfg(unix)]

mod common;

use common::*;
use putio_sync_core::events::{self, Payload};
use putio_sync_core::hooks::{HookConfig, HookStage};
use putio_sync_download::pipeline::{Stage, State};
use putio_sync_pipeline::model::{ItemState, SkipReason, StepState};
use putio_sync_pipeline::{ActionError, Pipeline};

async fn state_of(p: &Pipeline, key: &str, step: &str) -> String {
    let t = p.timeline(key).await.unwrap();
    t.stages
        .iter()
        .flat_map(|s| s.steps.iter())
        .find(|s| s.name == step)
        .unwrap()
        .state
        .to_string()
}

#[tokio::test]
async fn a_command_gets_the_item_in_its_environment_and_as_json() {
    let env = env_with(|dir| {
        let out = dir.join("seen");
        std::fs::create_dir_all(dir.join("item/Show.S01E01")).unwrap();
        std::fs::write(dir.join("item/Show.S01E01/a.mkv"), b"12345").unwrap();
        let body = format!(
            "{{ echo \"$PUTIO_SYNC_ITEM_KEY|$PUTIO_SYNC_ITEM_NAME|$PUTIO_SYNC_ITEM_CATEGORY|$PUTIO_SYNC_ITEM_SOURCE|$PUTIO_SYNC_ITEM_DIR|$PUTIO_SYNC_ITEM_STAGE\"; }} > {o}; cp \"$PUTIO_SYNC_ITEM_JSON\" {o}.json; echo \"$PUTIO_SYNC_ITEM_JSON\" > {o}.path",
            o = out.display()
        );
        vec![hook("tidy", HookStage::PostProcessing, script(dir, "tidy", &body))]
    })
    .await;
    let item_dir = env.dir.path().join("item/Show.S01E01");
    downloaded_item(&env, "transfer:7", &item_dir).await;
    env.pipeline.run_due().await;

    let line = std::fs::read_to_string(env.dir.path().join("seen")).unwrap();
    assert_eq!(
        line.trim(),
        format!(
            "transfer:7|name-of-transfer:7|sonarr|arr|{}|post_processing",
            item_dir.display()
        )
    );
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(env.dir.path().join("seen.json")).unwrap())
            .unwrap();
    assert_eq!(json["key"], "transfer:7");
    assert_eq!(json["category"], "sonarr");
    assert_eq!(json["source"], "arr");
    assert_eq!(json["dir"], item_dir.to_string_lossy().as_ref());
    assert_eq!(json["transfer_id"], 7);
    assert_eq!(json["files"][0]["relative_path"], "a.mkv");
    assert_eq!(json["files"][0]["size"], 5);
    assert!(json["files"][0].get("putio_id").is_some());
    let path = std::fs::read_to_string(env.dir.path().join("seen.path")).unwrap();
    assert!(
        !std::path::Path::new(path.trim()).exists(),
        "the JSON file is removed after the run"
    );
}

#[tokio::test]
async fn exit_zero_is_done_exit_three_is_failed_with_output_and_a_timeout_is_failed() {
    let env = env_with(|dir| {
        vec![
            hook(
                "ok",
                HookStage::PreProcessing,
                script(dir, "ok", "echo fine"),
            ),
            hook(
                "bad",
                HookStage::PostProcessing,
                script(dir, "bad", "echo oops; echo more >&2; exit 3"),
            ),
        ]
    })
    .await;
    downloaded_item(&env, "transfer:1", env.dir.path()).await;
    env.pipeline.run_due().await;
    assert_eq!(state_of(&env.pipeline, "transfer:1", "ok").await, "done");
    assert_eq!(state_of(&env.pipeline, "transfer:1", "bad").await, "failed");
    let t = env.pipeline.timeline("transfer:1").await.unwrap();
    assert_eq!(t.state, ItemState::WaitingForOperator);
    let bad = &t.stages[5].steps[0];
    assert_eq!(bad.attempts.len(), 1);
    assert_eq!(bad.attempts[0].result, Some("failed"));
    assert_eq!(bad.attempts[0].exit_code, Some(3));
    assert!(bad.attempts[0].output.contains("oops") && bad.attempts[0].output.contains("more"));

    let env = env_with(|dir| {
        let mut h = hook(
            "slow",
            HookStage::PostProcessing,
            script(dir, "slow", "sleep 20"),
        );
        h.timeout_secs = 1;
        vec![h]
    })
    .await;
    downloaded_item(&env, "transfer:2", env.dir.path()).await;
    env.pipeline.run_due().await;
    let t = env.pipeline.timeline("transfer:2").await.unwrap();
    assert_eq!(t.stages[5].steps[0].state, "failed");
    assert_eq!(t.stages[5].steps[0].attempts[0].result, Some("timeout"));
}

#[tokio::test]
async fn output_never_keeps_a_secret() {
    let env = env_with(|dir| {
        let mut h = hook(
            "leaky",
            HookStage::PostProcessing,
            script(dir, "leaky", "echo \"token=$MY_TOKEN\"; exit 1"),
        );
        h.env
            .insert("MY_TOKEN".to_string(), "s3cr3t-value-123".to_string());
        vec![h]
    })
    .await;
    putio_sync_core::log_redact::set_secrets(vec!["s3cr3t-value-123".to_string()]);
    downloaded_item(&env, "transfer:1", env.dir.path()).await;
    env.pipeline.run_due().await;
    let t = env.pipeline.timeline("transfer:1").await.unwrap();
    assert!(!t.stages[5].steps[0].attempts[0]
        .output
        .contains("s3cr3t-value-123"));
    putio_sync_core::log_redact::set_secrets(vec![]);
}

#[tokio::test]
async fn exit_ten_counts_as_done_after_download() {
    let env = env_with(|dir| {
        vec![hook(
            "late",
            HookStage::PostProcessing,
            script(dir, "late", "exit 10"),
        )]
    })
    .await;
    // Only before download does 10 skip the item (see the pre-download skip test)
    downloaded_item(&env, "transfer:1", env.dir.path()).await;
    env.pipeline.run_due().await;
    assert_eq!(state_of(&env.pipeline, "transfer:1", "late").await, "done");
    assert_eq!(
        env.pipeline.timeline("transfer:1").await.unwrap().state,
        ItemState::Done
    );
}

#[tokio::test]
async fn a_pre_download_hook_holds_the_transfer_until_it_passes() {
    let env = env_with(|dir| {
        // Fails until a marker exists, so the test can look at the hold in between
        let body = format!("[ -f {}/go ]", dir.display());
        vec![hook(
            "gate",
            HookStage::PreDownload,
            script(dir, "gate", &body),
        )]
    })
    .await;
    let list = view(vec![active(
        "transfer:5",
        Some(5),
        Stage::Putio,
        State::Downloading,
    )]);
    env.pipeline.on_tick(&list).await;
    assert!(
        env.db.holds().is_held(5).await.unwrap(),
        "held before any file starts"
    );
    assert_eq!(
        env.pipeline.timeline("transfer:5").await.unwrap().stages[0].steps[0].state,
        "waiting"
    );

    env.pipeline.run_due().await;
    assert_eq!(
        state_of(&env.pipeline, "transfer:5", "gate").await,
        "failed"
    );
    assert!(
        env.db.holds().is_held(5).await.unwrap(),
        "still held while the hook fails"
    );

    std::fs::write(env.dir.path().join("go"), b"").unwrap();
    let steps = env.store.steps("transfer:5").await.unwrap();
    env.pipeline.retry("transfer:5", steps[0].id).await.unwrap();
    env.pipeline.run_due().await;
    assert!(
        !env.db.holds().is_held(5).await.unwrap(),
        "released when the hook passes"
    );
    let t = env.pipeline.timeline("transfer:5").await.unwrap();
    assert_eq!(t.stages[0].steps[0].state, "done");
    assert_eq!(t.state, ItemState::Running);
}

#[tokio::test]
async fn a_pre_download_skip_ends_the_item_and_removes_its_queued_files() {
    let env = env_with(|dir| {
        vec![hook(
            "gate",
            HookStage::PreDownload,
            script(dir, "gate", "exit 10"),
        )]
    })
    .await;
    // A file of the transfer was queued paused by the hold
    env.db
        .queue()
        .add(
            55,
            "a.mkv",
            10,
            "",
            "",
            0,
            putio_sync_db::models::QueueStatus::Pending,
        )
        .await
        .unwrap();
    env.db
        .sources()
        .record(
            55,
            &putio_sync_db::models::DownloadSource {
                transfer_id: Some(5),
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
    let list = view(vec![active(
        "transfer:5",
        Some(5),
        Stage::Putio,
        State::Downloading,
    )]);
    env.pipeline.on_tick(&list).await;
    env.pipeline.run_due().await;
    let t = env.pipeline.timeline("transfer:5").await.unwrap();
    assert_eq!(t.state, ItemState::Skipped);
    assert_eq!(t.stages[0].steps[0].state, "skipped");
    assert_eq!(t.stages[0].steps[0].skip_reason, Some("hook"));
    assert!(
        env.db.holds().is_held(5).await.unwrap(),
        "the hold stays so the sync does not queue it again"
    );
}

#[tokio::test]
async fn a_required_failure_waits_for_the_operator_who_can_retry_or_skip() {
    let env = env_with(|dir| {
        let body = format!("[ -f {}/fixed ]", dir.display());
        vec![
            hook("a", HookStage::PostProcessing, script(dir, "a", &body)),
            hook("b", HookStage::PostProcessing, script(dir, "b", "exit 0")),
        ]
    })
    .await;
    downloaded_item(&env, "transfer:61", env.dir.path()).await;
    let mut rx = events::subscribe();
    env.pipeline.run_due().await;
    let t = env.pipeline.timeline("transfer:61").await.unwrap();
    assert_eq!(t.state, ItemState::WaitingForOperator);
    assert_eq!(
        t.stages[5].steps[1].state, "waiting",
        "b has not run: a comes first"
    );
    let id_a = t.stages[5].steps[0].id;

    // pipeline.step_failed went out with the step and attempt
    let mut seen = None;
    while let Ok(event) = rx.try_recv() {
        if let Payload::PipelineStepFailed(p) = &event.payload {
            if p.key != "transfer:61" {
                continue;
            }
            seen = Some(p.clone());
        }
    }
    let failed = seen.expect("pipeline.step_failed");
    assert_eq!(
        (
            failed.key.as_str(),
            failed.step.as_str(),
            failed.attempt,
            failed.required
        ),
        ("transfer:61", "a", 1, true)
    );

    // A step that did not fail cannot be retried or skipped
    let id_b = t.stages[5].steps[1].id;
    assert!(matches!(
        env.pipeline.retry("transfer:61", id_b).await,
        Err(ActionError::Conflict(_))
    ));
    assert!(matches!(
        env.pipeline.skip("transfer:61", id_b).await,
        Err(ActionError::Conflict(_))
    ));
    assert_eq!(
        env.pipeline.retry("transfer:61", 9999).await,
        Err(ActionError::NotFound)
    );
    assert_eq!(
        env.pipeline.retry("transfer:nope", id_a).await,
        Err(ActionError::NotFound)
    );

    // Skip: the item goes on and records who skipped it
    env.pipeline.skip("transfer:61", id_a).await.unwrap();
    env.pipeline.run_due().await;
    let t = env.pipeline.timeline("transfer:61").await.unwrap();
    assert_eq!(t.state, ItemState::Done);
    assert_eq!(t.stages[5].steps[0].state, "skipped");
    assert_eq!(t.stages[5].steps[0].skip_reason, Some("operator"));
    assert_eq!(t.stages[5].steps[1].state, "done");
    let _ = SkipReason::Operator;
    let _ = StepState::Done;
}

#[tokio::test]
async fn an_optional_failure_is_retried_then_given_up_and_the_item_finishes_with_warnings() {
    let env = env_with(|dir| {
        let mut h = hook(
            "meh",
            HookStage::PostProcessing,
            script(
                dir,
                "meh",
                &format!("echo x >> {}/runs; exit 1", dir.display()),
            ),
        );
        h.required = false;
        vec![h]
    })
    .await;
    downloaded_item(&env, "transfer:62", env.dir.path()).await;
    let mut rx = events::subscribe();
    // optional_retries is [0, 0]: three attempts in all
    env.pipeline.run_due().await;
    let t = env.pipeline.timeline("transfer:62").await.unwrap();
    assert_eq!(t.state, ItemState::DoneWithWarnings);
    assert_eq!(t.stages[5].steps[0].attempts.len(), 3);
    assert_eq!(
        std::fs::read_to_string(env.dir.path().join("runs"))
            .unwrap()
            .lines()
            .count(),
        3
    );
    let mut completed = None;
    let mut step_failed = 0;
    while let Ok(event) = rx.try_recv() {
        match &event.payload {
            Payload::PipelineCompleted(p) if p.key == "transfer:62" => completed = Some(p.clone()),
            Payload::PipelineStepFailed(p) if p.key == "transfer:62" => step_failed += 1,
            _ => {}
        }
    }
    assert_eq!(step_failed, 3);
    assert_eq!(completed.unwrap().warnings, ["meh"]);
}

#[tokio::test]
async fn an_event_hook_is_never_run_as_a_step_and_a_step_hook_never_matches_an_event() {
    let env = env_with(|dir| {
        let mut event_hook = HookConfig {
            name: "ev".to_string(),
            events: vec!["*".to_string()],
            command: Some(script(
                dir,
                "ev",
                &format!("touch {}/ev-ran", dir.display()),
            )),
            ..Default::default()
        };
        event_hook.stage = None;
        vec![event_hook]
    })
    .await;
    downloaded_item(&env, "transfer:1", env.dir.path()).await;
    env.pipeline.run_due().await;
    assert!(!env.dir.path().join("ev-ran").exists());
    let steps = env.store.steps("transfer:1").await.unwrap();
    assert!(steps.iter().all(|s| s.name != "ev"));

    let stage_hook = hook(
        "s",
        HookStage::PostProcessing,
        vec!["/bin/true".to_string()],
    );
    assert!(!stage_hook.matches(putio_sync_core::events::EventType::DownloadFinished));
}

#[tokio::test]
async fn a_hook_with_a_category_filter_skips_other_categories() {
    let env = env_with(|dir| {
        let mut h = hook(
            "movies-only",
            HookStage::PostProcessing,
            script(dir, "m", &format!("touch {}/ran", dir.display())),
        );
        h.categories = vec!["radarr".to_string()];
        vec![h]
    })
    .await;
    downloaded_item(&env, "transfer:1", env.dir.path()).await; // category: sonarr
    env.pipeline.run_due().await;
    assert!(!env.dir.path().join("ran").exists());
    let t = env.pipeline.timeline("transfer:1").await.unwrap();
    assert_eq!(t.stages[5].steps[0].state, "skipped");
    assert_eq!(t.stages[5].steps[0].skip_reason, Some("category"));
}

#[tokio::test]
async fn a_url_hook_reads_2xx_as_done_a_skip_body_as_skip_and_500_as_failed() {
    let mut server = mockito::Server::new_async().await;
    let ok = server
        .mock("POST", "/ok")
        .with_status(200)
        .create_async()
        .await;
    let skip = server
        .mock("POST", "/skip")
        .with_status(200)
        .with_body(r#"{"action":"skip"}"#)
        .create_async()
        .await;
    let boom = server
        .mock("POST", "/boom")
        .with_status(500)
        .with_body("kaput")
        .create_async()
        .await;
    let url = server.url();
    let env = env_with(|_| {
        let mk = |name: &str, stage| HookConfig {
            name: name.to_string(),
            stage: Some(stage),
            url: Some(format!("{url}/{name}")),
            timeout_secs: 5,
            ..Default::default()
        };
        vec![
            mk("ok", HookStage::PreProcessing),
            mk("boom", HookStage::PostProcessing),
        ]
    })
    .await;
    downloaded_item(&env, "transfer:1", env.dir.path()).await;
    env.pipeline.run_due().await;
    ok.assert_async().await;
    boom.assert_async().await;
    assert_eq!(state_of(&env.pipeline, "transfer:1", "ok").await, "done");
    let t = env.pipeline.timeline("transfer:1").await.unwrap();
    assert_eq!(t.stages[5].steps[0].state, "failed");
    assert_eq!(t.stages[5].steps[0].attempts[0].exit_code, Some(500));
    assert!(t.stages[5].steps[0].attempts[0].output.contains("kaput"));

    // A skip body before download ends the item
    let env = env_with(|_| {
        vec![HookConfig {
            name: "skip".to_string(),
            stage: Some(HookStage::PreDownload),
            url: Some(format!("{url}/skip")),
            timeout_secs: 5,
            ..Default::default()
        }]
    })
    .await;
    env.pipeline
        .on_tick(&view(vec![active(
            "transfer:3",
            Some(3),
            Stage::Putio,
            State::Downloading,
        )]))
        .await;
    env.pipeline.run_due().await;
    skip.assert_async().await;
    assert_eq!(
        env.pipeline.timeline("transfer:3").await.unwrap().state,
        ItemState::Skipped
    );
}
