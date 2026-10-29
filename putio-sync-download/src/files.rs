//! File utilities for download management

use putio_sync_core::config::ExistingFile;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

// Error type will need to be updated to use the new crate's error type
use crate::error::{DownloadError, DownloadResult};

/// Create a temporary download path with .downloading extension and . prefix
///
/// # Arguments
///
/// * `target_path` - The final destination path
///
/// # Returns
///
/// A path like `/path/to/.filename.ext.downloading`
pub fn create_temp_path(target_path: &Path) -> DownloadResult<PathBuf> {
    let parent = target_path.parent();
    let filename = target_path
        .file_name()
        .ok_or_else(|| DownloadError::InvalidInput("Invalid file path".to_string()))?
        .to_str()
        .ok_or_else(|| DownloadError::InvalidInput("Invalid filename encoding".to_string()))?;

    let temp_filename = format!(".{}.downloading", filename);

    let temp_path = if let Some(parent) = parent {
        parent.join(temp_filename)
    } else {
        PathBuf::from(temp_filename)
    };

    Ok(temp_path)
}

/// Check if a file is a partial download (has .downloading extension)
pub fn is_partial_download(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext == "downloading")
        .unwrap_or(false)
}

/// Get the original path from a partial download path
pub fn get_original_path(partial_path: &Path) -> Option<PathBuf> {
    if !is_partial_download(partial_path) {
        return None;
    }

    let parent = partial_path.parent();
    let stem = partial_path.file_stem()?.to_str()?;

    // Remove leading dot
    let original_name = stem.strip_prefix('.')?;

    let original_path = if let Some(parent) = parent {
        parent.join(original_name)
    } else {
        PathBuf::from(original_name)
    };

    Some(original_path)
}

/// Check if a download can be resumed
///
/// Returns the number of bytes already downloaded if resume is possible
pub async fn check_resume(temp_path: &Path, expected_size: u64) -> DownloadResult<Option<u64>> {
    if temp_path.exists() && is_partial_download(temp_path) {
        // Get the current size of the partial file
        match tokio::fs::metadata(temp_path).await {
            Ok(metadata) => {
                let partial_size = metadata.len();
                if partial_size > 0 && partial_size < expected_size {
                    Ok(Some(partial_size))
                } else if partial_size >= expected_size {
                    // File appears to be complete, clean it up and start fresh
                    let _ = tokio::fs::remove_file(temp_path).await;
                    Ok(None)
                } else {
                    // Empty partial file, clean it up
                    let _ = tokio::fs::remove_file(temp_path).await;
                    Ok(None)
                }
            }
            Err(_) => {
                // Can't read metadata, clean up and start fresh
                let _ = tokio::fs::remove_file(temp_path).await;
                Ok(None)
            }
        }
    } else {
        Ok(None)
    }
}

/// What is at a file's target path before it is downloaded (feature 040)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalState {
    /// Nothing
    Missing,
    /// A folder
    Dir,
    /// A file of this many bytes
    File {
        /// Its size
        size: u64,
    },
}

impl LocalState {
    /// Look at `path` without following a symlink at the end
    pub async fn of(path: &Path) -> Self {
        match tokio::fs::symlink_metadata(path).await {
            Ok(meta) if meta.is_dir() => LocalState::Dir,
            Ok(_) => match tokio::fs::metadata(path).await {
                Ok(meta) if meta.is_dir() => LocalState::Dir,
                Ok(meta) => LocalState::File { size: meta.len() },
                // A dangling symlink: replaced like any file
                Err(_) => LocalState::File { size: 0 },
            },
            Err(_) => LocalState::Missing,
        }
    }
}

/// Why a download is saved under a numbered name
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveAsReason {
    /// "keep both" was chosen
    KeepBoth,
    /// The local file is larger than the one on Put.io
    Larger,
    /// Same size, but the checksum differs
    Checksum,
}

impl SaveAsReason {
    /// The logged name
    pub fn as_str(self) -> &'static str {
        match self {
            SaveAsReason::KeepBoth => "keep_both",
            SaveAsReason::Larger => "larger",
            SaveAsReason::Checksum => "checksum",
        }
    }
}

