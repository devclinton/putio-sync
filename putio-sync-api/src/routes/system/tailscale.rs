//! Tailscale route (feature 036)
//!
//! - GET /tailscale - What Tailscale says about this machine, the addresses that open
//!   putio-sync over the tailnet, hints, the `[tailscale]` settings, and the Tailscale
//!   identity of this request
//!
//! Also the Radarr/Sonarr host candidates, which put the tailnet address first when
//! putio-sync listens on the tailnet only.

use axum::{extract::State, routing::get, Extension, Json, Router};
use putio_sync_core::config::{Config, TailscaleConfig};
use putio_sync_core::listen::ListenAddr;
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

/// Where Radarr/Sonarr might reach putio-sync, best first: `candidate_hosts`, with the
/// tailnet IPv4 address first when `tailnet_only` is on (nothing else answers then)
pub async fn tailnet_first_hosts(
    state: &AppState,
    browser_host: Option<&str>,
    host_override: Option<&str>,
) -> Vec<String> {
    let hosts = putio_sync_arr::setup::candidate_hosts(browser_host, host_override);
    let overridden = host_override.is_some_and(|h| !h.trim().is_empty());
    if overridden || !state.config.read().await.tailscale.tailnet_only {
        return hosts;
    }
    tailnet_first(hosts, state.tailscale.status().await.ipv4())
}

/// `hosts` with `ip` first, at most `MAX_HOSTS` long
pub(crate) fn tailnet_first(mut hosts: Vec<String>, ip: Option<IpAddr>) -> Vec<String> {
    if let Some(ip) = ip {
        let ip = ip.to_string();
        hosts.retain(|h| *h != ip);
        hosts.insert(0, ip);
        hosts.truncate(putio_sync_arr::setup::MAX_HOSTS);
    }
    hosts
}

async fn details(
    State(state): State<AppState>,
    identity: Option<Extension<Identity>>,
) -> Json<TailscaleDetails> {
    let config = state.config.read().await.clone();
    let status = state.tailscale.status().await;
    let port = state.api_port().await;
    Json(build(&config, status, identity.map(|Extension(i)| i), port))
}

pub(crate) fn build(
    config: &Config,
    status: Status,
    identity: Option<Identity>,
    port: u16,
) -> TailscaleDetails {
    let ts = &config.tailscale;
    // The effective list: on "tailnet only" it holds tailnet entries, never loopback
    let entries = config.api_listen().unwrap_or_default();
    let loopback_only = !entries.is_empty() && entries.iter().all(ListenAddr::is_loopback);
    let on = entries
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let urls = status.urls(port, loopback_only);
    let (tailnet_bind, tailnet_bind_error) = match tailnet_bind(&status, port) {
        Ok(addr) => (Some(addr), None),
        Err(e) => (None, Some(e)),
    };
    let hints = hints(&status, ts, port, loopback_only, &on);
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
    on: &str,
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
                    "The API listens on {on} only, so the tailnet can't reach it directly. Share it with: {serve_cmd}"
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tailnet_address_goes_first() {
        let hosts: Vec<String> = [
            "putio-sync",
            "nas",
            "192.168.1.2",
            "172.17.0.1",
            "100.101.102.103",
        ]
        .map(String::from)
        .to_vec();
        let ip = Some(IpAddr::from([100, 101, 102, 103]));
        assert_eq!(
            tailnet_first(hosts.clone(), ip),
            [
                "100.101.102.103",
                "putio-sync",
                "nas",
                "192.168.1.2",
                "172.17.0.1"
            ]
        );
        assert_eq!(tailnet_first(hosts.clone(), None), hosts);
        let ip = Some(IpAddr::from([100, 64, 0, 1]));
        assert_eq!(
            tailnet_first(hosts, ip).len(),
            putio_sync_arr::setup::MAX_HOSTS
        );
    }
}
