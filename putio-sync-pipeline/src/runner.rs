//! The worker (feature 038): takes the items that have work and runs their due steps.
//!
//! One task advances one item at a time, under the concurrency limit. A stage's steps run
//! in order inside a branch, and different branches of one stage run together. A required
//! step that fails stops the item until the operator retries or skips it; an optional one
//! is retried after the configured delays and then given up, and the item goes on with a
//! warning.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use putio_sync_core::events::{self, Payload, PipelineCompleted, PipelineStepFailed};
use putio_sync_core::log_redact;
use putio_sync_hooks::step::{self, ItemPayload, StepResult};
use tokio::sync::Notify;
use tokio::task::{JoinHandle, JoinSet};
use tracing::{info, warn};

use crate::locate;
use crate::model::{AttemptResult, ItemState, SkipReason, Stage, StepKind, StepState};
use crate::store::{ItemRow, StepRow};
use crate::Pipeline;

/// How one attempt at a step ended
pub(crate) struct Ended {
    pub attempt_id: i64,
    pub n: i64,
    /// The metrics `kind` label
    pub kind: &'static str,
    pub result: AttemptResult,
    pub code: Option<i32>,
    pub output: String,
    pub error: Option<String>,
    pub took: std::time::Duration,
}

/// What running one step meant for its item
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// The item goes on, or waits for a retry time
    Continue,
    /// The item ended or stopped: skipped, cancelled, or waiting for the operator
    Stopped,
}

impl Pipeline {
    /// Start a task for each running item that is past downloading, unless one is working
    /// on it already
    pub(crate) async fn spawn_due(&self) -> Vec<JoinHandle<()>> {
        let Ok(items) = self.inner.store.items().await else {
            return Vec::new();
        };
        let mut handles = Vec::new();
        for item in items
            .into_iter()
            .filter(|i| i.state == ItemState::Running && i.stage != Stage::Downloading)
        {
            if !self.inner.busy.lock().unwrap().insert(item.key.clone()) {
                continue;
            }
            let me = self.clone();
            let permits = self.inner.permits.read().unwrap().clone();
            handles.push(tokio::spawn(async move {
                if let Ok(_permit) = permits.acquire_owned().await {
                    me.advance(&item.key).await;
                }
                me.inner.busy.lock().unwrap().remove(&item.key);
            }));
        }
        handles
    }

