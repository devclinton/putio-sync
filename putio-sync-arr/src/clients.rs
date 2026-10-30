//! Putio-sync's own download client inside a Radarr/Sonarr (feature 018, R6), and the
//! categories read from it (R4)

use chrono::{DateTime, Utc};
use putio_sync_core::config::{normalize_arr_category, ArrKind};
use serde::Serialize;

use crate::arr_api::{ArrApi, DownloadClient};
use crate::path_check::PathCheck;

/// How often saved applications' categories are re-read (R4)
pub const CATEGORY_REFRESH: std::time::Duration = std::time::Duration::from_secs(15 * 60);

/// The URL base putio-sync serves the Transmission RPC under
pub const URL_BASE: &str = "/transmission/";

/// The name putio-sync gives its client, and the prefix it recognises
pub const CLIENT_NAME: &str = "putio-sync";

fn normalize_url_base(base: &str) -> String {
    let trimmed = base.trim().trim_matches('/');
    if trimmed.is_empty() {
        "/".into()
    } else {
        format!("/{}/", trimmed.to_ascii_lowercase())
    }
}

/// True when `client` is a Transmission client that points at this putio-sync: the URL base
/// and port match, and either the host is one of ours or the name says so (R6)
pub fn points_at_putio_sync(
    client: &DownloadClient,
    port: u16,
    candidate_hosts: &[String],
) -> bool {
    if client.implementation() != "Transmission" {
        return false;
    }
    if normalize_url_base(client.field_str("urlBase")) != URL_BASE {
        return false;
    }
    let client_port = client.get_field("port").and_then(|v| {
        v.as_u64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
    });
    if client_port != Some(u64::from(port)) {
        return false;
    }
    let host = client.field_str("host");
    candidate_hosts.iter().any(|h| h.eq_ignore_ascii_case(host))
        || client.name().to_ascii_lowercase().starts_with(CLIENT_NAME)
}

/// The field holding the client's category in each kind
pub fn category_field(kind: ArrKind) -> &'static str {
    match kind {
        ArrKind::Radarr => "movieCategory",
        ArrKind::Sonarr => "tvCategory",
    }
}

/// What an *arr has for putio-sync (data-model "ClientInfo")
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ClientInfo {
    /// The categories of the clients that point at putio-sync, normalised
    pub categories: Vec<String>,
    /// At least one client points at putio-sync
    pub client_present: bool,
    /// The last successful read
    pub read_at: Option<DateTime<Utc>>,
    /// The kind of the last failed read; the values above are kept
    pub last_error: Option<&'static str>,
    /// The host of the first client that points at putio-sync
    pub host: Option<String>,
    /// The last path check (R10); a clients read keeps it
    pub path_check: Option<PathCheck>,
}

/// The categories of `clients` that point at putio-sync, and whether any does
pub fn categories_of(
    clients: &[DownloadClient],
    kind: ArrKind,
    port: u16,
    candidate_hosts: &[String],
) -> (Vec<String>, bool) {
    let mut categories = Vec::new();
    let mut present = false;
    for client in clients
        .iter()
        .filter(|c| points_at_putio_sync(c, port, candidate_hosts))
    {
        present = true;
        if let Some(c) = normalize_arr_category(client.field_str(category_field(kind))) {
            if !c.is_empty() && !categories.contains(&c) {
                categories.push(c);
            }
        }
    }
    (categories, present)
}

