//! Recent log lines kept in memory for the web UI's Logs page (feature 047)
//!
//! [`CaptureLayer`] sits right after the level filter, so it only sees the lines the level
//! in effect lets through: debug and trace are kept only while they are on. Each line is
//! masked by [`crate::log_redact`] before it is stored, then kept in a bounded ring
//! ([`set_capacity`]) and sent to live tails ([`subscribe`]).
//!
//! Nothing here may log: a line logged while capturing would be captured again.

use std::collections::{BTreeSet, VecDeque};
use std::fmt::Write as _;
use std::sync::{Arc, LazyLock, Mutex};

use serde::Serialize;
use tokio::sync::broadcast;
use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::Layer;

use crate::log_redact::{redact, redact_field};
use crate::logging::BUFFER_LINES_RANGE;

/// Most bytes of message and fields kept for one line
pub const MAX_LINE_BYTES: usize = 4096;

/// Lines a live tail can fall behind by before it misses some
const CHANNEL_SIZE: usize = 1024;

/// One captured log line, already masked
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LogLine {
    /// Starts at 1 and goes up by one for each line
    pub seq: u64,
    /// RFC 3339, UTC, milliseconds
    pub time: String,
    /// `ERROR`, `WARN`, `INFO`, `DEBUG` or `TRACE`
    pub level: String,
    /// Module path the line came from
    pub target: String,
    /// The message
    pub message: String,
    /// Other fields in logging order
    pub fields: Vec<(String, String)>,
    /// Message and fields were cut to [`MAX_LINE_BYTES`]
    pub truncated: bool,
}

impl LogLine {
    fn level(&self) -> Level {
        self.level.parse().unwrap_or(Level::TRACE)
    }

    /// Whether the line passes `query`'s filters (`limit` aside)
    pub fn matches(&self, query: &LogQuery) -> bool {
        if query.after.is_some_and(|after| self.seq <= after) {
            return false;
        }
        // tracing orders levels by verbosity: ERROR is the smallest
        if query.level.is_some_and(|level| self.level() > level) {
            return false;
        }
        if let Some(target) = query.target.as_deref().filter(|t| !t.is_empty()) {
            let under = self
                .target
                .strip_prefix(target)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with("::"));
            if !under {
                return false;
            }
        }
        if let Some(q) = query.q.as_deref().filter(|q| !q.is_empty()) {
            let q = q.to_lowercase();
            let hit = |s: &str| s.to_lowercase().contains(&q);
            if !hit(&self.message)
                && !hit(&self.target)
                && !self.fields.iter().any(|(k, v)| hit(k) || hit(v))
            {
                return false;
            }
        }
        true
    }

    /// The line as one line of text, as Copy and Download give it
    pub fn to_text(&self) -> String {
        let mut out = format!(
            "{} {:5} {}: {}",
            self.time, self.level, self.target, self.message
        );
        for (k, v) in &self.fields {
            let _ = write!(out, " {}={}", k, v);
        }
        out
    }
}

/// Which kept lines to return
#[derive(Debug, Clone, Default)]
pub struct LogQuery {
    /// Only lines after this seq
    pub after: Option<u64>,
    /// This level or more severe
    pub level: Option<Level>,
    /// This target or one under it (`a` matches `a` and `a::b`, not `ab`)
    pub target: Option<String>,
    /// Text in the message, target, or a field name or value, ignoring case
    pub q: Option<String>,
    /// The newest this many matches (default and most: the capacity)
    pub limit: Option<usize>,
}

/// Counts for the API
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct LogStats {
    /// Newest seq kept (0 when empty)
    pub last_seq: u64,
    /// Oldest seq kept (0 when empty)
    pub oldest_seq: u64,
    /// Lines kept at most
    pub capacity: usize,
    /// Lines pushed out since start
    pub evicted: u64,
}

struct Ring {
    lines: VecDeque<Arc<LogLine>>,
    capacity: usize,
    next_seq: u64,
    evicted: u64,
}

