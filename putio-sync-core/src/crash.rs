//! Crash reports (feature 058, part 1)
//!
//! A panic hook ([`install`]) writes one JSON file per crash to `crash-reports/` next to the
//! config file: version, build, OS and arch, uptime, the thread, the panic message, a
//! backtrace and the last [`LOG_LINES`] log lines (without their fields), all masked by
//! [`crate::log_redact`] and scrubbed of paths, IP addresses and long tokens. At most
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
    /// The process ended: a panic on the main thread, a run found stopped at the next start, or
    /// a stop without a report. `false` is an internal error the process survived (a panic in
    /// a task the runtime caught).
    #[serde(default = "yes")]
    pub stopped: bool,
}

fn yes() -> bool {
    true
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

/// Mask secrets and the home folder in `text`, line by line, then scrub what a crash report
/// must not carry: paths, IP addresses and long tokens (see [`scrub`])
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
            scrub(&redact(&line))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// [`clean`] for a log line: also drops the values of name-like fields, quoted strings and
/// words that look like file names, which the Logs page shows but a report must not carry
fn clean_log(line: &str) -> String {
    scrub_log_message(&clean(line))
}

const SOURCE_MARKS: [&str; 3] = ["/src/", ".cargo/registry", "/rustc/"];

/// Whether `chunk` (no spaces) names source code: a `.rs` file, optionally `:line:col`, or a
/// path through `/src/`, the cargo registry or the rustc tree
fn is_source(chunk: &str) -> bool {
    if SOURCE_MARKS.iter().any(|m| chunk.contains(m)) {
        return true;
    }
    let mut s = chunk.trim_end_matches([')', ',', ';']);
    for _ in 0..2 {
        if let Some((head, tail)) = s.rsplit_once(':') {
            if !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) {
                s = head;
            }
        }
    }
    s.ends_with(".rs")
}

/// Replace absolute paths, IP addresses and long tokens. Source-code locations stay so
/// backtraces remain useful; `127.0.0.1` and `::1` stay.
fn scrub(text: &str) -> String {
    let text = scrub_paths(text);
    let text = map_chunks(&text, scrub_ipv6);
    let text = map_chunks(&text, scrub_ipv4);
    map_chunks(&text, scrub_tokens)
}

/// Run `f` on each run of non-space characters except source-code locations
fn map_chunks(text: &str, f: fn(&str) -> String) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chunk = String::new();
    let flush = |chunk: &mut String, out: &mut String| {
        if !chunk.is_empty() {
            if is_source(chunk) {
                out.push_str(chunk);
            } else {
                out.push_str(&f(chunk));
            }
            chunk.clear();
        }
    };
    for c in text.chars() {
        if c.is_whitespace() {
            flush(&mut chunk, &mut out);
            out.push(c);
        } else {
            chunk.push(c);
        }
    }
    flush(&mut chunk, &mut out);
    out
}

fn path_boundary(prev: Option<char>) -> bool {
    match prev {
        None => true,
        Some(c) => c.is_whitespace() || "\"'`(=[<:,".contains(c),
    }
}

/// Absolute Unix (`/a/b`) and Windows (`C:\a`) paths become `<path>`. A path runs to the
/// closing quote when it opens right after one, else to the end of the line, because file
/// names hold spaces.
fn scrub_paths(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let prev = if i == 0 { None } else { Some(chars[i - 1]) };
        let next = chars.get(i + 1).copied();
        let unix = c == '/'
            && next.is_some_and(|n| n.is_alphanumeric() || "._~-@".contains(n))
            && path_boundary(prev);
        let windows = c.is_ascii_alphabetic()
            && next == Some(':')
            && matches!(chars.get(i + 2), Some('\\') | Some('/'))
            && path_boundary(prev);
        if !(unix || windows) {
            out.push(c);
            i += 1;
            continue;
        }
        let word_end = (i..chars.len())
            .find(|&j| chars[j].is_whitespace())
            .unwrap_or(chars.len());
        let word: String = chars[i..word_end].iter().collect();
        if is_source(&word) {
            out.push_str(&word);
            i = word_end;
            continue;
        }
        let quote = prev.filter(|p| "\"'`".contains(*p));
        let mut end = chars.len();
        if let Some(q) = quote {
            if let Some(k) = (i..chars.len()).find(|&j| chars[j] == q) {
                end = k;
            }
        }
        if quote.is_none() {
            let rest: String = chars[i..].iter().collect();
            if let Some(k) = rest.find(" (os error") {
                end = i + rest[..k].chars().count();
            }
        }
        out.push_str("<path>");
        i = end;
    }
    out
}