/// What to do with a file whose target path may already be taken (research R2)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExistingAction {
    /// Nothing there: a normal download
    Fresh,
    /// The local file is complete
    Skip,
    /// Continue the local file from this byte
    Resume {
        /// Bytes already there
        from: u64,
    },
    /// Download again and replace the local file once complete
    Overwrite {
        /// Overwritten because the file must keep its path (Radarr/Sonarr)
        fixed_path: bool,
    },
    /// Keep the local file; save the new one under a numbered name
    SaveAs {
        /// Why
        reason: SaveAsReason,
    },
    /// A folder is in the way
    Refuse,
}

/// True when [`plan_existing`] needs the local file's CRC32 to decide: a same-size
/// file, Put.io has a checksum, and the choice is not overwrite
pub fn needs_checksum(
    local: LocalState,
    remote_size: u64,
    remote_crc: Option<&str>,
    choice: ExistingFile,
) -> bool {
    matches!(local, LocalState::File { size } if size == remote_size)
        && remote_crc.is_some()
        && choice != ExistingFile::Overwrite
}

/// Decide what to do with an existing local file (research R2). `crc_match` is the
/// result of comparing the local file's CRC32 with Put.io's, or `None` when there was
/// nothing to compare. With `fixed_path` a numbered name is never used: it overwrites.
pub fn plan_existing(
    local: LocalState,
    remote_size: u64,
    crc_match: Option<bool>,
    choice: ExistingFile,
    resume_enabled: bool,
    fixed_path: bool,
) -> ExistingAction {
    use ExistingAction::*;
    let size = match local {
        LocalState::Missing => return Fresh,
        LocalState::Dir => return Refuse,
        LocalState::File { size } => size,
    };
    let overwrite = Overwrite { fixed_path: false };
    let action = match choice {
        ExistingFile::Overwrite => overwrite,
        ExistingFile::Resume if size < remote_size => {
            if resume_enabled && size > 0 {
                Resume { from: size }
            } else {
                overwrite
            }
        }
        ExistingFile::Resume if size > remote_size => SaveAs {
            reason: SaveAsReason::Larger,
        },
        ExistingFile::KeepBoth if size != remote_size => SaveAs {
            reason: SaveAsReason::KeepBoth,
        },
        ExistingFile::Resume | ExistingFile::KeepBoth => match crc_match {
            Some(false) if choice == ExistingFile::Resume => SaveAs {
                reason: SaveAsReason::Checksum,
            },
            Some(false) => SaveAs {
                reason: SaveAsReason::KeepBoth,
            },
            _ => Skip,
        },
    };
    match action {
        SaveAs { .. } if fixed_path => Overwrite { fixed_path: true },
        action => action,
    }
}

/// `name` with ` (n)` before its extension: `Movie.mkv` → `Movie (1).mkv`. A name
/// whose only dot leads it (`.hidden`) has no extension.
pub fn numbered_name(name: &str, n: u32) -> String {
    match name.rfind('.') {
        Some(dot) if dot > 0 => format!("{} ({n}){}", &name[..dot], &name[dot..]),
        _ => format!("{name} ({n})"),
    }
}

/// The first `numbered_name(name, n)` in `dir`, from 1, with no file, no
/// `.downloading` file and no entry in `reserved` (paths other workers write to)
pub fn next_free_name(dir: &Path, name: &str, reserved: &HashSet<PathBuf>) -> String {
    (1..)
        .map(|n| numbered_name(name, n))
        .find(|candidate| {
            let path = dir.join(candidate);
            let temp_taken = create_temp_path(&path)
                .map(|t| t.symlink_metadata().is_ok())
                .unwrap_or(false);
            path.symlink_metadata().is_err() && !temp_taken && !reserved.contains(&path)
        })
        .expect("an unbounded range always finds a free name")
}

