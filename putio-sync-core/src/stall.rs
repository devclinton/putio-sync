//! Stalled Put.io transfer detection (feature 007).
//!
//! The daemon feeds every transfer list it fetches to a [`StallTracker`], which remembers per
//! downloading transfer when its byte count last changed. A transfer whose bytes have not
//! moved for the threshold is stalled. The API reads the verdicts to badge the Transfers
//! page. State is in memory only: after a restart the clock starts again.

use putio_sync_client::Transfer;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

/// Why a stalled transfer is probably stuck, most telling first
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StallReason {
    /// No peer in the swarm has a complete copy (availability below 100%)
    Availability(u8),
    /// Nobody is sending data
    NoPeers,
    /// Peers are there but no bytes arrive
    NoProgress,
}

impl StallReason {
    /// Short text for the UI: `availability 92%`, `no peers` or `no progress`
    pub fn label(&self) -> String {
        match self {
            StallReason::Availability(pct) => format!("availability {}%", pct),
            StallReason::NoPeers => "no peers".to_string(),
            StallReason::NoProgress => "no progress".to_string(),
        }
    }

    fn of(transfer: &Transfer) -> Self {
        if let Some(pct) = transfer.availability.filter(|pct| *pct < 100) {
            return StallReason::Availability(pct.clamp(0, 99) as u8);
        }
        if transfer.peers_sending_to_us == Some(0) {
            return StallReason::NoPeers;
        }
        StallReason::NoProgress
    }
}

/// A stalled transfer's verdict
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stall {
    /// When the transfer last made progress
    pub since: SystemTime,
    /// Most likely cause
    pub reason: StallReason,
}

#[derive(Debug)]
struct Record {
    downloaded: i64,
    changed_at: SystemTime,
    stall: Option<Stall>,
}

/// Remembers per downloading transfer when its byte count last changed
#[derive(Debug)]
pub struct StallTracker {
    threshold: Duration,
    records: Mutex<HashMap<i64, Record>>,
}

impl Default for StallTracker {
    /// The default 6 h threshold
    fn default() -> Self {
        Self::new(Duration::from_secs(6 * 3600))
    }
}

impl StallTracker {
    /// A tracker that calls a transfer stalled after `threshold` without new bytes
    pub fn new(threshold: Duration) -> Self {
        Self {
            threshold,
            records: Mutex::new(HashMap::new()),
        }
    }

    /// Update from a full transfer list fetched at `now`; returns how many are stalled.
    ///
    /// Only transfers in the `DOWNLOADING` state that report `downloaded` are tracked;
    /// any other transfer, and any transfer missing from `transfers`, is forgotten.
    pub fn observe(&self, transfers: &[Transfer], now: SystemTime) -> usize {
        let mut records = self.lock();
        let mut seen = HashMap::with_capacity(transfers.len());
        for transfer in transfers {
            let downloaded = match transfer.downloaded {
                Some(bytes) if transfer.status.eq_ignore_ascii_case("DOWNLOADING") => bytes,
                _ => continue,
            };
            let record = match records.remove(&transfer.id) {
                Some(mut record) if record.downloaded == downloaded => {
                    let still = now
                        .duration_since(record.changed_at)
                        .unwrap_or(Duration::ZERO);
                    record.stall = (still >= self.threshold).then(|| Stall {
                        since: record.changed_at,
                        reason: StallReason::of(transfer),
                    });
                    record
                }
                _ => Record {
                    downloaded,
                    changed_at: now,
                    stall: None,
                },
            };
            seen.insert(transfer.id, record);
        }
        *records = seen;
        records.values().filter(|r| r.stall.is_some()).count()
    }

    /// The verdict for transfer `id`, if it is stalled
    pub fn get(&self, id: i64) -> Option<Stall> {
        self.lock().get(&id).and_then(|r| r.stall)
    }

