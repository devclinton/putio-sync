//! Listen addresses (feature 037)
//!
//! - GET  /listen - What this process listens on now, entry by entry
//! - GET  /settings/network - The saved `[api] bind` and `[metrics] bind` lists
//! - PUT  /settings/network - Save them (refused when no API entry gives an address now)
//! - POST /settings/network/preview - The assistant's preview of a PUT
//! - POST /settings/network/check - What a PUT would listen on, and whether the page
//!   making the request would still reach putio-sync; never saves

use super::settings::{body, invalid, preview, saved_message, update};
use crate::error::ApiError;
use crate::listen::{resolve_all, Gives, ListenedOn};
use crate::models::SuccessResponse;
use crate::{AppState, Engine};
use axum::{
    extract::{rejection::JsonRejection, State},
    http::{header, HeaderMap},
    routing::{get, post, put},
    Extension, Json, Router,
};
use putio_sync_core::config::{Config, DEFAULT_API_BIND};
use putio_sync_core::listen::{BindList, ListenAddr};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, SocketAddr};

const API_BIND_VAR: &str = "PUTIO_SYNC_API_BIND";
const METRICS_BIND_VAR: &str = "PUTIO_SYNC_METRICS_BIND";

/// Shown when identity sign-in is on and the API listens on loopback
pub const LOOPBACK_IDENTITY_WARNING: &str = "Only `tailscale serve` should reach putio-sync on loopback: another local proxy could forward forged identity headers.";

/// The network settings group: both lists as written
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct NetworkSettings {
    pub api_bind: Vec<String>,
    pub metrics_bind: Vec<String>,
}

pub(crate) fn network_view(config: &Config) -> NetworkSettings {
    NetworkSettings {
        api_bind: config
            .api
            .bind
            .clone()
            .unwrap_or_else(|| BindList::from(DEFAULT_API_BIND))
            .entries(),
        metrics_bind: config.metrics.bind.entries(),
    }
}

/// Apply a network body to `config` (shared by PUT, preview and check); blank entries
/// are dropped, and `Config::validate` checks the rest
fn apply_network(settings: NetworkSettings, config: &mut Config) -> Result<(), ApiError> {
    let clean = |list: Vec<String>| -> Vec<String> {
        list.into_iter()
            .map(|e| e.trim().to_string())
            .filter(|e| !e.is_empty())
            .collect()
    };
    config.api.bind = Some(BindList::from_entries(clean(settings.api_bind)));
    config.metrics.bind = BindList::from_entries(clean(settings.metrics_bind));
    Ok(())
}

/// One entry and what it gives now
#[derive(Debug, Serialize)]
pub struct EntryView {
    pub entry: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub addresses: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waiting: Option<String>,
}

fn entry_views(entries: &[ListenAddr], gives: Vec<Gives>) -> Vec<EntryView> {
    entries
        .iter()
        .zip(gives)
        .map(|(entry, gives)| match gives {
            Gives::Addrs(addrs) => EntryView {
                entry: entry.to_string(),
                addresses: Some(addrs.iter().map(SocketAddr::to_string).collect()),
                waiting: None,
            },
            Gives::Waiting(why) => EntryView {
                entry: entry.to_string(),
                addresses: None,
                waiting: Some(why),
            },
        })
        .collect()
}

fn strings(list: &[ListenAddr]) -> Vec<String> {
    list.iter().map(ListenAddr::to_string).collect()
}

fn env_set(name: &str) -> bool {
    std::env::var_os(name).is_some()
}

#[derive(Debug, Serialize)]
pub struct ApiListen {
    pub configured: Vec<String>,
    pub effective: Vec<String>,
    pub listening: Vec<String>,
    pub entries: Vec<EntryView>,
    pub port: u16,
    pub tailnet_only: bool,
    pub env_override: bool,
}

#[derive(Debug, Serialize)]
pub struct MetricsListenView {
    pub enabled: bool,
    pub configured: Vec<String>,
    pub effective: Vec<String>,
    pub entries: Vec<EntryView>,
    pub env_override: bool,
}

#[derive(Debug, Serialize)]
pub struct ListenResponse {
    pub api: ApiListen,
    pub metrics: MetricsListenView,
    pub engine: &'static str,
    pub server_flags: bool,
}

