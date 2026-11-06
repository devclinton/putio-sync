//! The OpenAI-compatible chat client (research R2): one request, one reply, and the error
//! classes the chunk driver acts on. Works with Ollama (`/v1`), LM Studio, OpenAI, Open WebUI
//! and anything else that speaks `POST .../chat/completions`.

use std::sync::Arc;
use std::time::Duration;

use putio_sync_subtitles::BoxFut;
use serde_json::{json, Value};
use tokio::sync::Semaphore;

use crate::chunk::{AttemptError, CallError, ChatCall, Fatal, Request};
use crate::prompt::{self, ReplyError};

/// Longest error text kept from a server's answer
const MAX_ERROR_CHARS: usize = 300;
/// All 429 waits of one request may not add up to more than this
pub const RATE_LIMIT_BUDGET_SECS: u64 = 300;
/// One 429 wait is never longer than this
const MAX_WAIT_SECS: u64 = 120;
/// Connect timeout, fixed
const CONNECT_SECS: u64 = 10;

/// The URL requests go to (R2): empty path gets `/v1/chat/completions`, a path that already
/// ends in `/chat/completions` stays, anything else gets `/chat/completions` appended.
pub fn resolve_endpoint(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    let (origin, path) = split_origin(base);
    if path.is_empty() {
        format!("{origin}/v1/chat/completions")
    } else if path.ends_with("/chat/completions") {
        base.to_string()
    } else {
        format!("{base}/chat/completions")
    }
}

/// Where the model list is: `<base>/models`, or `/v1/models` for a base with no path
pub fn models_url(base_url: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    let (origin, path) = split_origin(base);
    if path.is_empty() {
        return format!("{origin}/v1/models");
    }
    let path = path.strip_suffix("/chat/completions").unwrap_or(path);
    format!("{origin}{path}/models")
}

/// `("http://host:1234", "/v1")`
fn split_origin(url: &str) -> (&str, &str) {
    let after = url.find("://").map(|i| i + 3).unwrap_or(0);
    match url[after..].find('/') {
        Some(i) => (&url[..after + i], &url[after + i..]),
        None => (url, ""),
    }
}

/// What the answer to one HTTP request means
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// The model's text
    Text(String),
    /// 429, with the `Retry-After` seconds when it was an integer
    RateLimited(Option<u64>),
    /// Anything else
    Failed(CallError),
}

/// The message in an error body: `error.message`, or `error` as a string, or the start of the
/// body. The caller scrubs it.
pub fn error_message(body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<Value>(body) {
        if let Some(m) = v.pointer("/error/message").and_then(Value::as_str) {
            return m.to_string();
        }
        if let Some(m) = v.get("error").and_then(Value::as_str) {
            return m.to_string();
        }
        if let Some(m) = v.get("message").and_then(Value::as_str) {
            return m.to_string();
        }
    }
    body.chars().take(MAX_ERROR_CHARS).collect()
}

fn names_a_missing_model(message: &str) -> bool {
    let m = message.to_lowercase();
    m.contains("model")
        && (m.contains("not found") || m.contains("does not exist") || m.contains("not exist"))
}

fn is_context_problem(message: &str) -> bool {
    let m = message.to_lowercase();
    ["context", "too long", "maximum", "token"]
        .iter()
        .any(|w| m.contains(w))
}

/// Turn an HTTP answer into what the driver acts on. `scrub` removes secrets and shortens.
pub fn classify(
    status: u16,
    retry_after: Option<&str>,
    body: &str,
    endpoint: &str,
    scrub: &dyn Fn(&str) -> String,
) -> Answer {
    match status {
        200..=299 => match reply_text(body) {
            Ok(text) => Answer::Text(text),
            Err(e) => Answer::Failed(AttemptError::Invalid(e).into()),
        },
        401 | 403 => Answer::Failed(Fatal::Key.into()),
        404 => {
            let message = scrub(&error_message(body));
            if names_a_missing_model(&message) {
                Answer::Failed(Fatal::ModelNotFound(message).into())
            } else {
                Answer::Failed(
                    Fatal::EndpointNotFound(format!(
                        "endpoint not found (HTTP 404) at {endpoint}: check the base URL"
                    ))
                    .into(),
                )
            }
        }
        429 => Answer::RateLimited(retry_after.and_then(|v| v.trim().parse::<u64>().ok())),
        400 => {
            let message = scrub(&error_message(body));
            if is_context_problem(&message) {
                Answer::Failed(AttemptError::Http { status, message }.into())
            } else {
                Answer::Failed(Fatal::BadRequest(message).into())
            }
        }
        408 | 500..=599 => Answer::Failed(
            AttemptError::Http {
                status,
                message: scrub(&error_message(body)),
            }
            .into(),
        ),
        _ => Answer::Failed(
            Fatal::BadRequest(format!("HTTP {status}: {}", scrub(&error_message(body)))).into(),
        ),
    }
}

