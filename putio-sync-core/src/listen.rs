//! Listen addresses (feature 037): what `[api] bind` and `[metrics] bind` hold.
//!
//! An entry is `host:port`, where host is an IPv4 address, an IPv6 address in brackets,
//! the word `tailnet` (this node's Tailscale address, resolved by the API crate), or a
//! network interface name, meaning every address the interface has right now. A setting
//! holds one entry or a list of them.
//!
//! Port 0 (the OS picks one) is accepted for fixed addresses only, as before 037; tests
//! use it.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::str::FromStr;

/// Most entries one list may hold
pub const MAX_ENTRIES: usize = 16;

/// What an entry may look like, for error messages
pub const FORMS: &str = "ip:port, [ipv6]:port, interface:port or tailnet:port";

/// One listen entry
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ListenAddr {
    /// A fixed address, `0.0.0.0:15432` or `[::1]:15432`
    Ip(SocketAddr),
    /// Every address of a network interface, `eth0:15432`
    Interface {
        /// Interface name as the OS gives it
        name: String,
        /// Port
        port: u16,
    },
    /// This node's Tailscale IPv4 address, `tailnet:15432`
    Tailnet {
        /// Port
        port: u16,
    },
}

impl ListenAddr {
    /// The entry's port
    pub fn port(&self) -> u16 {
        match self {
            ListenAddr::Ip(addr) => addr.port(),
            ListenAddr::Interface { port, .. } | ListenAddr::Tailnet { port } => *port,
        }
    }

    /// The same entry on another port
    pub fn with_port(&self, port: u16) -> ListenAddr {
        match self {
            ListenAddr::Ip(addr) => ListenAddr::Ip(SocketAddr::new(addr.ip(), port)),
            ListenAddr::Interface { name, .. } => ListenAddr::Interface {
                name: name.clone(),
                port,
            },
            ListenAddr::Tailnet { .. } => ListenAddr::Tailnet { port },
        }
    }

    /// Whether its addresses can change while running (interface and tailnet entries)
    pub fn is_dynamic(&self) -> bool {
        !matches!(self, ListenAddr::Ip(_))
    }

    /// Whether every address it can give is loopback (only fixed addresses can tell)
    pub fn is_loopback(&self) -> bool {
        matches!(self, ListenAddr::Ip(addr) if addr.ip().is_loopback())
    }

    /// The socket addresses this entry gives now, or why it gives none. Tailnet entries
    /// need Tailscale, which this crate doesn't talk to: they are `Err`.
    pub fn resolve_local(&self) -> Result<Vec<SocketAddr>, String> {
        match self {
            ListenAddr::Ip(addr) => Ok(vec![*addr]),
            ListenAddr::Interface { name, port } => {
                let ips = interface_addrs(name);
                if ips.is_empty() {
                    Err(format!("no interface {name} with an address"))
                } else {
                    Ok(ips
                        .into_iter()
                        .map(|ip| SocketAddr::new(ip, *port))
                        .collect())
                }
            }
            ListenAddr::Tailnet { .. } => Err("needs Tailscale".to_string()),
        }
    }
}

impl fmt::Display for ListenAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ListenAddr::Ip(addr) => write!(f, "{addr}"),
            ListenAddr::Interface { name, port } => write!(f, "{name}:{port}"),
            ListenAddr::Tailnet { port } => write!(f, "tailnet:{port}"),
        }
    }
}

impl FromStr for ListenAddr {
    type Err = String;

