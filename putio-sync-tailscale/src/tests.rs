//! The service against a fake tailscaled: a LocalAPI on a temp unix socket and a fake
//! CLI script. Nothing here needs Tailscale installed.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const STATUS: &str = r#"{
    "Version": "1.76.1",
    "TUN": true,
    "BackendState": "Running",
    "TailscaleIPs": ["100.101.102.103", "fd7a:115c:a1e0::1"],
    "Self": {"HostName": "nas", "DNSName": "nas.tail1234.ts.net.", "UserID": 7},
    "CurrentTailnet": {"Name": "example.com", "MagicDNSSuffix": "tail1234.ts.net", "MagicDNSEnabled": true},
    "CertDomains": ["nas.tail1234.ts.net"],
    "User": {"7": {"LoginName": "alice@example.com", "DisplayName": "Alice"}}
}"#;

const SERVE: &str = r#"{"TCP":{"443":{"HTTPS":true}},"Web":{"nas.tail1234.ts.net:443":{"Handlers":{"/":{"Proxy":"http://127.0.0.1:15432"}}}}}"#;

const ALICE: &str = r#"{"Node":{"Name":"laptop.tail1234.ts.net.","Tags":[]},"UserProfile":{"LoginName":"alice@example.com","DisplayName":"Alice"}}"#;
const TAGGED: &str = r#"{"Node":{"Name":"server.tail1234.ts.net.","Tags":["tag:server"]},"UserProfile":{"LoginName":"tagged-devices"}}"#;

/// A fake LocalAPI: answers by path; `None` hangs
struct Fake {
    _dir: tempfile::TempDir,
    socket: PathBuf,
    hits: Arc<AtomicUsize>,
}

fn fake(routes: Vec<(&'static str, Option<(u16, &'static str)>)>) -> Fake {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("tailscaled.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    let routes = Arc::new(routes);
    tokio::spawn(async move {
        loop {
            let Ok((mut conn, _)) = listener.accept().await else {
                return;
            };
            let routes = routes.clone();
            counter.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut req = Vec::new();
                let mut buf = [0u8; 1024];
                while !req.windows(4).any(|w| w == b"\r\n\r\n") {
                    let n = conn.read(&mut buf).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    req.extend_from_slice(&buf[..n]);
                }
                let req = String::from_utf8_lossy(&req).to_string();
                assert!(req.contains("Sec-Tailscale: localapi"), "{req}");
                let path = req.split_whitespace().nth(1).unwrap_or("").to_string();
                let found = routes.iter().find(|(p, _)| path.starts_with(p));
                match found {
                    Some((_, Some((code, body)))) => {
                        let resp = format!("HTTP/1.0 {code} X\r\nContent-Type: application/json\r\n\r\n{body}");
                        let _ = conn.write_all(resp.as_bytes()).await;
                    }
                    Some((_, None)) => {
                        tokio::time::sleep(Duration::from_secs(30)).await;
                    }
                    None => {
                        let _ = conn.write_all(b"HTTP/1.0 404 Not Found\r\n\r\nnot found").await;
                    }
                }
            });
        }
    });
    Fake {
        _dir: dir,
        socket,
        hits,
    }
}

fn service(socket: &Path) -> TailscaleService {
    TailscaleService::new(socket, Cli::Off).with_timeout(Duration::from_millis(300))
}

fn loopback() -> Option<SocketAddr> {
    Some("127.0.0.1:50000".parse().unwrap())
}

fn tailnet_peer() -> Option<SocketAddr> {
    Some("100.90.1.2:50000".parse().unwrap())
}

#[tokio::test]
async fn running_node_with_serve() {
    let f = fake(vec![
        ("/localapi/v0/status", Some((200, STATUS))),
        ("/localapi/v0/serve-config", Some((200, SERVE))),
    ]);
    let ts = service(&f.socket);
    let s = ts.status().await;
    assert_eq!(s.state, State::Running);
    assert_eq!(s.via, Some(Via::Socket));
    assert_eq!(s.dns_name.as_deref(), Some("nas.tail1234.ts.net"));
    assert_eq!(s.serve_for_port(15432).len(), 1);
    assert_eq!(
        tailnet_bind(&s, 15432).unwrap(),
        "100.101.102.103:15432".parse().unwrap()
    );

    // Cached: a second call doesn't ask again
    let before = f.hits.load(Ordering::SeqCst);
    let _ = ts.status().await;
    assert_eq!(f.hits.load(Ordering::SeqCst), before);
    let _ = ts.refresh().await;
    assert!(f.hits.load(Ordering::SeqCst) > before);
}

