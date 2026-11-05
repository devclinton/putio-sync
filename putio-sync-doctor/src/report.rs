//! What the doctor reports: checks, their findings and the fixes on offer

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use std::time::Duration;

/// Most items one check carries in a report; previews list them all
pub const MAX_ITEMS: usize = 100;

/// How a check came out
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ok,
    Warning,
    Problem,
    /// The check could not apply (a missing database, failed sign-in, ...)
    Skipped,
}

impl Status {
    /// Gauge value: ok 0, warning 1, problem 2, skipped -1
    pub fn metric_value(self) -> f64 {
        match self {
            Status::Ok => 0.0,
            Status::Warning => 1.0,
            Status::Problem => 2.0,
            Status::Skipped => -1.0,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Warning => "warning",
            Status::Problem => "problem",
            Status::Skipped => "skipped",
        }
    }

    /// Severity for the overall status; skipped counts as nothing
    fn severity(self) -> u8 {
        match self {
            Status::Ok | Status::Skipped => 0,
            Status::Warning => 1,
            Status::Problem => 2,
        }
    }
}

/// Group a check belongs to
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Area {
    Putio,
    Sync,
    Local,
    Connectivity,
}

impl Area {
    pub const ALL: [Area; 4] = [Area::Putio, Area::Sync, Area::Local, Area::Connectivity];

    pub fn title(self) -> &'static str {
        match self {
            Area::Putio => "Put.io",
            Area::Sync => "Sync",
            Area::Local => "Local",
            Area::Connectivity => "Connectivity",
        }
    }
}

macro_rules! ids {
    ($(#[$meta:meta])* $name:ident { $($variant:ident => $s:literal),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        pub enum $name {
            $(#[serde(rename = $s)] $variant),+
        }

        impl $name {
            pub const ALL: &'static [$name] = &[$($name::$variant),+];

            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $s),+
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl FromStr for $name {
            type Err = String;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Self::ALL
                    .iter()
                    .copied()
                    .find(|id| id.as_str() == s)
                    .ok_or_else(|| format!("unknown {}: {}", stringify!($name), s))
            }
        }
    };
}

ids!(
    /// Every check, in run and report order
    CheckId {
        Auth => "auth",
        Storage => "storage",
        Transfers => "transfers",
        EmptyFolders => "empty_folders",
        Duplicates => "duplicates",
        OrphanedTorrents => "orphaned_torrents",
        StuckWatchFiles => "stuck_watch_files",
        DeadQueueRows => "dead_queue_rows",
        FailingDownloads => "failing_downloads",
        OrphanPartials => "orphan_partials",
        Bandwidth => "bandwidth",
        Verification => "verification",
        DownloadDir => "download_dir",
        DiskSpace => "disk_space",
        Database => "database",
        Config => "config",
        Reachability => "reachability",
        RateLimit => "rate_limit",
        ClockSkew => "clock_skew",
        Tailscale => "tailscale",
        Listen => "listen",
        Hooks => "hooks",
        Plugins => "plugins",
        LogLevel => "log_level",
    }
);

impl CheckId {
    pub fn area(self) -> Area {
        use CheckId::*;
        match self {
            Auth | Storage | Transfers | EmptyFolders | Duplicates | OrphanedTorrents
            | StuckWatchFiles => Area::Putio,
            DeadQueueRows | FailingDownloads | OrphanPartials | Bandwidth | Verification => {
                Area::Sync
            }
            DownloadDir | DiskSpace | Database | Config | Hooks | Plugins | LogLevel => Area::Local,
            Reachability | RateLimit | ClockSkew | Tailscale | Listen => Area::Connectivity,
        }
    }

    pub fn title(self) -> &'static str {
        use CheckId::*;
        match self {
            Auth => "Put.io sign-in",
            Storage => "Put.io storage",
            Transfers => "Transfers",
            EmptyFolders => "Empty folders",
            Duplicates => "Duplicate files",
            OrphanedTorrents => "Leftover .torrent files",
            StuckWatchFiles => "Watch folder",
            DeadQueueRows => "Queue entries for deleted files",
            FailingDownloads => "Failing downloads",
            OrphanPartials => "Leftover partial downloads",
            Bandwidth => "Bandwidth limit",
            Verification => "Download verification",
            DownloadDir => "Download folder",
            DiskSpace => "Disk space",
            Database => "Database",
            Config => "Config file",
            Reachability => "Put.io reachability",
            RateLimit => "Rate limit",
            ClockSkew => "Clock",
            Tailscale => "Tailscale",
            Listen => "Listening addresses",
            Hooks => "Event hooks",
            Plugins => "Plugins",
            LogLevel => "Log level",
        }
    }

    /// Fixes this check can offer
    pub fn fixes(self) -> &'static [FixId] {
        match self {
            CheckId::Transfers => &[FixId::TransfersRetry, FixId::TransfersReadd],
            CheckId::EmptyFolders => &[FixId::EmptyFoldersClean],
            CheckId::DeadQueueRows => &[FixId::DeadQueueRowsRemove],
            CheckId::FailingDownloads => &[FixId::FailingDownloadsRequeue],
            CheckId::OrphanPartials => &[FixId::OrphanPartialsDelete],
            _ => &[],
        }
    }
}

