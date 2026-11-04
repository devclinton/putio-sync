//! Put.io account checks: sign-in, storage, transfers and leftovers in the account

use super::{age, bytes, count, verb, Ctx, STALE_AGE};
use crate::report::{Finding, FixId, Item, Status};
use putio_sync_client::{ClientError, Transfer};
use putio_sync_core::cleanup::{CleanupError, CleanupTrigger};
use std::collections::{HashMap, HashSet};
use std::time::SystemTime;

/// Storage use that gives a warning / a problem, in percent
/// Free space, in percent of the account, under which storage is a problem; the warning
/// level is `[storage] warn_free_percent` (feature 033)
const STORAGE_PROBLEM_FREE: f64 = 2.0;
/// Smaller files are not worth reporting as duplicates
const DUPLICATE_MIN_SIZE: i64 = 1024 * 1024;

const SIGN_IN_HINT: &str =
    "Sign in again with `putio-sync login`, or put a new token in the config file.";

pub(crate) fn is_errored(transfer: &Transfer) -> bool {
    matches!(transfer.status.as_str(), "ERROR" | "FAILED")
}

pub(crate) async fn auth(ctx: &Ctx) -> Finding {
    let (probe, _) = ctx.probe().await;
    match &probe.result {
        Ok(info) => Finding::ok(format!("Signed in as {}.", info.username)),
        Err(ClientError::AuthError(_)) => {
            Finding::new(Status::Problem, "No Put.io token is configured.").suggest(SIGN_IN_HINT)
        }
        Err(_) if probe.unauthorized() => Finding::new(
            Status::Problem,
            "Put.io rejected the token; it may have expired or been revoked.",
        )
        .suggest(SIGN_IN_HINT),
        Err(_) if probe.rate_limited() => Finding::skipped(
            "Put.io is limiting requests right now, so the sign-in could not be checked.",
        ),
        Err(_) if probe.unreachable() => {
            Finding::skipped("Put.io could not be reached; see the reachability check.")
        }
        Err(e) => Finding::new(
            Status::Warning,
            format!("Put.io gave an unexpected answer: {e}"),
        )
        .suggest("Try again in a few minutes."),
    }
}

pub(crate) async fn storage(ctx: &Ctx) -> Finding {
    let (probe, _) = ctx.probe().await;
    let Ok(info) = &probe.result else {
        return Finding::skipped("Put.io could not be used.");
    };
    let disk = &info.disk;
    if disk.size <= 0 {
        return Finding::skipped("Put.io did not report the account size.");
    }
    let pct = disk.used as f64 * 100.0 / disk.size as f64;
    let free_pct = 100.0 - pct;
    let warn = ctx.config.storage.warn_free_percent;
    let summary = format!(
        "Using {} of {} ({:.0}%).",
        bytes(disk.used.max(0) as u64),
        bytes(disk.size as u64),
        pct
    );
    let status = if free_pct < STORAGE_PROBLEM_FREE {
        Status::Problem
    } else if free_pct < warn as f64 {
        Status::Warning
    } else {
        Status::Ok
    };
    let summary = if status == Status::Ok {
        summary
    } else {
        format!("{summary} Less than {warn}% is free.")
    };
    Finding::new(status, summary).suggest(
        "Use Free up space on the Put.io page to delete the oldest files that are already \
         downloaded, or delete files you no longer need; new transfers fail when the account \
         is full.",
    )
}

pub(crate) async fn transfers(ctx: &Ctx) -> Finding {
    let transfers = match ctx.transfers().await {
        Ok(t) => t,
        Err(e) => {
            return Finding::new(Status::Warning, format!("Could not list transfers: {e}"))
                .suggest("Try again in a few minutes.")
        }
    };
    let mut errored = Vec::new();
    let mut stalled = Vec::new();
    for t in transfers {
        if is_errored(t) {
            let item = Item::new(t.id, &t.name).fix(FixId::TransfersRetry);
            errored.push(match &t.error_message {
                Some(msg) if !msg.is_empty() => item.detail(msg.clone()),
                _ => item,
            });
        } else if let Some(stall) = ctx.stalls.as_ref().and_then(|s| s.get(t.id)) {
            let still = SystemTime::now()
                .duration_since(stall.since)
                .unwrap_or_default();
            let detail = format!("no new data for {} ({})", age(still), stall.reason.label());
            let item = Item::new(t.id, &t.name);
            stalled.push(if t.magnet().is_some() {
                item.detail(detail).fix(FixId::TransfersReadd)
            } else {
                item.detail(format!("{detail}; no magnet link, so it can't be re-added"))
            });
        }
    }

    let mut summary = if errored.is_empty() && stalled.is_empty() {
        format!(
            "{}, none failed or stalled.",
            count(transfers.len(), "transfer")
        )
    } else {
        let mut parts = Vec::new();
        if !errored.is_empty() {
            parts.push(format!("{} failed", count(errored.len(), "transfer")));
        }
        if !stalled.is_empty() {
            parts.push(format!("{} stalled", count(stalled.len(), "transfer")));
        }
        format!("{}.", parts.join(" and "))
    };
    if ctx.stalls.is_none() {
        summary.push_str(" Stall detection needs the daemon running.");
    }
    let status = if !errored.is_empty() {
        Status::Problem
    } else if !stalled.is_empty() {
        Status::Warning
    } else {
        Status::Ok
    };
    errored.extend(stalled);
    Finding::new(status, summary)
        .suggest("Retry failed transfers. Re-add stalled ones to look for peers again.")
        .items(errored)
}

