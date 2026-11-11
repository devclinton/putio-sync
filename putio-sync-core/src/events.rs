//! Events on the internal event bus (feature 027)
//!
//! Engine code calls [`emit`] when something happens; hooks (and later plugins and
//! notifications) read them through [`subscribe`]. The bus is process-wide because
//! `server --download` runs the download manager on a runtime of its own. The wire form
//! is versioned and documented in `EVENTS.md`; [`SCHEMA`] is its JSON Schema.

use std::fmt;
use std::str::FromStr;
use std::sync::{Arc, OnceLock};

use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Serialize, Serializer};
use tokio::sync::broadcast;

/// Raised only when a field is removed, renamed or changes type.
pub const SCHEMA_VERSION: u32 = 1;

/// The JSON Schema for every event.
pub const SCHEMA: &str = include_str!("events/schema.json");

/// Events the bus holds for a slow subscriber before it lags.
const BUS_CAPACITY: usize = 4096;

/// Longest `error` text an event carries, in bytes.
const MAX_ERROR: usize = 4096;

/// Every event type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventType {
    /// A file finished downloading
    DownloadFinished,
    /// A file download failed for good
    DownloadFailed,
    /// A new Put.io transfer showed up
    TransferAdded,
    /// A Put.io transfer finished
    TransferFinished,
    /// A Put.io transfer failed
    TransferFailed,
    /// A Put.io transfer stopped moving for the stall threshold (feature 030)
    TransferStuck,
    /// A sync run ended
    SyncCompleted,
    /// Put.io storage fell below the warning level
    StorageLow,
    /// A new download matched Put.io's CRC32 or size (feature 034)
    VerifyPassed,
    /// A file did not match Put.io's CRC32 or size
    VerifyFailed,
    /// A file is being downloaded again after a mismatch
    VerifyRedownload,
    /// A processing pipeline step failed (feature 038)
    PipelineStepFailed,
    /// An item finished its processing pipeline with warnings (feature 038)
    PipelineCompleted,
    /// A doctor check became a problem (feature 030)
    DoctorProblem,
    /// Sent by the "send test event" button only
    HookTest,
    /// Sent by a plugin's "send test event" button only
    PluginTest,
}

impl EventType {
    /// Every type, in documentation order.
    pub const ALL: [EventType; 16] = [
        EventType::DownloadFinished,
        EventType::DownloadFailed,
        EventType::TransferAdded,
        EventType::TransferFinished,
        EventType::TransferFailed,
        EventType::TransferStuck,
        EventType::SyncCompleted,
        EventType::StorageLow,
        EventType::VerifyPassed,
        EventType::VerifyFailed,
        EventType::VerifyRedownload,
        EventType::PipelineStepFailed,
        EventType::PipelineCompleted,
        EventType::DoctorProblem,
        EventType::HookTest,
        EventType::PluginTest,
    ];

    /// The wire name, such as `download.finished`.
    pub fn as_str(&self) -> &'static str {
        match self {
            EventType::DownloadFinished => "download.finished",
            EventType::DownloadFailed => "download.failed",
            EventType::TransferAdded => "transfer.added",
            EventType::TransferFinished => "transfer.finished",
            EventType::TransferFailed => "transfer.failed",
            EventType::TransferStuck => "transfer.stuck",
            EventType::SyncCompleted => "sync.completed",
            EventType::StorageLow => "storage.low",
            EventType::VerifyPassed => "verify.passed",
            EventType::VerifyFailed => "verify.failed",
            EventType::VerifyRedownload => "verify.redownload",
            EventType::PipelineStepFailed => "pipeline.step_failed",
            EventType::PipelineCompleted => "pipeline.completed",
            EventType::DoctorProblem => "doctor.problem",
            EventType::HookTest => "hook.test",
            EventType::PluginTest => "plugin.test",
        }
    }

    /// Whether only a test button sends it, so `*` leaves it out.
    pub fn is_test(&self) -> bool {
        matches!(self, EventType::HookTest | EventType::PluginTest)
    }
}

