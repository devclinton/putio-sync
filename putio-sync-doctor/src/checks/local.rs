//! Local checks: the download folder, its disk, the database, the config file and the
//! log level

use super::{bytes, count, Ctx};
use crate::report::{Finding, Item, Status};
use putio_sync_core::logging::{self, LevelStatus};
use putio_sync_core::{assistant, config_lint};
use putio_sync_telemetry::gauges::disk_usage;

/// Free space that gives a warning / a problem, in percent
const DISK_WARNING: f64 = 10.0;
const DISK_PROBLEM: f64 = 2.0;
/// Integrity messages kept in the report
const MAX_DB_MESSAGES: usize = 20;

pub(crate) async fn download_dir(ctx: &Ctx) -> Finding {
    let dir = &ctx.config.download.target_directory;
    let shown = dir.display().to_string();
    if !dir.is_dir() {
        return Finding::new(Status::Problem, format!("{shown} does not exist."))
            .suggest("Create the folder or fix `download.target_directory` in the config.");
    }
    let probe = dir.join(format!(".putio-sync-doctor-{}", std::process::id()));
    match std::fs::write(&probe, b"doctor") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            Finding::ok(format!("{shown} exists and is writable."))
        }
        Err(e) => Finding::new(Status::Problem, format!("Can't write to {shown}: {e}."))
            .suggest("Check the folder's permissions and that the disk is mounted read-write."),
    }
}

pub(crate) async fn disk_space(ctx: &Ctx) -> Finding {
    let dir = &ctx.config.download.target_directory;
    if !dir.is_dir() {
        return Finding::skipped("The download folder does not exist.");
    }
    let (free, total) = match disk_usage(dir) {
        Ok(usage) => usage,
        Err(e) => return Finding::skipped(format!("Could not read the disk usage: {e}")),
    };
    if total == 0 {
        return Finding::skipped("The disk reported no size.");
    }
    let pending: u64 = match &ctx.db {
        Some(db) => db
            .queue()
            .claimed()
            .await
            .map(|rows| rows.iter().map(|r| r.file_size.max(0) as u64).sum())
            .unwrap_or(0),
        None => 0,
    };
    let pct = free as f64 * 100.0 / total as f64;
    let mut summary = format!("{} free of {} ({:.0}%).", bytes(free), bytes(total), pct);
    if pending > 0 {
        summary.push_str(&format!(" The queue still needs {}.", bytes(pending)));
    }
    let status = if pct < DISK_PROBLEM {
        Status::Problem
    } else if pct < DISK_WARNING || free < pending {
        Status::Warning
    } else {
        Status::Ok
    };
    Finding::new(status, summary)
        .suggest("Free up space on the disk; downloads fail when it fills up.")
}

pub(crate) async fn database(ctx: &Ctx) -> Finding {
    let path = match &ctx.db {
        Some(db) => db.path().to_path_buf(),
        None => putio_sync_core::database::path_for(&ctx.config, ctx.config_path.as_deref()),
    };
    // A 0.2.x database still in the download folder, next to or instead of the new one
    let legacy = putio_sync_core::database::legacy_path(&ctx.config);
    let leftover = legacy != path && legacy.is_file();
    let Some(db) = &ctx.db else {
        if leftover {
            return Finding::new(
                Status::Warning,
                format!(
                    "The database is still in the download folder at {}.",
                    legacy.display()
                ),
            )
            .suggest(format!(
                "Restart the putio-sync daemon; it moves the database to {}.",
                path.display()
            ));
        }
        return Finding::skipped(format!("No database at {} yet.", path.display()));
    };
    match db.quick_check().await {
        Ok(lines) if lines == ["ok"] && leftover => Finding::new(
            Status::Warning,
            format!(
                "Using {}, but an older database is still at {}.",
                path.display(),
                legacy.display()
            ),
        )
        .suggest("Delete the older database once you no longer need it."),
        Ok(lines) if lines == ["ok"] => {
            Finding::ok(format!("The integrity check of {} passed.", path.display()))
        }
        Ok(lines) => {
            let items = lines
                .iter()
                .take(MAX_DB_MESSAGES)
                .enumerate()
                .map(|(i, line)| Item::new(i + 1, line.clone()))
                .collect();
            Finding::new(
                Status::Problem,
                format!("The integrity check found {}.", count(lines.len(), "problem")),
            )
            .suggest("Stop putio-sync, back up the database file, and restore it or let putio-sync create a new one.")
            .items(items)
        }
        Err(e) => Finding::new(Status::Problem, format!("The integrity check failed: {e}"))
            .suggest("Check that the database file is readable and the disk is healthy."),
    }
}