pub(crate) async fn empty_folders(ctx: &Ctx) -> Finding {
    match ctx.cleanup.run(CleanupTrigger::Manual, true).await {
        Ok(report) => {
            let items: Vec<Item> = report
                .removed
                .iter()
                .map(|f| {
                    Item::new(f.id, &f.path)
                        .detail(format!(
                            "{} old",
                            age(std::time::Duration::from_secs(f.age_hours * 3600))
                        ))
                        .fix(FixId::EmptyFoldersClean)
                })
                .collect();
            let mut summary = if items.is_empty() {
                "No empty folders to remove.".to_string()
            } else {
                format!("{} could be removed.", count(items.len(), "empty folder"))
            };
            if !report.failures.is_empty() {
                summary.push_str(&format!(
                    " {} could not be checked.",
                    count(report.failures.len(), "folder")
                ));
            }
            let status = if items.is_empty() {
                Status::Ok
            } else {
                Status::Warning
            };
            Finding::new(status, summary)
                .suggest("Clean them up; you will see the list before anything is deleted.")
                .items(items)
        }
        Err(CleanupError::AlreadyRunning) => {
            Finding::skipped("An empty-folder cleanup is running right now.")
        }
        Err(e) => Finding::new(
            Status::Warning,
            format!("Could not look for empty folders: {e}"),
        )
        .suggest("Try again in a few minutes."),
    }
}

fn incomplete_note(complete: bool) -> &'static str {
    if complete {
        ""
    } else {
        " The account listing did not finish, so this may be incomplete."
    }
}

pub(crate) async fn duplicates(ctx: &Ctx) -> Finding {
    let walk = ctx.walk().await;
    let mut groups: HashMap<(&str, i64), Vec<&putio_sync_client::WalkedFile>> = HashMap::new();
    for f in walk
        .files
        .iter()
        .filter(|f| !f.is_folder && f.size >= DUPLICATE_MIN_SIZE)
    {
        groups.entry((f.name.as_str(), f.size)).or_default().push(f);
    }
    let mut dupes: Vec<_> = groups.into_values().filter(|g| g.len() > 1).collect();
    // Most wasted space first
    dupes.sort_by_key(|g| std::cmp::Reverse(g[0].size * (g.len() as i64 - 1)));
    let wasted: i64 = dupes.iter().map(|g| g[0].size * (g.len() as i64 - 1)).sum();
    let items: Vec<Item> = dupes
        .iter()
        .map(|g| {
            let ids: Vec<String> = g.iter().map(|f| f.id.to_string()).collect();
            let paths: Vec<&str> = g.iter().map(|f| f.path.as_str()).collect();
            Item::new(ids.join(","), &g[0].name).detail(format!(
                "{} copies, {} each: {}",
                g.len(),
                bytes(g[0].size as u64),
                paths.join(", ")
            ))
        })
        .collect();
    let summary = if items.is_empty() {
        "No duplicate files found.".to_string()
    } else {
        format!(
            "{} with more than one copy, using {} extra.",
            count(items.len(), "file"),
            bytes(wasted as u64)
        )
    };
    let status = if items.is_empty() {
        Status::Ok
    } else {
        Status::Warning
    };
    Finding::new(status, summary + incomplete_note(walk.complete))
        .suggest("Delete the copies you don't need from Put.io.")
        .items(items)
}

