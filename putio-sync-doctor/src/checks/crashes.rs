//! Crash reports (feature 058): warn about a crash in the last 7 days that nobody dismissed

use super::Ctx;
use crate::report::{Finding, Item, Status};
use putio_sync_core::crash::{self, CrashReport, Kind};
use std::path::Path;

pub(crate) async fn crashes(ctx: &Ctx) -> Finding {
    let Some(dir) = crash::dir_for(ctx.config_path.as_deref()) else {
        return Finding::skipped("Skipped: there is no config folder to keep crash reports in.");
    };
    let recent = tokio::task::spawn_blocking(move || recent(&dir))
        .await
        .unwrap_or_default();
    finding(&recent)
}

fn recent(dir: &Path) -> Vec<CrashReport> {
    crash::recent_undismissed(dir)
}

pub(crate) fn finding(recent: &[CrashReport]) -> Finding {
    if recent.is_empty() {
        return Finding::ok(format!(
            "No crashes in the last {} days.",
            crash::RECENT_DAYS
        ));
    }
    let items = recent
        .iter()
        .map(|r| {
            let what = match r.kind {
                Kind::Panic => r.signature(),
                Kind::Unclean => "stopped without a report".to_string(),
            };
            Item::new(r.id.clone(), format!("{} · {what}", r.time)).detail(format!(
                "Version {}. Run `putio-sync crash-report show {}`.",
                r.version, r.id
            ))
        })
        .collect();
    Finding::new(
        Status::Warning,
        format!(
            "putio-sync crashed or stopped unexpectedly {} in the last {} days.",
            if recent.len() == 1 {
                "once".to_string()
            } else {
                format!("{} times", recent.len())
            },
            crash::RECENT_DAYS
        ),
    )
    .suggest(
        "Open the notice in the web UI, or run `putio-sync crash-report list`, check the report \
         and file it on GitHub if you like. Dismiss it to clear this warning.",
    )
    .items(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn report(dir: &Path) -> CrashReport {
        crash::write_panic(
            dir,
            Instant::now(),
            &crash::PanicDetails {
                thread: "main".into(),
                message: "boom".into(),
                location: None,
            },
            "   0: putio_sync_cli::main\n",
            vec![],
        )
        .unwrap()
    }

    #[test]
    fn an_undismissed_recent_crash_warns_until_dismissed() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(finding(&recent(dir.path())).status, Status::Ok);

        let r = report(dir.path());
        let warned = finding(&recent(dir.path()));
        assert_eq!(warned.status, Status::Warning);
        assert!(warned.summary.contains("once"), "{}", warned.summary);

        assert!(crash::dismiss(dir.path(), &r.id));
        assert_eq!(finding(&recent(dir.path())).status, Status::Ok);
    }

    #[test]
    fn an_old_crash_is_not_recent() {
        let dir = tempfile::tempdir().unwrap();
        let mut r = report(dir.path());
        r.time = "2020-01-01T00:00:00.000Z".into();
        std::fs::write(
            dir.path().join(format!("crash-{}.json", r.id)),
            serde_json::to_vec(&r).unwrap(),
        )
        .unwrap();
        assert_eq!(finding(&recent(dir.path())).status, Status::Ok);
    }
}
