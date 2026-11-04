//! Setup mode (feature 013): instead of exiting when it has no usable config, the daemon
//! prints a one-time setup code and serves the setup wizard on its API address until
//! the config is complete.

use anyhow::Result;
use putio_sync_api::{setup_router, SetupState};
use putio_sync_client::{PutioApiClient, DEFAULT_BASE_URL};
use putio_sync_core::config::Config;
use putio_sync_core::setup::{check_local, SetupReason};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};
use tracing::{error, info, warn};

/// How often setup mode looks at the config file for changes made by hand
const POLL: Duration = Duration::from_secs(5);

/// How long to wait for the API port to be free (a previous listener may linger)
const BIND_RETRY: Duration = Duration::from_secs(5);

/// Why setup is needed for the config at `path`, with the config when it parsed, or
/// `None` when the daemon can start. Asks Put.io once: a rejected token needs setup,
/// an unreachable Put.io does not (the daemon retries as usual).
pub async fn needs_setup(path: &Path) -> Option<(Option<Config>, Vec<SetupReason>)> {
    needs_setup_at(path, DEFAULT_BASE_URL).await
}

async fn needs_setup_at(
    path: &Path,
    putio_base_url: &str,
) -> Option<(Option<Config>, Vec<SetupReason>)> {
    let local = check_local(path);
    if !local.reasons.is_empty() {
        return Some((local.config, local.reasons));
    }
    // App-password auth has no token to check; the daemon reports it as before
    let token = local.putio_token()?.to_string();
    let probe = PutioApiClient::with_base_url(token, putio_base_url.to_string())
        .probe_account()
        .await;
    if probe.unauthorized() {
        return Some((local.config, vec![SetupReason::CredentialsRejected]));
    }
    None
}

/// Wait in setup mode until the config is complete. Returns `false` when stopped with
/// Ctrl-C or SIGTERM instead.
pub async fn run(path: &Path, base: Option<Config>, reasons: Vec<SetupReason>) -> Result<bool> {
    let described: Vec<String> = reasons.iter().map(SetupReason::describe).collect();
    warn!(reasons = ?described, config = %path.display(), "Setup needed; waiting in setup mode");
    putio_sync_telemetry::setup::set_required(true);

    let api = base
        .as_ref()
        .and_then(|c| c.resolve_api().ok())
        .or_else(|| Config::default().resolve_api().ok())
        .map(|r| r.value);
    let listen = api.as_ref().is_none_or(|a| a.listen);
    let state = SetupState::new(path.to_path_buf(), base, reasons);

    let finished = if listen {
        let bind = api
            .map(|a| a.bind)
            .unwrap_or_else(|| SocketAddr::from(([0, 0, 0, 0], 15432)));
        serve(path, state, bind, &described).await?
    } else {
        banner_without_listener(path, &described);
        wait(path, None).await
    };

    putio_sync_telemetry::setup::set_required(false);
    if finished {
        info!("Setup complete; starting");
    }
    Ok(finished)
}

/// Serve the wizard on `bind` until setup is finished, the file is fixed, or a signal
async fn serve(
    path: &Path,
    state: SetupState,
    bind: SocketAddr,
    reasons: &[String],
) -> Result<bool> {
    let listener = match bind_with_retry(bind).await {
        Ok(listener) => listener,
        Err(e) => {
            error!(bind = %bind, error = %e, "Setup mode could not listen; edit the config file or run `putio-sync setup`");
            banner_without_listener(path, reasons);
            return Ok(wait(path, None).await);
        }
    };
    let addr = listener.local_addr().unwrap_or(bind);
    banner(path, reasons, addr, &state);

    let app = setup_router(state.clone()).into_make_service_with_connect_info::<SocketAddr>();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        let shutdown = async {
            let _ = stop_rx.await;
        };
        if let Err(e) = axum::serve(listener, app)
            .with_graceful_shutdown(shutdown)
            .await
        {
            error!(error = %e, "Setup listener stopped");
        }
    });

    let finished = wait(path, Some(&state)).await;
    let _ = stop_tx.send(());
    // Open connections get a moment to finish; then the port is freed for the API
    let abort = server.abort_handle();
    if tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .is_err()
    {
        abort.abort();
    }
    Ok(finished)
}

/// Until setup finishes (`true`), the config file changes into one that needs no setup
/// (`true`), or a stop signal (`false`)
async fn wait(path: &Path, state: Option<&SetupState>) -> bool {
    let finished = async {
        match state {
            Some(state) => state.finished().await,
            None => std::future::pending().await,
        }
    };
    tokio::select! {
        _ = finished => true,
        _ = poll_file(path) => true,
        _ = stop_signal() => {
            info!("Stopped in setup mode");
            false
        }
    }
}

