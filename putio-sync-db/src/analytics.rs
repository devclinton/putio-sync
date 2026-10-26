//! Pre-aggregated download statistics for the Stats page (feature 006)
//!
//! One range-filtered scan of `download_history` joined to `download_sources`, folded in
//! Rust by [`aggregate`] into every section the page draws (research R3).

use super::error::{tracked, DbResult};
use super::models::Origin;
use super::stats::StatsRepository;
use chrono::{DateTime, Duration, Utc};
use putio_sync_telemetry::DbOp;
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::collections::HashMap;

/// Most bars the bandwidth chart shows
const MAX_BUCKETS: i64 = 120;
/// Monday 1970-01-05 00:00 UTC; the epoch itself is a Thursday
const FIRST_MONDAY: i64 = 4 * 86_400;
/// Groups kept before the rest is summed into one "other" row
const TOP_EXTENSIONS: usize = 8;
const TOP_TRACKERS: usize = 10;
const TOP_FOLDERS: usize = 10;
/// Longest accepted `ext`/`tracker` and `watch`/`folder` filter values
const MAX_SHORT_FILTER: usize = 64;
const MAX_PATH_FILTER: usize = 512;

/// The date range the page asks for
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangePreset {
    Day,
    Week,
    Month,
    Quarter,
    Year,
    All,
}

impl RangePreset {
    /// The query-string value
    pub fn as_str(self) -> &'static str {
        match self {
            RangePreset::Day => "24h",
            RangePreset::Week => "7d",
            RangePreset::Month => "30d",
            RangePreset::Quarter => "90d",
            RangePreset::Year => "1y",
            RangePreset::All => "all",
        }
    }

    /// The range's length, or `None` for everything
    fn length(self) -> Option<Duration> {
        match self {
            RangePreset::Day => Some(Duration::hours(24)),
            RangePreset::Week => Some(Duration::days(7)),
            RangePreset::Month => Some(Duration::days(30)),
            RangePreset::Quarter => Some(Duration::days(90)),
            RangePreset::Year => Some(Duration::days(365)),
            RangePreset::All => None,
        }
    }
}

impl std::str::FromStr for RangePreset {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "24h" => Ok(RangePreset::Day),
            "7d" => Ok(RangePreset::Week),
            "30d" => Ok(RangePreset::Month),
            "90d" => Ok(RangePreset::Quarter),
            "1y" => Ok(RangePreset::Year),
            "all" => Ok(RangePreset::All),
            _ => Err("range must be one of 24h, 7d, 30d, 90d, 1y, all".to_string()),
        }
    }
}

/// Width of one bar in the bandwidth chart
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BucketSize {
    Hour,
    SixHours,
    Day,
    Week,
    FourWeeks,
}

impl BucketSize {
    const ALL: [BucketSize; 5] = [
        BucketSize::Hour,
        BucketSize::SixHours,
        BucketSize::Day,
        BucketSize::Week,
        BucketSize::FourWeeks,
    ];

    /// The label in the response
    pub fn as_str(self) -> &'static str {
        match self {
            BucketSize::Hour => "1h",
            BucketSize::SixHours => "6h",
            BucketSize::Day => "1d",
            BucketSize::Week => "1w",
            BucketSize::FourWeeks => "4w",
        }
    }

    /// Length in seconds
    pub fn seconds(self) -> i64 {
        match self {
            BucketSize::Hour => 3_600,
            BucketSize::SixHours => 21_600,
            BucketSize::Day => 86_400,
            BucketSize::Week => 604_800,
            BucketSize::FourWeeks => 2_419_200,
        }
    }

    /// Start of the bucket holding `t`, aligned to local time (`tz` seconds east of UTC).
    /// Weeks and four-week blocks start on Monday.
    fn align(self, t: i64, tz: i64) -> i64 {
        let local = t + tz;
        let aligned = match self {
            BucketSize::Hour | BucketSize::SixHours | BucketSize::Day => {
                local.div_euclid(self.seconds()) * self.seconds()
            }
            BucketSize::Week | BucketSize::FourWeeks => {
                let week = BucketSize::Week.seconds();
                (local - FIRST_MONDAY).div_euclid(week) * week + FIRST_MONDAY
            }
        };
        aligned - tz
    }

    /// The smallest size giving at most [`MAX_BUCKETS`] buckets over `[since, until]`
    fn choose(since: i64, until: i64, tz: i64) -> BucketSize {
        BucketSize::ALL
            .into_iter()
            .find(|b| (until - b.align(since, tz)) / b.seconds() < MAX_BUCKETS)
            .unwrap_or(BucketSize::FourWeeks)
    }
}

/// File category, from a fixed extension table (spec Assumptions)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Video,
    Audio,
    Archive,
    Image,
    Document,
    Subtitle,
    Other,
}

impl Category {
    /// The category of a (lower-case) extension
    pub fn of(extension: Option<&str>) -> Category {
        match extension.unwrap_or("") {
            "mkv" | "mp4" | "avi" | "mov" | "m4v" | "ts" | "webm" | "wmv" | "mpg" | "mpeg"
            | "m2ts" => Category::Video,
            "mp3" | "flac" | "m4a" | "aac" | "ogg" | "opus" | "wav" | "m4b" => Category::Audio,
            "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "iso" => Category::Archive,
            "jpg" | "jpeg" | "png" | "gif" | "webp" => Category::Image,
            "pdf" | "epub" | "txt" | "nfo" | "doc" | "docx" | "mobi" => Category::Document,
            "srt" | "sub" | "ass" | "ssa" | "idx" | "vtt" => Category::Subtitle,
            _ => Category::Other,
        }
    }
}

