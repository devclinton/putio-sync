//! Removing empty folders from the Put.io account
//!
//! One walk from the root lists every folder, decides bottom-up which hold no files, and
//! deletes the top-most deletable folder of each empty subtree; Put.io removes its empty
//! sub-folders with it. With `nested` off, only folders with nothing in them are deleted. Anything not fully seen (a listing error, a further page, the depth
//! limit) counts as not empty, so nothing that could hold a file is ever deleted.
//!
//! Feature 021: the caller can keep folders the user marked (a folder rule or a keep
//! pattern) with [`KeepFolders`]; every empty folder kept, for any reason, is listed in the
//! report with why.

use crate::client::PutioApiClient;
use crate::error::ClientResult;
use crate::models::{FileEntry, FilesListResponse};
use chrono::{DateTime, NaiveDateTime, Utc};
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, warn};

/// Folders deeper than this are not listed and count as not empty
pub const MAX_DEPTH: usize = 64;

/// Transfer statuses whose save folder may be deleted; any other status is still running
const FINISHED_TRANSFER_STATUSES: &[&str] = &["COMPLETED", "SEEDING"];

/// Folders the user keeps: given a folder's path from the root (`Movies/Old`, Put.io
/// names as listed), why it is kept, or `None`
#[derive(Clone)]
pub struct KeepFolders(Arc<KeepReason>);

type KeepReason = dyn Fn(&str) -> Option<String> + Send + Sync;

impl KeepFolders {
    /// Keep the folders `reason` gives a reason for
    pub fn new(reason: impl Fn(&str) -> Option<String> + Send + Sync + 'static) -> Self {
        Self(Arc::new(reason))
    }

    /// Why the folder at `path` is kept, if it is
    pub fn reason(&self, path: &str) -> Option<String> {
        (self.0)(path)
    }
}

impl fmt::Debug for KeepFolders {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("KeepFolders(..)")
    }
}

/// What one cleanup run may delete
#[derive(Debug, Clone)]
pub struct EmptyFolderOptions {
    /// Report what would be deleted, delete nothing
    pub dry_run: bool,
    /// Folders created less than this long ago are kept
    pub min_age: Duration,
    /// Folder ids never deleted (their empty sub-folders still are). The root, shared
    /// folders and the save folders of unfinished transfers are added by the run.
    pub protected: HashSet<i64>,
    /// The time ages are measured from
    pub now: DateTime<Utc>,
    /// Also delete folders whose sub-folders are all deletable. Off: a folder with any
    /// entry is kept, though its empty sub-folders are still deleted.
    pub nested: bool,
    /// Folders the user keeps out of the cleanup (feature 021)
    pub keep: Option<KeepFolders>,
}

/// A folder removed by the run, or that a dry run would remove
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemovedFolder {
    /// Put.io file id
    pub id: i64,
    /// Path from the root, e.g. `/Movies/Old`
    pub path: String,
    /// Hours since the folder was created
    pub age_hours: u64,
}

/// An empty folder the run kept, and why
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeptFolder {
    /// Put.io file id
    pub id: i64,
    /// Path from the root
    pub path: String,
    /// Why it was kept: `protected`, `too new`, or the user's keep setting
    pub reason: String,
}

/// What failed for a folder
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FailedStep {
    /// Listing the folder's contents
    List,
    /// Deleting the folder
    Delete,
}

/// A folder the run could not list or delete
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleanupFailure {
    /// Put.io file id
    pub id: i64,
    /// Whether listing or deleting it failed
    pub step: FailedStep,
    /// Path from the root
    pub path: String,
    /// The client error
    pub error: String,
}

/// What one cleanup run found and did
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmptyFolderReport {
    /// Whether this was a dry run (nothing deleted)
    pub dry_run: bool,
    /// Folders listed, the root included
    pub scanned: u64,
    /// Every folder removed (or that would be), nested ones included, parents after
    /// their children
    pub removed: Vec<RemovedFolder>,
    /// Empty folders kept because they are protected
    pub kept_protected: u64,
    /// Empty folders kept because they are younger than the minimum age, or of unknown age
    pub kept_too_new: u64,
    /// Empty folders kept by a folder rule or a keep pattern
    #[serde(default)]
    pub kept_by_user: u64,
    /// Every empty folder kept (the root aside), with why
    #[serde(default)]
    pub kept: Vec<KeptFolder>,
    /// Folders that could not be listed or deleted
    pub failures: Vec<CleanupFailure>,
}

