//! Can the *arr see putio-sync's download folder? (feature 018, R10)
//!
//! The folder is rewritten by the *arr's Remote Path Mappings for putio-sync's client host,
//! then its parent is listed and the folder's name looked for. Listing the folder itself
//! would give an ambiguous empty answer when it's missing.

use serde::Serialize;

use crate::arr_api::{ArrApi, RemotePathMapping};

/// The result of one check
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PathStatus {
    /// The *arr lists the folder
    Visible,
    /// The parent was listed, and the folder isn't in it
    NotVisible,
    /// The *arr couldn't be asked
    Unknown,
}

/// data-model "PathCheck"
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PathCheck {
    /// The result
    pub status: PathStatus,
    /// putio-sync's download folder, as it reports it
    pub folder: String,
    /// The folder the *arr reads, after the mappings
    pub arr_path: String,
    /// The *arr's Remote Path Mappings
    pub mappings: Vec<RemotePathMapping>,
    /// For `unknown`, what went wrong
    pub detail: Option<String>,
}

fn is_sep(c: char) -> bool {
    c == '/' || c == '\\'
}

/// `path` with one trailing separator, in the style it already uses
fn with_sep(path: &str) -> String {
    if path.ends_with(is_sep) {
        path.to_string()
    } else if path.contains('\\') && !path.contains('/') {
        format!("{}\\", path)
    } else {
        format!("{}/", path)
    }
}

/// The folder as the *arr reads it: the first mapping for `client_host` whose remote path
/// is a prefix rewrites it
pub fn mapped(folder: &str, client_host: &str, mappings: &[RemotePathMapping]) -> String {
    let folder_sep = with_sep(folder);
    for m in mappings {
        if !m.host.eq_ignore_ascii_case(client_host) || m.remote_path.is_empty() {
            continue;
        }
        let remote = with_sep(&m.remote_path);
        if let Some(rest) = folder_sep.strip_prefix(&remote) {
            let out = format!("{}{}", with_sep(&m.local_path), rest);
            let trimmed = out.trim_end_matches(is_sep);
            return if trimmed.is_empty() {
                out
            } else {
                trimmed.to_string()
            };
        }
    }
    folder.to_string()
}

/// `(parent, name)` of a path; `None` for a root
fn split(path: &str) -> Option<(String, String)> {
    let path = path.trim_end_matches(is_sep);
    let at = path.rfind(is_sep)?;
    let name = &path[at + 1..];
    if name.is_empty() {
        return None;
    }
    // Keep the separator for a root parent: `/` or `D:\`
    let parent = &path[..=at];
    let parent = if parent.trim_end_matches(is_sep).is_empty() || parent.ends_with(":\\") {
        parent.to_string()
    } else {
        parent.trim_end_matches(is_sep).to_string()
    };
    Some((parent, name.to_string()))
}