impl std::str::FromStr for Category {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "video" => Ok(Category::Video),
            "audio" => Ok(Category::Audio),
            "archive" => Ok(Category::Archive),
            "image" => Ok(Category::Image),
            "document" => Ok(Category::Document),
            "subtitle" => Ok(Category::Subtitle),
            "other" => Ok(Category::Other),
            _ => Err(
                "category must be one of video, audio, archive, image, document, subtitle, other"
                    .to_string(),
            ),
        }
    }
}

/// The `source` filter: a transfer type, or no recorded source
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFilter {
    Torrent,
    Url,
    Other,
    Unknown,
}

impl SourceFilter {
    fn key(self) -> &'static str {
        match self {
            SourceFilter::Torrent => "torrent",
            SourceFilter::Url => "url",
            SourceFilter::Other => "other",
            SourceFilter::Unknown => "unknown",
        }
    }
}

impl std::str::FromStr for SourceFilter {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "torrent" => Ok(SourceFilter::Torrent),
            "url" => Ok(SourceFilter::Url),
            "other" => Ok(SourceFilter::Other),
            "unknown" => Ok(SourceFilter::Unknown),
            _ => Err("source must be one of torrent, url, other, unknown".to_string()),
        }
    }
}

/// `GET /stats/summary` query parameters as received; empty values count as absent
#[derive(Debug, Clone, Default, Deserialize)]
pub struct StatsParams {
    pub range: Option<String>,
    pub tz: Option<String>,
    pub category: Option<String>,
    pub ext: Option<String>,
    pub source: Option<String>,
    pub tracker: Option<String>,
    pub private: Option<String>,
    pub origin: Option<String>,
    pub watch: Option<String>,
    pub folder: Option<String>,
}

/// A validated stats request
#[derive(Debug, Clone, PartialEq)]
pub struct StatsQuery {
    pub range: RangePreset,
    /// Minutes east of UTC, −840..=840
    pub tz_offset_minutes: i32,
    pub category: Option<Category>,
    /// Lower case; `(none)` matches files without an extension. Wins over `category`.
    pub extension: Option<String>,
    pub source: Option<SourceFilter>,
    /// Lower case
    pub tracker: Option<String>,
    pub private: Option<bool>,
    pub origin: Option<Origin>,
    pub watch_folder: Option<String>,
    pub putio_folder: Option<String>,
}

impl Default for StatsQuery {
    fn default() -> Self {
        Self {
            range: RangePreset::Week,
            tz_offset_minutes: 0,
            category: None,
            extension: None,
            source: None,
            tracker: None,
            private: None,
            origin: None,
            watch_folder: None,
            putio_folder: None,
        }
    }
}

fn present(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.is_empty())
}

fn bounded(value: Option<String>, name: &str, max: usize) -> Result<Option<String>, String> {
    match present(value) {
        Some(v) if v.chars().count() > max => {
            Err(format!("{} must be at most {} characters", name, max))
        }
        v => Ok(v),
    }
}

impl TryFrom<StatsParams> for StatsQuery {
    type Error = String;

    fn try_from(p: StatsParams) -> Result<Self, Self::Error> {
        let tz_offset_minutes = match present(p.tz) {
            None => 0,
            Some(tz) => tz
                .parse::<i32>()
                .ok()
                .filter(|m| m.abs() <= 840)
                .ok_or("tz must be minutes east of UTC, between -840 and 840")?,
        };
        let private = match present(p.private).as_deref() {
            None => None,
            Some("true") => Some(true),
            Some("false") => Some(false),
            Some(_) => return Err("private must be true or false".to_string()),
        };
        Ok(StatsQuery {
            range: present(p.range)
                .map(|r| r.parse())
                .transpose()?
                .unwrap_or(RangePreset::Week),
            tz_offset_minutes,
            category: present(p.category).map(|c| c.parse()).transpose()?,
            extension: bounded(p.ext, "ext", MAX_SHORT_FILTER)?.map(|e| e.to_lowercase()),
            source: present(p.source).map(|s| s.parse()).transpose()?,
            tracker: bounded(p.tracker, "tracker", MAX_SHORT_FILTER)?.map(|t| t.to_lowercase()),
            private,
            origin: present(p.origin)
                .map(|o| {
                    o.parse()
                        .map_err(|_| "origin must be one of watch, manual, putio, unknown")
                })
                .transpose()?,
            watch_folder: bounded(p.watch, "watch", MAX_PATH_FILTER)?,
            putio_folder: bounded(p.folder, "folder", MAX_PATH_FILTER)?,
        })
    }
}

/// One finished download with its recorded source, if any
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StatsRow {
    /// Finish time, UTC epoch seconds
    pub finish: i64,
    pub file_size: i64,
    pub extension: Option<String>,
    pub duration_seconds: Option<i64>,
    pub source_type: Option<String>,
    pub tracker_host: Option<String>,
    pub is_private: Option<bool>,
    pub origin: Option<String>,
    pub watch_folder: Option<String>,
    pub putio_folder: Option<String>,
}

impl StatsRow {
    fn ext_key(&self) -> &str {
        self.extension.as_deref().unwrap_or(NO_EXTENSION)
    }

