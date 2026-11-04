//! What tailscaled says, as small types: the node's status, `serve` entries and a
//! peer's identity. Parsing is tolerant: unknown or missing fields are left empty, so a
//! newer tailscaled with more fields still parses.

use serde::Serialize;
use serde_json::Value;
use std::net::IpAddr;

/// The Tailscale daemon's state, as far as putio-sync can tell
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// No socket and no CLI
    NotFound,
    /// Found, but it didn't answer, timed out or answered junk
    Unreachable,
    /// Installed and stopped (`tailscale down`), or not set up yet
    Stopped,
    /// Needs a login (or machine approval)
    NeedsLogin,
    /// Starting up
    Starting,
    /// Running and signed in
    Running,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::NotFound => "not found",
            State::Unreachable => "not reachable",
            State::Stopped => "stopped",
            State::NeedsLogin => "signed out",
            State::Starting => "starting",
            State::Running => "running",
        }
    }
}

/// How putio-sync reached tailscaled
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Via {
    /// The LocalAPI unix socket
    Socket,
    /// The `tailscale` CLI
    Cli,
}

/// One `tailscale serve` web handler
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServeEntry {
    /// `https://<host>` (with `:<port>` when not 443), or `http://` for an HTTP handler
    pub url: String,
    /// The mount path, `/` for the whole site
    pub path: String,
    /// Where it proxies to, as tailscaled has it
    pub target: String,
    /// Whether Funnel opens it to the internet
    pub funnel: bool,
}

impl ServeEntry {
    /// The local port it proxies to, when the target is this machine's loopback
    pub fn local_port(&self) -> Option<u16> {
        loopback_port(&self.target)
    }

    /// The address to open: the url, plus the path when it isn't `/`
    pub fn address(&self) -> String {
        if self.path == "/" || self.path.is_empty() {
            self.url.clone()
        } else {
            format!("{}{}", self.url, self.path)
        }
    }
}

/// The node's status. Fields that don't apply are empty.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Status {
    pub state: State,
    pub via: Option<Via>,
    pub version: Option<String>,
    /// The MagicDNS name, without the trailing dot; `None` when MagicDNS is off
    pub dns_name: Option<String>,
    pub host_name: Option<String>,
    /// The tailnet's name
    pub tailnet: Option<String>,
    pub magic_dns: bool,
    /// This node's tailnet addresses
    pub addresses: Vec<IpAddr>,
    /// HTTPS certificates are available in this tailnet
    pub https: bool,
    /// Userspace networking: no tailnet address on this machine, `serve` is the way in
    pub userspace: bool,
    /// The login that owns this node
    pub owner_login: Option<String>,
    /// Every `serve` web handler (see [`Status::serve_for_port`])
    pub serve: Vec<ServeEntry>,
    /// Why it is unreachable
    pub error: Option<String>,
}

impl Status {
    /// A status with only a state (not found, unreachable)
    pub fn bare(state: State, via: Option<Via>, error: Option<String>) -> Status {
        Status {
            state,
            via,
            version: None,
            dns_name: None,
            host_name: None,
            tailnet: None,
            magic_dns: false,
            addresses: Vec::new(),
            https: false,
            userspace: false,
            owner_login: None,
            serve: Vec::new(),
            error,
        }
    }

    /// The first IPv4 tailnet address
    pub fn ipv4(&self) -> Option<IpAddr> {
        self.addresses.iter().copied().find(IpAddr::is_ipv4)
    }

    /// The `serve` entries that proxy to `port` on this machine
    pub fn serve_for_port(&self, port: u16) -> Vec<&ServeEntry> {
        self.serve
            .iter()
            .filter(|e| e.local_port() == Some(port))
            .collect()
    }

    /// Addresses that open putio-sync on `port` over the tailnet: `serve` first, then
    /// the MagicDNS name and the IPv4 address. Direct ones only when the listener can be
    /// reached over the tailnet (not userspace, not loopback-only).
    pub fn urls(&self, port: u16, loopback_only: bool) -> Vec<String> {
        if self.state != State::Running {
            return Vec::new();
        }
        let mut urls: Vec<String> = self
            .serve_for_port(port)
            .iter()
            .map(|e| e.address())
            .collect();
        if !self.userspace && !loopback_only {
            if let Some(name) = &self.dns_name {
                urls.push(format!("http://{name}:{port}"));
            }
            if let Some(ip) = self.ipv4() {
                urls.push(format!("http://{ip}:{port}"));
            }
        }
        urls
    }
}

/// A peer's identity: who is on the other end of a request
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Identity {
    /// The Tailscale login, e.g. `alice@example.com`
    pub login: String,
    /// The display name, or the login when there is none
    pub name: String,
    /// The device's name; empty when it came from `serve`'s headers
    pub device: String,
    /// How it was found
    pub via: IdentityVia,
}

