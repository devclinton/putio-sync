//! DLNA / UPnP media server (feature 022): players on the home network find putio-sync,
//! browse the shared folders and play files, with seeking.
//!
//! - [`ssdp`]: discovery on UDP multicast.
//! - [`description`], [`soap`], [`content`]: the UPnP documents, SOAP and Browse.
//! - [`stream_local`], [`putio`]: the media bytes, from disk and from Put.io.
//!
//! [`MediaServer`] is the handle the daemon and the API hold. It is idle until
//! [`MediaServer::update`] gets a config with `[dlna] enabled`, and applies later changes
//! without a restart.

pub mod content;
pub mod description;
pub mod media;
pub mod metrics;
pub mod putio;
pub mod soap;
pub mod ssdp;
pub mod stream_local;

use axum::body::Body;
use axum::extract::{ConnectInfo, State as AxumState};
use axum::http::{
    header, HeaderMap, HeaderName, HeaderValue, Method, Request, Response, StatusCode,
};
use axum::Router;
use content::{BrowseFlag, Fault, ObjectId};
use metrics::{Kind, Outcome};
use percent_encoding::percent_decode_str;
use putio::{PutioError, PutioRoot};
use putio_sync_client::PutioApiClient;
use putio_sync_core::config::{Config, DlnaConfig};
use putio_sync_core::listen::ListenAddr;
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use stream_local::Active;
use tokio::task::JoinHandle;

/// Where the server's UUID is kept, in the config folder
pub const UUID_FILE: &str = "dlna-uuid";

/// How a root's content is found
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootSource {
    Local(PathBuf),
    /// The Put.io folder id
    Putio(i64),
}

/// One folder shared with players
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedRoot {
    /// `downloads`, `cat:<name>`, `watch:<path>` or `putio`: the first part of object ids
    pub key: String,
    pub title: String,
    pub source: RootSource,
}

/// What requests are answered from; swapped whole on each settings apply
pub struct Shared {
    pub name: String,
    pub roots: Vec<SharedRoot>,
    pub update_id: u32,
    pub putio: Option<Arc<PutioRoot>>,
    /// Put.io is shared in the settings, though it may be unavailable (no login)
    pub wants_putio: bool,
}

impl Shared {
    /// A shared root by key
    pub fn root(&self, key: &str) -> Option<&SharedRoot> {
        self.roots.iter().find(|r| r.key == key)
    }
}

/// The roots a config shares, in order: downloads, `cat:*` by name, `watch:*` in config
/// order, `putio`. A `cat:` or `watch:` folder that is missing is kept (shown empty).
pub fn roots(config: &Config, dlna: &DlnaConfig) -> Vec<SharedRoot> {
    let download = config.download.target_directory.clone();
    let mut out = Vec::new();
    let wanted = |key: &str| dlna.roots.iter().any(|r| r == key);
    if wanted("downloads") {
        out.push(SharedRoot {
            key: "downloads".into(),
            title: "Downloads".into(),
            source: RootSource::Local(download.clone()),
        });
    }
    let mut cats: Vec<&str> = dlna
        .roots
        .iter()
        .filter_map(|r| r.strip_prefix("cat:"))
        .collect();
    cats.sort_unstable();
    for name in cats {
        out.push(SharedRoot {
            key: format!("cat:{name}"),
            title: name.to_string(),
            source: RootSource::Local(download.join(name)),
        });
    }
    for path in &config.watch.paths {
        let text = path.to_string_lossy().into_owned();
        let key = format!("watch:{text}");
        if dlna.roots.contains(&key) {
            out.push(SharedRoot {
                key,
                title: text,
                source: RootSource::Local(path.clone()),
            });
        }
    }
    if dlna.shares_putio() {
        out.push(SharedRoot {
            key: "putio".into(),
            title: "Put.io".into(),
            source: RootSource::Putio(dlna.putio_root()),
        });
    }
    out
}

/// The server's state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Off,
    Starting,
    Running,
    Failed,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Off => "off",
            State::Starting => "starting",
            State::Running => "running",
            State::Failed => "failed",
        }
    }
}

/// How the Put.io root is doing
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PutioHealth {
    Off,
    Ok,
    Error(String),
}

