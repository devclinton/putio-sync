//! Browsing and fixing files in the download and watch folders by hand (feature 044)
//!
//! Every path comes from the web UI as a root plus a `/`-separated path relative to it.
//! [`resolve`] keeps it inside the root: no `..`, and the parent folders are canonicalized,
//! so a symlink anywhere above the target cannot lead out. The last segment is never
//! followed, so a link is listed, renamed and deleted as a link (research R1).

use crate::watcher::{calculate_file_checksum, calculate_string_checksum, TorrentWatcher};
use putio_sync_db::Database;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use thiserror::Error;

/// The most entries one listing returns (research R6)
pub const LIST_LIMIT: usize = 2000;

/// Suffix of a partial download (`.<name>.downloading`)
const PARTIAL: &str = ".downloading";

/// Suffix the watcher gives a watch file that failed the release check (042)
const REJECTED: &str = ".rejected";

/// Why a local file request was refused
#[derive(Error, Debug)]
pub enum LocalFileError {
    /// The path or name is not allowed (traversal, bad name, wrong kind)
    #[error("{0}")]
    Invalid(String),
    /// The root or the entry is not there
    #[error("{0}")]
    NotFound(String),
    /// The new name is taken, or a running download writes the entry
    #[error("{0}")]
    Conflict(String),
    /// The filesystem refused
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

pub type LocalResult<T> = Result<T, LocalFileError>;

/// A path checked to be inside its root
#[derive(Debug, Clone)]
pub struct Resolved {
    /// Canonical root
    pub root: PathBuf,
    /// Canonical parent joined with the last segment (not followed)
    pub path: PathBuf,
    /// The relative path as given, without trailing `/`; empty for the root
    pub rel: String,
}

impl Resolved {
    pub fn is_root(&self) -> bool {
        self.rel.is_empty()
    }

    /// `rel` of a child named `name`
    fn child_rel(&self, name: &str) -> String {
        if self.rel.is_empty() {
            name.to_string()
        } else {
            format!("{}/{}", self.rel, name)
        }
    }

    /// `rel` of a sibling named `name`
    fn sibling_rel(&self, name: &str) -> String {
        match self.rel.rfind('/') {
            Some(slash) => format!("{}/{}", &self.rel[..slash], name),
            None => name.to_string(),
        }
    }
}

/// Resolve `rel` under `root`, refusing anything that could leave it
pub fn resolve(root: &Path, rel: &str) -> LocalResult<Resolved> {
    let canonical_root = root
        .canonicalize()
        .map_err(|_| LocalFileError::NotFound(format!("Folder {} is not there", root.display())))?;
    if !canonical_root.is_dir() {
        return Err(LocalFileError::NotFound(format!(
            "{} is not a folder",
            root.display()
        )));
    }
    if rel.starts_with('/') {
        return Err(LocalFileError::Invalid(
            "Paths are relative to the folder".into(),
        ));
    }
    let rel = rel.trim_end_matches('/');
    if rel.is_empty() {
        return Ok(Resolved {
            path: canonical_root.clone(),
            root: canonical_root,
            rel: String::new(),
        });
    }
    let segments: Vec<&str> = rel.split('/').collect();
    for segment in &segments {
        check_segment(segment)?;
    }
    let (last, parents) = segments.split_last().expect("rel is not empty");
    let mut parent = canonical_root.clone();
    parent.extend(parents);
    let parent = parent
        .canonicalize()
        .map_err(|_| LocalFileError::NotFound(format!("{rel} is not there")))?;
    if !parent.starts_with(&canonical_root) {
        return Err(LocalFileError::Invalid(format!(
            "{rel} leads outside the folder"
        )));
    }
    Ok(Resolved {
        path: parent.join(last),
        root: canonical_root,
        rel: rel.to_string(),
    })
}

/// One path segment: not empty, `.` or `..`, and no separator or NUL
fn check_segment(segment: &str) -> LocalResult<()> {
    if segment.is_empty() || segment == "." || segment == ".." {
        return Err(LocalFileError::Invalid(format!(
            "\"{segment}\" is not allowed in a path"
        )));
    }
    if segment.contains(['/', '\\', '\0']) {
        return Err(LocalFileError::Invalid(format!(
            "\"{segment}\" has a character that is not allowed"
        )));
    }
    Ok(())
}

/// What an entry is; links are never followed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Dir,
    Link,
}

