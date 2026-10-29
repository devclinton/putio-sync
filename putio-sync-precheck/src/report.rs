//! The traits a `release_reports` row keeps (FR-025): file types, counts, sizes and
//! tracker hosts. Never a magnet link, URL or .torrent bytes.

use std::time::Duration;

use putio_sync_client::{FileWalk, PutioApiClient};
use serde::Serialize;
use tokio::time::Instant;

use crate::metainfo::{Metainfo, ReleaseFile};
use crate::rule::Policy;

/// How long the read-only walk for a flag's traits may take
const WALK_BUDGET: Duration = Duration::from_secs(5);

/// One extension's share of a release
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TypeCount {
    /// Lower case without the dot; empty for files without one
    pub ext: String,
    pub count: u64,
    pub largest: u64,
}

/// What a report says about a release
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Traits {
    pub file_count: u64,
    pub total_size: u64,
    /// Sorted by largest file, biggest first
    pub types: Vec<TypeCount>,
    pub largest_wanted: u64,
    pub tracker_hosts: Vec<String>,
    pub private: bool,
}

impl Traits {
    pub fn of(meta: &Metainfo, policy: &Policy) -> Traits {
        Traits {
            tracker_hosts: meta.tracker_hosts.clone(),
            private: meta.private,
            ..Traits::of_files(&meta.files, policy)
        }
    }

    /// The traits of a file list with no tracker or private flag known
    pub fn of_files(files: &[ReleaseFile], policy: &Policy) -> Traits {
        let mut types: Vec<TypeCount> = Vec::new();
        let mut largest_wanted = 0;
        for file in files {
            let ext = file.extension().unwrap_or_default();
            if policy.wanted.contains(&ext)
                || (policy.allow_archives && crate::rule::is_archive(&ext))
            {
                largest_wanted = largest_wanted.max(file.size);
            }
            match types.iter_mut().find(|t| t.ext == ext) {
                Some(t) => {
                    t.count += 1;
                    t.largest = t.largest.max(file.size);
                }
                None => types.push(TypeCount {
                    ext,
                    count: 1,
                    largest: file.size,
                }),
            }
        }
        types.sort_by(|a, b| b.largest.cmp(&a.largest).then_with(|| a.ext.cmp(&b.ext)));
        Traits {
            file_count: files.len() as u64,
            total_size: files.iter().map(|f| f.size).sum(),
            types,
            largest_wanted,
            tracker_hosts: Vec::new(),
            private: false,
        }
    }

    /// `types` as stored: `[{"ext":"exe","count":1,"largest":1503238553}]`
    pub fn types_json(&self) -> String {
        serde_json::to_string(&self.types).unwrap_or_else(|_| "[]".to_string())
    }

    /// `tracker_hosts` as stored: a JSON list of host names
    pub fn tracker_hosts_json(&self) -> String {
        serde_json::to_string(&self.tracker_hosts).unwrap_or_else(|_| "[]".to_string())
    }
}

/// The files of a walk, folders left out, paths below the root's parent
pub fn tree_files(tree: &FileWalk) -> Vec<ReleaseFile> {
    tree.files
        .iter()
        .filter(|f| !f.is_folder)
        .map(|f| ReleaseFile {
            path: f.path.trim_start_matches('/').to_string(),
            size: f.size.max(0) as u64,
        })
        .collect()
}

/// The traits of a flagged release (research R11): a read-only walk of `file_id` on
/// Put.io, else `known` (the local history rows). `None` when neither gives a file.
/// Tracker and private flag are the caller's, from what it recorded.
pub async fn traits_for(
    client: &PutioApiClient,
    file_id: Option<i64>,
    known: Vec<ReleaseFile>,
    policy: &Policy,
) -> Option<Traits> {
    let mut files = Vec::new();
    if let Some(file_id) = file_id {
        let tree = client
            .walk_tree(file_id, 4, Instant::now() + WALK_BUDGET)
            .await;
        if tree.complete && tree.errors.is_empty() {
            files = tree_files(&tree);
        }
    }
    if files.is_empty() {
        files = known;
    }
    (!files.is_empty()).then(|| Traits::of_files(&files, policy))
}