/// What the Settings page shows
#[derive(Debug, Clone)]
pub struct MediaServerStatus {
    pub state: State,
    /// Why it failed, or a warning while running
    pub reason: Option<String>,
    /// The `description.xml` URLs
    pub addresses: Vec<String>,
    pub streams_active: usize,
    pub players_last_hour: usize,
    pub putio: PutioHealth,
}

/// Knobs tests turn
#[derive(Debug, Clone)]
pub struct Options {
    /// The SSDP group and port
    pub ssdp_group: SocketAddrV4,
    /// How long Put.io listings are reused
    pub putio_ttl: Duration,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            ssdp_group: ssdp::GROUP,
            putio_ttl: putio::LISTING_TTL,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RunKey {
    port: u16,
    addrs: Vec<Ipv4Addr>,
    ssdp: bool,
    group: SocketAddrV4,
}

struct Running {
    key: RunKey,
    servers: Vec<JoinHandle<()>>,
    shutdown: tokio::sync::watch::Sender<bool>,
    ssdp: Option<ssdp::Ssdp>,
}

struct Inner {
    client: Option<Arc<PutioApiClient>>,
    uuid: String,
    options: Options,
    shared: RwLock<Arc<Shared>>,
    update_id: AtomicU32,
    status: Mutex<(State, Option<String>, Vec<String>)>,
    running: tokio::sync::Mutex<Option<Running>>,
    active: Active,
    players: Mutex<HashMap<IpAddr, Instant>>,
}

/// The media server handle
#[derive(Clone)]
pub struct MediaServer {
    inner: Arc<Inner>,
}

fn load_uuid(dir: &Path) -> String {
    let path = dir.join(UUID_FILE);
    if let Ok(text) = std::fs::read_to_string(&path) {
        let text = text.trim();
        if uuid::Uuid::parse_str(text).is_ok() {
            return text.to_string();
        }
    }
    let fresh = uuid::Uuid::new_v4().to_string();
    let saved = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&path, &fresh));
    if let Err(e) = saved {
        tracing::warn!(error = %e, "Could not save the DLNA server id; players may see a new server after a restart");
    }
    fresh
}

/// Interfaces that are not a home network: Docker and VM bridges, tunnels
fn is_bridge(name: &str) -> bool {
    [
        "docker",
        "br-",
        "veth",
        "virbr",
        "cni",
        "flannel",
        "cali",
        "tailscale",
        "utun",
        "tun",
        "wg",
        "zt",
        "ppp",
    ]
    .iter()
    .any(|p| name.starts_with(p))
}

/// Non-loopback IPv4 addresses: (LAN, bridges and tunnels)
fn local_v4() -> (Vec<Ipv4Addr>, Vec<Ipv4Addr>) {
    let (mut lan, mut other) = (Vec::new(), Vec::new());
    for iface in if_addrs::get_if_addrs().unwrap_or_default() {
        let if_addrs::IfAddr::V4(v4) = &iface.addr else {
            continue;
        };
        if v4.ip.is_loopback() || v4.ip.is_link_local() || v4.ip.is_unspecified() {
            continue;
        }
        if is_bridge(&iface.name) {
            other.push(v4.ip);
        } else {
            lan.push(v4.ip);
        }
    }
    lan.sort();
    lan.dedup();
    other.sort();
    other.dedup();
    (lan, other)
}

/// Where to listen
#[derive(Debug, PartialEq, Eq)]
struct Plan {
    addrs: Vec<Ipv4Addr>,
    ssdp: bool,
    warning: Option<String>,
}

const NO_LAN: &str = "No LAN interface: players can't find it. Use host networking.";

