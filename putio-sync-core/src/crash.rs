//! Crash reports (feature 058, part 1)
//!
//! A panic hook ([`install`]) writes one JSON file per crash to `crash-reports/` next to the
//! config file: version, build, OS and arch, uptime, the thread, the panic message, a
//! backtrace and the last [`LOG_LINES`] log lines, all masked by [`crate::log_redact`]. At most
//! [`KEEP`] files are kept. Nothing here is ever sent anywhere: the web UI and the CLI show the
//! report and a prefilled GitHub new-issue link ([`issue_url`]) that the person opens
//! themselves.
//!
//! A `running` marker is written at start and removed on a clean stop. When it is still there
//! at the next start and no crash report is newer than it, the run ended without a panic hook
//! (out of memory, SIGKILL, a container restart): a "stopped without a report" entry is written.
//!
//! Everything is best effort: the hook never panics and never logs.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::log_capture::{self, LogQuery};
use crate::log_redact::redact;

/// Crash files kept; the oldest go first
pub const KEEP: usize = 10;
/// Log lines in a report
pub const LOG_LINES: usize = 50;
/// Longest prefilled issue URL, in characters
pub const MAX_URL: usize = 6000;
/// The project's repository
pub const REPO: &str = "devclinton/putio-sync";
/// The folder of crash files, next to the config file
pub const DIR_NAME: &str = "crash-reports";
/// The marker that a run is in progress
const MARKER: &str = "running";
/// What an "unclean" entry says
pub const UNCLEAN_MESSAGE: &str =
    "putio-sync stopped without a report (killed, out of memory or restarted)";
/// A crash this recent counts for the doctor
pub const RECENT_DAYS: i64 = 7;

const TIME_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.3fZ";

/// How the run ended
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A panic hook ran
    Panic,
    /// The marker was left behind and no report followed
    Unclean,
}

/// One crash file
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CrashReport {
    /// Sortable, safe in a file name and a URL path
    pub id: String,
    /// How the run ended
    pub kind: Kind,
    /// RFC 3339, UTC, milliseconds
    pub time: String,
    /// The running version
    pub version: String,
    /// Docker, snap, Homebrew, archive...: the update checker's install kind
    pub build: String,
    /// `std::env::consts::OS`
    pub os: String,
    /// `std::env::consts::ARCH`
    pub arch: String,
    /// Seconds from start to the crash
    pub uptime_secs: u64,
    /// The panicking thread's name
    pub thread: String,
    /// The panic message, masked
    pub message: String,
    /// `file:line:col` of the panic
    #[serde(default)]
    pub location: Option<String>,
    /// The captured backtrace, masked
    #[serde(default)]
    pub backtrace: String,
    /// The last [`LOG_LINES`] log lines, masked
    #[serde(default)]
    pub log: Vec<String>,
    /// The person dismissed it in the UI or CLI
    #[serde(default)]
    pub dismissed: bool,
}

/// What a panic hook knows
#[derive(Debug, Clone, Default)]
pub struct PanicDetails {
    /// Thread name
    pub thread: String,
    /// Panic message
    pub message: String,
    /// `file:line:col`
    pub location: Option<String>,
}

struct State {
    dir: PathBuf,
    started: Instant,
}

static STATE: OnceLock<State> = OnceLock::new();
static BUILD: OnceLock<String> = OnceLock::new();
static COUNTER: AtomicU32 = AtomicU32::new(0);

/// The folder crash files go in, for a config file path (the default path when `None`)
pub fn dir_for(config_path: Option<&Path>) -> Option<PathBuf> {
    let default = crate::config::Config::default_path().ok();
    let parent = config_path.or(default.as_deref())?.parent()?;
    Some(parent.join(DIR_NAME))
}

fn now() -> String {
    chrono::Utc::now().format(TIME_FORMAT).to_string()
}

fn new_id() -> String {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed) % 1000;
    format!("{}-{:03}", chrono::Utc::now().format("%Y%m%dT%H%M%S%3f"), n)
}

/// Whether `id` is one this module made: no path can sneak in
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 40
        && id
            .chars()
            .all(|c| c.is_ascii_digit() || c == 'T' || c == '-')
}

fn file_name(id: &str) -> String {
    format!("crash-{id}.json")
}

