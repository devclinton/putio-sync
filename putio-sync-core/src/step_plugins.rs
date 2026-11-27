//! Plugins as pipeline steps (feature 057)
//!
//! A `[[hooks]]` entry with a `stage` and a `plugin` runs that plugin's `on_item(item,
//! stage)` as the step, in place of a command or a URL. The pipeline does not know the
//! plugins crate: it asks through [`StepPlugins`], which the plugins service implements
//! and the daemon hands over.

use serde_json::Value;

use crate::sync_hooks::HookFuture;

/// How a plugin's `on_item` ended
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepAnswer {
    /// It returned nothing, `true` or `"done"`
    Done,
    /// It returned `"skip"`: skip the item (`pre_download` only; later it counts as done)
    Skip,
    /// It raised an error, returned `{ fail = "…" }`, or could not be called
    Failed(String),
    /// It ran past the plugin's time limit
    Timeout(String),
}

/// What runs a plugin step
pub trait StepPlugins: Send + Sync {
    /// Call `plugin`'s `on_item` with the item (the JSON a command step gets) and the
    /// stage name, on the plugin's own queue and under its own limits. A plugin that is
    /// missing, off, broken or has no `on_item` answers [`StepAnswer::Failed`] with a
    /// message that names it.
    fn run_item<'a>(
        &'a self,
        plugin: &'a str,
        item: &'a Value,
        stage: &'a str,
    ) -> HookFuture<'a, StepAnswer>;

    /// Why a step that names `plugin` cannot run now: it is not installed, off, broken or
    /// has no `on_item`. `None` when it can, or when that is not known yet.
    fn problem(&self, plugin: &str) -> Option<String>;
}

/// Read what `on_item` returned. Nothing, `true` and `"done"` pass; `"skip"` skips;
/// `{ fail = "why" }` (or `error`) fails with that message; anything else is refused.
pub fn parse_answer(value: &Value) -> StepAnswer {
    match value {
        Value::Null | Value::Bool(true) => StepAnswer::Done,
        Value::String(s) if s == "done" => StepAnswer::Done,
        Value::String(s) if s == "skip" => StepAnswer::Skip,
        Value::Object(map) => {
            let why = ["fail", "error"]
                .iter()
                .find_map(|key| map.get(*key).and_then(Value::as_str));
            match why {
                Some(why) if !why.trim().is_empty() => StepAnswer::Failed(why.to_string()),
                _ => StepAnswer::Failed(
                    "on_item returned a table without a fail message".to_string(),
                ),
            }
        }
        Value::Bool(false) => StepAnswer::Failed("on_item returned false".to_string()),
        other => StepAnswer::Failed(format!(
            "on_item returned {}; return nothing, \"done\", \"skip\" or {{ fail = \"why\" }}",
            other
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn answers_are_read() {
        assert_eq!(parse_answer(&Value::Null), StepAnswer::Done);
        assert_eq!(parse_answer(&json!(true)), StepAnswer::Done);
        assert_eq!(parse_answer(&json!("done")), StepAnswer::Done);
        assert_eq!(parse_answer(&json!("skip")), StepAnswer::Skip);
        assert_eq!(
            parse_answer(&json!({ "fail": "too small" })),
            StepAnswer::Failed("too small".to_string())
        );
        assert_eq!(
            parse_answer(&json!({ "error": "no room" })),
            StepAnswer::Failed("no room".to_string())
        );
        assert!(matches!(parse_answer(&json!(false)), StepAnswer::Failed(_)));
        assert!(matches!(parse_answer(&json!({})), StepAnswer::Failed(_)));
        assert!(
            matches!(parse_answer(&json!("maybe")), StepAnswer::Failed(m) if m.contains("maybe"))
        );
        assert!(matches!(parse_answer(&json!(3)), StepAnswer::Failed(_)));
    }
}
