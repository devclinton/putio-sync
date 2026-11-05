//! Download verification (feature 034, research R7): read from the database only

use super::{count, Ctx};
use crate::report::{Finding, Item, Status};

/// Downloads that gave up on a mismatch within this many days are reported
const RECENT_DAYS: i64 = 7;
/// Problems listed as items at most
const ITEMS: i64 = 20;

pub(crate) async fn verification(ctx: &Ctx) -> Finding {
    if !ctx.config.download.verify {
        return Finding::new(
            Status::Warning,
            "New downloads are not checked against Put.io's CRC32.",
        )
        .suggest("Turn on download.verify (Settings › Downloads › Verification) to catch corrupted downloads.");
    }
    let Some(db) = &ctx.db else {
        return Finding::skipped("Needs the database.");
    };
    let failures = match db.verify().open_download_failures(RECENT_DAYS).await {
        Ok(n) => n,
        Err(e) => return Finding::skipped(format!("Could not read verify problems: {e}")),
    };
    let last_run = match db.verify().latest_run().await {
        Ok(run) => run,
        Err(e) => return Finding::skipped(format!("Could not read verify runs: {e}")),
    };
    let mismatches = match &last_run {
        Some(run) => db.verify().open_mismatches(run.id).await.unwrap_or(0),
        None => 0,
    };
    let last = match &last_run {
        Some(run) => format!(
            " The last verify run {} at {} UTC.",
            match run.state.as_str() {
                "running" => "started",
                "stopped" => "was stopped, started",
                "failed" => "failed, started",
                _ => "ran",
            },
            run.started_at
        ),
        None => " No verify run yet.".to_string(),
    };
    if failures == 0 && mismatches == 0 {
        return Finding::ok(format!("New downloads are checked.{last}"));
    }
    let mut parts = Vec::new();
    if failures > 0 {
        parts.push(format!(
            "{} gave up on a CRC32 mismatch in the last {RECENT_DAYS} days",
            count(failures as usize, "download")
        ));
    }
    if mismatches > 0 {
        parts.push(format!(
            "the last run found {} not matching Put.io",
            count(mismatches as usize, "file")
        ));
    }
    let last_id = last_run.as_ref().map(|run| run.id);
    let items = db
        .verify()
        .open_problems(ITEMS)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|p| {
            (p.source == "download" && p.result == "failed")
                || (p.run_id == last_id && p.result == "mismatch")
        })
        .map(|p| {
            let item = Item::new(p.file_id, p.local_path.unwrap_or(p.name));
            match p.detail {
                Some(detail) => item.detail(detail),
                None => item,
            }
        })
        .collect();
    let mut summary = parts.join("; ");
    if let Some(first) = summary.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    Finding::new(Status::Warning, format!("{summary}.{last}"))
        .suggest("See History › Verification; a mismatch is downloaded again unless the run was report only.")
        .items(items)
}
