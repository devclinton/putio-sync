//! Command implementations for putio-sync CLI

#[cfg(feature = "server")]
pub mod archive;
pub mod clean_empty;
pub mod create;
#[cfg(feature = "server")]
pub mod daemon;
pub mod delete;
pub mod doctor;
pub mod download;
pub mod free_space;
pub mod history;
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
        eprintln!("Warning: --filter {text:?}: {warning}");
    }
    Ok(())
}