ids!(
    /// Every fix
    FixId {
        TransfersRetry => "transfers.retry",
        TransfersReadd => "transfers.readd",
        EmptyFoldersClean => "empty_folders.clean",
        DeadQueueRowsRemove => "dead_queue_rows.remove",
        FailingDownloadsRequeue => "failing_downloads.requeue",
        OrphanPartialsDelete => "orphan_partials.delete",
    }
);

impl FixId {
    /// The check whose items the fix acts on
    pub fn check(self) -> CheckId {
        match self {
            FixId::TransfersRetry | FixId::TransfersReadd => CheckId::Transfers,
            FixId::EmptyFoldersClean => CheckId::EmptyFolders,
            FixId::DeadQueueRowsRemove => CheckId::DeadQueueRows,
            FixId::FailingDownloadsRequeue => CheckId::FailingDownloads,
            FixId::OrphanPartialsDelete => CheckId::OrphanPartials,
        }
    }

    /// Deletes or cancels something, so it needs a preview and a confirmation
    pub fn destructive(self) -> bool {
        !matches!(self, FixId::TransfersRetry | FixId::FailingDownloadsRequeue)
    }

    pub fn label(self) -> &'static str {
        match self {
            FixId::TransfersRetry => "Retry",
            FixId::TransfersReadd => "Re-add",
            FixId::EmptyFoldersClean => "Clean up",
            FixId::DeadQueueRowsRemove => "Remove",
            FixId::FailingDownloadsRequeue => "Requeue",
            FixId::OrphanPartialsDelete => "Delete",
        }
    }

    pub fn info(self) -> FixInfo {
        FixInfo {
            id: self,
            label: self.label().to_string(),
            destructive: self.destructive(),
        }
    }
}

/// One thing a check found
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    /// Stable id a fix takes: transfer id, file id, path relative to the download folder...
    pub key: String,
    /// What to show
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// The fix that applies to this item, when it differs per item
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix: Option<FixId>,
}

impl Item {
    pub fn new(key: impl ToString, label: impl Into<String>) -> Self {
        Self {
            key: key.to_string(),
            label: label.into(),
            detail: None,
            fix: None,
        }
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn fix(mut self, fix: FixId) -> Self {
        self.fix = Some(fix);
        self
    }
}

/// A fix as offered on a check
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixInfo {
    pub id: FixId,
    pub label: String,
    pub destructive: bool,
}

/// What a check found, before the item cap
#[derive(Debug, Clone)]
pub struct Finding {
    pub status: Status,
    pub summary: String,
    pub suggestion: Option<String>,
    pub items: Vec<Item>,
}

impl Finding {
    pub fn new(status: Status, summary: impl Into<String>) -> Self {
        Self {
            status,
            summary: summary.into(),
            suggestion: None,
            items: Vec::new(),
        }
    }

    pub fn ok(summary: impl Into<String>) -> Self {
        Self::new(Status::Ok, summary)
    }

    pub fn skipped(summary: impl Into<String>) -> Self {
        Self::new(Status::Skipped, summary)
    }

    pub fn suggest(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }

    pub fn items(mut self, items: Vec<Item>) -> Self {
        self.items = items;
        self
    }

    /// Items a fix acts on: those marked with it
    pub fn items_for(&self, fix: FixId) -> Vec<Item> {
        self.items
            .iter()
            .filter(|i| i.fix == Some(fix))
            .cloned()
            .collect()
    }

    /// The report entry for check `id`, with at most [`MAX_ITEMS`] items
    pub fn into_result(self, id: CheckId) -> CheckResult {
        let fixes = id
            .fixes()
            .iter()
            .filter(|f| !self.items_for(**f).is_empty())
            .map(|f| f.info())
            .collect();
        let item_count = self.items.len() as u64;
        let mut items = self.items;
        items.truncate(MAX_ITEMS);
        CheckResult {
            id,
            area: id.area(),
            title: id.title().to_string(),
            status: self.status,
            summary: self.summary,
            suggestion: self.suggestion.filter(|_| self.status != Status::Ok),
            item_count,
            items,
            fixes,
        }
    }
}

/// One check in a report
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckResult {
    pub id: CheckId,
    pub area: Area,
    pub title: String,
    pub status: Status,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
    /// All affected items; `items` holds the first [`MAX_ITEMS`]
    pub item_count: u64,
    pub items: Vec<Item>,
    /// Fixes that apply now
    pub fixes: Vec<FixInfo>,
}

/// Checks per status
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    pub ok: u32,
    pub warning: u32,
    pub problem: u32,
    pub skipped: u32,
}

/// One full doctor run
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Report {
    pub started_at: DateTime<Utc>,
    pub duration_ms: u64,
    /// The worst status of the checks that ran
    pub overall: Status,
    pub counts: Counts,
    pub checks: Vec<CheckResult>,
}

