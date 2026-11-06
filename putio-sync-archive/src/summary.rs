//! `archive summary`: daily and monthly usage CSVs (contracts/archive-cli.md,
//! data-model.md "Usage summary row")
//!
//! Every figure is a sum, count, mean or max over stored values taken in a fixed order,
//! and the output holds no clock reading, so the same inputs give the same bytes.

use crate::error::{ArchiveError, ArchiveResult};
use crate::files::{write_atomic, ArchiveDir};
use crate::prom::{PromClient, Series};
use crate::POINTS_PER_DAY;
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use putio_sync_db::Database;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Write;
use std::path::PathBuf;

/// Seconds one 5-minute point stands for
const POINT_SECONDS: f64 = 300.0;

const STATE: &str = "putio_sync:activity_state:5m";
const CPU_AVG: &str = "putio_sync:process_cpu_usage:avg5m";
const CPU_MAX: &str = "putio_sync:process_cpu_usage:max5m";
const MEM_AVG: &str = "putio_sync:process_resident_memory_bytes:avg5m";
const MEM_MAX: &str = "putio_sync:process_resident_memory_bytes:max5m";
const BYTES: &str = "putio_sync:downloaded_bytes:increase5m";
const START_TIME: &str = "putio_sync:start_time_seconds:max5m";
const BUILD_INFO: &str = "putio_sync:build_info:max5m";

/// Options of `archive summary`
pub struct SummaryOptions {
    pub dir: PathBuf,
    /// The live database, as the daemon uses it
    pub db_path: PathBuf,
    /// Without it, the resource columns are blank
    pub prometheus: Option<String>,
    pub instance: Option<String>,
    /// Default: the first day in the download history
    pub from: Option<NaiveDate>,
    /// Default: today
    pub to: Option<NaiveDate>,
    pub require_manifest: bool,
    pub now: DateTime<Utc>,
}

/// Write `summary/usage-daily.csv`, `usage-monthly.csv` and `usage-latest-month.txt`
pub async fn run(opts: &SummaryOptions, out: &mut dyn Write) -> ArchiveResult<()> {
    let dir = ArchiveDir::open(&opts.dir, opts.require_manifest)?;
    let to = opts.to.unwrap_or_else(|| opts.now.date_naive());

    let db = Database::open_read_only(&opts.db_path).await.map_err(|e| {
        ArchiveError::database(&format!("cannot open {}", opts.db_path.display()), e)
    })?;
    let totals = db.history().daily_totals(opts.from, Some(to)).await;
    let _ = db.close().await;
    let totals =
        totals.map_err(|e| ArchiveError::database("cannot read the download history", e))?;

    let from = opts
        .from
        .or_else(|| totals.first().map(|t| t.day))
        .unwrap_or(to);
    if from > to {
        return Err(ArchiveError::args(
            "bad range",
            format!("--from {from} is after --to {to}"),
        ));
    }
    let totals: HashMap<NaiveDate, _> = totals.into_iter().map(|t| (t.day, t)).collect();

    let source = match &opts.prometheus {
        Some(url) => {
            let client = PromClient::new(url)?;
            let instance = client.instance(opts.instance.as_deref()).await?;
            Some((client, instance))
        }
        None => None,
    };
    let mut days = Vec::new();
    for day in from.iter_days().take_while(|d| *d <= to) {
        let series = match &source {
            Some((client, instance)) => client.fetch_day(instance, day).await?,
            None => Vec::new(),
        };
        let t = totals.get(&day);
        days.push(DayInput {
            day,
            downloads: t.map_or(0, |t| t.downloads),
            bytes: t.map_or(0, |t| t.bytes),
            seconds: t.map_or(0, |t| t.download_seconds),
            series,
        });
    }

    let report = build(&days, source.is_some());
    let folder = dir.subdir("summary")?;
    for (name, text) in [
        ("usage-daily.csv", &report.daily),
        ("usage-monthly.csv", &report.monthly),
        ("usage-latest-month.txt", &report.latest_month),
    ] {
        write_atomic(&folder.join(name), |file| file.write_all(text.as_bytes()))?;
        let _ = writeln!(out, "wrote summary/{name}");
    }
    Ok(())
}

/// One day of input: the download history totals and the day's stored series
pub(crate) struct DayInput {
    pub day: NaiveDate,
    pub downloads: i64,
    pub bytes: i64,
    pub seconds: i64,
    pub series: Vec<Series>,
}

