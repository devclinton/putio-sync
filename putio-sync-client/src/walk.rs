//! A read-only walk of the whole account for the doctor (feature 010)
//!
//! Lists folders a few at a time, breadth first, and stops at a deadline so a huge account
//! cannot hold the doctor up. A folder that fails to list is recorded and skipped.

use crate::client::PutioApiClient;
use crate::models::FileEntry;
use futures_util::stream::{FuturesUnordered, StreamExt};
use serde::Serialize;
use std::collections::VecDeque;
use tokio::time::Instant;
use tracing::warn;

/// One file or folder found by [`PutioApiClient::walk_files`]
#[derive(Debug, Clone, Serialize)]
pub struct WalkedFile {
    pub id: i64,
    pub name: String,
    pub size: i64,
    pub parent_id: i64,
    /// Path from the root, starting with `/`
    pub path: String,
    pub is_folder: bool,
    /// Lower-case extension without the dot, when the name has one
    pub extension: Option<String>,
    /// Put.io `created_at` (`YYYY-MM-DDTHH:MM:SS`, UTC)
    pub created_at: Option<String>,
    /// Shared with another Put.io user (feature 033 never deletes these or what is
    /// under them)
    #[serde(skip)]
    pub is_shared: bool,
    /// Put.io's CRC32, when the listing gives one (feature 034)
    #[serde(skip)]
    pub crc32: Option<String>,
}

/// A folder that could not be listed
#[derive(Debug, Clone, Serialize)]
pub struct WalkError {
    pub folder_id: i64,
    pub path: String,
    pub error: String,
}

/// Everything found by one walk
#[derive(Debug, Default)]
pub struct FileWalk {
    pub files: Vec<WalkedFile>,
    /// False when the deadline cut the walk short
    pub complete: bool,
    pub folders_listed: usize,
    pub errors: Vec<WalkError>,
}

fn extension(name: &str) -> Option<String> {
    let (stem, ext) = name.rsplit_once('.')?;
    (!stem.is_empty() && !ext.is_empty()).then(|| ext.to_ascii_lowercase())
}

fn walked(entry: FileEntry, parent_id: i64, parent_path: &str) -> WalkedFile {
    WalkedFile {
        path: format!("{}/{}", parent_path, entry.name),
        extension: if entry.is_folder {
            None
        } else {
            extension(&entry.name)
        },
        id: entry.id,
        size: entry.size,
        parent_id,
        is_folder: entry.is_folder,
        created_at: entry.created_at,
        is_shared: entry.is_shared,
        crc32: entry.crc32,
        name: entry.name,
    }
}

impl PutioApiClient {
    /// List every file and folder in the account, `concurrency` folders at a time
    ///
    /// Stops listing new folders at `deadline`; the result then has `complete: false`.
    pub async fn walk_files(&self, concurrency: usize, deadline: Instant) -> FileWalk {
        let start = VecDeque::from([(0, String::new())]);
        self.walk_from(start, FileWalk::default(), concurrency, deadline)
            .await
    }

    /// List `root_id` and, when it is a folder, everything under it (feature 042's check
    /// on Put.io). Paths start at the root's name: `/Show.S01E03/Show.S01E03.mkv`.
    pub async fn walk_tree(&self, root_id: i64, concurrency: usize, deadline: Instant) -> FileWalk {
        let mut walk = FileWalk::default();
        let details = tokio::time::timeout_at(deadline, self.get_file_details(root_id)).await;
        let root = match details {
            Ok(Ok(details)) => details.file,
            Ok(Err(e)) => {
                walk.errors.push(WalkError {
                    folder_id: root_id,
                    path: String::new(),
                    error: e.to_string(),
                });
                return walk;
            }
            Err(_) => return walk,
        };
        let parent_id = root.parent_id.unwrap_or(0);
        let root = walked(root, parent_id, "");
        let start = if root.is_folder {
            VecDeque::from([(root.id, root.path.clone())])
        } else {
            VecDeque::new()
        };
        walk.files.push(root);
        self.walk_from(start, walk, concurrency, deadline).await
    }