    /// Parse one entry; the error says what is wrong, without the field name
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        if s.starts_with('[') {
            return match s.parse::<SocketAddr>() {
                Ok(addr) => Ok(ListenAddr::Ip(addr)),
                Err(_) => Err("expected [ipv6]:port".to_string()),
            };
        }
        let Some((host, port)) = s.rsplit_once(':') else {
            return Err("missing :port".to_string());
        };
        if host.contains(':') {
            return Err("put an IPv6 address in brackets, [ipv6]:port".to_string());
        }
        let port: u16 = port
            .parse()
            .map_err(|_| "the port must be 1 to 65535".to_string())?;
        if host.is_empty() {
            return Err("missing host before :port".to_string());
        }
        if let Ok(ip) = host.parse::<std::net::Ipv4Addr>() {
            return Ok(ListenAddr::Ip(SocketAddr::new(IpAddr::V4(ip), port)));
        }
        if port == 0 {
            return Err("the port must be 1 to 65535".to_string());
        }
        if host.eq_ignore_ascii_case("tailnet") {
            return Ok(ListenAddr::Tailnet { port });
        }
        if !is_interface_name(host) {
            return Err(format!("'{host}' is not an IPv4 address or interface name"));
        }
        Ok(ListenAddr::Interface {
            name: host.to_string(),
            port,
        })
    }
}

/// Whether `host` can be an interface name: letters (any script), digits, `.` `_` `-`, and
/// for Windows' friendly names (`Ethernet 2`, `vEthernet (WSL)`, `Local Area Connection* 3`)
/// single spaces, `(` `)`, `*` and `#`. Not a comma, which separates env entries, nor
/// anything that reads as an IPv4 address.
fn is_interface_name(host: &str) -> bool {
    let name_chars = host
        .chars()
        .all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-' | ' ' | '(' | ')' | '*' | '#'));
    let spaced_inside = !host.starts_with(' ') && !host.ends_with(' ') && !host.contains("  ");
    let looks_numeric = host.chars().all(|c| c.is_ascii_digit() || c == '.');
    host.chars().count() <= 128 && name_chars && spaced_inside && !looks_numeric
}

/// A bind setting: one entry (as before 037) or a list
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum BindList {
    /// `bind = "0.0.0.0:15432"`
    One(String),
    /// `bind = ["eth0:15432", "127.0.0.1:15432"]`
    Many(Vec<String>),
}

impl BindList {
    /// A setting holding `entries`; one entry is written as a plain string
    pub fn from_entries(mut entries: Vec<String>) -> BindList {
        if entries.len() == 1 {
            BindList::One(entries.remove(0))
        } else {
            BindList::Many(entries)
        }
    }

    /// The entries as written
    pub fn entries(&self) -> Vec<String> {
        match self {
            BindList::One(s) => vec![s.clone()],
            BindList::Many(v) => v.clone(),
        }
    }

    /// A comma-separated list, as the environment variables take
    pub fn from_env(value: &str) -> BindList {
        BindList::from_entries(
            value
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect(),
        )
    }