/// The three output files
pub(crate) struct Report {
    pub daily: String,
    pub monthly: String,
    pub latest_month: String,
}

/// What putio-sync was doing in a 5-minute window
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum State {
    Idle,
    Syncing,
    /// Downloading with this many at once
    Downloading(u32),
}

impl State {
    /// From the value of `putio_sync:activity_state:5m`
    fn from_value(value: f64) -> Option<Self> {
        if value.is_nan() {
            None
        } else if value >= 0.5 {
            Some(Self::Downloading(value.round() as u32))
        } else if value < 0.0 {
            Some(Self::Syncing)
        } else {
            Some(Self::Idle)
        }
    }
}

/// Mean and peak figures of the points in one state
#[derive(Debug, Default, Clone)]
struct StateAcc {
    points: u64,
    cpu_sum: f64,
    cpu_n: u64,
    cpu_max: Option<f64>,
    mem_sum: f64,
    mem_n: u64,
    mem_max: Option<f64>,
}

fn max_of(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

impl StateAcc {
    fn merge(&mut self, other: &StateAcc) {
        self.points += other.points;
        self.cpu_sum += other.cpu_sum;
        self.cpu_n += other.cpu_n;
        self.cpu_max = max_of(self.cpu_max, other.cpu_max);
        self.mem_sum += other.mem_sum;
        self.mem_n += other.mem_n;
        self.mem_max = max_of(self.mem_max, other.mem_max);
    }
}

/// Everything one period (a day or a month) adds up to
#[derive(Debug, Default, Clone)]
struct Acc {
    /// Days in the period
    days: u64,
    downloads: i64,
    bytes: i64,
    seconds: i64,
    /// Whether Prometheus was asked for this period
    with_resources: bool,
    /// Points that have a state
    points: u64,
    states: BTreeMap<State, StateAcc>,
    cpu_per_mbps: (f64, u64),
    mem_per_mbps: (f64, u64),
    /// Distinct process start times, in milliseconds
    starts: BTreeSet<i64>,
    first_start: Option<i64>,
    last_start: Option<i64>,
    versions: BTreeSet<String>,
}

/// Values of the named series by timestamp, series with the same name merged
fn by_name(series: &[Series]) -> HashMap<&str, BTreeMap<i64, f64>> {
    let mut out: HashMap<&str, BTreeMap<i64, f64>> = HashMap::new();
    for s in series {
        let map = out.entry(s.name.as_str()).or_default();
        for sample in &s.samples {
            if sample.value.is_finite() {
                map.insert(sample.ts_ms, sample.value);
            }
        }
    }
    out
}

impl Acc {
    fn from_day(day: &DayInput, with_resources: bool) -> Self {
        let mut acc = Acc {
            days: 1,
            downloads: day.downloads,
            bytes: day.bytes,
            seconds: day.seconds,
            with_resources,
            ..Acc::default()
        };
        let values = by_name(&day.series);
        let empty = BTreeMap::new();
        let get = |name: &str| values.get(name).unwrap_or(&empty);
        let (cpu_avg, cpu_max) = (get(CPU_AVG), get(CPU_MAX));
        let (mem_avg, mem_max) = (get(MEM_AVG), get(MEM_MAX));
        let bytes = get(BYTES);

        for (ts, raw) in get(STATE) {
            let Some(state) = State::from_value(*raw) else {
                continue;
            };
            acc.points += 1;
            let entry = acc.states.entry(state).or_default();
            entry.points += 1;
            if let Some(v) = cpu_avg.get(ts) {
                entry.cpu_sum += v;
                entry.cpu_n += 1;
            }
            entry.cpu_max = max_of(entry.cpu_max, cpu_max.get(ts).copied());
            if let Some(v) = mem_avg.get(ts) {
                entry.mem_sum += v;
                entry.mem_n += 1;
            }
            entry.mem_max = max_of(entry.mem_max, mem_max.get(ts).copied());

            // CPU and memory per MB/s, over downloading points that moved bytes
            if let (State::Downloading(_), Some(b)) = (state, bytes.get(ts)) {
                let mbps = b / POINT_SECONDS / 1e6;
                if mbps > 0.0 {
                    if let Some(v) = cpu_avg.get(ts) {
                        acc.cpu_per_mbps.0 += v / mbps;
                        acc.cpu_per_mbps.1 += 1;
                    }
                    if let Some(v) = mem_avg.get(ts) {
                        acc.mem_per_mbps.0 += v / mbps;
                        acc.mem_per_mbps.1 += 1;
                    }
                }
            }
        }

        for value in get(START_TIME).values() {
            let ms = (value * 1000.0).round() as i64;
            acc.first_start.get_or_insert(ms);
            acc.last_start = Some(ms);
            acc.starts.insert(ms);
        }
        for s in day.series.iter().filter(|s| s.name == BUILD_INFO) {
            if let Some(version) = s.labels.get("version").filter(|_| !s.samples.is_empty()) {
                acc.versions.insert(version.clone());
            }
        }
        acc
    }

    /// Add the period that follows this one
    fn merge(&mut self, next: &Acc) {
        self.days += next.days;
        self.downloads += next.downloads;
        self.bytes += next.bytes;
        self.seconds += next.seconds;
        self.with_resources |= next.with_resources;
        self.points += next.points;
        for (state, other) in &next.states {
            self.states.entry(*state).or_default().merge(other);
        }
        self.cpu_per_mbps.0 += next.cpu_per_mbps.0;
        self.cpu_per_mbps.1 += next.cpu_per_mbps.1;
        self.mem_per_mbps.0 += next.mem_per_mbps.0;
        self.mem_per_mbps.1 += next.mem_per_mbps.1;
        self.starts.extend(next.starts.iter().copied());
        self.first_start = self.first_start.or(next.first_start);
        self.last_start = next.last_start.or(self.last_start);
        self.versions.extend(next.versions.iter().cloned());
    }

    /// The largest number of downloads at once
    fn max_n(&self) -> u32 {
        self.states
            .keys()
            .filter_map(|s| match s {
                State::Downloading(n) => Some(*n),
                _ => None,
            })
            .max()
            .unwrap_or(0)
    }

    /// The column group `idle`, `syncing`, `downloading` (all) or `downloading_<n>`
    fn group(&self, name: &str) -> StateAcc {
        let mut out = StateAcc::default();
        for (state, acc) in &self.states {
            let hit = match (name, state) {
                ("idle", State::Idle) | ("syncing", State::Syncing) => true,
                ("downloading", State::Downloading(_)) => true,
                (_, State::Downloading(n)) => name == format!("downloading_{n}"),
                _ => false,
            };
            if hit {
                out.merge(acc);
            }
        }
        out
    }

    /// Restarts: distinct start times minus one, plus one when the first differs from
    /// the last one seen before the period
    fn restarts(&self, before: Option<i64>) -> Option<i64> {
        let first = self.first_start?;
        let changed = before.is_some_and(|b| b != first);
        Some(self.starts.len() as i64 - 1 + i64::from(changed))
    }
}

fn group_names(max_n: u32) -> Vec<String> {
    ["idle", "syncing", "downloading"]
        .into_iter()
        .map(str::to_string)
        .chain((1..=max_n).map(|n| format!("downloading_{n}")))
        .collect()
}

fn header(max_n: u32) -> String {
    let mut cols: Vec<String> = [
        "period",
        "downloads",
        "bytes",
        "download_seconds",
        "avg_speed_bytes_per_s",
    ]
    .map(str::to_string)
    .to_vec();
    let groups = group_names(max_n);
    cols.extend(groups.iter().map(|g| format!("{g}_s")));
    for g in &groups {
        cols.push(format!("cpu_avg_{g}"));
        cols.push(format!("cpu_max_{g}"));
        cols.push(format!("mem_avg_{g}_bytes"));
        cols.push(format!("mem_max_{g}_bytes"));
    }
    cols.extend(
        [
            "cpu_per_mbps",
            "mem_per_mbps_bytes",
            "restarts",
            "versions",
            "resource_coverage",
        ]
        .map(str::to_string),
    );
    cols.join(",")
}

/// `sum / n` with `decimals`, blank when there is nothing
fn mean(sum: f64, n: u64, decimals: usize) -> String {
    if n == 0 {
        String::new()
    } else {
        format!("{:.decimals$}", sum / n as f64)
    }
}

fn peak(value: Option<f64>, decimals: usize) -> String {
    value.map_or_else(String::new, |v| format!("{v:.decimals$}"))
}

fn row(period: &str, acc: &Acc, before: Option<i64>, max_n: u32) -> String {
    let mut cells = vec![
        period.to_string(),
        acc.downloads.to_string(),
        acc.bytes.to_string(),
        acc.seconds.to_string(),
        if acc.seconds > 0 {
            acc.bytes / acc.seconds
        } else {
            0
        }
        .to_string(),
    ];
    let groups = group_names(max_n);
    for g in &groups {
        cells.push(if acc.points == 0 {
            String::new()
        } else {
            ((acc.group(g).points as f64 * POINT_SECONDS) as u64).to_string()
        });
    }
    for g in &groups {
        let s = acc.group(g);
        cells.push(mean(s.cpu_sum, s.cpu_n, 4));
        cells.push(peak(s.cpu_max, 4));
        cells.push(mean(s.mem_sum, s.mem_n, 0));
        cells.push(peak(s.mem_max, 0));
    }
    cells.push(mean(acc.cpu_per_mbps.0, acc.cpu_per_mbps.1, 4));
    cells.push(mean(acc.mem_per_mbps.0, acc.mem_per_mbps.1, 0));
    cells.push(
        acc.restarts(before)
            .map_or_else(String::new, |r| r.to_string()),
    );
    cells.push(if acc.versions.is_empty() {
        String::new()
    } else {
        format!(
            "\"{}\"",
            acc.versions.iter().cloned().collect::<Vec<_>>().join(";")
        )
    });
    cells.push(if acc.with_resources {
        let expected = (POINTS_PER_DAY * acc.days) as f64;
        format!("{:.3}", acc.points as f64 / expected)
    } else {
        String::new()
    });
    cells.join(",")
}

/// Build the three files from per-day input
pub(crate) fn build(days: &[DayInput], with_resources: bool) -> Report {
    let accs: Vec<Acc> = days
        .iter()
        .map(|d| Acc::from_day(d, with_resources))
        .collect();
    let max_n = accs.iter().map(Acc::max_n).max().unwrap_or(0);

    let mut daily = format!("{}\n", header(max_n));
    let mut monthly = format!("{}\n", header(max_n));
    let mut last_start: Option<i64> = None;
    // The month being added up: its key, the last start time before it, and its total
    let mut month: Option<((i32, u32), Option<i64>, Acc)> = None;
    let mut months: Vec<(String, Option<i64>, Acc)> = Vec::new();
    let flush = |m: ((i32, u32), Option<i64>, Acc),
                 months: &mut Vec<(String, Option<i64>, Acc)>| {
        months.push((format!("{:04}-{:02}", m.0 .0, m.0 .1), m.1, m.2));
    };
    for (input, acc) in days.iter().zip(&accs) {
        daily.push_str(&row(&input.day.to_string(), acc, last_start, max_n));
        daily.push('\n');
        let key = (input.day.year(), input.day.month());
        match &mut month {
            Some((k, _, total)) if *k == key => total.merge(acc),
            _ => {
                if let Some(done) = month.take() {
                    flush(done, &mut months);
                }
                month = Some((key, last_start, acc.clone()));
            }
        }
        last_start = acc.last_start.or(last_start);
    }
    if let Some(done) = month.take() {
        flush(done, &mut months);
    }
    for (period, before, acc) in &months {
        monthly.push_str(&row(period, acc, *before, max_n));
        monthly.push('\n');
    }

    let latest_month = match (months.last(), days.last()) {
        (Some((period, before, acc)), Some(last)) => latest(period, last.day, acc, *before),
        _ => String::new(),
    };
    Report {
        daily,
        monthly,
        latest_month,
    }
}

/// Bytes with a decimal unit
fn size(bytes: f64) -> String {
    let units = ["B", "kB", "MB", "GB", "TB"];
    let mut value = bytes;
    let mut unit = 0;
    while value >= 1000.0 && unit < units.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{value:.0} B")
    } else {
        format!("{value:.1} {}", units[unit])
    }
}

