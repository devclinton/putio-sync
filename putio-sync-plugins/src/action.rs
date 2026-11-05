//! What `plugin.on_action` and `plugin.health` return (feature 031, data-model.md),
//! checked and with the plugin's secrets replaced before anything leaves the host.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::host::scrub;
use crate::manifest::Manifest;

/// Longest message
pub const MAX_MESSAGE_CHARS: usize = 500;
/// Longest code
pub const MAX_CODE_CHARS: usize = 32;
/// Most choices
pub const MAX_CHOICES: usize = 200;
/// Most health items
pub const MAX_HEALTH_ITEMS: usize = 8;
/// Longest wait before a next step, in seconds
pub const MAX_AFTER_SECONDS: u64 = 30;
/// The wait before a next step when none is given
pub const DEFAULT_AFTER_SECONDS: u64 = 2;

/// One value a field can take
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Choice {
    /// What is saved
    pub value: String,
    /// What is shown; the value when none is given
    pub label: String,
}

/// The action the page calls next
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Next {
    /// A declared action
    pub action: String,
    /// Its input
    pub input: Value,
    /// Seconds to wait first, 1 to 30
    pub after_seconds: u64,
}

/// Values an action asks to keep in its plugin's settings
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Save {
    /// Non-secret values; `null` removes the key
    pub config: BTreeMap<String, Value>,
    /// Secret values; `None` removes the key
    pub secrets: BTreeMap<String, Option<String>>,
}

impl Save {
    /// Every key saved, sorted
    pub fn keys(&self) -> Vec<String> {
        let mut keys: Vec<String> = self
            .config
            .keys()
            .chain(self.secrets.keys())
            .cloned()
            .collect();
        keys.sort();
        keys
    }
}

/// A checked action result
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ActionResult {
    /// Shown to the person
    pub message: Option<String>,
    /// A short code to show in large type
    pub code: Option<String>,
    /// A link to open
    pub open_url: Option<String>,
    /// Values offered for a field
    pub choices: Option<Vec<Choice>>,
    /// The action to call next
    pub next: Option<Next>,
    /// The flow is over
    pub done: bool,
    /// What to save; never serialized, secrets stay here
    #[serde(skip)]
    pub save: Option<Save>,
}

fn text(v: &Value, what: &str, max: usize) -> Result<Option<String>, String> {
    match v {
        Value::Null => Ok(None),
        Value::String(s) if s.chars().count() <= max => Ok(Some(s.clone())),
        Value::String(_) => Err(format!("{}: at most {} characters", what, max)),
        _ => Err(format!("{}: must be text", what)),
    }
}

/// Every string in `v` scrubbed
fn scrub_value(v: Value, secrets: &[String]) -> Value {
    match v {
        Value::String(s) => Value::String(scrub(&s, secrets)),
        Value::Array(items) => {
            Value::Array(items.into_iter().map(|i| scrub_value(i, secrets)).collect())
        }
        Value::Object(o) => Value::Object(
            o.into_iter()
                .map(|(k, v)| (k, scrub_value(v, secrets)))
                .collect(),
        ),
        v => v,
    }
}