/// Mask secrets and the home folder in `text`, line by line
fn clean(text: &str) -> String {
    let home = dirs::home_dir()
        .map(|h| h.to_string_lossy().into_owned())
        .filter(|h| h.len() > 3);
    text.lines()
        .map(|line| {
            let line = match &home {
                Some(h) => line.replace(h.as_str(), "~"),
                None => line.to_string(),
            };
            redact(&line)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn write_file(dir: &Path, report: &CrashReport) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(file_name(&report.id));
    let tmp = dir.join(format!(".{}.tmp", file_name(&report.id)));
    let json = serde_json::to_vec_pretty(report).map_err(std::io::Error::other)?;
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)
}

/// Delete the oldest files beyond [`KEEP`]
fn prune(dir: &Path) {
    let mut names = files(dir);
    names.sort();
    while names.len() > KEEP {
        let _ = std::fs::remove_file(dir.join(names.remove(0)));
    }
}

fn files(dir: &Path) -> Vec<String> {
    std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.starts_with("crash-") && n.ends_with(".json"))
        .collect()
}

/// Every report, newest first; unreadable files are skipped
pub fn list(dir: &Path) -> Vec<CrashReport> {
    let mut names = files(dir);
    names.sort();
    names.reverse();
    names
        .iter()
        .filter_map(|n| std::fs::read(dir.join(n)).ok())
        .filter_map(|b| serde_json::from_slice(&b).ok())
        .collect()
}

/// One report by id
pub fn get(dir: &Path, id: &str) -> Option<CrashReport> {
    if !valid_id(id) {
        return None;
    }
    serde_json::from_slice(&std::fs::read(dir.join(file_name(id))).ok()?).ok()
}

/// Mark a report dismissed; `false` when there is none
pub fn dismiss(dir: &Path, id: &str) -> bool {
    match get(dir, id) {
        Some(mut report) => {
            report.dismissed = true;
            write_file(dir, &report).is_ok()
        }
        None => false,
    }
}

/// The reports from the last [`RECENT_DAYS`] days that nobody has dismissed
pub fn recent_undismissed(dir: &Path) -> Vec<CrashReport> {
    let since = (chrono::Utc::now() - chrono::Duration::days(RECENT_DAYS))
        .format(TIME_FORMAT)
        .to_string();
    list(dir)
        .into_iter()
        .filter(|r| !r.dismissed && r.time >= since)
        .collect()
}

/// The last log lines, masked. A panic inside the log buffer's lock would leave it held while
/// the hook runs, so the read gets a thread and one second.
fn recent_log() -> Vec<String> {
    let (tx, rx) = mpsc::channel();
    let spawned = std::thread::Builder::new()
        .name("crash-log".into())
        .spawn(move || {
            let lines = log_capture::query(&LogQuery {
                limit: Some(LOG_LINES),
                ..Default::default()
            });
            let _ = tx.send(lines.iter().map(|l| l.to_text()).collect::<Vec<_>>());
        });
    if spawned.is_err() {
        return Vec::new();
    }
    rx.recv_timeout(Duration::from_secs(1))
        .map(|lines| lines.iter().map(|l| clean(l)).collect())
        .unwrap_or_default()
}

/// Name the install kind for reports (call once at start)
pub fn set_build(build: &str) {
    let _ = BUILD.set(build.to_string());
}

fn build() -> String {
    BUILD.get().cloned().unwrap_or_else(|| "unknown".into())
}

/// Write the report for a panic. The hook calls this; tests call it with made-up details.
pub fn write_panic(
    dir: &Path,
    started: Instant,
    details: &PanicDetails,
    backtrace: &str,
    log: Vec<String>,
) -> std::io::Result<CrashReport> {
    let report = CrashReport {
        id: new_id(),
        kind: Kind::Panic,
        time: now(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        build: build(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        uptime_secs: started.elapsed().as_secs(),
        thread: clean(&details.thread),
        message: clean(&details.message),
        location: details.location.as_deref().map(clean),
        backtrace: clean(backtrace),
        log: log.iter().map(|l| clean(l)).take(LOG_LINES).collect(),
        dismissed: false,
    };
    write_file(dir, &report)?;
    prune(dir);
    Ok(report)
}

fn on_panic(info: &std::panic::PanicHookInfo<'_>) {
    let Some(state) = STATE.get() else { return };
    let payload = info.payload();
    let message = payload
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(no message)".into());
    let details = PanicDetails {
        thread: std::thread::current()
            .name()
            .unwrap_or("<unnamed>")
            .to_string(),
        message,
        location: info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column())),
    };
    let backtrace = std::backtrace::Backtrace::force_capture().to_string();
    let _ = write_panic(
        &state.dir,
        state.started,
        &details,
        &backtrace,
        recent_log(),
    );
}

