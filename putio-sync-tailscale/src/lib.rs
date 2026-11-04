//! Tailscale awareness (feature 036): what tailscaled says about this node, its `serve`
//! entries, and who a peer is. Read-only: putio-sync never changes the Tailscale setup.
//!
//! tailscaled is asked over its LocalAPI socket first and through the `tailscale` CLI
//! when there is no socket. Answers are cached briefly, every call has a timeout, and a
//! missing or broken daemon is a state, never an error that stops anything.

mod cli;
mod localapi;
mod model;

#[cfg(test)]
mod tests;

pub use model::{
    is_tailnet_ip, parse_serve, parse_status, parse_whois, Identity, IdentityVia, ServeEntry,
    State, Status, Via, Whois,
};

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long one call to tailscaled may take
pub const CALL_TIMEOUT: Duration = Duration::from_secs(3);
/// How long a status answer is reused
pub const STATUS_TTL: Duration = Duration::from_secs(10);
/// How long a peer's identity is reused
pub const WHOIS_TTL: Duration = Duration::from_secs(60);
/// The most peers kept in the whois cache
const WHOIS_MAX: usize = 1024;

/// Which `tailscale` CLI to use when there is no socket
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Cli {
    /// `PATH`, then the macOS app
    Auto,
    /// This binary
    Path(PathBuf),
    /// Socket only
    Off,
}

/// The result of looking for a request's Tailscale identity
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lookup {
    /// The request makes no Tailscale claim: not loopback with `serve`'s headers, not a
    /// tailnet address
    None,
    /// Who it is (the allow-list is the caller's to check)
    Found(Identity),
    /// It is a tailnet request, but no user could be given to it
    Refused { via: IdentityVia, reason: String },
}

struct Inner {
    socket: PathBuf,
    cli: Cli,
    /// `Cli::Auto`'s answer, found once
    found_cli: Option<Option<PathBuf>>,
    status: Option<(Instant, Status)>,
    whois: HashMap<IpAddr, (Instant, Option<Whois>)>,
}

/// Talks to tailscaled. Cheap to share; clone the `Arc` it lives in.
pub struct TailscaleService {
    inner: Mutex<Inner>,
    timeout: Duration,
}

impl TailscaleService {
    pub fn new(socket: impl Into<PathBuf>, cli: Cli) -> Self {
        TailscaleService {
            inner: Mutex::new(Inner {
                socket: socket.into(),
                cli,
                found_cli: None,
                status: None,
                whois: HashMap::new(),
            }),
            timeout: CALL_TIMEOUT,
        }
    }

    /// A shorter timeout, for tests
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Point at another socket (a settings save); forgets every cached answer
    pub fn configure(&self, socket: impl Into<PathBuf>) {
        let mut inner = self.lock();
        inner.socket = socket.into();
        inner.status = None;
        inner.whois.clear();
    }

    /// A new service on another socket, with this one's CLI and timeout (and an empty
    /// cache)
    pub fn sibling(&self, socket: impl Into<PathBuf>) -> TailscaleService {
        let cli = self.lock().cli.clone();
        TailscaleService::new(socket, cli).with_timeout(self.timeout)
    }

