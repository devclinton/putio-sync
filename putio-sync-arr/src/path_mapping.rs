//! Work out the Remote Path Mapping the *arr needs to see putio-sync's download folder, and
//! make it (feature 049, R1–R4).
//!
//! putio-sync makes an empty marker folder with a random name in its download folder, then
//! walks the *arr's filesystem browser for a folder that holds it. Where it's found is the
//! download folder as the *arr sees it.

use std::collections::{HashSet, VecDeque};
use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};
use std::time::Duration;

use putio_sync_telemetry::arr as metrics;
use putio_sync_telemetry::labels::ArrMappingResult;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::arr_api::{ArrApi, ArrApiError, RemotePathMapping};
use crate::path_check::{self, is_sep, with_sep, PathCheck, PathStatus};

/// The marker's name starts with this
pub const MARKER_PREFIX: &str = "putio-sync-check-";

/// An empty folder in the download folder, removed when dropped
pub struct Marker {
    path: PathBuf,
    name: String,
}

impl Marker {
    /// Make `<folder>/putio-sync-check-<12 hex>`
    pub fn create(folder: &Path) -> std::io::Result<Self> {
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default(),
        );
        let name = format!(
            "{}{:012x}",
            MARKER_PREFIX,
            hasher.finish() & 0xffff_ffff_ffff
        );
        let path = folder.join(&name);
        std::fs::create_dir(&path)?;
        Ok(Self { path, name })
    }

    /// The folder's name
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl Drop for Marker {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_dir(&self.path) {
            warn!(error = %e, "couldn't remove the path check marker");
        }
    }
}

/// How far a search may go (R3)
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Folders listed at most
    pub listings: usize,
    /// Path depth at most
    pub depth: usize,
    /// Time in all
    pub time: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            listings: 60,
            depth: 6,
            time: Duration::from_secs(20),
        }
    }
}

/// Mount names listed first
const BOOST: &[&str] = &[
    "data",
    "downloads",
    "media",
    "mnt",
    "storage",
    "share",
    "torrents",
    "volume1",
    "user",
];

/// System folders never listed
const SKIP: &[&str] = &[
    "proc", "sys", "dev", "etc", "usr", "bin", "sbin", "run", "boot", "var", "tmp", "root",
    "config", "app", "opt", "snap",
];

fn skipped(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n.starts_with("lib") || SKIP.contains(&n.as_str()) || n.starts_with('.')
}

fn depth(path: &str) -> usize {
    path.split(is_sep).filter(|p| !p.is_empty()).count()
}

fn trimmed(path: &str) -> &str {
    let t = path.trim_end_matches(is_sep);
    if t.is_empty() || t.ends_with(':') {
        path
    } else {
        t
    }
}

/// Where to start: the ancestors of the root folders, nearest first, then the top
fn seeds(root_folders: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for root in root_folders {
        let mut at = trimmed(root).to_string();
        while let Some((parent, _)) = path_check::split(&at) {
            if depth(&parent) > 0 {
                out.push(trimmed(&parent).to_string());
            }
            at = parent;
        }
        // `/` or `D:\`
        out.push(at);
    }
    if out.is_empty() {
        out.push("/".to_string());
    }
    // Tops last, so every root's nearer ancestors come first
    let (tops, mut rest): (Vec<_>, Vec<_>) = out.into_iter().partition(|p| depth(p) == 0);
    rest.extend(tops);
    let mut seen = HashSet::new();
    rest.retain(|p| seen.insert(p.clone()));
    rest
}

/// What a search found
#[derive(Debug, Clone, Default)]
pub struct Found {
    /// The *arr's path of the folder that holds the marker
    pub path: Option<String>,
    /// The folders listed, in order
    pub looked: Vec<String>,
}