impl Ring {
    /// Drop the oldest lines over capacity; returns how many went.
    fn trim(&mut self) -> u64 {
        let mut gone = 0;
        while self.lines.len() > self.capacity {
            self.lines.pop_front();
            gone += 1;
        }
        self.evicted += gone;
        gone
    }
}

/// A bounded ring of recent lines plus a channel for live tails
pub struct LogBuffer {
    ring: Mutex<Ring>,
    tx: broadcast::Sender<Arc<LogLine>>,
}

impl LogBuffer {
    /// An empty buffer of `capacity` lines (clamped to the allowed range)
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(CHANNEL_SIZE);
        LogBuffer {
            ring: Mutex::new(Ring {
                lines: VecDeque::new(),
                capacity: clamp(capacity),
                next_seq: 1,
                evicted: 0,
            }),
            tx,
        }
    }

    fn ring(&self) -> std::sync::MutexGuard<'_, Ring> {
        self.ring.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Change the capacity; a smaller one drops the oldest lines at once.
    pub fn set_capacity(&self, capacity: usize) {
        let gone = {
            let mut ring = self.ring();
            ring.capacity = clamp(capacity);
            ring.trim()
        };
        record_evicted(gone);
    }

    /// Keep a line, giving it the next seq, and send it to live tails.
    pub fn push(&self, mut line: LogLine) -> u64 {
        let (seq, gone) = {
            let mut ring = self.ring();
            line.seq = ring.next_seq;
            ring.next_seq += 1;
            let line = Arc::new(line);
            ring.lines.push_back(line.clone());
            let gone = ring.trim();
            // Sent under the lock so tails get lines in seq order; no receiver is fine
            let _ = self.tx.send(line.clone());
            (line.seq, gone)
        };
        record_evicted(gone);
        seq
    }

    /// The newest `limit` matching lines, oldest first
    pub fn query(&self, query: &LogQuery) -> Vec<Arc<LogLine>> {
        let ring = self.ring();
        let limit = query.limit.unwrap_or(ring.capacity).min(ring.capacity);
        let mut out: Vec<Arc<LogLine>> = ring
            .lines
            .iter()
            .rev()
            .filter(|line| line.matches(query))
            .take(limit)
            .cloned()
            .collect();
        out.reverse();
        out
    }

    /// Distinct targets of the kept lines, sorted
    pub fn targets(&self) -> Vec<String> {
        let ring = self.ring();
        let set: BTreeSet<&str> = ring.lines.iter().map(|l| l.target.as_str()).collect();
        set.into_iter().map(str::to_string).collect()
    }

    /// New lines as they are kept
    pub fn subscribe(&self) -> broadcast::Receiver<Arc<LogLine>> {
        self.tx.subscribe()
    }

    /// Counts for the API
    pub fn stats(&self) -> LogStats {
        let ring = self.ring();
        LogStats {
            last_seq: ring.lines.back().map_or(0, |l| l.seq),
            oldest_seq: ring.lines.front().map_or(0, |l| l.seq),
            capacity: ring.capacity,
            evicted: ring.evicted,
        }
    }
}

fn clamp(capacity: usize) -> usize {
    capacity.clamp(*BUFFER_LINES_RANGE.start(), *BUFFER_LINES_RANGE.end())
}

fn record_evicted(n: u64) {
    if n > 0 {
        putio_sync_telemetry::logs::record_dropped("evicted", n);
    }
}

static BUFFER: LazyLock<LogBuffer> = LazyLock::new(|| LogBuffer::new(2000));

/// The process-wide buffer the Logs page reads
pub fn buffer() -> &'static LogBuffer {
    &BUFFER
}

/// Resize the process-wide buffer (`[logging] buffer_lines`).
pub fn set_capacity(capacity: usize) {
    BUFFER.set_capacity(capacity);
}