/// Check that the *arr behind `api` can see `folder`, which putio-sync's client at
/// `client_host` reports
pub async fn check(api: &ArrApi, folder: &str, client_host: &str) -> PathCheck {
    let mut result = PathCheck {
        status: PathStatus::Unknown,
        folder: folder.to_string(),
        arr_path: folder.to_string(),
        mappings: Vec::new(),
        detail: None,
    };
    match api.remote_path_mappings().await {
        Ok(mappings) => result.mappings = mappings,
        Err(e) => {
            result.detail = Some(e.to_string());
            return result;
        }
    }
    result.arr_path = mapped(folder, client_host, &result.mappings);
    let Some((parent, name)) = split(&result.arr_path) else {
        result.detail = Some("the folder is a root, which can't be checked".into());
        return result;
    };
    match api.filesystem(&parent).await {
        Ok(names) => {
            result.status = if names.iter().any(|n| n == &name) {
                PathStatus::Visible
            } else {
                PathStatus::NotVisible
            };
        }
        Err(e) => result.detail = Some(e.to_string()),
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Matcher;
    use putio_sync_core::config::ArrKind;
    use serde_json::json;

    fn mapping(host: &str, remote: &str, local: &str) -> RemotePathMapping {
        RemotePathMapping {
            host: host.into(),
            remote_path: remote.into(),
            local_path: local.into(),
        }
    }

    #[test]
    fn mappings_apply_to_the_client_host_and_a_prefix_only() {
        let maps = vec![
            mapping("other", "/downloads/", "/elsewhere/"),
            mapping("putio-sync", "/down/", "/wrong/"),
            mapping("PUTIO-SYNC", "/downloads/", "/data/downloads/"),
        ];
        assert_eq!(mapped("/downloads", "putio-sync", &maps), "/data/downloads");
        assert_eq!(
            mapped("/downloads/tv", "putio-sync", &maps),
            "/data/downloads/tv"
        );
        assert_eq!(mapped("/downloads", "nas", &maps), "/downloads");
        assert_eq!(
            mapped(
                "/downloads",
                "putio-sync",
                &[mapping("putio-sync", "/downloads", "D:\\Downloads\\")]
            ),
            "D:\\Downloads"
        );
    }

    #[test]
    fn parents_and_names() {
        assert_eq!(split("/downloads"), Some(("/".into(), "downloads".into())));
        assert_eq!(
            split("/data/downloads/"),
            Some(("/data".into(), "downloads".into()))
        );
        assert_eq!(
            split("D:\\Downloads"),
            Some(("D:\\".into(), "Downloads".into()))
        );
        assert_eq!(split("/"), None);
    }

    async fn arr(mappings: serde_json::Value, parent: &str, dirs: &[&str]) -> mockito::ServerGuard {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/v3/remotepathmapping")
            .with_body(mappings.to_string())
            .create_async()
            .await;
        let dirs: Vec<_> = dirs
            .iter()
            .map(|d| json!({"type": "folder", "name": d}))
            .collect();
        server
            .mock("GET", "/api/v3/filesystem")
            .match_query(Matcher::UrlEncoded("path".into(), parent.into()))
            .with_body(json!({"directories": dirs, "files": []}).to_string())
            .create_async()
            .await;
        server
    }

    fn api(server: &mockito::Server) -> ArrApi {
        ArrApi::new(
            ArrKind::Sonarr,
            &server.url(),
            "key",
            reqwest::Client::new(),
        )
    }

    #[tokio::test]
    async fn visible() {
        let server = arr(json!([]), "/", &["config", "downloads"]).await;
        let result = check(&api(&server), "/downloads", "putio-sync").await;
        assert_eq!(result.status, PathStatus::Visible);
        assert_eq!(result.arr_path, "/downloads");
        assert_eq!(result.detail, None);
    }

    #[tokio::test]
    async fn not_visible_lists_the_mappings() {
        let maps = json!([{"host": "nas", "remotePath": "/downloads/", "localPath": "/mnt/dl/"}]);
        let server = arr(maps, "/", &["config", "tv"]).await;
        let result = check(&api(&server), "/downloads", "putio-sync").await;
        assert_eq!(result.status, PathStatus::NotVisible);
        assert_eq!(
            result.mappings,
            vec![mapping("nas", "/downloads/", "/mnt/dl/")]
        );
    }

    #[tokio::test]
    async fn a_mapping_is_applied_before_listing() {
        let maps = json!([{"host": "putio-sync", "remotePath": "/downloads/", "localPath": "/data/putio/"}]);
        let server = arr(maps, "/data", &["putio"]).await;
        let result = check(&api(&server), "/downloads", "putio-sync").await;
        assert_eq!(result.status, PathStatus::Visible);
        assert_eq!(result.arr_path, "/data/putio");
    }

    #[tokio::test]
    async fn an_error_is_unknown() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/v3/remotepathmapping")
            .with_status(401)
            .create_async()
            .await;
        let result = check(&api(&server), "/downloads", "putio-sync").await;
        assert_eq!(result.status, PathStatus::Unknown);
        assert!(result.detail.is_some());
    }
}
