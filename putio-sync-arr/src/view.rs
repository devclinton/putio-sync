//! An item as a Transmission torrent (data-model "Transmission view"): a pure mapping
//! from what is stored plus the item's local progress

use std::path::Path;

use putio_sync_db::{ArrItem, ArrState};
use serde_json::{json, Value};

use crate::service::HELD;

/// Transmission status codes the *arr apps read
const STATUS_STOPPED: i64 = 0;
const STATUS_QUEUED: i64 = 3;
const STATUS_DOWNLOADING: i64 = 4;

/// Transmission's "local error" code; the *arr shows `errorString` and fails the item
const ERROR_LOCAL: i64 = 3;

/// What the view needs besides the stored item
#[derive(Debug, Clone, Copy, Default)]
pub struct LocalProgress {
    /// Bytes of the item's accepted files that are complete on disk
    pub completed_bytes: i64,
    /// Number of accepted files
    pub file_count: i64,
    /// Current download speed of the item's files, bytes per second
    pub speed_bps: u64,
}

/// `<target>/<category>` with no trailing slash, or just `<target>` for no category
pub fn download_dir(target: &Path, category: &str) -> String {
    let target = target.to_string_lossy();
    let target = target.trim_end_matches('/');
    if category.is_empty() {
        target.to_string()
    } else {
        format!("{}/{}", target, category)
    }
}

/// Bytes still missing before the *arr may import. Progress is split into a Put.io half
/// and a local half so the *arr never sees 100% before the files are on disk.
fn left_until_done(item: &ArrItem, local: &LocalProgress) -> i64 {
    let total = item.total_size.max(0) as f64;
    let left = match item.state {
        ArrState::Putio => {
            let percent = item.putio_percent.clamp(0, 100) as f64;
            total * (1.0 - 0.5 * percent / 100.0)
        }
        ArrState::Local => {
            let done = if total > 0.0 {
                (local.completed_bytes.max(0) as f64 / total).min(1.0)
            } else {
                0.0
            };
            total * 0.5 * (1.0 - done)
        }
        ArrState::Done => 0.0,
        ArrState::Failed => total,
    };
    left.round() as i64
}

fn status(item: &ArrItem) -> i64 {
    match item.state {
        // Held before Put.io (043) reads as queued, as Put.io's own queue does
        ArrState::Putio if matches!(item.putio_status.as_deref(), Some("IN_QUEUE" | HELD)) => {
            STATUS_QUEUED
        }
        ArrState::Putio | ArrState::Local => STATUS_DOWNLOADING,
        ArrState::Done | ArrState::Failed => STATUS_STOPPED,
    }
}

fn eta(item: &ArrItem, local: &LocalProgress) -> i64 {
    match item.state {
        ArrState::Putio => item.putio_eta.filter(|e| *e >= 0).unwrap_or(-1),
        ArrState::Local if local.speed_bps > 0 => {
            let left = (item.total_size - local.completed_bytes).max(0) as u64;
            (left / local.speed_bps) as i64
        }
        _ => -1,
    }
}

