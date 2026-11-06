//! `archive metrics`: one OpenMetrics file per finished UTC day (contracts/archive-cli.md)

use crate::error::ArchiveResult;
use crate::files::{write_atomic, ArchiveDir, ManifestEntry};
use crate::openmetrics;
use crate::prom::PromClient;
use crate::POINTS_PER_DAY;
use chrono::{DateTime, Duration, NaiveDate, SecondsFormat, Utc};
use flate2::write::GzEncoder;
use flate2::Compression;
use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;
use tracing::info;

/// Days looked back on a first run
const DEFAULT_LOOKBACK_DAYS: i64 = 366;

/// The series whose sample count says how complete a day is
const UP_SERIES: &str = "putio_sync:up:min5m";

/// Options of `archive metrics`
pub struct MetricsOptions {
    pub dir: PathBuf,
    pub prometheus: String,
    pub instance: Option<String>,
    pub keep_days: Option<u32>,
    pub require_manifest: bool,
    /// First day to try; default 366 days before today
    pub since: Option<NaiveDate>,
    pub now: DateTime<Utc>,
}

/// Save every finished day from `since` to yesterday that the manifest does not list,
/// writing one line per day to `out`
///
/// With `keep_days`, days older than that are neither fetched nor kept, so pruning never
/// makes the next run fetch them again.
pub async fn run(opts: &MetricsOptions, out: &mut dyn Write) -> ArchiveResult<()> {
    let today = opts.now.date_naive();
    let dir = ArchiveDir::open(&opts.dir, opts.require_manifest)?;
    let folder = dir.subdir("metrics")?;
    let done: HashSet<NaiveDate> = dir
        .read_manifest()?
        .into_iter()
        .filter_map(|entry| match entry {
            ManifestEntry::Metrics { day, .. } => Some(day),
            ManifestEntry::Db { .. } => None,
        })
        .collect();

    let mut first = opts
        .since
        .unwrap_or(today - Duration::days(DEFAULT_LOOKBACK_DAYS));
    if let Some(keep) = opts.keep_days {
        first = first.max(today - Duration::days(i64::from(keep)));
    }
    let yesterday = today - Duration::days(1);
    let todo: Vec<NaiveDate> = first
        .iter_days()
        .take_while(|day| *day <= yesterday)
        .filter(|day| !done.contains(day))
        .collect();

    if !todo.is_empty() {
        let client = PromClient::new(&opts.prometheus)?;
        let instance = client.instance(opts.instance.as_deref()).await?;
        for day in todo {
            let series = client.fetch_day(&instance, day).await?;
            if series.is_empty() {
                let _ = writeln!(out, "no data {day}");
                continue;
            }
            let points = series
                .iter()
                .filter(|s| s.name == UP_SERIES)
                .map(|s| s.samples.len() as u64)
                .sum::<u64>();
            let text = openmetrics::render(&series);
            let name = format!("putio-sync-metrics-{day}.om.gz");
            write_atomic(&folder.join(&name), |file| {
                let mut gz = GzEncoder::new(file, Compression::default());
                gz.write_all(text.as_bytes())?;
                gz.finish().map(|_| ())
            })?;
            let partial = points < POINTS_PER_DAY;
            dir.append_manifest(&ManifestEntry::Metrics {
                day,
                file: format!("metrics/{name}"),
                saved_at: opts.now.to_rfc3339_opts(SecondsFormat::Secs, true),
                points,
                expected: POINTS_PER_DAY,
                partial,
            })?;
            info!(%day, points, partial, "Saved a metrics day");
            let _ = writeln!(
                out,
                "saved {day} points={points}/{POINTS_PER_DAY}{}",
                if partial { " partial" } else { "" }
            );
        }
    }

    if let Some(keep) = opts.keep_days {
        let removed = dir.prune(keep, today)?;
        if removed > 0 {
            let _ = writeln!(out, "pruned {removed} old files");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::{ManifestEntry, MANIFEST};
    use flate2::read::GzDecoder;
    use mockito::{Matcher, Mock, Server, ServerGuard};
    use std::io::Read;

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    fn day_start(day: &str) -> i64 {
        at(&format!("{day}T00:00:00Z")).timestamp()
    }

    /// An answer with `points` samples of `up` and one other series, from the day's start
    fn matrix(day: &str, points: i64) -> String {
        let values: Vec<String> = (0..points)
            .map(|i| format!(r#"[{},"1"]"#, day_start(day) + i * 300))
            .collect();
        let values = values.join(",");
        format!(
            r#"{{"status":"success","data":{{"resultType":"matrix","result":[
            {{"metric":{{"__name__":"putio_sync:up:min5m","app":"putio-sync","instance":"nas:9742"}},"values":[{values}]}},
            {{"metric":{{"__name__":"putio_sync:process_cpu_usage:avg5m","app":"putio-sync","instance":"nas:9742"}},"values":[{values}]}}
            ]}}}}"#
        )
    }

    async fn mock_day(server: &mut ServerGuard, next: &str, body: String) -> Mock {
        server
            .mock("GET", "/api/v1/query")
            .match_query(Matcher::AllOf(vec![
                Matcher::Regex("instance%3D%22nas%3A9742%22".into()),
                Matcher::UrlEncoded("time".into(), format!("{next}T00:00:00Z")),
            ]))
            .with_body(body)
            .expect(1)
            .create_async()
            .await
    }

    fn opts(dir: &std::path::Path, url: &str, now: &str) -> MetricsOptions {
        MetricsOptions {
            dir: dir.to_path_buf(),
            prometheus: url.to_string(),
            instance: Some("nas:9742".to_string()),
            keep_days: None,
            require_manifest: false,
            since: Some("2026-11-01".parse().unwrap()),
            now: at(now),
        }
    }

    fn gunzip(path: &std::path::Path) -> String {
        let mut text = String::new();
        GzDecoder::new(std::fs::File::open(path).unwrap())
            .read_to_string(&mut text)
            .unwrap();
        text
    }

    fn manifest(dir: &std::path::Path) -> Vec<ManifestEntry> {
        crate::files::ArchiveDir::open(dir, false)
            .unwrap()
            .read_manifest()
            .unwrap()
    }

    #[tokio::test]
    async fn saves_each_finished_day_once() {
        let mut server = Server::new_async().await;
        let m1 = mock_day(&mut server, "2026-11-02", matrix("2026-11-01", 288)).await;
        let m2 = mock_day(&mut server, "2026-11-03", matrix("2026-11-02", 288)).await;
        let dir = tempfile::tempdir().unwrap();
        // Today (11-03) is still running and is not asked for
        let o = opts(dir.path(), &server.url(), "2026-11-03T03:30:00Z");

        let mut out = Vec::new();
        run(&o, &mut out).await.unwrap();

        assert_eq!(
            String::from_utf8(out).unwrap(),
            "saved 2026-11-01 points=288/288\nsaved 2026-11-02 points=288/288\n"
        );
        let text = gunzip(
            &dir.path()
                .join("metrics/putio-sync-metrics-2026-11-01.om.gz"),
        );
        assert!(text.starts_with("# TYPE putio_sync:process_cpu_usage:avg5m gauge\n"));
        assert!(text.ends_with("# EOF\n"));
        assert!(text.contains(&format!(
            "putio_sync:up:min5m{{app=\"putio-sync\",instance=\"nas:9742\"}} 1 {}\n",
            day_start("2026-11-01")
        )));
        let entries = manifest(dir.path());
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0],
            ManifestEntry::Metrics {
                day: "2026-11-01".parse().unwrap(),
                file: "metrics/putio-sync-metrics-2026-11-01.om.gz".to_string(),
                saved_at: "2026-11-03T03:30:00Z".to_string(),
                points: 288,
                expected: 288,
                partial: false,
            }
        );

        // A second run does nothing: the mocks were each hit exactly once
        let mut out = Vec::new();
        run(&o, &mut out).await.unwrap();
        assert!(out.is_empty());
        m1.assert_async().await;
        m2.assert_async().await;
        assert_eq!(manifest(dir.path()).len(), 2);
    }

    #[tokio::test]
    async fn a_partial_day_is_saved_and_marked() {
        let mut server = Server::new_async().await;
        mock_day(&mut server, "2026-11-02", matrix("2026-11-01", 201)).await;
        let dir = tempfile::tempdir().unwrap();
        let o = opts(dir.path(), &server.url(), "2026-11-02T03:30:00Z");

        let mut out = Vec::new();
        run(&o, &mut out).await.unwrap();

        assert_eq!(
            String::from_utf8(out).unwrap(),
            "saved 2026-11-01 points=201/288 partial\n"
        );
        match &manifest(dir.path())[0] {
            ManifestEntry::Metrics {
                points, partial, ..
            } => {
                assert_eq!(*points, 201);
                assert!(*partial);
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn a_gap_day_writes_nothing_and_is_tried_again() {
        let mut server = Server::new_async().await;
        let empty = r#"{"status":"success","data":{"resultType":"matrix","result":[]}}"#;
        let gap = mock_day(&mut server, "2026-11-02", empty.to_string()).await;
        let dir = tempfile::tempdir().unwrap();
        let o = opts(dir.path(), &server.url(), "2026-11-02T03:30:00Z");

        let mut out = Vec::new();
        run(&o, &mut out).await.unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "no data 2026-11-01\n");
        gap.assert_async().await;
        assert!(manifest(dir.path()).is_empty());
        assert_eq!(
            std::fs::read_dir(dir.path().join("metrics"))
                .unwrap()
                .count(),
            0
        );

        // Prometheus has the day on the next run
        server.reset();
        mock_day(&mut server, "2026-11-02", matrix("2026-11-01", 288)).await;
        let mut out = Vec::new();
        run(&o, &mut out).await.unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "saved 2026-11-01 points=288/288\n"
        );
    }

    #[tokio::test]
    async fn an_outage_is_exit_2_and_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let o = opts(dir.path(), "http://127.0.0.1:1", "2026-11-03T03:30:00Z");
        let mut out = Vec::new();
        let err = run(&o, &mut out).await.unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(out.is_empty());
        assert!(!dir.path().join(MANIFEST).exists());
        assert_eq!(
            std::fs::read_dir(dir.path().join("metrics"))
                .unwrap()
                .count(),
            0
        );
    }

    #[tokio::test]
    async fn keep_days_prunes_and_does_not_fetch_pruned_days() {
        let mut server = Server::new_async().await;
        // 2026-11-10 is "today"; keep 3 days means 11-07 on; 11-06 and older are not asked for
        for d in 7..=9 {
            let day = format!("2026-11-{d:02}");
            let next = format!("2026-11-{:02}", d + 1);
            mock_day(&mut server, &next, matrix(&day, 288)).await;
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("metrics")).unwrap();
        let old = dir
            .path()
            .join("metrics/putio-sync-metrics-2026-11-01.om.gz");
        std::fs::write(&old, "x").unwrap();
        let mut o = opts(dir.path(), &server.url(), "2026-11-10T03:30:00Z");
        o.keep_days = Some(3);

        let mut out = Vec::new();
        run(&o, &mut out).await.unwrap();

        assert!(!old.exists());
        assert!(dir
            .path()
            .join("metrics/putio-sync-metrics-2026-11-09.om.gz")
            .exists());
        assert_eq!(manifest(dir.path()).len(), 3);
    }

    #[tokio::test]
    async fn require_manifest_stops_on_an_empty_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut o = opts(dir.path(), "http://127.0.0.1:1", "2026-11-03T03:30:00Z");
        o.require_manifest = true;
        let err = run(&o, &mut Vec::new()).await.unwrap_err();
        assert_eq!(err.exit_code(), 4);
    }
}
