//! The traits a `release_reports` row keeps (FR-025): file types, counts, sizes and
//! tracker hosts. Never a magnet link, URL or .torrent bytes.

use serde::Serialize;

use crate::metainfo::Metainfo;
use crate::rule::Policy;

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
        let mut types: Vec<TypeCount> = Vec::new();
        let mut largest_wanted = 0;
        for file in &meta.files {
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
            file_count: meta.files.len() as u64,
            total_size: meta.total_size(),
            types,
            largest_wanted,
            tracker_hosts: meta.tracker_hosts.clone(),
            private: meta.private,
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