fn bind_plan(dlna: &DlnaConfig) -> Result<Plan, String> {
    let entries = dlna.bind_entries().map_err(|e| e.to_string())?;
    let (lan, other) = local_v4();
    let all = |warn: &mut Option<String>| -> Result<(Vec<Ipv4Addr>, bool), String> {
        if !lan.is_empty() {
            return Ok((lan.clone(), true));
        }
        if !other.is_empty() {
            *warn = Some(NO_LAN.to_string());
            return Ok((other.clone(), false));
        }
        Err("No network address to listen on".to_string())
    };
    let mut warning = None;
    if entries.is_empty() {
        let (addrs, ssdp) = all(&mut warning)?;
        return Ok(Plan {
            addrs,
            ssdp,
            warning,
        });
    }
    let mut addrs = Vec::new();
    let mut ssdp = true;
    for entry in entries {
        match &entry {
            ListenAddr::Ip(SocketAddr::V4(a)) if a.ip().is_unspecified() => {
                let (found, s) = all(&mut warning)?;
                ssdp &= s;
                addrs.extend(found);
            }
            ListenAddr::Ip(SocketAddr::V4(a)) => addrs.push(*a.ip()),
            ListenAddr::Ip(SocketAddr::V6(_)) => {
                return Err("IPv6 addresses aren't supported for the media server".to_string())
            }
            ListenAddr::Tailnet { .. } => {
                return Err("tailnet addresses aren't supported for the media server".to_string())
            }
            ListenAddr::Interface { .. } => {
                let found = entry.resolve_local()?;
                for a in found {
                    if let SocketAddr::V4(a) = a {
                        addrs.push(*a.ip());
                    }
                }
            }
        }
    }
    addrs.sort();
    addrs.dedup();
    if addrs.is_empty() {
        return Err("The listen addresses have no IPv4 address".to_string());
    }
    Ok(Plan {
        addrs,
        ssdp,
        warning,
    })
}

/// One HTTP listener: the address it is on is the address in the URLs it hands out
struct Listener {
    inner: Arc<Inner>,
    addr: Ipv4Addr,
    port: u16,
}

impl MediaServer {
    /// An idle server. `client` is for the Put.io root; the UUID is kept in `config_dir`.
    pub fn new(client: Option<Arc<PutioApiClient>>, config_dir: &Path) -> MediaServer {
        MediaServer::with_options(client, config_dir, Options::default())
    }

