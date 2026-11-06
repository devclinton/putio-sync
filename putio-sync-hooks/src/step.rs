//! Hooks as pipeline steps (feature 038, research R3).
//!
//! A hook with a `stage` runs for one item, not for an event. A command gets the item in
//! `PUTIO_SYNC_ITEM_*` variables and as JSON in a temp file named by `PUTIO_SYNC_ITEM_JSON`
//! (removed after the run); a URL hook gets the same JSON as the POST body. The output is
//! kept as a tail, because the end of a log is what tells you why it failed.

use std::collections::VecDeque;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use putio_sync_core::hooks::HookConfig;
use putio_sync_core::redact;
use reqwest::Client;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

use crate::command::{kill_group, DEFAULT_PATH, DRAIN};

/// Most lines of output kept for a step
pub const MAX_LINES: usize = 200;

/// Most bytes of output kept for a step
pub const MAX_BYTES: usize = 64 * 1024;

/// The exit code that asks to skip the item (pre_download only)
pub const SKIP_EXIT_CODE: i32 = 10;

/// The item a step runs for, as the hook sees it
#[derive(Debug, Clone)]
pub struct ItemPayload {
    pub key: String,
    pub name: String,
    pub category: Option<String>,
    pub source: String,
    pub dir: String,
    pub stage: String,
    /// The JSON document (research R3)
    pub json: String,
}

/// How a step run ended
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepResult {
    /// Exit 0, or a 2xx
    Done,
    /// Exit 10, or 204 with `{"action":"skip"}`
    Skip,
    /// Anything else
    Failed,
    /// The hook's timeout passed
    Timeout,
}

/// What a step run left behind
#[derive(Debug, Clone)]
pub struct StepRun {
    pub result: StepResult,
    /// A command's exit code
    pub exit_code: Option<i32>,
    /// A URL hook's status
    pub http_status: Option<u16>,
    /// The tail of stdout and stderr, or the response body; not yet redacted
    pub output: String,
    /// What went wrong, without secrets
    pub error: Option<String>,
    pub duration_ms: u64,
}

impl StepRun {
    fn new() -> Self {
        StepRun {
            result: StepResult::Failed,
            exit_code: None,
            http_status: None,
            output: String::new(),
            error: None,
            duration_ms: 0,
        }
    }
}

/// Run `hook` for `item`, a command or a URL by its kind
pub async fn run_step(client: &Client, hook: &HookConfig, item: &ItemPayload) -> StepRun {
    if hook.url.is_some() {
        run_url(client, hook, item).await
    } else {
        run_command(hook, item).await
    }
}

/// Keep the last [`MAX_LINES`] lines and [`MAX_BYTES`] bytes of a text
pub fn tail(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let skip = lines.len().saturating_sub(MAX_LINES);
    let mut out = lines[skip..].join("\n");
    let mut cut = skip > 0;
    if out.len() > MAX_BYTES {
        let mut start = out.len() - MAX_BYTES;
        while !out.is_char_boundary(start) {
            start += 1;
        }
        out = out[start..].to_string();
        cut = true;
    }
    if cut {
        format!("(earlier output cut)\n{out}")
    } else {
        out
    }
}

/// A byte tail shared by the two pipe readers
#[derive(Default)]
struct Tail {
    chunks: VecDeque<Vec<u8>>,
    bytes: usize,
}

impl Tail {
    fn push(&mut self, data: &[u8]) {
        self.bytes += data.len();
        self.chunks.push_back(data.to_vec());
        // Keep a little over the cap; `tail` trims exactly
        while self.bytes > MAX_BYTES * 2 && self.chunks.len() > 1 {
            if let Some(old) = self.chunks.pop_front() {
                self.bytes -= old.len();
            }
        }
    }

    fn text(&self) -> String {
        let all: Vec<u8> = self.chunks.iter().flatten().copied().collect();
        String::from_utf8_lossy(&all).into_owned()
    }
}

async fn read(mut pipe: impl AsyncRead + Unpin, into: Arc<Mutex<Tail>>) {
    let mut buf = [0u8; 8192];
    while let Ok(n) = pipe.read(&mut buf).await {
        if n == 0 {
            break;
        }
        if let Ok(mut t) = into.lock() {
            t.push(&buf[..n]);
        }
    }
}