/// Line number (1-based) of byte `offset` in `text`
fn line_of(text: &str, offset: usize) -> usize {
    text.as_bytes()[..offset.min(text.len())]
        .iter()
        .filter(|b| **b == b'\n')
        .count()
        + 1
}

/// Unknown or misspelt keys in the config file, and risky combinations of settings in the
/// running config (feature 015)
pub(crate) async fn config(ctx: &Ctx) -> Finding {
    let risks: Vec<Item> = assistant::risks(&ctx.config)
        .into_iter()
        .map(|r| Item::new(r.id, r.keys.join(" + ")).detail(r.message))
        .collect();
    let lint = match &ctx.config_path {
        None => None,
        Some(path) => match std::fs::read_to_string(path) {
            Ok(text) => Some(lint_items(&text)),
            Err(e) if risks.is_empty() => {
                return Finding::skipped(format!("Could not read {}: {e}", path.display()));
            }
            Err(_) => None,
        },
    };
    let warnings = match lint {
        Some(Err(problem)) => return problem,
        Some(Ok(items)) => items,
        None if risks.is_empty() => return Finding::skipped("No config file is in use."),
        None => Vec::new(),
    };
    if warnings.is_empty() && risks.is_empty() {
        return Finding::ok("No unknown or risky settings.");
    }

    let mut parts = Vec::new();
    let mut suggestions = Vec::new();
    if !warnings.is_empty() {
        let verb = if warnings.len() == 1 { "needs" } else { "need" };
        parts.push(format!(
            "{} {verb} a look in the config file",
            count(warnings.len(), "setting")
        ));
        suggestions.push("Fix or remove these lines in the config file.");
    }
    if !risks.is_empty() {
        parts.push(format!(
            "{} could lose data or cause trouble",
            count(risks.len(), "combination")
        ));
        suggestions
            .push("Change one of the settings named, on the Settings page or in the config file.");
    }
    let mut summary = parts.join("; ");
    summary.push('.');
    let mut items = risks;
    items.extend(warnings);
    Finding::new(Status::Warning, summary)
        .suggest(suggestions.join(" "))
        .items(items)
}

/// The lint warnings of the config file `text` as items, or a Problem when it is not
/// valid TOML
fn lint_items(text: &str) -> Result<Vec<Item>, Finding> {
    match config_lint::lint(text) {
        Ok(warnings) => Ok(warnings
            .into_iter()
            .map(|w| Item::new(&w.key, &w.key).detail(w.message))
            .collect()),
        // Only the message and line: the error's own text quotes the line, which could
        // hold the token
        Err(e) => {
            let line = e.span().map(|s| line_of(text, s.start));
            let at = line.map(|l| format!(" (line {l})")).unwrap_or_default();
            Err(Finding::new(
                Status::Problem,
                format!("The config file can't be read{at}: {}", e.message()),
            )
            .suggest("Fix the file; putio-sync will not start with it as it is."))
        }
    }
}

/// Warn when anything logs at debug or trace (feature 047)
pub(crate) async fn log_level(ctx: &Ctx) -> Finding {
    let mut status = logging::level_status();
    // A process that didn't set up logging itself (`putio-sync doctor`): read the config
    if !status.available && status.base.is_empty() {
        let (base, source) = match std::env::var("RUST_LOG") {
            Ok(v) if !v.trim().is_empty() => (v, "env"),
            _ => {
                let l = &ctx.config.logging;
                let items = std::iter::once(l.level.clone()).chain(l.module_filters.clone());
                (items.collect::<Vec<_>>().join(","), "config")
            }
        };
        status.verbose = logging::is_verbose(&base);
        status.effective = base.clone();
        status.base = base;
        status.source = source;
    }
    level_finding(&status)
}

