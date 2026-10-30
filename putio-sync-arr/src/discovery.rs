//! Finding Radarr and Sonarr without a key (feature 018, R9): a bounded list of likely
//! addresses, each probed with `/ping` and the page title. No network sweep, and never
//! `initialize.json`, which carries the API key.

use std::time::Duration;

use putio_sync_core::config::ArrKind;
use serde::Serialize;
use tokio::sync::Mutex;
use tokio::task::JoinSet;

/// Radarr's and Sonarr's default ports
pub const PORTS: [u16; 2] = [7878, 8989];

/// How a candidate address was found, best first
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FoundBy {
    /// A saved application
    Saved,
    /// The `radarr` / `sonarr` container names
    Container,
    /// The Docker host or this machine
    Host,
    /// The host the browser used to reach putio-sync
    Browser,
}

/// One address to probe
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Base URL, without a trailing slash
    pub url: String,
    /// Its tier
    pub found_by: FoundBy,
}

/// One *arr that answered
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiscoveredInstance {
    /// `radarr` or `sonarr`
    pub kind: ArrKind,
    /// Its base URL
    pub url: String,
    /// Its tier
    pub found_by: FoundBy,
    /// Unknown until a key is given
    pub version: Option<String>,
}

/// The result of one discovery
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Discovery {
    /// What answered, best tier first
    pub instances: Vec<DiscoveredInstance>,
    /// Every URL probed
    pub tried: Vec<String>,
}

/// Another discovery is running
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Busy;

/// Probe and overall time limits
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Per request
    pub probe: Duration,
    /// For the whole discovery (SC-002)
    pub overall: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            probe: Duration::from_secs(3),
            overall: Duration::from_secs(10),
        }
    }
}

/// The addresses to probe, in tier order, without duplicates
pub fn candidates(
    saved: &[String],
    gateway: Option<&str>,
    browser_host: Option<&str>,
) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    let mut push = |url: String, found_by| {
        let url = url.trim_end_matches('/').to_string();
        if !out.iter().any(|c| c.url.eq_ignore_ascii_case(&url)) {
            out.push(Candidate { url, found_by });
        }
    };
    for url in saved {
        push(url.clone(), FoundBy::Saved);
    }
    let on_ports = |host: &str| PORTS.map(|p| format!("http://{}:{}", host, p));
    for host in ["radarr", "sonarr"] {
        for url in on_ports(host) {
            push(url, FoundBy::Container);
        }
    }
    for host in gateway.into_iter().chain(["localhost"]) {
        for url in on_ports(host) {
            push(url, FoundBy::Host);
        }
    }
    if let Some(host) = browser_host.filter(|h| !h.is_empty()) {
        for url in on_ports(host) {
            push(url, FoundBy::Browser);
        }
    }
    out
}

/// `Radarr` or `Sonarr` from the page's `<title>`
fn kind_from_title(html: &str) -> Option<ArrKind> {
    let lower = html.to_ascii_lowercase();
    let start = lower.find("<title>")? + "<title>".len();
    let end = start + lower[start..].find("</title>")?;
    let title = &lower[start..end];
    if title.contains("radarr") {
        Some(ArrKind::Radarr)
    } else if title.contains("sonarr") {
        Some(ArrKind::Sonarr)
    } else {
        None
    }
}

/// `/ping` answers OK, then the title names the app. Neither needs a key.
async fn probe(http: &reqwest::Client, url: &str, timeout: Duration) -> Option<ArrKind> {
    let ping = http
        .get(format!("{}/ping", url))
        .timeout(timeout)
        .send()
        .await
        .ok()?;
    if !ping.status().is_success() || !ping.text().await.ok()?.contains("OK") {
        return None;
    }
    let page = http
        .get(format!("{}/", url))
        .timeout(timeout)
        .send()
        .await
        .ok()?;
    if !page.status().is_success() {
        return None;
    }
    kind_from_title(&page.text().await.ok()?)
}

fn port_of(url: &str) -> Option<u16> {
    reqwest::Url::parse(url).ok()?.port_or_known_default()
}