    fn source_key(&self) -> &str {
        self.source_type.as_deref().unwrap_or("unknown")
    }

    fn origin_key(&self) -> &str {
        self.origin.as_deref().unwrap_or("unknown")
    }

    fn is_torrent(&self) -> bool {
        self.source_type.as_deref() == Some("torrent")
    }

    fn private(&self) -> bool {
        self.is_private.unwrap_or(false)
    }

    /// Does the row pass the type and source filters?
    fn matches(&self, q: &StatsQuery) -> bool {
        let type_ok = match (&q.extension, q.category) {
            (Some(ext), _) => self.ext_key() == ext,
            (None, Some(cat)) => Category::of(self.extension.as_deref()) == cat,
            (None, None) => true,
        };
        type_ok
            && q.source.is_none_or(|s| self.source_key() == s.key())
            && q.tracker
                .as_deref()
                .is_none_or(|t| self.tracker_host.as_deref() == Some(t))
            && q.private.is_none_or(|p| match p {
                true => self.private(),
                false => self.is_torrent() && !self.private(),
            })
            && q.origin.is_none_or(|o| self.origin_key() == o.as_str())
            && q.watch_folder
                .as_deref()
                .is_none_or(|w| self.watch_folder.as_deref() == Some(w))
            && q.putio_folder
                .as_deref()
                .is_none_or(|f| self.putio_folder.as_deref() == Some(f))
    }
}

/// How a missing extension is shown and filtered
pub const NO_EXTENSION: &str = "(none)";

/// `range` section
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RangeInfo {
    pub range: &'static str,
    pub since: Option<i64>,
    pub until: i64,
    pub bucket: &'static str,
    pub bucket_seconds: i64,
    pub tz: i32,
}

/// `totals` section
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Totals {
    pub files: i64,
    pub bytes: i64,
    pub seconds: i64,
    /// Bytes of downloads with a duration, over that duration
    pub avg_bps: i64,
}

/// One bar of the bandwidth chart
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SeriesPoint {
    pub start: i64,
    pub bytes: i64,
    pub files: i64,
}

/// `heatmap` section: 7 rows (Monday first) × 24 hours, local time
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Heatmap {
    pub files: Vec<Vec<i64>>,
    pub bytes: Vec<Vec<i64>>,
}

/// Files and bytes for one extension
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExtensionCount {
    pub ext: String,
    pub files: i64,
    pub bytes: i64,
}

/// One category of the file types breakdown
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TypeGroup {
    pub category: Category,
    pub files: i64,
    pub bytes: i64,
    pub extensions: Vec<ExtensionCount>,
}

/// Files and bytes for one group of a breakdown
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GroupCount {
    pub key: String,
    pub files: i64,
    pub bytes: i64,
}

/// `sources` section
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SourceBreakdown {
    pub types: Vec<GroupCount>,
    pub trackers: Vec<GroupCount>,
    pub privacy: Vec<GroupCount>,
    pub origins: Vec<GroupCount>,
    pub watch_folders: Vec<GroupCount>,
    pub folders: Vec<GroupCount>,
}

/// `options` section: filter menu entries, before the type and source filters
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct FilterOptions {
    pub extensions: Vec<String>,
    pub trackers: Vec<String>,
    pub watch_folders: Vec<String>,
    pub folders: Vec<String>,
}

/// Everything the Stats page draws
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StatsSummary {
    pub range: RangeInfo,
    pub totals: Totals,
    pub series: Vec<SeriesPoint>,
    pub heatmap: Heatmap,
    pub types: Vec<TypeGroup>,
    pub sources: SourceBreakdown,
    pub options: FilterOptions,
}

/// Files and bytes per key, in first-seen order until sorted
#[derive(Default)]
struct Tally(HashMap<String, (i64, i64)>);

impl Tally {
    fn add(&mut self, key: &str, bytes: i64) {
        let entry = self.0.entry(key.to_string()).or_default();
        entry.0 += 1;
        entry.1 += bytes;
    }

    /// Groups by bytes, largest first (ties by key)
    fn sorted(self) -> Vec<GroupCount> {
        let mut groups: Vec<GroupCount> = self
            .0
            .into_iter()
            .map(|(key, (files, bytes))| GroupCount { key, files, bytes })
            .collect();
        groups.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.key.cmp(&b.key)));
        groups
    }

    /// The `top` largest groups, then the rest summed under `rest`
    fn top(self, top: usize, rest: &str) -> Vec<GroupCount> {
        let mut groups = self.sorted();
        if groups.len() > top {
            let tail = groups.split_off(top);
            groups.push(GroupCount {
                key: rest.to_string(),
                files: tail.iter().map(|g| g.files).sum(),
                bytes: tail.iter().map(|g| g.bytes).sum(),
            });
        }
        groups
    }
}

/// Distinct values, sorted by name
fn names<'r>(values: impl Iterator<Item = Option<&'r str>>) -> Vec<String> {
    let mut out: Vec<String> = values.flatten().map(str::to_string).collect();
    out.sort();
    out.dedup();
    out
}