/// Walk the *arr's filesystem for the folder holding `marker` (R3). `folder_name` is the
/// download folder's own name, boosted like the mount names.
pub async fn search(
    api: &ArrApi,
    marker: &str,
    folder_name: &str,
    root_folders: &[String],
    limits: Limits,
) -> Found {
    let started = tokio::time::Instant::now();
    let mut found = Found::default();
    let mut queue: VecDeque<String> = seeds(root_folders).into();
    let mut seen = HashSet::new();
    let boosted = |name: &str| {
        name.eq_ignore_ascii_case(folder_name) || BOOST.iter().any(|b| name.eq_ignore_ascii_case(b))
    };
    while let Some(path) = queue.pop_front() {
        if found.looked.len() >= limits.listings || started.elapsed() >= limits.time {
            break;
        }
        if !seen.insert(path.clone()) {
            continue;
        }
        let Ok(children) = api.folders(&path).await else {
            found.looked.push(path);
            continue;
        };
        found.looked.push(path.clone());
        if children.iter().any(|c| c.name == marker) {
            found.path = Some(trimmed(&path).to_string());
            return found;
        }
        let mut front = Vec::new();
        for child in children {
            if child.name.is_empty() || skipped(&child.name) {
                continue;
            }
            let child_path = if child.path.is_empty() {
                format!("{}{}", with_sep(&path), child.name)
            } else {
                trimmed(&child.path).to_string()
            };
            if depth(&child_path) > limits.depth || seen.contains(&child_path) {
                continue;
            }
            if boosted(&child.name) {
                front.push(child_path);
            } else {
                queue.push_back(child_path);
            }
        }
        for p in front.into_iter().rev() {
            queue.push_front(p);
        }
    }
    found
}

/// What the folder step proposes (R4)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProposalAction {
    /// The *arr already sees the folder
    None,
    /// Add a mapping
    Create,
    /// Change the mapping `id`
    Update,
    /// Delete the mapping `id`, which hides a folder the *arr sees as is
    Remove,
    /// The folder wasn't found
    NotFound,
}

/// data-model "Proposal"
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Proposal {
    /// What to do
    pub action: ProposalAction,
    /// The host in the *arr's putio-sync client
    pub host: String,
    /// putio-sync's download folder, with a trailing separator
    pub remote_path: String,
    /// Where the *arr sees it, with a trailing separator; empty for remove and none
    pub local_path: String,
    /// The existing mapping for update and remove
    pub id: Option<i64>,
}

/// data-model "FolderAnalysis"
#[derive(Debug, Clone, Serialize)]
pub struct FolderAnalysis {
    /// The check before any change
    pub path_check: PathCheck,
    /// What to do
    pub proposal: Proposal,
    /// The *arr's path of the folder
    pub found: Option<String>,
    /// The folders listed
    pub looked: Vec<String>,
}

fn same_path(a: &str, b: &str) -> bool {
    trimmed(a) == trimmed(b)
}

/// The proposal for a folder found (or not) at `found` (R4)
pub fn propose(
    folder: &str,
    client_host: &str,
    mappings: &[RemotePathMapping],
    found: Option<&str>,
) -> Proposal {
    let mut p = Proposal {
        action: ProposalAction::None,
        host: client_host.to_string(),
        remote_path: with_sep(folder),
        local_path: String::new(),
        id: None,
    };
    let covering = path_check::covering(folder, client_host, mappings);
    match (found, covering) {
        (None, _) => p.action = ProposalAction::NotFound,
        (Some(at), Some(m)) if same_path(at, folder) => {
            p.action = ProposalAction::Remove;
            p.host = m.host.clone();
            p.remote_path = m.remote_path.clone();
            p.id = m.id;
        }
        (Some(at), _) if same_path(at, folder) => {}
        (Some(at), Some(m)) => {
            p.action = ProposalAction::Update;
            p.host = m.host.clone();
            p.local_path = with_sep(at);
            p.id = m.id;
        }
        (Some(at), None) => {
            p.action = ProposalAction::Create;
            p.local_path = with_sep(at);
        }
    }
    p
}