/// `choices[0].message.content` without `<think>` blocks
fn reply_text(body: &str) -> Result<String, ReplyError> {
    let v: Value = serde_json::from_str(body).map_err(|_| ReplyError::NoContent)?;
    let choice = v.pointer("/choices/0").ok_or(ReplyError::NoContent)?;
    if choice.get("finish_reason").and_then(Value::as_str) == Some("length") {
        return Err(ReplyError::Truncated);
    }
    let content = choice
        .pointer("/message/content")
        .and_then(Value::as_str)
        .ok_or(ReplyError::NoContent)?;
    let text = prompt::strip_think(content).trim().to_string();
    if text.is_empty() {
        return Err(ReplyError::NoContent);
    }
    Ok(text)
}

/// A chat server and the limit on requests in flight
#[derive(Clone)]
pub struct ChatClient {
    http: reqwest::Client,
    endpoint: String,
    model: String,
    api_key: String,
    timeout_secs: u64,
    permits: Arc<Semaphore>,
    /// Smallest 429 wait; tests lower it
    min_wait: Duration,
}

impl ChatClient {
    /// A client for `base_url`. `permits` is shared by every translation in the daemon.
    pub fn new(
        base_url: &str,
        model: &str,
        api_key: &str,
        timeout_secs: u64,
        permits: Arc<Semaphore>,
    ) -> Result<ChatClient, String> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(CONNECT_SECS))
            .timeout(Duration::from_secs(timeout_secs.max(1)))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(ChatClient {
            http,
            endpoint: resolve_endpoint(base_url),
            model: model.to_string(),
            api_key: api_key.to_string(),
            timeout_secs,
            permits,
            min_wait: Duration::from_secs(1),
        })
    }

    /// The URL requests go to
    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    /// The model name
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Smallest wait after a 429 (default 1 s)
    pub fn with_min_wait(mut self, wait: Duration) -> Self {
        self.min_wait = wait;
        self
    }

    fn scrub(&self, text: &str) -> String {
        crate::callback::scrub(text, std::slice::from_ref(&self.api_key), MAX_ERROR_CHARS)
    }

    /// One request, with 429 waits that give the permit back
    pub async fn send(&self, system: &str, user: &str) -> Result<String, CallError> {
        let body = json!({
            "model": self.model,
            "stream": false,
            "temperature": 0,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
        });
        let mut waited = 0u64;
        let mut backoff = 2u64;
        loop {
            let permit = self
                .permits
                .acquire()
                .await
                .map_err(|_| AttemptError::Unreachable("translation stopped".to_string()))?;
            let answer = self.once(&body).await;
            drop(permit);
            match answer? {
                Answer::Text(t) => return Ok(t),
                Answer::Failed(e) => return Err(e),
                Answer::RateLimited(after) => {
                    putio_sync_telemetry::translation::rate_limited();
                    let secs = after.unwrap_or(backoff).min(MAX_WAIT_SECS);
                    backoff = (backoff * 2).min(60);
                    let wait = Duration::from_secs(secs).max(self.min_wait);
                    waited += wait.as_secs().max(1);
                    if waited > RATE_LIMIT_BUDGET_SECS {
                        return Err(Fatal::RateLimited(RATE_LIMIT_BUDGET_SECS).into());
                    }
                    tokio::time::sleep(wait).await;
                }
            }
        }
    }

    async fn once(&self, body: &Value) -> Result<Answer, CallError> {
        let mut req = self.http.post(&self.endpoint).json(body);
        if !self.api_key.is_empty() {
            req = req.bearer_auth(&self.api_key);
        }
        let resp = req.send().await.map_err(|e| self.transport(&e))?;
        let status = resp.status().as_u16();
        let retry_after = resp
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        let text = resp.text().await.map_err(|e| self.transport(&e))?;
        Ok(classify(
            status,
            retry_after.as_deref(),
            &text,
            &self.endpoint,
            &|s| self.scrub(s),
        ))
    }

    fn transport(&self, e: &reqwest::Error) -> CallError {
        if e.is_timeout() {
            AttemptError::Timeout(self.timeout_secs).into()
        } else {
            AttemptError::Unreachable(self.scrub(&redact_url(e))).into()
        }
    }
}

/// The error text without the request URL
fn redact_url(e: &reqwest::Error) -> String {
    let mut text = e.to_string();
    if let Some(url) = e.url() {
        text = text.replace(url.as_str(), "<url>");
    }
    text
}