/// Spread `size` bytes evenly over `[start, end]`, clipped to `[since, until]`, into the
/// buckets of `series` (research R2). A zero-length span puts every byte at `end`.
fn spread(
    series: &mut [f64],
    origin: i64,
    bucket: i64,
    (since, until): (i64, i64),
    (start, end): (i64, i64),
    size: i64,
) {
    let index = |t: i64| ((t - origin).div_euclid(bucket)).clamp(0, series.len() as i64 - 1);
    if end <= start {
        series[index(end) as usize] += size as f64;
        return;
    }
    let per_second = size as f64 / (end - start) as f64;
    let (from, to) = (start.max(since), end.min(until));
    if to <= from {
        return;
    }
    for i in index(from)..=index(to - 1) {
        let lo = (origin + i * bucket).max(from);
        let hi = (origin + (i + 1) * bucket).min(to);
        if hi > lo {
            series[i as usize] += per_second * (hi - lo) as f64;
        }
    }
}

/// Fold range rows into every section of the summary. `rows` are the downloads that
/// finished in `[since, until]` (all of them for the `all` range), before any type or
/// source filter.
pub fn aggregate(q: &StatsQuery, rows: &[StatsRow], now: DateTime<Utc>) -> StatsSummary {
    let until = now.timestamp();
    let tz = i64::from(q.tz_offset_minutes) * 60;
    let since = match q.range.length() {
        Some(length) => Some(until - length.num_seconds()),
        None => rows.iter().map(|r| r.finish).min(),
    };
    let bucket = since.map_or(BucketSize::Day, |s| BucketSize::choose(s, until, tz));
    let b = bucket.seconds();

    let mut extensions = Tally::default();
    for row in rows {
        extensions.add(row.ext_key(), row.file_size);
    }
    let mut extension_options = extensions.sorted();
    extension_options.sort_by(|a, b| b.files.cmp(&a.files).then_with(|| a.key.cmp(&b.key)));
    let options = FilterOptions {
        extensions: extension_options.into_iter().map(|g| g.key).collect(),
        trackers: names(rows.iter().map(|r| r.tracker_host.as_deref())),
        watch_folders: names(rows.iter().map(|r| r.watch_folder.as_deref())),
        folders: names(rows.iter().map(|r| r.putio_folder.as_deref())),
    };

    let origin = since.map(|s| bucket.align(s, tz));
    let mut bytes_series =
        vec![0.0; origin.map_or(0, |o| ((until - o).div_euclid(b) + 1) as usize)];
    let mut files_series = vec![0i64; bytes_series.len()];

    let mut totals = Totals::default();
    let mut timed_bytes = 0i64;
    let mut heat_files = vec![vec![0i64; 24]; 7];
    let mut heat_bytes = vec![vec![0i64; 24]; 7];
    let mut categories: HashMap<Category, Tally> = HashMap::new();
    let (mut types, mut trackers, mut privacy) =
        (Tally::default(), Tally::default(), Tally::default());
    let (mut origins, mut watch_folders, mut folders) =
        (Tally::default(), Tally::default(), Tally::default());

    for row in rows.iter().filter(|r| r.matches(q)) {
        let duration = row.duration_seconds.unwrap_or(0).max(0);
        totals.files += 1;
        totals.bytes += row.file_size;
        totals.seconds += duration;
        if duration > 0 {
            timed_bytes += row.file_size;
        }

        if let (Some(since), Some(origin)) = (since, origin) {
            let span = (row.finish - duration, row.finish);
            spread(
                &mut bytes_series,
                origin,
                b,
                (since, until),
                span,
                row.file_size,
            );
            let i = (row.finish - origin).div_euclid(b);
            if (0..files_series.len() as i64).contains(&i) {
                files_series[i as usize] += 1;
            }
        }

        let local = row.finish + tz;
        let dow = (local.div_euclid(86_400) + 3).rem_euclid(7) as usize;
        let hour = (local.rem_euclid(86_400) / 3_600) as usize;
        heat_files[dow][hour] += 1;
        heat_bytes[dow][hour] += row.file_size;

        categories
            .entry(Category::of(row.extension.as_deref()))
            .or_default()
            .add(row.ext_key(), row.file_size);

        types.add(row.source_key(), row.file_size);
        if row.is_torrent() {
            let tracker = row.tracker_host.as_deref().unwrap_or("no tracker");
            trackers.add(tracker, row.file_size);
            let key = if row.private() { "private" } else { "public" };
            privacy.add(key, row.file_size);
        } else if row.source_type.is_none() {
            privacy.add("unknown", row.file_size);
        }
        origins.add(row.origin_key(), row.file_size);
        if row.origin_key() == "watch" {
            if let Some(folder) = row.watch_folder.as_deref() {
                watch_folders.add(folder, row.file_size);
            }
        }
        folders.add(
            row.putio_folder.as_deref().unwrap_or("unknown"),
            row.file_size,
        );
    }
    if totals.seconds > 0 {
        totals.avg_bps = timed_bytes / totals.seconds;
    }

    let series = bytes_series
        .iter()
        .zip(&files_series)
        .enumerate()
        .map(|(i, (bytes, files))| SeriesPoint {
            start: origin.unwrap_or(0) + i as i64 * b,
            bytes: bytes.round() as i64,
            files: *files,
        })
        .collect();

    let mut types_section: Vec<TypeGroup> = categories
        .into_iter()
        .map(|(category, tally)| {
            let extensions: Vec<ExtensionCount> = tally
                .top(TOP_EXTENSIONS, "…other")
                .into_iter()
                .map(|g| ExtensionCount {
                    ext: g.key,
                    files: g.files,
                    bytes: g.bytes,
                })
                .collect();
            TypeGroup {
                category,
                files: extensions.iter().map(|e| e.files).sum(),
                bytes: extensions.iter().map(|e| e.bytes).sum(),
                extensions,
            }
        })
        .collect();
    types_section.sort_by(|a, b| {
        b.bytes
            .cmp(&a.bytes)
            .then_with(|| (a.category as u8).cmp(&(b.category as u8)))
    });

    StatsSummary {
        range: RangeInfo {
            range: q.range.as_str(),
            since,
            until,
            bucket: bucket.as_str(),
            bucket_seconds: b,
            tz: q.tz_offset_minutes,
        },
        totals,
        series,
        heatmap: Heatmap {
            files: heat_files,
            bytes: heat_bytes,
        },
        types: types_section,
        sources: SourceBreakdown {
            types: types.sorted(),
            trackers: tail_last(trackers.top(TOP_TRACKERS, "other trackers"), "no tracker"),
            privacy: privacy.sorted(),
            origins: origins.sorted(),
            watch_folders: watch_folders.sorted(),
            folders: tail_last(folders.top(TOP_FOLDERS, "other folders"), "unknown"),
        },
        options,
    }
}