/// The Transmission torrent object for `torrent-get`
pub fn torrent(item: &ArrItem, local: &LocalProgress, target: &Path) -> Value {
    let left = left_until_done(item, local);
    let percent_done = if item.total_size > 0 {
        1.0 - left as f64 / item.total_size as f64
    } else if item.state == ArrState::Done {
        1.0
    } else {
        0.0
    };
    let failed = item.state == ArrState::Failed;
    json!({
        "id": item.id,
        "hashString": item.hash,
        "name": item.name,
        "downloadDir": download_dir(target, &item.category),
        "totalSize": item.total_size,
        "leftUntilDone": left,
        "percentDone": percent_done,
        "status": status(item),
        "isFinished": item.state == ArrState::Done,
        "errorString": if failed { item.error.clone().unwrap_or_default() } else { String::new() },
        "error": if failed { ERROR_LOCAL } else { 0 },
        "eta": eta(item, local),
        "labels": [],
        "seedRatioMode": 0,
        "seedIdleMode": 0,
        "seedRatioLimit": 0,
        "seedIdleLimit": 0,
        "uploadedEver": 0,
        "downloadedEver": local.completed_bytes,
        "secondsSeeding": 0,
        "secondsDownloading": 0,
        "fileCount": local.file_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use putio_sync_db::BlocklistState;

    fn item(state: ArrState) -> ArrItem {
        ArrItem {
            id: 7,
            hash: "abc".into(),
            name: "Show.S01E01".into(),
            category: "sonarr".into(),
            transfer_id: Some(1),
            file_id: None,
            output_name: None,
            total_size: 1000,
            putio_status: Some("DOWNLOADING".into()),
            putio_percent: 50,
            putio_eta: Some(30),
            state,
            error: None,
            blocklist: BlocklistState::None,
            blocklist_attempts: 0,
        }
    }

    fn target() -> &'static Path {
        Path::new("/downloads/")
    }

    #[test]
    fn on_putio_counts_half() {
        let v = torrent(&item(ArrState::Putio), &LocalProgress::default(), target());
        assert_eq!(v["leftUntilDone"], 750);
        assert_eq!(v["percentDone"], 0.25);
        assert_eq!(v["status"], STATUS_DOWNLOADING);
        assert_eq!(v["eta"], 30);
        assert_eq!(v["isFinished"], false);
        assert_eq!(v["downloadDir"], "/downloads/sonarr");
        assert_eq!(v["error"], 0);
        assert_eq!(v["errorString"], "");
    }

    #[test]
    fn queued_on_putio() {
        let mut i = item(ArrState::Putio);
        i.putio_status = Some("IN_QUEUE".into());
        i.putio_eta = None;
        let v = torrent(&i, &LocalProgress::default(), target());
        assert_eq!(v["status"], STATUS_QUEUED);
        assert_eq!(v["eta"], -1);
    }

    #[test]
    fn held_reads_as_queued_with_no_error() {
        let mut i = item(ArrState::Putio);
        i.transfer_id = None;
        i.putio_status = Some(HELD.into());
        i.total_size = 0;
        i.putio_eta = None;
        let v = torrent(&i, &LocalProgress::default(), target());
        assert_eq!(v["status"], STATUS_QUEUED);
        assert_eq!(v["percentDone"], 0.0);
        assert_eq!(v["error"], 0);
        assert_eq!(v["errorString"], "");
        assert_eq!(v["eta"], -1);
        assert_eq!(v["isFinished"], false);
    }

    #[test]
    fn local_is_the_second_half() {
        let local = LocalProgress {
            completed_bytes: 600,
            file_count: 2,
            speed_bps: 100,
        };
        let v = torrent(&item(ArrState::Local), &local, target());
        assert_eq!(v["leftUntilDone"], 200);
        assert_eq!(v["percentDone"], 0.8);
        assert_eq!(v["status"], STATUS_DOWNLOADING);
        assert_eq!(v["eta"], 4);
        assert_eq!(v["downloadedEver"], 600);
        assert_eq!(v["fileCount"], 2);
    }

    #[test]
    fn done_and_failed() {
        let v = torrent(&item(ArrState::Done), &LocalProgress::default(), target());
        assert_eq!(v["leftUntilDone"], 0);
        assert_eq!(v["percentDone"], 1.0);
        assert_eq!(v["isFinished"], true);
        assert_eq!(v["status"], STATUS_STOPPED);

        let mut f = item(ArrState::Failed);
        f.error = Some("no files to import".into());
        let v = torrent(&f, &LocalProgress::default(), target());
        assert_eq!(v["leftUntilDone"], 1000);
        assert_eq!(v["error"], ERROR_LOCAL);
        assert_eq!(v["errorString"], "no files to import");
        assert_eq!(v["isFinished"], false);
        assert_eq!(v["status"], STATUS_STOPPED);
    }

    #[test]
    fn empty_category_and_zero_size() {
        let mut i = item(ArrState::Putio);
        i.category = String::new();
        i.total_size = 0;
        let v = torrent(&i, &LocalProgress::default(), Path::new("/downloads"));
        assert_eq!(v["downloadDir"], "/downloads");
        assert_eq!(v["percentDone"], 0.0);
        assert_eq!(v["labels"], json!([]));
        assert_eq!(v["seedRatioMode"], 0);
    }
}
