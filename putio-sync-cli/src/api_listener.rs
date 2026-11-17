//! Starts the API and web UI listener on the daemon's own engine (feature 004, R4).
//!
//! It listens on [`Config::api_listen`] through the API crate's supervisor (feature
//! 037): every listed address, interface and tailnet entries followed as their addresses
//! change. With `[tailscale] tailnet_only` (feature 036) the list is the tailnet address
//! on each port. An entry with no address waits; nothing falls back to another address.

use putio_sync_api::listen::{Bound, Supervisor};
use putio_sync_api::{router, spawn_arr_refresh, AppState};
use putio_sync_core::config::Config;
use putio_sync_core::error::CoreError;
use std::time::Duration;
use tracing::{info, warn};

/// How long `start` waits for a first address before returning
const FIRST_BIND: Duration = Duration::from_secs(2);

/// Start the listener for `state` when `[api] listen` is on.
///
/// The resolved token (env override applied) is written into the state's config, so
/// the auth middleware sees it. Returns what the listener holds (filled in the
/// background), or `None` when `[api] listen` is off. A bind failure is logged and
/// retried, and the daemon carries on (FR-004); an invalid setting is an error.
pub async fn start(config: &Config, state: AppState) -> Result<Option<Bound>, CoreError> {
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

    let entries = config.api_listen()?;
    let shown: Vec<String> = entries.iter().map(ToString::to_string).collect();
    let bound = Bound::default();
    let state = state.with_listening(bound.clone());
    let handle = Supervisor::spawn(
        "API",
        entries,
        state.tailscale.clone(),
        router(state.clone()),
        bound.clone(),
    );
    handle.first_bound(FIRST_BIND).await;
    // After the first bind, so Radarr/Sonarr hear the port actually listened on
    spawn_arr_refresh(&state).await;
    info!(
        bind = %shown.join(", "),
        tailnet_only = config.tailscale.tailnet_only,
        token = if token_set { "set" } else { "unset" },
        source = ?source,
        "API listener enabled"
    );
    Ok(Some(bound))
}

// The fake tailscaled listens on a unix socket
#[cfg(all(test, unix))]
mod tests {
    use putio_sync_api::listen::{resolve_all, Bound, Gives, Schedule, Supervisor};
    use putio_sync_core::listen::ListenAddr;
    use putio_sync_tailscale::{Cli, TailscaleService};
    use std::net::SocketAddr;
    use std::sync::Arc;
    use std::time::Duration;

    /// A fake tailscaled on `socket` that answers Running with 100.101.102.103
    fn fake_tailscaled(socket: std::path::PathBuf, after: Duration) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        tokio::spawn(async move {
            tokio::time::sleep(after).await;
            let listener = tokio::net::UnixListener::bind(&socket).unwrap();
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
    }

    #[tokio::test]
    async fn tailnet_waits_then_gives_the_tailnet_address() {
        let dir = short_dir();
        let socket = dir.path().join("tailscaled.sock");
        let tailscale =
            TailscaleService::new(&socket, Cli::Off).with_timeout(Duration::from_millis(200));
        let entries = [ListenAddr::Tailnet { port: 15432 }];
        match &resolve_all(&entries, &tailscale).await[0] {
            Gives::Waiting(why) => assert!(why.contains("isn't on"), "{why}"),
            other => panic!("expected waiting, got {other:?}"),
        }
        fake_tailscaled(socket, Duration::ZERO);
        let mut got = None;
        for _ in 0..100 {
            if let Gives::Addrs(a) = &resolve_all(&entries, &tailscale).await[0] {
                got = Some(a.clone());
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(
            got.unwrap(),
            vec![SocketAddr::from(([100, 101, 102, 103], 15432))]
        );
    }

    #[tokio::test]
    async fn waiting_tailnet_never_falls_back() {
        let dir = short_dir();
        let tailscale = Arc::new(
            TailscaleService::new(dir.path().join("none.sock"), Cli::Off)
                .with_timeout(Duration::from_millis(100)),
        );
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let resolver: putio_sync_api::listen::Resolver = Arc::new(move |entries| {
            let tailscale = tailscale.clone();
            Box::pin(async move { resolve_all(&entries, &tailscale).await })
        });
        let h = Supervisor::spawn_with(
            "API",
            vec![ListenAddr::Tailnet { port }],
            resolver,
            axum::Router::new(),
            Bound::default(),
            Schedule {
                fast: Duration::from_millis(20),
                fast_for: Duration::from_millis(100),
                slow: Duration::from_millis(20),
            },
        );
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(h.bound.get().is_empty());
        // Nothing took the port on any address
        assert!(std::net::TcpListener::bind(("0.0.0.0", port)).is_ok());
        h.stop(Duration::from_millis(100)).await;
    }

    /// A temp dir under /tmp: unix socket paths must stay under about 104 bytes, which a
    /// long `TMPDIR` breaks
    fn short_dir() -> tempfile::TempDir {
        tempfile::Builder::new()
            .prefix("ts")
            .tempdir_in("/tmp")
            .unwrap()
    }
}