impl fmt::Display for EventType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for EventType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        EventType::ALL
            .into_iter()
            .find(|t| t.as_str() == s)
            .ok_or_else(|| format!("unknown event \"{}\"", s))
    }
}

/// Where the event came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Source {
    /// Always `putio-sync`
    pub app: String,
    /// The daemon's version
    pub version: String,
    /// The machine's host name
    pub host: String,
}

impl Source {
    fn current() -> Source {
        Source {
            app: "putio-sync".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            host: host().to_string(),
        }
    }
}

/// `HOSTNAME`, else `/etc/hostname`, else `COMPUTERNAME`, else `unknown`, read once.
fn host() -> &'static str {
    static HOST: OnceLock<String> = OnceLock::new();
    HOST.get_or_init(|| {
        let non_empty = |s: String| {
            let s = s.trim().to_string();
            (!s.is_empty()).then_some(s)
        };
        std::env::var("HOSTNAME")
            .ok()
            .and_then(non_empty)
            .or_else(|| {
                std::fs::read_to_string("/etc/hostname")
                    .ok()
                    .and_then(non_empty)
            })
            .or_else(|| std::env::var("COMPUTERNAME").ok().and_then(non_empty))
            .unwrap_or_else(|| "unknown".to_string())
    })
}

/// One event, as sent to hooks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// [`SCHEMA_VERSION`]
    pub schema_version: u32,
    /// UUID v4
    pub id: String,
    /// When it happened, UTC
    #[serde(serialize_with = "millis")]
    pub occurred_at: DateTime<Utc>,
    /// Where it came from
    pub source: Source,
    /// `type` and `data`
    #[serde(flatten)]
    pub payload: Payload,
}

fn millis<S: Serializer>(at: &DateTime<Utc>, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&at.to_rfc3339_opts(SecondsFormat::Millis, true))
}

impl Event {
    /// Stamp a payload with a new id, the time and the source.
    pub fn new(payload: Payload) -> Event {
        Event {
            schema_version: SCHEMA_VERSION,
            id: uuid::Uuid::new_v4().to_string(),
            occurred_at: Utc::now(),
            source: Source::current(),
            payload,
        }
    }

    /// The event's type.
    pub fn event_type(&self) -> EventType {
        self.payload.event_type()
    }

    /// The event as one JSON document.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("events serialise")
    }
}

/// The event's `type` and `data`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum Payload {
    /// `download.finished`
    #[serde(rename = "download.finished")]
    DownloadFinished(DownloadFinished),
    /// `download.failed`
    #[serde(rename = "download.failed")]
    DownloadFailed(DownloadFailed),
    /// `transfer.added`
    #[serde(rename = "transfer.added")]
    TransferAdded(TransferChanged),
    /// `transfer.finished`
    #[serde(rename = "transfer.finished")]
    TransferFinished(TransferChanged),
    /// `transfer.failed`
    #[serde(rename = "transfer.failed")]
    TransferFailed(TransferChanged),
    /// `transfer.stuck`
    #[serde(rename = "transfer.stuck")]
    TransferStuck(TransferStuck),
    /// `sync.completed`
    #[serde(rename = "sync.completed")]
    SyncCompleted(SyncCompleted),
    /// `storage.low`
    #[serde(rename = "storage.low")]
    StorageLow(StorageLow),
    /// `verify.passed`
    #[serde(rename = "verify.passed")]
    VerifyPassed(VerifyPassed),
    /// `verify.failed`
    #[serde(rename = "verify.failed")]
    VerifyFailed(VerifyFailed),
    /// `verify.redownload`
    #[serde(rename = "verify.redownload")]
    VerifyRedownload(VerifyRedownload),
    /// `pipeline.step_failed`
    #[serde(rename = "pipeline.step_failed")]
    PipelineStepFailed(PipelineStepFailed),
    /// `pipeline.completed`
    #[serde(rename = "pipeline.completed")]
    PipelineCompleted(PipelineCompleted),
    /// `doctor.problem`
    #[serde(rename = "doctor.problem")]
    DoctorProblem(DoctorProblem),
    /// `hook.test`
    #[serde(rename = "hook.test")]
    HookTest(HookTest),
    /// `plugin.test`
    #[serde(rename = "plugin.test")]
    PluginTest(PluginTest),
}

