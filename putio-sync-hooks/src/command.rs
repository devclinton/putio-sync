//! Command hooks: run a program with the event on stdin (research R3).
//!
//! No shell is involved, and the program gets a clean environment: `PATH`, `HOME`,
//! `LANG` and `TZ` from the daemon, the three `PUTIO_SYNC_*` event variables and the
//! hook's own `env`, so tokens in the daemon's environment never reach hook code.

use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::Utc;
use putio_sync_core::events::Event;
use putio_sync_core::hooks::HookConfig;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;

use crate::{Capped, HookRun, RunResult};

/// `PATH` when the daemon has none.
pub(crate) const DEFAULT_PATH: &str =
    "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

/// How long to wait for output after the program ends, for pipes a background child
/// keeps open.
pub(crate) const DRAIN: Duration = Duration::from_secs(2);

/// Run `hook`'s command for `event` and wait for it, at most the hook's timeout.
pub async fn run(hook: &HookConfig, event: &Event) -> HookRun {
    let started_at = Utc::now();
    let start = Instant::now();
    let mut run = HookRun {
        event_type: event.event_type().as_str().to_string(),
        event_id: event.id.clone(),
        started_at,
        duration_ms: 0,
        result: RunResult::Failure,
        attempts: 1,
        exit_code: None,
        http_status: None,
        error: None,
        stdout: String::new(),
        stderr: String::new(),
    };
    let Some((program, args)) = hook.command.as_deref().and_then(|c| c.split_first()) else {
        run.error = Some("no command set".to_string());
        return run;
    };

    let mut cmd = Command::new(program);
    cmd.args(args)
        .env_clear()
        .env(
            "PATH",
            std::env::var("PATH").unwrap_or_else(|_| DEFAULT_PATH.to_string()),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for name in ["HOME", "LANG", "TZ"] {
        if let Ok(value) = std::env::var(name) {
            cmd.env(name, value);
        }
    }
    cmd.envs(&hook.env)
        .env("PUTIO_SYNC_EVENT_TYPE", event.event_type().as_str())
        .env("PUTIO_SYNC_EVENT_ID", &event.id)
        .env("PUTIO_SYNC_HOOK_NAME", &hook.name);
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
            return run;
        }
    };

    let stdout = Arc::new(Mutex::new(Capped::default()));
    let stderr = Arc::new(Mutex::new(Capped::default()));
    let readers = [
        child
            .stdout
            .take()
            .map(|p| tokio::spawn(read(p, stdout.clone()))),
        child
            .stderr
            .take()
            .map(|p| tokio::spawn(read(p, stderr.clone()))),
    ];
    let stdin = child.stdin.take();
    let json = event.to_json();

    let waited = tokio::time::timeout(hook.timeout(), async {
        if let Some(mut stdin) = stdin {
            // A program that does not read stdin closes the pipe; that is fine.
            let _ = stdin.write_all(json.as_bytes()).await;
            let _ = stdin.shutdown().await;
        }
        child.wait().await
    })
    .await;

    match waited {
        Ok(Ok(status)) => {
            run.exit_code = status.code();
            if status.success() {
                run.result = RunResult::Success;
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
            run.result = RunResult::Timeout;
            run.error = Some(format!("timed out after {} s", hook.timeout_secs));
        }
    }

    for reader in readers.into_iter().flatten() {
        let _ = tokio::time::timeout(DRAIN, reader).await;
    }
    run.stdout = stdout.lock().map(|c| c.text()).unwrap_or_default();
    run.stderr = stderr.lock().map(|c| c.text()).unwrap_or_default();
    run.duration_ms = start.elapsed().as_millis() as u64;
    run
}

async fn read(mut pipe: impl AsyncRead + Unpin, into: Arc<Mutex<Capped>>) {
    let mut buf = [0u8; 8192];
    while let Ok(n) = pipe.read(&mut buf).await {
        if n == 0 {
            break;
        }
        if let Ok(mut c) = into.lock() {
            c.push(&buf[..n]);
        }
    }
}

/// Kill the program's process group, so children it started stop too.
#[cfg(unix)]
pub(crate) fn kill_group(child: &tokio::process::Child) {
    if let Some(pid) = child.id() {
        // SAFETY: killpg only sends a signal; the group id is the child's own pid,
        // set by `process_group(0)`.
        unsafe {
            libc::killpg(pid as libc::pid_t, libc::SIGKILL);
        }
    }
}

#[cfg(not(unix))]
pub(crate) fn kill_group(_child: &tokio::process::Child) {}
