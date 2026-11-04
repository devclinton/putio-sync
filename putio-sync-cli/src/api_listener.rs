//! Starts the API and web UI listener on the daemon's own engine (feature 004, R4).
//! The `server` command serves the same router itself, with its own flags.
//!
//! With `[tailscale] tailnet_only` (feature 036) the listener takes the tailnet IPv4
//! address instead of `[api] bind`. When Tailscale isn't ready it waits in the
//! background and never falls back to another address.

use putio_sync_api::{router, spawn_arr_refresh, AppState};
use putio_sync_core::config::Config;
use putio_sync_core::error::CoreError;
use putio_sync_core::logging::LevelSource;
use putio_sync_tailscale::{tailnet_bind, TailscaleService};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::time::Instant;
use tracing::{error, info, warn};

/// How often "tailnet only" asks Tailscale again while it isn't ready
const TAILNET_RETRY: Duration = Duration::from_secs(5);
/// How long it keeps asking
const TAILNET_WAIT: Duration = Duration::from_secs(5 * 60);
/// How often the wait is logged
const TAILNET_LOG: Duration = Duration::from_secs(60);

/// Start the listener for `state` when `[api] listen` is on.
///
/// The resolved token (env override applied) is written into the state's config, so
/// the auth middleware sees it. Returns the bound address when the listener started. A
/// bind failure is logged once and the daemon carries on (FR-004); an invalid setting
/// is an error. With "tailnet only" and Tailscale not ready, it returns `None` and the
/// listener starts later, once Tailscale gives an address.
pub async fn start(config: &Config, state: AppState) -> Result<Option<SocketAddr>, CoreError> {
    let resolved = config.resolve_api()?;
    let settings = resolved.value;
    let source = resolved.source;

    if !settings.listen {
        info!(source = ?source, "API listener disabled");
        return Ok(None);
    }

    let token_set = settings.token.is_some();
    if !token_set {
        warn!("API token not configured (set [api] auth_token or PUTIO_SYNC_API_TOKEN); all API and UI requests will be refused");
    }
    state.config.write().await.api.auth_token = settings.token;

    let bind = if config.tailscale.tailnet_only {
        let port = settings.bind.port();
        match tailnet_bind(&state.tailscale.refresh().await, port) {
            Ok(addr) => addr,
            Err(reason) => {
                warn!(reason = %reason, "Tailnet only: waiting for Tailscale before the API listens");
                tokio::spawn(async move {
                    let tailscale = state.tailscale.clone();
                    match wait_for_tailnet(&tailscale, port, TAILNET_RETRY, TAILNET_WAIT).await {
                        Ok(addr) => {
                            serve(addr, state, token_set, source).await;
                        }
                        Err(reason) => error!(
                            reason = %reason,
                            "Tailnet only: Tailscale gave no address in 5 minutes; the API isn't listening. Fix Tailscale and restart putio-sync"
                        ),
                    }
                });
                return Ok(None);
            }
        }
    } else {
        settings.bind
    };
    Ok(serve(bind, state, token_set, source).await)
}

/// Bind `bind` and serve the router on it; `None` (logged) when the bind fails
async fn serve(
    bind: SocketAddr,
    state: AppState,
    token_set: bool,
    source: LevelSource,
) -> Option<SocketAddr> {
    let listener = match tokio::net::TcpListener::bind(bind).await {
        Ok(listener) => listener,
        Err(e) => {
            error!(bind = %bind, error = %e, "API listener failed to start");
            return None;
        }
    };
    let addr = listener.local_addr().unwrap_or(bind);
    spawn_arr_refresh(&state).await;

    let app = router(state).into_make_service_with_connect_info::<SocketAddr>();
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            error!(error = %e, "API listener stopped");
        }
    });
    info!(
        bind = %addr,
        token = if token_set { "set" } else { "unset" },
        source = ?source,
        "API listener enabled"
    );
    Some(addr)
}

/// Ask Tailscale for the tailnet address every `retry` until `give_up`, logging once a
/// minute. The error is the last reason there was none.
async fn wait_for_tailnet(
    tailscale: &TailscaleService,
    port: u16,
    retry: Duration,
    give_up: Duration,
) -> Result<SocketAddr, String> {
    let started = Instant::now();
    let mut logged = started;
    loop {
        let reason = match tailnet_bind(&tailscale.refresh().await, port) {
            Ok(addr) => return Ok(addr),
            Err(reason) => reason,
        };
        if started.elapsed() >= give_up {
            return Err(reason);
        }
        if logged.elapsed() >= TAILNET_LOG {
            logged = Instant::now();
            warn!(reason = %reason, waited_secs = started.elapsed().as_secs(), "Tailnet only: still waiting for Tailscale");
        }
        tokio::time::sleep(retry).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use putio_sync_tailscale::Cli;

    #[tokio::test]
    async fn waiting_gives_up_without_falling_back() {
        let dir = tempfile::tempdir().unwrap();
        let tailscale = TailscaleService::new(dir.path().join("none.sock"), Cli::Off)
            .with_timeout(Duration::from_millis(100));
        let started = std::time::Instant::now();
        let result = wait_for_tailnet(
            &tailscale,
            15432,
            Duration::from_millis(10),
            Duration::from_millis(60),
        )
        .await;
        assert!(result.unwrap_err().contains("isn't on"));
        assert!(started.elapsed() >= Duration::from_millis(60));
    }

    #[tokio::test]
    async fn waiting_returns_the_tailnet_address_once_tailscale_is_up() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("tailscaled.sock");
        let tailscale =
            TailscaleService::new(&socket, Cli::Off).with_timeout(Duration::from_millis(200));
        // Tailscale comes up a little after the wait starts
        let up = socket.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let listener = tokio::net::UnixListener::bind(&up).unwrap();
            while let Ok((mut conn, _)) = listener.accept().await {
                let mut buf = [0u8; 2048];
                let _ = conn.read(&mut buf).await;
                let body =
                    r#"{"BackendState":"Running","TUN":true,"TailscaleIPs":["100.101.102.103"]}"#;
                let _ = conn
                    .write_all(format!("HTTP/1.0 200 OK\r\n\r\n{body}").as_bytes())
                    .await;
            }
        });
        let addr = wait_for_tailnet(
            &tailscale,
            15432,
            Duration::from_millis(20),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
        assert_eq!(addr, SocketAddr::from(([100, 101, 102, 103], 15432)));
    }
}