    /// Run what can run for `key`, stage by stage, until it waits, stops or finishes
    async fn advance(&self, key: &str) {
        let store = &self.inner.store;
        // The gate must know the item has steps due before the first one starts
        self.refresh_gate().await;
        loop {
            let Ok(Some(item)) = store.item(key).await else {
                return;
            };
            if item.state != ItemState::Running {
                return;
            }
            let Ok(steps) = store.steps(key).await else {
                return;
            };
            let passed =
                |s: &StepRow| s.state.is_settled() || (s.state == StepState::Failed && !s.required);
            let Some(stage) = steps.iter().find(|s| !passed(s)).map(|s| s.stage) else {
                self.finalize(&item, &steps).await;
                return;
            };
            if stage == Stage::Downloading {
                // The pre-download steps are done: let the transfer go
                if item.stage == Stage::PreDownload {
                    if let Some(id) = item.transfer_id {
                        let _ = self.inner.db.holds().remove(id).await;
                    }
                    let _ = store
                        .set_item(key, Stage::Downloading, ItemState::Running)
                        .await;
                }
                return;
            }
            if item.stage != stage {
                let _ = store.set_item(key, stage, ItemState::Running).await;
            }

            // The next step of each branch in this stage
            let mut lines: BTreeMap<Option<&str>, Vec<&StepRow>> = BTreeMap::new();
            for s in steps.iter().filter(|s| s.stage == stage && !passed(s)) {
                lines.entry(s.branch.as_deref()).or_default().push(s);
            }
            let due_ids: std::collections::HashSet<i64> = store
                .due_steps()
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|s| s.id)
                .collect();
            let next: Vec<StepRow> = lines
                .into_values()
                .filter_map(|mut line| {
                    line.sort_by_key(|s| (s.position, s.id));
                    line.first().map(|s| (*s).clone())
                })
                .filter(|s| due_ids.contains(&s.id))
                .collect();
            if next.is_empty() {
                return;
            }

            let mut set = JoinSet::new();
            for step in next {
                let me = self.clone();
                let item = item.clone();
                set.spawn(async move { me.run_step(&item, &step).await });
            }
            let mut stopped = false;
            while let Some(done) = set.join_next().await {
                if done.unwrap_or(Outcome::Stopped) == Outcome::Stopped {
                    stopped = true;
                }
            }
            self.refresh_gate().await;
            if stopped {
                // Let the loop's state check decide whether the item goes on
                continue;
            }
        }
    }

    /// Every step passed: the item is done, with warnings if an optional step was given up
    async fn finalize(&self, item: &ItemRow, steps: &[StepRow]) {
        if let Err(e) = self.finish_staging(item).await {
            warn!(key = %item.key, error = %e, "Could not move the staged item into place");
            self.inner
                .store
                .set_item(&item.key, item.stage, ItemState::WaitingForOperator)
                .await
                .ok();
            return;
        }
        let warnings: Vec<String> = steps
            .iter()
            .filter(|s| {
                s.state == StepState::Failed
                    && matches!(
                        s.kind,
                        StepKind::Hook
                            | StepKind::SubtitleScan
                            | StepKind::Subtitles
                            | StepKind::TranslateScan
                            | StepKind::Translate
                    )
            })
            .map(|s| s.name.clone())
            .collect();
        let state = if warnings.is_empty() {
            ItemState::Done
        } else {
            ItemState::DoneWithWarnings
        };
        let _ = self
            .inner
            .store
            .set_item(&item.key, Stage::Notifications, state)
            .await;
        if let Some(notify) = steps.iter().find(|s| s.kind == StepKind::Notify) {
            let _ = self
                .inner
                .store
                .set_step(notify.id, StepState::Done, None, None)
                .await;
        }
        if !warnings.is_empty() {
            events::emit(Payload::PipelineCompleted(PipelineCompleted {
                key: item.key.clone(),
                name: item.name.clone(),
                warnings,
            }));
        }
        info!(key = %item.key, state = state.as_str(), "Pipeline item finished");
        self.refresh_gate().await;
    }

    /// Run one step, by what kind it is
    async fn run_step(&self, item: &ItemRow, step: &StepRow) -> Outcome {
        match step.kind {
            StepKind::Notify => {
                // Notifications go out through the event hooks; the step records the stage
                let _ = self
                    .inner
                    .store
                    .set_step(step.id, StepState::Done, None, None)
                    .await;
                Outcome::Continue
            }
            StepKind::SubtitleScan => self.run_subtitle_scan(item, step).await,
            StepKind::Subtitles => self.run_subtitles(item, step).await,
            StepKind::TranslateScan => self.run_translate_scan(item, step).await,
            StepKind::Translate => self.run_translate(item, step).await,
            _ => self.run_hook(item, step).await,
        }
    }

    async fn run_hook(&self, item: &ItemRow, step: &StepRow) -> Outcome {
        let store = &self.inner.store;
        let settings = self.settings();
        let Some(hook) = settings
            .hooks
            .iter()
            .find(|h| {
                h.enabled && h.name == step.name && h.stage.map(Stage::from) == Some(step.stage)
            })
            .cloned()
        else {
            let _ = store
                .set_step(
                    step.id,
                    StepState::Skipped,
                    Some(SkipReason::NotConfigured),
                    None,
                )
                .await;
            return Outcome::Continue;
        };
        if !hook.categories.is_empty()
            && !item
                .category
                .as_deref()
                .is_some_and(|c| hook.categories.iter().any(|h| h.eq_ignore_ascii_case(c)))
        {
            let _ = store
                .set_step(
                    step.id,
                    StepState::Skipped,
                    Some(SkipReason::Category),
                    None,
                )
                .await;
            return Outcome::Continue;
        }

        let payload = self.payload(item, step.stage).await;
        let Ok((attempt_id, n)) = store.start_attempt(step.id).await else {
            return Outcome::Stopped;
        };
        let cancel = {
            let mut cancels = self.inner.cancels.lock().unwrap();
            cancels
                .entry(item.key.clone())
                .or_insert_with(|| Arc::new(Notify::new()))
                .clone()
        };
        let started = Instant::now();
        let run = tokio::select! {
            run = step::run_step(&self.inner.client, &hook, &payload) => Some(run),
            _ = cancel.notified() => None,
        };
        let took = started.elapsed();
        let stage = step.stage.as_str();

        let Some(run) = run else {
            let _ = store
                .end_attempt(attempt_id, AttemptResult::Interrupted, None, "")
                .await;
            let _ = store
                .set_step(step.id, StepState::Interrupted, None, None)
                .await;
            putio_sync_telemetry::steps::step(stage, "hook", "interrupted", took);
            return Outcome::Stopped;
        };
        let result = match run.result {
            StepResult::Done => AttemptResult::Done,
            StepResult::Skip => AttemptResult::Skip,
            StepResult::Failed => AttemptResult::Failed,
            StepResult::Timeout => AttemptResult::Timeout,
        };
        let error = run.error.as_deref().map(log_redact::redact);
        let mut output = log_redact::redact(&run.output);
        if let Some(e) = &error {
            if output.is_empty() {
                output = e.clone();
            }
        }
        let code = run.exit_code.or(run.http_status.map(i32::from));
        self.conclude(
            item,
            step,
            Ended {
                attempt_id,
                n,
                kind: "hook",
                result,
                code,
                output,
                error,
                took,
            },
        )
        .await
    }

    /// The cancel signal of an item's running step
    pub(crate) fn cancel_of(&self, key: &str) -> Arc<Notify> {
        self.inner
            .cancels
            .lock()
            .unwrap()
            .entry(key.to_string())
            .or_insert_with(|| Arc::new(Notify::new()))
            .clone()
    }

    /// Record how an attempt ended and decide what the step does next: done, a hook's skip,
    /// or a failure with its retries. Shared by every kind of step.
    pub(crate) async fn conclude(&self, item: &ItemRow, step: &StepRow, e: Ended) -> Outcome {
        let store = &self.inner.store;
        let Ended {
            attempt_id,
            n,
            kind,
            result,
            code,
            output,
            error,
            took,
        } = e;
        let stage = step.stage.as_str();
        let _ = store.end_attempt(attempt_id, result, code, &output).await;
        putio_sync_telemetry::steps::step(stage, kind, result.as_str(), took);
        info!(
            key = %item.key, stage, step = %step.name, attempt = n,
            result = result.as_str(), duration_ms = took.as_millis() as u64,
            "Pipeline step ended"
        );

        match result {
            AttemptResult::Done => {
                let _ = store.set_step(step.id, StepState::Done, None, None).await;
                Outcome::Continue
            }
            AttemptResult::Skip if step.stage == Stage::PreDownload => {
                self.skip_item(item, step).await;
                Outcome::Stopped
            }
            // Exit 10 means skip the item only before download; later it counts as done
            AttemptResult::Skip => {
                let _ = store.set_step(step.id, StepState::Done, None, None).await;
                Outcome::Continue
            }
            _ => {
                self.failed(item, step, n, error.unwrap_or_else(|| "failed".to_string()))
                    .await
            }
        }
    }

    /// A step failed or timed out
    async fn failed(&self, item: &ItemRow, step: &StepRow, n: i64, error: String) -> Outcome {
        let store = &self.inner.store;
        events::emit(Payload::PipelineStepFailed(PipelineStepFailed {
            key: item.key.clone(),
            name: item.name.clone(),
            stage: step.stage.as_str().to_string(),
            step: step.name.clone(),
            attempt: n as u32,
            required: step.required,
            error,
        }));
        if step.required {
            let _ = store.set_step(step.id, StepState::Failed, None, None).await;
            let _ = store
                .set_item(&item.key, step.stage, ItemState::WaitingForOperator)
                .await;
            self.refresh_gate().await;
            return Outcome::Stopped;
        }
        let retries = self.settings().pipeline.optional_retries;
        match retries.get((n - 1) as usize) {
            Some(delay) => {
                let _ = store
                    .set_step(step.id, StepState::Waiting, None, Some(*delay))
                    .await;
            }
            None => {
                let _ = store.set_step(step.id, StepState::Failed, None, None).await;
            }
        }
        Outcome::Continue
    }

    /// A pre-download hook asked to skip the item: its later steps are skipped, its queued
    /// files are removed, and the hold stays so the sync does not queue them again
    async fn skip_item(&self, item: &ItemRow, step: &StepRow) {
        let store = &self.inner.store;
        let _ = store
            .set_step(step.id, StepState::Skipped, Some(SkipReason::Hook), None)
            .await;
        if let Ok(steps) = store.steps(&item.key).await {
            for s in steps.iter().filter(|s| s.state == StepState::Waiting) {
                let _ = store
                    .set_step(s.id, StepState::Skipped, Some(SkipReason::Hook), None)
                    .await;
            }
        }
        if let Some(id) = item.transfer_id {
            if let Ok(queued) = self.inner.db.queue().get_for_transfer(id).await {
                for q in queued {
                    let _ = self.inner.db.queue().remove(q.file_id).await;
                }
            }
        }
        let _ = store
            .set_item(&item.key, Stage::PreDownload, ItemState::Skipped)
            .await;
        info!(key = %item.key, "Pipeline item skipped by a pre-download hook");
    }

    /// The item as a hook gets it (research R3)
    async fn payload(&self, item: &ItemRow, stage: Stage) -> ItemPayload {
        let settings = self.settings();
        let (files, dir) = if item.dir.is_empty() {
            (Vec::new(), String::new())
        } else {
            let dir = std::path::PathBuf::from(&item.dir);
            let ids =
                locate::local_paths(&self.inner.db, &item.key, settings.retention_hours).await;
            let files = tokio::task::spawn_blocking({
                let dir = dir.clone();
                move || locate::list_files(&dir, &ids)
            })
            .await
            .unwrap_or_default();
            (files, item.dir.clone())
        };
        let json = serde_json::json!({
            "key": item.key,
            "name": item.name,
            "category": item.category,
            "source": item.source,
            "dir": dir,
            "stage": stage.as_str(),
            "files": files,
            "transfer_id": item.transfer_id,
        })
        .to_string();
        ItemPayload {
            key: item.key.clone(),
            name: item.name.clone(),
            category: item.category.clone(),
            source: item.source.clone(),
            dir,
            stage: stage.as_str().to_string(),
            json,
        }
    }
}