impl EntryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EntryKind::File => "file",
            EntryKind::Dir => "dir",
            EntryKind::Link => "link",
        }
    }

    fn of(meta: &std::fs::Metadata) -> EntryKind {
        if meta.file_type().is_symlink() {
            EntryKind::Link
        } else if meta.is_dir() {
            EntryKind::Dir
        } else {
            EntryKind::File
        }
    }
}

/// One name in a listed folder
#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    /// Relative to the root, `/`-separated
    pub path: String,
    pub kind: EntryKind,
    /// Bytes; 0 for folders and links
    pub size: u64,
    pub modified: Option<SystemTime>,
    /// The name a partial download gets when done
    pub partial: Option<String>,
    /// The name a rejected watch file had
    pub rejected: Option<String>,
    /// A running download writes this partial, or this folder holds one
    pub in_use: bool,
    /// False for names that are not UTF-8; nothing is offered on them
    pub valid_name: bool,
}

/// A listed folder
#[derive(Debug, Clone)]
pub struct Listing {
    pub entries: Vec<Entry>,
    /// More than [`LIST_LIMIT`] entries; the rest were left out
    pub truncated: bool,
}

/// `.<name>.downloading` → `<name>`
pub fn partial_name(name: &str) -> Option<&str> {
    let base = name.strip_suffix(PARTIAL)?;
    let base = base.strip_prefix('.').unwrap_or(base);
    (!base.is_empty()).then_some(base)
}

/// `<name>.rejected` → `<name>`
pub fn rejected_name(name: &str) -> Option<&str> {
    name.strip_suffix(REJECTED).filter(|base| !base.is_empty())
}

/// Canonical forms of the partial paths a download writes, comparable with
/// [`Resolved::path`]. A path whose folder is gone is left out.
pub fn canonical_in_use(paths: impl IntoIterator<Item = PathBuf>) -> HashSet<PathBuf> {
    paths
        .into_iter()
        .filter_map(|path| {
            let parent = path.parent()?.canonicalize().ok()?;
            Some(parent.join(path.file_name()?))
        })
        .collect()
}

/// Whether `path` is in `in_use` (a partial or a database file), or a folder holding one
fn is_in_use(path: &Path, kind: EntryKind, in_use: &HashSet<PathBuf>) -> bool {
    match kind {
        EntryKind::Dir => in_use.iter().any(|p| p.starts_with(path)),
        _ => in_use.contains(path),
    }
}

