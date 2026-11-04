//! Tailscale route (feature 036)
//!
//! - GET /tailscale - What Tailscale says about this machine, the addresses that open
//!   putio-sync over the tailnet, hints, the `[tailscale]` settings, and the Tailscale
//!   identity of this request

use axum::{extract::State, routing::get, Extension, Json, Router};
use putio_sync_core::config::{Config, TailscaleConfig};
use putio_sync_tailscale::{tailnet_bind, Identity, ServeEntry, State as TsState, Status};
use serde::Serialize;
use std::net::{IpAddr, SocketAddr};

pub use crate::AppState;

/// Create the Tailscale router
pub fn router() -> Router<AppState> {
    Router::new().route("/", get(details))
}

#[derive(Serialize)]
pub struct TailscaleDetails {
    #[serde(flatten)]
    status: Status,
    /// The API port the addresses point at
    port: u16,
    /// Addresses that open putio-sync over the tailnet, HTTPS `serve` first
    urls: Vec<String>,
    hints: Vec<String>,
    /// Where "tailnet only" would listen
    tailnet_bind: Option<SocketAddr>,
    /// Why it can't
    tailnet_bind_error: Option<String>,
    settings: SettingsView,
    /// Who this request is, when Tailscale sign-in let it in
    identity: Option<Identity>,
}

#[derive(Serialize)]
struct SettingsView {
    tailnet_only: bool,
    identity_sign_in: bool,
    allowed_users: Vec<String>,
}

/// The listener address the daemon uses (or would use) for the API
pub(crate) fn api_bind(config: &Config) -> SocketAddr {
    config
        .resolve_api()
        .map(|r| r.value.bind)
        .unwrap_or_else(|_| SocketAddr::from(([0, 0, 0, 0], 15432)))
}

async fn details(
    State(state): State<AppState>,
    identity: Option<Extension<Identity>>,
) -> Json<TailscaleDetails> {
    let config = state.config.read().await.clone();
    let status = state.tailscale.status().await;
    Json(build(&config, status, identity.map(|Extension(i)| i)))
}

pub(crate) fn build(
    config: &Config,
    status: Status,
    identity: Option<Identity>,
) -> TailscaleDetails {
    let bind = api_bind(config);
    let port = bind.port();
    let ts = &config.tailscale;
    // On "tailnet only" the daemon listens on the tailnet address, not `[api] bind`
    let loopback_only = !ts.tailnet_only && bind.ip().is_loopback();
    let urls = status.urls(port, loopback_only);
    let (tailnet_bind, tailnet_bind_error) = match tailnet_bind(&status, port) {
        Ok(addr) => (Some(addr), None),
        Err(e) => (None, Some(e)),
    };
    let hints = hints(&status, ts, port, loopback_only, bind.ip());
    TailscaleDetails {
        port,
        urls,
        hints,
        tailnet_bind,
        tailnet_bind_error,
        settings: SettingsView {
            tailnet_only: ts.tailnet_only,
            identity_sign_in: ts.identity_sign_in,
            allowed_users: ts.allowed_users.clone(),
        },
        identity,
        status,
    }
}

fn hints(
    status: &Status,
    ts: &TailscaleConfig,
    port: u16,
    loopback_only: bool,
    ip: IpAddr,
) -> Vec<String> {
    let mut hints = Vec::new();
    let serve_cmd = format!("tailscale serve --bg {port}");
    match status.state {
        TsState::NotFound => hints.push(format!(
            "Tailscale isn't on this machine. In Docker, mount tailscaled's socket into the container (for example -v /var/run/tailscale:/var/run/tailscale); putio-sync looks for {}.",
            ts.socket_path().display()
        )),
        TsState::Unreachable => hints.push(
            "tailscaled didn't answer. Check that it is running and that putio-sync's user can read its socket."
                .to_string(),
        ),
        TsState::NeedsLogin => hints.push("Sign this machine in to Tailscale: tailscale up".to_string()),
        TsState::Stopped => hints.push("Tailscale is stopped. Start it with: tailscale up".to_string()),
        TsState::Starting => hints.push("Tailscale is starting; this page updates when it is up.".to_string()),
        TsState::Running => {
            let served: Vec<&ServeEntry> = status.serve_for_port(port);
            if status.userspace && served.is_empty() {
                hints.push(format!(
                    "Tailscale uses userspace networking here, so the tailnet can't reach port {port} directly. Share it with: {serve_cmd}"
                ));
            } else if status.https && served.is_empty() {
                hints.push(format!("For an HTTPS address, run: {serve_cmd}"));
            } else if !status.https {
                hints.push(
                    "HTTPS is off in this tailnet. Turn on HTTPS certificates in the Tailscale admin console (DNS page) for an https:// address."
                        .to_string(),
                );
            }
            if loopback_only && served.is_empty() && !status.userspace {
                hints.push(format!(
                    "The API listens on {ip} only, so the tailnet can't reach it directly. Share it with: {serve_cmd}"
                ));
            }
            for e in served.iter().filter(|e| e.funnel) {
                hints.push(format!(
                    "Funnel opens {} to the internet. Tailscale sign-in never applies there; the API token is needed.",
                    e.address()
                ));
            }
            if !status.magic_dns {
                hints.push("MagicDNS is off in this tailnet, so only the IP address works.".to_string());
            }
        }
    }
    if ts.identity_sign_in && ts.allowed_users.is_empty() {
        hints.push(
            "Tailscale sign-in is on, but no user is allowed, so nobody gets in that way."
                .to_string(),
        );
    }
    hints
}