fn hours(seconds: f64) -> String {
    let h = seconds / 3600.0;
    if h >= 10.0 {
        format!("{h:.0} h")
    } else {
        format!("{h:.1} h")
    }
}

/// The short lines of `usage-latest-month.txt`
fn latest(period: &str, through: NaiveDate, acc: &Acc, before: Option<i64>) -> String {
    let mut text = format!("putio-sync usage for {period} (through {through})\n");
    let speed = if acc.seconds > 0 {
        acc.bytes as f64 / acc.seconds as f64
    } else {
        0.0
    };
    text.push_str(&format!(
        "Downloads: {} files, {}, {} downloading, average {}/s\n",
        acc.downloads,
        size(acc.bytes as f64),
        hours(acc.seconds as f64),
        size(speed)
    ));
    if acc.points == 0 {
        text.push_str("No resource data for this period.\n");
        return text;
    }
    let mut groups = vec![("Idle".to_string(), "idle".to_string())];
    groups.push(("Syncing".to_string(), "syncing".to_string()));
    groups.extend((1..=acc.max_n()).map(|n| {
        (
            format!("Downloading {n} at once"),
            format!("downloading_{n}"),
        )
    }));
    for (label, name) in groups {
        let s = acc.group(&name);
        if s.points == 0 {
            continue;
        }
        let mut line = format!("{label}: {}", hours(s.points as f64 * POINT_SECONDS));
        if s.cpu_n > 0 {
            line.push_str(&format!(
                ", CPU avg {} cores (peak {})",
                mean(s.cpu_sum, s.cpu_n, 4),
                peak(s.cpu_max, 4)
            ));
        }
        if s.mem_n > 0 {
            line.push_str(&format!(
                ", memory avg {:.0} MB (peak {:.0} MB)",
                s.mem_sum / s.mem_n as f64 / 1e6,
                s.mem_max.unwrap_or_default() / 1e6
            ));
        }
        text.push_str(&line);
        text.push('\n');
    }
    let restarts = acc
        .restarts(before)
        .map_or_else(|| "unknown".to_string(), |r| r.to_string());
    let versions = acc.versions.iter().cloned().collect::<Vec<_>>().join(";");
    text.push_str(&format!("Restarts: {restarts}. Versions: {versions}\n"));
    let expected = (POINTS_PER_DAY * acc.days) as f64;
    text.push_str(&format!(
        "Resource data covers {:.0}% of the period.\n",
        acc.points as f64 / expected * 100.0
    ));
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::{Matcher, Server, ServerGuard};
    use putio_sync_db::Database;
    use std::collections::HashMap;

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn day(text: &str) -> NaiveDate {
        text.parse().unwrap()
    }

    fn start(day: &str) -> i64 {
        at(&format!("{day}T00:00:00Z")).timestamp()
    }

    /// One recorded series with a value per 5-minute point of `day`
    fn series(name: &str, extra: &str, day: &str, values: &[f64]) -> String {
        let points: Vec<String> = values
            .iter()
            .enumerate()
            .map(|(i, v)| format!(r#"[{},"{v}"]"#, start(day) + i as i64 * 300))
            .collect();
        format!(
            r#"{{"metric":{{"__name__":"putio_sync:{name}","app":"putio-sync","instance":"nas:9742"{extra}}},"values":[{}]}}"#,
            points.join(",")
        )
    }

    fn matrix(series: &[String]) -> String {
        format!(
            r#"{{"status":"success","data":{{"resultType":"matrix","result":[{}]}}}}"#,
            series.join(",")
        )
    }

    fn day1() -> String {
        let d = "2026-11-01";
        matrix(&[
            series("activity_state:5m", "", d, &[0.0, -1.0, 1.0, 2.0]),
            series("process_cpu_usage:avg5m", "", d, &[0.1, 0.2, 0.5, 0.9]),
            series("process_cpu_usage:max5m", "", d, &[0.2, 0.4, 0.8, 1.0]),
            series(
                "process_resident_memory_bytes:avg5m",
                "",
                d,
                &[1e8, 1.1e8, 1.2e8, 1.3e8],
            ),
            series(
                "process_resident_memory_bytes:max5m",
                "",
                d,
                &[1.5e8, 1.5e8, 1.6e8, 1.7e8],
            ),
            series("downloaded_bytes:increase5m", "", d, &[0.0, 0.0, 3e8, 6e8]),
            series("start_time_seconds:max5m", "", d, &[1000.0; 4]),
            series("build_info:max5m", r#","version":"0.1.1""#, d, &[1.0; 4]),
        ])
    }

    fn day2() -> String {
        let d = "2026-11-02";
        matrix(&[
            series("activity_state:5m", "", d, &[0.0, 1.0]),
            series("process_cpu_usage:avg5m", "", d, &[0.3, 0.7]),
            series("process_cpu_usage:max5m", "", d, &[0.5, 0.9]),
            series("process_resident_memory_bytes:avg5m", "", d, &[2e8, 2.1e8]),
            series(
                "process_resident_memory_bytes:max5m",
                "",
                d,
                &[2.2e8, 2.3e8],
            ),
            series("downloaded_bytes:increase5m", "", d, &[0.0, 1.5e8]),
            series("start_time_seconds:max5m", "", d, &[2000.0; 2]),
            series("build_info:max5m", r#","version":"0.1.2""#, d, &[1.0; 2]),
        ])
    }

    const EMPTY: &str = r#"{"status":"success","data":{"resultType":"matrix","result":[]}}"#;

    async fn mock_day(server: &mut ServerGuard, next: &str, body: &str) {
        server
            .mock("GET", "/api/v1/query")
            .match_query(Matcher::AllOf(vec![
                Matcher::Regex("instance%3D%22nas%3A9742%22".into()),
                Matcher::UrlEncoded("time".into(), format!("{next}T00:00:00Z")),
            ]))
            .with_body(body)
            .create_async()
            .await;
    }

    /// A live database with two downloads on 11-01 and one on 11-02
    async fn live_db(dir: &std::path::Path) -> PathBuf {
        let db = Database::new(dir.join("live.db")).await.unwrap();
        let history = db.history();
        history
            .insert_download_at(1, "a.mkv", 1000, Some(10), None, at("2026-11-01T01:00:00Z"))
            .await
            .unwrap();
        history
            .insert_download_at(2, "b.mkv", 3000, Some(30), None, at("2026-11-01T23:59:59Z"))
            .await
            .unwrap();
        history
            .insert_download_at(3, "c.mkv", 500, Some(5), None, at("2026-11-02T00:00:00Z"))
            .await
            .unwrap();
        let path = db.path().to_path_buf();
        db.close().await.unwrap();
        path
    }

    fn opts(dir: &std::path::Path, db_path: PathBuf, url: Option<&str>) -> SummaryOptions {
        SummaryOptions {
            dir: dir.join("archive"),
            db_path,
            prometheus: url.map(str::to_string),
            instance: Some("nas:9742".to_string()),
            from: None,
            to: Some(day("2026-11-02")),
            require_manifest: false,
            now: at("2026-11-03T03:30:00Z"),
        }
    }

    /// Rows of a CSV as header to cell; the quotes of a quoted field are dropped
    fn rows(path: &std::path::Path) -> Vec<HashMap<String, String>> {
        let text = std::fs::read_to_string(path).unwrap();
        let mut lines = text.lines();
        let header: Vec<&str> = lines.next().unwrap().split(',').collect();
        lines
            .map(|line| {
                let cells: Vec<&str> = line.split(',').collect();
                assert_eq!(cells.len(), header.len(), "{line}");
                header
                    .iter()
                    .zip(cells)
                    .map(|(h, c)| (h.to_string(), c.trim_matches('"').to_string()))
                    .collect()
            })
            .collect()
    }

    fn cells(row: &HashMap<String, String>, wanted: &[(&str, &str)]) {
        for (key, value) in wanted {
            assert_eq!(
                row.get(*key).map(String::as_str),
                Some(*value),
                "column {key}"
            );
        }
    }

    async fn run_with_prometheus(dir: &std::path::Path) -> (String, ServerGuard) {
        let mut server = Server::new_async().await;
        mock_day(&mut server, "2026-11-02", &day1()).await;
        mock_day(&mut server, "2026-11-03", &day2()).await;
        let o = opts(dir, live_db(dir).await, Some(&server.url()));
        let mut out = Vec::new();
        run(&o, &mut out).await.unwrap();
        (String::from_utf8(out).unwrap(), server)
    }

    #[tokio::test]
    async fn daily_csv_has_the_documented_columns_and_figures() {
        let dir = tempfile::tempdir().unwrap();
        let (_, _server) = run_with_prometheus(dir.path()).await;
        let path = dir.path().join("archive/summary/usage-daily.csv");

        let header = std::fs::read_to_string(&path).unwrap();
        assert_eq!(
            header.lines().next().unwrap(),
            "period,downloads,bytes,download_seconds,avg_speed_bytes_per_s,\
idle_s,syncing_s,downloading_s,downloading_1_s,downloading_2_s,\
cpu_avg_idle,cpu_max_idle,mem_avg_idle_bytes,mem_max_idle_bytes,\
cpu_avg_syncing,cpu_max_syncing,mem_avg_syncing_bytes,mem_max_syncing_bytes,\
cpu_avg_downloading,cpu_max_downloading,mem_avg_downloading_bytes,mem_max_downloading_bytes,\
cpu_avg_downloading_1,cpu_max_downloading_1,mem_avg_downloading_1_bytes,mem_max_downloading_1_bytes,\
cpu_avg_downloading_2,cpu_max_downloading_2,mem_avg_downloading_2_bytes,mem_max_downloading_2_bytes,\
cpu_per_mbps,mem_per_mbps_bytes,restarts,versions,resource_coverage"
        );

        let rows = rows(&path);
        assert_eq!(rows.len(), 2);
        cells(
            &rows[0],
            &[
                ("period", "2026-11-01"),
                ("downloads", "2"),
                ("bytes", "4000"),
                ("download_seconds", "40"),
                ("avg_speed_bytes_per_s", "100"),
                ("idle_s", "300"),
                ("syncing_s", "300"),
                ("downloading_s", "600"),
                ("downloading_1_s", "300"),
                ("downloading_2_s", "300"),
                ("cpu_avg_idle", "0.1000"),
                ("cpu_max_idle", "0.2000"),
                ("mem_avg_idle_bytes", "100000000"),
                ("mem_max_idle_bytes", "150000000"),
                ("cpu_avg_syncing", "0.2000"),
                ("cpu_avg_downloading", "0.7000"),
                ("cpu_max_downloading", "1.0000"),
                ("mem_avg_downloading_bytes", "125000000"),
                ("mem_max_downloading_bytes", "170000000"),
                ("cpu_avg_downloading_1", "0.5000"),
                ("cpu_avg_downloading_2", "0.9000"),
                ("cpu_per_mbps", "0.4750"),
                ("mem_per_mbps_bytes", "92500000"),
                ("restarts", "0"),
                ("versions", "0.1.1"),
                ("resource_coverage", "0.014"),
            ],
        );
        // A restart is a start time that differs from the previous day's last
        cells(
            &rows[1],
            &[
                ("period", "2026-11-02"),
                ("downloads", "1"),
                ("idle_s", "300"),
                ("syncing_s", "0"),
                ("downloading_2_s", "0"),
                ("cpu_avg_syncing", ""),
                ("cpu_avg_downloading_2", ""),
                ("cpu_per_mbps", "1.4000"),
                ("mem_per_mbps_bytes", "420000000"),
                ("restarts", "1"),
                ("versions", "0.1.2"),
                ("resource_coverage", "0.007"),
            ],
        );
    }

    #[tokio::test]
    async fn monthly_csv_and_latest_month_text() {
        let dir = tempfile::tempdir().unwrap();
        let (_, _server) = run_with_prometheus(dir.path()).await;

        let rows = rows(&dir.path().join("archive/summary/usage-monthly.csv"));
        assert_eq!(rows.len(), 1);
        cells(
            &rows[0],
            &[
                ("period", "2026-11"),
                ("downloads", "3"),
                ("bytes", "4500"),
                ("download_seconds", "45"),
                ("avg_speed_bytes_per_s", "100"),
                ("idle_s", "600"),
                ("syncing_s", "300"),
                ("downloading_s", "900"),
                ("downloading_1_s", "600"),
                ("downloading_2_s", "300"),
                ("cpu_avg_idle", "0.2000"),
                ("cpu_max_idle", "0.5000"),
                ("mem_avg_idle_bytes", "150000000"),
                ("cpu_per_mbps", "0.7833"),
                ("mem_per_mbps_bytes", "201666667"),
                ("restarts", "1"),
                ("versions", "0.1.1;0.1.2"),
                ("resource_coverage", "0.010"),
            ],
        );

        let text =
            std::fs::read_to_string(dir.path().join("archive/summary/usage-latest-month.txt"))
                .unwrap();
        assert!(
            text.starts_with("putio-sync usage for 2026-11 (through 2026-11-02)\n"),
            "{text}"
        );
        assert!(text.contains("Downloads: 3 files, 4.5 kB"), "{text}");
        assert!(
            text.contains(
                "Downloading 2 at once: 0.1 h, CPU avg 0.9000 cores (peak 1.0000), \
                 memory avg 130 MB (peak 170 MB)"
            ),
            "{text}"
        );
        assert!(
            text.contains("Restarts: 1. Versions: 0.1.1;0.1.2"),
            "{text}"
        );
    }

    #[tokio::test]
    async fn two_runs_give_identical_files() {
        let dir = tempfile::tempdir().unwrap();
        let (_, _server) = run_with_prometheus(dir.path()).await;
        let read = || {
            [
                "usage-daily.csv",
                "usage-monthly.csv",
                "usage-latest-month.txt",
            ]
            .map(|n| std::fs::read(dir.path().join("archive/summary").join(n)).unwrap())
        };
        let first = read();

        let o = opts(dir.path(), dir.path().join("live.db"), Some(&_server.url()));
        run(&o, &mut Vec::new()).await.unwrap();

        assert!(read() == first);
        assert_eq!(
            std::fs::read_dir(dir.path().join("archive/summary"))
                .unwrap()
                .count(),
            3
        );
    }

    #[tokio::test]
    async fn without_prometheus_the_resource_columns_are_blank() {
        let dir = tempfile::tempdir().unwrap();
        let o = opts(dir.path(), live_db(dir.path()).await, None);
        run(&o, &mut Vec::new()).await.unwrap();

        let rows = rows(&dir.path().join("archive/summary/usage-daily.csv"));
        cells(
            &rows[0],
            &[
                ("downloads", "2"),
                ("bytes", "4000"),
                ("idle_s", ""),
                ("downloading_s", ""),
                ("cpu_avg_idle", ""),
                ("cpu_per_mbps", ""),
                ("restarts", ""),
                ("versions", ""),
                ("resource_coverage", ""),
            ],
        );
        assert!(!rows[0].contains_key("downloading_1_s"));
        let text =
            std::fs::read_to_string(dir.path().join("archive/summary/usage-latest-month.txt"))
                .unwrap();
        assert!(text.contains("No resource data"), "{text}");
    }

    #[tokio::test]
    async fn a_day_without_samples_is_blank_not_zero() {
        let dir = tempfile::tempdir().unwrap();
        let mut server = Server::new_async().await;
        mock_day(&mut server, "2026-11-02", &day1()).await;
        mock_day(&mut server, "2026-11-03", EMPTY).await;
        let o = opts(dir.path(), live_db(dir.path()).await, Some(&server.url()));
        run(&o, &mut Vec::new()).await.unwrap();

        let rows = rows(&dir.path().join("archive/summary/usage-daily.csv"));
        cells(
            &rows[1],
            &[
                ("downloads", "1"),
                ("bytes", "500"),
                ("idle_s", ""),
                ("cpu_avg_idle", ""),
                ("mem_avg_downloading_bytes", ""),
                ("restarts", ""),
                ("versions", ""),
                ("resource_coverage", "0.000"),
            ],
        );
    }

    #[tokio::test]
    async fn days_without_downloads_get_zero_totals_and_today_is_partial() {
        let dir = tempfile::tempdir().unwrap();
        let mut server = Server::new_async().await;
        mock_day(&mut server, "2026-11-02", &day1()).await;
        mock_day(&mut server, "2026-11-03", &day2()).await;
        mock_day(&mut server, "2026-11-04", EMPTY).await;
        let mut o = opts(dir.path(), live_db(dir.path()).await, Some(&server.url()));
        o.to = None;
        o.now = at("2026-11-03T10:00:00Z");
        run(&o, &mut Vec::new()).await.unwrap();

        let rows = rows(&dir.path().join("archive/summary/usage-daily.csv"));
        assert_eq!(rows.len(), 3);
        cells(
            &rows[2],
            &[
                ("period", "2026-11-03"),
                ("downloads", "0"),
                ("bytes", "0"),
                ("download_seconds", "0"),
                ("avg_speed_bytes_per_s", "0"),
                ("resource_coverage", "0.000"),
            ],
        );
    }

    #[tokio::test]
    async fn an_unreachable_prometheus_is_exit_2_and_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let o = opts(
            dir.path(),
            live_db(dir.path()).await,
            Some("http://127.0.0.1:1"),
        );
        let err = run(&o, &mut Vec::new()).await.unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(!dir.path().join("archive/summary/usage-daily.csv").exists());
    }

    #[tokio::test]
    async fn a_missing_database_is_exit_3_and_a_backwards_range_exit_1() {
        let dir = tempfile::tempdir().unwrap();
        let o = opts(dir.path(), dir.path().join("none.db"), None);
        assert_eq!(run(&o, &mut Vec::new()).await.unwrap_err().exit_code(), 3);

        let mut o = opts(dir.path(), live_db(dir.path()).await, None);
        o.from = Some(day("2026-11-05"));
        assert_eq!(run(&o, &mut Vec::new()).await.unwrap_err().exit_code(), 1);
    }
}