    pub fn with_options(
        client: Option<Arc<PutioApiClient>>,
        config_dir: &Path,
        options: Options,
    ) -> MediaServer {
        let shared = Shared {
            name: DlnaConfig::default().effective_name(),
            roots: Vec::new(),
            update_id: 1,
            putio: None,
            wants_putio: false,
        };
        MediaServer {
            inner: Arc::new(Inner {
                client,
                uuid: load_uuid(config_dir),
                options,
                shared: RwLock::new(Arc::new(shared)),
                update_id: AtomicU32::new(1),
                status: Mutex::new((State::Off, None, Vec::new())),
                running: tokio::sync::Mutex::new(None),
                active: Arc::new(AtomicUsize::new(0)),
                players: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// The server's UUID
    pub fn uuid(&self) -> &str {
        &self.inner.uuid
    }

    /// The state for the Settings page
    pub fn status(&self) -> MediaServerStatus {
        let (state, reason, addresses) = self.inner.status.lock().unwrap().clone();
        let putio = {
            let shared = self.inner.shared.read().unwrap().clone();
            if state == State::Off {
                PutioHealth::Off
            } else if shared.root("putio").is_none() {
                if shared.wants_putio && self.inner.client.is_none() {
                    PutioHealth::Error("No Put.io login".to_string())
                } else {
                    PutioHealth::Off
                }
            } else {
                match shared.putio.as_ref().and_then(|p| p.last_error()) {
                    Some(e) => PutioHealth::Error(e),
                    None => PutioHealth::Ok,
                }
            }
        };
        let hour = Duration::from_secs(3600);
        let players_last_hour = self
            .inner
            .players
            .lock()
            .unwrap()
            .values()
            .filter(|at| at.elapsed() < hour)
            .count();
        MediaServerStatus {
            state,
            reason,
            addresses,
            streams_active: self.inner.active.load(Ordering::Relaxed),
            players_last_hour,
            putio,
        }
    }

    fn set_status(&self, state: State, reason: Option<String>, addresses: Vec<String>) {
        *self.inner.status.lock().unwrap() = (state, reason, addresses);
    }

    fn build_shared(&self, config: &Config, dlna: &DlnaConfig) -> Shared {
        let mut roots = roots(config, dlna);
        for root in &roots {
            if let RootSource::Local(path) = &root.source {
                if !path.is_dir() {
                    tracing::warn!(root = %root.key, "A shared DLNA folder is missing; players see it empty");
                }
            }
        }
        let previous = self.inner.shared.read().unwrap().clone();
        // No folder chosen is the synced folder, where stream mode keeps files (022)
        let putio = match (roots.iter().any(|r| r.key == "putio"), dlna.putio_root()) {
            (true, folder) => match &self.inner.client {
                Some(client) => match &previous.putio {
                    Some(p) if p.folder() == folder => Some(p.clone()),
                    _ => Some(Arc::new(PutioRoot::with_ttl(
                        client.clone(),
                        folder,
                        self.inner.options.putio_ttl,
                    ))),
                },
                None => {
                    roots.retain(|r| r.key != "putio");
                    None
                }
            },
            _ => None,
        };
        Shared {
            name: dlna.effective_name(),
            roots,
            update_id: self.inner.update_id.fetch_add(1, Ordering::Relaxed) + 1,
            putio,
            wants_putio: dlna.shares_putio(),
        }
    }

    /// Apply a config: start, rebind, change what is shared, or stop. Never fails the
    /// caller: a problem is the `failed` state with its reason.
    pub async fn update(&self, config: &Config) {
        let mut running = self.inner.running.lock().await;
        let dlna = match config.resolve_dlna() {
            Ok(d) => d,
            Err(e) => {
                self.stop_locked(&mut running).await;
                tracing::warn!(error = %e, "DLNA media server settings are not valid");
                self.set_status(State::Failed, Some(e.to_string()), Vec::new());
                return;
            }
        };
        if !dlna.enabled {
            self.stop_locked(&mut running).await;
            *self.inner.shared.write().unwrap() = Arc::new(self.build_shared(config, &dlna));
            self.set_status(State::Off, None, Vec::new());
            return;
        }
        let plan = match bind_plan(&dlna) {
            Ok(p) => p,
            Err(reason) => {
                self.stop_locked(&mut running).await;
                tracing::warn!(%reason, "DLNA media server can't listen");
                self.set_status(State::Failed, Some(reason), Vec::new());
                return;
            }
        };
        let key = RunKey {
            port: dlna.port,
            addrs: plan.addrs.clone(),
            ssdp: plan.ssdp,
            group: self.inner.options.ssdp_group,
        };
        let shared = Arc::new(self.build_shared(config, &dlna));
        if running.as_ref().is_some_and(|r| r.key == key) {
            // Same sockets: what is shared and its name change in place
            *self.inner.shared.write().unwrap() = shared;
            let addresses = self.inner.status.lock().unwrap().2.clone();
            self.set_status(State::Running, plan.warning, addresses);
            return;
        }
        self.stop_locked(&mut running).await;
        *self.inner.shared.write().unwrap() = shared;
        self.set_status(State::Starting, None, Vec::new());
        match self.start_locked(&key).await {
            Ok((run, warning)) => {
                *running = Some(run);
                let addresses = key
                    .addrs
                    .iter()
                    .map(|a| format!("http://{a}:{}/description.xml", key.port))
                    .collect();
                metrics::up(true);
                tracing::info!(port = key.port, addresses = ?key.addrs, "DLNA media server running");
                self.set_status(State::Running, plan.warning.or(warning), addresses);
            }
            Err(reason) => {
                tracing::warn!(%reason, "DLNA media server could not start");
                self.set_status(State::Failed, Some(reason), Vec::new());
            }
        }
    }

    async fn start_locked(&self, key: &RunKey) -> Result<(Running, Option<String>), String> {
        // Bind everything first: all or nothing
        let mut listeners = Vec::new();
        for addr in &key.addrs {
            let listener =
                tokio::net::TcpListener::bind(SocketAddr::new(IpAddr::V4(*addr), key.port))
                    .await
                    .map_err(|e| format!("Can't listen on {addr}:{}: {e}", key.port))?;
            listeners.push((*addr, listener));
        }
        let (shutdown, rx) = tokio::sync::watch::channel(false);
        let mut servers = Vec::new();
        for (addr, listener) in listeners {
            let state = Arc::new(Listener {
                inner: self.inner.clone(),
                addr,
                port: key.port,
            });
            let app = Router::new().fallback(dispatch).with_state(state);
            let mut rx = rx.clone();
            servers.push(tokio::spawn(async move {
                let serve = axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<SocketAddr>(),
                )
                .with_graceful_shutdown(async move {
                    let _ = rx.wait_for(|stop| *stop).await;
                });
                if let Err(e) = serve.await {
                    tracing::warn!(error = %e, "DLNA HTTP server stopped");
                }
            }));
        }
        let mut warning = None;
        let ssdp = if key.ssdp {
            let cfg = ssdp::SsdpConfig {
                group: key.group,
                ifaces: key.addrs.clone(),
                port: key.port,
                uuid: self.inner.uuid.clone(),
            };
            match ssdp::Ssdp::start(cfg).await {
                Ok(s) => Some(s),
                Err(e) => {
                    tracing::warn!(error = %e, "DLNA discovery could not start");
                    warning = Some(format!(
                        "Discovery isn't working ({e}): add the server to a player by its address"
                    ));
                    None
                }
            }
        } else {
            None
        };
        Ok((
            Running {
                key: key.clone(),
                servers,
                shutdown,
                ssdp,
            },
            warning,
        ))
    }

    async fn stop_locked(&self, running: &mut Option<Running>) {
        let Some(run) = running.take() else { return };
        if let Some(ssdp) = run.ssdp {
            ssdp.stop().await;
        }
        let _ = run.shutdown.send(true);
        // Streams still open would hold a graceful shutdown; give it a moment, then cut
        tokio::time::sleep(Duration::from_millis(50)).await;
        for server in run.servers {
            server.abort();
        }
        metrics::up(false);
    }

    /// Say goodbye and stop listening
    pub async fn stop(&self) {
        let mut running = self.inner.running.lock().await;
        self.stop_locked(&mut running).await;
        self.set_status(State::Off, None, Vec::new());
    }
}

impl Inner {
    fn shared(&self) -> Arc<Shared> {
        self.shared.read().unwrap().clone()
    }

    fn note_player(&self, ip: IpAddr) {
        let mut players = self.players.lock().unwrap();
        let hour = Duration::from_secs(3600);
        players.retain(|_, at| at.elapsed() < hour);
        players.insert(ip, Instant::now());
    }
}

fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

fn xml_response(status: StatusCode, body: String) -> Response<Body> {
    let mut r = Response::new(Body::from(body));
    *r.status_mut() = status;
    r.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/xml; charset=\"utf-8\""),
    );
    r.headers_mut()
        .insert(HeaderName::from_static("ext"), HeaderValue::from_static(""));
    r
}

fn count(kind: Kind, outcome: Outcome) {
    metrics::request(kind, outcome);
}

async fn dispatch(
    AxumState(l): AxumState<Arc<Listener>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    req: Request<Body>,
) -> Response<Body> {
    l.inner.note_player(peer.ip());
    let (parts, body) = req.into_parts();
    let path = parts.uri.path().to_string();
    let method = parts.method.clone();
    let get = method == Method::GET || method == Method::HEAD;
    match path.as_str() {
        "/description.xml" if get => {
            let shared = l.inner.shared();
            count(Kind::Description, Outcome::Ok);
            xml_response(
                StatusCode::OK,
                description::device(&shared.name, &l.inner.uuid),
            )
        }
        "/cds.xml" if get => {
            count(Kind::Description, Outcome::Ok);
            xml_response(StatusCode::OK, description::content_directory())
        }
        "/cms.xml" if get => {
            count(Kind::Description, Outcome::Ok);
            xml_response(StatusCode::OK, description::connection_manager())
        }
        "/ctl/cds" | "/ctl/cms" if method == Method::POST => {
            let bytes = match axum::body::to_bytes(body, 1 << 20).await {
                Ok(b) => b,
                Err(_) => {
                    count(Kind::Browse, Outcome::Error);
                    return stream_local::empty(StatusCode::PAYLOAD_TOO_LARGE);
                }
            };
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let service = if path == "/ctl/cds" {
                soap::CDS
            } else {
                soap::CMS
            };
            control(&l, peer, service, &text).await
        }
        "/evt/cds" | "/evt/cms" if matches!(method.as_str(), "SUBSCRIBE" | "UNSUBSCRIBE") => {
            // No events are sent; the SystemUpdateID change is found by polling
            count(Kind::Other, Outcome::Ok);
            let mut r = Response::new(Body::empty());
            let sid = format!("uuid:{}", uuid::Uuid::new_v4());
            if let Ok(v) = HeaderValue::from_str(&sid) {
                r.headers_mut().insert(HeaderName::from_static("sid"), v);
            }
            r.headers_mut().insert(
                HeaderName::from_static("timeout"),
                HeaderValue::from_static("Second-1800"),
            );
            r
        }
        p if p.starts_with("/media/") && get => {
            media_route(&l, peer, &method, &parts.headers, p).await
        }
        _ => {
            count(Kind::Other, Outcome::Refused);
            stream_local::empty(StatusCode::NOT_FOUND)
        }
    }
}

async fn control(l: &Listener, peer: SocketAddr, service: &str, body: &str) -> Response<Body> {
    let Ok(action) = soap::parse_action(body) else {
        count(Kind::Browse, Outcome::Error);
        return xml_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            soap::fault(401, "Invalid Action"),
        );
    };
    let shared = l.inner.shared();
    let reply = |args: &[(&str, String)]| {
        count(Kind::Browse, Outcome::Ok);
        xml_response(StatusCode::OK, soap::envelope(&action.name, service, args))
    };
    let fault = |f: Fault, object_id: &str| {
        if matches!(f, Fault::NoSuchObject(_)) {
            tracing::warn!(peer = %peer.ip(), object_id = %truncate(object_id, 200), reason = %f.reason(), "DLNA browse refused");
            count(Kind::Browse, Outcome::Refused);
        } else {
            tracing::warn!(peer = %peer.ip(), object_id = %truncate(object_id, 200), reason = %f.reason(), "DLNA browse failed");
            count(Kind::Browse, Outcome::Error);
        }
        xml_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            soap::fault(f.code(), f.description()),
        )
    };
    match (service == soap::CDS, action.name.as_str()) {
        (true, "Browse") => {
            let object_id = action.arg("ObjectID").unwrap_or("").to_string();
            let flag = match action.arg("BrowseFlag") {
                Some("BrowseMetadata") => BrowseFlag::Metadata,
                Some("BrowseDirectChildren") => BrowseFlag::DirectChildren,
                _ => {
                    count(Kind::Browse, Outcome::Error);
                    return xml_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        soap::fault(402, "Invalid Args"),
                    );
                }
            };
            let start = action
                .arg("StartingIndex")
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(0);
            let want = action
                .arg("RequestedCount")
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(0);
            let base = format!("http://{}:{}", l.addr, l.port);
            match content::browse(&shared, &base, &object_id, flag, start, want).await {
                Ok(out) => reply(&[
                    ("Result", out.didl),
                    ("NumberReturned", out.returned.to_string()),
                    ("TotalMatches", out.total.to_string()),
                    ("UpdateID", shared.update_id.to_string()),
                ]),
                Err(f) => fault(f, &object_id),
            }
        }
        (true, "GetSystemUpdateID") => reply(&[("Id", shared.update_id.to_string())]),
        (true, "GetSearchCapabilities") => reply(&[("SearchCaps", String::new())]),
        (true, "GetSortCapabilities") => reply(&[("SortCaps", String::new())]),
        (false, "GetProtocolInfo") => reply(&[
            ("Source", media::all_protocol_info().join(",")),
            ("Sink", String::new()),
        ]),
        (false, "GetCurrentConnectionIDs") => reply(&[("ConnectionIDs", "0".to_string())]),
        (false, "GetCurrentConnectionInfo") => reply(&[
            ("RcsID", "-1".to_string()),
            ("AVTransportID", "-1".to_string()),
            ("ProtocolInfo", String::new()),
            ("PeerConnectionManager", String::new()),
            ("PeerConnectionID", "-1".to_string()),
            ("Direction", "Output".to_string()),
            ("Status", "OK".to_string()),
        ]),
        _ => fault(Fault::InvalidAction, &action.name),
    }
}