/// Resolves when the file at `path` changed and no longer needs setup
async fn poll_file(path: &Path) {
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let mut seen: Option<SystemTime> = modified(path);
    loop {
        tokio::time::sleep(POLL).await;
        let now = modified(path);
        if now == seen {
            continue;
        }
        seen = now;
        if needs_setup(path).await.is_none() {
            info!("Config file fixed by hand; leaving setup mode");
            return;
        }
    }
}

async fn stop_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term = match signal(SignalKind::terminate()) {
            Ok(term) => term,
            Err(_) => {
                let _ = tokio::signal::ctrl_c().await;
                return;
            }
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

async fn bind_with_retry(bind: SocketAddr) -> std::io::Result<tokio::net::TcpListener> {
    let started = Instant::now();
    loop {
        match tokio::net::TcpListener::bind(bind).await {
            Err(e)
                if e.kind() == std::io::ErrorKind::AddrInUse && started.elapsed() < BIND_RETRY =>
            {
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
            other => return other,
        }
    }
}

/// The setup code goes to the console only, never to the log
fn banner(path: &Path, reasons: &[String], addr: SocketAddr, state: &SetupState) {
    let host = if addr.ip().is_unspecified() {
        "localhost".to_string()
    } else {
        addr.ip().to_string()
    };
    eprintln!();
    eprintln!("  putio-sync needs setting up: {}", reasons.join("; "));
    eprintln!(
        "  Open http://{}:{}/ and enter the setup code:",
        host,
        addr.port()
    );
    eprintln!();
    eprintln!("      {}", state.code());
    eprintln!();
    if state.api_token_configured() {
        eprintln!("  Your API token also unlocks the setup.");
    }
    eprintln!(
        "  Or run `putio-sync setup --config {}` in a terminal.",
        shown(path)
    );
    eprintln!();
}

fn banner_without_listener(path: &Path, reasons: &[String]) {
    eprintln!();
    eprintln!("  putio-sync needs setting up: {}", reasons.join("; "));
    eprintln!(
        "  The web UI is off, so run `putio-sync setup --config {}`",
        shown(path)
    );
    eprintln!("  or edit the config file. Waiting for it to change...");
    eprintln!();
}

fn shown(path: &Path) -> String {
    PathBuf::from(path).display().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, config: &Config) -> PathBuf {
        let path = dir.join("config.toml");
        config.save(&path).unwrap();
        path
    }

    fn complete(dir: &Path) -> Config {
        let mut config = Config {
            auth: Some(putio_sync_core::config::AuthConfig::with_token(
                "tok".to_string(),
            )),
            ..Config::default()
        };
        config.download.target_directory = dir.to_path_buf();
        config
    }

    fn reasons(found: Option<(Option<Config>, Vec<SetupReason>)>) -> Vec<SetupReason> {
        found.map(|(_, r)| r).unwrap_or_default()
    }

    #[tokio::test]
    async fn missing_and_invalid_configs_need_setup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let found = needs_setup_at(&path, "http://127.0.0.1:9").await;
        assert_eq!(reasons(found), vec![SetupReason::NoConfig]);

        std::fs::write(&path, "[download\nnot toml").unwrap();
        let found = needs_setup_at(&path, "http://127.0.0.1:9").await;
        assert!(matches!(
            reasons(found).as_slice(),
            [SetupReason::InvalidConfig { .. }]
        ));
    }

    #[tokio::test]
    async fn no_token_and_unusable_folder_need_setup() {
        let dir = tempfile::tempdir().unwrap();
        let mut config = complete(dir.path());
        config.auth = None;
        let file = std::path::PathBuf::from(dir.path()).join("a-file");
        std::fs::write(&file, b"x").unwrap();
        config.download.target_directory = file.clone();
        let path = write(dir.path(), &config);
        let found = reasons(needs_setup_at(&path, "http://127.0.0.1:9").await);
        assert!(found.contains(&SetupReason::NoCredentials));
        assert!(found.contains(&SetupReason::FolderNotWritable {
            path: file.display().to_string()
        }));
    }

    #[tokio::test]
    async fn a_rejected_token_needs_setup() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/account/info")
            .with_status(401)
            .with_body(r#"{"status":"ERROR","error_type":"invalid_grant"}"#)
            .create_async()
            .await;
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), &complete(dir.path()));
        let found = needs_setup_at(&path, &server.url()).await;
        assert_eq!(reasons(found), vec![SetupReason::CredentialsRejected]);
    }

    #[tokio::test]
    async fn a_complete_config_or_an_unreachable_putio_needs_none() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/account/info")
            .with_body(r#"{"status":"OK","info":{"user_id":1,"username":"a","mail":"a@b","account_active":true,"disk":{"size":10,"used":1,"avail":9},"avatar_url":null}}"#)
            .create_async()
            .await;
        let dir = tempfile::tempdir().unwrap();
        let path = write(dir.path(), &complete(dir.path()));
        assert!(needs_setup_at(&path, &server.url()).await.is_none());
        // Nothing listens on port 9: a network error is not a reason for setup
        assert!(needs_setup_at(&path, "http://127.0.0.1:9").await.is_none());
    }
}
