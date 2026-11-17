//! Event hooks and plugins for one-shot commands: they start with the command and get
//! time to finish what its events queued before it exits. Without the `hooks` and
//! `plugins` features (the bare lite build) this does nothing.

use putio_sync_core::config::Config;
use putio_sync_download::DownloadManager;
use std::path::Path;
#[cfg(any(feature = "hooks", feature = "plugins"))]
use std::time::Duration;

/// How long a command waits for its hooks and plugins before it exits anyway
#[cfg(any(feature = "hooks", feature = "plugins"))]
const FINISH_TIMEOUT: Duration = Duration::from_secs(120);

/// The hooks and plugins one command runs
pub struct Services {
    #[cfg(feature = "hooks")]
    hooks: putio_sync_hooks::HooksService,
    #[cfg(feature = "plugins")]
    plugins: std::sync::Arc<putio_sync_plugins::PluginsService>,
}

impl Services {
    /// Start the configured hooks and plugins. Needs a Tokio runtime.
    #[cfg_attr(not(feature = "plugins"), allow(unused_variables))]
    pub fn start(config: &Config, config_path: &Path) -> Self {
        #[cfg(feature = "hooks")]
        let hooks = {
            let hooks = putio_sync_hooks::HooksService::new(config.hooks.clone());
            hooks.start();
            hooks
        };
        #[cfg(feature = "plugins")]
        let plugins = {
            let plugins = std::sync::Arc::new(putio_sync_plugins::PluginsService::from_config(
                config,
                Some(config_path),
            ));
            plugins.start();
            plugins
        };
        Self {
            #[cfg(feature = "hooks")]
            hooks,
            #[cfg(feature = "plugins")]
            plugins,
        }
    }

    /// Let the plugins' filters, rename rules and folder rules join `manager`'s downloads
    #[cfg_attr(not(feature = "plugins"), allow(unused_variables))]
    pub fn attach(&self, manager: &DownloadManager) {
        #[cfg(feature = "plugins")]
        manager.set_sync_hooks(self.plugins.clone());
    }

    /// Wait for the hooks and plugins to run every event emitted so far
    pub async fn finish(self) {
        #[cfg(feature = "hooks")]
        if !self.hooks.drain(FINISH_TIMEOUT).await {
            tracing::warn!(
                "Event hooks still running after {}s; exiting anyway",
                FINISH_TIMEOUT.as_secs()
            );
        }
        #[cfg(feature = "plugins")]
        if !self.plugins.drain(FINISH_TIMEOUT).await {
            tracing::warn!(
                "Plugins still running after {}s; exiting anyway",
                FINISH_TIMEOUT.as_secs()
            );
        }
    }
}