/// Check the folder and work out what mapping it needs. `marker` is the name of a marker
/// already made in `folder`.
pub async fn analyse(
    api: &ArrApi,
    folder: &str,
    client_host: &str,
    marker: &str,
    limits: Limits,
) -> Result<FolderAnalysis, ArrApiError> {
    let mappings = api.remote_path_mappings().await?;
    let mut check = path_check::check_with(api, folder, client_host, mappings).await;
    // R2: the folder the *arr reads holds the marker, so it's truly the same folder
    if let Ok(children) = api.folders(&check.arr_path).await {
        if children.iter().any(|c| c.name == marker) {
            check.status = PathStatus::Visible;
            let proposal = propose(folder, client_host, &[], Some(folder));
            return Ok(FolderAnalysis {
                found: Some(check.arr_path.clone()),
                looked: vec![check.arr_path.clone()],
                path_check: check,
                proposal,
            });
        }
    }
    if check.status == PathStatus::Visible {
        // A folder of that name, but not putio-sync's
        check.status = PathStatus::NotVisible;
    }
    let roots = api.root_folders().await.unwrap_or_default();
    let folder_name = path_check::split(folder)
        .map(|(_, n)| n)
        .unwrap_or_default();
    let found = search(api, marker, &folder_name, &roots, limits).await;
    let proposal = propose(folder, client_host, &check.mappings, found.path.as_deref());
    Ok(FolderAnalysis {
        path_check: check,
        proposal,
        found: found.path,
        looked: found.looked,
    })
}

