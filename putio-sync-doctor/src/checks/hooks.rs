//! Event hooks (feature 027): every enabled command hook's program can run, and no hook's
//! last run failed in the last day

use super::{count, Ctx};
use crate::report::{Finding, Item, Status};
use chrono::{Duration, Utc};
use putio_sync_hooks::RunResult;
use std::path::Path;

/// A failed last run older than this is not reported
const RECENT: Duration = Duration::hours(24);

/// Why `program` cannot be run, if it cannot
fn not_runnable(program: &Path) -> Option<&'static str> {
    let meta = match std::fs::metadata(program) {
        Ok(meta) => meta,
        Err(_) => return Some("not found"),
    };
    if !meta.is_file() {
        return Some("not a file");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o111 == 0 {
            return Some("not executable");
        }
    }
    None
}

pub(crate) async fn hooks(ctx: &Ctx) -> Finding {
    let hooks = &ctx.config.hooks;
    if hooks.is_empty() {
        return Finding::ok("No hooks are set up.");
    }
    let broken: Vec<Item> = hooks
        .iter()
        .filter(|h| h.enabled)
        .filter_map(|h| {
            let program = h.command.as_ref()?.first()?;
            let why = not_runnable(Path::new(program))?;
            Some(Item::new(&h.name, h.name.clone()).detail(format!("{program}: {why}")))
        })
        .collect();
    if !broken.is_empty() {
        return Finding::new(
            Status::Problem,
            format!("{} cannot run its program.", count(broken.len(), "hook")),
        )
        .suggest("Fix the program path, make it executable (chmod +x), or turn the hook off.")
        .items(broken);
    }

    let since = Utc::now() - RECENT;
    let failed: Vec<Item> = ctx
        .hooks
        .as_ref()
        .map(|service| service.status())
        .unwrap_or_default()
        .into_iter()
        .filter(|s| hooks.iter().any(|h| h.enabled && h.name == s.name))
        .filter_map(|s| {
            let run = s.runs.first()?;
            if run.result == RunResult::Success || run.started_at < since {
                return None;
            }
            let error = run
                .error
                .clone()
                .unwrap_or_else(|| run.result.as_str().to_string());
            Some(Item::new(&s.name, s.name.clone()).detail(format!(
                "{error} ({}, {})",
                run.event_type,
                run.started_at.format("%Y-%m-%d %H:%M UTC")
            )))
        })
        .collect();
    let enabled = hooks.iter().filter(|h| h.enabled).count();
    if !failed.is_empty() {
        return Finding::new(
            Status::Warning,
            format!(
                "The last run of {} failed in the last 24 hours.",
                count(failed.len(), "hook")
            ),
        )
        .suggest(
            "See the hook's output in the log, then use Send test event in Settings › Event hooks.",
        )
        .items(failed);
    }
    Finding::ok(format!(
        "{} set up, {enabled} on; none failed in the last 24 hours.",
        count(hooks.len(), "hook")
    ))
}

#[cfg(test)]
mod tests {
    use super::not_runnable;

    #[test]
    fn programs_that_cannot_run_say_why() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(not_runnable(&dir.path().join("missing")), Some("not found"));
        assert_eq!(not_runnable(dir.path()), Some("not a file"));
        let script = dir.path().join("hook.sh");
        std::fs::write(&script, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert_eq!(not_runnable(&script), Some("not executable"));
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        assert_eq!(not_runnable(&script), None);
    }
}
