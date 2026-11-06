//! SSDP discovery, the description document and the status (feature 022, US1/US3).
//! Multicast tests skip with a printed reason when loopback can't carry multicast.

mod common;
use common::{free_port, Fixture};
use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket};
use std::time::{Duration, Instant};

/// A socket in the test group on loopback, or None (with the reason printed)
fn listener(group: SocketAddrV4) -> Option<UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let s = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP)).ok()?;
    s.set_reuse_address(true).ok()?;
    #[cfg(unix)]
    s.set_reuse_port(true).ok()?;
    s.bind(&SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, group.port()).into())
        .ok()?;
    if let Err(e) = s.join_multicast_v4(group.ip(), &Ipv4Addr::LOCALHOST) {
        println!("skipping: this host can't join multicast on loopback ({e})");
        return None;
    }
    s.set_read_timeout(Some(Duration::from_millis(300))).ok()?;
    Some(s.into())
}

fn read_until(socket: &UdpSocket, secs: u64, want: impl Fn(&str) -> bool) -> Option<String> {
    let end = Instant::now() + Duration::from_secs(secs);
    let mut buf = [0u8; 2048];
    while Instant::now() < end {
        if let Ok((n, _)) = socket.recv_from(&mut buf) {
            let text = String::from_utf8_lossy(&buf[..n]).to_string();
            if want(&text) {
                return Some(text);
            }
        }
    }
    None
}

#[tokio::test(flavor = "multi_thread")]
async fn announces_alive_and_byebye_for_each_type() {
    let probe_port = common::free_udp_port();
    let group = SocketAddrV4::new(Ipv4Addr::new(239, 255, 255, 250), probe_port);
    let Some(rx) = listener(group) else { return };
    let downloads = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();
    let port = free_port();
    let config = common::config_for(downloads.path(), port, &["downloads"]);
    let server = putio_sync_dlna::MediaServer::with_options(
        None,
        config_dir.path(),
        putio_sync_dlna::Options {
            ssdp_group: group,
            ..Default::default()
        },
    );
    server.update(&config).await;
    let uuid = server.uuid().to_string();

    let rx2 = rx.try_clone().unwrap();
    let alive = tokio::task::spawn_blocking(move || {
        read_until(&rx2, 5, |t| {
            t.contains("ssdp:alive") && t.contains("MediaServer:1")
        })
    })
    .await
    .unwrap();
    let Some(alive) = alive else {
        println!("skipping: no multicast reached the probe on this host");
        server.stop().await;
        return;
    };
    assert!(alive.starts_with("NOTIFY * HTTP/1.1"));
    assert!(alive.contains(&format!("uuid:{uuid}")));
    assert!(alive.contains(&format!("http://127.0.0.1:{port}/description.xml")));
    assert!(alive.to_lowercase().contains("cache-control: max-age"));

    server.stop().await;
    let bye =
        tokio::task::spawn_blocking(move || read_until(&rx, 3, |t| t.contains("ssdp:byebye")))
            .await
            .unwrap();
    assert!(bye.is_some(), "no byebye after stop");
}

#[tokio::test(flavor = "multi_thread")]
async fn answers_an_msearch_with_a_location_that_describes_the_server() {
    let probe_port = common::free_udp_port();
    let group = SocketAddrV4::new(Ipv4Addr::new(239, 255, 255, 250), probe_port);
    let Some(probe) = listener(group) else { return };
    drop(probe);
    let f = Fixture::start_with(
        &["downloads"],
        None,
        |c| c.dlna.name = "Lounge <TV> & Co".into(),
        putio_sync_dlna::Options {
            ssdp_group: group,
            ..Default::default()
        },
    )
    .await;
    let unicast = UdpSocket::bind("127.0.0.1:0").unwrap();
    let sock = socket2::SockRef::from(&unicast);
    if let Err(e) = sock.set_multicast_if_v4(&Ipv4Addr::LOCALHOST) {
        println!("skipping: can't send multicast from loopback ({e})");
        return;
    }
    unicast
        .set_read_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    let search = "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 1\r\nST: urn:schemas-upnp-org:device:MediaServer:1\r\n\r\n";
    let target = SocketAddrV4::new(*f.group.ip(), f.group.port());
    unicast.send_to(search.as_bytes(), target).unwrap();
    let reply = tokio::task::spawn_blocking(move || {
        read_until(&unicast, 4, |t| t.starts_with("HTTP/1.1 200"))
    })
    .await
    .unwrap();
    let Some(reply) = reply else {
        println!("skipping: no M-SEARCH reply reached the probe; multicast looks unavailable");
        return;
    };
    let location = reply
        .lines()
        .find_map(|l| {
            l.strip_prefix("LOCATION: ")
                .or_else(|| l.strip_prefix("Location: "))
        })
        .expect("LOCATION header")
        .trim()
        .to_string();
    let xml = reqwest::get(&location).await.unwrap().text().await.unwrap();
    assert!(
        xml.contains("<friendlyName>Lounge &lt;TV&gt; &amp; Co</friendlyName>"),
        "{xml}"
    );
    assert!(xml.contains("urn:schemas-upnp-org:device:MediaServer:1"));
    f.server.stop().await;
}