#[tokio::test]
async fn needs_login_has_no_bind() {
    let f = fake(vec![(
        "/localapi/v0/status",
        Some((200, r#"{"BackendState":"NeedsLogin"}"#)),
    )]);
    let s = service(&f.socket).status().await;
    assert_eq!(s.state, State::NeedsLogin);
    assert!(tailnet_bind(&s, 15432).unwrap_err().contains("signed out"));
}

#[tokio::test]
async fn hang_junk_and_missing() {
    let f = fake(vec![("/localapi/v0/status", None)]);
    let started = Instant::now();
    let s = service(&f.socket).status().await;
    assert_eq!(s.state, State::Unreachable);
    assert!(s.error.unwrap().contains("in time"));
    assert!(started.elapsed() < Duration::from_secs(2));

    let f = fake(vec![("/localapi/v0/status", Some((200, "not json")))]);
    let s = service(&f.socket).status().await;
    assert_eq!(s.state, State::Unreachable);
    assert!(tailnet_bind(&s, 1).is_err());

    let f = fake(vec![("/localapi/v0/status", Some((403, "denied")))]);
    assert_eq!(service(&f.socket).status().await.state, State::Unreachable);

    let dir = tempfile::tempdir().unwrap();
    let s = service(&dir.path().join("none.sock")).status().await;
    assert_eq!(s.state, State::NotFound);
    assert!(tailnet_bind(&s, 1).unwrap_err().contains("isn't on"));
}

#[tokio::test]
async fn userspace_has_no_bind() {
    let json: &'static str = Box::leak(STATUS.replace("\"TUN\": true", "\"TUN\": false").into_boxed_str());
    let f = fake(vec![("/localapi/v0/status", Some((200, json)))]);
    let s = service(&f.socket).status().await;
    assert!(s.userspace);
    assert!(tailnet_bind(&s, 1).unwrap_err().contains("userspace"));
}

#[tokio::test]
async fn identities() {
    let f = fake(vec![
        ("/localapi/v0/whois?addr=100.90.1.2", Some((200, ALICE))),
        ("/localapi/v0/whois?addr=100.90.1.3", Some((200, TAGGED))),
    ]);
    let ts = service(&f.socket);

    // serve's headers on loopback
    match ts.identity_for(loopback(), Some("bob@example.com"), Some("Bob")).await {
        Lookup::Found(id) => {
            assert_eq!(id.login, "bob@example.com");
            assert_eq!(id.name, "Bob");
            assert_eq!(id.via, IdentityVia::Serve);
        }
        other => panic!("{other:?}"),
    }
    // Loopback without headers, headers from the LAN, no peer at all: nothing
    assert_eq!(ts.identity_for(loopback(), None, None).await, Lookup::None);
    assert_eq!(
        ts.identity_for(Some("192.168.1.9:1".parse().unwrap()), Some("bob@example.com"), None)
            .await,
        Lookup::None
    );
    assert_eq!(ts.identity_for(None, Some("bob@example.com"), None).await, Lookup::None);

    // whois for a tailnet peer, cached
    match ts.identity_for(tailnet_peer(), Some("mallory@evil"), None).await {
        Lookup::Found(id) => {
            assert_eq!(id.login, "alice@example.com");
            assert_eq!(id.device, "laptop");
            assert_eq!(id.via, IdentityVia::Whois);
        }
        other => panic!("{other:?}"),
    }
    let before = f.hits.load(Ordering::SeqCst);
    let _ = ts.identity_for(tailnet_peer(), None, None).await;
    assert_eq!(f.hits.load(Ordering::SeqCst), before);

    // Tagged and unknown peers are refused
    assert!(matches!(
        ts.identity_for(Some("100.90.1.3:1".parse().unwrap()), None, None).await,
        Lookup::Refused { via: IdentityVia::Whois, .. }
    ));
    assert!(matches!(
        ts.identity_for(Some("100.90.1.4:1".parse().unwrap()), None, None).await,
        Lookup::Refused { .. }
    ));

    // configure() forgets cached answers
    ts.configure(f.socket.clone());
    let _ = ts.identity_for(tailnet_peer(), None, None).await;
    assert!(f.hits.load(Ordering::SeqCst) > before);
}

#[tokio::test]
async fn daemon_down_refuses_tailnet_peers() {
    let dir = tempfile::tempdir().unwrap();
    let ts = service(&dir.path().join("none.sock"));
    assert!(matches!(
        ts.identity_for(tailnet_peer(), None, None).await,
        Lookup::Refused { .. }
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn cli_fallback() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let bin = dir.path().join("tailscale");
    let script = format!(
        "#!/bin/sh\ncase \"$1\" in\n  status) cat <<'EOF'\n{STATUS}\nEOF\n;;\n  serve) echo '{SERVE}' ;;\n  whois) [ \"$3\" = 100.90.1.2 ] && echo '{ALICE}' || {{ echo 'no match for IP' >&2; exit 1; }} ;;\nesac\n"
    );
    std::fs::write(&bin, script).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();

    let ts = TailscaleService::new(dir.path().join("none.sock"), Cli::Path(bin.clone()));
    let s = ts.status().await;
    assert_eq!(s.state, State::Running, "{s:?}");
    assert_eq!(s.via, Some(Via::Cli));
    assert_eq!(s.serve.len(), 1);
    assert!(matches!(
        ts.identity_for(tailnet_peer(), None, None).await,
        Lookup::Found(_)
    ));
    match ts.identity_for(Some("100.90.1.9:1".parse().unwrap()), None, None).await {
        Lookup::Refused { reason, .. } => assert!(reason.contains("no match"), "{reason}"),
        other => panic!("{other:?}"),
    }

    // A failing CLI is unreachable, with its message
    std::fs::write(&bin, "#!/bin/sh\necho 'failed to connect to local tailscaled' >&2\nexit 1\n").unwrap();
    let ts = TailscaleService::new(dir.path().join("none.sock"), Cli::Path(bin));
    let s = ts.status().await;
    assert_eq!(s.state, State::Unreachable);
    assert_eq!(s.via, Some(Via::Cli));
    assert!(s.error.unwrap().contains("failed to connect"));
}
