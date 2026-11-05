//! Which files a free-up run may delete, and how many (feature 033, research R3/R4)
//!
//! Pure: the service gathers the walk, the database rows and the transfers, and this
//! module decides. Nothing here talks to Put.io or the disk except through `local_size`.

use putio_sync_client::WalkedFile;
use serde::Serialize;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

/// Most files one plan lists
pub const MAX_PLAN_FILES: usize = 1000;

/// A file the run may delete
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Candidate {
    pub id: i64,
    pub path: String,
    pub size: i64,
    /// Put.io `created_at`, the date the file was added
    pub created_at: Option<String>,
    pub local_path: String,
}

/// What the planner decides from
pub struct Inputs<'a> {
    /// The whole account, folders included
    pub files: &'a [WalkedFile],
    /// Completed downloads: file id → every `(size, local path)` recorded for it
    pub completed: &'a HashMap<i64, Vec<(i64, String)>>,
    /// Ids pending, downloading or paused in the queue
    pub queued: &'a HashSet<i64>,
    /// Folder and file ids of unfinished transfers (never the root)
    pub unfinished: &'a HashSet<i64>,
}

/// Bytes free at `target_percent` of `size`, rounded up
pub fn target_bytes(size: i64, target_percent: u8) -> i64 {
    if size <= 0 {
        return 0;
    }
    let size = size as i128;
    ((size * target_percent as i128 + 99) / 100) as i64
}

/// Bytes still to free to reach the target; 0 when it is met
pub fn needed_bytes(size: i64, free: i64, target_percent: u8) -> i64 {
    (target_bytes(size, target_percent) - free).max(0)
}

/// Ids whose own entry, or an ancestor's, is shared or belongs to an unfinished transfer
fn protected(files: &[WalkedFile], unfinished: &HashSet<i64>) -> HashSet<i64> {
    let by_id: HashMap<i64, &WalkedFile> = files.iter().map(|f| (f.id, f)).collect();
    let mut memo: HashMap<i64, bool> = HashMap::new();
    let mut out = HashSet::new();
    for file in files {
        // Walk up until an answer is known, then fill it in on the way back
        let mut chain = Vec::new();
        let mut id = file.id;
        let answer = loop {
            if let Some(&known) = memo.get(&id) {
                break known;
            }
            if unfinished.contains(&id) && id != 0 {
                break true;
            }
            match by_id.get(&id) {
                Some(entry) if entry.is_shared => break true,
                Some(entry) if entry.parent_id != id && chain.len() < 256 => {
                    chain.push(id);
                    id = entry.parent_id;
                }
                _ => break false,
            }
        };
        memo.insert(id, answer);
        for link in chain {
            memo.insert(link, answer);
        }
        if answer {
            out.insert(file.id);
        }
    }
    out
}

/// Files the run may delete, oldest first (by `created_at`, missing dates last, then id)
///
/// `local_size` answers the size of a local file, or `None` when it is missing.
pub fn eligible(inputs: &Inputs<'_>, local_size: impl Fn(&str) -> Option<u64>) -> Vec<Candidate> {
    let protected = protected(inputs.files, inputs.unfinished);
    let mut out: Vec<Candidate> = inputs
        .files
        .iter()
        .filter(|f| !f.is_folder && !protected.contains(&f.id) && !inputs.queued.contains(&f.id))
        .filter_map(|f| {
            let records = inputs.completed.get(&f.id)?;
            let local_path = records.iter().find_map(|(size, path)| {
                (*size == f.size && local_size(path) == Some(f.size as u64)).then(|| path.clone())
            })?;
            Some(Candidate {
                id: f.id,
                path: f.path.clone(),
                size: f.size,
                created_at: f.created_at.clone(),
                local_path,
            })
        })
        .collect();
    out.sort_by(oldest_first);
    out
}