/// Make the change a proposal describes
pub async fn apply(api: &ArrApi, proposal: &Proposal) -> Result<(), ArrApiError> {
    let mapping = RemotePathMapping {
        id: proposal.id,
        host: proposal.host.clone(),
        remote_path: proposal.remote_path.clone(),
        local_path: proposal.local_path.clone(),
    };
    let (result, label) = match (proposal.action, proposal.id) {
        (ProposalAction::Create, _) => (
            api.create_remote_path_mapping(&mapping).await.map(|_| ()),
            ArrMappingResult::Created,
        ),
        (ProposalAction::Update, Some(id)) => (
            api.update_remote_path_mapping(id, &mapping).await,
            ArrMappingResult::Updated,
        ),
        (ProposalAction::Remove, Some(id)) => (
            api.delete_remote_path_mapping(id).await,
            ArrMappingResult::Removed,
        ),
        _ => return Ok(()),
    };
    match &result {
        Ok(()) => {
            info!(
                kind = api.kind().as_str(),
                result = label.as_str(),
                remote_path = %proposal.remote_path,
                local_path = %proposal.local_path,
                "remote path mapping changed"
            );
            metrics::path_mapping(label);
        }
        Err(e) => {
            warn!(
                kind = api.kind().as_str(),
                reason = e.kind(),
                "remote path mapping failed"
            );
            metrics::path_mapping(ArrMappingResult::Failed);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Matcher;
    use putio_sync_core::config::ArrKind;
    use serde_json::json;

    fn api(server: &mockito::Server) -> ArrApi {
        ArrApi::new(
            ArrKind::Sonarr,
            &server.url(),
            "key-123",
            reqwest::Client::new(),
        )
    }

    fn mapping(id: i64, remote: &str, local: &str) -> RemotePathMapping {
        RemotePathMapping {
            id: Some(id),
            host: "putio-sync".into(),
            remote_path: remote.into(),
            local_path: local.into(),
        }
    }

    /// A listing of `path` with `names` in it
    async fn listing(server: &mut mockito::Server, path: &str, names: &[&str]) -> mockito::Mock {
        let sep = if path.ends_with('/') { "" } else { "/" };
        let dirs: Vec<_> = names
            .iter()
            .map(|n| json!({"name": n, "path": format!("{}{}{}", path, sep, n)}))
            .collect();
        server
            .mock("GET", "/api/v3/filesystem")
            .match_query(Matcher::UrlEncoded("path".into(), path.into()))
            .with_body(json!({"directories": dirs}).to_string())
            // Always preferred over the catch-all, however often it's hit
            .expect_at_most(usize::MAX)
            .create_async()
            .await
    }

    async fn empty(server: &mut mockito::Server, mappings: &str, roots: &str) {
        server
            .mock("GET", "/api/v3/filesystem")
            .match_query(Matcher::Any)
            .with_body(r#"{"directories": []}"#)
            .create_async()
            .await;
        server
            .mock("GET", "/api/v3/remotepathmapping")
            .with_body(mappings)
            .create_async()
            .await;
        server
            .mock("GET", "/api/v3/rootfolder")
            .with_body(roots)
            .create_async()
            .await;
    }

    #[test]
    fn the_marker_is_removed_when_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let marker = Marker::create(dir.path()).unwrap();
        assert!(marker.name().starts_with(MARKER_PREFIX));
        assert_eq!(marker.name().len(), MARKER_PREFIX.len() + 12);
        assert!(dir.path().join(marker.name()).is_dir());
        let path = dir.path().join(marker.name());
        drop(marker);
        assert!(!path.exists());
    }

    #[test]
    fn seeds_are_root_ancestors_nearest_first_then_the_top() {
        assert_eq!(
            seeds(&["/data/media/tv/".into(), "/tv".into()]),
            vec!["/data/media", "/data", "/"]
        );
        assert_eq!(seeds(&[]), vec!["/"]);
        assert_eq!(seeds(&["D:\\Media\\TV".into()]), vec!["D:\\Media", "D:\\"]);
    }

    #[tokio::test]
    async fn a_docker_layout_is_found_in_a_few_listings() {
        let mut server = mockito::Server::new_async().await;
        listing(&mut server, "/data/media", &["tv", "movies"]).await;
        listing(&mut server, "/data", &["media", "downloads"]).await;
        listing(
            &mut server,
            "/data/downloads",
            &["putio-sync-check-abc", "tv"],
        )
        .await;
        empty(&mut server, "[]", "[]").await;
        let found = search(
            &api(&server),
            "putio-sync-check-abc",
            "downloads",
            &["/data/media/tv".into()],
            Limits::default(),
        )
        .await;
        assert_eq!(found.path.as_deref(), Some("/data/downloads"));
        assert!(found.looked.len() <= 4, "{:?}", found.looked);
    }

    #[tokio::test]
    async fn not_found_reports_where_it_looked_within_the_limits() {
        let mut server = mockito::Server::new_async().await;
        listing(&mut server, "/", &["proc", "data", "etc", "srv"]).await;
        listing(&mut server, "/data", &["a", "b", "c"]).await;
        empty(&mut server, "[]", "[]").await;
        let limits = Limits {
            listings: 3,
            ..Limits::default()
        };
        let found = search(&api(&server), "putio-sync-check-x", "dl", &[], limits).await;
        assert_eq!(found.path, None);
        // `data` is boosted, `proc` and `etc` skipped
        assert_eq!(found.looked, vec!["/", "/data", "/srv"]);
    }

    #[tokio::test]
    async fn windows_paths_keep_their_separator() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/v3/filesystem")
            .match_query(Matcher::UrlEncoded("path".into(), "D:\\".into()))
            .with_body(r#"{"directories": [{"name": "Downloads", "path": "D:\\Downloads\\"}]}"#)
            .create_async()
            .await;
        server
            .mock("GET", "/api/v3/filesystem")
            .match_query(Matcher::UrlEncoded("path".into(), "D:\\Downloads".into()))
            .with_body(r#"{"directories": [{"name": "m", "path": "D:\\Downloads\\m\\"}]}"#)
            .create_async()
            .await;
        empty(&mut server, "[]", "[]").await;
        let found = search(
            &api(&server),
            "m",
            "downloads",
            &["D:\\TV".into()],
            Limits::default(),
        )
        .await;
        assert_eq!(found.path.as_deref(), Some("D:\\Downloads"));
        let p = propose("/downloads", "putio-sync", &[], found.path.as_deref());
        assert_eq!(p.local_path, "D:\\Downloads\\");
        assert_eq!(p.remote_path, "/downloads/");
    }

    #[test]
    fn proposals_follow_r4() {
        let wrong = [mapping(7, "/downloads/", "/wrong/")];
        let p = propose("/downloads", "putio-sync", &wrong, Some("/data/downloads"));
        assert_eq!(
            (p.action, p.id, p.local_path.as_str()),
            (ProposalAction::Update, Some(7), "/data/downloads/")
        );
        let p = propose("/downloads", "putio-sync", &wrong, Some("/downloads"));
        assert_eq!((p.action, p.id), (ProposalAction::Remove, Some(7)));
        let p = propose("/downloads", "putio-sync", &[], Some("/downloads/"));
        assert_eq!(p.action, ProposalAction::None);
        let p = propose("/downloads", "putio-sync", &[], None);
        assert_eq!(p.action, ProposalAction::NotFound);
        // Another host's mapping isn't touched
        let other = [RemotePathMapping {
            host: "qbit".into(),
            ..mapping(3, "/downloads/", "/x/")
        }];
        let p = propose("/downloads", "putio-sync", &other, Some("/data/downloads"));
        assert_eq!((p.action, p.id), (ProposalAction::Create, None));
    }

    #[tokio::test]
    async fn a_visible_folder_holding_the_marker_needs_nothing() {
        let mut server = mockito::Server::new_async().await;
        listing(&mut server, "/", &["downloads"]).await;
        listing(&mut server, "/downloads", &["putio-sync-check-m"]).await;
        empty(&mut server, "[]", "[]").await;
        let a = analyse(
            &api(&server),
            "/downloads",
            "putio-sync",
            "putio-sync-check-m",
            Limits::default(),
        )
        .await
        .unwrap();
        assert_eq!(a.path_check.status, PathStatus::Visible);
        assert_eq!(a.proposal.action, ProposalAction::None);
    }

    #[tokio::test]
    async fn a_same_named_folder_without_the_marker_is_not_visible() {
        let mut server = mockito::Server::new_async().await;
        listing(&mut server, "/", &["downloads", "data"]).await;
        listing(&mut server, "/downloads", &["other"]).await;
        listing(&mut server, "/data", &["dl"]).await;
        listing(&mut server, "/data/dl", &["putio-sync-check-m"]).await;
        empty(&mut server, "[]", "[]").await;
        let a = analyse(
            &api(&server),
            "/downloads",
            "putio-sync",
            "putio-sync-check-m",
            Limits::default(),
        )
        .await
        .unwrap();
        assert_eq!(a.path_check.status, PathStatus::NotVisible);
        assert_eq!(a.found.as_deref(), Some("/data/dl"), "{:?}", a.looked);
        assert_eq!(a.proposal.action, ProposalAction::Create);
        assert_eq!(a.proposal.local_path, "/data/dl/");
    }

    #[tokio::test]
    async fn apply_creates_the_mapping() {
        let mut server = mockito::Server::new_async().await;
        let create = server
            .mock("POST", "/api/v3/remotepathmapping")
            .match_body(Matcher::Json(json!({"host": "putio-sync",
                "remotePath": "/downloads/", "localPath": "/data/dl/"})))
            .with_status(201)
            .with_body(
                r#"{"id": 9, "host": "putio-sync", "remotePath": "/downloads/",
                "localPath": "/data/dl/"}"#,
            )
            .create_async()
            .await;
        let p = propose("/downloads", "putio-sync", &[], Some("/data/dl"));
        apply(&api(&server), &p).await.unwrap();
        create.assert_async().await;
        // None and not_found write nothing
        let none = propose("/downloads", "putio-sync", &[], None);
        apply(&api(&server), &none).await.unwrap();
    }
}
