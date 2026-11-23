//! `putio-sync usage-stats show|enable|disable` (feature 058, part 2)
//!
//! `show` prints the exact JSON that would be sent, without sending it. `enable` and `disable`
//! change `[usage_stats] enabled` in the config file. Nothing is sent from here.

use putio_sync_core::config::Config;
use putio_sync_core::usage_stats::{self as stats, Inputs};
use std::io::Write;
use std::path::{Path, PathBuf};

/// What to do
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageStatsCommands {
    /// Print the payload
    Show,
    /// Turn the daily send on
    Enable,
    /// Turn it off
    Disable,
}

fn config_file(config: Option<&Path>) -> Option<PathBuf> {
    config
        .map(Path::to_path_buf)
        .or_else(|| Config::default_path().ok())
}

/// The counts the payload needs, from the database when there is one
async fn inputs(config: &Config, config_path: &Path) -> Inputs {
    #[cfg(feature = "server")]
    let build = putio_sync_update::channel::Channel::detect()
        .kind()
        .to_string();
    #[cfg(not(feature = "server"))]
    let build = "unknown".to_string();
    let mut inputs = Inputs {
        build,
        ..Inputs::default()
    };
    let db_path = putio_sync_core::database::path_for(config, Some(config_path));
    if db_path.exists() {
        if let Ok(db) = putio_sync_db::Database::new(&db_path).await {
            let since = (chrono::Utc::now() - chrono::Duration::days(7))
                .format("%Y-%m-%d %H:%M:%S")
                .to_string();
            let repo = db.stats();
            inputs.downloads_7d = repo
                .get_for_range(Some(&since), None)
                .await
                .map_or(0, |(n, _, _)| n.max(0) as u64);
            inputs.library_bytes = repo.library_bytes().await.map_or(0, |b| b.max(0) as u64);
        }
    }
    inputs.crash_reports = putio_sync_core::crash::dir_for(Some(config_path))
        .is_some_and(|d| !putio_sync_core::crash::list(&d).is_empty());
    inputs
}

/// Run the command, writing to `out`. Exit code: 0 done, 1 error.
pub async fn execute(
    command: UsageStatsCommands,
    config_arg: Option<&Path>,
    out: &mut impl Write,
) -> i32 {
    let Some(path) = config_file(config_arg) else {
        eprintln!("Could not find the config file");
        return 1;
    };
    let mut config = if path.exists() {
        match Config::load(&path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Could not read {}: {e}", path.display());
                return 1;
            }
        }
    } else {
        Config::default()
    };
    match command {
        UsageStatsCommands::Show => {
            let inputs = inputs(&config, &path).await;
            let Some(file) = stats::state_path(Some(&path)) else {
                eprintln!("Could not find the config folder");
                return 1;
            };
            let id = stats::load(&file).install_id;
            let json = serde_json::to_string_pretty(&stats::payload(&config, &inputs, &id))
                .unwrap_or_default();
            let status = match (config.usage_stats.enabled, config.usage_stats.active()) {
                (false, _) => "off: nothing is sent",
                (true, false) => {
                    "on, but no receiver is set up yet ([usage_stats] endpoint): nothing is sent"
                }
                (true, true) => "on: sent about once a day",
            };
            let _ = writeln!(
                out,
                "Usage stats are {status}. This is what would be sent:\n"
            );
            let _ = writeln!(out, "{json}");
            0
        }
        UsageStatsCommands::Enable | UsageStatsCommands::Disable => {
            let on = command == UsageStatsCommands::Enable;
            if !path.exists() {
                eprintln!("No config file at {}", path.display());
                return 1;
            }
            config.usage_stats.enabled = on;
            if let Err(e) = config.save_keeping_file_credentials(&path) {
                eprintln!("{e}");
                return 1;
            }
            let _ = writeln!(
                out,
                "Usage stats are now {}. A running putio-sync picks this up after a restart, or turn it {} in Settings.",
                if on { "on" } else { "off" },
                if on { "off" } else { "on" },
            );
            if on && config.usage_stats.endpoint.trim().is_empty() {
                let _ = writeln!(
                    out,
                    "No receiver is set up yet ([usage_stats] endpoint is empty), so nothing is sent."
                );
            }
            0
        }
    }
}