/// What this process listens on now
///
/// GET /listen
pub async fn get_listen(State(state): State<AppState>) -> Json<ListenResponse> {
    let config = state.config.read().await.clone();
    let view = network_view(&config);
    let effective = match &state.listen_flags {
        Some(flags) => flags.clone(),
        None => config.api_listen().unwrap_or_default(),
    };
    let api_entries = entry_views(&effective, resolve_all(&effective, &state.tailscale).await);
    let (metrics_enabled, metrics_effective) = match config.resolve_metrics() {
        Ok(r) => (r.value.settings.enabled, r.value.bind),
        Err(_) => (config.metrics.enabled, Vec::new()),
    };
    let metrics_entries = entry_views(
        &metrics_effective,
        resolve_all(&metrics_effective, &state.tailscale).await,
    );
    Json(ListenResponse {
        api: ApiListen {
            configured: view.api_bind,
            effective: strings(&effective),
            listening: state
                .listening
                .get()
                .iter()
                .map(SocketAddr::to_string)
                .collect(),
            entries: api_entries,
            port: state.api_port().await,
            tailnet_only: config.tailscale.tailnet_only,
            env_override: env_set(API_BIND_VAR),
        },
        metrics: MetricsListenView {
            enabled: metrics_enabled,
            configured: view.metrics_bind,
            effective: strings(&metrics_effective),
            entries: metrics_entries,
            env_override: env_set(METRICS_BIND_VAR),
        },
        engine: match state.engine {
            Engine::Daemon => "daemon",
            Engine::Server => "server",
        },
        server_flags: state.listen_flags.is_some(),
    })
}

/// GET /settings/network
async fn get_network_settings(State(state): State<AppState>) -> Json<NetworkSettings> {
    Json(network_view(&*state.config.read().await))
}

/// A candidate config with `settings` applied and validated, the API list it listens
/// on (saved list, tailnet-only applied) and what each API entry gives now
struct Assessed {
    candidate: Config,
    api: Vec<ListenAddr>,
    api_gives: Vec<Gives>,
}

async fn assess(state: &AppState, settings: NetworkSettings) -> Result<Assessed, ApiError> {
    let mut candidate = state.config.read().await.clone();
    apply_network(settings, &mut candidate)?;
    candidate.validate().map_err(invalid)?;
    let api = candidate.with_tailnet_only(candidate.api_bind_saved().map_err(invalid)?);
    let api_gives = resolve_all(&api, &state.tailscale).await;
    Ok(Assessed {
        candidate,
        api,
        api_gives,
    })
}

/// The 400 for a list that gives nothing now, naming each entry and why
fn nothing_now(api: &[ListenAddr], gives: &[Gives]) -> Option<ApiError> {
    if gives
        .iter()
        .any(|g| matches!(g, Gives::Addrs(a) if !a.is_empty()))
    {
        return None;
    }
    let why: Vec<String> = api
        .iter()
        .zip(gives)
        .map(|(entry, g)| match g {
            Gives::Waiting(why) => format!("{entry} ({why})"),
            Gives::Addrs(_) => entry.to_string(),
        })
        .collect();
    Some(ApiError::BadRequest(format!(
        "No API address gives an address now: {}",
        why.join(", ")
    )))
}

/// Update the listen addresses
///
/// PUT /settings/network
///
/// Refused unless at least one API entry gives an address now, so a save can't leave
/// putio-sync with nothing to listen on after a restart.
async fn update_network_settings(
    State(state): State<AppState>,
    settings: Result<Json<NetworkSettings>, JsonRejection>,
) -> Result<Json<SuccessResponse>, ApiError> {
    let settings = body(settings)?;
    let candidate = NetworkSettings {
        api_bind: settings.api_bind.clone(),
        metrics_bind: settings.metrics_bind.clone(),
    };
    let assessed = assess(&state, candidate).await?;
    if let Some(e) = nothing_now(&assessed.api, &assessed.api_gives) {
        return Err(e);
    }
    let changed = update(&state, network_view, |config| {
        apply_network(settings, config)
    })
    .await?;

    let mut notes = Vec::new();
    if env_set(API_BIND_VAR) {
        notes.push(format!(
            "{API_BIND_VAR} is set and overrides the saved API addresses."
        ));
    }
    if env_set(METRICS_BIND_VAR) {
        notes.push(format!(
            "{METRICS_BIND_VAR} is set and overrides the saved metrics addresses."
        ));
    }
    if assessed.candidate.tailscale.tailnet_only {
        notes.push(
            "Tailnet only is on: the API listens on the tailnet address at the saved ports."
                .to_string(),
        );
    }
    if state.listen_flags.is_some() {
        notes.push(
            "This `server` was started with --bind or --port, which override the saved API addresses."
                .to_string(),
        );
    }
    Ok(Json(SuccessResponse {
        message: saved_message("Network", &changed, &[], "", &notes),
    }))
}