fn oldest_first(a: &Candidate, b: &Candidate) -> Ordering {
    match (&a.created_at, &b.created_at) {
        (Some(x), Some(y)) => x.cmp(y),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
    .then(a.id.cmp(&b.id))
}

/// The first candidates whose sizes reach `needed`, capped at [`MAX_PLAN_FILES`], and
/// their total
pub fn select(candidates: Vec<Candidate>, needed: i64) -> (Vec<Candidate>, i64) {
    let mut total = 0;
    let mut picked = Vec::new();
    if needed <= 0 {
        return (picked, total);
    }
    for c in candidates {
        if total >= needed || picked.len() >= MAX_PLAN_FILES {
            break;
        }
        total += c.size;
        picked.push(c);
    }
    (picked, total)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walked(
        id: i64,
        parent_id: i64,
        size: i64,
        folder: bool,
        created: Option<&str>,
    ) -> WalkedFile {
        WalkedFile {
            id,
            name: format!("f{id}"),
            size,
            parent_id,
            path: format!("/f{id}"),
            is_folder: folder,
            extension: None,
            created_at: created.map(str::to_string),
            is_shared: false,
            crc32: None,
        }
    }

    struct Fixture {
        files: Vec<WalkedFile>,
        completed: HashMap<i64, Vec<(i64, String)>>,
        queued: HashSet<i64>,
        unfinished: HashSet<i64>,
        local: HashMap<String, u64>,
    }

    impl Fixture {
        fn new(files: Vec<WalkedFile>) -> Self {
            let mut f = Self {
                files,
                completed: HashMap::new(),
                queued: HashSet::new(),
                unfinished: HashSet::new(),
                local: HashMap::new(),
            };
            // Every non-folder is downloaded and present unless a test says otherwise
            for w in f.files.clone() {
                if !w.is_folder {
                    f.downloaded(w.id, w.size);
                }
            }
            f
        }

        fn downloaded(&mut self, id: i64, size: i64) {
            let path = format!("/data/{id}");
            self.completed.insert(id, vec![(size, path.clone())]);
            self.local.insert(path, size as u64);
        }

        fn ids(&self) -> Vec<i64> {
            let inputs = Inputs {
                files: &self.files,
                completed: &self.completed,
                queued: &self.queued,
                unfinished: &self.unfinished,
            };
            eligible(&inputs, |p| self.local.get(p).copied())
                .iter()
                .map(|c| c.id)
                .collect()
        }
    }

    #[test]
    fn only_verified_downloads_are_eligible() {
        let mut fx = Fixture::new(vec![
            walked(1, 0, 10, false, Some("2026-01-01T00:00:00")),
            walked(2, 0, 10, false, Some("2026-01-02T00:00:00")),
            walked(3, 0, 10, false, Some("2026-01-03T00:00:00")),
            walked(4, 0, 10, false, Some("2026-01-04T00:00:00")),
            walked(5, 0, 10, false, Some("2026-01-05T00:00:00")),
        ]);
        fx.completed.remove(&2); // never downloaded
        fx.local.remove("/data/3"); // local copy gone
        fx.local.insert("/data/4".into(), 9); // local copy truncated
        fx.completed.insert(5, vec![(11, "/data/5".into())]); // Put.io size changed
        assert_eq!(fx.ids(), vec![1]);
    }

    #[test]
    fn any_matching_record_counts() {
        let mut fx = Fixture::new(vec![walked(1, 0, 10, false, None)]);
        fx.completed
            .insert(1, vec![(10, "/gone".into()), (10, "/data/1".into())]);
        let inputs = Inputs {
            files: &fx.files,
            completed: &fx.completed,
            queued: &fx.queued,
            unfinished: &fx.unfinished,
        };
        let c = eligible(&inputs, |p| fx.local.get(p).copied());
        assert_eq!(c[0].local_path, "/data/1");
    }

    #[test]
    fn queued_shared_and_transfer_files_are_kept() {
        let mut shared = walked(10, 0, 0, true, None);
        shared.is_shared = true;
        let mut shared_file = walked(6, 0, 10, false, None);
        shared_file.is_shared = true;
        let mut fx = Fixture::new(vec![
            shared,
            walked(11, 10, 0, true, None),
            walked(1, 11, 10, false, None), // under a shared folder, two levels up
            walked(20, 0, 0, true, None),
            walked(2, 20, 10, false, None), // in an unfinished transfer's save folder
            walked(3, 0, 10, false, None),  // queued
            walked(4, 0, 10, false, None),  // fine
            walked(5, 0, 0, true, None),    // a folder is never eligible
            shared_file,
        ]);
        fx.unfinished.insert(20);
        fx.unfinished.insert(0); // the root is never protected as a whole
        fx.queued.insert(3);
        assert_eq!(fx.ids(), vec![4]);
    }

    #[test]
    fn oldest_first_missing_dates_last_then_id() {
        let fx = Fixture::new(vec![
            walked(4, 0, 10, false, None),
            walked(3, 0, 10, false, Some("2026-03-01T00:00:00")),
            walked(2, 0, 10, false, None),
            walked(1, 0, 10, false, Some("2026-02-01T00:00:00")),
            walked(5, 0, 10, false, Some("2026-02-01T00:00:00")),
        ]);
        assert_eq!(fx.ids(), vec![1, 5, 3, 2, 4]);
    }

    fn cand(id: i64, size: i64) -> Candidate {
        Candidate {
            id,
            path: String::new(),
            size,
            created_at: None,
            local_path: String::new(),
        }
    }

    #[test]
    fn select_stops_once_needed_is_reached() {
        let (files, total) = select(vec![cand(1, 5), cand(2, 5), cand(3, 5)], 8);
        assert_eq!(files.iter().map(|c| c.id).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(total, 10);
        let (files, total) = select(vec![cand(1, 5)], 0);
        assert!(files.is_empty());
        assert_eq!(total, 0);
        // Not enough: everything, and the caller reports the shortfall
        let (files, total) = select(vec![cand(1, 5), cand(2, 5)], 100);
        assert_eq!(files.len(), 2);
        assert_eq!(total, 10);
    }

    #[test]
    fn select_caps_the_plan() {
        let many = (0..MAX_PLAN_FILES as i64 + 5).map(|i| cand(i, 1)).collect();
        let (files, total) = select(many, i64::MAX);
        assert_eq!(files.len(), MAX_PLAN_FILES);
        assert_eq!(total, MAX_PLAN_FILES as i64);
    }

    #[test]
    fn target_maths() {
        assert_eq!(target_bytes(100, 20), 20);
        assert_eq!(target_bytes(101, 20), 21); // rounded up
        assert_eq!(target_bytes(0, 20), 0);
        assert_eq!(needed_bytes(100, 5, 20), 15);
        assert_eq!(needed_bytes(100, 30, 20), 0);
        assert_eq!(needed_bytes(0, 0, 20), 0);
        // 10 TiB at 90% does not overflow
        assert_eq!(target_bytes(10 << 40, 90), (10i64 << 40) / 10 * 9);
    }
}