fn level_finding(s: &LevelStatus) -> Finding {
    if !s.verbose {
        return Finding::ok(format!("Logging at {}.", s.effective));
    }
    let loud: Vec<&str> = s
        .effective
        .split(',')
        .map(str::trim)
        .filter(|i| logging::is_verbose(i))
        .collect();
    let mut summary = format!(
        "{} is on: {}.",
        if loud.iter().any(|i| i.ends_with("trace")) {
            "Trace logging"
        } else {
            "Debug logging"
        },
        loud.join(", ")
    );
    let page: Vec<String> = s
        .global
        .iter()
        .chain(&s.modules)
        .filter(|o| logging::is_verbose(&o.level))
        .map(|o| {
            let what = o.target.as_deref().unwrap_or("everything");
            match &o.until {
                Some(until) => format!("{what} until {until}"),
                None => format!("{what} until restart"),
            }
        })
        .collect();
    if page.is_empty() {
        let from = if s.source == "env" {
            "RUST_LOG"
        } else {
            "the config file"
        };
        summary.push_str(&format!(" Set in {from}."));
    } else {
        summary.push_str(&format!(" Set on the Logs page: {}.", page.join(", ")));
    }
    Finding::new(Status::Warning, summary).suggest(
        "Debug and trace slow the daemon and fill the log file. Revert on the Logs page, or lower the level in Settings › Advanced › Logging.",
    )
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;

    fn level(effective: &str, modules: Vec<logging::OverrideStatus>) -> LevelStatus {
        LevelStatus {
            base: "info".into(),
            source: "config",
            effective: effective.into(),
            global: None,
            modules,
            verbose: logging::is_verbose(effective),
            available: true,
        }
    }

    #[test]
    fn log_level_warns_on_debug_and_trace() {
        let ok = level_finding(&level("info", vec![]));
        assert_eq!(ok.status, Status::Ok);

        let config = level_finding(&level("info,putio_sync_hooks=debug", vec![]));
        assert_eq!(config.status, Status::Warning);
        assert!(
            config.summary.contains("putio_sync_hooks=debug"),
            "{}",
            config.summary
        );
        assert!(config.summary.contains("config file"), "{}", config.summary);

        let page = level_finding(&level(
            "info,putio_sync_download=trace",
            vec![logging::OverrideStatus {
                target: Some("putio_sync_download".into()),
                level: "trace".into(),
                until: Some("2026-11-05T10:30:00Z".into()),
            }],
        ));
        assert_eq!(page.status, Status::Warning);
        assert!(
            page.summary.starts_with("Trace logging"),
            "{}",
            page.summary
        );
        assert!(
            page.summary
                .contains("Logs page: putio_sync_download until 2026-11-05T10:30:00Z"),
            "{}",
            page.summary
        );
    }

    #[tokio::test]
    async fn the_log_level_check_reads_the_config_without_logging() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = ctx("http://127.0.0.1:9", dir.path(), None);
        if std::env::var_os("RUST_LOG").is_none() {
            c.config.logging.level = "debug".into();
            assert_eq!(log_level(&c).await.status, Status::Warning);
        }
    }

    #[tokio::test]
    async fn download_dir_missing_is_a_problem() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = ctx("http://127.0.0.1:9", dir.path(), None);
        assert_eq!(download_dir(&c).await.status, Status::Ok);
        c.config.download.target_directory = dir.path().join("nope");
        assert_eq!(download_dir(&c).await.status, Status::Problem);
        assert_eq!(disk_space(&c).await.status, Status::Skipped);
    }

    #[tokio::test]
    async fn disk_space_reads_the_disk() {
        let dir = tempfile::tempdir().unwrap();
        let f = disk_space(&ctx("http://127.0.0.1:9", dir.path(), None)).await;
        assert_ne!(f.status, Status::Skipped);
        assert!(f.summary.contains("free of"));
    }

    #[tokio::test]
    async fn database_check_passes_on_a_fresh_db() {
        let dir = tempfile::tempdir().unwrap();
        let c = ctx("http://127.0.0.1:9", dir.path(), Some(db(dir.path()).await));
        assert_eq!(database(&c).await.status, Status::Ok);
        let none = ctx("http://127.0.0.1:9", dir.path(), None);
        assert_eq!(database(&none).await.status, Status::Skipped);
    }

    #[tokio::test]
    async fn database_check_reports_the_path_and_a_leftover_in_the_download_folder() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = ctx("http://127.0.0.1:9", dir.path(), None);
        c.config_path = Some(dir.path().join("config.toml"));
        let path = dir.path().join(".putiosync.db");
        let f = database(&c).await;
        assert_eq!(f.status, Status::Skipped);
        assert!(f.summary.contains(&path.display().to_string()));

        let legacy = dir.path().join("downloads").join(".putiosync.db");
        std::fs::write(&legacy, b"").unwrap();
        let f = database(&c).await;
        assert_eq!(f.status, Status::Warning);
        assert!(f.summary.contains("download folder"));

        c.db = Some(db(dir.path()).await);
        let f = database(&c).await;
        assert_eq!(f.status, Status::Warning);
        assert!(f.summary.contains(&legacy.display().to_string()));

        std::fs::remove_file(&legacy).unwrap();
        let f = database(&c).await;
        assert_eq!(f.status, Status::Ok);
        assert!(f.summary.contains(&path.display().to_string()));
    }

    #[tokio::test]
    async fn config_findings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut c = ctx("http://127.0.0.1:9", dir.path(), None);
        assert_eq!(config(&c).await.status, Status::Skipped);
        c.config_path = Some(path.clone());

        let good = toml::to_string(&putio_sync_core::config::Config::default()).unwrap();
        std::fs::write(&path, &good).unwrap();
        assert_eq!(config(&c).await.status, Status::Ok);

        std::fs::write(&path, format!("typo_section = 1\n{good}")).unwrap();
        let f = config(&c).await;
        assert_eq!(f.status, Status::Warning);
        assert_eq!(f.items[0].key, "typo_section");
        assert_eq!(f.summary, "1 setting needs a look in the config file.");

        std::fs::write(&path, "[auth]\ntoken = \"SECRET\n").unwrap();
        let f = config(&c).await;
        assert_eq!(f.status, Status::Problem);
        assert!(!f.summary.contains("SECRET"), "{}", f.summary);
        assert!(f.summary.contains("line 2"), "{}", f.summary);
    }

    #[tokio::test]
    async fn config_lists_risks_with_the_lint() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut c = ctx("http://127.0.0.1:9", dir.path(), None);
        c.config.download.delete_after_download = true;
        c.config.download.target_directory = std::env::temp_dir().join("putio-sync-015");

        // No config file: the risks alone
        let f = config(&c).await;
        assert_eq!(f.status, Status::Warning);
        assert_eq!(f.items[0].key, "delete_temp_folder");
        assert!(f.summary.contains("1 combination"), "{}", f.summary);

        c.config_path = Some(path.clone());
        let good = toml::to_string(&putio_sync_core::config::Config::default()).unwrap();
        std::fs::write(&path, format!("typo_section = 1\n{good}")).unwrap();
        let f = config(&c).await;
        assert_eq!(f.status, Status::Warning);
        let keys: Vec<&str> = f.items.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(keys, ["delete_temp_folder", "typo_section"]);
        assert!(
            f.summary.starts_with("1 setting needs a look"),
            "{}",
            f.summary
        );
    }

    #[tokio::test]
    async fn config_warns_about_deleting_what_a_pattern_skips() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = ctx("http://127.0.0.1:9", dir.path(), None);
        c.config.download.target_directory = dir.path().join("dl");
        c.config.download.rules = vec![putio_sync_core::rules::FolderRule {
            path: "Movies".into(),
            include: Some("*.mkv".into()),
            delete_blocked: Some(true),
            ..Default::default()
        }];
        let f = config(&c).await;
        assert_eq!(f.status, Status::Warning);
        assert_eq!(f.items[0].key, "delete_unmatched");
        assert!(
            f.items[0]
                .detail
                .as_deref()
                .unwrap_or("")
                .contains("`*.mkv`"),
            "{:?}",
            f.items[0]
        );
    }
}