/// Check what `on_action` returned against `manifest`. `secrets` are the plugin's saved
/// secret values; secrets the result saves are scrubbed from it too.
pub fn parse(
    manifest: &Manifest,
    value: Value,
    secrets: &[String],
) -> Result<ActionResult, String> {
    let Value::Object(obj) = value else {
        return Err("on_action must return a table".to_string());
    };
    let get = |k: &str| obj.get(k).cloned().unwrap_or(Value::Null);

    let mut save = None;
    if let Value::Object(values) = get("save") {
        let mut out = Save::default();
        for (key, v) in values {
            let Some(field) = manifest.config.iter().find(|f| f.key == key) else {
                return Err(format!("save: {} has no setting {}", manifest.name, key));
            };
            if field.secret {
                match v {
                    Value::String(s) => out.secrets.insert(key, Some(s)),
                    Value::Null => out.secrets.insert(key, None),
                    _ => return Err(format!("save.{}: must be text", key)),
                };
            } else {
                out.config.insert(key, v);
            }
        }
        save = Some(out);
    } else if !get("save").is_null() {
        return Err("save: must be a table of settings".to_string());
    }
    let mut all_secrets: Vec<String> = secrets.to_vec();
    if let Some(s) = &save {
        all_secrets.extend(s.secrets.values().flatten().cloned());
    }
    let clean = |s: Option<String>| s.map(|s| scrub(&s, &all_secrets));

    let open_url = text(&get("open_url"), "open_url", 2000)?;
    if let Some(u) = &open_url {
        if crate::manifest::url_host(u).is_none() {
            return Err("open_url: must be an http or https URL".to_string());
        }
    }
    let choices = match get("choices") {
        Value::Null => None,
        Value::Array(items) => {
            if items.len() > MAX_CHOICES {
                return Err(format!("choices: at most {}", MAX_CHOICES));
            }
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                let (value, label) = match &item {
                    Value::String(s) => (s.clone(), s.clone()),
                    Value::Object(o) => {
                        let value = match o.get("value") {
                            Some(Value::String(s)) => s.clone(),
                            Some(Value::Number(n)) => n.to_string(),
                            _ => return Err("choices: each needs a text value".to_string()),
                        };
                        let label = o
                            .get("label")
                            .and_then(Value::as_str)
                            .map(str::to_string)
                            .unwrap_or_else(|| value.clone());
                        (value, label)
                    }
                    _ => return Err("choices: each must be text or {value, label}".to_string()),
                };
                out.push(Choice {
                    value: scrub(&value, &all_secrets),
                    label: scrub(&label, &all_secrets),
                });
            }
            Some(out)
        }
        // An empty Lua table converts to an object
        Value::Object(o) if o.is_empty() => Some(Vec::new()),
        _ => return Err("choices: must be a list".to_string()),
    };
    let next = match get("next") {
        Value::Null => None,
        Value::Object(o) => {
            let action = o
                .get("action")
                .and_then(Value::as_str)
                .ok_or("next.action: required")?;
            if !manifest.has_action(action) {
                return Err(format!(
                    "next.action: \"{}\" is not one of the actions",
                    action
                ));
            }
            let after = match o.get("after_seconds") {
                None | Some(Value::Null) => DEFAULT_AFTER_SECONDS,
                Some(v) => v
                    .as_f64()
                    .filter(|n| (1.0..=MAX_AFTER_SECONDS as f64).contains(n))
                    .ok_or(format!("next.after_seconds: 1 to {}", MAX_AFTER_SECONDS))?
                    .round() as u64,
            };
            Some(Next {
                action: action.to_string(),
                input: scrub_value(o.get("input").cloned().unwrap_or(Value::Null), &all_secrets),
                after_seconds: after,
            })
        }
        _ => return Err("next: must be {action, input, after_seconds}".to_string()),
    };
    let done = match get("done") {
        Value::Null => false,
        Value::Bool(b) => b,
        _ => return Err("done: must be true or false".to_string()),
    };
    Ok(ActionResult {
        message: clean(text(&get("message"), "message", MAX_MESSAGE_CHARS)?),
        code: clean(text(&get("code"), "code", MAX_CODE_CHARS)?),
        open_url: clean(open_url),
        choices,
        next,
        done,
        save,
    })
}

/// How a health item stands
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    /// Works
    Ok,
    /// Works, with something to look at
    Warn,
    /// Does not work
    Fail,
}

/// One line of what `health()` returned
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthItem {
    /// How it stands
    pub status: HealthStatus,
    /// What was checked, secrets replaced
    pub message: String,
}