fn refuse(peer: SocketAddr, object_id: &str, reason: &str) -> Response<Body> {
    tracing::warn!(peer = %peer.ip(), object_id = %truncate(object_id, 200), %reason, "DLNA stream refused");
    count(Kind::Stream, Outcome::Refused);
    stream_local::empty(StatusCode::NOT_FOUND)
}

async fn media_route(
    l: &Listener,
    peer: SocketAddr,
    method: &Method,
    headers: &HeaderMap,
    raw_path: &str,
) -> Response<Body> {
    // The id was encoded once more to sit in the URL; decode that layer exactly once
    let Ok(id_text) = percent_decode_str(&raw_path["/media/".len()..]).decode_utf8() else {
        return refuse(peer, raw_path, "not valid text");
    };
    let id_text = id_text.into_owned();
    let Some(ObjectId::Node { key, rel }) = ObjectId::parse(&id_text) else {
        return refuse(peer, &id_text, "not an item id");
    };
    let shared = l.inner.shared();
    let Some(root) = shared.root(&key) else {
        return refuse(peer, &id_text, "root is not shared");
    };
    match &root.source {
        RootSource::Local(dir) => {
            let dir = dir.clone();
            let rel_checked = rel.clone();
            let checked =
                tokio::task::spawn_blocking(move || -> Result<(PathBuf, String), String> {
                    let real = content::local_path(&dir, &rel_checked)?;
                    if !real.is_file() {
                        return Err("not a file".to_string());
                    }
                    let name = real
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    let kind = media::classify(&name).ok_or_else(|| "not media".to_string())?;
                    Ok((real, kind.mime))
                })
                .await;
            let (real, mime) = match checked {
                Ok(Ok(v)) => v,
                Ok(Err(reason)) => return refuse(peer, &id_text, &reason),
                Err(_) => return refuse(peer, &id_text, "check failed"),
            };
            let response =
                stream_local::serve(real, &key, method, headers, &mime, l.inner.active.clone())
                    .await;
            let outcome = if response.status().is_success() {
                Outcome::Ok
            } else {
                Outcome::Error
            };
            count(Kind::Stream, outcome);
            response
        }
        RootSource::Putio(_) => {
            let Some(putio) = shared.putio.clone() else {
                return refuse(peer, &id_text, "Put.io is not shared");
            };
            let Ok(id) = rel.parse::<i64>() else {
                return refuse(peer, &id_text, "not a Put.io file");
            };
            putio_route(l, peer, &putio, id, method, headers, &id_text).await
        }
    }
}

