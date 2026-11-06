//! SSDP discovery: NOTIFY alive and byebye, and replies to M-SEARCH (feature 022).
//!
//! One receive socket listens on the SSDP port (shared with other UPnP software through
//! `SO_REUSEADDR`/`SO_REUSEPORT`) and joins the group on every interface; each interface
//! has its own send socket, so the `LOCATION` a player gets is an address it can reach.

use crate::metrics::{self, Kind, Outcome};
use rand::Rng;
use socket2::{Domain, Protocol, Socket, Type};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::task::JoinHandle;

/// The SSDP multicast group and port
pub const GROUP: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::new(239, 255, 255, 250), 1900);

/// Seconds an announcement is good for
const MAX_AGE: u32 = 1800;
/// Seconds between announcements
const ALIVE_EVERY: Duration = Duration::from_secs(900);
/// Announcements at start
const START_BURST: usize = 3;
/// Longest wait before answering a search
const MAX_REPLY_DELAY: u64 = 5;

/// What an SSDP responder announces
#[derive(Debug, Clone)]
pub struct SsdpConfig {
    pub group: SocketAddrV4,
    /// The interface addresses to announce on (the HTTP server listens on each)
    pub ifaces: Vec<Ipv4Addr>,
    /// The HTTP port of the description
    pub port: u16,
    pub uuid: String,
}

/// A running responder
pub struct Ssdp {
    cfg: Arc<SsdpConfig>,
    senders: Arc<Vec<(Ipv4Addr, Ipv4Addr, UdpSocket)>>,
    tasks: Vec<JoinHandle<()>>,
}

fn server_header() -> String {
    format!(
        "{}/1.0 UPnP/1.0 putio-sync/{}",
        std::env::consts::OS,
        env!("CARGO_PKG_VERSION")
    )
}

/// The notification types a MediaServer announces
pub fn notification_types(uuid: &str) -> Vec<String> {
    vec![
        "upnp:rootdevice".to_string(),
        format!("uuid:{uuid}"),
        "urn:schemas-upnp-org:device:MediaServer:1".to_string(),
        "urn:schemas-upnp-org:service:ContentDirectory:1".to_string(),
        "urn:schemas-upnp-org:service:ConnectionManager:1".to_string(),
    ]
}

fn usn(uuid: &str, nt: &str) -> String {
    if nt == format!("uuid:{uuid}") {
        nt.to_string()
    } else {
        format!("uuid:{uuid}::{nt}")
    }
}

fn netmask_of(ip: Ipv4Addr) -> Ipv4Addr {
    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        for i in ifaces {
            if let if_addrs::IfAddr::V4(v4) = i.addr {
                if v4.ip == ip {
                    return v4.netmask;
                }
            }
        }
    }
    Ipv4Addr::new(255, 255, 255, 0)
}

fn same_net(a: Ipv4Addr, b: Ipv4Addr, mask: Ipv4Addr) -> bool {
    let m = u32::from(mask);
    u32::from(a) & m == u32::from(b) & m
}

fn send_socket(iface: Ipv4Addr) -> std::io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    socket.bind(&SocketAddr::V4(SocketAddrV4::new(iface, 0)).into())?;
    socket.set_multicast_if_v4(&iface)?;
    socket.set_multicast_ttl_v4(2)?;
    socket.set_multicast_loop_v4(true)?;
    socket.set_nonblocking(true)?;
    UdpSocket::from_std(socket.into())
}

fn receive_socket(group: SocketAddrV4, ifaces: &[Ipv4Addr]) -> std::io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    #[cfg(unix)]
    socket.set_reuse_port(true)?;
    socket.bind(&SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, group.port())).into())?;
    let mut joined = 0;
    let mut last = None;
    for iface in ifaces {
        match socket.join_multicast_v4(group.ip(), iface) {
            Ok(()) => joined += 1,
            Err(e) => {
                tracing::warn!(%iface, error = %e, "DLNA discovery could not join the multicast group");
                last = Some(e);
            }
        }
    }
    if joined == 0 {
        return Err(last.unwrap_or_else(|| std::io::Error::other("no interface to listen on")));
    }
    socket.set_nonblocking(true)?;
    UdpSocket::from_std(socket.into())
}

impl Ssdp {
    /// Start announcing. Fails when no interface can join the group.
    pub async fn start(cfg: SsdpConfig) -> std::io::Result<Ssdp> {
        let receiver = receive_socket(cfg.group, &cfg.ifaces)?;
        let mut senders = Vec::new();
        for iface in &cfg.ifaces {
            match send_socket(*iface) {
                Ok(s) => senders.push((*iface, netmask_of(*iface), s)),
                Err(e) => {
                    tracing::warn!(%iface, error = %e, "DLNA discovery could not open a send socket")
                }
            }
        }
        if senders.is_empty() {
            return Err(std::io::Error::other("no interface to announce on"));
        }
        let cfg = Arc::new(cfg);
        let senders = Arc::new(senders);
        let mut tasks = Vec::new();

        // Announce at start (a few times, UDP may drop one) then keep announcing
        let (c, s) = (cfg.clone(), senders.clone());
        tasks.push(tokio::spawn(async move {
            for i in 0..START_BURST {
                if i > 0 {
                    tokio::time::sleep(Duration::from_millis(250)).await;
                }
                announce(&c, &s, "ssdp:alive").await;
            }
            loop {
                tokio::time::sleep(ALIVE_EVERY).await;
                announce(&c, &s, "ssdp:alive").await;
            }
        }));

        // Answer searches
        let (c, s) = (cfg.clone(), senders.clone());
        tasks.push(tokio::spawn(async move {
            let mut buf = vec![0u8; 2048];
            loop {
                let Ok((n, from)) = receiver.recv_from(&mut buf).await else {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                };
                let SocketAddr::V4(from) = from else { continue };
                let Some(st) = parse_search(&String::from_utf8_lossy(&buf[..n])) else {
                    continue;
                };
                let (c, s) = (c.clone(), s.clone());
                tokio::spawn(async move { reply(&c, &s, from, st).await });
            }
        }));
        Ok(Ssdp {
            cfg,
            senders,
            tasks,
        })
    }

