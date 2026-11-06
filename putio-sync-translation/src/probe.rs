//! The settings "Test" for the chat translator (feature 026, contracts/api.md): one request
//! with two short sample lines, the problem named when it fails, and the model list when the
//! server did not know the model. The whole test takes at most [`LIMIT_SECS`] seconds.

use std::sync::Arc;
use std::time::{Duration, Instant};

use putio_sync_core::lang;
use serde::Serialize;
use serde_json::Value;
use tokio::sync::Semaphore;

use crate::chat::{models_url, resolve_endpoint, ChatClient};
use crate::chunk::{AttemptError, CallError, Fatal};
use crate::prompt;

/// Total time of a test, seconds
pub const LIMIT_SECS: u64 = 15;
/// Most model ids listed
pub const MAX_MODELS: usize = 20;

/// The two sample cues
const SAMPLE: [(usize, &str); 2] = [
    (1, "Good morning."),
    (2, "Where are you going?\nIt is late."),
];

/// One sample cue and what came back for it
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SampleLine {
    /// Cue number
    pub number: usize,
    /// The English source
    pub source: String,
    /// The translation, or the raw reply line when the reply failed validation
    pub translation: String,
}

/// What a test found
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ChatProbe {
    /// The sample was translated
    pub ok: bool,
    /// The URL requests go to
    pub endpoint: String,
    /// Milliseconds the test took
    pub took_ms: u64,
    /// `url`, `key`, `model`, `timeout`, `format` or `server`
    pub problem: Option<&'static str>,
    /// The server's message, scrubbed
    pub error: Option<String>,
    /// The sample lines (raw reply lines for `format`)
    pub sample: Vec<SampleLine>,
    /// Up to 20 model ids, for `model`
    pub models: Option<Vec<String>>,
}

/// Run the test against `base_url`
pub async fn chat(
    base_url: &str,
    model: &str,
    api_key: &str,
    target_code: &str,
    instruction: &str,
) -> ChatProbe {
    let started = Instant::now();
    let endpoint = resolve_endpoint(base_url);
    let mut out = ChatProbe {
        ok: false,
        endpoint,
        took_ms: 0,
        problem: None,
        error: None,
        sample: Vec::new(),
        models: None,
    };
    let limit = Duration::from_secs(LIMIT_SECS);
    let client = match ChatClient::new(
        base_url,
        model,
        api_key,
        LIMIT_SECS,
        Arc::new(Semaphore::new(1)),
    ) {
        Ok(c) => c,
        Err(e) => {
            out.problem = Some("url");
            out.error = Some(e);
            return finish(out, started);
        }
    };
    let target_name = lang::name(target_code).unwrap_or_else(|| target_code.to_string());
    let system = prompt::system_prompt("English", "en", &target_name, target_code, instruction);
    let user = prompt::user_prompt(
        &[],
        &SAMPLE
            .iter()
            .map(|(n, t)| (*n, t.to_string()))
            .collect::<Vec<_>>(),
    );
    let sent = tokio::time::timeout(limit, client.send(&system, &user)).await;
    match sent {
        Err(_) => {
            out.problem = Some("timeout");
            out.error = Some(format!("no answer within {LIMIT_SECS} s"));
        }
        Ok(Ok(reply)) => {
            let expected: Vec<usize> = SAMPLE.iter().map(|(n, _)| *n).collect();
            let parsed = prompt::parse_reply(&reply);
            match prompt::validate(&expected, &[], parsed.clone()) {
                Ok(texts) => {
                    out.ok = true;
                    out.sample = sample(&texts);
                }
                Err(e) => {
                    out.problem = Some("format");
                    out.error = Some(e.to_string());
                    out.sample = parsed
                        .into_iter()
                        .map(|(number, translation)| SampleLine {
                            number,
                            source: SAMPLE
                                .iter()
                                .find(|(n, _)| *n == number)
                                .map(|(_, t)| t.to_string())
                                .unwrap_or_default(),
                            translation,
                        })
                        .collect();
                }
            }
        }
        Ok(Err(CallError::Fatal(f))) => {
            out.error = Some(f.to_string());
            out.problem = Some(match f {
                Fatal::Key => "key",
                Fatal::ModelNotFound(_) => "model",
                Fatal::EndpointNotFound(_) => "url",
                Fatal::BadRequest(_) | Fatal::RateLimited(_) => "server",
            });
        }
        Ok(Err(CallError::Attempt(e))) => {
            out.error = Some(e.to_string());
            out.problem = Some(match e {
                AttemptError::Invalid(_) => "format",
                AttemptError::Http { .. } => "server",
                AttemptError::Timeout(_) => "timeout",
                AttemptError::Unreachable(_) => "url",
            });
        }
    }
    if out.problem == Some("model") {
        let left = limit
            .saturating_sub(started.elapsed())
            .max(Duration::from_secs(2));
        out.models = list_models(base_url, api_key, left).await;
    }
    finish(out, started)
}

fn finish(mut out: ChatProbe, started: Instant) -> ChatProbe {
    out.took_ms = started.elapsed().as_millis() as u64;
    out
}

fn sample(texts: &[String]) -> Vec<SampleLine> {
    SAMPLE
        .iter()
        .zip(texts)
        .map(|((number, source), t)| SampleLine {
            number: *number,
            source: source.to_string(),
            translation: t.clone(),
        })
        .collect()
}

/// `GET <base>/models`: up to [`MAX_MODELS`] ids, or `None` when the server did not answer
async fn list_models(base_url: &str, api_key: &str, within: Duration) -> Option<Vec<String>> {
    let http = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(within)
        .build()
        .ok()?;
    let mut req = http.get(models_url(base_url));
    if !api_key.is_empty() {
        req = req.bearer_auth(api_key);
    }
    let resp = req.send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: Value = resp.json().await.ok()?;
    let ids: Vec<String> = v
        .get("data")?
        .as_array()?
        .iter()
        .filter_map(|m| m.get("id").and_then(Value::as_str))
        .take(MAX_MODELS)
        .map(str::to_string)
        .collect();
    Some(ids)
}
