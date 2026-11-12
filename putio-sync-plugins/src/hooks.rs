//! Sync extension points (feature 028, research R2–R5): the answers to the sync's
//! questions ([`SyncHooks`]) from the plugins that declare `hooks` in their manifest.
//!
//! Each such plugin gets a second worker, built from the same spec and sharing its store,
//! so a sync question never waits behind the plugin's queued events. A call has the
//! plugin's `hook_timeout_ms`. An error, a timeout or an answer that does not check out
//! passes the plugin over; after [`MAX_FAILURES`] in a row its hooks rest for [`PAUSE`].

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use putio_sync_core::sync_hooks::{self, FilterAnswer, Hook, HookFile, HookFuture, SyncHooks};
use serde_json::Value;
use tokio::sync::oneshot;

use crate::host::scrub;
use crate::service::PluginsService;
use crate::worker::{now_text, Job, PluginRun, RunResult, Slot};

/// Failures in a row that pause a plugin's hooks
pub const MAX_FAILURES: u32 = 3;
/// How long a failing plugin's hooks rest
pub const PAUSE: Duration = Duration::from_secs(300);
/// Extra wait on top of the limit, for a call queued behind another one
pub const GRACE: Duration = Duration::from_millis(250);

#[derive(Default)]
struct Failures {
    in_a_row: u32,
    paused_until: Option<Instant>,
}

/// A running plugin's hook worker
pub(crate) struct HookSlot {
    pub(crate) slot: Arc<Slot>,
    hooks: Vec<Hook>,
    limit: Duration,
    secrets: Vec<String>,
    failures: Mutex<Failures>,
}

impl HookSlot {
    /// A hook worker's state for `slot`'s spec; failures start at 0
    pub(crate) fn new(slot: Arc<Slot>) -> HookSlot {
        let spec = slot.spec();
        HookSlot {
            hooks: Hook::ALL
                .into_iter()
                .filter(|h| spec.manifest.has_hook(*h))
                .collect(),
            limit: spec.settings.hook_timeout(),
            secrets: spec.settings.secrets.values().cloned().collect(),
            failures: Mutex::default(),
            slot,
        }
    }

    fn name(&self) -> &str {
        &self.slot.name
    }

    /// Whether to ask it about `hook`: until its state is built the built-in rule
    /// applies, rather than a call that waits for the plugin to load
    fn ready(&self, hook: Hook, now: Instant) -> bool {
        self.hooks.contains(&hook)
            && self.slot.loaded()
            && self
                .failures
                .lock()
                .unwrap()
                .paused_until
                .is_none_or(|until| now >= until)
    }

    fn succeeded(&self) {
        let mut f = self.failures.lock().unwrap();
        f.in_a_row = 0;
        f.paused_until = None;
    }

    /// Count a failure; true when it starts a pause
    fn failed(&self) -> bool {
        let mut f = self.failures.lock().unwrap();
        f.in_a_row += 1;
        if f.in_a_row >= MAX_FAILURES {
            f.in_a_row = 0;
            f.paused_until = Some(Instant::now() + PAUSE);
            return true;
        }
        false
    }

    /// One call: the plugin's JSON answer, or why it failed and whether the worker
    /// already kept that run
    async fn call(&self, hook: Hook, file: &Value) -> Result<Value, (String, bool)> {
        let (reply, answer) = oneshot::channel();
        self.slot.push(Job::Call {
            function: hook.as_str().to_string(),
            kind: hook.as_str().to_string(),
            args: vec![file.clone()],
            limit: Some(self.limit),
            reply,
        });
        match tokio::time::timeout(self.limit + GRACE, answer).await {
            Err(_) => Err((
                format!("no answer within {} ms", self.limit.as_millis()),
                false,
            )),
            Ok(Err(_)) => Err(("the plugin stopped during the call".to_string(), false)),
            Ok(Ok(reply)) if reply.run.result != RunResult::Success => Err((
                reply
                    .run
                    .error
                    .unwrap_or_else(|| reply.run.result.as_str().to_string()),
                true,
            )),
            Ok(Ok(reply)) => Ok(reply.value.unwrap_or(Value::Null)),
        }
    }
}

impl PluginsService {
    /// The first valid answer to `hook` about `file`, from the plugins in name order, and
    /// the plugin that gave it
    async fn ask<T>(
        &self,
        hook: Hook,
        file: &HookFile,
        parse: fn(&Value) -> Result<Option<T>, String>,
    ) -> Option<(T, String)> {
        let slots = self.hook_slots(hook);
        if slots.is_empty() {
            return None;
        }
        let value = serde_json::to_value(file).ok()?;
        for slot in slots {
            let answer = slot
                .call(hook, &value)
                .await
                .and_then(|v| parse(&v).map_err(|e| (scrub(&e, &slot.secrets), false)));
            match answer {
                Ok(Some(t)) => {
                    slot.succeeded();
                    return Some((t, slot.name().to_string()));
                }
                Ok(None) => slot.succeeded(),
                Err((error, kept)) => {
                    tracing::warn!(
                        plugin = %slot.name(),
                        hook = hook.as_str(),
                        file_id = file.id,
                        error = %error,
                        "Sync hook failed; the built-in rule applies"
                    );
                    if !kept {
                        self.keep_run(slot.name(), failed_run(hook, &error));
                    }
                    if slot.failed() {
                        tracing::warn!(
                            plugin = %slot.name(),
                            failures = MAX_FAILURES,
                            pause_secs = PAUSE.as_secs(),
                            "Sync hooks paused after failures in a row"
                        );
                    }
                }
            }
        }
        None
    }

    /// The hook workers that answer `hook` now, in name order
    fn hook_slots(&self, hook: Hook) -> Vec<Arc<HookSlot>> {
        let now = Instant::now();
        let mut slots: Vec<Arc<HookSlot>> = self
            .hook_workers()
            .into_iter()
            .filter(|s| s.ready(hook, now))
            .collect();
        slots.sort_by(|a, b| a.name().cmp(b.name()));
        slots
    }
}

/// A failed hook run the worker did not see: a late reply or an answer refused here
fn failed_run(hook: Hook, error: &str) -> PluginRun {
    PluginRun {
        event_type: hook.as_str().to_string(),
        event_id: String::new(),
        started_at: now_text(),
        duration_ms: 0,
        result: RunResult::Failure,
        error: Some(error.to_string()),
        logs: Vec::new(),
    }
}

impl SyncHooks for PluginsService {
    fn provides(&self, hook: Hook) -> bool {
        !self.hook_slots(hook).is_empty()
    }

    fn filter<'a>(&'a self, file: &'a HookFile) -> HookFuture<'a, Option<(FilterAnswer, String)>> {
        Box::pin(self.ask(Hook::Filter, file, sync_hooks::parse_filter))
    }

    fn rename<'a>(&'a self, file: &'a HookFile) -> HookFuture<'a, Option<String>> {
        Box::pin(async move {
            self.ask(Hook::Rename, file, sync_hooks::parse_name)
                .await
                .map(|(name, _)| name)
        })
    }

    fn folder<'a>(&'a self, file: &'a HookFile) -> HookFuture<'a, Option<String>> {
        Box::pin(async move {
            self.ask(Hook::Folder, file, sync_hooks::parse_folder)
                .await
                .map(|(folder, _)| folder)
        })
    }
}