/// Why a folder may not be deleted even when empty
#[derive(Debug, Clone, PartialEq, Eq)]
enum Keep {
    Root,
    Protected,
    /// A folder rule or keep pattern, with its reason
    User(String),
    TooNew,
}

/// A folder about to be walked
struct Node {
    id: i64,
    path: String,
    age_hours: u64,
    keep: Option<Keep>,
}

/// State carried through one walk
struct Walk {
    dry_run: bool,
    min_age: Duration,
    now: DateTime<Utc>,
    nested: bool,
    protected: HashSet<i64>,
    keep: Option<KeepFolders>,
    report: EmptyFolderReport,
}

impl Walk {
    fn node(&self, parent_path: &str, entry: &FileEntry) -> Node {
        let path = format!("{}/{}", parent_path, entry.name);
        let age = entry.created_at.as_deref().and_then(|c| age(c, self.now));
        let user = self.keep.as_ref().and_then(|k| k.reason(&path[1..]));
        let keep = if self.protected.contains(&entry.id) {
            Some(Keep::Protected)
        } else if let Some(reason) = user {
            Some(Keep::User(reason))
        } else {
            match age {
                Some(age) if age >= self.min_age => None,
                _ => Some(Keep::TooNew),
            }
        };
        Node {
            id: entry.id,
            path,
            age_hours: age.map_or(0, |a| a.as_secs() / 3600),
            keep,
        }
    }
}

/// Time since a Put.io `created_at` (`YYYY-MM-DDTHH:MM:SS`, UTC); none when unreadable.
/// A time in the future is age zero.
fn age(created_at: &str, now: DateTime<Utc>) -> Option<Duration> {
    let created = NaiveDateTime::parse_from_str(created_at, "%Y-%m-%dT%H:%M:%S")
        .ok()?
        .and_utc();
    Some((now - created).to_std().unwrap_or(Duration::ZERO))
}

impl PutioApiClient {
    /// Find empty folders in the account and delete them (or, with `dry_run`, only
    /// report them).
    ///
    /// A folder is deleted when it holds no files, directly or below, every sub-folder is
    /// deletable too (with `options.nested`; without it, when it holds nothing), and it is
    /// neither protected (`options.protected`, the root, shared,
    /// the save folder of an unfinished transfer) nor younger than `options.min_age`.
    /// Errors listing or deleting one folder are recorded in the report and keep its
    /// parents; the run goes on. Failing to read the transfer list or the root ends the
    /// run before anything is deleted.
    pub async fn remove_empty_folders(
        &self,
        options: &EmptyFolderOptions,
    ) -> ClientResult<EmptyFolderReport> {
        let mut protected = options.protected.clone();
        protected.insert(0);
        let transfers = self.list_transfers().await?;
        protected.extend(
            transfers
                .transfers
                .iter()
                .filter(|t| !FINISHED_TRANSFER_STATUSES.contains(&t.status.as_str()))
                .map(|t| t.save_parent_id),
        );

        let root = self.list_files(0).await?;
        let mut walk = Walk {
            dry_run: options.dry_run,
            min_age: options.min_age,
            now: options.now,
            nested: options.nested,
            protected,
            keep: options.keep.clone(),
            report: EmptyFolderReport {
                dry_run: options.dry_run,
                ..Default::default()
            },
        };
        let node = Node {
            id: 0,
            path: String::new(),
            age_hours: 0,
            keep: Some(Keep::Root),
        };
        self.settle(&mut walk, node, root, 0).await;
        Ok(walk.report)
    }