/// Live tail of the process-wide buffer
pub fn subscribe() -> broadcast::Receiver<Arc<LogLine>> {
    BUFFER.subscribe()
}

/// Lines from the process-wide buffer
pub fn query(query: &LogQuery) -> Vec<Arc<LogLine>> {
    BUFFER.query(query)
}

/// Targets in the process-wide buffer
pub fn targets() -> Vec<String> {
    BUFFER.targets()
}

/// Counts for the process-wide buffer
pub fn stats() -> LogStats {
    BUFFER.stats()
}

/// The `tracing` layer that fills a [`LogBuffer`]
pub struct CaptureLayer {
    buffer: &'static LogBuffer,
}

impl CaptureLayer {
    /// A layer that fills the process-wide buffer
    pub fn new() -> Self {
        CaptureLayer { buffer: buffer() }
    }

    /// A layer that fills `buffer` (tests)
    pub fn with_buffer(buffer: &'static LogBuffer) -> Self {
        CaptureLayer { buffer }
    }
}

impl Default for CaptureLayer {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Default)]
struct Fields {
    message: String,
    fields: Vec<(String, String)>,
}

impl Visit for Fields {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.add(field, value.to_string());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.add(field, format!("{:?}", value));
    }
}

impl Fields {
    fn add(&mut self, field: &Field, value: String) {
        if field.name() == "message" {
            self.message = redact(&value);
        } else {
            let value = redact_field(field.name(), &value);
            self.fields.push((field.name().to_string(), value));
        }
    }
}