/// Read the *arr's clients into a new [`ClientInfo`]. A failed read keeps `previous`'s
/// values and records the error.
pub async fn read_clients(
    api: &ArrApi,
    port: u16,
    candidate_hosts: &[String],
    previous: Option<ClientInfo>,
) -> ClientInfo {
    match api.download_clients().await {
        Ok(clients) => {
            let (categories, client_present) =
                categories_of(&clients, api.kind(), port, candidate_hosts);
            let host = clients
                .iter()
                .find(|c| points_at_putio_sync(c, port, candidate_hosts))
                .map(|c| c.field_str("host").to_string());
            ClientInfo {
                categories,
                client_present,
                read_at: Some(Utc::now()),
                last_error: None,
                host,
                path_check: previous.and_then(|p| p.path_check),
            }
        }
        Err(e) => ClientInfo {
            last_error: Some(e.kind()),
            ..previous.unwrap_or_default()
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn client(
        implementation: &str,
        name: &str,
        host: &str,
        port: u64,
        base: &str,
    ) -> DownloadClient {
        DownloadClient(json!({
            "name": name, "implementation": implementation,
            "fields": [
                {"name": "host", "value": host},
                {"name": "port", "value": port},
                {"name": "urlBase", "value": base}
            ]
        }))
    }

    #[test]
    fn matches_by_host_or_name() {
        let hosts = vec!["putio-sync".to_string(), "192.168.1.20".to_string()];
        assert!(points_at_putio_sync(
            &client(
                "Transmission",
                "Put.io",
                "192.168.1.20",
                15432,
                "/transmission/"
            ),
            15432,
            &hosts
        ));
        assert!(points_at_putio_sync(
            &client(
                "Transmission",
                "putio-sync (2)",
                "nas.local",
                15432,
                "transmission"
            ),
            15432,
            &hosts
        ));
        // A real Transmission elsewhere
        assert!(!points_at_putio_sync(
            &client(
                "Transmission",
                "Transmission",
                "seedbox",
                15432,
                "/transmission/"
            ),
            15432,
            &hosts
        ));
    }

    #[test]
    fn needs_transmission_base_and_port() {
        let hosts = vec!["putio-sync".to_string()];
        assert!(!points_at_putio_sync(
            &client(
                "TorrentBlackhole",
                "putio-sync",
                "putio-sync",
                15432,
                "/transmission/"
            ),
            15432,
            &hosts
        ));
        assert!(!points_at_putio_sync(
            &client(
                "Transmission",
                "putio-sync",
                "putio-sync",
                9091,
                "/transmission/"
            ),
            15432,
            &hosts
        ));
        assert!(!points_at_putio_sync(
            &client("Transmission", "putio-sync", "putio-sync", 15432, "/rpc/"),
            15432,
            &hosts
        ));
    }

    #[test]
    fn categories_come_from_matching_clients_only() {
        let hosts = vec!["putio-sync".to_string()];
        let with_category = |c: DownloadClient, cat: &str| {
            let mut c = c;
            c.set_field("movieCategory", json!(cat));
            c
        };
        let clients = vec![
            with_category(
                client(
                    "Transmission",
                    "putio-sync",
                    "putio-sync",
                    15432,
                    "/transmission/",
                ),
                "/movies/",
            ),
            with_category(
                client(
                    "Transmission",
                    "putio-sync (2)",
                    "nas",
                    15432,
                    "/transmission/",
                ),
                "movies-4k",
            ),
            with_category(
                client(
                    "Transmission",
                    "putio-sync 3",
                    "nas",
                    15432,
                    "/transmission/",
                ),
                "movies",
            ),
            with_category(
                client("Transmission", "Other", "other", 9091, "/transmission/"),
                "elsewhere",
            ),
            with_category(
                client("QBittorrent", "qbit", "putio-sync", 15432, "/transmission/"),
                "qb",
            ),
        ];
        assert_eq!(
            categories_of(&clients, ArrKind::Radarr, 15432, &hosts),
            (vec!["movies".to_string(), "movies-4k".to_string()], true)
        );
        assert_eq!(
            categories_of(&clients[3..], ArrKind::Radarr, 15432, &hosts),
            (vec![], false)
        );
    }

    #[tokio::test]
    async fn a_failed_read_keeps_the_last_value() {
        let mut server = mockito::Server::new_async().await;
        let ok = server
            .mock("GET", "/api/v3/downloadclient")
            .with_body(
                json!([{"name": "putio-sync", "implementation": "Transmission", "fields": [
                    {"name": "host", "value": "putio-sync"}, {"name": "port", "value": 15432},
                    {"name": "urlBase", "value": "/transmission/"},
                    {"name": "tvCategory", "value": "tv"}]}])
                .to_string(),
            )
            .create_async()
            .await;
        let api = ArrApi::new(
            ArrKind::Sonarr,
            &server.url(),
            "key",
            reqwest::Client::new(),
        );
        let hosts = vec!["putio-sync".to_string()];
        let first = read_clients(&api, 15432, &hosts, None).await;
        assert_eq!(first.categories, vec!["tv"]);
        assert!(first.client_present && first.read_at.is_some());
        assert_eq!(first.last_error, None);
        assert_eq!(first.host.as_deref(), Some("putio-sync"));

        // A read keeps the last path check, which only a test or setup runs
        let checked = ClientInfo {
            path_check: Some(PathCheck {
                status: crate::path_check::PathStatus::Visible,
                folder: "/downloads".into(),
                arr_path: "/downloads".into(),
                mappings: vec![],
                detail: None,
            }),
            ..first.clone()
        };
        let reread = read_clients(&api, 15432, &hosts, Some(checked.clone())).await;
        assert_eq!(reread.path_check, checked.path_check);

        ok.remove_async().await;
        server
            .mock("GET", "/api/v3/downloadclient")
            .with_status(401)
            .create_async()
            .await;
        let second = read_clients(&api, 15432, &hosts, Some(first.clone())).await;
        assert_eq!(second.categories, first.categories);
        assert_eq!(second.read_at, first.read_at);
        assert_eq!(second.last_error, Some("unauthorized"));
    }
}