/// Read what `health()` returned: a list of `{status, message}` or one of them
pub fn parse_health(value: Value, secrets: &[String]) -> Vec<HealthItem> {
    let items = match value {
        Value::Array(items) => items,
        v @ Value::Object(_) => vec![v],
        _ => {
            return vec![HealthItem {
                status: HealthStatus::Fail,
                message: "health() must return a list of {status, message}".to_string(),
            }]
        }
    };
    items
        .into_iter()
        .take(MAX_HEALTH_ITEMS)
        .map(|item| {
            let message = item
                .get("message")
                .and_then(Value::as_str)
                .map(|m| m.chars().take(MAX_MESSAGE_CHARS).collect::<String>())
                .unwrap_or_default();
            let status = match item.get("status").and_then(Value::as_str) {
                Some("ok") => Some(HealthStatus::Ok),
                Some("warn") => Some(HealthStatus::Warn),
                Some("fail") => Some(HealthStatus::Fail),
                _ => None,
            };
            match status {
                Some(status) => HealthItem {
                    status,
                    message: scrub(&message, secrets),
                },
                None => HealthItem {
                    status: HealthStatus::Fail,
                    message: format!(
                        "health item without ok, warn or fail: {}",
                        scrub(&message, secrets)
                    ),
                },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn manifest() -> Manifest {
        Manifest::parse(
            r#"
name = "m"
version = "1"
events = ["*"]

[[config]]
key = "server"
type = "url"

[[config]]
key = "token"
type = "string"
secret = true

[[actions]]
id = "sign_in"
label = "Sign in"

[[actions]]
id = "poll"
label = "Poll"
hidden = true
"#,
            "plugin.toml",
        )
        .unwrap()
    }

    #[test]
    fn full_result() {
        let r = parse(
            &manifest(),
            json!({
                "message": "Signed in with tok_NEW1",
                "code": "AB12",
                "open_url": "https://app.plex.tv/auth#?code=AB12",
                "choices": ["a", {"value": "1", "label": "TV"}, {"value": 2}],
                "next": {"action": "poll", "input": {"pin": 7, "t": "old_SECRET"}},
                "done": false,
                "save": {"token": "tok_NEW1", "server": "http://plex:32400"},
            }),
            &["old_SECRET".to_string()],
        )
        .unwrap();
        assert_eq!(r.message.as_deref(), Some("Signed in with ********"));
        assert_eq!(r.code.as_deref(), Some("AB12"));
        assert_eq!(r.choices.as_ref().unwrap().len(), 3);
        assert_eq!(r.choices.as_ref().unwrap()[1].label, "TV");
        assert_eq!(r.choices.as_ref().unwrap()[2].label, "2");
        let next = r.next.clone().unwrap();
        assert_eq!((next.action.as_str(), next.after_seconds), ("poll", 2));
        assert_eq!(next.input["t"], "********");
        let save = r.save.clone().unwrap();
        assert_eq!(save.keys(), vec!["server", "token"]);
        assert_eq!(save.secrets["token"].as_deref(), Some("tok_NEW1"));
        let shown = serde_json::to_string(&r).unwrap();
        assert!(
            !shown.contains("tok_NEW1") && !shown.contains("save"),
            "{}",
            shown
        );
    }

    #[test]
    fn checks() {
        let m = manifest();
        let e = |v: Value| parse(&m, v, &[]).unwrap_err();
        assert!(e(json!("x")).contains("must return a table"));
        assert!(e(json!({"open_url": "javascript:alert(1)"})).contains("open_url"));
        assert!(e(json!({"next": {"action": "nope"}})).contains("next.action: \"nope\""));
        assert!(e(json!({"next": {"action": "poll", "after_seconds": 60}})).contains("1 to 30"));
        assert!(e(json!({"save": {"colour": 1}})).contains("no setting colour"));
        assert!(e(json!({"save": {"token": 5}})).contains("save.token: must be text"));
        assert!(e(json!({"message": "x".repeat(501)})).contains("at most 500"));
        assert!(e(json!({"code": "x".repeat(33)})).contains("at most 32"));
        assert!(e(json!({"done": "yes"})).contains("done"));
        assert_eq!(parse(&m, json!({}), &[]).unwrap(), ActionResult::default());
        assert_eq!(
            parse(&m, json!({"save": {"token": null}}), &[])
                .unwrap()
                .save
                .unwrap()
                .secrets["token"],
            None
        );
    }

    #[test]
    fn health_items() {
        let items = parse_health(
            json!([
                {"status": "ok", "message": "reachable"},
                {"status": "warn", "message": "token tk_ABCD"},
                {"status": "maybe", "message": "x"},
            ]),
            &["tk_ABCD".to_string()],
        );
        assert_eq!(items[0].status, HealthStatus::Ok);
        assert_eq!(items[1].message, "token ********");
        assert_eq!(items[2].status, HealthStatus::Fail);
        assert_eq!(
            parse_health(json!({"status": "ok", "message": "y"}), &[]).len(),
            1
        );
        assert_eq!(parse_health(json!(5), &[])[0].status, HealthStatus::Fail);
    }
}
