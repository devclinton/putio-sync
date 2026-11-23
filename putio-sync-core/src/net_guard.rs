//! Which addresses a fetch on someone else's say-so may reach (plugin install by URL, the
//! `.torrent` URL of a Transmission RPC `torrent-add`). [`blocked`] names why an address
//! is refused; [`GuardedResolver`] applies it to every name a client resolves, and
//! [`check_url`] to a literal address, so each redirect hop is checked.

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs};

/// The environment variable that lets those fetches reach local addresses (a plugin server
/// or indexer on this machine). Cloud metadata addresses stay refused.
pub const ALLOW_LOCAL_ENV: &str = "PUTIO_SYNC_ALLOW_LOCAL_FETCH";

/// How far a fetch may reach
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// Public addresses only
    Public,
    /// Public and private LAN addresses (10/8, 172.16/12, 192.168/16, fc00::/7, 100.64/10)
    Lan,
    /// Everything except unspecified, multicast and cloud metadata addresses
    Local,
}

impl Reach {
    /// `base`, or [`Reach::Local`] when [`ALLOW_LOCAL_ENV`] is `1` or `true`
    pub fn or_opt_in(base: Reach) -> Reach {
        match std::env::var(ALLOW_LOCAL_ENV) {
            Ok(v) if v == "1" || v.eq_ignore_ascii_case("true") => Reach::Local,
            _ => base,
        }
    }
}

/// Why `ip` is not allowed at `reach`, or `None` when it is
pub fn blocked(ip: IpAddr, reach: Reach) -> Option<&'static str> {
    let ip = match ip {
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => IpAddr::V6(v6),
        },
        v4 => v4,
    };
    if ip.is_unspecified() {
        return Some("an unspecified address");
    }
    if ip.is_multicast() {
        return Some("a multicast address");
    }
    if is_metadata(ip) {
        return Some("a cloud metadata address");
    }
    if reach == Reach::Local {
        return None;
    }
    if ip.is_loopback() {
        return Some("a loopback address");
    }
    if is_link_local(ip) {
        return Some("a link-local address");
    }
    if reach == Reach::Public && is_private(ip) {
        return Some("a private address");
    }
    None
}

/// Whether `ip` is a public address (what [`Reach::Public`] allows)
pub fn is_public(ip: IpAddr) -> bool {
    blocked(ip, Reach::Public).is_none()
}

fn is_metadata(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4 == Ipv4Addr::new(169, 254, 169, 254),
        IpAddr::V6(v6) => v6 == Ipv6Addr::new(0xfd00, 0xec2, 0, 0, 0, 0, 0, 0x254),
    }
}

fn is_link_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_link_local(),
        IpAddr::V6(v6) => v6.segments()[0] & 0xffc0 == 0xfe80,
    }
}

fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_private() || v4.is_broadcast() || o[0] == 0 || (o[0] == 100 && o[1] & 0xc0 == 64)
        }
        IpAddr::V6(v6) => v6.segments()[0] & 0xfe00 == 0xfc00,
    }
}

/// `Err` with the reason when `url` names an address `reach` refuses: a literal IP, which a
/// client connects to without resolving it. Names are checked by [`GuardedResolver`].
pub fn check_url(url: &reqwest::Url, reach: Reach) -> Result<(), String> {
    let host = url.host_str().unwrap_or_default();
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if let Ok(ip) = host.parse::<IpAddr>() {
        if let Some(why) = blocked(ip, reach) {
            return Err(format!("{} is {}", ip, why));
        }
    }
    Ok(())
}

/// A DNS resolver that drops the addresses `reach` refuses, and fails when none are left.
/// The client connects to what it returns, so a name that points inside is refused and a
/// rebinding answer cannot slip through between a check and the connection.
#[derive(Debug, Clone, Copy)]
pub struct GuardedResolver {
    reach: Reach,
}

impl GuardedResolver {
    /// A resolver for `reach`
    pub fn new(reach: Reach) -> Self {
        GuardedResolver { reach }
    }
}

impl Resolve for GuardedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let reach = self.reach;
        Box::pin(async move {
            let host = name.as_str().to_string();
            let found = tokio::task::spawn_blocking(move || {
                (host.as_str(), 0)
                    .to_socket_addrs()
                    .map(|found| found.collect::<Vec<SocketAddr>>())
            })
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
            let mut refused = None;
            let allowed: Vec<SocketAddr> = found
                .into_iter()
                .filter(|a| match blocked(a.ip(), reach) {
                    Some(why) => {
                        refused = Some(format!("{} is {}", a.ip(), why));
                        false
                    }
                    None => true,
                })
                .collect();
            if allowed.is_empty() {
                let why = refused.unwrap_or_else(|| "no address".to_string());
                return Err(format!("address not allowed: {}", why).into());
            }
            Ok(Box::new(allowed.into_iter()) as Addrs)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn public_means_routable_on_the_internet() {
        for s in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111", "172.32.0.1"] {
            assert!(is_public(ip(s)), "{s}");
        }
        for s in [
            "127.0.0.1",
            "::1",
            "0.0.0.0",
            "::",
            "169.254.169.254",
            "169.254.1.1",
            "fe80::1",
            "fd00:ec2::254",
            "224.0.0.1",
            "ff02::1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "100.64.0.1",
            "fc00::1",
            "fd12::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "255.255.255.255",
        ] {
            assert!(!is_public(ip(s)), "{s}");
        }
    }

    #[test]
    fn lan_allows_private_but_not_loopback_link_local_or_metadata() {
        for s in ["192.168.1.9", "10.0.0.2", "fd12::1", "8.8.8.8"] {
            assert_eq!(blocked(ip(s), Reach::Lan), None, "{s}");
        }
        for s in [
            "127.0.0.1",
            "::1",
            "169.254.169.254",
            "fe80::1",
            "fd00:ec2::254",
            "0.0.0.0",
        ] {
            assert!(blocked(ip(s), Reach::Lan).is_some(), "{s}");
        }
    }

    #[test]
    fn local_keeps_only_metadata_unspecified_and_multicast_out() {
        assert_eq!(blocked(ip("127.0.0.1"), Reach::Local), None);
        assert_eq!(blocked(ip("10.0.0.1"), Reach::Local), None);
        assert!(blocked(ip("169.254.169.254"), Reach::Local).is_some());
        assert!(blocked(ip("fd00:ec2::254"), Reach::Local).is_some());
        assert!(blocked(ip("0.0.0.0"), Reach::Local).is_some());
    }

    #[test]
    fn a_literal_address_in_a_url_is_checked() {
        let url = |s: &str| reqwest::Url::parse(s).unwrap();
        assert!(check_url(&url("http://127.0.0.1:8080/x"), Reach::Public).is_err());
        assert!(check_url(&url("http://[::1]/x"), Reach::Lan).is_err());
        assert!(check_url(&url("http://[fd00:ec2::254]/"), Reach::Local).is_err());
        assert!(check_url(&url("http://192.168.1.5/x"), Reach::Lan).is_ok());
        assert!(check_url(&url("http://example.com/x"), Reach::Public).is_ok());
    }

    #[tokio::test]
    async fn the_resolver_refuses_a_name_that_points_inside() {
        let client = reqwest::Client::builder()
            .dns_resolver(std::sync::Arc::new(GuardedResolver::new(Reach::Public)))
            .build()
            .unwrap();
        let err = client.get("http://localhost:9/").send().await.unwrap_err();
        assert!(
            format!("{:?}", err).contains("address not allowed"),
            "{err:?}"
        );
    }
}