    /// Say goodbye and stop
    pub async fn stop(self) {
        for task in &self.tasks {
            task.abort();
        }
        for _ in 0..2 {
            announce(&self.cfg, &self.senders, "ssdp:byebye").await;
        }
    }
}

async fn announce(cfg: &SsdpConfig, senders: &[(Ipv4Addr, Ipv4Addr, UdpSocket)], nts: &str) {
    for (iface, _, socket) in senders {
        for nt in notification_types(&cfg.uuid) {
            let mut msg = format!(
                "NOTIFY * HTTP/1.1\r\nHOST: {}\r\nNT: {nt}\r\nNTS: {nts}\r\nUSN: {}\r\n",
                cfg.group,
                usn(&cfg.uuid, &nt)
            );
            if nts == "ssdp:alive" {
                msg.push_str(&format!(
                    "CACHE-CONTROL: max-age={MAX_AGE}\r\nLOCATION: http://{iface}:{}/description.xml\r\nSERVER: {}\r\n",
                    cfg.port,
                    server_header()
                ));
            }
            msg.push_str("\r\n");
            if let Err(e) = socket.send_to(msg.as_bytes(), cfg.group).await {
                tracing::debug!(%iface, error = %e, "DLNA announcement not sent");
            }
        }
    }
}

/// The ST of an M-SEARCH and its MX, or `None` for anything else
fn parse_search(text: &str) -> Option<(String, u64)> {
    let mut lines = text.lines();
    let first = lines.next()?;
    if !first.to_ascii_uppercase().starts_with("M-SEARCH") {
        return None;
    }
    let (mut st, mut mx) = (None, 1u64);
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        match name.trim().to_ascii_uppercase().as_str() {
            "ST" => st = Some(value.trim().to_string()),
            "MX" => mx = value.trim().parse().unwrap_or(1),
            _ => {}
        }
    }
    Some((st?, mx))
}

async fn reply(
    cfg: &SsdpConfig,
    senders: &[(Ipv4Addr, Ipv4Addr, UdpSocket)],
    to: SocketAddrV4,
    (st, mx): (String, u64),
) {
    let all = notification_types(&cfg.uuid);
    let wanted: Vec<&String> = if st == "ssdp:all" {
        all.iter().collect()
    } else {
        all.iter().filter(|nt| **nt == st).collect()
    };
    if wanted.is_empty() {
        return;
    }
    // The interface the player is on
    let (iface, _, socket) = senders
        .iter()
        .find(|(ip, mask, _)| same_net(*ip, *to.ip(), *mask))
        .unwrap_or(&senders[0]);
    let delay = rand::thread_rng().gen_range(0..=(mx.clamp(1, MAX_REPLY_DELAY) * 1000));
    tokio::time::sleep(Duration::from_millis(delay)).await;
    for nt in wanted {
        let msg = format!(
            "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age={MAX_AGE}\r\nEXT:\r\n\
LOCATION: http://{iface}:{}/description.xml\r\nSERVER: {}\r\nST: {nt}\r\nUSN: {}\r\n\r\n",
            cfg.port,
            server_header(),
            usn(&cfg.uuid, nt)
        );
        let outcome = match socket.send_to(msg.as_bytes(), to).await {
            Ok(_) => Outcome::Ok,
            Err(_) => Outcome::Error,
        };
        metrics::request(Kind::Ssdp, outcome);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_searches() {
        let q = "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 3\r\nST: ssdp:all\r\n\r\n";
        assert_eq!(parse_search(q), Some(("ssdp:all".to_string(), 3)));
        assert_eq!(parse_search("NOTIFY * HTTP/1.1\r\nST: x\r\n\r\n"), None);
        assert_eq!(parse_search("M-SEARCH * HTTP/1.1\r\nMX: 2\r\n\r\n"), None);
        let lower = "M-SEARCH * HTTP/1.1\r\nst: upnp:rootdevice\r\n\r\n";
        assert_eq!(
            parse_search(lower),
            Some(("upnp:rootdevice".to_string(), 1))
        );
    }

    #[test]
    fn usn_of_the_uuid_is_bare() {
        assert_eq!(usn("u", "uuid:u"), "uuid:u");
        assert_eq!(usn("u", "upnp:rootdevice"), "uuid:u::upnp:rootdevice");
        assert_eq!(notification_types("u").len(), 5);
    }

    #[test]
    fn same_net_uses_the_mask() {
        let m = Ipv4Addr::new(255, 255, 255, 0);
        assert!(same_net(
            Ipv4Addr::new(10, 0, 0, 5),
            Ipv4Addr::new(10, 0, 0, 77),
            m
        ));
        assert!(!same_net(
            Ipv4Addr::new(10, 0, 0, 5),
            Ipv4Addr::new(10, 0, 1, 5),
            m
        ));
    }
}