    /// List `node` and settle it. `None` when it is not deletable (its deletable
    /// sub-folders are then already deleted).
    fn visit<'s, 'w>(
        &'s self,
        walk: &'w mut Walk,
        node: Node,
        depth: usize,
    ) -> BoxFuture<'w, Option<Vec<RemovedFolder>>>
    where
        's: 'w,
    {
        Box::pin(async move {
            if depth > MAX_DEPTH {
                warn!(folder_id = node.id, path = %node.path, "Folder below the depth limit; kept");
                return None;
            }
            match self.list_files(node.id).await {
                Ok(listing) => self.settle(walk, node, listing, depth).await,
                Err(e) => {
                    warn!(folder_id = node.id, path = %node.path, error = %e, "Failed to list folder; kept");
                    walk.report.failures.push(CleanupFailure {
                        id: node.id,
                        step: FailedStep::List,
                        path: node.path,
                        error: e.to_string(),
                    });
                    None
                }
            }
        })
    }

    /// Decide `node` from its listing: `Some` with its deletable subtree (children first,
    /// itself last) when it is deletable; otherwise delete its deletable sub-folders now.
    fn settle<'s, 'w>(
        &'s self,
        walk: &'w mut Walk,
        node: Node,
        listing: FilesListResponse,
        depth: usize,
    ) -> BoxFuture<'w, Option<Vec<RemovedFolder>>>
    where
        's: 'w,
    {
        Box::pin(async move {
            walk.report.scanned += 1;
            // A further page was not read, so the folder is not known to be empty. Without
            // `nested`, a folder holding even empty folders is kept (they still go).
            let mut empty = listing.cursor.is_none() && (walk.nested || listing.files.is_empty());
            let mut deletable = Vec::new();
            for entry in &listing.files {
                // Shared folders belong to someone else's share: not descended into
                if !entry.is_folder || entry.is_shared {
                    empty = false;
                    continue;
                }
                let child = walk.node(&node.path, entry);
                match self.visit(&mut *walk, child, depth + 1).await {
                    Some(subtree) => deletable.push(subtree),
                    None => empty = false,
                }
            }

            if empty {
                match node.keep {
                    None => {
                        let mut subtree: Vec<RemovedFolder> =
                            deletable.into_iter().flatten().collect();
                        subtree.push(RemovedFolder {
                            id: node.id,
                            path: node.path,
                            age_hours: node.age_hours,
                        });
                        return Some(subtree);
                    }
                    Some(Keep::Root) => {}
                    Some(keep) => {
                        let reason = match keep {
                            Keep::Protected => {
                                walk.report.kept_protected += 1;
                                "protected".to_string()
                            }
                            Keep::TooNew => {
                                walk.report.kept_too_new += 1;
                                "too new".to_string()
                            }
                            Keep::User(reason) => {
                                walk.report.kept_by_user += 1;
                                reason
                            }
                            Keep::Root => unreachable!(),
                        };
                        debug!(folder_id = node.id, path = %node.path, %reason, "Kept empty folder");
                        walk.report.kept.push(KeptFolder {
                            id: node.id,
                            path: node.path,
                            reason,
                        });
                    }
                }
            }
            for subtree in deletable {
                self.delete_subtree(walk, subtree).await;
            }
            None
        })
    }

