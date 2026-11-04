//! Local checks: the download folder, its disk, the database and the config file

use super::{bytes, count, Ctx};
use crate::report::{Finding, Item, Status};
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
    let Some(db) = &ctx.db else {
        return Finding::skipped("No database is open.");
    };
    match db.quick_check().await {
        Ok(lines) if lines == ["ok"] => Finding::ok("The integrity check passed."),
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

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;

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
}