/// How an identity was found
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum IdentityVia {
    /// `tailscale serve`'s headers on a loopback connection
    Serve,
    /// tailscaled's `whois` for a tailnet peer address
    Whois,
}

impl IdentityVia {
    pub fn as_str(self) -> &'static str {
        match self {
            IdentityVia::Serve => "serve",
            IdentityVia::Whois => "whois",
        }
    }
}

/// A `whois` answer
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Whois {
    pub login: String,
    pub name: String,
    pub device: String,
    /// A tagged device has no user and is never signed in
    pub tagged: bool,
}

/// Parse `/localapi/v0/status` (or `tailscale status --json`)
pub fn parse_status(json: &[u8], via: Via) -> Result<Status, String> {
    let v: Value = serde_json::from_slice(json).map_err(|e| format!("bad status JSON: {e}"))?;
    if !v.is_object() {
        return Err("bad status JSON: not an object".to_string());
    }
    let state = match str_at(&v, &["BackendState"]).as_deref() {
        Some("Running") => State::Running,
        Some("NeedsLogin") | Some("NeedsMachineAuth") => State::NeedsLogin,
        Some("Stopped") | Some("NoState") => State::Stopped,
        _ => State::Starting,
    };
    let mut status = Status::bare(state, Some(via), None);
    status.version = str_at(&v, &["Version"]);
    if state != State::Running {
        return Ok(status);
    }

    let suffix = str_at(&v, &["CurrentTailnet", "MagicDNSSuffix"])
        .or_else(|| str_at(&v, &["MagicDNSSuffix"]));
    status.magic_dns = match v.pointer("/CurrentTailnet/MagicDNSEnabled") {
        Some(Value::Bool(b)) => *b,
        // Older daemons don't say; a suffix means MagicDNS names exist
        _ => suffix.is_some(),
    };
    status.dns_name = str_at(&v, &["Self", "DNSName"])
        .map(|n| n.trim_end_matches('.').to_string())
        .filter(|n| !n.is_empty() && status.magic_dns);
    status.host_name = str_at(&v, &["Self", "HostName"]);
    status.tailnet = str_at(&v, &["CurrentTailnet", "Name"]);
    status.addresses = v
        .get("TailscaleIPs")
        .and_then(Value::as_array)
        .map(|ips| {
            ips.iter()
                .filter_map(Value::as_str)
                .filter_map(|s| s.parse().ok())
                .collect()
        })
        .unwrap_or_default();
    status.https = v
        .get("CertDomains")
        .and_then(Value::as_array)
        .is_some_and(|d| !d.is_empty());
    // TUN is whether a tun device is in use; without it there is no tailnet address here
    status.userspace = v.get("TUN") == Some(&Value::Bool(false));
    status.owner_login = v
        .pointer("/Self/UserID")
        .map(|id| match id {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .and_then(|id| str_at(&v, &["User", &id, "LoginName"]));
    Ok(status)
}

/// Parse `/localapi/v0/serve-config` (or `tailscale serve status --json`) into entries
pub fn parse_serve(json: &[u8]) -> Result<Vec<ServeEntry>, String> {
    let text = std::str::from_utf8(json).map_err(|_| "bad serve JSON".to_string())?;
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let v: Value = serde_json::from_str(text).map_err(|e| format!("bad serve JSON: {e}"))?;
    let mut entries = Vec::new();
    serve_entries(&v, &mut entries);
    // Foreground `serve` sessions keep their own config per session
    if let Some(sessions) = v.get("Foreground").and_then(Value::as_object) {
        for config in sessions.values() {
            serve_entries(config, &mut entries);
        }
    }
    entries.sort_by(|a, b| (&a.url, &a.path).cmp(&(&b.url, &b.path)));
    entries.dedup();
    Ok(entries)
}

fn serve_entries(config: &Value, out: &mut Vec<ServeEntry>) {
    let Some(web) = config.get("Web").and_then(Value::as_object) else {
        return;
    };
    for (host_port, site) in web {
        let (host, port) = match host_port.rsplit_once(':') {
            Some((h, p)) => (h, p.parse::<u16>().unwrap_or(443)),
            None => (host_port.as_str(), 443),
        };
        let https = config
            .pointer(&format!("/TCP/{port}/HTTPS"))
            .and_then(Value::as_bool)
            .unwrap_or_else(|| {
                // No TCP entry: HTTPS unless it is marked plain HTTP
                config.pointer(&format!("/TCP/{port}/HTTP")) != Some(&Value::Bool(true))
            });
        let url = match (https, port) {
            (true, 443) => format!("https://{host}"),
            (false, 80) => format!("http://{host}"),
            (true, p) => format!("https://{host}:{p}"),
            (false, p) => format!("http://{host}:{p}"),
        };
        let funnel = config
            .get("AllowFunnel")
            .and_then(|f| f.get(host_port))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let Some(handlers) = site.get("Handlers").and_then(Value::as_object) else {
            continue;
        };
        for (path, handler) in handlers {
            if let Some(target) = handler.get("Proxy").and_then(Value::as_str) {
                out.push(ServeEntry {
                    url: url.clone(),
                    path: path.clone(),
                    target: target.to_string(),
                    funnel,
                });
            }
        }
    }
}

/// Parse a `whois` answer
pub fn parse_whois(json: &[u8]) -> Result<Whois, String> {
    let v: Value = serde_json::from_slice(json).map_err(|e| format!("bad whois JSON: {e}"))?;
    let tagged = v
        .pointer("/Node/Tags")
        .and_then(Value::as_array)
        .is_some_and(|t| !t.is_empty());
    let login = str_at(&v, &["UserProfile", "LoginName"]).unwrap_or_default();
    if login.is_empty() && !tagged {
        return Err("whois answer has no user".to_string());
    }
    let name = str_at(&v, &["UserProfile", "DisplayName"])
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| login.clone());
    let device = str_at(&v, &["Node", "ComputedName"])
        .or_else(|| {
            str_at(&v, &["Node", "Name"])
                .map(|n| n.split('.').next().unwrap_or_default().to_string())
        })
        .unwrap_or_default();
    Ok(Whois {
        login,
        name,
        device,
        tagged,
    })
}

/// Whether `ip` is a Tailscale address: `100.64.0.0/10` or `fd7a:115c:a1e0::/48`
pub fn is_tailnet_ip(ip: IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            o[0] == 100 && (o[1] & 0xc0) == 64
        }
        IpAddr::V6(v6) => {
            let s = v6.segments();
            s[0] == 0xfd7a && s[1] == 0x115c && s[2] == 0xa1e0
        }
    }
}