async fn run_command(hook: &HookConfig, item: &ItemPayload) -> StepRun {
    let start = Instant::now();
    let mut run = StepRun::new();
    let Some((program, args)) = hook.command.as_deref().and_then(|c| c.split_first()) else {
        run.error = Some("no command set".to_string());
        return run;
    };

    // The JSON goes to a file the hook can read at any size; it is removed on the way out
    let file = std::env::temp_dir().join(format!(
        "putio-sync-item-{}-{}.json",
        std::process::id(),
        uuid_like()
    ));
    if let Err(e) = tokio::fs::write(&file, item.json.as_bytes()).await {
        run.error = Some(format!("could not write the item file: {}", e));
        return run;
    }

    let mut cmd = Command::new(program);
    cmd.args(args)
        .env_clear()
        .env(
            "PATH",
            std::env::var("PATH").unwrap_or_else(|_| DEFAULT_PATH.to_string()),
        )
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for name in ["HOME", "LANG", "TZ"] {
        if let Ok(value) = std::env::var(name) {
            cmd.env(name, value);
        }
    }
    cmd.envs(&hook.env)
        .env("PUTIO_SYNC_HOOK_NAME", &hook.name)
        .env("PUTIO_SYNC_ITEM_KEY", &item.key)
        .env("PUTIO_SYNC_ITEM_NAME", &item.name)
        .env(
            "PUTIO_SYNC_ITEM_CATEGORY",
            item.category.as_deref().unwrap_or(""),
        )
        .env("PUTIO_SYNC_ITEM_SOURCE", &item.source)
        .env("PUTIO_SYNC_ITEM_DIR", &item.dir)
        .env("PUTIO_SYNC_ITEM_STAGE", &item.stage)
        .env("PUTIO_SYNC_ITEM_JSON", &file);
    #[cfg(unix)]
    cmd.process_group(0);

    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            run.error = Some(match e.kind() {
                std::io::ErrorKind::NotFound => format!("program not found: {}", program),
                std::io::ErrorKind::PermissionDenied => {
                    format!("program is not executable: {}", program)
                }
                _ => format!("could not start {}: {}", program, e),
            });
            run.duration_ms = start.elapsed().as_millis() as u64;
            let _ = tokio::fs::remove_file(&file).await;
            return run;
        }
    };

    let out = Arc::new(Mutex::new(Tail::default()));
    let readers = [
        child
            .stdout
            .take()
            .map(|p| tokio::spawn(read(p, out.clone()))),
        child
            .stderr
            .take()
            .map(|p| tokio::spawn(read(p, out.clone()))),
    ];

    match tokio::time::timeout(hook.timeout(), child.wait()).await {
        Ok(Ok(status)) => {
            run.exit_code = status.code();
            if status.success() {
                run.result = StepResult::Done;
            } else if status.code() == Some(SKIP_EXIT_CODE) {
                run.result = StepResult::Skip;
            } else {
                run.error = Some(match status.code() {
                    Some(code) => format!("exited with code {}", code),
                    None => "killed by a signal".to_string(),
                });
            }
        }
        Ok(Err(e)) => run.error = Some(format!("could not wait for {}: {}", program, e)),
        Err(_) => {
            kill_group(&child);
            let _ = child.kill().await;
            run.result = StepResult::Timeout;
            run.error = Some(format!("timed out after {} s", hook.timeout_secs));
        }
    }
    for reader in readers.into_iter().flatten() {
        let _ = tokio::time::timeout(DRAIN, reader).await;
    }
    run.output = tail(&out.lock().map(|t| t.text()).unwrap_or_default());
    run.duration_ms = start.elapsed().as_millis() as u64;
    let _ = tokio::fs::remove_file(&file).await;
    run
}

/// A short unique suffix for the temp file name
fn uuid_like() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    format!("{:x}{:x}", nanos, NEXT.fetch_add(1, Ordering::Relaxed))
}

async fn run_url(client: &Client, hook: &HookConfig, item: &ItemPayload) -> StepRun {
    let start = Instant::now();
    let mut run = StepRun::new();
    let url = hook.url.clone().unwrap_or_default();
    let shown = redact::url(&url);
    let mut request = client
        .post(&url)
        .timeout(hook.timeout())
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(item.json.clone());
    for (name, value) in &hook.headers {
        request = request.header(name, value);
    }
    match request.send().await {
        Ok(response) => {
            let status = response.status();
            run.http_status = Some(status.as_u16());
            let body = response.bytes().await.unwrap_or_default();
            run.output = tail(&String::from_utf8_lossy(&body));
            if status.is_success() {
                // A 2xx whose body says skip asks to skip the item (pre_download only)
                run.result = if wants_skip(&body) {
                    StepResult::Skip
                } else {
                    StepResult::Done
                };
            } else {
                run.error = Some(format!("HTTP {} from {}", status.as_u16(), shown));
            }
        }
        Err(e) if e.is_timeout() => {
            run.result = StepResult::Timeout;
            run.error = Some(format!(
                "timed out after {} s calling {}",
                hook.timeout_secs, shown
            ));
        }
        Err(e) if e.is_connect() => {
            run.error = Some(format!("could not connect to {}", shown));
        }
        Err(_) => run.error = Some(format!("request to {} failed", shown)),
    }
    run.duration_ms = start.elapsed().as_millis() as u64;
    run
}

/// Whether a response body is `{"action":"skip"}`
fn wants_skip(body: &[u8]) -> bool {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.get("action")
                .and_then(|a| a.as_str())
                .map(|a| a == "skip")
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_keeps_the_end() {
        let text: String = (1..=500).map(|n| format!("line {n}\n")).collect();
        let t = tail(&text);
        assert!(t.starts_with("(earlier output cut)"));
        assert!(t.ends_with("line 500"));
        assert!(!t.contains("line 300\n"));
        assert_eq!(tail("one\ntwo"), "one\ntwo");
    }

    #[test]
    fn tail_caps_bytes() {
        let t = tail(&"x".repeat(MAX_BYTES * 2));
        assert!(t.len() < MAX_BYTES + 40);
    }

    #[test]
    fn skip_body_is_recognised() {
        assert!(wants_skip(br#"{"action":"skip"}"#));
        assert!(!wants_skip(br#"{"action":"go"}"#));
        assert!(!wants_skip(b"not json"));
    }
}