impl Report {
    pub fn new(started_at: DateTime<Utc>, duration: Duration, checks: Vec<CheckResult>) -> Self {
        let mut report = Self {
            started_at,
            duration_ms: duration.as_millis() as u64,
            overall: Status::Ok,
            counts: Counts::default(),
            checks,
        };
        report.tally();
        report
    }

    /// Swap in a check run again after a fix
    pub fn replace(&mut self, check: CheckResult) {
        if let Some(slot) = self.checks.iter_mut().find(|c| c.id == check.id) {
            *slot = check;
            self.tally();
        }
    }

    pub fn check(&self, id: CheckId) -> Option<&CheckResult> {
        self.checks.iter().find(|c| c.id == id)
    }

    fn tally(&mut self) {
        let mut counts = Counts::default();
        for check in &self.checks {
            match check.status {
                Status::Ok => counts.ok += 1,
                Status::Warning => counts.warning += 1,
                Status::Problem => counts.problem += 1,
                Status::Skipped => counts.skipped += 1,
            }
        }
        self.counts = counts;
        self.overall = self
            .checks
            .iter()
            .map(|c| c.status)
            .max_by_key(|s| s.severity())
            .filter(|s| *s != Status::Skipped)
            .unwrap_or(Status::Ok);
    }
}

/// What a fix would act on now
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixPreview {
    pub fix: FixId,
    pub destructive: bool,
    /// Every item, with no cap, so a confirmation covers exactly what was shown
    pub items: Vec<Item>,
    pub item_count: u64,
}

/// Body of an apply request
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixRequest {
    /// Item keys from the preview
    pub items: Vec<String>,
    /// Required for a destructive fix
    #[serde(default)]
    pub confirm: bool,
}

/// An item a fix left alone because it no longer qualifies
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkippedItem {
    pub item: String,
    pub reason: String,
}

/// An item a fix tried and failed
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailedItem {
    pub item: String,
    pub error: String,
}

/// What an applied fix did
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixOutcome {
    pub fix: FixId,
    pub done: Vec<Item>,
    pub skipped: Vec<SkippedItem>,
    pub failed: Vec<FailedItem>,
    /// The check, run again after the fix
    pub check: CheckResult,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(id: CheckId, status: Status) -> CheckResult {
        Finding::new(status, "x").into_result(id)
    }

    #[test]
    fn overall_is_worst_non_skipped() {
        let report = Report::new(
            Utc::now(),
            Duration::ZERO,
            vec![
                result(CheckId::Auth, Status::Ok),
                result(CheckId::Storage, Status::Warning),
                result(CheckId::Database, Status::Skipped),
            ],
        );
        assert_eq!(report.overall, Status::Warning);
        assert_eq!(
            report.counts,
            Counts {
                ok: 1,
                warning: 1,
                problem: 0,
                skipped: 1
            }
        );

        let all_skipped = Report::new(
            Utc::now(),
            Duration::ZERO,
            vec![result(CheckId::Database, Status::Skipped)],
        );
        assert_eq!(all_skipped.overall, Status::Ok);
    }

    #[test]
    fn replace_recounts() {
        let mut report = Report::new(
            Utc::now(),
            Duration::ZERO,
            vec![result(CheckId::DeadQueueRows, Status::Problem)],
        );
        report.replace(result(CheckId::DeadQueueRows, Status::Ok));
        assert_eq!(report.overall, Status::Ok);
    }

    #[test]
    fn items_are_capped_but_counted() {
        let items = (0..250)
            .map(|i| Item::new(i, "f").fix(FixId::DeadQueueRowsRemove))
            .collect();
        let check = Finding::new(Status::Warning, "many")
            .items(items)
            .into_result(CheckId::DeadQueueRows);
        assert_eq!(check.items.len(), MAX_ITEMS);
        assert_eq!(check.item_count, 250);
        assert_eq!(check.fixes, vec![FixId::DeadQueueRowsRemove.info()]);
    }

    #[test]
    fn transfer_fixes_follow_item_marks() {
        let check = Finding::new(Status::Warning, "x")
            .items(vec![Item::new(1, "a").fix(FixId::TransfersRetry)])
            .into_result(CheckId::Transfers);
        assert_eq!(check.fixes, vec![FixId::TransfersRetry.info()]);
    }

    #[test]
    fn ids_round_trip_through_strings_and_serde() {
        for fix in FixId::ALL {
            assert_eq!(fix.as_str().parse::<FixId>().unwrap(), *fix);
            assert_eq!(
                serde_json::to_string(fix).unwrap(),
                format!("\"{}\"", fix.as_str())
            );
        }
        assert_eq!(CheckId::ALL.len(), 24);
        assert!("nope".parse::<FixId>().is_err());
    }

    #[test]
    fn suggestion_is_dropped_when_ok() {
        let check = Finding::ok("fine")
            .suggest("do x")
            .into_result(CheckId::Auth);
        assert!(check.suggestion.is_none());
    }
}
