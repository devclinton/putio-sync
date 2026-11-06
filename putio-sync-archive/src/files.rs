//! The archive folder: atomic writes, `manifest.jsonl`, leftover cleanup and pruning

use crate::error::{ArchiveError, ArchiveResult};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use tracing::{debug, warn};

/// The manifest's file name
pub const MANIFEST: &str = "manifest.jsonl";

/// One line of `manifest.jsonl` (data-model.md)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum ManifestEntry {
    /// A metrics day
    Metrics {
        day: NaiveDate,
        file: String,
        saved_at: String,
        points: u64,
        expected: u64,
        partial: bool,
    },
    /// A database copy
    Db {
        day: NaiveDate,
        file: String,
        saved_at: String,
        rows: i64,
    },
}

impl ManifestEntry {
    fn file(&self) -> &str {
        match self {
            Self::Metrics { file, .. } | Self::Db { file, .. } => file,
        }
    }
}

/// Archive file families that `--keep-days` may delete: folder, name prefix, name suffix
const PRUNABLE: [(&str, &str, &str); 2] = [
    ("metrics", "putio-sync-metrics-", ".om.gz"),
    ("db", "putio-sync-db-", ".sqlite.gz"),
];

/// Folders that hold files written through temp names
const SUBDIRS: [&str; 3] = ["metrics", "db", "summary"];

/// An opened archive folder
pub struct ArchiveDir {
    root: PathBuf,
}

impl ArchiveDir {
    /// Open the folder, creating it when its parent exists, and delete `.tmp` leftovers
    /// of a crashed run. With `require_manifest`, a folder without `manifest.jsonl` is an
    /// error and nothing is created: it is probably an unmounted share.
    pub fn open(root: &Path, require_manifest: bool) -> ArchiveResult<Self> {
        if require_manifest && !root.join(MANIFEST).is_file() {
            return Err(ArchiveError::folder(
                &root.display().to_string(),
                format!("{MANIFEST} not found (is the share mounted?)"),
            ));
        }
        match std::fs::metadata(root) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => {
                return Err(ArchiveError::folder(
                    &root.display().to_string(),
                    "not a folder",
                ))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(root).map_err(|e| {
                    ArchiveError::folder(&format!("cannot create {}", root.display()), e)
                })?;
            }
            Err(e) => return Err(ArchiveError::folder(&root.display().to_string(), e)),
        }
        let dir = Self {
            root: root.to_path_buf(),
        };
        dir.sweep()?;
        Ok(dir)
    }

    /// The folder
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// A subfolder (`metrics`, `db`, `summary`), created when missing
    pub fn subdir(&self, name: &str) -> ArchiveResult<PathBuf> {
        let path = self.root.join(name);
        std::fs::create_dir_all(&path)
            .map_err(|e| ArchiveError::folder(&format!("cannot create {}", path.display()), e))?;
        Ok(path)
    }

    /// Delete the `.tmp` files and `.tmp-*` copies a crashed run left behind
    fn sweep(&self) -> ArchiveResult<()> {
        let folders = std::iter::once(self.root.clone())
            .chain(SUBDIRS.iter().map(|name| self.root.join(name)));
        for folder in folders {
            let Ok(entries) = std::fs::read_dir(&folder) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if (name.ends_with(".tmp") || name.starts_with(".tmp-"))
                    && entry.file_type().is_ok_and(|t| t.is_file())
                {
                    debug!("Removing leftover {}", entry.path().display());
                    std::fs::remove_file(entry.path()).map_err(|e| {
                        ArchiveError::folder(
                            &format!("cannot remove {}", entry.path().display()),
                            e,
                        )
                    })?;
                }
            }
        }
        Ok(())
    }

    /// Add a line to the manifest
    pub fn append_manifest(&self, entry: &ManifestEntry) -> ArchiveResult<()> {
        let path = self.root.join(MANIFEST);
        let line = serde_json::to_string(entry)
            .map_err(|e| ArchiveError::folder("cannot write the manifest", e))?;
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| ArchiveError::folder(&format!("cannot open {}", path.display()), e))?;
        writeln!(file, "{line}")
            .and_then(|()| file.sync_all())
            .map_err(|e| ArchiveError::folder(&format!("cannot write {}", path.display()), e))
    }

    /// Every line of the manifest; lines that do not parse are skipped
    pub fn read_manifest(&self) -> ArchiveResult<Vec<ManifestEntry>> {
        let path = self.root.join(MANIFEST);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(ArchiveError::folder(
                    &format!("cannot read {}", path.display()),
                    e,
                ))
            }
        };
        Ok(text
            .lines()
            .filter_map(|line| match serde_json::from_str(line) {
                Ok(entry) => Some(entry),
                Err(e) => {
                    warn!("Skipping a manifest line that does not parse: {e}");
                    None
                }
            })
            .collect())
    }

    /// Delete archive files dated before `keep_days` days before `today`, with their
    /// manifest lines, and return how many files went. Only
    /// `metrics/putio-sync-metrics-<day>.om.gz` and `db/putio-sync-db-<day>.sqlite.gz`
    /// qualify; every other file is left alone.
    pub fn prune(&self, keep_days: u32, today: NaiveDate) -> ArchiveResult<usize> {
        let cutoff = today - chrono::Duration::days(i64::from(keep_days));
        let mut removed: Vec<String> = Vec::new();
        for (folder, prefix, suffix) in PRUNABLE {
            let Ok(entries) = std::fs::read_dir(self.root.join(folder)) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let Some(day) = name
                    .strip_prefix(prefix)
                    .and_then(|rest| rest.strip_suffix(suffix))
                    .and_then(|day| day.parse::<NaiveDate>().ok())
                else {
                    continue;
                };
                if day < cutoff {
                    std::fs::remove_file(entry.path()).map_err(|e| {
                        ArchiveError::folder(
                            &format!("cannot remove {}", entry.path().display()),
                            e,
                        )
                    })?;
                    removed.push(format!("{folder}/{name}"));
                }
            }
        }
        if !removed.is_empty() {
            let keep: Vec<String> = self
                .read_manifest()?
                .iter()
                .filter(|entry| !removed.iter().any(|file| file == entry.file()))
                .filter_map(|entry| serde_json::to_string(entry).ok())
                .collect();
            write_atomic(&self.root.join(MANIFEST), |file| {
                keep.iter().try_for_each(|line| writeln!(file, "{line}"))
            })?;
        }
        Ok(removed.len())
    }
}

