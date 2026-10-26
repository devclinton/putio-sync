//! Starts the API and web UI listener on the daemon's own engine (feature 004, R4).
//! The `server` command serves the same router itself, with its own flags.

use putio_sync_api::{router, AppState};
use putio_sync_core::config::Config;
use putio_sync_core::error::CoreError;
use std::net::SocketAddr;
use tracing::{error, info, warn};

/// Start the listener for `state` when `[api] listen` is on.
///
/// The resolved token (env override applied) is written into the state's config, so
/// the auth middleware sees it. Returns the bound address when the listener started. A
/// bind failure is logged once and the daemon carries on (FR-004); an invalid setting
/// is an error.
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

    let listener = match tokio::net::TcpListener::bind(settings.bind).await {
        Ok(listener) => listener,
        Err(e) => {
            error!(bind = %settings.bind, error = %e, "API listener failed to start");
            return Ok(None);
        }
    };
    let addr = listener.local_addr().unwrap_or(settings.bind);

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
    Ok(Some(addr))
}
