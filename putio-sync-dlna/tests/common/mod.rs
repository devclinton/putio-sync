//! Helpers for the media server tests: a running server on loopback, a SOAP client, and
//! captured logs.
#![allow(dead_code)]

use putio_sync_core::config::{Config, DlnaConfig};
use putio_sync_core::listen::BindList;
use putio_sync_dlna::soap;
use putio_sync_dlna::{MediaServer, Options};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

/// A port nobody listens on right now
pub fn free_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    l.local_addr().unwrap().port()
}

/// A random UDP port for the SSDP group, so tests don't meet real UPnP software
pub fn free_udp_port() -> u16 {
    let s = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    s.local_addr().unwrap().port()
}

pub struct Fixture {
    pub downloads: TempDir,
    pub config_dir: TempDir,
    pub server: MediaServer,
    pub config: Config,
    pub port: u16,
    pub group: SocketAddrV4,
}

pub fn config_for(downloads: &std::path::Path, port: u16, roots: &[&str]) -> Config {
    let mut config = Config::builder()
        .target_directory(downloads.to_path_buf())
        .build();
    config.dlna = DlnaConfig {
        enabled: true,
        name: "Test Server".into(),
        port,
        bind: BindList::One(format!("127.0.0.1:{port}")),
        roots: roots.iter().map(|r| r.to_string()).collect(),
        putio_folder: None,
    };
    config
}

impl Fixture {
    /// A server sharing `roots` of a fresh download folder, already running
    pub async fn start(roots: &[&str]) -> Fixture {
        Fixture::start_with(roots, None, |_| {}, Options::default()).await
    }

    pub async fn start_with(
        roots: &[&str],
        client: Option<Arc<putio_sync_client::PutioApiClient>>,
        tweak: impl FnOnce(&mut Config),
        mut options: Options,
    ) -> Fixture {
        let downloads = tempfile::tempdir().unwrap();
        let config_dir = tempfile::tempdir().unwrap();
        let mut port = free_port();
        let group = SocketAddrV4::new(Ipv4Addr::new(239, 255, 255, 250), free_udp_port());
        options.ssdp_group = group;
        let mut config = config_for(downloads.path(), port, roots);
        tweak(&mut config);
        let server = MediaServer::with_options(client, config_dir.path(), options);
        server.update(&config).await;
        // `free_port` lets the port go before the server binds it, so a test running
        // alongside can take the same one; pick another when the bind loses that race.
        for _ in 0..5 {
            let status = server.status();
            let lost = status.state == putio_sync_dlna::State::Failed
                && config.dlna.port == port
                && status.reason.is_some_and(|r| r.starts_with("Can't listen"));
            if !lost {
                break;
            }
            port = free_port();
            config.dlna.port = port;
            config.dlna.bind = BindList::One(format!("127.0.0.1:{port}"));
            server.update(&config).await;
        }
        Fixture {
            downloads,
            config_dir,
            server,
            config,
            port,
            group,
        }
    }

    pub fn base(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub fn write(&self, rel: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = self.downloads.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, bytes).unwrap();
        path
    }

    pub async fn browse(&self, id: &str, flag: &str, start: u32, count: u32) -> Reply {
        browse(&self.base(), id, flag, start, count).await
    }

    pub async fn children(&self, id: &str) -> Reply {
        self.browse(id, "BrowseDirectChildren", 0, 0).await
    }

    pub async fn get(&self, path: &str) -> reqwest::Response {
        reqwest::get(format!("{}{path}", self.base()))
            .await
            .unwrap()
    }
}

pub struct Reply {
    pub status: u16,
    pub body: String,
}

impl Reply {
    /// The unescaped DIDL-Lite of a Browse reply
    pub fn didl(&self) -> String {
        assert_eq!(self.status, 200, "{}", self.body);
        soap::parse_action(&self.body)
            .unwrap()
            .arg("Result")
            .unwrap()
            .to_string()
    }