/// Write `path` through `<path>.tmp` in the same folder: fsync, then rename. A failed
/// write leaves the old file and no `.tmp`.
pub fn write_atomic(
    path: &Path,
    write: impl FnOnce(&mut File) -> std::io::Result<()>,
) -> ArchiveResult<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let result = File::create(&tmp).and_then(|mut file| {
        write(&mut file)?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)
    });
    result.map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        ArchiveError::folder(&format!("cannot write {}", path.display()), e)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn day(text: &str) -> NaiveDate {
        text.parse().unwrap()
    }

    fn metrics_entry(date: &str) -> ManifestEntry {
        ManifestEntry::Metrics {
            day: day(date),
            file: format!("metrics/putio-sync-metrics-{date}.om.gz"),
            saved_at: "2026-11-02T03:30:00Z".to_string(),
            points: 288,
            expected: 288,
            partial: false,
        }
    }

    #[test]
    fn write_atomic_leaves_only_the_final_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a.txt");
        write_atomic(&path, |f| f.write_all(b"one")).unwrap();
        write_atomic(&path, |f| f.write_all(b"two")).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "two");
        let names: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
        assert_eq!(names.len(), 1);
    }

    #[test]
    fn a_failed_write_leaves_the_old_file_and_no_tmp() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a.txt");
        write_atomic(&path, |f| f.write_all(b"old")).unwrap();
        let err = write_atomic(&path, |_| Err(std::io::Error::other("disk full"))).unwrap_err();
        assert_eq!(err.exit_code(), 4);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "old");
        assert!(!dir.path().join("a.txt.tmp").exists());
    }

    #[test]
    fn open_creates_the_folder_when_the_parent_exists() {
        let dir = tempdir().unwrap();
        let root = dir.path().join("archive");
        ArchiveDir::open(&root, false).unwrap();
        assert!(root.is_dir());
    }

    #[test]
    fn open_fails_when_the_parent_is_missing() {
        let dir = tempdir().unwrap();
        let err = ArchiveDir::open(&dir.path().join("no/such/archive"), false)
            .err()
            .unwrap();
        assert_eq!(err.exit_code(), 4);
    }

    #[test]
    fn require_manifest_guards_an_empty_mount() {
        let dir = tempdir().unwrap();
        let err = ArchiveDir::open(dir.path(), true).err().unwrap();
        assert_eq!(err.exit_code(), 4);
        assert!(err.to_string().contains("manifest.jsonl"));

        std::fs::write(dir.path().join(MANIFEST), "").unwrap();
        ArchiveDir::open(dir.path(), true).unwrap();
    }

    #[test]
    fn open_sweeps_tmp_leftovers_only() {
        let dir = tempdir().unwrap();
        std::fs::create_dir(dir.path().join("metrics")).unwrap();
        std::fs::create_dir(dir.path().join("db")).unwrap();
        for leftover in [
            "manifest.jsonl.tmp",
            "metrics/putio-sync-metrics-2026-11-01.om.gz.tmp",
            "db/.tmp-1234.sqlite",
        ] {
            std::fs::write(dir.path().join(leftover), "x").unwrap();
        }
        std::fs::write(dir.path().join("notes.txt"), "mine").unwrap();
        ArchiveDir::open(dir.path(), false).unwrap();
        assert!(dir.path().join("notes.txt").exists());
        for gone in [
            "manifest.jsonl.tmp",
            "metrics/putio-sync-metrics-2026-11-01.om.gz.tmp",
            "db/.tmp-1234.sqlite",
        ] {
            assert!(!dir.path().join(gone).exists(), "{gone} should be gone");
        }
    }

    #[test]
    fn manifest_lines_have_the_documented_shape_and_round_trip() {
        let dir = tempdir().unwrap();
        let archive = ArchiveDir::open(dir.path(), false).unwrap();
        assert!(archive.read_manifest().unwrap().is_empty());

        let metrics = metrics_entry("2026-11-01");
        let db = ManifestEntry::Db {
            day: day("2026-11-02"),
            file: "db/putio-sync-db-2026-11-02.sqlite.gz".to_string(),
            saved_at: "2026-11-02T03:30:00Z".to_string(),
            rows: 17,
        };
        archive.append_manifest(&metrics).unwrap();
        archive.append_manifest(&db).unwrap();

        let text = std::fs::read_to_string(dir.path().join(MANIFEST)).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines[0],
            r#"{"kind":"metrics","day":"2026-11-01","file":"metrics/putio-sync-metrics-2026-11-01.om.gz","saved_at":"2026-11-02T03:30:00Z","points":288,"expected":288,"partial":false}"#
        );
        assert_eq!(
            lines[1],
            r#"{"kind":"db","day":"2026-11-02","file":"db/putio-sync-db-2026-11-02.sqlite.gz","saved_at":"2026-11-02T03:30:00Z","rows":17}"#
        );
        assert_eq!(archive.read_manifest().unwrap(), vec![metrics, db]);
    }

    #[test]
    fn prune_deletes_only_old_archive_files_and_their_manifest_lines() {
        let dir = tempdir().unwrap();
        let archive = ArchiveDir::open(dir.path(), false).unwrap();
        let metrics = archive.subdir("metrics").unwrap();
        let db = archive.subdir("db").unwrap();
        let old = "putio-sync-metrics-2026-01-01.om.gz";
        let new = "putio-sync-metrics-2026-10-30.om.gz";
        let old_db = "putio-sync-db-2026-01-05.sqlite.gz";
        for (folder, name) in [
            (&metrics, old),
            (&metrics, new),
            (&metrics, "readme-2020-01-01.txt"),
            (&metrics, "putio-sync-metrics-2020-01-01.om"),
            (&db, old_db),
        ] {
            std::fs::write(folder.join(name), "x").unwrap();
        }
        std::fs::write(dir.path().join("putio-sync-metrics-2020-01-01.om.gz"), "x").unwrap();
        archive
            .append_manifest(&metrics_entry("2026-01-01"))
            .unwrap();
        archive
            .append_manifest(&metrics_entry("2026-10-30"))
            .unwrap();
        archive
            .append_manifest(&ManifestEntry::Db {
                day: day("2026-01-05"),
                file: format!("db/{old_db}"),
                saved_at: "x".to_string(),
                rows: 1,
            })
            .unwrap();

        let removed = archive.prune(30, day("2026-11-02")).unwrap();

        assert_eq!(removed, 2);
        assert!(!metrics.join(old).exists());
        assert!(!db.join(old_db).exists());
        assert!(metrics.join(new).exists());
        assert!(metrics.join("readme-2020-01-01.txt").exists());
        assert!(metrics.join("putio-sync-metrics-2020-01-01.om").exists());
        assert!(dir
            .path()
            .join("putio-sync-metrics-2020-01-01.om.gz")
            .exists());
        assert_eq!(
            archive.read_manifest().unwrap(),
            vec![metrics_entry("2026-10-30")]
        );
    }

    #[test]
    fn prune_keeps_the_day_exactly_n_days_old() {
        let dir = tempdir().unwrap();
        let archive = ArchiveDir::open(dir.path(), false).unwrap();
        let metrics = archive.subdir("metrics").unwrap();
        std::fs::write(metrics.join("putio-sync-metrics-2026-10-03.om.gz"), "x").unwrap();
        assert_eq!(archive.prune(30, day("2026-11-02")).unwrap(), 0);
        assert_eq!(archive.prune(29, day("2026-11-02")).unwrap(), 1);
    }
}
