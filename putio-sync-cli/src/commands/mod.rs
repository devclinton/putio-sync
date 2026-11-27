//! Command implementations for putio-sync CLI

use putio_sync_core::t;

#[cfg(feature = "server")]
pub mod archive;
pub mod clean_empty;
pub mod crash_report;
pub mod create;
#[cfg(feature = "server")]
pub mod daemon;
pub mod delete;
pub mod doctor;
pub mod download;
pub mod free_space;
pub mod history;
pub mod languages;
pub mod list;
pub mod login;
#[cfg(feature = "plugins")]
pub mod plugins;
pub mod queue;
#[cfg(feature = "server")]
pub mod server;
#[cfg(feature = "server")]
pub mod setup;
#[cfg(feature = "server")]
pub mod setup_mode;
pub mod stat;
pub mod status;
pub mod transfers;
pub mod usage_stats;

#[cfg(feature = "server")]
pub use archive::execute_archive;
pub use clean_empty::execute_clean_empty;
pub use create::execute_create;
#[cfg(feature = "server")]
pub use daemon::execute_daemon;
pub use delete::execute_delete;
pub use doctor::execute_doctor;
pub use download::execute_download;
pub use free_space::execute_free_space;
pub use history::execute_history;
pub use list::execute_list;
pub use login::execute_login;
#[cfg(feature = "server")]
pub use server::execute_server;
#[cfg(feature = "server")]
pub use setup::execute_setup;
pub use stat::execute_stat;
pub use status::{execute_debug, execute_health, execute_status};

/// The config a running `daemon` holds for the API: `[api]` token resolved (the
/// environment override wins over the file) when the API listens, as `api_listener::start`
/// writes it into the state. The hub starts from this, so a hand edit of the file that
/// changes nothing compares equal to it.
#[cfg(feature = "server")]
pub(crate) fn daemon_runtime_config(
    mut config: putio_sync_core::config::Config,
) -> putio_sync_core::error::CoreResult<putio_sync_core::config::Config> {
    let api = config.resolve_api()?.value;
    if api.listen {
        config.api.auth_token = api.token;
    }
    Ok(config)
}

/// What a reload does to a config read from the file before it can be applied: the
/// startup validation, then what `daemon` does to the config it holds. A config that
/// fails is refused and the running one stays.
#[cfg(feature = "server")]
pub(crate) fn daemon_reload(
    config: putio_sync_core::config::Config,
) -> putio_sync_core::error::CoreResult<putio_sync_core::config::Config> {
    config.validate()?;
    daemon_runtime_config(config)
}

/// The same for `server`: the token applies in every case, the environment wins
#[cfg(feature = "server")]
pub(crate) fn server_reload(
    mut config: putio_sync_core::config::Config,
) -> putio_sync_core::error::CoreResult<putio_sync_core::config::Config> {
    config.validate()?;
    config.api.auth_token = config.resolve_api()?.value.token;
    Ok(config)
}

/// The error for a missing OAuth token, with the next step (the same for every command)
pub(crate) fn missing_token() -> anyhow::Error {
    anyhow::anyhow!(
        "No Put.io token is configured. Run 'putio-sync login' to sign in, or run 'putio-sync setup' \
         (it reads PUTIO_SYNC_PUTIO_TOKEN), or set [auth] token in the config file"
    )
}

/// The error for a missing app-password username
pub(crate) fn missing_username() -> anyhow::Error {
    anyhow::anyhow!(
        "No Put.io username is configured for app-password sign-in. Set [auth] username in the \
         config file, or run 'putio-sync login' to use an OAuth token instead"
    )
}

/// The error for a missing app password
pub(crate) fn missing_password() -> anyhow::Error {
    anyhow::anyhow!(
        "No Put.io app password is configured. Set [auth] password in the config file, or run \
         'putio-sync login' to use an OAuth token instead"
    )
}

/// Check a `--filter` value like a saved folder-rule pattern (feature 020): text that
/// can't mean what was typed stops the command, and warnings go to stderr
pub(crate) fn check_filter(text: &str) -> anyhow::Result<()> {
    let warnings = putio_sync_core::pattern::check(text)
        .map_err(|e| anyhow::anyhow!("invalid --filter {text:?}: {e}"))?;
    for warning in warnings {
        eprintln!(
            "{}",
            t!("Warning: --filter {0}: {1}", format!("{:?}", text), warning)
        );
    }
    Ok(())
}