    /// Number of transfers stalled at the last [`observe`](Self::observe)
    pub fn stalled_count(&self) -> usize {
        self.lock().values().filter(|r| r.stall.is_some()).count()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<i64, Record>> {
        // A panic while holding the lock leaves plain data behind; keep using it
        self.records.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR: Duration = Duration::from_secs(3600);

    fn transfer(id: i64, status: &str, downloaded: i64) -> Transfer {
        serde_json::from_value(serde_json::json!({
            "id": id, "name": format!("t{}", id), "status": status,
            "downloaded": downloaded, "availability": 100, "peers_sending_to_us": 3,
            "created_at": "2026-10-26T10:00:00Z", "save_parent_id": 0,
            "estimated_time": null, "seconds_seeding": null, "error_message": null,
            "finished_at": null, "file_id": null
        }))
        .unwrap()
    }

    fn at(hours: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(1_800_000_000) + HOUR * hours as u32
    }

    #[test]
    fn unchanged_bytes_for_the_threshold_is_stalled() {
        let tracker = StallTracker::new(6 * HOUR);
        let t = transfer(1, "DOWNLOADING", 970);
        assert_eq!(tracker.observe(std::slice::from_ref(&t), at(0)), 0);
        assert_eq!(tracker.observe(std::slice::from_ref(&t), at(5)), 0);
        assert_eq!(tracker.get(1), None);
        assert_eq!(tracker.observe(&[t], at(6)), 1);
        let stall = tracker.get(1).unwrap();
        assert_eq!(stall.since, at(0));
        assert_eq!(stall.reason, StallReason::NoProgress);
        assert_eq!(tracker.stalled_count(), 1);
    }

    #[test]
    fn progress_clears_a_stall_and_restarts_the_clock() {
        let tracker = StallTracker::new(6 * HOUR);
        tracker.observe(&[transfer(1, "DOWNLOADING", 970)], at(0));
        assert_eq!(
            tracker.observe(&[transfer(1, "DOWNLOADING", 970)], at(7)),
            1
        );
        assert_eq!(
            tracker.observe(&[transfer(1, "DOWNLOADING", 971)], at(8)),
            0
        );
        assert_eq!(tracker.get(1), None);
        assert_eq!(
            tracker.observe(&[transfer(1, "DOWNLOADING", 971)], at(13)),
            0
        );
        assert_eq!(
            tracker.observe(&[transfer(1, "DOWNLOADING", 971)], at(14)),
            1
        );
        assert_eq!(tracker.get(1).unwrap().since, at(8));
    }

    /// Put.io restarting a transfer lowers its bytes; that is a change too
    #[test]
    fn fewer_bytes_count_as_a_change() {
        let tracker = StallTracker::new(6 * HOUR);
        tracker.observe(&[transfer(1, "DOWNLOADING", 970)], at(0));
        assert_eq!(tracker.observe(&[transfer(1, "DOWNLOADING", 10)], at(7)), 0);
    }

    #[test]
    fn only_downloading_transfers_can_stall() {
        let tracker = StallTracker::new(6 * HOUR);
        let others: Vec<Transfer> = [
            "IN_QUEUE",
            "WAITING",
            "PREPARING_DOWNLOAD",
            "COMPLETING",
            "SEEDING",
            "COMPLETED",
            "ERROR",
        ]
        .iter()
        .enumerate()
        .map(|(i, s)| transfer(i as i64 + 10, s, 500))
        .collect();
        tracker.observe(&others, at(0));
        assert_eq!(tracker.observe(&others, at(48)), 0);
        for t in &others {
            assert_eq!(tracker.get(t.id), None, "{}", t.status);
        }
    }

    #[test]
    fn leaving_downloading_or_the_list_forgets_the_transfer() {
        let tracker = StallTracker::new(6 * HOUR);
        tracker.observe(
            &[transfer(1, "DOWNLOADING", 5), transfer(2, "DOWNLOADING", 5)],
            at(0),
        );
        // 1 completes, 2 disappears
        tracker.observe(&[transfer(1, "COMPLETING", 5)], at(7));
        assert_eq!(tracker.stalled_count(), 0);
        // Both back and unchanged: their clocks start again
        let back = [transfer(1, "DOWNLOADING", 5), transfer(2, "DOWNLOADING", 5)];
        assert_eq!(tracker.observe(&back, at(8)), 0);
        assert_eq!(tracker.observe(&back, at(14)), 2);
    }

    #[test]
    fn reason_prefers_availability_then_peers() {
        let tracker = StallTracker::new(HOUR);
        let mut low = transfer(1, "DOWNLOADING", 5);
        low.availability = Some(92);
        low.peers_sending_to_us = Some(0);
        let mut lonely = transfer(2, "DOWNLOADING", 5);
        lonely.peers_sending_to_us = Some(0);
        let busy = transfer(3, "DOWNLOADING", 5);
        let all = [low, lonely, busy];
        tracker.observe(&all, at(0));
        assert_eq!(tracker.observe(&all, at(1)), 3);
        assert_eq!(
            tracker.get(1).unwrap().reason,
            StallReason::Availability(92)
        );
        assert_eq!(tracker.get(2).unwrap().reason, StallReason::NoPeers);
        assert_eq!(tracker.get(3).unwrap().reason, StallReason::NoProgress);
        assert_eq!(StallReason::Availability(92).label(), "availability 92%");
        assert_eq!(StallReason::NoPeers.label(), "no peers");
        assert_eq!(StallReason::NoProgress.label(), "no progress");
    }

    /// A clock that steps backwards never panics or stalls a transfer early
    #[test]
    fn clock_going_back_is_not_progress_or_stall() {
        let tracker = StallTracker::new(6 * HOUR);
        let t = transfer(1, "DOWNLOADING", 5);
        tracker.observe(std::slice::from_ref(&t), at(10));
        assert_eq!(tracker.observe(&[t], at(2)), 0);
    }
}