async fn putio_route(
    l: &Listener,
    peer: SocketAddr,
    root: &PutioRoot,
    id: i64,
    method: &Method,
    headers: &HeaderMap,
    id_text: &str,
) -> Response<Body> {
    let range = headers.get(header::RANGE).and_then(|v| v.to_str().ok());
    let head = *method == Method::HEAD;
    let opened = match root.open(id, range, head).await {
        Ok(o) => o,
        Err(PutioError::Outside(reason)) => return refuse(peer, id_text, &reason),
        Err(PutioError::Upstream(reason)) => {
            tracing::warn!(peer = %peer.ip(), %reason, "DLNA stream failed at Put.io");
            count(Kind::Stream, Outcome::Error);
            return stream_local::empty(StatusCode::BAD_GATEWAY);
        }
    };
    let mut response = Response::new(Body::empty());
    *response.status_mut() = StatusCode::from_u16(opened.status).unwrap_or(StatusCode::BAD_GATEWAY);
    stream_local::media_headers(response.headers_mut(), &opened.mime);
    for (name, value) in [
        (header::CONTENT_LENGTH, &opened.content_length),
        (header::CONTENT_RANGE, &opened.content_range),
    ] {
        if let Some(Ok(v)) = value.as_deref().map(HeaderValue::from_str) {
            response.headers_mut().insert(name, v);
        }
    }
    count(Kind::Stream, Outcome::Ok);
    let Some(upstream) = opened.body else {
        return response;
    };
    if opened.status == 416 {
        return response;
    }
    let size = opened
        .content_length
        .as_deref()
        .and_then(|v| v.parse().ok());
    let guard = stream_local::StreamGuard::start(
        "putio",
        "putio",
        Some(if opened.mp4 { "mp4" } else { "original" }),
        size,
        l.inner.active.clone(),
    );
    let stream = stream_local::Counted::new(upstream.bytes_stream(), guard);
    *response.body_mut() = Body::from_stream(stream);
    response
}