/// Move the `key` group, if present, to the end
fn tail_last(mut groups: Vec<GroupCount>, key: &str) -> Vec<GroupCount> {
    if let Some(i) = groups.iter().position(|g| g.key == key) {
        let group = groups.remove(i);
        groups.push(group);
    }
    groups
}

impl StatsRepository<'_> {
    /// The Stats page summary for one range and filter set
    pub async fn summary(&self, q: &StatsQuery, now: DateTime<Utc>) -> DbResult<StatsSummary> {
        let format = |t: DateTime<Utc>| t.format("%Y-%m-%d %H:%M:%S").to_string();
        let since = q.range.length().map(|length| format(now - length));
        let rows = sqlx::query(
            r#"
            SELECT CAST(strftime('%s', h.timestamp) AS INTEGER) AS finish,
                   h.file_size, h.extension, h.duration_seconds,
                   s.source_type, s.tracker_host, s.is_private, s.origin,
                   s.watch_folder, s.putio_folder
            FROM download_history h
            LEFT JOIN download_sources s ON s.file_id = h.file_id
            WHERE h.action = 'downloaded'
              AND (?1 IS NULL OR h.timestamp >= ?1)
              AND h.timestamp <= ?2
            "#,
        )
        .bind(since)
        .bind(format(now))
        .fetch_all(self.pool)
        .await
        .map_err(tracked(DbOp::History, "Failed to read stats rows"))?;

        let rows: Vec<StatsRow> = rows
            .into_iter()
            .map(|row| StatsRow {
                finish: row.get("finish"),
                file_size: row.get("file_size"),
                extension: row.get("extension"),
                duration_seconds: row.get("duration_seconds"),
                source_type: row.get("source_type"),
                tracker_host: row.get("tracker_host"),
                is_private: row.get("is_private"),
                origin: row.get("origin"),
                watch_folder: row.get("watch_folder"),
                putio_folder: row.get("putio_folder"),
            })
            .collect();

        Ok(aggregate(q, &rows, now))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{DownloadSource, SourceType};
    use crate::Database;
    use tempfile::TempDir;

    /// Wednesday 2026-10-21 12:00:00 UTC
    const NOW: i64 = 1_792_584_000;

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(NOW, 0).unwrap()
    }

    fn query(range: RangePreset) -> StatsQuery {
        StatsQuery {
            range,
            ..StatsQuery::default()
        }
    }

    fn row(finish: i64, size: i64, ext: Option<&str>, duration: Option<i64>) -> StatsRow {
        StatsRow {
            finish,
            file_size: size,
            extension: ext.map(str::to_string),
            duration_seconds: duration,
            ..StatsRow::default()
        }
    }

    fn torrent(mut r: StatsRow, tracker: Option<&str>, private: bool) -> StatsRow {
        r.source_type = Some("torrent".into());
        r.tracker_host = tracker.map(str::to_string);
        r.is_private = Some(private);
        r.origin = Some("putio".into());
        r
    }

    fn params(pairs: &[(&str, &str)]) -> Result<StatsQuery, String> {
        let mut p = StatsParams::default();
        for (k, v) in pairs {
            let v = Some(v.to_string());
            match *k {
                "range" => p.range = v,
                "tz" => p.tz = v,
                "category" => p.category = v,
                "ext" => p.ext = v,
                "source" => p.source = v,
                "tracker" => p.tracker = v,
                "private" => p.private = v,
                "origin" => p.origin = v,
                "watch" => p.watch = v,
                "folder" => p.folder = v,
                _ => unreachable!(),
            }
        }
        StatsQuery::try_from(p)
    }

    #[test]
    fn test_now_is_a_wednesday_noon() {
        assert_eq!(
            now().format("%a %H:%M").to_string(),
            "Wed 12:00",
            "test fixture"
        );
    }

    #[test]
    fn test_params_validation() {
        assert_eq!(params(&[]).unwrap(), StatsQuery::default());
        assert_eq!(params(&[("range", "")]).unwrap().range, RangePreset::Week);
        assert_eq!(params(&[("range", "1y")]).unwrap().range, RangePreset::Year);
        assert_eq!(params(&[("tz", "-300")]).unwrap().tz_offset_minutes, -300);
        assert_eq!(
            params(&[("ext", "MKV")]).unwrap().extension.as_deref(),
            Some("mkv")
        );
        assert_eq!(
            params(&[("private", "false")]).unwrap().private,
            Some(false)
        );
        assert_eq!(
            params(&[("origin", "watch")]).unwrap().origin,
            Some(Origin::Watch)
        );
        assert_eq!(
            params(&[("folder", "TV Shows")])
                .unwrap()
                .putio_folder
                .as_deref(),
            Some("TV Shows")
        );

        for bad in [
            ("range", "2d"),
            ("tz", "abc"),
            ("tz", "841"),
            ("category", "movies"),
            ("source", "dht"),
            ("private", "yes"),
            ("origin", "upload"),
        ] {
            let err = params(&[bad]).unwrap_err();
            assert!(err.starts_with(bad.0), "{:?} → {}", bad, err);
        }
        assert!(params(&[("ext", &"x".repeat(65))]).is_err());
        assert!(params(&[("watch", &"x".repeat(513))]).is_err());
    }

    #[test]
    fn test_bucket_choice_per_range() {
        for (range, bucket, min, max) in [
            (RangePreset::Day, BucketSize::Hour, 24, 25),
            (RangePreset::Week, BucketSize::SixHours, 28, 29),
            (RangePreset::Month, BucketSize::Day, 30, 31),
            (RangePreset::Quarter, BucketSize::Day, 90, 91),
            (RangePreset::Year, BucketSize::Week, 53, 54),
        ] {
            let s = aggregate(&query(range), &[], now());
            assert_eq!(s.range.bucket, bucket.as_str(), "{:?}", range);
            assert_eq!(s.range.bucket_seconds, bucket.seconds());
            assert!(
                (min..=max).contains(&s.series.len()),
                "{:?}: {} buckets",
                range,
                s.series.len()
            );
            assert_eq!(
                s.range.since,
                Some(NOW - range.length().unwrap().num_seconds())
            );
        }
    }

    #[test]
    fn test_all_range_fits_oldest_download() {
        let rows = [row(NOW - 3 * 365 * 86_400, 1, None, None)];
        let s = aggregate(&query(RangePreset::All), &rows, now());
        assert_eq!(s.range.since, Some(NOW - 3 * 365 * 86_400));
        assert_eq!(s.range.bucket, "4w");
        assert!(s.series.len() <= MAX_BUCKETS as usize);

        let empty = aggregate(&query(RangePreset::All), &[], now());
        assert_eq!(empty.range.since, None);
        assert!(empty.series.is_empty());
    }

    #[test]
    fn test_buckets_align_to_local_time() {
        // UTC−5: local midnight is 05:00 UTC
        let q = StatsQuery {
            range: RangePreset::Month,
            tz_offset_minutes: -300,
            ..StatsQuery::default()
        };
        let s = aggregate(&q, &[], now());
        for point in &s.series {
            assert_eq!((point.start - 5 * 3_600).rem_euclid(86_400), 0);
        }

        // Weeks start on local Monday 00:00
        let q = StatsQuery {
            range: RangePreset::Year,
            tz_offset_minutes: 120,
            ..StatsQuery::default()
        };
        let s = aggregate(&q, &[], now());
        let first = DateTime::from_timestamp(s.series[0].start + 7_200, 0).unwrap();
        assert_eq!(first.format("%a %H:%M").to_string(), "Mon 00:00");
        assert!(s.series[0].start <= s.range.since.unwrap());
    }

    #[test]
    fn test_empty_history() {
        let s = aggregate(&query(RangePreset::Week), &[], now());
        assert_eq!(s.totals, Totals::default());
        assert!(s.series.iter().all(|p| p.bytes == 0 && p.files == 0));
        assert_eq!(s.heatmap.files.len(), 7);
        assert!(s.heatmap.files.iter().all(|r| r.len() == 24));
        assert!(s.types.is_empty());
        assert_eq!(s.sources, SourceBreakdown::default());
        assert_eq!(s.options, FilterOptions::default());
    }

    #[test]
    fn test_totals() {
        let rows = [
            row(NOW - 3_600, 1_000, Some("mkv"), Some(10)),
            row(NOW - 7_200, 500, Some("srt"), None),
        ];
        let t = aggregate(&query(RangePreset::Day), &rows, now()).totals;
        assert_eq!(t.files, 2);
        assert_eq!(t.bytes, 1_500);
        assert_eq!(t.seconds, 10);
        assert_eq!(t.avg_bps, 100);
    }

    #[test]
    fn test_series_spreads_bytes_across_buckets() {
        // 7 200 bytes over 09:00–11:00 UTC: 3 600 in each hourly bucket
        let finish = NOW - 3_600;
        let rows = [row(finish, 7_200, Some("mkv"), Some(7_200))];
        let s = aggregate(&query(RangePreset::Day), &rows, now());
        let at = |t: i64| s.series.iter().find(|p| p.start == t).unwrap().clone();
        assert_eq!(at(NOW - 3 * 3_600).bytes, 3_600);
        assert_eq!(at(NOW - 2 * 3_600).bytes, 3_600);
        assert_eq!(at(finish).bytes, 0);
        // The file counts where it finished
        assert_eq!(at(finish).files, 1);
        assert_eq!(s.series.iter().map(|p| p.files).sum::<i64>(), 1);
        assert_eq!(s.series.iter().map(|p| p.bytes).sum::<i64>(), 7_200);
    }

    #[test]
    fn test_series_clips_to_range_but_totals_do_not() {
        let since = NOW - 86_400;
        // Half of this download happened before the range starts
        let rows = [row(since + 1_800, 3_600, Some("mkv"), Some(3_600))];
        let s = aggregate(&query(RangePreset::Day), &rows, now());
        assert_eq!(s.series.iter().map(|p| p.bytes).sum::<i64>(), 1_800);
        assert_eq!(s.totals.bytes, 3_600);
    }

    #[test]
    fn test_zero_duration_puts_bytes_at_finish() {
        let finish = NOW - 5 * 3_600 + 60;
        for duration in [None, Some(0)] {
            let rows = [row(finish, 900, None, duration)];
            let s = aggregate(&query(RangePreset::Day), &rows, now());
            let bucket = s.series.iter().find(|p| p.bytes > 0).unwrap();
            assert_eq!(bucket.start, NOW - 5 * 3_600);
            assert_eq!(bucket.bytes, 900);
            assert_eq!(bucket.files, 1);
        }
    }

    #[test]
    fn test_heatmap_uses_local_time() {
        // Wednesday 03:00 UTC is Tuesday 22:00 at UTC−5
        let rows = [row(NOW - 9 * 3_600, 10, None, None)];
        let utc = aggregate(&query(RangePreset::Week), &rows, now()).heatmap;
        assert_eq!(utc.files[2][3], 1);
        assert_eq!(utc.bytes[2][3], 10);

        let q = StatsQuery {
            tz_offset_minutes: -300,
            ..StatsQuery::default()
        };
        let local = aggregate(&q, &rows, now()).heatmap;
        assert_eq!(local.files[1][22], 1);
        assert_eq!(local.files.iter().flatten().sum::<i64>(), 1);
    }

    #[test]
    fn test_category_mapping() {
        assert_eq!(Category::of(Some("mkv")), Category::Video);
        assert_eq!(Category::of(Some("flac")), Category::Audio);
        assert_eq!(Category::of(Some("rar")), Category::Archive);
        assert_eq!(Category::of(Some("png")), Category::Image);
        assert_eq!(Category::of(Some("nfo")), Category::Document);
        assert_eq!(Category::of(Some("srt")), Category::Subtitle);
        assert_eq!(Category::of(Some("exe")), Category::Other);
        assert_eq!(Category::of(None), Category::Other);
    }

    #[test]
    fn test_types_sorted_with_top_extensions() {
        let mut rows = vec![
            row(NOW - 60, 5, Some("srt"), None),
            row(NOW - 60, 7, None, None),
        ];
        for (i, ext) in [
            "mkv", "mp4", "avi", "mov", "m4v", "ts", "webm", "wmv", "mpg", "mpeg",
        ]
        .into_iter()
        .enumerate()
        {
            rows.push(row(NOW - 60, 1_000 - i as i64, Some(ext), None));
        }
        let s = aggregate(&query(RangePreset::Day), &rows, now());

        let categories: Vec<Category> = s.types.iter().map(|t| t.category).collect();
        assert_eq!(
            categories,
            [Category::Video, Category::Other, Category::Subtitle]
        );
        let video = &s.types[0];
        assert_eq!(video.files, 10);
        assert_eq!(video.extensions.len(), 9);
        assert_eq!(video.extensions[0].ext, "mkv");
        assert_eq!(video.extensions[8].ext, "…other");
        assert_eq!(video.extensions[8].files, 2);
        assert_eq!(s.types[1].extensions[0].ext, NO_EXTENSION);
    }

    #[test]
    fn test_type_filters_and_options() {
        let rows = [
            row(NOW - 60, 100, Some("mkv"), None),
            row(NOW - 60, 100, Some("mkv"), None),
            row(NOW - 120, 10, Some("srt"), None),
            row(NOW - 180, 1, None, None),
        ];
        let by_cat = StatsQuery {
            category: Some(Category::Subtitle),
            ..StatsQuery::default()
        };
        let s = aggregate(&by_cat, &rows, now());
        assert_eq!(s.totals.files, 1);
        assert_eq!(s.series.iter().map(|p| p.files).sum::<i64>(), 1);
        assert_eq!(s.heatmap.files.iter().flatten().sum::<i64>(), 1);
        assert_eq!(s.options.extensions, ["mkv", NO_EXTENSION, "srt"]);

        let by_ext = StatsQuery {
            extension: Some(NO_EXTENSION.into()),
            category: Some(Category::Video),
            ..StatsQuery::default()
        };
        let s = aggregate(&by_ext, &rows, now());
        assert_eq!(s.totals.bytes, 1, "ext wins over category");
    }

    #[test]
    fn test_sources_breakdown_and_filters() {
        let mut url = row(NOW - 60, 50, Some("zip"), None);
        url.source_type = Some("url".into());
        url.origin = Some("manual".into());
        url.putio_folder = Some("(root)".into());
        let mut watched = torrent(
            row(NOW - 60, 400, Some("mkv"), None),
            Some("tracker.example.org"),
            true,
        );
        watched.origin = Some("watch".into());
        watched.watch_folder = Some("/watch/tv".into());
        watched.putio_folder = Some("TV".into());
        let rows = [
            watched,
            torrent(
                row(NOW - 60, 300, Some("mkv"), None),
                Some("open.example.com"),
                false,
            ),
            torrent(row(NOW - 60, 200, Some("mkv"), None), None, false),
            url,
            row(NOW - 60, 1, None, None),
        ];
        let s = aggregate(&StatsQuery::default(), &rows, now());

        let keys = |groups: &[GroupCount]| groups.iter().map(|g| g.key.clone()).collect::<Vec<_>>();
        assert_eq!(keys(&s.sources.types), ["torrent", "url", "unknown"]);
        assert_eq!(s.sources.types[0].files, 3);
        assert_eq!(
            keys(&s.sources.trackers),
            ["tracker.example.org", "open.example.com", "no tracker"]
        );
        assert_eq!(keys(&s.sources.privacy), ["public", "private", "unknown"]);
        assert_eq!(s.sources.privacy[0].bytes, 500);
        assert_eq!(
            keys(&s.sources.origins),
            ["putio", "watch", "manual", "unknown"]
        );
        assert_eq!(keys(&s.sources.watch_folders), ["/watch/tv"]);
        assert_eq!(keys(&s.sources.folders), ["TV", "(root)", "unknown"]);
        assert_eq!(
            s.options.trackers,
            ["open.example.com", "tracker.example.org"]
        );
        assert_eq!(s.options.watch_folders, ["/watch/tv"]);
        assert_eq!(s.options.folders, ["(root)", "TV"]);

        let total = |q: StatsQuery| aggregate(&q, &rows, now()).totals.bytes;
        let q = StatsQuery::default;
        assert_eq!(
            total(StatsQuery {
                source: Some(SourceFilter::Unknown),
                ..q()
            }),
            1
        );
        assert_eq!(
            total(StatsQuery {
                source: Some(SourceFilter::Url),
                ..q()
            }),
            50
        );
        assert_eq!(
            total(StatsQuery {
                tracker: Some("open.example.com".into()),
                ..q()
            }),
            300
        );
        assert_eq!(
            total(StatsQuery {
                private: Some(true),
                ..q()
            }),
            400
        );
        assert_eq!(
            total(StatsQuery {
                private: Some(false),
                ..q()
            }),
            500
        );
        assert_eq!(
            total(StatsQuery {
                origin: Some(Origin::Unknown),
                ..q()
            }),
            1
        );
        assert_eq!(
            total(StatsQuery {
                origin: Some(Origin::Manual),
                ..q()
            }),
            50
        );
        assert_eq!(
            total(StatsQuery {
                watch_folder: Some("/watch/tv".into()),
                ..q()
            }),
            400
        );
        assert_eq!(
            total(StatsQuery {
                putio_folder: Some("(root)".into()),
                ..q()
            }),
            50
        );

        // Options ignore the filters
        let filtered = aggregate(
            &StatsQuery {
                source: Some(SourceFilter::Url),
                ..q()
            },
            &rows,
            now(),
        );
        assert_eq!(filtered.options, s.options);
    }

    #[test]
    fn test_trackers_and_folders_keep_top_ten() {
        let rows: Vec<StatsRow> = (0..12)
            .map(|i| {
                let mut r = torrent(
                    row(NOW - 60, 100 - i, None, None),
                    Some(&format!("t{:02}.example", i)),
                    false,
                );
                r.putio_folder = Some(format!("F{:02}", i));
                r
            })
            .collect();
        let s = aggregate(&StatsQuery::default(), &rows, now());
        assert_eq!(s.sources.trackers.len(), 11);
        assert_eq!(s.sources.trackers[10].key, "other trackers");
        assert_eq!(s.sources.trackers[10].files, 2);
        assert_eq!(s.sources.folders.len(), 11);
        assert_eq!(s.sources.folders[10].key, "other folders");
    }

    #[tokio::test]
    async fn test_summary_reads_history_and_sources() {
        let dir = TempDir::new().unwrap();
        let db = Database::new(&dir.path().join("test.db")).await.unwrap();
        let history = db.history();
        let at = |secs: i64| DateTime::from_timestamp(NOW - secs, 0).unwrap();

        history
            .insert_download_at(1, "a.mkv", 1_000, Some(10), None, at(3_600))
            .await
            .unwrap();
        history
            .insert_download_at(2, "b.srt", 10, None, None, at(2 * 86_400))
            .await
            .unwrap();
        history
            .insert_download_at(3, "old.mkv", 5, None, None, at(30 * 86_400))
            .await
            .unwrap();
        db.sources()
            .record(
                1,
                &DownloadSource {
                    transfer_id: Some(7),
                    source_type: Some(SourceType::Torrent),
                    tracker_host: Some("tracker.example.org".into()),
                    is_private: true,
                    origin: Origin::Putio,
                    watch_folder: None,
                    putio_folder: Some("TV".into()),
                },
            )
            .await
            .unwrap();

        let stats = StatsRepository::new(db.pool());
        let s = stats.summary(&StatsQuery::default(), now()).await.unwrap();
        assert_eq!(s.totals.files, 2);
        assert_eq!(s.totals.bytes, 1_010);
        assert_eq!(s.sources.trackers[0].key, "tracker.example.org");
        assert_eq!(s.sources.privacy[0].key, "private");
        assert_eq!(s.sources.origins[0].key, "putio");

        let all = StatsQuery {
            range: RangePreset::All,
            ..StatsQuery::default()
        };
        let s = stats.summary(&all, now()).await.unwrap();
        assert_eq!(s.totals.files, 3);
        assert_eq!(s.range.since, Some(NOW - 30 * 86_400));
    }
}