impl Payload {
    /// The payload's type.
    pub fn event_type(&self) -> EventType {
        match self {
            Payload::DownloadFinished(_) => EventType::DownloadFinished,
            Payload::DownloadFailed(_) => EventType::DownloadFailed,
            Payload::TransferAdded(_) => EventType::TransferAdded,
            Payload::TransferFinished(_) => EventType::TransferFinished,
            Payload::TransferFailed(_) => EventType::TransferFailed,
            Payload::TransferStuck(_) => EventType::TransferStuck,
            Payload::SyncCompleted(_) => EventType::SyncCompleted,
            Payload::StorageLow(_) => EventType::StorageLow,
            Payload::VerifyPassed(_) => EventType::VerifyPassed,
            Payload::VerifyFailed(_) => EventType::VerifyFailed,
            Payload::VerifyRedownload(_) => EventType::VerifyRedownload,
            Payload::PipelineStepFailed(_) => EventType::PipelineStepFailed,
            Payload::PipelineCompleted(_) => EventType::PipelineCompleted,
            Payload::DoctorProblem(_) => EventType::DoctorProblem,
            Payload::HookTest(_) => EventType::HookTest,
            Payload::PluginTest(_) => EventType::PluginTest,
        }
    }

    fn cut_errors(&mut self) {
        match self {
            Payload::DownloadFailed(d) => cut(&mut d.error),
            Payload::TransferAdded(t)
            | Payload::TransferFinished(t)
            | Payload::TransferFailed(t) => {
                if let Some(e) = t.error.as_mut() {
                    cut(e)
                }
            }
            Payload::SyncCompleted(s) => {
                if let Some(e) = s.error.as_mut() {
                    cut(e)
                }
            }
            Payload::PipelineStepFailed(p) => cut(&mut p.error),
            _ => {}
        }
    }
}

/// Cut `s` to [`MAX_ERROR`] bytes on a character boundary.
fn cut(s: &mut String) {
    if s.len() <= MAX_ERROR {
        return;
    }
    let mut end = MAX_ERROR - '…'.len_utf8();
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
    s.push('…');
}

/// `download.finished`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DownloadFinished {
    /// Put.io file id
    pub file_id: i64,
    /// File name as saved
    pub name: String,
    /// Path under the download folder, `/`-separated
    pub relative_path: String,
    /// Absolute path on the daemon's disk
    pub local_path: String,
    /// Size in bytes
    pub size_bytes: u64,
    /// How long the download took
    pub duration_seconds: f64,
    /// Average speed
    pub average_bytes_per_second: u64,
    /// Whether it carried on from a partial file
    pub resumed: bool,
}

/// `download.failed`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DownloadFailed {
    /// Put.io file id
    pub file_id: i64,
    /// File name
    pub name: String,
    /// Path under the download folder, `/`-separated
    pub relative_path: String,
    /// Size in bytes
    pub size_bytes: u64,
    /// What went wrong, token-free
    pub error: String,
    /// `network`, `http`, `io`, `integrity` or `other`
    pub reason: String,
    /// Attempts made
    pub attempts: u32,
}

/// `transfer.added`, `transfer.finished` and `transfer.failed`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransferChanged {
    /// Put.io transfer id
    pub transfer_id: i64,
    /// Transfer name
    pub name: String,
    /// `radarr`, `sonarr`, `arr`, `watch`, `manual` or `putio`
    pub origin: String,
    /// Put.io status, such as `DOWNLOADING`
    pub status: String,
    /// Size in bytes, when Put.io knows it
    pub size_bytes: Option<i64>,
    /// The Put.io file or folder the transfer made
    pub file_id: Option<i64>,
    /// Put.io's error message, set on `transfer.failed`
    pub error: Option<String>,
}