    /// Delete the top-most folder of an empty subtree (the last entry); Put.io removes
    /// the rest with it
    async fn delete_subtree(&self, walk: &mut Walk, subtree: Vec<RemovedFolder>) {
        let Some(top) = subtree.last() else { return };
        if !walk.dry_run {
            debug!(folder_id = top.id, path = %top.path, "Deleting empty folder");
            if let Err(e) = self.delete_files(vec![top.id]).await {
                warn!(folder_id = top.id, path = %top.path, error = %e, "Failed to delete empty folder");
                walk.report.failures.push(CleanupFailure {
                    id: top.id,
                    step: FailedStep::Delete,
                    path: top.path.clone(),
                    error: e.to_string(),
                });
                return;
            }
        }
        walk.report.removed.extend(subtree);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::RetryPolicy;
    use mockito::{Matcher, Mock, Server, ServerGuard};
    use serde_json::{json, Value};

    const OLD: &str = "2026-10-01T00:00:00";
    const NEW: &str = "2026-10-26T10:00:00";

    fn now() -> DateTime<Utc> {
        "2026-10-26T12:00:00Z".parse().unwrap()
    }

    fn options(dry_run: bool) -> EmptyFolderOptions {
        EmptyFolderOptions {
            dry_run,
            min_age: Duration::from_secs(24 * 3600),
            protected: HashSet::new(),
            now: now(),
            nested: true,
            keep: None,
        }
    }

    fn client(server: &ServerGuard) -> PutioApiClient {
        PutioApiClient::with_base_url("test_token".to_string(), server.url()).with_retry_policy(
            RetryPolicy {
                max_attempts: 1,
                initial_delay: Duration::from_millis(1),
                max_delay: Duration::from_millis(1),
            },
        )
    }

    // Shaped like Put.io's listing, which marks folders only by `file_type`
    fn folder(id: i64, name: &str, parent: i64, created_at: Option<&str>) -> Value {
        json!({
            "id": id, "name": name, "size": 0, "file_type": "FOLDER", "parent_id": parent,
            "created_at": created_at, "is_shared": false
        })
    }

    fn file(id: i64, parent: i64) -> Value {
        json!({
            "id": id, "name": format!("file{id}.mkv"), "size": 10, "file_type": "VIDEO",
            "parent_id": parent, "created_at": OLD
        })
    }

    async fn list(server: &mut ServerGuard, id: i64, files: Vec<Value>) -> Mock {
        list_with(server, id, files, None).await
    }

    async fn list_with(
        server: &mut ServerGuard,
        id: i64,
        files: Vec<Value>,
        cursor: Option<&str>,
    ) -> Mock {
        let body = json!({
            "status": "OK",
            "files": files,
            "parent": folder(id, "parent", 0, Some(OLD)),
            "total": 0,
            "cursor": cursor,
        });
        server
            .mock("GET", format!("/files/list?parent_id={id}").as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body.to_string())
            .create_async()
            .await
    }

    async fn transfers(server: &mut ServerGuard, transfers: Vec<Value>) -> Mock {
        server
            .mock("GET", "/transfers/list")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(json!({"status": "OK", "transfers": transfers}).to_string())
            .create_async()
            .await
    }

    fn transfer(id: i64, status: &str, save_parent_id: i64) -> Value {
        json!({
            "id": id, "name": "t", "status": status, "created_at": OLD,
            "save_parent_id": save_parent_id
        })
    }

    async fn delete(server: &mut ServerGuard, id: i64) -> Mock {
        server
            .mock("POST", "/files/delete")
            .match_body(Matcher::Json(json!({"file_ids": id.to_string()})))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status": "OK"}"#)
            .expect(1)
            .create_async()
            .await
    }

    async fn no_deletes(server: &mut ServerGuard) -> Mock {
        server
            .mock("POST", "/files/delete")
            .expect(0)
            .create_async()
            .await
    }

    fn ids(report: &EmptyFolderReport) -> Vec<i64> {
        report.removed.iter().map(|r| r.id).collect()
    }

    /// `A/` empty, `B/C/` both empty, `D/` holds a file
    async fn nested_tree(server: &mut ServerGuard) -> Vec<Mock> {
        vec![
            transfers(server, vec![]).await,
            list(
                server,
                0,
                vec![
                    folder(10, "A", 0, Some(OLD)),
                    folder(20, "B", 0, Some(OLD)),
                    folder(30, "D", 0, Some(OLD)),
                ],
            )
            .await,
            list(server, 10, vec![]).await,
            list(server, 20, vec![folder(21, "C", 20, Some(OLD))]).await,
            list(server, 21, vec![]).await,
            list(server, 30, vec![file(31, 30)]).await,
        ]
    }

    #[tokio::test]
    async fn nested_empties_removed_with_one_delete_per_subtree() {
        let mut server = Server::new_async().await;
        let _tree = nested_tree(&mut server).await;
        let del_a = delete(&mut server, 10).await;
        let del_b = delete(&mut server, 20).await;

        let report = client(&server)
            .remove_empty_folders(&options(false))
            .await
            .unwrap();

        del_a.assert_async().await;
        del_b.assert_async().await;
        assert_eq!(ids(&report), vec![10, 21, 20]);
        let paths: Vec<&str> = report.removed.iter().map(|r| r.path.as_str()).collect();
        assert_eq!(paths, vec!["/A", "/B/C", "/B"]);
        assert_eq!(report.removed[0].age_hours, 25 * 24 + 12);
        assert_eq!(report.scanned, 5);
        assert!(report.failures.is_empty());
        assert!(!report.dry_run);
    }

    #[tokio::test]
    async fn without_nested_only_folders_with_nothing_in_them_go() {
        let mut server = Server::new_async().await;
        let _tree = nested_tree(&mut server).await;
        let del_a = delete(&mut server, 10).await;
        let del_c = delete(&mut server, 21).await;

        let mut opts = options(false);
        opts.nested = false;
        let report = client(&server).remove_empty_folders(&opts).await.unwrap();

        del_a.assert_async().await;
        del_c.assert_async().await;
        // B held C when listed, so it stays until a later run. C goes while B is settled,
        // A when the root is.
        assert_eq!(ids(&report), vec![21, 10]);
        assert!(report.failures.is_empty());
    }

    #[tokio::test]
    async fn dry_run_reports_the_same_folders_and_deletes_nothing() {
        let mut server = Server::new_async().await;
        let _tree = nested_tree(&mut server).await;
        let deletes = no_deletes(&mut server).await;

        let report = client(&server)
            .remove_empty_folders(&options(true))
            .await
            .unwrap();

        deletes.assert_async().await;
        assert!(report.dry_run);
        assert_eq!(ids(&report), vec![10, 21, 20]);
    }

    #[tokio::test]
    async fn protected_folder_kept_but_its_empty_children_removed() {
        let mut server = Server::new_async().await;
        let _t = transfers(&mut server, vec![]).await;
        let _root = list(&mut server, 0, vec![folder(40, "Uploads", 0, Some(OLD))]).await;
        let _p = list(&mut server, 40, vec![folder(41, "E", 40, Some(OLD))]).await;
        let _e = list(&mut server, 41, vec![]).await;
        let del = delete(&mut server, 41).await;

        let mut opts = options(false);
        opts.protected.insert(40);
        let report = client(&server).remove_empty_folders(&opts).await.unwrap();

        del.assert_async().await;
        assert_eq!(ids(&report), vec![41]);
        assert_eq!(report.kept_protected, 1);
    }

    #[tokio::test]
    async fn user_kept_folders_are_kept_and_listed_with_the_reason() {
        let mut server = Server::new_async().await;
        let _t = transfers(&mut server, vec![]).await;
        let _root = list(
            &mut server,
            0,
            vec![
                folder(100, "Movies", 0, Some(OLD)),
                folder(110, "Old", 0, Some(OLD)),
                folder(120, "New", 0, Some(NEW)),
                folder(130, "Uploads", 0, Some(OLD)),
            ],
        )
        .await;
        let _m = list(
            &mut server,
            100,
            vec![folder(101, "Extras", 100, Some(OLD))],
        )
        .await;
        let _x = list(&mut server, 101, vec![]).await;
        let _o = list(&mut server, 110, vec![]).await;
        let _n = list(&mut server, 120, vec![]).await;
        let _u = list(&mut server, 130, vec![]).await;
        let del = delete(&mut server, 110).await;

        let mut opts = options(false);
        opts.protected.insert(130);
        opts.keep = Some(KeepFolders::new(|path| {
            (path == "Movies" || path.starts_with("Movies/"))
                .then(|| "kept by the rule on Movies".to_string())
        }));
        let report = client(&server).remove_empty_folders(&opts).await.unwrap();

        del.assert_async().await;
        assert_eq!(ids(&report), vec![110]);
        // Movies holds a kept folder, so it is not empty and not listed itself
        assert_eq!(report.kept_by_user, 1);
        assert_eq!(report.kept_protected, 1);
        assert_eq!(report.kept_too_new, 1);
        let kept: Vec<(i64, &str, &str)> = report
            .kept
            .iter()
            .map(|k| (k.id, k.path.as_str(), k.reason.as_str()))
            .collect();
        assert_eq!(
            kept,
            vec![
                (101, "/Movies/Extras", "kept by the rule on Movies"),
                (120, "/New", "too new"),
                (130, "/Uploads", "protected"),
            ]
        );
    }

    #[tokio::test]
    async fn protected_wins_over_a_user_keep() {
        let mut server = Server::new_async().await;
        let _t = transfers(&mut server, vec![]).await;
        let _root = list(&mut server, 0, vec![folder(140, "Uploads", 0, Some(OLD))]).await;
        let _u = list(&mut server, 140, vec![]).await;
        let deletes = no_deletes(&mut server).await;

        let mut opts = options(false);
        opts.protected.insert(140);
        opts.keep = Some(KeepFolders::new(|_| Some("kept".to_string())));
        let report = client(&server).remove_empty_folders(&opts).await.unwrap();

        deletes.assert_async().await;
        assert_eq!(report.kept_protected, 1);
        assert_eq!(report.kept_by_user, 0);
        assert_eq!(report.kept.len(), 1);
    }

    #[tokio::test]
    async fn shared_folder_kept_and_not_listed() {
        let mut server = Server::new_async().await;
        let _t = transfers(&mut server, vec![]).await;
        let mut shared = folder(50, "Shared", 0, Some(OLD));
        shared["is_shared"] = json!(true);
        let _root = list(&mut server, 0, vec![shared]).await;
        let listed = server
            .mock("GET", "/files/list?parent_id=50")
            .expect(0)
            .create_async()
            .await;
        let deletes = no_deletes(&mut server).await;

        let report = client(&server)
            .remove_empty_folders(&options(false))
            .await
            .unwrap();

        listed.assert_async().await;
        deletes.assert_async().await;
        assert!(report.removed.is_empty());
        assert!(report.failures.is_empty());
    }

    #[tokio::test]
    async fn too_new_and_unknown_age_kept_with_their_parents() {
        let mut server = Server::new_async().await;
        let _t = transfers(&mut server, vec![]).await;
        let _root = list(
            &mut server,
            0,
            vec![
                folder(60, "New", 0, Some(NEW)),
                folder(70, "E", 0, Some(OLD)),
                folder(80, "Unknown", 0, None),
                folder(90, "Garbled", 0, Some("yesterday")),
            ],
        )
        .await;
        let _n = list(&mut server, 60, vec![]).await;
        let _e = list(&mut server, 70, vec![folder(71, "F", 70, Some(NEW))]).await;
        let _f = list(&mut server, 71, vec![]).await;
        let _u = list(&mut server, 80, vec![]).await;
        let _g = list(&mut server, 90, vec![]).await;
        let deletes = no_deletes(&mut server).await;

        let report = client(&server)
            .remove_empty_folders(&options(false))
            .await
            .unwrap();

        deletes.assert_async().await;
        assert!(report.removed.is_empty());
        // New, F, Unknown and Garbled; E is not empty while F stays
        assert_eq!(report.kept_too_new, 4);
    }

    #[tokio::test]
    async fn unfinished_transfer_target_kept() {
        let mut server = Server::new_async().await;
        let _t = transfers(
            &mut server,
            vec![
                transfer(1, "DOWNLOADING", 90),
                transfer(2, "COMPLETED", 91),
                transfer(3, "SEEDING", 92),
            ],
        )
        .await;
        let _root = list(
            &mut server,
            0,
            vec![
                folder(90, "Active", 0, Some(OLD)),
                folder(91, "Done", 0, Some(OLD)),
                folder(92, "Seeding", 0, Some(OLD)),
            ],
        )
        .await;
        let _a = list(&mut server, 90, vec![]).await;
        let _d = list(&mut server, 91, vec![]).await;
        let _s = list(&mut server, 92, vec![]).await;
        let del_d = delete(&mut server, 91).await;
        let del_s = delete(&mut server, 92).await;

        let report = client(&server)
            .remove_empty_folders(&options(false))
            .await
            .unwrap();

        del_d.assert_async().await;
        del_s.assert_async().await;
        assert_eq!(ids(&report), vec![91, 92]);
        assert_eq!(report.kept_protected, 1);
    }

    #[tokio::test]
    async fn list_error_is_a_failure_and_keeps_ancestors() {
        let mut server = Server::new_async().await;
        let _t = transfers(&mut server, vec![]).await;
        let _root = list(&mut server, 0, vec![folder(100, "G", 0, Some(OLD))]).await;
        let _g = list(&mut server, 100, vec![folder(101, "H", 100, Some(OLD))]).await;
        let _h = server
            .mock("GET", "/files/list?parent_id=101")
            .with_status(404)
            .with_body(r#"{"status":"ERROR","error_message":"not found"}"#)
            .create_async()
            .await;
        let deletes = no_deletes(&mut server).await;

        let report = client(&server)
            .remove_empty_folders(&options(false))
            .await
            .unwrap();

        deletes.assert_async().await;
        assert!(report.removed.is_empty());
        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.failures[0].id, 101);
        assert_eq!(report.failures[0].step, FailedStep::List);
        assert_eq!(report.failures[0].path, "/G/H");
        assert!(!report.failures[0].error.contains("test_token"));
    }

    #[tokio::test]
    async fn delete_error_is_a_failure() {
        let mut server = Server::new_async().await;
        let _t = transfers(&mut server, vec![]).await;
        let _root = list(&mut server, 0, vec![folder(10, "A", 0, Some(OLD))]).await;
        let _a = list(&mut server, 10, vec![]).await;
        let _del = server
            .mock("POST", "/files/delete")
            .with_status(400)
            .with_body(r#"{"status":"ERROR","error_message":"nope"}"#)
            .create_async()
            .await;

        let report = client(&server)
            .remove_empty_folders(&options(false))
            .await
            .unwrap();

        assert!(report.removed.is_empty());
        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.failures[0].id, 10);
        assert_eq!(report.failures[0].step, FailedStep::Delete);
    }

    #[tokio::test]
    async fn folder_with_a_further_page_is_kept() {
        let mut server = Server::new_async().await;
        let _t = transfers(&mut server, vec![]).await;
        let _root = list(&mut server, 0, vec![folder(110, "Big", 0, Some(OLD))]).await;
        let _k = list_with(
            &mut server,
            110,
            vec![folder(111, "L", 110, Some(OLD))],
            Some("next-page"),
        )
        .await;
        let _l = list(&mut server, 111, vec![]).await;
        let del = delete(&mut server, 111).await;

        let report = client(&server)
            .remove_empty_folders(&options(false))
            .await
            .unwrap();

        del.assert_async().await;
        assert_eq!(ids(&report), vec![111]);
    }

    #[tokio::test]
    async fn transfer_list_error_fails_before_anything_is_listed() {
        let mut server = Server::new_async().await;
        let _t = server
            .mock("GET", "/transfers/list")
            .with_status(401)
            .with_body(r#"{"status":"ERROR","error_message":"bad token"}"#)
            .create_async()
            .await;
        let listed = server
            .mock("GET", Matcher::Regex("^/files/list".to_string()))
            .expect(0)
            .create_async()
            .await;
        let deletes = no_deletes(&mut server).await;

        let result = client(&server).remove_empty_folders(&options(false)).await;

        assert!(result.is_err());
        listed.assert_async().await;
        deletes.assert_async().await;
    }

    #[test]
    fn age_parses_putio_times() {
        assert_eq!(
            age("2026-10-25T12:00:00", now()),
            Some(Duration::from_secs(24 * 3600))
        );
        assert_eq!(age("2026-10-27T12:00:00", now()), Some(Duration::ZERO));
        assert_eq!(age("not a time", now()), None);
    }
}