/// The CRC32 (ISO-HDLC, as Put.io reports it) of a file as 8 lowercase hex digits.
/// Reads in 1 MiB blocks on a blocking thread; `on_progress` gets the bytes read so far.
pub async fn crc32_file(
    path: &Path,
    on_progress: impl Fn(u64) + Send + 'static,
) -> std::io::Result<String> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        use std::io::Read;
        static CRC: crc::Crc<u32, crc::Table<16>> =
            crc::Crc::<u32, crc::Table<16>>::new(&crc::CRC_32_ISO_HDLC);
        let mut file = std::fs::File::open(&path)?;
        let mut digest = CRC.digest();
        let mut buf = vec![0u8; 1024 * 1024];
        let mut read = 0u64;
        loop {
            let n = file.read(&mut buf)?;
            if n == 0 {
                break;
            }
            digest.update(&buf[..n]);
            read += n as u64;
            on_progress(read);
        }
        Ok(format!("{:08x}", digest.finalize()))
    })
    .await
    .map_err(std::io::Error::other)?
}

/// True when two CRC32 strings name the same value (Put.io's case may differ)
pub fn crc_equal(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// Clean up a partial download file
pub async fn cleanup_partial(path: &Path) {
    use tracing::warn;

    if path.exists() {
        if let Err(e) = tokio::fs::remove_file(path).await {
            warn!(
                "Failed to clean up partial download {}: {}",
                path.display(),
                e
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_temp_path() {
        let target = PathBuf::from("/downloads/file.txt");
        let temp = create_temp_path(&target).unwrap();
        assert_eq!(temp, PathBuf::from("/downloads/.file.txt.downloading"));
    }

    #[test]
    fn test_create_temp_path_no_parent() {
        let target = PathBuf::from("file.txt");
        let temp = create_temp_path(&target).unwrap();
        assert_eq!(temp, PathBuf::from(".file.txt.downloading"));
    }

    #[test]
    fn test_is_partial_download() {
        assert!(is_partial_download(&PathBuf::from(".file.txt.downloading")));
        assert!(is_partial_download(&PathBuf::from(
            "/path/.file.txt.downloading"
        )));
        assert!(!is_partial_download(&PathBuf::from("file.txt")));
        assert!(!is_partial_download(&PathBuf::from(".file.txt")));
    }

    #[test]
    fn test_get_original_path() {
        let partial = PathBuf::from("/downloads/.file.txt.downloading");
        let original = get_original_path(&partial).unwrap();
        assert_eq!(original, PathBuf::from("/downloads/file.txt"));
    }

    #[test]
    fn test_get_original_path_no_parent() {
        let partial = PathBuf::from(".file.txt.downloading");
        let original = get_original_path(&partial).unwrap();
        assert_eq!(original, PathBuf::from("file.txt"));
    }

    use ExistingAction::*;
    use ExistingFile::{KeepBoth, Overwrite as Over, Resume as Res};

    fn file(size: u64) -> LocalState {
        LocalState::File { size }
    }

    fn plan(local: LocalState, crc: Option<bool>, choice: ExistingFile) -> ExistingAction {
        plan_existing(local, 100, crc, choice, true, false)
    }

    /// Every cell of research R2
    #[test]
    fn plan_table() {
        let over = Overwrite { fixed_path: false };
        let save = |reason| SaveAs { reason };
        for choice in [Res, Over, KeepBoth] {
            assert_eq!(plan(LocalState::Missing, None, choice), Fresh);
            assert_eq!(plan(LocalState::Dir, None, choice), Refuse);
        }
        // smaller
        assert_eq!(plan(file(40), None, Res), Resume { from: 40 });
        assert_eq!(plan(file(40), None, Over), over);
        assert_eq!(plan(file(40), None, KeepBoth), save(SaveAsReason::KeepBoth));
        // same size, checksum matches or none to compare
        for crc in [Some(true), None] {
            assert_eq!(plan(file(100), crc, Res), Skip);
            assert_eq!(plan(file(100), crc, Over), over);
            assert_eq!(plan(file(100), crc, KeepBoth), Skip);
        }
        // same size, checksum differs
        assert_eq!(
            plan(file(100), Some(false), Res),
            save(SaveAsReason::Checksum)
        );
        assert_eq!(plan(file(100), Some(false), Over), over);
        assert_eq!(
            plan(file(100), Some(false), KeepBoth),
            save(SaveAsReason::KeepBoth)
        );
        // larger
        assert_eq!(plan(file(140), None, Res), save(SaveAsReason::Larger));
        assert_eq!(plan(file(140), None, Over), over);
        assert_eq!(
            plan(file(140), None, KeepBoth),
            save(SaveAsReason::KeepBoth)
        );
    }

    #[test]
    fn fixed_path_never_saves_as() {
        let fixed = Overwrite { fixed_path: true };
        let plan = |local, crc, choice| plan_existing(local, 100, crc, choice, true, true);
        assert_eq!(plan(file(140), None, Res), fixed);
        assert_eq!(plan(file(100), Some(false), Res), fixed);
        assert_eq!(plan(file(40), None, KeepBoth), fixed);
        assert_eq!(plan(file(100), Some(true), KeepBoth), Skip);
        assert_eq!(plan(file(40), None, Res), Resume { from: 40 });
    }

    #[test]
    fn resume_off_or_empty_file_overwrites() {
        let over = Overwrite { fixed_path: false };
        assert_eq!(plan_existing(file(40), 100, None, Res, false, false), over);
        assert_eq!(plan_existing(file(0), 100, None, Res, true, false), over);
    }

    #[test]
    fn checksum_only_for_same_size_with_a_remote_crc() {
        assert!(needs_checksum(file(100), 100, Some("cbf43926"), Res));
        assert!(needs_checksum(file(100), 100, Some("cbf43926"), KeepBoth));
        assert!(!needs_checksum(file(100), 100, Some("cbf43926"), Over));
        assert!(!needs_checksum(file(100), 100, None, Res));
        assert!(!needs_checksum(file(99), 100, Some("cbf43926"), Res));
        assert!(!needs_checksum(LocalState::Missing, 100, Some("x"), Res));
    }

    #[test]
    fn numbered_names() {
        assert_eq!(
            numbered_name("Show.S01E01.1080p.mkv", 1),
            "Show.S01E01.1080p (1).mkv"
        );
        assert_eq!(numbered_name("README", 1), "README (1)");
        assert_eq!(numbered_name(".hidden", 2), ".hidden (2)");
        assert_eq!(numbered_name("archive.tar.gz", 3), "archive.tar (3).gz");
    }

    #[test]
    fn next_free_name_skips_files_partials_and_reserved() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        std::fs::write(d.join("Movie (1).mkv"), b"x").unwrap();
        std::fs::write(d.join(".Movie (2).mkv.downloading"), b"x").unwrap();
        let reserved = HashSet::from([d.join("Movie (3).mkv")]);
        assert_eq!(next_free_name(d, "Movie.mkv", &reserved), "Movie (4).mkv");
        assert_eq!(next_free_name(d, "Other.mkv", &reserved), "Other (1).mkv");
    }

    #[tokio::test]
    async fn crc32_of_a_known_value() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("check");
        std::fs::write(&path, b"123456789").unwrap();
        let read = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let seen = read.clone();
        let crc = crc32_file(&path, move |n| {
            seen.store(n, std::sync::atomic::Ordering::Relaxed)
        })
        .await
        .unwrap();
        assert_eq!(crc, "cbf43926");
        assert_eq!(read.load(std::sync::atomic::Ordering::Relaxed), 9);
        assert!(crc_equal(&crc, "CBF43926"));
    }

    #[tokio::test]
    async fn local_state_of_paths() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        assert_eq!(LocalState::of(&path).await, LocalState::Missing);
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(LocalState::of(&path).await, file(3));
        assert_eq!(LocalState::of(dir.path()).await, LocalState::Dir);
    }

    #[test]
    fn test_get_original_path_not_partial() {
        let path = PathBuf::from("/downloads/file.txt");
        assert!(get_original_path(&path).is_none());
    }
}