/// `transfer.stuck`: sent once each time a transfer stalls (feature 007's verdict)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransferStuck {
    /// Put.io transfer id
    pub transfer_id: i64,
    /// Transfer name
    pub name: String,
    /// Why it counts as stuck: `availability N%`, `no peers` or `no progress`
    pub reason: String,
    /// When it stopped moving
    #[serde(serialize_with = "millis")]
    pub stalled_since: DateTime<Utc>,
    /// Size in bytes, when Put.io knows it
    pub size_bytes: Option<i64>,
    /// Put.io's percent done
    pub percent_done: i64,
}

/// `sync.completed`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncCompleted {
    /// `schedule` or `manual`
    pub trigger: String,
    /// `success` or `failure`
    pub result: String,
    /// Why it failed
    pub error: Option<String>,
    /// When it started
    #[serde(serialize_with = "millis")]
    pub started_at: DateTime<Utc>,
    /// How long it took
    pub duration_seconds: f64,
    /// Files found on Put.io
    pub files_found: u64,
    /// Files queued for download
    pub files_queued: u64,
    /// Files skipped
    pub files_skipped: u64,
    /// Files blocked by rules
    pub files_blocked: u64,
    /// Files that failed
    pub files_failed: u64,
}

/// `storage.low`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StorageLow {
    /// Account size
    pub total_bytes: i64,
    /// In use
    pub used_bytes: i64,
    /// Free
    pub free_bytes: i64,
    /// Free, as a percentage of the size
    pub free_percent: f64,
    /// The configured warning level
    pub warn_free_percent: u8,
}

/// `verify.passed`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerifyPassed {
    /// Put.io file id
    pub file_id: i64,
    /// File name as saved
    pub name: String,
    /// Path under the download folder, `/`-separated
    pub relative_path: String,
    /// Absolute path on the daemon's disk
    pub local_path: String,
    /// Size in bytes
    pub size_bytes: u64,
    /// `crc32` or `size`
    pub method: String,
    /// Put.io's CRC32, lower case, when it has one
    pub crc32: Option<String>,
    /// Re-downloads it took
    pub redownloads: u32,
    /// Always `download`: a verify run does not send it
    pub trigger: String,
}

/// `verify.failed`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerifyFailed {
    /// Put.io file id
    pub file_id: i64,
    /// File name
    pub name: String,
    /// Path under the download folder, `/`-separated
    pub relative_path: String,
    /// Absolute path on the daemon's disk
    pub local_path: String,
    /// Size in bytes Put.io gives
    pub size_bytes: u64,
    /// Put.io's CRC32; null for a size mismatch
    pub expected_crc32: Option<String>,
    /// What the file hashed to; null for a size mismatch
    pub actual_crc32: Option<String>,
    /// 1 for the first download
    pub attempt: u32,
    /// Whether it will be downloaded again
    pub will_redownload: bool,
    /// `download` or `run`
    pub trigger: String,
}

/// `verify.redownload`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VerifyRedownload {
    /// Put.io file id
    pub file_id: i64,
    /// File name
    pub name: String,
    /// Path under the download folder, `/`-separated
    pub relative_path: String,
    /// Size in bytes
    pub size_bytes: u64,
    /// The download about to start (2 = the first re-download); 1 for a run
    pub attempt: u32,
    /// `download.verify_max_redownloads`
    pub max_redownloads: u32,
    /// `download` or `run`
    pub trigger: String,
}

/// `pipeline.step_failed` (feature 038)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipelineStepFailed {
    /// The item key, such as `transfer:123`
    pub key: String,
    /// The item name
    pub name: String,
    /// The stage, such as `post_processing`
    pub stage: String,
    /// The step name, such as the hook's name
    pub step: String,
    /// The attempt that failed, from 1
    pub attempt: u32,
    /// Whether the item waits for the operator (`true`) or the step is retried and given up
    pub required: bool,
    /// What went wrong, secret-free
    pub error: String,
}

/// `pipeline.completed` (feature 038)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PipelineCompleted {
    /// The item key
    pub key: String,
    /// The item name
    pub name: String,
    /// The optional steps that were given up
    pub warnings: Vec<String>,
}

