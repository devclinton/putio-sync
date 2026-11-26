//! Applying a changed config to the running services, whoever changed it.
//!
//! [`apply_config`] is the one place that pushes a whole config to the services the API
//! holds. The import route calls it; so does the subscriber ([`spawn_config_subscriber`])
//! for a hand edit of the file. Settings saves keep their own per-route pushes (they know
//! which group changed) and only publish to the hub, so the subscriber leaves their
//! [`Source::Api`] snapshots alone, apart from `[logging]`, which has no route push.

use crate::{extract_download_config, AppState};
use putio_sync_core::config::Config;
use putio_sync_core::config_hub::Source;
use serde::Serialize;

/// True when the two values serialise differently
fn differs<T: Serialize>(a: &T, b: &T) -> bool {
    serde_json::to_value(a).ok() != serde_json::to_value(b).ok()
}

/// Apply `candidate` to every service the API holds, then make it the API's config.
///
/// Used where the whole config changed at once: an import, or a hand edit of the file.
/// The caller has already saved or validated it. Cheap services are told every time; the
/// ones that restart or rebind (media server, Radarr/Sonarr, updates, organiser,
/// Tailscale socket, log level) only when their own section changed.
///
/// Not live, by design: listener addresses, metrics, the Put.io token and the
/// `[logging]` outputs.
pub async fn apply_config(state: &AppState, candidate: Config) {
    let before = state.config.read().await.clone();

    state.doctor.configure(&candidate);
    putio_sync_core::log_redact::set_secrets(candidate.secret_values());
    state.cleanup.configure(&candidate);
    state.storage.configure(&candidate);
    state.hooks.configure(candidate.hooks.clone());
    state.email.configure(candidate.notifications.smtp.clone());
    if let Some(pipeline) = &state.pipeline {
        pipeline.configure(&candidate);
    }
    if candidate.download.target_directory != before.download.target_directory {
        // Plugins see the folder as putio.download_dir (031)
        state
            .plugins
            .set_download_dir(Some(candidate.download.target_directory.clone()));
    }
    state.plugins.configure(candidate.plugins.clone());
    let plugins_dir = putio_sync_core::plugins::dir_for(
        candidate.plugins_dir.as_deref(),
        state
            .config_path
            .as_deref()
            .map(std::path::PathBuf::as_path),
    );
    if plugins_dir != state.plugins.dir() {
        state.plugins.set_dir(plugins_dir);
    }

    if differs(&before.logging, &candidate.logging) {
        apply_logging(state, &candidate);
    }
    let socket = candidate.tailscale.socket_path();
    if state.tailscale.socket() != socket {
        state.tailscale.configure(socket);
    }
    if differs(&before.organise, &candidate.organise) {
        if let Some(organiser) = &state.organiser {
            organiser.configure(candidate.organise.clone());
        }
    }
    if differs(&before.updates, &candidate.updates) {
        if let Some(updates) = &state.updates {
            updates.configure(candidate.updates.clone());
        }
    }
    if differs(&before.dlna, &candidate.dlna) {
        if let Some(server) = &state.media_server {
            server.update(&candidate).await;
        }
    }
    if differs(&before.arr, &candidate.arr) {
        if let (Some(arr), Ok(resolved)) = (&state.arr, candidate.resolve_arr()) {
            arr.service.apply(resolved);
        }
    }

    let download = extract_download_config(&candidate);
    *state.config.write().await = candidate;
    crate::routes::system::settings::push_watcher(state).await;
    state.download_manager.update_config(download).await;
}

/// Apply `[logging]`: the level and module filters (unless `RUST_LOG` is set, or the
/// config file didn't exist at startup) and the Logs page buffer size
fn apply_logging(state: &AppState, config: &Config) {
    if state.logging_reload {
        let _ = putio_sync_core::logging::apply_level(&config.logging);
    }
    putio_sync_core::log_capture::set_capacity(config.logging.buffer_lines);
}

/// Follow the state's hub: a hand edit of the file ([`Source::File`]) is applied with
/// [`apply_config`]; a settings save ([`Source::Api`]) was applied by its route, so only
/// its `[logging]` is looked at here. Bursts coalesce: only the latest snapshot is used.
pub fn spawn_config_subscriber(state: AppState) -> tokio::task::JoinHandle<()> {
    let mut changes = state.hub.subscribe();
    tokio::spawn(async move {
        let mut logging = state.config.read().await.logging.clone();
        while changes.changed().await.is_ok() {
            let snapshot = changes.borrow_and_update().clone();
            let config = (*snapshot.config).clone();
            match snapshot.source {
                Source::File => apply_config(&state, config.clone()).await,
                Source::Api | Source::Startup => {
                    if differs(&logging, &config.logging) {
                        apply_logging(&state, &config);
                    }
                }
            }
            logging = config.logging;
        }
    })
}