fn is_ip_char(c: char) -> bool {
    c.is_ascii_hexdigit() || c == ':' || c == '.'
}

/// IPv6 literals become `<ip>`; `::1` stays
fn scrub_ipv6(chunk: &str) -> String {
    let chars: Vec<char> = chunk.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let prev_ok = i == 0 || !(chars[i - 1].is_alphanumeric() || chars[i - 1] == '_');
        if prev_ok && is_ip_char(chars[i]) {
            let mut j = i;
            while j < chars.len() && is_ip_char(chars[j]) {
                j += 1;
            }
            let run: String = chars[i..j].iter().collect();
            let mut hit = None;
            let mut cand = run.as_str();
            // Trailing punctuation is not part of the address
            loop {
                if cand.matches(':').count() >= 2
                    && cand.bytes().any(|b| b.is_ascii_digit())
                    && cand.parse::<std::net::Ipv6Addr>().is_ok()
                {
                    hit = Some(cand.len());
                    break;
                }
                match cand.strip_suffix(['.', ':']) {
                    Some(c) if !c.is_empty() => cand = c,
                    _ => break,
                }
            }
            if let Some(len) = hit {
                if &run[..len] == "::1" {
                    out.push_str("::1");
                } else {
                    out.push_str("<ip>");
                }
                out.push_str(&run[len..]);
            } else {
                out.push_str(&run);
            }
            i = j;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// IPv4 literals (and a `:port` after one) become `<ip>`; `127.0.0.1` stays
fn scrub_ipv4(chunk: &str) -> String {
    let b = chunk.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    let mut copied = 0;
    while i < b.len() {
        let prev_ok =
            i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'.' || b[i - 1] == b'_');
        if b[i].is_ascii_digit() && prev_ok {
            let mut j = i;
            let mut octets = 0;
            let mut ok = true;
            loop {
                let s = j;
                while j < b.len() && b[j].is_ascii_digit() {
                    j += 1;
                }
                if j == s || j - s > 3 || chunk[s..j].parse::<u16>().map_or(true, |n| n > 255) {
                    ok = false;
                    break;
                }
                octets += 1;
                if octets == 4 {
                    break;
                }
                if j < b.len() && b[j] == b'.' {
                    j += 1;
                } else {
                    ok = false;
                    break;
                }
            }
            let after_ok = j >= b.len() || !(b[j].is_ascii_alphanumeric() || b[j] == b'_');
            let more_digits = j + 1 < b.len() && b[j] == b'.' && b[j + 1].is_ascii_digit();
            if ok && after_ok && !more_digits {
                let ip = &chunk[i..j];
                let mut end = j;
                if ip != "127.0.0.1" {
                    if b.get(end) == Some(&b':') {
                        let digits = b[end + 1..]
                            .iter()
                            .take_while(|d| d.is_ascii_digit())
                            .count();
                        end += if digits > 0 { 1 + digits } else { 0 };
                    }
                    out.push_str(&chunk[copied..i]);
                    out.push_str("<ip>");
                    copied = end;
                }
                i = end.max(j);
                continue;
            }
        }
        i += 1;
    }
    out.push_str(&chunk[copied..]);
    out
}

/// Runs of 32 or more of `[A-Za-z0-9_-]` that hold a digit become `<redacted>`
fn scrub_tokens(chunk: &str) -> String {
    let mut out = String::new();
    let mut run = String::new();
    let flush = |run: &mut String, out: &mut String| {
        if run.len() >= 32 && run.bytes().any(|b| b.is_ascii_digit()) {
            out.push_str("<redacted>");
        } else {
            out.push_str(run);
        }
        run.clear();
    };
    for c in chunk.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            run.push(c);
        } else {
            flush(&mut run, &mut out);
            out.push(c);
        }
    }
    flush(&mut run, &mut out);
    out
}