/// POST /settings/network/preview
async fn preview_network(
    State(state): State<AppState>,
    settings: Result<Json<NetworkSettings>, JsonRejection>,
) -> Result<Json<super::assistant::Preview>, ApiError> {
    let settings = body(settings)?;
    Ok(preview(&state, network_view, (&[], ""), |c| {
        apply_network(settings, c)
    })
    .await)
}

/// The check's answer
#[derive(Debug, Default, Serialize)]
pub struct NetworkCheck {
    pub valid: bool,
    pub error: Option<String>,
    pub api: Vec<EntryView>,
    pub metrics: Vec<EntryView>,
    /// `kept`, `cut_off` or `unknown`
    pub this_page: &'static str,
    pub listened_on: Option<String>,
    pub ui_at: Vec<String>,
    pub warnings: Vec<String>,
}

/// What a save of this body would listen on, and whether this page would still reach
/// putio-sync; never saves
///
/// POST /settings/network/check
async fn check_network(
    State(state): State<AppState>,
    listened_on: Option<Extension<ListenedOn>>,
    headers: HeaderMap,
    settings: Result<Json<NetworkSettings>, JsonRejection>,
) -> Result<Json<NetworkCheck>, ApiError> {
    let settings = body(settings)?;
    let listened_on = listened_on.map(|Extension(ListenedOn(addr))| addr);
    let host = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .map(host_name);
    let refused = |e: ApiError| NetworkCheck {
        valid: false,
        error: Some(error_text(e)),
        this_page: "unknown",
        listened_on: listened_on.map(|a| a.to_string()),
        ..NetworkCheck::default()
    };
    let assessed = match assess(&state, settings).await {
        Ok(a) => a,
        Err(e) => return Ok(Json(refused(e))),
    };
    let nothing = nothing_now(&assessed.api, &assessed.api_gives);
    let new_addrs: Vec<SocketAddr> = assessed
        .api_gives
        .iter()
        .flat_map(|g| match g {
            Gives::Addrs(a) => a.clone(),
            Gives::Waiting(_) => Vec::new(),
        })
        .collect();
    let metrics = assessed
        .candidate
        .metrics
        .bind
        .parse("metrics.bind")
        .unwrap_or_default();
    let metrics_gives = resolve_all(&metrics, &state.tailscale).await;

    let mut warnings = Vec::new();
    if assessed.candidate.tailscale.identity_sign_in
        && assessed.api.iter().any(ListenAddr::is_loopback)
    {
        warnings.push(LOOPBACK_IDENTITY_WARNING.to_string());
    }

    Ok(Json(NetworkCheck {
        valid: nothing.is_none(),
        error: nothing.map(error_text),
        this_page: this_page(listened_on, host.as_deref(), &new_addrs),
        listened_on: listened_on.map(|a| a.to_string()),
        ui_at: ui_at(&new_addrs, host.as_deref()),
        api: entry_views(&assessed.api, assessed.api_gives),
        metrics: entry_views(&metrics, metrics_gives),
        warnings,
    }))
}

fn error_text(e: ApiError) -> String {
    match e {
        ApiError::BadRequest(msg) => msg,
        other => other.to_string(),
    }
}

/// The host part of a `Host` header: `name`, `1.2.3.4` or `::1` (brackets dropped)
fn host_name(host: &str) -> String {
    if let Some(rest) = host.strip_prefix('[') {
        return rest.split(']').next().unwrap_or_default().to_string();
    }
    match host.rsplit_once(':') {
        Some((name, port)) if port.chars().all(|c| c.is_ascii_digit()) => name.to_string(),
        _ => host.to_string(),
    }
}

