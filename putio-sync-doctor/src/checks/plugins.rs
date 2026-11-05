//! Lua plugins (feature 028): no enabled plugin is broken, waits for approval or a value,
//! settings name only plugins that exist, and no plugin's last run failed in the last day

use super::{count, Ctx};
use crate::report::{Finding, Item, Status};
use chrono::{DateTime, Duration, Utc};
use putio_sync_core::plugins::dir_for;
use putio_sync_plugins::{PluginState, PluginStatus, PluginsOptions, PluginsService, RunResult};

/// A failed last run older than this is not reported
const RECENT: Duration = Duration::hours(24);

/// The running service's view, or an unstarted one built from the config
fn status(ctx: &Ctx) -> Vec<PluginStatus> {
    match &ctx.plugins {
        Some(service) => service.status(),
        None => PluginsService::new(PluginsOptions {
            dir: dir_for(
                ctx.config.plugins_dir.as_deref(),
                ctx.config_path.as_deref(),
            ),
            data_dir: None,
            settings: ctx.config.plugins.clone(),
            clock: None,
            download_dir: None,
        })
        .status(),
    }
}

/// The last run, when it failed in the last day
fn recent_failure(p: &PluginStatus, since: DateTime<Utc>) -> Option<Item> {
    let run = p.runs.last()?;
    let started = DateTime::parse_from_rfc3339(&run.started_at).ok()?;
    if run.result == RunResult::Success || started < since {
        return None;
    }
    let error = run
        .error
        .clone()
        .unwrap_or_else(|| run.result.as_str().to_string());
    Some(Item::new(&p.name, p.name.clone()).detail(format!(
        "{error} ({}, {})",
        run.event_type,
        started.with_timezone(&Utc).format("%Y-%m-%d %H:%M UTC")
    )))
}

pub(crate) async fn plugins(ctx: &Ctx) -> Finding {
    let plugins = status(ctx);
    let enabled = |name: &str| {
        ctx.config
            .plugins
            .iter()
            .any(|s| s.enabled && s.name == name)
    };
    let item = |p: &PluginStatus| {
        let item = Item::new(&p.name, p.name.clone());
        match &p.error {
            Some(e) => item.detail(e.clone()),
            None => item,
        }
    };

    let broken: Vec<Item> = plugins
        .iter()
        .filter(|p| p.state == PluginState::Broken && enabled(&p.name))
        .map(item)
        .collect();
    if !broken.is_empty() {
        return Finding::new(
            Status::Problem,
            format!("{} cannot load.", count(broken.len(), "enabled plugin")),
        )
        .suggest("Fix the plugin's plugin.toml or main.lua and press Reload plugins, or turn it off in Settings › Plugins.")
        .items(broken);
    }

    let since = Utc::now() - RECENT;
    let mut warnings: Vec<Item> = plugins
        .iter()
        .filter(|p| {
            matches!(
                p.state,
                PluginState::NeedsApproval | PluginState::MissingConfig
            )
        })
        .map(item)
        .collect();
    warnings.extend(
        ctx.config
            .plugins
            .iter()
            .filter(|s| !plugins.iter().any(|p| p.name == s.name))
            .map(|s| Item::new(&s.name, s.name.clone()).detail("no plugin of this name was found")),
    );
    warnings.extend(
        plugins
            .iter()
            .filter(|p| enabled(&p.name))
            .filter_map(|p| recent_failure(p, since)),
    );
    if !warnings.is_empty() {
        return Finding::new(
            Status::Warning,
            format!(
                "{} {} attention.",
                count(warnings.len(), "plugin"),
                if warnings.len() == 1 { "needs" } else { "need" }
            ),
        )
        .suggest("Open Settings › Plugins: save to approve hosts, fill in required settings, or use Send test event.")
        .items(warnings);
    }

    let on = plugins
        .iter()
        .filter(|p| p.state == PluginState::Enabled)
        .count();
    if on == 0 {
        return Finding::ok(format!(
            "{} found, none on.",
            count(plugins.len(), "plugin")
        ));
    }
    Finding::ok(format!(
        "{} found, {on} on; none failed in the last 24 hours.",
        count(plugins.len(), "plugin")
    ))
}
