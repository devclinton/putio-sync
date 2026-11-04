//! URL hooks: POST the event as JSON (research R4).
//!
//! The URL may hold a token, so no error or log line uses `reqwest::Error`'s `Display`
//! (which prints the full URL); messages are built from the status or the error kind
//! and [`redact::url`]. Redirects are followed only to the same host and port, so the
//! hook's headers never go elsewhere.

use std::time::{Duration, Instant};

use chrono::Utc;
use putio_sync_core::events::Event;
use putio_sync_core::hooks::HookConfig;
use putio_sync_core::redact;
use reqwest::{redirect, Client, StatusCode};

use crate::{Capped, HookRun, RunResult};

/// Waits before the second and third attempts.
pub const RETRY_DELAYS: [Duration; 2] = [Duration::from_secs(5), Duration::from_secs(30)];

/// Most redirects followed.
const MAX_REDIRECTS: usize = 3;

/// The client every URL hook shares.
pub fn client() -> Client {
    Client::builder()
        .user_agent(concat!("putio-sync/", env!("CARGO_PKG_VERSION")))
        .redirect(redirect::Policy::custom(|attempt| {
            let first = &attempt.previous()[0];
            let same = attempt.url().host_str() == first.host_str()
                && attempt.url().port_or_known_default() == first.port_or_known_default();
            if attempt.previous().len() > MAX_REDIRECTS || !same {
                attempt.stop()
            } else {
                attempt.follow()
            }
        }))
        .build()
        .expect("the webhook client builds")
}

/// POST `event` to `hook`'s URL, retrying after each of `delays` on a connect error, a
/// timeout, 429 or 5xx.
pub async fn run(
    client: &Client,
    hook: &HookConfig,
    event: &Event,
    delays: &[Duration],
) -> HookRun {
    let start = Instant::now();
    let mut run = HookRun {
        event_type: event.event_type().as_str().to_string(),
        event_id: event.id.clone(),
        started_at: Utc::now(),
        duration_ms: 0,
        result: RunResult::Failure,
        attempts: 0,
        exit_code: None,
        http_status: None,
        error: None,
        stdout: String::new(),
        stderr: String::new(),
    };
    let url = hook.url.clone().unwrap_or_default();
    let shown = redact::url(&url);
    let body = event.to_json();

    let mut delays = delays.iter();
    loop {
        run.attempts += 1;
        let mut request = client
            .post(&url)
            .timeout(hook.timeout())
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body.clone());
        for (name, value) in &hook.headers {
            request = request.header(name, value);
        }
        let retry = match request.send().await {
            Ok(response) => {
                let status = response.status();
                run.http_status = Some(status.as_u16());
                let mut out = Capped::default();
                if let Ok(bytes) = response.bytes().await {
                    out.push(&bytes);
                }
                run.stdout = out.text();
                if status.is_success() {
                    run.result = RunResult::Success;
                    run.error = None;
                    break;
                }
                run.result = RunResult::Failure;
                run.error = Some(format!("{} from {}", describe(status), shown));
                status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
            }
            Err(e) => {
                run.http_status = None;
                run.stdout.clear();
                if e.is_timeout() {
                    run.result = RunResult::Timeout;
                    run.error = Some(format!(
                        "timed out after {} s calling {}",
                        hook.timeout_secs, shown
                    ));
                    true
                } else if e.is_connect() {
                    run.result = RunResult::Failure;
                    run.error = Some(format!("could not connect to {}", shown));
                    true
                } else {
                    run.result = RunResult::Failure;
                    run.error = Some(format!("request to {} failed", shown));
                    false
                }
            }
        };
        match delays.next() {
            Some(delay) if retry => tokio::time::sleep(*delay).await,
            _ => break,
        }
    }
    run.duration_ms = start.elapsed().as_millis() as u64;
    run
}

fn describe(status: StatusCode) -> String {
    match status.canonical_reason() {
        Some(reason) => format!("HTTP {} {}", status.as_u16(), reason),
        None => format!("HTTP {}", status.as_u16()),
    }
}