pub(crate) async fn orphaned_torrents(ctx: &Ctx) -> Finding {
    let names: HashSet<String> = match ctx.transfers().await {
        Ok(t) => t.iter().map(|t| t.name.to_lowercase()).collect(),
        Err(e) => return Finding::skipped(format!("Could not list transfers: {e}")),
    };
    let walk = ctx.walk().await;
    let items: Vec<Item> = walk
        .files
        .iter()
        .filter(|f| f.extension.as_deref() == Some("torrent"))
        .filter(|f| {
            let stem = &f.name[..f.name.len() - ".torrent".len()];
            !names.contains(&stem.to_lowercase())
        })
        .map(|f| Item::new(f.id, &f.path).detail(bytes(f.size.max(0) as u64)))
        .collect();
    let summary = if items.is_empty() {
        "Every .torrent file belongs to a transfer.".to_string()
    } else {
        format!(
            "{} in the account {} no transfer.",
            count(items.len(), ".torrent file"),
            verb(items.len(), "matches", "match")
        )
    };
    let status = if items.is_empty() {
        Status::Ok
    } else {
        Status::Warning
    };
    Finding::new(status, summary + incomplete_note(walk.complete))
        .suggest("Add them as transfers if you still want them, or delete them from Put.io.")
        .items(items)
}