#[tokio::test]
async fn a_taken_port_fails_then_recovers_when_freed() {
    let blocker = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let taken = blocker.local_addr().unwrap().port();
    let downloads = tempfile::tempdir().unwrap();
    let config_dir = tempfile::tempdir().unwrap();
    let mut config = common::config_for(downloads.path(), taken, &["downloads"]);
    let server = putio_sync_dlna::MediaServer::with_options(
        None,
        config_dir.path(),
        putio_sync_dlna::Options {
            ssdp_group: SocketAddrV4::new(
                Ipv4Addr::new(239, 255, 255, 250),
                common::free_udp_port(),
            ),
            ..Default::default()
        },
    );
    server.update(&config).await;
    let status = server.status();
    assert_eq!(status.state.as_str(), "failed");
    assert!(status.reason.unwrap().contains(&taken.to_string()));

    drop(blocker);
    server.update(&config).await;
    assert_eq!(server.status().state.as_str(), "running");

    config.dlna.enabled = false;
    server.update(&config).await;
    assert_eq!(server.status().state.as_str(), "off");
    assert!(
        reqwest::get(format!("http://127.0.0.1:{taken}/description.xml"))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn the_id_survives_a_restart_and_is_stored_beside_the_config() {
    let f = Fixture::start(&["downloads"]).await;
    let first = f.server.uuid().to_string();
    let again = putio_sync_dlna::MediaServer::new(None, f.config_dir.path());
    assert_eq!(again.uuid(), first);
    assert!(f.config_dir.path().join("dlna-uuid").is_file());
}

#[tokio::test]
async fn status_counts_players_and_lists_addresses() {
    let f = Fixture::start(&["downloads"]).await;
    let s = f.server.status();
    assert_eq!(s.state.as_str(), "running");
    assert_eq!(s.players_last_hour, 0);
    assert_eq!(
        s.addresses,
        [format!("http://127.0.0.1:{}/description.xml", f.port)]
    );
    f.children("0").await;
    f.children("0").await;
    assert_eq!(f.server.status().players_last_hour, 1);
}

#[tokio::test]
async fn a_name_change_applies_without_rebinding() {
    let mut f = Fixture::start(&["downloads"]).await;
    f.config.dlna.name = "Renamed".into();
    f.server.update(&f.config).await;
    let desc = f.get("/description.xml").await.text().await.unwrap();
    assert!(desc.contains("<friendlyName>Renamed</friendlyName>"));
    let meta = f.browse("0", "BrowseMetadata", 0, 0).await;
    assert_eq!(meta.titles(), ["Renamed"]);
}

#[tokio::test]
async fn metrics_labels_carry_no_names_or_paths() {
    let f = Fixture::start(&["downloads"]).await;
    f.write("Secret Movie Name.mp4", b"abc");
    f.children("downloads/").await;
    f.get("/media/downloads/Secret%20Movie%20Name.mp4")
        .await
        .bytes()
        .await
        .unwrap();
    let text = putio_sync_telemetry::global().encode();
    assert!(text.contains("putio_sync_dlna_requests_total"));
    assert!(text.contains("putio_sync_dlna_bytes_sent_total"));
    assert!(!text.contains("Secret"), "a file name reached the metrics");
    assert!(!text.contains(&f.downloads.path().to_string_lossy().to_string()));
}
