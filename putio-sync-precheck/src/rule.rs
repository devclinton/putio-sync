//! The rule (research R4): a release passes when one file is a wanted type of at least
//! the minimum size. Pure, so it is tested without a network.

use std::collections::BTreeSet;

use crate::metainfo::ReleaseFile;

/// Archive formats, accepted only with `allow_archives`. Split parts (`.r00`, `.001`)
/// are matched by [`is_archive`].
pub const ARCHIVES: &[&str] = &[
    "7z", "zip", "rar", "tar", "gz", "bz2", "xz", "zst", "cab", "iso", "img",
];

/// How many extensions a "found" list names
const FOUND_LIMIT: usize = 5;

const MB: u64 = 1024 * 1024;

/// What a folder wants, from `[precheck]` and its 009 rule
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    pub enabled: bool,
    /// Lower-case extensions without the dot
    pub wanted: BTreeSet<String>,
    /// The 009 rule path the wanted types come from; `None` for the video default
    pub wanted_from: Option<String>,
    /// Bytes
    pub min_size: u64,
    pub allow_archives: bool,
    /// The word in a reason: "video", "audio", or the rule's types
    pub kind: String,
}

impl Policy {
    fn wants(&self, ext: &str) -> bool {
        self.wanted.contains(ext) || (self.allow_archives && is_archive(ext))
    }
}

/// Where the file list was checked
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Before anything reached Put.io
    Before,
    /// On Put.io, once a magnet fallback's transfer completed
    Putio,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Before => "before",
            Stage::Putio => "putio",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Pass { largest_wanted: u64 },
    Reject(Reason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// No file of a wanted type; `found` lists the extensions there are, largest first
    NoWanted { kind: String, found: Vec<String> },
    /// Wanted files, all under the minimum
    SmallWanted {
        kind: String,
        largest: u64,
        min: u64,
    },
    /// Not a .torrent we can read
    Invalid,
    /// A pending fallback whose transfer can no longer be checked; never counted as fake
    Gone,
}

impl Reason {
    /// The stable key for stats and the API
    pub fn key(&self) -> &'static str {
        match self {
            Reason::NoWanted { .. } => "no_wanted",
            Reason::SmallWanted { .. } => "small_wanted",
            Reason::Invalid => "invalid",
            Reason::Gone => "gone",
        }
    }

    /// The text the *arr app shows as the item's error (contracts/api.md)
    pub fn message(&self, stage: Stage) -> String {
        let detail = match self {
            Reason::NoWanted { kind, found } if found.is_empty() => format!("no {kind} file"),
            Reason::NoWanted { kind, found } => {
                let found: Vec<String> = found.iter().map(|e| format!(".{e}")).collect();
                format!("no {kind} file (found {})", found.join(", "))
            }
            Reason::SmallWanted { kind, largest, min } => format!(
                "only a small {kind} ({} MB, minimum {} MB)",
                largest / MB,
                min / MB
            ),
            Reason::Invalid => "not a valid .torrent".to_string(),
            Reason::Gone => return "transfer no longer on Put.io".to_string(),
        };
        match stage {
            Stage::Before => format!("rejected before Put.io: {detail}"),
            Stage::Putio => format!("rejected on Put.io: {detail} (checked on Put.io)"),
        }
    }
}

/// An archive extension, or a split part: `rNN`, `NNN` (`.partN.rar` ends in `rar`)
pub fn is_archive(ext: &str) -> bool {
    let b = ext.as_bytes();
    ARCHIVES.contains(&ext)
        || (b.len() == 3 && b[0] == b'r' && b[1..].iter().all(u8::is_ascii_digit))
        || (b.len() == 3 && b.iter().all(u8::is_ascii_digit))
}

pub fn evaluate(policy: &Policy, files: &[ReleaseFile]) -> Verdict {
    let largest_wanted = files
        .iter()
        .filter(|f| f.extension().is_some_and(|e| policy.wants(&e)))
        .map(|f| f.size)
        .max();
    if !policy.enabled {
        return Verdict::Pass {
            largest_wanted: largest_wanted.unwrap_or(0),
        };
    }
    match largest_wanted {
        None => Verdict::Reject(Reason::NoWanted {
            kind: policy.kind.clone(),
            found: found(files),
        }),
        Some(largest) if largest < policy.min_size => Verdict::Reject(Reason::SmallWanted {
            kind: policy.kind.clone(),
            largest,
            min: policy.min_size,
        }),
        Some(largest) => Verdict::Pass {
            largest_wanted: largest,
        },
    }
}

/// The distinct extensions, by their largest file, biggest first
fn found(files: &[ReleaseFile]) -> Vec<String> {
    let mut by_size: Vec<(u64, String)> = Vec::new();
    for file in files {
        let Some(ext) = file.extension() else {
            continue;
        };
        match by_size.iter_mut().find(|(_, e)| *e == ext) {
            Some(entry) => entry.0 = entry.0.max(file.size),
            None => by_size.push((file.size, ext)),
        }
    }
    by_size.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    by_size
        .into_iter()
        .take(FOUND_LIMIT)
        .map(|(_, e)| e)
        .collect()
}