/// Whether `addrs` takes connections to `target`: the address itself, or the wildcard
/// of its family on its port
fn covers(addrs: &[SocketAddr], target: SocketAddr) -> bool {
    addrs.iter().any(|a| {
        *a == target
            || (a.port() == target.port()
                && a.ip().is_unspecified()
                && a.is_ipv4() == target.is_ipv4())
    })
}

/// Would the page that sent the request still reach putio-sync on `addrs`? The listener
/// it came in on says the address, unless that was a wildcard: then the `Host` header
/// does when it is an IP, and a name says nothing for sure.
fn this_page(
    listened_on: Option<SocketAddr>,
    host: Option<&str>,
    addrs: &[SocketAddr],
) -> &'static str {
    let Some(local) = listened_on else {
        return "unknown";
    };
    let target = if local.ip().is_unspecified() {
        match host.and_then(|h| h.parse::<IpAddr>().ok()) {
            Some(ip) => SocketAddr::new(ip, local.port()),
            None => {
                // Reached by name: only the same wildcard surely keeps it
                return if addrs.contains(&local) {
                    "kept"
                } else {
                    "unknown"
                };
            }
        }
    } else {
        local
    };
    if covers(addrs, target) {
        "kept"
    } else {
        "cut_off"
    }
}

/// `http://` URLs the new list serves the UI on; a wildcard shows the `Host` name
fn ui_at(addrs: &[SocketAddr], host: Option<&str>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for addr in addrs {
        let host = if addr.ip().is_unspecified() {
            host.unwrap_or("localhost").to_string()
        } else {
            addr.ip().to_string()
        };
        let host = if host.contains(':') {
            format!("[{host}]")
        } else {
            host
        };
        let url = format!("http://{host}:{}/", addr.port());
        if !out.contains(&url) {
            out.push(url);
        }
    }
    out
}

/// The `/settings/network*` routes, merged into the settings router
pub(crate) fn settings_router() -> Router<AppState> {
    Router::new()
        .route("/network", get(get_network_settings))
        .route("/network", put(update_network_settings))
        .route("/network/preview", post(preview_network))
        .route("/network/check", post(check_network))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(s: &str) -> SocketAddr {
        s.parse().unwrap()
    }

    #[test]
    fn host_names() {
        assert_eq!(host_name("nas:15432"), "nas");
        assert_eq!(host_name("nas"), "nas");
        assert_eq!(host_name("[::1]:15432"), "::1");
        assert_eq!(host_name("10.0.0.2:15432"), "10.0.0.2");
    }

    #[test]
    fn this_page_from_listener_and_host() {
        let new = [a("192.168.1.20:15432")];
        assert_eq!(this_page(None, None, &new), "unknown");
        assert_eq!(this_page(Some(a("192.168.1.20:15432")), None, &new), "kept");
        assert_eq!(this_page(Some(a("127.0.0.1:15432")), None, &new), "cut_off");
        // A wildcard listener: the Host IP decides, a name can't
        let any = a("0.0.0.0:15432");
        assert_eq!(this_page(Some(any), Some("192.168.1.20"), &new), "kept");
        assert_eq!(this_page(Some(any), Some("10.0.0.9"), &new), "cut_off");
        assert_eq!(this_page(Some(any), Some("nas"), &new), "unknown");
        assert_eq!(this_page(Some(any), Some("nas"), &[any]), "kept");
        // The new list's wildcard covers a specific address of its family
        assert_eq!(this_page(Some(a("127.0.0.1:15432")), None, &[any]), "kept");
        assert_eq!(this_page(Some(a("[::1]:15432")), None, &[any]), "cut_off");
        // Another port is another listener
        assert_eq!(
            this_page(Some(a("127.0.0.1:15432")), None, &[a("0.0.0.0:8080")]),
            "cut_off"
        );
    }

    #[test]
    fn ui_urls() {
        assert_eq!(
            ui_at(&[a("0.0.0.0:8080"), a("[::1]:8080")], Some("nas")),
            vec!["http://nas:8080/", "http://[::1]:8080/"]
        );
        assert_eq!(
            ui_at(&[a("[::]:8080")], None),
            vec!["http://localhost:8080/"]
        );
    }
}