/// The folder at `dir`: folders first, then files, by name ignoring case
pub fn list(dir: &Resolved, in_use: &HashSet<PathBuf>) -> LocalResult<Listing> {
    let meta = dir
        .path
        .symlink_metadata()
        .map_err(|_| LocalFileError::NotFound(format!("{} is not there", dir.rel)))?;
    if !meta.is_dir() {
        return Err(LocalFileError::Invalid(format!(
            "{} is not a folder",
            dir.rel
        )));
    }
    let mut entries = Vec::new();
    let mut truncated = false;
    for item in std::fs::read_dir(&dir.path)? {
        let item = item?;
        if entries.len() == LIST_LIMIT {
            truncated = true;
            break;
        }
        let Ok(meta) = item.path().symlink_metadata() else {
            continue; // gone since read_dir
        };
        let os_name = item.file_name();
        let valid_name = os_name.to_str().is_some();
        let name = os_name.to_string_lossy().into_owned();
        let kind = EntryKind::of(&meta);
        let file_like = kind == EntryKind::File;
        entries.push(Entry {
            path: dir.child_rel(&name),
            kind,
            size: if file_like { meta.len() } else { 0 },
            modified: meta.modified().ok(),
            partial: file_like
                .then(|| partial_name(&name).map(str::to_string))
                .flatten(),
            rejected: file_like
                .then(|| rejected_name(&name).map(str::to_string))
                .flatten(),
            in_use: is_in_use(&item.path(), kind, in_use),
            valid_name,
            name,
        });
    }
    entries.sort_by(|a, b| {
        (b.kind == EntryKind::Dir)
            .cmp(&(a.kind == EntryKind::Dir))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(Listing { entries, truncated })
}

/// What deleting an entry would remove, for the confirmation (research R5)
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Summary {
    pub files: u64,
    pub folders: u64,
    pub bytes: u64,
    pub in_use: bool,
}

/// Count what is under `target`, without following links
pub fn summary(target: &Resolved, in_use: &HashSet<PathBuf>) -> LocalResult<Summary> {
    let meta = action_target(target)?;
    let mut summary = Summary {
        in_use: is_in_use(&target.path, EntryKind::of(&meta), in_use),
        ..Summary::default()
    };
    let mut todo = vec![(target.path.clone(), meta)];
    while let Some((path, meta)) = todo.pop() {
        if meta.is_dir() {
            summary.folders += 1;
            for item in std::fs::read_dir(&path)?.flatten() {
                if let Ok(meta) = item.path().symlink_metadata() {
                    todo.push((item.path(), meta));
                }
            }
        } else {
            summary.files += 1;
            if meta.is_file() {
                summary.bytes += meta.len();
            }
        }
    }
    Ok(summary)
}

/// The entry an action works on: not the root, and there
fn action_target(target: &Resolved) -> LocalResult<std::fs::Metadata> {
    if target.is_root() {
        return Err(LocalFileError::Invalid(
            "The folder itself can't be changed here".into(),
        ));
    }
    target
        .path
        .symlink_metadata()
        .map_err(|_| LocalFileError::NotFound(format!("{} is not there", target.rel)))
}

fn refuse_in_use(target: &Resolved, kind: EntryKind, in_use: &HashSet<PathBuf>) -> LocalResult<()> {
    if is_in_use(&target.path, kind, in_use) {
        return Err(LocalFileError::Conflict(format!(
            "{} is being downloaded; cancel it in the Queue first",
            target.rel
        )));
    }
    Ok(())
}

/// Delete a file, a link (not what it points to) or a folder with everything in it
pub fn delete(target: &Resolved, in_use: &HashSet<PathBuf>) -> LocalResult<()> {
    let meta = action_target(target)?;
    let kind = EntryKind::of(&meta);
    refuse_in_use(target, kind, in_use)?;
    if kind == EntryKind::Dir {
        // Removes links inside as links; it does not follow them
        std::fs::remove_dir_all(&target.path)?;
    } else {
        std::fs::remove_file(&target.path)?;
    }
    Ok(())
}

/// Check a new name: one segment, not `.`/`..`, no `/`, `\` or NUL, at most 255 bytes
fn check_name(name: &str) -> LocalResult<()> {
    if name.len() > 255 {
        return Err(LocalFileError::Invalid("The name is too long".into()));
    }
    check_segment(name)
}

/// Move `from` to `to` in the same folder unless `to` exists
fn rename_free(from: &Path, to: &Path, name: &str) -> LocalResult<()> {
    if to.symlink_metadata().is_ok() {
        return Err(LocalFileError::Conflict(format!("{name} already exists")));
    }
    std::fs::rename(from, to)?;
    Ok(())
}

/// Rename an entry within its folder; returns the new relative path
pub fn rename(target: &Resolved, name: &str, in_use: &HashSet<PathBuf>) -> LocalResult<String> {
    let meta = action_target(target)?;
    check_name(name)?;
    refuse_in_use(target, EntryKind::of(&meta), in_use)?;
    let to = target.path.with_file_name(name);
    rename_free(&target.path, &to, name)?;
    Ok(target.sibling_rel(name))
}

/// Give a rejected watch file its name back and forget it as processed, so the watcher's
/// next scan checks it again (research R4). Returns the new relative path.
pub async fn requeue(
    target: &Resolved,
    database: Option<&Database>,
    watcher: Option<&TorrentWatcher>,
) -> LocalResult<String> {
    let meta = action_target(target)?;
    let name = target
        .path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let original = rejected_name(name)
        .filter(|o| o.ends_with(".torrent") || o.ends_with(".magnet"))
        .ok_or_else(|| {
            LocalFileError::Invalid(format!("{name} is not a rejected .torrent or .magnet file"))
        })?
        .to_string();
    if !meta.is_file() {
        return Err(LocalFileError::Invalid(format!("{name} is not a file")));
    }
    let to = target.path.with_file_name(&original);
    rename_free(&target.path, &to, &original)?;

    // The same checksum the watcher recorded when it rejected the file
    let checksum = if original.ends_with(".magnet") {
        tokio::fs::read_to_string(&to)
            .await
            .ok()
            .map(|content| calculate_string_checksum(content.trim()))
    } else {
        calculate_file_checksum(&to).await.ok()
    };
    if let (Some(db), Some(checksum)) = (database, checksum) {
        db.processed_torrents()
            .forget(&checksum)
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))?;
    }
    if let Some(watcher) = watcher {
        watcher.forget(&to);
    }
    Ok(target.sibling_rel(&original))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invalid(root: &Path, rel: &str) -> bool {
        matches!(resolve(root, rel), Err(LocalFileError::Invalid(_)))
    }

    #[test]
    fn resolve_refuses_traversal() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        for rel in [
            "..",
            "../x",
            "sub/../..",
            "sub/../../x",
            "./sub",
            "sub//x",
            "/etc",
            "sub\\..\\x",
            "a\0b",
        ] {
            assert!(invalid(dir.path(), rel), "{rel:?}");
        }
    }

    #[test]
    fn resolve_inside() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let root = resolve(dir.path(), "").unwrap();
        assert!(root.is_root());
        let r = resolve(dir.path(), "sub/file.mkv").unwrap();
        assert_eq!(r.rel, "sub/file.mkv");
        assert_eq!(r.path, root.root.join("sub/file.mkv"));
        assert_eq!(resolve(dir.path(), "sub/").unwrap().rel, "sub");
    }

    #[test]
    fn missing_root_or_parent_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("gone");
        assert!(matches!(
            resolve(&gone, ""),
            Err(LocalFileError::NotFound(_))
        ));
        assert!(matches!(
            resolve(dir.path(), "nope/x"),
            Err(LocalFileError::NotFound(_))
        ));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_parent_outside_refused() {
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret"), b"x").unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("escape")).unwrap();
        assert!(invalid(dir.path(), "escape/secret"));
        // The link itself is fine to name: it is not followed
        let link = resolve(dir.path(), "escape").unwrap();
        let listing = list(&link, &HashSet::new());
        assert!(matches!(listing, Err(LocalFileError::Invalid(_))));
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_root_allowed() {
        let real = tempfile::tempdir().unwrap();
        std::fs::create_dir(real.path().join("sonarr")).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("downloads");
        std::os::unix::fs::symlink(real.path(), &root).unwrap();
        let r = resolve(&root, "sonarr").unwrap();
        assert_eq!(list(&r, &HashSet::new()).unwrap().entries.len(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn delete_link_keeps_target() {
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("keep"), b"x").unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("link")).unwrap();
        std::fs::create_dir(dir.path().join("folder")).unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("folder/inner")).unwrap();
        delete(&resolve(dir.path(), "link").unwrap(), &HashSet::new()).unwrap();
        delete(&resolve(dir.path(), "folder").unwrap(), &HashSet::new()).unwrap();
        assert!(outside.path().join("keep").exists());
        assert!(!dir.path().join("link").exists());
        assert!(!dir.path().join("folder").exists());
    }

    #[test]
    fn names() {
        assert_eq!(partial_name(".Ep.mkv.downloading"), Some("Ep.mkv"));
        assert_eq!(partial_name(".downloading"), None);
        assert_eq!(rejected_name("x.torrent.rejected"), Some("x.torrent"));
        assert_eq!(rejected_name(".rejected"), None);
        assert!(check_name("..").is_err());
        assert!(check_name("a/b").is_err());
        assert!(check_name(&"a".repeat(256)).is_err());
        assert!(check_name("Movie (2024).mkv").is_ok());
    }

    #[test]
    fn listing_sorts_and_marks() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("b.mkv"), b"12345").unwrap();
        std::fs::write(dir.path().join(".A.mkv.downloading"), b"1").unwrap();
        std::fs::create_dir(dir.path().join("zfolder")).unwrap();
        let root = resolve(dir.path(), "").unwrap();
        let in_use = canonical_in_use([dir.path().join(".A.mkv.downloading")]);
        let listing = list(&root, &in_use).unwrap();
        let names: Vec<&str> = listing.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["zfolder", ".A.mkv.downloading", "b.mkv"]);
        assert_eq!(listing.entries[1].partial.as_deref(), Some("A.mkv"));
        assert!(listing.entries[1].in_use);
        assert_eq!(listing.entries[2].size, 5);
    }
}