/// The port of a proxy target on this machine's loopback: `http://127.0.0.1:15432`,
/// `localhost:15432`, `https+insecure://[::1]:15432/x`, or a bare `15432`
fn loopback_port(target: &str) -> Option<u16> {
    let rest = target.split_once("://").map_or(target, |(_, r)| r);
    let rest = rest.split('/').next().unwrap_or(rest);
    if let Ok(port) = rest.parse::<u16>() {
        return Some(port);
    }
    let (host, port) = rest.rsplit_once(':')?;
    let host = host.trim_start_matches('[').trim_end_matches(']');
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_loopback() || ip.is_unspecified());
    if loopback {
        port.parse().ok()
    } else {
        None
    }
}

fn str_at(v: &Value, path: &[&str]) -> Option<String> {
    let mut cur = v;
    for key in path {
        cur = cur.get(key)?;
    }
    cur.as_str().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) const STATUS: &str = r#"{
        "Version": "1.76.1-t1234",
        "TUN": true,
        "BackendState": "Running",
        "TailscaleIPs": ["100.101.102.103", "fd7a:115c:a1e0::1"],
        "Self": {"HostName": "nas", "DNSName": "nas.tail1234.ts.net.", "UserID": 123},
        "MagicDNSSuffix": "tail1234.ts.net",
        "CurrentTailnet": {"Name": "example.com", "MagicDNSSuffix": "tail1234.ts.net", "MagicDNSEnabled": true},
        "CertDomains": ["nas.tail1234.ts.net"],
        "User": {"123": {"ID": 123, "LoginName": "alice@example.com", "DisplayName": "Alice"}}
    }"#;

    pub(crate) const SERVE: &str = r#"{
        "TCP": {"443": {"HTTPS": true}},
        "Web": {"nas.tail1234.ts.net:443": {"Handlers": {"/": {"Proxy": "http://127.0.0.1:15432"}}}},
        "AllowFunnel": {"nas.tail1234.ts.net:443": false}
    }"#;

    #[test]
    fn parses_a_running_node() {
        let s = parse_status(STATUS.as_bytes(), Via::Socket).unwrap();
        assert_eq!(s.state, State::Running);
        assert_eq!(s.version.as_deref(), Some("1.76.1-t1234"));
        assert_eq!(s.dns_name.as_deref(), Some("nas.tail1234.ts.net"));
        assert_eq!(s.tailnet.as_deref(), Some("example.com"));
        assert!(s.magic_dns && s.https && !s.userspace);
        assert_eq!(s.ipv4(), Some("100.101.102.103".parse().unwrap()));
        assert_eq!(s.addresses.len(), 2);
        assert_eq!(s.owner_login.as_deref(), Some("alice@example.com"));
    }

    #[test]
    fn parses_other_states_and_flags() {
        let s = parse_status(br#"{"BackendState":"NeedsLogin","Version":"1.2"}"#, Via::Cli).unwrap();
        assert_eq!(s.state, State::NeedsLogin);
        assert!(s.addresses.is_empty());
        let s = parse_status(br#"{"BackendState":"Stopped"}"#, Via::Cli).unwrap();
        assert_eq!(s.state, State::Stopped);
        let s = parse_status(br#"{"BackendState":"Starting"}"#, Via::Cli).unwrap();
        assert_eq!(s.state, State::Starting);
        assert!(parse_status(b"junk", Via::Cli).is_err());
        assert!(parse_status(b"[]", Via::Cli).is_err());

        // MagicDNS off hides the name; no certs means no HTTPS; TUN false is userspace
        let json = STATUS
            .replace("\"MagicDNSEnabled\": true", "\"MagicDNSEnabled\": false")
            .replace("[\"nas.tail1234.ts.net\"]", "[]")
            .replace("\"TUN\": true", "\"TUN\": false");
        let s = parse_status(json.as_bytes(), Via::Socket).unwrap();
        assert_eq!(s.dns_name, None);
        assert!(!s.https);
        assert!(s.userspace);
        assert!(s.urls(15432, false).is_empty());
    }

    #[test]
    fn parses_serve_entries_and_matches_the_port() {
        let mut s = parse_status(STATUS.as_bytes(), Via::Socket).unwrap();
        s.serve = parse_serve(SERVE.as_bytes()).unwrap();
        assert_eq!(s.serve.len(), 1);
        assert_eq!(s.serve[0].url, "https://nas.tail1234.ts.net");
        assert!(!s.serve[0].funnel);
        assert_eq!(s.serve_for_port(15432).len(), 1);
        assert!(s.serve_for_port(8080).is_empty());
        assert_eq!(
            s.urls(15432, false),
            vec![
                "https://nas.tail1234.ts.net",
                "http://nas.tail1234.ts.net:15432",
                "http://100.101.102.103:15432"
            ]
        );
        assert_eq!(s.urls(15432, true), vec!["https://nas.tail1234.ts.net"]);

        assert!(parse_serve(b"").unwrap().is_empty());
        assert!(parse_serve(b"{}").unwrap().is_empty());
        assert!(parse_serve(b"null").unwrap().is_empty());

        // Another port, a path, funnel, a foreground session, plain HTTP
        let json = r#"{
            "TCP": {"8443": {"HTTPS": true}, "80": {"HTTP": true}},
            "Web": {
                "nas.ts.net:8443": {"Handlers": {"/sync": {"Proxy": "localhost:15432"}}},
                "nas.ts.net:80": {"Handlers": {"/": {"Proxy": "http://192.168.1.5:15432"}}}
            },
            "AllowFunnel": {"nas.ts.net:8443": true},
            "Foreground": {"abc": {"Web": {"nas.ts.net:443": {"Handlers": {"/": {"Proxy": "15432"}}}}}}
        }"#;
        let entries = parse_serve(json.as_bytes()).unwrap();
        let addrs: Vec<String> = entries.iter().map(ServeEntry::address).collect();
        assert_eq!(
            addrs,
            vec!["http://nas.ts.net", "https://nas.ts.net", "https://nas.ts.net:8443/sync"]
        );
        assert!(entries[2].funnel);
        // A LAN target is not this machine's loopback
        assert_eq!(entries[0].local_port(), None);
        assert_eq!(entries[1].local_port(), Some(15432));
    }

    #[test]
    fn parses_whois() {
        let w = parse_whois(br#"{"Node":{"Name":"laptop.tail1234.ts.net.","Tags":[]},"UserProfile":{"LoginName":"alice@example.com","DisplayName":"Alice"}}"#).unwrap();
        assert_eq!(w.login, "alice@example.com");
        assert_eq!(w.name, "Alice");
        assert_eq!(w.device, "laptop");
        assert!(!w.tagged);
        let w = parse_whois(br#"{"Node":{"Name":"srv.ts.net.","Tags":["tag:server"]},"UserProfile":{"LoginName":"tagged-devices"}}"#).unwrap();
        assert!(w.tagged);
        assert!(parse_whois(br#"{"Node":{}}"#).is_err());
        assert!(parse_whois(b"nope").is_err());
    }

    #[test]
    fn tailnet_ranges() {
        for ip in ["100.64.0.1", "100.127.255.254", "fd7a:115c:a1e0::1", "::ffff:100.101.102.103"] {
            assert!(is_tailnet_ip(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["100.63.0.1", "100.128.0.1", "192.168.1.5", "127.0.0.1", "fd7a:115c:a1e1::1"] {
            assert!(!is_tailnet_ip(ip.parse().unwrap()), "{ip}");
        }
    }
}