/// Marker contents: when this run started
#[derive(Serialize, Deserialize)]
struct Marker {
    started: String,
    version: String,
    build: String,
}

/// A running daemon or server. Dropping it (a clean stop) removes the marker.
pub struct Session {
    dir: PathBuf,
}

impl Session {
    /// Stop cleanly now (before an exec that would skip the drop)
    pub fn finish(self) {}
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.dir.join(MARKER));
    }
}

/// Install the panic hook, record a run that ended without a report, and write the marker.
/// `None` when no folder can be found. Calling it twice installs the hook once.
pub fn start(config_path: Option<&Path>, build: &str) -> Option<Session> {
    let dir = dir_for(config_path)?;
    set_build(build);
    if STATE
        .set(State {
            dir: dir.clone(),
            started: Instant::now(),
        })
        .is_ok()
    {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| on_panic(info)));
            previous(info);
        }));
    }
    note_unclean_stop(&dir);
    let marker = Marker {
        started: now(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        build: build.to_string(),
    };
    if std::fs::create_dir_all(&dir).is_ok() {
        if let Ok(json) = serde_json::to_vec(&marker) {
            let _ = std::fs::write(dir.join(MARKER), json);
        }
    }
    Some(Session { dir })
}

/// A marker left by the last run with no newer crash file: write the "stopped without a
/// report" entry. Returns it.
pub fn note_unclean_stop(dir: &Path) -> Option<CrashReport> {
    let marker: Marker = serde_json::from_slice(&std::fs::read(dir.join(MARKER)).ok()?).ok()?;
    let reported = list(dir).iter().any(|r| r.time >= marker.started);
    let _ = std::fs::remove_file(dir.join(MARKER));
    if reported {
        return None;
    }
    let report = CrashReport {
        id: new_id(),
        kind: Kind::Unclean,
        time: now(),
        version: marker.version,
        build: marker.build,
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        uptime_secs: 0,
        thread: String::new(),
        message: format!("{UNCLEAN_MESSAGE}. The run started {}.", marker.started),
        location: None,
        backtrace: String::new(),
        log: Vec::new(),
        dismissed: false,
    };
    write_file(dir, &report).ok()?;
    prune(dir);
    Some(report)
}

fn duration(secs: u64) -> String {
    let (d, h, m) = (secs / 86400, secs % 86400 / 3600, secs % 3600 / 60);
    match (d, h) {
        (0, 0) => format!("{m}m {}s", secs % 60),
        (0, _) => format!("{h}h {m}m"),
        _ => format!("{d}d {h}h {m}m"),
    }
}

/// Frames that are plumbing, not the cause
fn plumbing(name: &str) -> bool {
    const SKIP: [&str; 10] = [
        "std::",
        "core::",
        "alloc::",
        "<alloc",
        "<core",
        "rust_begin_unwind",
        "backtrace::",
        "putio_sync_core::crash::",
        "tokio::",
        "__rust",
    ];
    SKIP.iter().any(|p| name.starts_with(p))
}

/// The last two path segments of a frame, without the hash: `download::fetch::{{closure}}`
fn short_frame(name: &str) -> String {
    let name = match name.rfind("::h") {
        Some(i) if name[i + 3..].len() == 16 => &name[..i],
        _ => name,
    };
    let parts: Vec<&str> = name.split("::").collect();
    parts[parts.len().saturating_sub(2)..].join("::")
}

impl CrashReport {
    /// A short signature from the top frames, to title an issue and find repeats
    pub fn signature(&self) -> String {
        if self.kind == Kind::Unclean {
            return "stopped without a report".into();
        }
        let frames: Vec<String> = self
            .backtrace
            .lines()
            .filter_map(|l| {
                let (n, rest) = l.trim_start().split_once(": ")?;
                n.chars().all(|c| c.is_ascii_digit()).then_some(rest.trim())
            })
            .filter(|name| !plumbing(name))
            .take(3)
            .map(short_frame)
            .collect();
        if frames.is_empty() {
            let line = self.message.lines().next().unwrap_or("panic");
            return line.chars().take(60).collect();
        }
        frames.join(" < ")
    }