pub(crate) async fn stuck_watch_files(ctx: &Ctx) -> Finding {
    let watch = &ctx.config.watch;
    if !watch.enabled {
        return Finding::skipped("Watching for .torrent and .magnet files is off.");
    }
    let Some(db) = &ctx.db else {
        return Finding::skipped("Needs the database to know which files were uploaded.");
    };
    let processed = match db.processed_torrents().paths().await {
        Ok(paths) => paths,
        Err(e) => return Finding::skipped(format!("Could not read the database: {e}")),
    };
    let processed_names: HashSet<&str> = processed
        .iter()
        .filter_map(|p| std::path::Path::new(p).file_name()?.to_str())
        .collect();

    let mut items = Vec::new();
    let mut unreadable = Vec::new();
    let now = SystemTime::now();
    for dir in &watch.paths {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(_) => {
                unreadable.push(dir.display().to_string());
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let ext = path
                .extension()
                .and_then(|e| e.to_str())
                .map(str::to_ascii_lowercase);
            if !matches!(ext.as_deref(), Some("torrent" | "magnet")) {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            let waiting = meta
                .modified()
                .ok()
                .and_then(|m| now.duration_since(m).ok())
                .unwrap_or_default();
            let path_str = path.display().to_string();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !meta.is_file()
                || waiting < STALE_AGE
                || processed.contains(&path_str)
                || processed_names.contains(name)
            {
                continue;
            }
            items.push(Item::new(&path_str, name).detail(format!("waiting {}", age(waiting))));
        }
    }

    let mut summary = if items.is_empty() {
        "No watch files are waiting.".to_string()
    } else {
        format!(
            "{} waited over an hour without becoming a transfer.",
            count(items.len(), "file")
        )
    };
    if !unreadable.is_empty() {
        summary.push_str(&format!(" Could not read {}.", unreadable.join(", ")));
    }
    let status = if items.is_empty() && unreadable.is_empty() {
        Status::Ok
    } else {
        Status::Warning
    };
    Finding::new(status, summary)
        .suggest("Check the putio-sync logs for upload errors; the file may be invalid.")
        .items(items)
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use mockito::Server;

    #[tokio::test]
    async fn auth_ok_and_rejected() {
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/account/info")
            .with_body(ACCOUNT)
            .create_async()
            .await;
        let dir = tempfile::tempdir().unwrap();
        let f = auth(&ctx(&server.url(), dir.path(), None)).await;
        assert_eq!(f.status, Status::Ok);

        let mut server = Server::new_async().await;
        server
            .mock("GET", "/account/info")
            .with_status(401)
            .create_async()
            .await;
        let f = auth(&ctx(&server.url(), dir.path(), None)).await;
        assert_eq!(f.status, Status::Problem);
        assert!(f.suggestion.unwrap().contains("login"));
    }

    async fn storage_at(used: i64) -> Status {
        storage_with(used, 10).await.status
    }

    async fn storage_with(used: i64, warn_free_percent: u8) -> Finding {
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/account/info")
            .with_body(ACCOUNT.replace("\"used\":100000000000", &format!("\"used\":{used}")))
            .create_async()
            .await;
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = ctx(&server.url(), dir.path(), None);
        ctx.config.storage.warn_free_percent = warn_free_percent;
        storage(&ctx).await
    }

    #[tokio::test]
    async fn storage_thresholds() {
        assert_eq!(storage_at(100_000_000_000).await, Status::Ok);
        assert_eq!(storage_at(930_000_000_000).await, Status::Warning);
        assert_eq!(storage_at(985_000_000_000).await, Status::Problem);
    }

    #[tokio::test]
    async fn storage_uses_the_configured_warning_level() {
        // 40% free: fine at 10%, a warning at 50%
        assert_eq!(storage_with(600_000_000_000, 10).await.status, Status::Ok);
        let f = storage_with(600_000_000_000, 50).await;
        assert_eq!(f.status, Status::Warning);
        assert!(f.summary.contains("Less than 50% is free"));
        assert!(f.suggestion.unwrap().contains("Free up space"));
        // Under 2% free is a problem whatever the level
        assert_eq!(
            storage_with(985_000_000_000, 1).await.status,
            Status::Problem
        );
    }

    #[tokio::test]
    async fn errored_transfers_offer_retry() {
        let mut server = Server::new_async().await;
        server
            .mock("GET", "/transfers/list")
            .with_body(transfer_list(&[
                transfer(1, "Good", "DOWNLOADING"),
                transfer(2, "Bad", "ERROR"),
            ]))
            .create_async()
            .await;
        let dir = tempfile::tempdir().unwrap();
        let f = super::transfers(&ctx(&server.url(), dir.path(), None)).await;
        assert_eq!(f.status, Status::Problem);
        assert_eq!(f.items.len(), 1);
        assert_eq!(f.items[0].key, "2");
        assert_eq!(f.items[0].fix, Some(FixId::TransfersRetry));
        assert!(f.summary.contains("needs the daemon"));
    }

    #[tokio::test]
    async fn stalled_transfer_offers_readd() {
        use putio_sync_core::stall::StallTracker;
        let mut server = Server::new_async().await;
        let body = transfer_list(&[transfer(5, "Slow", "DOWNLOADING")]);
        server
            .mock("GET", "/transfers/list")
            .with_body(&body)
            .create_async()
            .await;
        let tracker = std::sync::Arc::new(StallTracker::new(std::time::Duration::from_secs(60)));
        let list: putio_sync_client::TransfersListResponse = serde_json::from_str(&body).unwrap();
        let t0 = SystemTime::now() - std::time::Duration::from_secs(3600);
        tracker.observe(&list.transfers, t0);
        tracker.observe(&list.transfers, SystemTime::now());

        let dir = tempfile::tempdir().unwrap();
        let mut c = ctx(&server.url(), dir.path(), None);
        c.stalls = Some(tracker);
        let f = super::transfers(&c).await;
        assert_eq!(f.status, Status::Warning);
        assert_eq!(f.items[0].fix, Some(FixId::TransfersReadd));
    }

    #[tokio::test]
    async fn duplicates_and_orphaned_torrents_from_one_walk() {
        let mut server = Server::new_async().await;
        let walk = server
            .mock("GET", "/files/list?parent_id=0")
            .with_body(listing(&[
                entry(1, "A", 0, true),
                entry(2, "film.mkv", 5_000_000, false),
                entry(3, "small.txt", 10, false),
                entry(4, "small.txt", 10, false),
                entry(5, "Kept.torrent", 100, false),
                entry(6, "Lost.torrent", 100, false),
            ]))
            .expect(1)
            .create_async()
            .await;
        server
            .mock("GET", "/files/list?parent_id=1")
            .with_body(listing(&[entry(7, "film.mkv", 5_000_000, false)]))
            .create_async()
            .await;
        server
            .mock("GET", "/transfers/list")
            .with_body(transfer_list(&[transfer(9, "kept", "COMPLETED")]))
            .create_async()
            .await;
        let dir = tempfile::tempdir().unwrap();
        let c = ctx(&server.url(), dir.path(), None);

        let d = duplicates(&c).await;
        assert_eq!(d.status, Status::Warning);
        assert_eq!(d.items.len(), 1, "small files are ignored");
        assert_eq!(d.items[0].key, "2,7");

        let o = orphaned_torrents(&c).await;
        assert_eq!(o.items.len(), 1);
        assert_eq!(o.items[0].key, "6");
        walk.assert_async().await;
    }

    #[tokio::test]
    async fn stuck_watch_files_ignores_new_and_processed() {
        let dir = tempfile::tempdir().unwrap();
        let watch = dir.path().join("watch");
        std::fs::create_dir_all(&watch).unwrap();
        let old = filetime::FileTime::from_unix_time(1_600_000_000, 0);
        for name in ["stuck.torrent", "done.magnet", "notes.txt"] {
            std::fs::write(watch.join(name), "x").unwrap();
            filetime::set_file_mtime(watch.join(name), old).unwrap();
        }
        std::fs::write(watch.join("new.torrent"), "x").unwrap();

        let db = db(dir.path()).await;
        db.processed_torrents()
            .record_processed(
                &watch.join("done.magnet").display().to_string(),
                "c",
                1,
                None,
            )
            .await
            .unwrap();
        let mut c = ctx("http://127.0.0.1:9", dir.path(), Some(db));
        c.config.watch.enabled = true;
        c.config.watch.paths = vec![watch.clone()];

        let f = stuck_watch_files(&c).await;
        assert_eq!(f.status, Status::Warning);
        let names: Vec<&str> = f.items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(names, vec!["stuck.torrent"]);

        c.config.watch.enabled = false;
        assert_eq!(stuck_watch_files(&c).await.status, Status::Skipped);
    }
}