/// Probe every candidate at once, for at most `limits.overall`. One discovery runs at a
/// time: a second caller gets [`Busy`].
pub async fn discover(
    lock: &Mutex<()>,
    candidates: Vec<Candidate>,
    http: reqwest::Client,
    limits: Limits,
) -> Result<Discovery, Busy> {
    let _guard = lock.try_lock().map_err(|_| Busy)?;
    let tried: Vec<String> = candidates.iter().map(|c| c.url.clone()).collect();

    let mut set = JoinSet::new();
    for (order, candidate) in candidates.into_iter().enumerate() {
        let http = http.clone();
        set.spawn(async move {
            let kind = probe(&http, &candidate.url, limits.probe).await;
            (order, candidate, kind)
        });
    }
    let mut hits = Vec::new();
    let collect = async {
        while let Some(joined) = set.join_next().await {
            if let Ok((order, candidate, Some(kind))) = joined {
                hits.push((order, candidate, kind));
            }
        }
    };
    // On the deadline, what answered so far is kept and the rest is dropped
    let _ = tokio::time::timeout(limits.overall, collect).await;
    set.abort_all();

    // Best tier first; then the same app on the same port found again lower down is
    // the same instance (decision 2 in plan.md). Saved applications are always kept.
    hits.sort_by_key(|(order, c, _)| (c.found_by, *order));
    let mut instances: Vec<DiscoveredInstance> = Vec::new();
    for (_, candidate, kind) in hits {
        let port = port_of(&candidate.url);
        let seen = candidate.found_by != FoundBy::Saved
            && instances
                .iter()
                .any(|i| i.kind == kind && port_of(&i.url) == port);
        if !seen {
            instances.push(DiscoveredInstance {
                kind,
                url: candidate.url,
                found_by: candidate.found_by,
                version: None,
            });
        }
    }
    Ok(Discovery { instances, tried })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    async fn arr(title: &str) -> mockito::ServerGuard {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/ping")
            .with_body(r#"{"status": "OK"}"#)
            .create_async()
            .await;
        server
            .mock("GET", "/")
            .with_body(format!(
                "<html><head><title>{}</title></head></html>",
                title
            ))
            .create_async()
            .await;
        server
            .mock("GET", "/initialize.json")
            .expect(0)
            .create_async()
            .await;
        server
    }

    fn candidate(url: String, found_by: FoundBy) -> Candidate {
        Candidate { url, found_by }
    }

    #[test]
    fn candidates_are_tiered_and_unique() {
        let list = candidates(
            &["http://radarr:7878/".into()],
            Some("172.17.0.1"),
            Some("nas"),
        );
        let urls: Vec<_> = list.iter().map(|c| (c.url.as_str(), c.found_by)).collect();
        assert_eq!(
            urls,
            vec![
                ("http://radarr:7878", FoundBy::Saved),
                ("http://radarr:8989", FoundBy::Container),
                ("http://sonarr:7878", FoundBy::Container),
                ("http://sonarr:8989", FoundBy::Container),
                ("http://172.17.0.1:7878", FoundBy::Host),
                ("http://172.17.0.1:8989", FoundBy::Host),
                ("http://localhost:7878", FoundBy::Host),
                ("http://localhost:8989", FoundBy::Host),
                ("http://nas:7878", FoundBy::Browser),
                ("http://nas:8989", FoundBy::Browser),
            ]
        );
    }

    #[tokio::test]
    async fn finds_arrs_and_ignores_other_pages() {
        let radarr = arr("Radarr").await;
        let sonarr = arr("Sonarr").await;
        let mut other = mockito::Server::new_async().await;
        other
            .mock("GET", "/ping")
            .with_status(404)
            .create_async()
            .await;
        let lock = Mutex::new(());
        let found = discover(
            &lock,
            vec![
                candidate(radarr.url(), FoundBy::Container),
                candidate(other.url(), FoundBy::Container),
                candidate(sonarr.url(), FoundBy::Host),
            ],
            reqwest::Client::new(),
            Limits::default(),
        )
        .await
        .unwrap();
        let kinds: Vec<_> = found.instances.iter().map(|i| i.kind).collect();
        assert_eq!(kinds, vec![ArrKind::Radarr, ArrKind::Sonarr]);
        assert_eq!(found.tried.len(), 3);
        assert_eq!(found.instances[0].version, None);
    }

    #[tokio::test]
    async fn a_lower_tier_hit_of_the_same_app_and_port_is_dropped() {
        let radarr = arr("Radarr").await;
        let via_ip = radarr.url();
        let via_name = via_ip.replace("127.0.0.1", "localhost");
        let lock = Mutex::new(());
        let found = discover(
            &lock,
            vec![
                candidate(via_name, FoundBy::Host),
                candidate(via_ip.clone(), FoundBy::Container),
            ],
            reqwest::Client::new(),
            Limits::default(),
        )
        .await
        .unwrap();
        assert_eq!(found.instances.len(), 1);
        assert_eq!(found.instances[0].url, via_ip);
        assert_eq!(found.instances[0].found_by, FoundBy::Container);
    }

    #[tokio::test]
    async fn a_silent_address_hits_the_deadline() {
        // Accepts connections and never answers
        let silent = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let silent_url = format!("http://{}", silent.local_addr().unwrap());
        let radarr = arr("Radarr").await;
        let lock = Mutex::new(());
        let started = std::time::Instant::now();
        let found = discover(
            &lock,
            vec![
                candidate(silent_url, FoundBy::Host),
                candidate(radarr.url(), FoundBy::Host),
            ],
            reqwest::Client::new(),
            Limits {
                probe: Duration::from_secs(30),
                overall: Duration::from_millis(500),
            },
        )
        .await
        .unwrap();
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(found.instances.len(), 1);

        // The per-probe limit alone also ends it
        let started = std::time::Instant::now();
        let found = discover(
            &lock,
            vec![candidate(
                format!("http://{}", silent.local_addr().unwrap()),
                FoundBy::Host,
            )],
            reqwest::Client::new(),
            Limits {
                probe: Duration::from_millis(300),
                overall: Duration::from_secs(30),
            },
        )
        .await
        .unwrap();
        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(found.instances.is_empty());
    }

    #[tokio::test]
    async fn a_second_run_is_busy() {
        let lock = Arc::new(Mutex::new(()));
        let _held = lock.lock().await;
        let result = discover(&lock, vec![], reqwest::Client::new(), Limits::default()).await;
        assert_eq!(result, Err(Busy));
    }
}