    pub fn arg(&self, name: &str) -> String {
        soap::parse_action(&self.body)
            .unwrap()
            .arg(name)
            .unwrap_or_default()
            .to_string()
    }

    /// The UPnP error code of a fault, if it is one
    pub fn fault(&self) -> Option<u32> {
        let rest = self.body.split("<errorCode>").nth(1)?;
        rest.split("</errorCode>").next()?.parse().ok()
    }

    /// (tag, id, title) of every container and item in the DIDL
    pub fn entries(&self) -> Vec<(String, String, String)> {
        entries(&self.didl())
    }

    pub fn titles(&self) -> Vec<String> {
        self.entries().into_iter().map(|e| e.2).collect()
    }
}

pub fn entries(didl: &str) -> Vec<(String, String, String)> {
    use quick_xml::events::Event;
    let mut reader = quick_xml::Reader::from_str(didl);
    let mut out: Vec<(String, String, String)> = Vec::new();
    let mut in_title = false;
    loop {
        match reader.read_event().unwrap() {
            Event::Start(e) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if name == "container" || name == "item" {
                    let id = e
                        .attributes()
                        .flatten()
                        .find(|a| a.key.as_ref() == b"id")
                        .map(|a| a.unescape_value().unwrap().to_string())
                        .unwrap_or_default();
                    out.push((name, id, String::new()));
                } else if name == "dc:title" {
                    in_title = true;
                }
            }
            Event::Text(t) if in_title => {
                out.last_mut().unwrap().2 = t.unescape().unwrap().to_string();
                in_title = false;
            }
            Event::Eof => break,
            _ => {}
        }
    }
    out
}

pub async fn soap_call(
    base: &str,
    control: &str,
    service: &str,
    action: &str,
    args: &str,
) -> Reply {
    let body = format!(
        "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\">\
<s:Body><u:{action} xmlns:u=\"{service}\">{args}</u:{action}></s:Body></s:Envelope>"
    );
    let response = reqwest::Client::new()
        .post(format!("{base}{control}"))
        .header("Content-Type", "text/xml; charset=\"utf-8\"")
        .header("SOAPACTION", format!("\"{service}#{action}\""))
        .body(body)
        .send()
        .await
        .unwrap();
    Reply {
        status: response.status().as_u16(),
        body: response.text().await.unwrap(),
    }
}

pub async fn browse(base: &str, id: &str, flag: &str, start: u32, count: u32) -> Reply {
    let args = format!(
        "<ObjectID>{}</ObjectID><BrowseFlag>{flag}</BrowseFlag><Filter>*</Filter>\
<StartingIndex>{start}</StartingIndex><RequestedCount>{count}</RequestedCount><SortCriteria></SortCriteria>",
        soap::escape(id)
    );
    soap_call(base, "/ctl/cds", soap::CDS, "Browse", &args).await
}

/// A GET with the path sent exactly as given (no URL normalising)
pub async fn raw_get(port: u16, path: &str) -> (u16, Vec<u8>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    let request = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut raw = Vec::new();
    let _ = stream.read_to_end(&mut raw).await;
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| p + 4)
        .unwrap_or(raw.len());
    let head = String::from_utf8_lossy(&raw[..split]).to_string();
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    (status, raw[split..].to_vec())
}

/// Log lines captured while the guard lives (single-threaded test runtime only)
#[derive(Clone, Default)]
pub struct Logs(Arc<Mutex<Vec<u8>>>);

impl Logs {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).to_string()
    }
}

impl std::io::Write for Logs {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Logs {
    type Writer = Logs;
    fn make_writer(&'a self) -> Logs {
        self.clone()
    }
}

pub fn capture_logs() -> (tracing::subscriber::DefaultGuard, Logs) {
    let logs = Logs::default();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(logs.clone())
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .finish();
    (tracing::subscriber::set_default(subscriber), logs)
}
