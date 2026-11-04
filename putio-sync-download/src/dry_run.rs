//! What a first sync would do (feature 013): walk the Put.io account read-only, apply
//! the folder rules the sync applies, and see which files are already in the download
//! folder. Nothing is downloaded, deleted, queued or written.

use crate::error::{DownloadError, DownloadResult};
use crate::manager::DownloadConfig;
use putio_sync_client::PutioApiClient;
use putio_sync_core::rules::{sanitize_name, Decision, Effective};
use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

/// How long a dry run may take before it reports what it has
pub const DRY_RUN_LIMIT: Duration = Duration::from_secs(60);

/// How many files a dry run names
pub const DRY_RUN_SAMPLES: usize = 20;

/// A number of files and their size
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Tally {
    /// Files
    pub count: u64,
    /// Bytes
    pub bytes: u64,
}

impl Tally {
    fn add(&mut self, bytes: u64) {
        self.count += 1;
        self.bytes += bytes;
    }
}

/// One file the sync would download
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DryRunItem {
    /// Path relative to the Put.io root
    pub path: String,
    /// Bytes
    pub size: u64,
}

/// What a dry run found
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DryRunReport {
    /// Files listed
    pub files_seen: u64,
    /// Would be downloaded
    pub to_download: Tally,
    /// Already in the download folder with the same size
    pub already_present: Tally,
    /// Kept out by a preset, folder rule or blocked extension
    pub skipped: Tally,
    /// The largest files that would be downloaded, largest first
    pub samples: Vec<DryRunItem>,
    /// The time limit was reached; the counts cover what was listed
    pub partial: bool,
    /// How long it took
    pub elapsed_ms: u64,
}

/// Walk the whole account under `config` and report, stopping after `limit`
pub async fn dry_run(
    client: &PutioApiClient,
    config: &DownloadConfig,
    limit: Duration,
) -> DownloadResult<DryRunReport> {
    let started = Instant::now();
    let deadline = started + limit;
    let mut report = DryRunReport::default();
    let mut samples: Vec<DryRunItem> = Vec::new();
    let mut folders: HashMap<String, Effective> = HashMap::new();
    let mut pending = vec![(0_i64, String::new())];

    while let Some((folder_id, folder_path)) = pending.pop() {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            report.partial = true;
            break;
        }
        let listing = match tokio::time::timeout(left, client.list_files(folder_id)).await {
            Ok(listing) => listing.map_err(DownloadError::ClientError)?,
            Err(_) => {
                report.partial = true;
                break;
            }
        };
        for file in listing.files {
            let name = sanitize_name(&file.name);
            if file.file_type == "FOLDER" {
                let path = if folder_path.is_empty() {
                    name
                } else {
                    format!("{folder_path}/{name}")
                };
                pending.push((file.id, path));
                continue;
            }
            report.files_seen += 1;
            let size = u64::try_from(file.size).unwrap_or(0);
            let rules = folders
                .entry(folder_path.clone())
                .or_insert_with(|| config.effective(&folder_path));
            if rules.decide(&file.name) != Decision::Download {
                report.skipped.add(size);
                continue;
            }
            let local = config.target_directory.join(&folder_path).join(&name);
            if same_size(&local, size) {
                report.already_present.add(size);
                continue;
            }
            report.to_download.add(size);
            let path = if folder_path.is_empty() {
                file.name
            } else {
                format!("{folder_path}/{}", file.name)
            };
            samples.push(DryRunItem { path, size });
            if samples.len() > DRY_RUN_SAMPLES * 4 {
                keep_largest(&mut samples);
            }
        }
    }
    keep_largest(&mut samples);
    report.samples = samples;
    report.elapsed_ms = started.elapsed().as_millis() as u64;
    Ok(report)
}

fn keep_largest(samples: &mut Vec<DryRunItem>) {
    samples.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.path.cmp(&b.path)));
    samples.truncate(DRY_RUN_SAMPLES);
}

fn same_size(path: &Path, size: u64) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.len() == size)
}
