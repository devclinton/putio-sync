//! Plugins as pipeline steps (feature 057): [`StepPlugins`] for the plugins service.
//!
//! A step call is one more job on the plugin's own queue, behind its events, and runs
//! under the plugin's own time and memory limits. So a slow step plugin holds up its own
//! events and steps, and nobody else's.

use putio_sync_core::step_plugins::{parse_answer, StepAnswer, StepPlugins};
use putio_sync_core::sync_hooks::HookFuture;
use serde_json::Value;
use tokio::sync::oneshot;

use crate::service::PluginsService;
use crate::worker::{Job, RunResult};

impl StepPlugins for PluginsService {
    fn run_item<'a>(
        &'a self,
        plugin: &'a str,
        item: &'a Value,
        stage: &'a str,
    ) -> HookFuture<'a, StepAnswer> {
        Box::pin(async move {
            let Some(slot) = self.step_slot(plugin) else {
                return StepAnswer::Failed(
                    self.step_problem(plugin)
                        .unwrap_or_else(|| format!("the plugin \"{plugin}\" is not running")),
                );
            };
            let (reply, answer) = oneshot::channel();
            slot.push(Job::Call {
                function: "on_item".to_string(),
                kind: "item".to_string(),
                args: vec![item.clone(), Value::String(stage.to_string())],
                limit: None,
                reply,
            });
            let Ok(reply) = answer.await else {
                return StepAnswer::Failed(format!(
                    "the plugin \"{plugin}\" stopped before the step ran"
                ));
            };
            if !reply.defined {
                return StepAnswer::Failed(format!("the plugin \"{plugin}\" has no on_item"));
            }
            let why = |fallback: &str| {
                format!(
                    "plugin \"{plugin}\": {}",
                    reply.run.error.as_deref().unwrap_or(fallback)
                )
            };
            match reply.run.result {
                RunResult::Success => match parse_answer(&reply.value.unwrap_or(Value::Null)) {
                    StepAnswer::Failed(e) => {
                        StepAnswer::Failed(format!("plugin \"{plugin}\": {e}"))
                    }
                    other => other,
                },
                RunResult::Timeout => StepAnswer::Timeout(why("ran past its time limit")),
                RunResult::Failure => StepAnswer::Failed(why("on_item failed")),
            }
        })
    }

    fn problem(&self, plugin: &str) -> Option<String> {
        self.step_problem(plugin)
    }
}