    /// The whole report as text, for the page, Copy and the issue
    pub fn to_text(&self) -> String {
        let mut out = format!(
            "putio-sync crash report\n\nVersion: {}\nBuild: {}\nOS: {} {}\nUptime: {}\nTime: {}\n",
            self.version,
            self.build,
            self.os,
            self.arch,
            duration(self.uptime_secs),
            self.time
        );
        if !self.thread.is_empty() {
            out += &format!("Thread: {}\n", self.thread);
        }
        out += &format!("\nMessage: {}\n", self.message);
        if let Some(at) = &self.location {
            out += &format!("At: {at}\n");
        }
        if !self.backtrace.is_empty() {
            out += &format!("\nBacktrace:\n{}\n", self.backtrace.trim_end());
        }
        if !self.log.is_empty() {
            out += &format!(
                "\nLast {} log lines:\n{}\n",
                self.log.len(),
                self.log.join("\n")
            );
        }
        out
    }

    /// The new-issue link, the title and whether the report was cut
    pub fn issue(&self) -> Issue {
        issue_for(self)
    }
}

/// What the person opens on GitHub
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Issue {
    /// `Crash: <signature>`
    pub title: String,
    /// A prefilled new `crash` issue
    pub url: String,
    /// Existing crash issues with this signature
    pub search_url: String,
    /// The report was cut to fit the URL; the issue asks for the copied report
    pub truncated: bool,
}

const CUT_NOTE: &str = "\n\n[Cut to fit the link. Paste the copied full report below.]";

