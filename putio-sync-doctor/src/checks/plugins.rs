//! Lua plugins (feature 028): no enabled plugin is broken, waits for approval or a value,
//! settings name only plugins that exist, and no plugin's last run failed in the last day.
//! Feature 031: each enabled plugin with a `health()` says how it stands (server reachable,
//! token accepted, folders mapped); a failed item is a problem, a warning a warning.
//! Feature 059: a plugin from the registry that was withdrawn or is no longer listed is a
//! warning, from the list last fetched (the check fetches nothing).
//! Feature 057: a pipeline step that names a plugin which cannot run is a problem.

use super::{count, Ctx};
use crate::report::{Finding, Item, Status};
use chrono::{DateTime, Duration, Utc};
use putio_sync_core::msg;
use putio_sync_core::step_plugins::StepPlugins;
use putio_sync_plugins::action::{HealthItem, HealthStatus};
use putio_sync_plugins::{CallError, PluginState, PluginStatus, PluginsService, RunResult};
use std::sync::Arc;

/// A failed last run older than this is not reported
const RECENT: Duration = Duration::hours(24);

/// The running service, or an unstarted one built from the config
fn service(ctx: &Ctx) -> Arc<PluginsService> {
    match &ctx.plugins {
        Some(service) => service.clone(),
        None => Arc::new(PluginsService::from_config(
            &ctx.config,
            ctx.config_path.as_deref(),
        )),
    }
}

/// What each enabled plugin's `health()` says, as problems and warnings; a plugin busy
/// with a settings action is skipped
async fn health(service: &PluginsService, plugins: &[PluginStatus]) -> (Vec<Item>, Vec<Item>) {
    let mut problems = Vec::new();
    let mut warnings = Vec::new();
    for p in plugins.iter().filter(|p| p.state == PluginState::Enabled) {
        let items: Vec<HealthItem> = match service.health(&p.name).await {
            Ok(Some(items)) => items,
            Ok(None) => continue,
            Err(CallError::Busy) => {
                warnings.push(Item::new(&p.name, p.name.clone()).detail("busy, skipped"));
                continue;
            }
            Err(e) => vec![HealthItem {
                status: HealthStatus::Fail,
                message: e.to_string(),
            }],
        };
        for item in items {
            let found = Item::new(&p.name, p.name.clone()).detail(item.message);
            match item.status {
                HealthStatus::Ok => {}
                HealthStatus::Warn => warnings.push(found),
                HealthStatus::Fail => problems.push(found),
            }
        }
    }
    (problems, warnings)
}

/// Why a plugin from the registry needs a look: withdrawn, or gone from the list
fn registry_warning(p: &PluginStatus) -> Option<Item> {
    let info = p.registry.as_ref()?;
    let detail = match (&info.withdrawn, info.listed) {
        (Some(reason), _) => format!("withdrawn from the plugin registry: {reason}"),
        (None, Some(false)) => "no longer in the plugin registry".to_string(),
        _ => return None,
    };
    Some(Item::new(&p.name, p.name.clone()).detail(detail))
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
    let service = service(ctx);
    let plugins = service.status();
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
    // Feature 057: a pipeline step that names a plugin which is not installed, off,
    // broken or has no on_item fails every item that reaches it
    let steps: Vec<Item> = ctx
        .config
        .hooks
        .iter()
        .filter(|h| h.enabled && h.stage.is_some())
        .filter_map(|h| {
            let why = service.problem(h.plugin.as_deref()?)?;
            Some(Item::new(&h.name, h.name.clone()).detail(why))
        })
        .collect();
    if broken.is_empty() && !steps.is_empty() {
        return Finding::new(
            Status::Problem,
            format!(
                "{} cannot run: the plugin is missing, off or broken.",
                count(steps.len(), "pipeline step")
            ),
        )
        .suggest("Install or turn on the plugin in Settings › Plugins, or change the step under Settings › Integrations.")
        .items(steps);
    }
    let (unhealthy, unsure) = health(&service, &plugins).await;
    if broken.is_empty() && !unhealthy.is_empty() {
        return Finding::new(
            Status::Problem,
            msg!("{0} failed.", count(unhealthy.len(), "plugin check")))
        .suggest(msg!("Open Settings › Plugins: check the server address, sign in again, or fix the path rules."))
        .items(unhealthy);
    }
    if !broken.is_empty() {
        return Finding::new(
            Status::Problem,
            msg!("{0} cannot load.", count(broken.len(), "enabled plugin")))
        .suggest(msg!("Fix the plugin's plugin.toml or main.lua and press Reload plugins, or turn it off in Settings › Plugins."))
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
    warnings.extend(plugins.iter().filter_map(registry_warning));
    warnings.extend(unsure);
    if !warnings.is_empty() {
        return Finding::new(
            Status::Warning,
            msg!("{0} {1} attention.", count(warnings.len(), "plugin"), if warnings.len() == 1 { "needs" } else { "need" }))
        .suggest(msg!("Open Settings › Plugins: save to approve hosts, fill in required settings, use Send test event, or remove a plugin the registry no longer lists."))
        .items(warnings);
    }

    let on = plugins
        .iter()
        .filter(|p| p.state == PluginState::Enabled)
        .count();
    if on == 0 {
        return Finding::ok(msg!("{0} found, none on.", count(plugins.len(), "plugin")));
    }
    Finding::ok(msg!(
        "{0} found, {1} on; none failed in the last 24 hours.",
        count(plugins.len(), "plugin"),
        on
    ))
}