/// `hook.test`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HookTest {
    /// The hook's name
    pub hook: String,
    /// A fixed message
    pub message: String,
}

/// `plugin.test`
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PluginTest {
    /// The plugin's name
    pub plugin: String,
    /// A fixed message
    pub message: String,
}

fn bus() -> &'static broadcast::Sender<Arc<Event>> {
    static BUS: OnceLock<broadcast::Sender<Arc<Event>>> = OnceLock::new();
    BUS.get_or_init(|| broadcast::channel(BUS_CAPACITY).0)
}

/// Put an event on the bus. Nobody listening is fine.
pub fn emit(mut payload: Payload) -> Arc<Event> {
    payload.cut_errors();
    let event = Arc::new(Event::new(payload));
    putio_sync_telemetry::hooks::record_event(event.event_type().as_str());
    let _ = bus().send(event.clone());
    event
}

/// Listen for events emitted from now on.
pub fn subscribe() -> broadcast::Receiver<Arc<Event>> {
    bus().subscribe()
}

/// `doctor.problem`: a check that is a problem now and was not in the previous run
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DoctorProblem {
    /// The check's id, such as `putio_token`
    pub check: String,
    /// The check's title
    pub title: String,
    /// What it found
    pub summary: String,
    /// The area it belongs to: `Put.io`, `Sync`, `Local` or `Connectivity`
    pub area: String,
    /// How many items it found; 0 for checks that do not count
    pub item_count: u64,
}

/// One sample payload of every type, for tests and the docs check.
#[doc(hidden)]
pub fn samples() -> Vec<Payload> {
    let transfer = TransferChanged {
        transfer_id: 991,
        name: "Some.Show.S01E01.1080p".to_string(),
        origin: "sonarr".to_string(),
        status: "DOWNLOADING".to_string(),
        size_bytes: Some(1_400_000_000),
        file_id: None,
        error: None,
    };
    vec![
        Payload::DownloadFinished(DownloadFinished {
            file_id: 123,
            name: "episode.mkv".to_string(),
            relative_path: "Shows/episode.mkv".to_string(),
            local_path: "/data/Shows/episode.mkv".to_string(),
            size_bytes: 1_400_000_000,
            duration_seconds: 92.5,
            average_bytes_per_second: 15_135_135,
            resumed: false,
        }),
        Payload::DownloadFailed(DownloadFailed {
            file_id: 124,
            name: "movie.mkv".to_string(),
            relative_path: "Movies/movie.mkv".to_string(),
            size_bytes: 4_000_000_000,
            error: "connection reset".to_string(),
            reason: "network".to_string(),
            attempts: 3,
        }),
        Payload::TransferAdded(transfer.clone()),
        Payload::TransferFinished(TransferChanged {
            status: "COMPLETED".to_string(),
            file_id: Some(555),
            ..transfer.clone()
        }),
        Payload::TransferFailed(TransferChanged {
            status: "ERROR".to_string(),
            error: Some("no seeders".to_string()),
            ..transfer
        }),
        Payload::TransferStuck(TransferStuck {
            transfer_id: 992,
            name: "Some.Show.S01E02.1080p".to_string(),
            reason: "no peers".to_string(),
            stalled_since: Utc::now(),
            size_bytes: Some(1_400_000_000),
            percent_done: 37,
        }),
        Payload::SyncCompleted(SyncCompleted {
            trigger: "schedule".to_string(),
            result: "success".to_string(),
            error: None,
            started_at: Utc::now(),
            duration_seconds: 3.2,
            files_found: 40,
            files_queued: 2,
            files_skipped: 37,
            files_blocked: 1,
            files_failed: 0,
        }),
        Payload::StorageLow(StorageLow {
            total_bytes: 100_000_000_000,
            used_bytes: 95_000_000_000,
            free_bytes: 5_000_000_000,
            free_percent: 5.0,
            warn_free_percent: 10,
        }),
        Payload::VerifyPassed(VerifyPassed {
            file_id: 123,
            name: "episode.mkv".to_string(),
            relative_path: "Shows/episode.mkv".to_string(),
            local_path: "/data/Shows/episode.mkv".to_string(),
            size_bytes: 1_400_000_000,
            method: "crc32".to_string(),
            crc32: Some("8f2c1a07".to_string()),
            redownloads: 0,
            trigger: "download".to_string(),
        }),
        Payload::VerifyFailed(VerifyFailed {
            file_id: 123,
            name: "episode.mkv".to_string(),
            relative_path: "Shows/episode.mkv".to_string(),
            local_path: "/data/Shows/episode.mkv".to_string(),
            size_bytes: 1_400_000_000,
            expected_crc32: Some("8f2c1a07".to_string()),
            actual_crc32: Some("00a1b2c3".to_string()),
            attempt: 1,
            will_redownload: true,
            trigger: "download".to_string(),
        }),
        Payload::VerifyRedownload(VerifyRedownload {
            file_id: 123,
            name: "episode.mkv".to_string(),
            relative_path: "Shows/episode.mkv".to_string(),
            size_bytes: 1_400_000_000,
            attempt: 2,
            max_redownloads: 2,
            trigger: "download".to_string(),
        }),
        Payload::PipelineStepFailed(PipelineStepFailed {
            key: "transfer:991".to_string(),
            name: "Some.Show.S01E01.1080p".to_string(),
            stage: "post_processing".to_string(),
            step: "tidy".to_string(),
            attempt: 1,
            required: true,
            error: "exited with code 3".to_string(),
        }),
        Payload::PipelineCompleted(PipelineCompleted {
            key: "transfer:991".to_string(),
            name: "Some.Show.S01E01.1080p".to_string(),
            warnings: vec!["tidy".to_string()],
        }),
        Payload::DoctorProblem(DoctorProblem {
            check: "disk_space".to_string(),
            title: "Disk space".to_string(),
            summary: "2.1 GB free on /data, below the 5 GB minimum".to_string(),
            area: "Local".to_string(),
            item_count: 0,
        }),
        Payload::HookTest(HookTest {
            hook: "home-assistant".to_string(),
            message: "Test event from putio-sync".to_string(),
        }),
        Payload::PluginTest(PluginTest {
            plugin: "ntfy".to_string(),
            message: "Test event from putio-sync".to_string(),
        }),
    ]
}