/// Keys whose value names a file, a place or a host
fn sensitive_key(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    matches!(
        k.as_str(),
        "name" | "filename" | "file" | "path" | "ip" | "addr" | "url" | "dest" | "target_directory"
    ) || k.ends_with("_path")
        || k.ends_with("_name")
}

const FILE_EXTENSIONS: [&str; 24] = [
    "mkv", "mp4", "avi", "mov", "m4v", "ts", "wmv", "srt", "sub", "ass", "nfo", "jpg", "jpeg",
    "png", "zip", "rar", "7z", "iso", "mp3", "flac", "m4a", "epub", "pdf", "part",
];

/// In a log line: `key=value` for name-like keys, quoted strings and file names become
/// placeholders. Plain words stay: only what looks like a name goes.
fn scrub_log_message(line: &str) -> String {
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let prev = if i == 0 { None } else { Some(chars[i - 1]) };
        // Quoted strings
        if (c == '"' || c == '\'') && prev.is_none_or(|p| p.is_whitespace() || "(=[:,".contains(p))
        {
            if let Some(k) = (i + 1..chars.len())
                .find(|&j| chars[j] == c && chars.get(j + 1).is_none_or(|n| !n.is_alphanumeric()))
            {
                out.push(c);
                out.push_str("<…>");
                out.push(c);
                i = k + 1;
                continue;
            }
        }
        // key=value
        if prev.is_none_or(|p| p.is_whitespace() || p == '(' || p == ',') && c.is_alphabetic() {
            let kend = (i..chars.len())
                .find(|&j| !(chars[j].is_alphanumeric() || chars[j] == '_' || chars[j] == '.'))
                .unwrap_or(chars.len());
            let key: String = chars[i..kend].iter().collect();
            if chars.get(kend) == Some(&'=') && sensitive_key(&key) {
                let vstart = kend + 1;
                let vend = match chars.get(vstart) {
                    Some(&q) if q == '"' || q == '\'' => (vstart + 1..chars.len())
                        .find(|&j| chars[j] == q)
                        .map_or(chars.len(), |j| j + 1),
                    _ => (vstart..chars.len())
                        .find(|&j| chars[j].is_whitespace())
                        .unwrap_or(chars.len()),
                };
                out.push_str(&key);
                out.push_str("=<…>");
                i = vend;
                continue;
            }
        }
        // A word with a media or archive extension
        if prev.is_none_or(|p| p.is_whitespace()) && !c.is_whitespace() {
            let wend = (i..chars.len())
                .find(|&j| chars[j].is_whitespace())
                .unwrap_or(chars.len());
            let word: String = chars[i..wend].iter().collect();
            let bare = word.trim_end_matches(['.', ',', ';', ')', ']']);
            let ext = bare.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
            if bare.contains('.')
                && ext.is_some_and(|e| FILE_EXTENSIONS.contains(&e.as_str()))
                && !bare.starts_with("<")
            {
                out.push_str("<name>");
                i += bare.chars().count();
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
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
            // Fields (file names, addresses) are left out: only level, time, target, message
            let _ = tx.send(
                lines
                    .iter()
                    .map(|l| format!("{} {:5} {}: {}", l.time, l.level, l.target, l.message))
                    .collect::<Vec<_>>(),
            );
        });
    if spawned.is_err() {
        return Vec::new();
    }
    rx.recv_timeout(Duration::from_secs(1))
        .map(|lines| lines.iter().map(|l| clean_log(l)).collect())
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
        log: log.iter().map(|l| clean_log(l)).take(LOG_LINES).collect(),
        dismissed: false,
        // Only a panic on the main thread is known to end the process now; a later look at
        // the leftover marker (`note_unclean_stop`) can show others did
        stopped: details.thread == "main",
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

/// Remove the running marker now, for a restart that skips destructors (an update rollback)
pub fn clear_marker() {
    if let Some(state) = STATE.get() {
        let _ = std::fs::remove_file(state.dir.join(MARKER));
    }
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
    let _ = std::fs::remove_file(dir.join(MARKER));
    // The newest report since that start explains the stop: the process ended with it
    if let Some(mut newest) = list(dir).into_iter().find(|r| r.time >= marker.started) {
        if !newest.stopped {
            newest.stopped = true;
            let _ = write_file(dir, &newest);
        }
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
        stopped: true,
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
    /// `Crash: <signature>` when putio-sync stopped, `Panic: <signature>` when it kept running
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
    let prefix = if report.stopped { "Crash" } else { "Panic" };
    let title: String = format!("{prefix}: {}", report.signature())
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

    fn report_text(message: &str, backtrace: &str, log: Vec<String>) -> String {
        let dir = tempfile::tempdir().unwrap();
        let report = write_panic(
            dir.path(),
            Instant::now(),
            &details(message),
            backtrace,
            log,
        )
        .unwrap();
        std::fs::read_to_string(dir.path().join(file_name(&report.id))).unwrap()
    }

    #[test]
    fn log_fields_and_names_do_not_reach_a_report() {
        let log = vec![
            "2026-11-22T10:20:30.123Z WARN putio_sync_download: Download failed for \"Some Film (2020).mkv\" name=Some.Film.2020.mkv filename=\"My Show S01E01.mkv\" ip=192.168.1.5 dest_path=/volume1/media/x".to_string(),
            "2026-11-22T10:20:31.000Z INFO putio_sync_download: finished Some.Film.2020.mkv ok".to_string(),
        ];
        let text = report_text("x", "", log);
        for leak in [
            "Some Film",
            "Some.Film",
            "My Show",
            "192.168",
            "/volume1",
            "mkv",
        ] {
            assert!(!text.contains(leak), "{leak}: {text}");
        }
        assert!(text.contains("WARN"));
        assert!(text.contains("putio_sync_download"));
        assert!(text.contains("Download failed for"));
        assert!(text.contains("2026-11-22T10:20:30.123Z"));
    }

    #[test]
    fn library_paths_become_a_placeholder() {
        let out = clean("failed to open /volume1/media/Movies/Some Film (2020)/file.mkv");
        assert_eq!(out, "failed to open <path>");
        let out = clean("rename \"/volume1/media/Some Film/a.mkv\" failed, retrying");
        assert_eq!(out, "rename \"<path>\" failed, retrying");
        let out = clean(r"cannot write C:\Users\bob\Videos\Some Film.mkv");
        assert_eq!(out, "cannot write <path>");
    }

    #[test]
    fn ip_addresses_become_a_placeholder() {
        assert_eq!(
            clean("auth failed ip=192.168.1.5 again"),
            "auth failed ip=<ip> again"
        );
        assert_eq!(clean("from 10.0.0.7:51234 denied"), "from <ip> denied");
        assert_eq!(
            clean("peer 2001:db8:85a3::8a2e:370:7334 and fe80::1 gone"),
            "peer <ip> and <ip> gone"
        );
        assert_eq!(
            clean("listening on [::1]:8080 and 127.0.0.1"),
            "listening on [::1]:8080 and 127.0.0.1"
        );
        assert_eq!(
            clean("putio-sync 0.9.3 and 1.2.3"),
            "putio-sync 0.9.3 and 1.2.3"
        );
        assert_eq!(
            clean("core::panicking::panic a::b"),
            "core::panicking::panic a::b"
        );
    }

    #[test]
    fn an_io_error_panic_message_loses_its_path() {
        let err = std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "No such file or directory (os error 2) at /volume1/media/Movies/Some Film (2020)/file.mkv",
        );
        let message = format!("called `Result::unwrap()` on an `Err` value: {err}");
        let text = report_text(&message, "", vec![]);
        assert!(!text.contains("volume1"), "{text}");
        assert!(!text.contains("Some Film"), "{text}");
        assert!(text.contains("<path>"));
        assert!(text.contains("No such file or directory"));
    }

    #[test]
    fn source_locations_and_versions_stay() {
        let bt = "   2: putio_sync_download::fetch::part\n             at /root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tokio-1.40.0/src/runtime/task.rs:10:5\n   3: app::run\n             at ./putio-sync-core/src/daemon.rs:42:9\n   4: x\n             at /rustc/0123456789abcdef0123456789abcdef01234567/library/core/src/panicking.rs:72:5\n   5: y\n             at /build/putio-sync/putio-sync-core/src/crash.rs:1:1\n";
        let text = report_text("boom in v0.9.3", bt, vec![]);
        assert!(
            text.contains("tokio-1.40.0/src/runtime/task.rs:10:5"),
            "{text}"
        );
        assert!(text.contains("./putio-sync-core/src/daemon.rs:42:9"));
        assert!(text.contains("/rustc/0123456789abcdef0123456789abcdef01234567/library"));
        assert!(text.contains("/build/putio-sync/putio-sync-core/src/crash.rs:1:1"));
        assert!(text.contains("boom in v0.9.3"));
        assert!(text.contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn long_tokens_are_redacted_but_short_words_stay() {
        let token = "A1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q7r8S9t0";
        assert_eq!(token.len(), 40);
        let out = clean(&format!("bad value {token} for header"));
        assert_eq!(out, "bad value <redacted> for header");
        assert_eq!(
            clean("putio_sync_download_fetch_part_closure_handler_long_name"),
            "putio_sync_download_fetch_part_closure_handler_long_name"
        );
        assert_eq!(clean("sha 0123456789abcdef"), "sha 0123456789abcdef");
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

    #[test]
    fn a_panic_the_process_survived_is_an_internal_error_until_a_leftover_marker_says_otherwise() {
        let dir = tempfile::tempdir().unwrap();
        let task = PanicDetails {
            thread: "tokio-runtime-worker".into(),
            ..details("task panicked")
        };
        let report = write_panic(dir.path(), Instant::now(), &task, BACKTRACE, vec![]).unwrap();
        assert!(!report.stopped);
        assert!(report.issue().title.starts_with("Panic: "));
        let main =
            write_panic(dir.path(), Instant::now(), &details("x"), BACKTRACE, vec![]).unwrap();
        assert!(main.stopped);
        assert!(main.issue().title.starts_with("Crash: "));

        // A clean stop removed the marker: the report stays an internal error
        assert!(note_unclean_stop(dir.path()).is_none());
        assert!(!get(dir.path(), &report.id).unwrap().stopped);

        // The marker was left behind after the panic: the run did end
        let marker = Marker {
            started: "2020-01-01T00:00:00.000Z".into(),
            version: "0.1.0".into(),
            build: "docker".into(),
        };
        std::fs::write(
            dir.path().join(MARKER),
            serde_json::to_vec(&marker).unwrap(),
        )
        .unwrap();
        let only = tempfile::tempdir().unwrap();
        let lone = write_panic(only.path(), Instant::now(), &task, "", vec![]).unwrap();
        std::fs::write(
            only.path().join(MARKER),
            serde_json::to_vec(&marker).unwrap(),
        )
        .unwrap();
        assert!(note_unclean_stop(only.path()).is_none(), "no extra entry");
        assert!(get(only.path(), &lone.id).unwrap().stopped);
        assert!(list(only.path()).len() == 1);
    }
}