    async fn walk_from(
        &self,
        mut pending: VecDeque<(i64, String)>,
        mut walk: FileWalk,
        concurrency: usize,
        deadline: Instant,
    ) -> FileWalk {
        let concurrency = concurrency.max(1);
        let mut in_flight = FuturesUnordered::new();
        let mut timed_out = false;

        loop {
            while in_flight.len() < concurrency && !timed_out {
                let Some((id, path)) = pending.pop_front() else {
                    break;
                };
                in_flight.push(async move { (id, path, self.list_files(id).await) });
            }
            if in_flight.is_empty() {
                break;
            }
            let next = tokio::time::timeout_at(deadline, in_flight.next()).await;
            let Ok(Some((id, path, listing))) = next else {
                // Deadline reached: abandon whatever is still listing
                timed_out = true;
                break;
            };
            match listing {
                Ok(listing) => {
                    walk.folders_listed += 1;
                    for entry in listing.files {
                        let file = walked(entry, id, &path);
                        if file.is_folder {
                            pending.push_back((file.id, file.path.clone()));
                        }
                        walk.files.push(file);
                    }
                }
                Err(e) => {
                    warn!(folder_id = id, path = %path, error = %e, "Doctor walk: failed to list folder");
                    walk.errors.push(WalkError {
                        folder_id: id,
                        path,
                        error: e.to_string(),
                    });
                }
            }
        }
        walk.complete = !timed_out && pending.is_empty();
        walk
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::RetryPolicy;
    use mockito::Server;
    use std::time::Duration;

    fn entry(id: i64, name: &str, size: i64, folder: bool) -> String {
        format!(
            r#"{{"id":{id},"name":"{name}","size":{size},"file_type":"{}","parent_id":null,
               "is_folder":{folder},"created_at":"2026-01-01T00:00:00"}}"#,
            if folder { "FOLDER" } else { "VIDEO" }
        )
    }

    fn listing(entries: &[String]) -> String {
        format!(
            r#"{{"status":"OK","files":[{}],"parent":{{"id":0,"name":"root","size":0,
               "file_type":"FOLDER","parent_id":null,"is_folder":true}}}}"#,
            entries.join(",")
        )
    }

    fn client(url: String) -> PutioApiClient {
        PutioApiClient::with_base_url("t".into(), url).with_retry_policy(RetryPolicy {
            max_attempts: 1,
            ..RetryPolicy::default()
        })
    }

    fn far() -> Instant {
        Instant::now() + Duration::from_secs(30)
    }

    #[tokio::test]
    async fn walks_nested_folders_with_paths() {
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/files/list?parent_id=0")
            .with_body(listing(&[
                entry(1, "Movies", 0, true),
                entry(2, "a.torrent", 10, false),
            ]))
            .create_async()
            .await;
        server
            .mock("GET", "/files/list?parent_id=1")
            .with_body(listing(&[entry(3, "Film.MKV", 5_000_000, false)]))
            .create_async()
            .await;

        let walk = client(server.url()).walk_files(4, far()).await;
        assert!(walk.complete);
        assert_eq!(walk.folders_listed, 2);
        let film = walk.files.iter().find(|f| f.id == 3).unwrap();
        assert_eq!(film.path, "/Movies/Film.MKV");
        assert_eq!(film.parent_id, 1);
        assert_eq!(film.extension.as_deref(), Some("mkv"));
        let torrent = walk.files.iter().find(|f| f.id == 2).unwrap();
        assert_eq!(torrent.extension.as_deref(), Some("torrent"));
        assert!(walk
            .files
            .iter()
            .find(|f| f.id == 1)
            .unwrap()
            .extension
            .is_none());
    }

    #[tokio::test]
    async fn failed_folder_is_recorded_and_skipped() {
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/files/list?parent_id=0")
            .with_body(listing(&[entry(1, "Broken", 0, true)]))
            .create_async()
            .await;
        server
            .mock("GET", "/files/list?parent_id=1")
            .with_status(500)
            .create_async()
            .await;

        let walk = client(server.url()).walk_files(4, far()).await;
        assert!(walk.complete);
        assert_eq!(walk.errors.len(), 1);
        assert_eq!(walk.errors[0].path, "/Broken");
    }

    #[tokio::test]
    async fn deadline_marks_walk_incomplete() {
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/files/list?parent_id=0")
            .with_body(listing(&[entry(1, "A", 0, true)]))
            .create_async()
            .await;
        let walk = client(server.url())
            .walk_files(4, Instant::now() - Duration::from_secs(1))
            .await;
        assert!(!walk.complete);
    }

    #[tokio::test]
    async fn walks_one_tree_from_its_root() {
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/files/7")
            .with_body(format!(
                r#"{{"status":"OK","file":{}}}"#,
                entry(7, "Show", 0, true)
            ))
            .create_async()
            .await;
        server
            .mock("GET", "/files/list?parent_id=7")
            .with_body(listing(&[entry(8, "Show.mkv", 9, false)]))
            .create_async()
            .await;
        let walk = client(server.url()).walk_tree(7, 4, far()).await;
        assert!(walk.complete);
        let paths: Vec<_> = walk.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["/Show", "/Show/Show.mkv"]);

        server
            .mock("GET", "/files/9")
            .with_body(format!(
                r#"{{"status":"OK","file":{}}}"#,
                entry(9, "a.mkv", 5, false)
            ))
            .create_async()
            .await;
        let walk = client(server.url()).walk_tree(9, 4, far()).await;
        assert!(walk.complete);
        assert_eq!(walk.files.len(), 1);
        assert_eq!(walk.files[0].path, "/a.mkv");
    }

    #[test]
    fn extension_rules() {
        assert_eq!(extension("x.Torrent").as_deref(), Some("torrent"));
        assert_eq!(extension(".hidden"), None);
        assert_eq!(extension("noext"), None);
        assert_eq!(extension("trailing."), None);
    }
}