/// A validator for the JSON Schema keywords `schema.json` uses: `type`, `required`,
/// `properties`, `enum`, `const`, `oneOf` and `$ref` to `#/$defs/…`. Returns the first
/// problem found.
#[doc(hidden)]
pub fn validate_against_schema(value: &serde_json::Value) -> Result<(), String> {
    let schema: serde_json::Value = serde_json::from_str(SCHEMA).expect("schema.json parses");
    check(&schema, &schema, value, "$")
}

fn check(
    root: &serde_json::Value,
    schema: &serde_json::Value,
    value: &serde_json::Value,
    at: &str,
) -> Result<(), String> {
    use serde_json::Value;
    if let Some(r) = schema.get("$ref").and_then(Value::as_str) {
        let name = r
            .strip_prefix("#/$defs/")
            .ok_or(format!("bad $ref {}", r))?;
        return check(root, &root["$defs"][name], value, at);
    }
    if let Some(c) = schema.get("const") {
        if c != value {
            return Err(format!("{}: expected {}", at, c));
        }
    }
    if let Some(options) = schema.get("enum").and_then(Value::as_array) {
        if !options.contains(value) {
            return Err(format!("{}: {} is not one of {:?}", at, value, options));
        }
    }
    if let Some(t) = schema.get("type") {
        let types: Vec<&str> = match t {
            Value::String(s) => vec![s.as_str()],
            Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
            _ => vec![],
        };
        let ok = types.iter().any(|t| match *t {
            "object" => value.is_object(),
            "string" => value.is_string(),
            "integer" => value.is_i64() || value.is_u64(),
            "number" => value.is_number(),
            "boolean" => value.is_boolean(),
            "null" => value.is_null(),
            "array" => value.is_array(),
            _ => false,
        });
        if !ok {
            return Err(format!("{}: {} is not {:?}", at, value, types));
        }
    }
    if let Some(required) = schema.get("required").and_then(Value::as_array) {
        for key in required.iter().filter_map(Value::as_str) {
            if value.get(key).is_none() {
                return Err(format!("{}: missing {}", at, key));
            }
        }
    }
    if let Some(props) = schema.get("properties").and_then(Value::as_object) {
        for (key, sub) in props {
            if let Some(v) = value.get(key) {
                check(root, sub, v, &format!("{}.{}", at, key))?;
            }
        }
    }
    if let Some(options) = schema.get("oneOf").and_then(Value::as_array) {
        let matched = options
            .iter()
            .filter(|o| check(root, o, value, at).is_ok())
            .count();
        if matched != 1 {
            return Err(format!("{}: matches {} of oneOf, not 1", at, matched));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_form_has_the_envelope_and_type_and_data() {
        let event = Event::new(samples().remove(0));
        let v: serde_json::Value = serde_json::from_str(&event.to_json()).unwrap();
        assert_eq!(v["schema_version"], 1);
        assert_eq!(v["type"], "download.finished");
        assert_eq!(v["data"]["relative_path"], "Shows/episode.mkv");
        assert_eq!(v["source"]["app"], "putio-sync");
        assert_eq!(v["id"].as_str().unwrap().len(), 36);
        let at = v["occurred_at"].as_str().unwrap();
        assert!(at.ends_with('Z') && at.len() == 24, "{}", at);
        let back: Event = serde_json::from_value(v).unwrap();
        assert_eq!(back.event_type(), EventType::DownloadFinished);
    }

    #[test]
    fn every_sample_validates_against_the_schema() {
        let samples = samples();
        assert_eq!(samples.len(), EventType::ALL.len());
        for payload in samples {
            let v: serde_json::Value =
                serde_json::from_str(&Event::new(payload.clone()).to_json()).unwrap();
            validate_against_schema(&v).unwrap_or_else(|e| panic!("{:?}: {}", payload, e));
        }
    }

    #[test]
    fn the_schema_rejects_a_wrong_payload() {
        let mut v: serde_json::Value =
            serde_json::from_str(&Event::new(samples().remove(0)).to_json()).unwrap();
        v["data"]["size_bytes"] = serde_json::json!("big");
        assert!(validate_against_schema(&v).is_err());
        v["data"]["size_bytes"] = serde_json::json!(1);
        v["type"] = serde_json::json!("storage.low");
        assert!(validate_against_schema(&v).is_err());
    }

    #[test]
    fn types_match_the_schema_and_telemetry() {
        let schema: serde_json::Value = serde_json::from_str(SCHEMA).unwrap();
        let listed: Vec<&str> = schema["properties"]["type"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        let ours: Vec<&str> = EventType::ALL.iter().map(EventType::as_str).collect();
        assert_eq!(listed, ours);
        assert_eq!(putio_sync_telemetry::hooks::EVENT_TYPES.to_vec(), ours);
        for t in EventType::ALL {
            assert_eq!(t.as_str().parse::<EventType>().unwrap(), t);
        }
        assert!("download.done".parse::<EventType>().is_err());
    }

    #[test]
    fn long_errors_are_cut() {
        let mut payload = samples().remove(1);
        if let Payload::DownloadFailed(d) = &mut payload {
            d.error = "é".repeat(5000);
        }
        let event = emit(payload);
        let Payload::DownloadFailed(d) = &event.payload else {
            panic!()
        };
        assert!(d.error.len() <= MAX_ERROR && d.error.ends_with('…'));
    }

    #[tokio::test]
    async fn a_subscriber_receives_an_emitted_event() {
        let mut rx = subscribe();
        let sent = emit(Payload::HookTest(HookTest {
            hook: "bus-test".to_string(),
            message: "hi".to_string(),
        }));
        loop {
            let got = rx.recv().await.unwrap();
            if got.id == sent.id {
                break;
            }
        }
    }
}