/// A [`ChatClient`] with the system prompt of one translation
pub struct ChatSession {
    /// The server
    pub client: ChatClient,
    /// The system prompt for this language pair
    pub system: String,
}

impl ChatCall for ChatSession {
    fn call<'a>(&'a self, request: &'a Request) -> BoxFut<'a, Result<String, CallError>> {
        Box::pin(async move {
            let user = prompt::user_prompt(&request.context, &request.lines);
            self.client.send(&self.system, &user).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_endpoint_rule() {
        for (base, want) in [
            ("http://host:1234", "http://host:1234/v1/chat/completions"),
            ("http://host:1234/", "http://host:1234/v1/chat/completions"),
            (
                "http://ollama.lan:11434/v1",
                "http://ollama.lan:11434/v1/chat/completions",
            ),
            (
                "https://api.openai.com/v1/",
                "https://api.openai.com/v1/chat/completions",
            ),
            (
                "http://h/v1/chat/completions",
                "http://h/v1/chat/completions",
            ),
            ("http://h:3000/api", "http://h:3000/api/chat/completions"),
        ] {
            assert_eq!(resolve_endpoint(base), want, "{base}");
        }
    }

    #[test]
    fn the_models_url() {
        assert_eq!(models_url("http://h:1234"), "http://h:1234/v1/models");
        assert_eq!(models_url("http://h/v1"), "http://h/v1/models");
        assert_eq!(
            models_url("http://h/v1/chat/completions"),
            "http://h/v1/models"
        );
    }

    #[test]
    fn error_messages_come_from_the_known_shapes() {
        assert_eq!(error_message(r#"{"error":{"message":"nope"}}"#), "nope");
        assert_eq!(error_message(r#"{"error":"plain"}"#), "plain");
        assert_eq!(error_message("not json at all"), "not json at all");
    }

    #[test]
    fn a_404_names_the_model_or_the_url() {
        let id = |s: &str| s.to_string();
        let m = classify(
            404,
            None,
            r#"{"error":{"message":"model \"x\" not found, try pulling it first"}}"#,
            "http://h/e",
            &id,
        );
        assert!(matches!(
            m,
            Answer::Failed(CallError::Fatal(Fatal::ModelNotFound(_)))
        ));
        let m = classify(
            404,
            None,
            r#"{"error":"The model `x` does not exist"}"#,
            "http://h/e",
            &id,
        );
        assert!(matches!(
            m,
            Answer::Failed(CallError::Fatal(Fatal::ModelNotFound(_)))
        ));
        let u = classify(404, None, "404 page not found", "http://h/e", &id);
        match u {
            Answer::Failed(CallError::Fatal(Fatal::EndpointNotFound(m))) => {
                assert!(m.contains("http://h/e"))
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn length_and_empty_replies_are_invalid() {
        let id = |s: &str| s.to_string();
        let cut = r#"{"choices":[{"finish_reason":"length","message":{"content":"1| a"}}]}"#;
        assert!(matches!(
            classify(200, None, cut, "e", &id),
            Answer::Failed(CallError::Attempt(AttemptError::Invalid(
                ReplyError::Truncated
            )))
        ));
        let none = r#"{"choices":[{"message":{"content":null}}]}"#;
        assert!(matches!(
            classify(200, None, none, "e", &id),
            Answer::Failed(CallError::Attempt(AttemptError::Invalid(
                ReplyError::NoContent
            )))
        ));
        let think = r#"{"choices":[{"finish_reason":"stop","message":{"content":"<think>hmm</think>1| a"}}]}"#;
        assert_eq!(
            classify(200, None, think, "e", &id),
            Answer::Text("1| a".into())
        );
    }

    #[test]
    fn rate_limit_reads_an_integer_retry_after_only() {
        let id = |s: &str| s.to_string();
        assert_eq!(
            classify(429, Some("7"), "", "e", &id),
            Answer::RateLimited(Some(7))
        );
        assert_eq!(
            classify(429, Some("Wed, 21 Oct 2026 07:28:00 GMT"), "", "e", &id),
            Answer::RateLimited(None)
        );
    }

    #[test]
    fn a_400_about_context_is_split_not_fatal() {
        let id = |s: &str| s.to_string();
        let ctx = classify(
            400,
            None,
            r#"{"error":{"message":"maximum context length is 4096 tokens"}}"#,
            "e",
            &id,
        );
        assert!(matches!(ctx, Answer::Failed(CallError::Attempt(_))));
        let other = classify(400, None, r#"{"error":{"message":"bad field"}}"#, "e", &id);
        assert!(matches!(
            other,
            Answer::Failed(CallError::Fatal(Fatal::BadRequest(_)))
        ));
    }
}