    /// Parse and check the entries; errors name `field` and the bad entry
    pub fn parse(&self, field: &str) -> Result<Vec<ListenAddr>, String> {
        let list = self
            .entries()
            .iter()
            .map(|e| {
                e.parse::<ListenAddr>()
                    .map_err(|why| format!("invalid {field} '{e}': {why} (expected {FORMS})"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        validate_list(field, &list)?;
        Ok(list)
    }
}

impl fmt::Display for BindList {
    /// The entries joined with `, `, as the env variables take them
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.entries().join(", "))
    }
}

impl From<&str> for BindList {
    fn from(s: &str) -> Self {
        BindList::One(s.to_string())
    }
}

/// Two entries that can't both be listened on: the same, or a wildcard and another
/// address of its family on one port
fn clash(a: &ListenAddr, b: &ListenAddr) -> bool {
    // Port 0 gets a different free port each time
    if a.port() != b.port() || a.port() == 0 {
        return false;
    }
    match (a, b) {
        (ListenAddr::Ip(x), ListenAddr::Ip(y)) => {
            x == y
                || (x.is_ipv4() == y.is_ipv4()
                    && (x.ip().is_unspecified() || y.ip().is_unspecified()))
        }
        // An interface or tailnet address on a port a wildcard already holds
        (ListenAddr::Ip(x), _) | (_, ListenAddr::Ip(x)) => x.ip().is_unspecified(),
        (x, y) => x == y,
    }
}

/// The rules for one list: 1 to 16 entries, none twice, no wildcard next to another
/// address of its family on the same port
pub fn validate_list(field: &str, list: &[ListenAddr]) -> Result<(), String> {
    if list.is_empty() {
        return Err(format!("invalid {field}: give at least one address"));
    }
    if list.len() > MAX_ENTRIES {
        return Err(format!(
            "invalid {field}: at most {MAX_ENTRIES} addresses, got {}",
            list.len()
        ));
    }
    for (i, a) in list.iter().enumerate() {
        for b in &list[i + 1..] {
            if a == b {
                return Err(format!("invalid {field}: {a} is listed twice"));
            }
            if clash(a, b) {
                return Err(format!(
                    "invalid {field}: {a} and {b} overlap on port {}; keep one",
                    a.port()
                ));
            }
        }
    }
    Ok(())
}

/// The API and metrics lists must not want the same address
pub fn validate_apart(api: &[ListenAddr], metrics: &[ListenAddr]) -> Result<(), String> {
    for a in api {
        for m in metrics {
            if clash(a, m) {
                return Err(format!(
                    "api.bind {a} and metrics.bind {m} overlap on port {}; use another port for one",
                    a.port()
                ));
            }
        }
    }
    Ok(())
}

/// The addresses interface `name` has now: IPv4 and non-link-local IPv6, in OS order.
/// Empty when there is no such interface or it has no address.
pub fn interface_addrs(name: &str) -> Vec<IpAddr> {
    let Ok(interfaces) = if_addrs::get_if_addrs() else {
        return Vec::new();
    };
    let mut ips: Vec<IpAddr> = interfaces
        .into_iter()
        .filter(|i| i.name == name)
        .map(|i| i.ip())
        .filter(|ip| match ip {
            IpAddr::V4(_) => true,
            IpAddr::V6(v6) => (v6.segments()[0] & 0xffc0) != 0xfe80,
        })
        .collect();
    ips.dedup();
    ips
}

/// The name of the interface holding `ip`, if any (tests use it to find loopback's name)
pub fn interface_of(ip: IpAddr) -> Option<String> {
    if_addrs::get_if_addrs()
        .ok()?
        .into_iter()
        .find(|i| i.ip() == ip)
        .map(|i| i.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Result<ListenAddr, String> {
        s.parse()
    }

    #[test]
    fn parses_every_form_and_prints_it_back() {
        for s in [
            "0.0.0.0:15432",
            "192.168.1.20:80",
            "[::]:15432",
            "[fd00::5]:9742",
            "eth0:15432",
            "tailscale0:1",
            "br-lan.10:65535",
            "tailnet:15432",
            "Ethernet 2:15432",
            "Wi-Fi:15432",
            "Loopback Pseudo-Interface 1:15432",
            "vEthernet (Default Switch):80",
            "Local Area Connection* 3:80",
            "Ethernet-Verbindung 2:80",
            "Подключение по локальной сети:80",
        ] {
            let a = p(s).unwrap();
            assert_eq!(a.to_string(), s);
        }
        assert_eq!(p("TailNet:80").unwrap(), ListenAddr::Tailnet { port: 80 });
        assert!(matches!(
            p("eth0:80").unwrap(),
            ListenAddr::Interface { .. }
        ));
        assert_eq!(p(" 127.0.0.1:5 ").unwrap().port(), 5);
    }

    #[test]
    fn refuses_bad_entries() {
        for s in [
            "nope",
            "1.2.3.4",
            "[::1]",
            "eth0",
            "eth0:0",
            "tailnet:0",
            "eth0:70000",
            ":80",
            "::1:80",
            "1.2.3:80",
            "999.1.1.1:80",
            "[nope]:80",
            "a/b:80",
            "Ethernet 2 :80",
            " :80",
            "Ethernet  2:80",
            "a,b:80",
            "a\\b:80",
            "eth\t0:80",
            "a\"b:80",
            "a=b:80",
        ] {
            assert!(p(s).is_err(), "{s} should be refused");
        }
        assert!(p("::1:80").unwrap_err().contains("brackets"));
        assert!(p(&format!("{}:80", "a".repeat(128))).is_ok());
        assert!(p(&format!("{}:80", "a".repeat(129))).is_err());
        assert_eq!(
            p("Ethernet 2:15432").unwrap(),
            ListenAddr::Interface {
                name: "Ethernet 2".to_string(),
                port: 15432
            }
        );
    }

    #[test]
    fn bind_list_reads_one_or_many_and_writes_one_as_a_string() {
        #[derive(Deserialize, Serialize)]
        struct T {
            bind: BindList,
        }
        let one: T = toml::from_str(r#"bind = "0.0.0.0:1""#).unwrap();
        assert_eq!(one.bind, BindList::One("0.0.0.0:1".into()));
        let many: T = toml::from_str(r#"bind = ["eth0:1", "127.0.0.1:1"]"#).unwrap();
        assert_eq!(many.bind.entries(), vec!["eth0:1", "127.0.0.1:1"]);
        let back = toml::to_string(&T {
            bind: BindList::from_entries(vec!["a:1".into()]),
        })
        .unwrap();
        assert_eq!(back.trim(), r#"bind = "a:1""#);
        assert_eq!(
            BindList::from_env(" eth0:1, [::1]:1 ,").entries(),
            vec!["eth0:1", "[::1]:1"]
        );
    }

    #[test]
    fn list_rules() {
        let list = |v: &[&str]| BindList::Many(v.iter().map(|s| s.to_string()).collect());
        assert!(list(&[])
            .parse("api.bind")
            .unwrap_err()
            .contains("at least one"));
        assert!(list(&["eth0:1", "eth0:1"])
            .parse("api.bind")
            .unwrap_err()
            .contains("twice"));
        assert!(list(&["0.0.0.0:1", "10.0.0.1:1"])
            .parse("api.bind")
            .unwrap_err()
            .contains("overlap"));
        assert!(list(&["0.0.0.0:1", "eth0:1"]).parse("api.bind").is_err());
        assert!(list(&["[::]:1", "[::1]:1"]).parse("api.bind").is_err());
        // Families apart, ports apart: fine
        assert!(list(&["0.0.0.0:1", "[::]:1", "10.0.0.1:2", "tailnet:3"])
            .parse("api.bind")
            .is_ok());
        let many: Vec<String> = (1..=17).map(|p| format!("127.0.0.1:{p}")).collect();
        assert!(BindList::Many(many)
            .parse("api.bind")
            .unwrap_err()
            .contains("at most 16"));
        let err = list(&["0.0.0.0:1", "x/y:2"])
            .parse("metrics.bind")
            .unwrap_err();
        assert!(err.contains("metrics.bind 'x/y:2'"), "{err}");
    }

    #[test]
    fn api_and_metrics_stay_apart() {
        let a = vec![p("0.0.0.0:15432").unwrap()];
        assert!(validate_apart(&a, &[p("0.0.0.0:9742").unwrap()]).is_ok());
        assert!(validate_apart(&a, &[p("127.0.0.1:15432").unwrap()]).is_err());
        assert!(validate_apart(&[p("eth0:1").unwrap()], &[p("eth0:1").unwrap()]).is_err());
        assert!(validate_apart(&[p("[::]:1").unwrap()], &[p("10.0.0.1:1").unwrap()]).is_ok());
    }

    #[test]
    fn loopback_interface_resolves_and_unknown_gives_nothing() {
        let Some(lo) = interface_of(IpAddr::from([127, 0, 0, 1])) else {
            return; // no loopback interface listed on this host
        };
        let entry = ListenAddr::Interface {
            name: lo.clone(),
            port: 8,
        };
        let addrs = entry.resolve_local().unwrap();
        assert!(
            addrs.contains(&SocketAddr::from(([127, 0, 0, 1], 8))),
            "{addrs:?}"
        );
        assert!(addrs.iter().all(|a| a.port() == 8));
        let missing = ListenAddr::Interface {
            name: "nope-037".into(),
            port: 8,
        };
        assert!(missing.resolve_local().unwrap_err().contains("nope-037"));
        assert!(interface_addrs("nope-037").is_empty());
    }
}