/// `s` cut to at most `max` bytes, on a character boundary
fn cut(s: &mut String, max: usize) -> bool {
    if s.len() <= max {
        return false;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
    true
}

impl<S: Subscriber> Layer<S> for CaptureLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        let mut visited = Fields::default();
        event.record(&mut visited);

        let mut budget = MAX_LINE_BYTES;
        let mut truncated = cut(&mut visited.message, budget);
        budget -= visited.message.len();
        let mut fields = Vec::with_capacity(visited.fields.len());
        for (name, mut value) in visited.fields {
            if budget <= name.len() {
                truncated = true;
                break;
            }
            budget -= name.len();
            truncated |= cut(&mut value, budget);
            budget -= value.len();
            fields.push((name, value));
        }

        let meta = event.metadata();
        self.buffer.push(LogLine {
            seq: 0,
            time: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            level: meta.level().to_string(),
            target: meta.target().to_string(),
            message: visited.message,
            fields,
            truncated,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::layer::SubscriberExt;

    fn line(level: &str, target: &str, message: &str) -> LogLine {
        LogLine {
            seq: 0,
            time: String::new(),
            level: level.to_string(),
            target: target.to_string(),
            message: message.to_string(),
            fields: vec![("file_id".to_string(), "42".to_string())],
            truncated: false,
        }
    }

    fn leaked(capacity: usize) -> &'static LogBuffer {
        Box::leak(Box::new(LogBuffer::new(capacity)))
    }

    #[test]
    fn the_ring_drops_the_oldest_and_counts_it() {
        let buffer = LogBuffer::new(100);
        for i in 0..105 {
            buffer.push(line("INFO", "a", &format!("n{i}")));
        }
        let stats = buffer.stats();
        assert_eq!(stats.evicted, 5);
        assert_eq!((stats.oldest_seq, stats.last_seq), (6, 105));
        assert_eq!(buffer.query(&LogQuery::default()).len(), 100);
    }

    #[test]
    fn evicted_lines_are_counted_on_the_metric() {
        let metric = || {
            putio_sync_telemetry::global()
                .encode()
                .lines()
                .find(|l| l.starts_with("putio_sync_log_lines_dropped_total{reason=\"evicted\"}"))
                .and_then(|l| l.rsplit(' ').next()?.parse::<u64>().ok())
                .unwrap()
        };
        let before = metric();
        let buffer = LogBuffer::new(100);
        for _ in 0..103 {
            buffer.push(line("INFO", "a", "x"));
        }
        assert!(metric() >= before + 3);
    }

    #[test]
    fn shrinking_drops_lines_and_capacity_is_clamped() {
        let buffer = LogBuffer::new(1000);
        for _ in 0..300 {
            buffer.push(line("INFO", "a", "x"));
        }
        buffer.set_capacity(150);
        assert_eq!(buffer.query(&LogQuery::default()).len(), 150);
        assert_eq!(buffer.stats().evicted, 150);
        buffer.set_capacity(10);
        assert_eq!(buffer.stats().capacity, 100);
        buffer.set_capacity(1_000_000);
        assert_eq!(buffer.stats().capacity, 50_000);
    }

    #[test]
    fn each_filter_applies() {
        let buffer = LogBuffer::new(100);
        buffer.push(line("ERROR", "putio_sync_core::download", "Disk full"));
        buffer.push(line("WARN", "putio_sync_core", "slow"));
        buffer.push(line("DEBUG", "putio_sync_core_extra", "detail"));
        buffer.push(line("INFO", "putio_sync_hooks::service", "ran"));

        let messages = |q: LogQuery| {
            buffer
                .query(&q)
                .iter()
                .map(|l| l.message.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(
            messages(LogQuery {
                level: Some(Level::WARN),
                ..Default::default()
            }),
            ["Disk full", "slow"]
        );
        assert_eq!(
            messages(LogQuery {
                target: Some("putio_sync_core".into()),
                ..Default::default()
            }),
            ["Disk full", "slow"]
        );
        assert_eq!(
            messages(LogQuery {
                q: Some("DISK".into()),
                ..Default::default()
            }),
            ["Disk full"]
        );
        assert_eq!(
            messages(LogQuery {
                q: Some("file_id".into()),
                limit: Some(2),
                ..Default::default()
            }),
            ["detail", "ran"]
        );
        assert_eq!(
            messages(LogQuery {
                after: Some(3),
                ..Default::default()
            }),
            ["ran"]
        );
        assert_eq!(
            buffer.targets(),
            [
                "putio_sync_core",
                "putio_sync_core::download",
                "putio_sync_core_extra",
                "putio_sync_hooks::service"
            ]
        );
    }

    #[test]
    fn the_layer_captures_fields_masks_secrets_and_cuts_long_lines() {
        let buffer = leaked(100);
        let subscriber = tracing_subscriber::registry()
            .with(tracing_subscriber::filter::LevelFilter::INFO)
            .with(CaptureLayer::with_buffer(buffer));
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(
                file_id = 42,
                api_key = "k-123456",
                "Fetched with token=abc123"
            );
            tracing::debug!("not kept at info");
            tracing::warn!(detail = %"y".repeat(5000), "long");
        });

        let lines = buffer.query(&LogQuery::default());
        assert_eq!(lines.len(), 2, "{lines:?}");
        let first = &lines[0];
        assert_eq!(first.level, "INFO");
        assert_eq!(first.message, "Fetched with token=********");
        assert_eq!(
            first.fields,
            [
                ("file_id".to_string(), "42".to_string()),
                ("api_key".to_string(), "********".to_string())
            ]
        );
        assert!(!first.truncated);
        let long = &lines[1];
        assert!(long.truncated);
        assert!(
            long.message.len()
                + long
                    .fields
                    .iter()
                    .map(|(k, v)| k.len() + v.len())
                    .sum::<usize>()
                <= MAX_LINE_BYTES
        );
    }

    #[test]
    fn subscribers_get_new_lines_in_order() {
        let buffer = LogBuffer::new(100);
        let mut rx = buffer.subscribe();
        buffer.push(line("INFO", "a", "one"));
        buffer.push(line("INFO", "a", "two"));
        assert_eq!(rx.try_recv().unwrap().seq, 1);
        assert_eq!(rx.try_recv().unwrap().message, "two");
    }
}
