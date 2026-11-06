//! Where a finished item's files are (feature 038): its folder, category and file list,
//! worked out from the downloads table and the files on disk.

use std::path::{Path, PathBuf};

use putio_sync_db::Database;
use serde::Serialize;

/// One file of an item, as a hook sees it
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ItemFile {
    /// Relative to the item folder; the file name for a single-file item
    pub relative_path: String,
    pub size: u64,
    /// The Put.io file id, when it is still where it was downloaded
    pub putio_id: Option<i64>,
}

/// What `locate` found
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    /// The item folder, or the file for a single-file item; empty when nothing downloaded
    pub dir: PathBuf,
    /// The first folder under the download folder, when the item sits in one
    pub category: Option<String>,
}

/// The local paths of an item's completed files, by its key
pub async fn local_paths(db: &Database, key: &str, retention_hours: u64) -> Vec<(i64, PathBuf)> {
    let mut ids: Vec<i64> = Vec::new();
    if let Some(id) = key
        .strip_prefix("transfer:")
        .and_then(|s| s.parse::<i64>().ok())
    {
        if let Ok(files) = db.pipeline_files(&[id], retention_hours).await {
            ids = files
                .iter()
                .filter(|f| {
                    f.transfer_id == Some(id) && f.state == putio_sync_db::pipeline::FileState::Done
                })
                .map(|f| f.file_id)
                .collect();
        }
    } else if let Some(id) = key
        .strip_prefix("file:")
        .and_then(|s| s.parse::<i64>().ok())
    {
        ids.push(id);
    }
    let mut out = Vec::new();
    for id in ids {
        if let Ok(Some(d)) = db.downloads().find_by_file_id(id).await {
            if d.status == "completed" && !d.local_path.is_empty() {
                out.push((id, PathBuf::from(d.local_path)));
            }
        }
    }
    out
}

/// The folder and category of an item named `name` whose files are at `paths`, under
/// `target`: the path through the component named like the item, else the single file,
/// else the files' common folder
pub fn derive(name: &str, paths: &[PathBuf], target: &Path) -> Location {
    let Some(first) = paths.first() else {
        return Location {
            dir: PathBuf::new(),
            category: None,
        };
    };
    let dir = {
        let mut found = None;
        let mut acc = PathBuf::new();
        for component in first.components() {
            acc.push(component);
            if component.as_os_str() == name {
                found = Some(acc.clone());
            }
        }
        match found {
            Some(d) if paths.iter().all(|p| p.starts_with(&d)) => d,
            _ if paths.len() == 1 => first.clone(),
            _ => common_parent(paths),
        }
    };
    // A first folder under the download folder is the category, if something sits below it
    let category = dir.strip_prefix(target).ok().and_then(|rel| {
        let mut parts = rel.components();
        let first = parts.next()?;
        parts
            .next()
            .map(|_| first.as_os_str().to_string_lossy().into_owned())
    });
    Location { dir, category }
}

fn common_parent(paths: &[PathBuf]) -> PathBuf {
    let mut common: PathBuf = paths[0].parent().map(Path::to_path_buf).unwrap_or_default();
    for p in &paths[1..] {
        while !p.starts_with(&common) {
            if !common.pop() {
                return PathBuf::new();
            }
        }
    }
    common
}

/// The files under `dir` (or `dir` itself when it is a file), with sizes. `ids` maps a
/// local path to its Put.io id.
pub fn list_files(dir: &Path, ids: &[(i64, PathBuf)]) -> Vec<ItemFile> {
    let mut out = Vec::new();
    let Ok(meta) = std::fs::metadata(dir) else {
        return out;
    };
    let id_of = |p: &Path| ids.iter().find(|(_, path)| path == p).map(|(id, _)| *id);
    if meta.is_file() {
        out.push(ItemFile {
            relative_path: dir
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            size: meta.len(),
            putio_id: id_of(dir),
        });
        return out;
    }
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            match entry.metadata() {
                Ok(m) if m.is_dir() => stack.push(path),
                Ok(m) => out.push(ItemFile {
                    relative_path: path
                        .strip_prefix(dir)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .into_owned(),
                    size: m.len(),
                    putio_id: id_of(&path),
                }),
                Err(_) => {}
            }
        }
    }
    out.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_folder_item_is_the_component_named_like_it() {
        let target = Path::new("/data");
        let paths = vec![
            PathBuf::from("/data/sonarr/Show.S01E01/Show.S01E01.mkv"),
            PathBuf::from("/data/sonarr/Show.S01E01/Subs/en.srt"),
        ];
        let loc = derive("Show.S01E01", &paths, target);
        assert_eq!(loc.dir, PathBuf::from("/data/sonarr/Show.S01E01"));
        assert_eq!(loc.category.as_deref(), Some("sonarr"));
    }

    #[test]
    fn a_single_file_item_is_the_file_and_has_no_category_at_the_top() {
        let target = Path::new("/data");
        let loc = derive("movie.mkv", &[PathBuf::from("/data/movie.mkv")], target);
        assert_eq!(loc.dir, PathBuf::from("/data/movie.mkv"));
        assert_eq!(loc.category, None);
        let loc = derive("x", &[PathBuf::from("/data/radarr/movie.mkv")], target);
        assert_eq!(loc.dir, PathBuf::from("/data/radarr/movie.mkv"));
        assert_eq!(loc.category.as_deref(), Some("radarr"));
    }

    #[test]
    fn several_files_without_a_named_folder_use_their_common_folder() {
        let target = Path::new("/data");
        let paths = vec![
            PathBuf::from("/data/a/x/1.mkv"),
            PathBuf::from("/data/a/y/2.mkv"),
        ];
        assert_eq!(derive("zzz", &paths, target).dir, PathBuf::from("/data/a"));
        assert_eq!(derive("zzz", &[], target).dir, PathBuf::new());
    }

    #[test]
    fn files_are_listed_relative_to_the_folder_with_sizes() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("Subs")).unwrap();
        std::fs::write(tmp.path().join("a.mkv"), b"12345").unwrap();
        std::fs::write(tmp.path().join("Subs/en.srt"), b"1").unwrap();
        let ids = vec![(7, tmp.path().join("a.mkv"))];
        let files = list_files(tmp.path(), &ids);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].relative_path, "Subs/en.srt");
        assert_eq!(files[1].relative_path, "a.mkv");
        assert_eq!((files[1].size, files[1].putio_id), (5, Some(7)));
        let single = list_files(&tmp.path().join("a.mkv"), &ids);
        assert_eq!(single[0].relative_path, "a.mkv");
    }
}