/// Percent-encode everything but the unreserved characters
pub fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn issue_for(report: &CrashReport) -> Issue {
    let title: String = format!("Crash: {}", report.signature())
        .chars()
        .take(100)
        .collect();
    let base = format!(
        "https://github.com/{REPO}/issues/new?template=crash.yml&labels=crash&title={}&report=",
        encode(&title)
    );
    let search_url = format!(
        "https://github.com/{REPO}/issues?q={}",
        encode(&format!("is:issue label:crash {}", report.signature()))
    );
    let text = report.to_text();
    let fits = |body: &str| base.chars().count() + encode(body).len() <= MAX_URL;
    if fits(&text) {
        return Issue {
            url: format!("{base}{}", encode(&text)),
            title,
            search_url,
            truncated: false,
        };
    }
    // The longest prefix that fits with the note
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let body: String = chars[..mid].iter().collect::<String>() + CUT_NOTE;
        if fits(&body) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    let body: String = chars[..lo].iter().collect::<String>() + CUT_NOTE;
    Issue {
        url: format!("{base}{}", encode(&body)),
        title,
        search_url,
        truncated: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BACKTRACE: &str = "   0: putio_sync_core::crash::write_panic\n             at ./crash.rs:1\n   1: core::panicking::panic_fmt\n   2: putio_sync_download::fetch::part::{{closure}}::h0123456789abcdef\n             at ./fetch.rs:10:5\n   3: putio_sync_daemon::daemon::run\n   4: tokio::runtime::task::raw::poll\n   5: putio_sync_cli::main\n";

    fn details(message: &str) -> PanicDetails {
        PanicDetails {
            thread: "main".into(),
            message: message.into(),
            location: Some("src/x.rs:1:2".into()),
        }
    }

    #[test]
    fn a_panic_writes_a_redacted_file() {
        let dir = tempfile::tempdir().unwrap();
        let log = vec!["2026-11-22 INFO x: sign in token=abcdef123456789 ok".to_string()];
        let report = write_panic(
            dir.path(),
            Instant::now(),
            &details("bad password=hunter2hunter2 here"),
            BACKTRACE,
            log,
        )
        .unwrap();
        let text = std::fs::read_to_string(dir.path().join(file_name(&report.id))).unwrap();
        assert!(!text.contains("hunter2"), "{text}");
        assert!(!text.contains("abcdef123456789"), "{text}");
        assert!(text.contains("********"));
        let back = get(dir.path(), &report.id).unwrap();
        assert_eq!(back.thread, "main");
        assert_eq!(back.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(back.kind, Kind::Panic);
        assert!(back.backtrace.contains("putio_sync_download"));
    }

    #[test]
    fn at_most_ten_files_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        let mut ids = Vec::new();
        for _ in 0..13 {
            ids.push(
                write_panic(dir.path(), Instant::now(), &details("x"), "", vec![])
                    .unwrap()
                    .id,
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        let kept = list(dir.path());
        assert_eq!(kept.len(), KEEP);
        assert_eq!(kept[0].id, ids[12], "newest first");
        assert!(get(dir.path(), &ids[0]).is_none());
    }

    #[test]
    fn the_signature_uses_the_top_frames_without_plumbing() {
        let report = write_panic(
            tempfile::tempdir().unwrap().path(),
            Instant::now(),
            &details("boom"),
            BACKTRACE,
            vec![],
        )
        .unwrap();
        assert_eq!(
            report.signature(),
            "part::{{closure}} < daemon::run < putio_sync_cli::main"
        );
    }

    #[test]
    fn the_issue_url_is_encoded_and_capped() {
        let dir = tempfile::tempdir().unwrap();
        let long_log: Vec<String> = (0..50)
            .map(|i| format!("line {i} with spaces & symbols = é {}", "x".repeat(150)))
            .collect();
        let report = write_panic(
            dir.path(),
            Instant::now(),
            &details("a b&c=d"),
            BACKTRACE,
            long_log,
        )
        .unwrap();
        let issue = report.issue();
        assert!(issue.truncated);
        assert!(issue.url.chars().count() <= MAX_URL, "{}", issue.url.len());
        assert!(issue.url.starts_with(
            "https://github.com/devclinton/putio-sync/issues/new?template=crash.yml&labels=crash&title=Crash%3A%20"
        ));
        assert!(issue.url.is_ascii());
        assert!(!issue.url.contains(' ') && !issue.url.contains('\n'));
        assert!(issue.url.contains("Paste%20the%20copied%20full%20report"));
        assert!(issue.search_url.contains("is%3Aissue%20label%3Acrash"));
    }

    #[test]
    fn a_short_report_is_not_cut() {
        let report = write_panic(
            tempfile::tempdir().unwrap().path(),
            Instant::now(),
            &details("short"),
            "",
            vec![],
        )
        .unwrap();
        assert!(!report.issue().truncated);
        assert!(report.issue().url.ends_with(&encode(&report.to_text())));
    }

    #[test]
    fn ids_cannot_escape_the_folder() {
        assert!(valid_id("20261122T101010123-001"));
        assert!(!valid_id("../x"));
        assert!(!valid_id(""));
        assert!(get(Path::new("."), "../Cargo").is_none());
    }

    #[test]
    fn dismissing_hides_a_report_from_the_recent_list() {
        let dir = tempfile::tempdir().unwrap();
        let r = write_panic(dir.path(), Instant::now(), &details("x"), "", vec![]).unwrap();
        assert_eq!(recent_undismissed(dir.path()).len(), 1);
        assert!(dismiss(dir.path(), &r.id));
        assert!(recent_undismissed(dir.path()).is_empty());
        assert!(list(dir.path())[0].dismissed);
        assert!(!dismiss(dir.path(), "20200101T000000000-000"));
    }

    #[test]
    fn a_left_marker_without_a_newer_report_is_an_unclean_stop() {
        let dir = tempfile::tempdir().unwrap();
        let marker = |started: &str| {
            std::fs::write(
                dir.path().join(MARKER),
                serde_json::to_vec(&Marker {
                    started: started.into(),
                    version: "0.1.0".into(),
                    build: "docker".into(),
                })
                .unwrap(),
            )
            .unwrap();
        };
        // No marker: nothing
        assert!(note_unclean_stop(dir.path()).is_none());

        marker("2026-01-01T00:00:00.000Z");
        let report = note_unclean_stop(dir.path()).expect("an entry");
        assert_eq!(report.kind, Kind::Unclean);
        assert_eq!(report.version, "0.1.0");
        assert_eq!(report.build, "docker");
        assert!(report.message.contains("without a report"));
        assert!(!dir.path().join(MARKER).exists(), "the marker is spent");

        // A report newer than the marker explains the stop
        marker("2020-01-01T00:00:00.000Z");
        assert!(note_unclean_stop(dir.path()).is_none());
        assert_eq!(list(dir.path()).len(), 1);
    }
}