    /// The socket in use
    pub fn socket(&self) -> PathBuf {
        self.lock().socket.clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The node's status, from the cache when it is under 10 s old
    pub async fn status(&self) -> Status {
        if let Some((at, status)) = &self.lock().status {
            if at.elapsed() < STATUS_TTL {
                return status.clone();
            }
        }
        self.refresh().await
    }

    /// The node's status, asked now
    pub async fn refresh(&self) -> Status {
        let status = self.fetch_status().await;
        self.lock().status = Some((Instant::now(), status.clone()));
        status
    }

    fn sources(&self) -> (Option<PathBuf>, Option<PathBuf>) {
        let mut inner = self.lock();
        let socket = std::fs::symlink_metadata(&inner.socket)
            .is_ok()
            .then(|| inner.socket.clone());
        let cli = match inner.cli.clone() {
            Cli::Off => None,
            Cli::Path(p) => Some(p),
            Cli::Auto => inner.found_cli.get_or_insert_with(cli::find).clone(),
        };
        (socket, cli)
    }

    async fn fetch_status(&self) -> Status {
        let (socket, cli) = self.sources();
        let mut socket_error = None;
        if let Some(socket) = &socket {
            match self.status_from_socket(socket).await {
                Ok(status) => return status,
                Err(e) => socket_error = Some(e),
            }
        }
        if let Some(bin) = &cli {
            match self.status_from_cli(bin).await {
                Ok(status) => return status,
                // The socket's reason is the more useful one when both failed
                Err(e) if socket_error.is_none() => {
                    return Status::bare(State::Unreachable, Some(Via::Cli), Some(e))
                }
                Err(_) => {}
            }
        }
        match socket_error {
            Some(e) => Status::bare(State::Unreachable, Some(Via::Socket), Some(e)),
            None => Status::bare(State::NotFound, None, None),
        }
    }

    async fn socket_get(&self, socket: &Path, path: &str) -> Result<localapi::Response, String> {
        tokio::time::timeout(self.timeout, localapi::get(socket, path))
            .await
            .map_err(|_| "tailscaled didn't answer in time".to_string())?
    }

    async fn status_from_socket(&self, socket: &Path) -> Result<Status, String> {
        let r = self.socket_get(socket, "/localapi/v0/status").await?;
        if r.code != 200 {
            return Err(format!("tailscaled answered {}", r.code));
        }
        let mut status = parse_status(&r.body, Via::Socket)?;
        if status.state == State::Running {
            // No serve config is not a problem; it just means no entries
            if let Ok(r) = self.socket_get(socket, "/localapi/v0/serve-config").await {
                if r.code == 200 {
                    status.serve = parse_serve(&r.body).unwrap_or_default();
                }
            }
        }
        Ok(status)
    }

    async fn status_from_cli(&self, bin: &Path) -> Result<Status, String> {
        let out = cli::run(bin, &["status", "--json"], self.timeout).await?;
        let mut status = parse_status(&out, Via::Cli)?;
        if status.state == State::Running {
            if let Ok(out) = cli::run(bin, &["serve", "status", "--json"], self.timeout).await {
                status.serve = parse_serve(&out).unwrap_or_default();
            }
        }
        Ok(status)
    }

    /// Who is at `ip`: `Ok(None)` when tailscaled doesn't know the address. Answers
    /// (including "unknown") are cached for 60 s; failures are not.
    pub async fn whois(&self, ip: IpAddr) -> Result<Option<Whois>, String> {
        let ip = ip.to_canonical();
        if let Some((at, answer)) = self.lock().whois.get(&ip) {
            if at.elapsed() < WHOIS_TTL {
                return Ok(answer.clone());
            }
        }
        let answer = self.fetch_whois(ip).await?;
        let mut inner = self.lock();
        inner.whois.retain(|_, (at, _)| at.elapsed() < WHOIS_TTL);
        if inner.whois.len() >= WHOIS_MAX {
            inner.whois.clear();
        }
        inner.whois.insert(ip, (Instant::now(), answer.clone()));
        Ok(answer)
    }

    async fn fetch_whois(&self, ip: IpAddr) -> Result<Option<Whois>, String> {
        let (socket, cli) = self.sources();
        if let Some(socket) = &socket {
            let r = self
                .socket_get(socket, &format!("/localapi/v0/whois?addr={ip}"))
                .await?;
            return match r.code {
                200 => parse_whois(&r.body).map(Some),
                404 => Ok(None),
                code => Err(format!("tailscaled answered {code}")),
            };
        }
        if let Some(bin) = &cli {
            let ip = ip.to_string();
            let out = cli::run(bin, &["whois", "--json", &ip], self.timeout).await?;
            return parse_whois(&out).map(Some);
        }
        Err("Tailscale isn't on this machine".to_string())
    }

    /// The Tailscale identity of a request (research R3).
    ///
    /// - `peer` is the TCP peer. Without it there is no identity: a missing peer must
    ///   never look like loopback.
    /// - On a loopback connection, `tailscale serve`'s `Tailscale-User-Login` (and
    ///   `Tailscale-User-Name`) headers say who it is.
    /// - From a tailnet address, tailscaled's `whois` says who it is; tagged devices
    ///   have no user and are refused.
    pub async fn identity_for(
        &self,
        peer: Option<SocketAddr>,
        login_header: Option<&str>,
        name_header: Option<&str>,
    ) -> Lookup {
        let Some(peer) = peer else {
            return Lookup::None;
        };
        let ip = peer.ip().to_canonical();
        if ip.is_loopback() {
            let login = login_header.map(str::trim).unwrap_or_default();
            if login.is_empty() {
                return Lookup::None;
            }
            let name = name_header
                .map(str::trim)
                .filter(|n| !n.is_empty())
                .unwrap_or(login);
            return Lookup::Found(Identity {
                login: login.to_string(),
                name: name.to_string(),
                device: String::new(),
                via: IdentityVia::Serve,
            });
        }
        if !is_tailnet_ip(ip) {
            return Lookup::None;
        }
        let refused = |reason: String| Lookup::Refused {
            via: IdentityVia::Whois,
            reason,
        };
        match self.whois(ip).await {
            Ok(Some(w)) if w.tagged => refused(format!("{ip} is a tagged device")),
            Ok(Some(w)) => Lookup::Found(Identity {
                login: w.login,
                name: w.name,
                device: w.device,
                via: IdentityVia::Whois,
            }),
            Ok(None) => refused(format!("tailscaled doesn't know {ip}")),
            Err(e) => refused(e),
        }
    }
}

/// The address to listen on for "tailnet only": the node's IPv4 tailnet address and
/// `port`. The error says why there is none yet.
pub fn tailnet_bind(status: &Status, port: u16) -> Result<SocketAddr, String> {
    match status.state {
        State::Running => {}
        State::NotFound => return Err("Tailscale isn't on this machine".to_string()),
        State::Unreachable => {
            return Err(format!(
                "Tailscale isn't answering{}",
                status
                    .error
                    .as_deref()
                    .map(|e| format!(" ({e})"))
                    .unwrap_or_default()
            ))
        }
        other => return Err(format!("Tailscale is {}", other.as_str())),
    }
    if status.userspace {
        return Err(
            "Tailscale uses userspace networking here, so there is no tailnet address to listen on; use tailscale serve"
                .to_string(),
        );
    }
    status
        .ipv4()
        .map(|ip| SocketAddr::new(ip, port))
        .ok_or_else(|| "Tailscale has no IPv4 address on this machine".to_string())
}
