//! Command implementations for putio-sync CLI

pub mod clean_empty;
pub mod create;
pub mod daemon;
pub mod delete;
pub mod doctor;
pub mod download;
pub mod free_space;
pub mod history;
pub mod list;
pub mod login;
pub mod queue;
pub mod server;
pub mod setup;
pub mod setup_mode;
pub mod stat;
pub mod status;
pub mod transfers;

pub use clean_empty::execute_clean_empty;
pub use create::execute_create;
pub use daemon::execute_daemon;
pub use delete::execute_delete;
pub use doctor::execute_doctor;
pub use download::execute_download;
pub use free_space::execute_free_space;
pub use history::execute_history;
pub use list::execute_list;
pub use login::execute_login;
pub use server::execute_server;
pub use setup::execute_setup;
pub use stat::execute_stat;
pub use status::{execute_debug, execute_health, execute_status};

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
