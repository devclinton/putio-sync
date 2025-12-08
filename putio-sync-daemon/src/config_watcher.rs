//! Configuration file watcher for hot-reloading

use crate::error::{DaemonError, DaemonResult};
use notify::{Event, EventKind, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use tracing::{debug, error, info};

/// Status of the config watcher
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatcherStatus {
    /// Watcher is not started
    Stopped,
    /// Watcher is running
    Running,
    /// Watcher encountered an error
    Error,
}

/// Configuration file watcher
///
/// Watches a configuration file for changes and triggers a callback
/// when the file is modified.
pub struct ConfigWatcher {
    /// Path to the config file being watched
    filepath: Option<PathBuf>,
    /// Event callback for config changes
    event_callback: Arc<dyn Fn() + Send + Sync>,
    /// Current watcher status
    status: WatcherStatus,
}

impl ConfigWatcher {
    /// Create a new config watcher with default config path
    ///
    /// The callback will be invoked whenever the config file changes
    pub fn new<F>(callback: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        Self {
            filepath: None,
            event_callback: Arc::new(callback),
            status: WatcherStatus::Stopped,
        }
    }

    /// Create a new config watcher with a specific file path
    ///
    /// The callback will be invoked whenever the config file changes
    pub fn new_with_path<F>(path: PathBuf, callback: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        Self {
            filepath: Some(path),
            event_callback: Arc::new(callback),
            status: WatcherStatus::Stopped,
        }
    }

    /// Get the current status of the watcher
    pub fn get_status(&self) -> WatcherStatus {
        self.status
    }

    /// Start watching the config file
    ///
    /// This will block and watch for file changes until an error occurs
    /// or the watcher is stopped.
    pub async fn start(&mut self) -> DaemonResult<()> {
        let config_path = match &self.filepath {
            Some(path) => path.clone(),
            None => {
                return Err(DaemonError::ConfigError(
                    "No config file path specified".to_string(),
                ))
            }
        };

        if !config_path.exists() {
            return Err(DaemonError::ConfigError(format!(
                "Config file does not exist: {}",
                config_path.display()
            )));
        }

        info!(
            "Starting config file watcher for: {}",
            config_path.display()
        );

        let (tx, rx) = channel();
        let mut watcher = self.create_watcher(tx)?;

        // Watch the parent directory of the config file
        // (watching the file directly doesn't work reliably on all platforms)
        let watch_dir = config_path
            .parent()
            .ok_or_else(|| DaemonError::ConfigError("Invalid config path".to_string()))?
            .to_path_buf();

        watcher
            .watch(&watch_dir, RecursiveMode::NonRecursive)
            .map_err(DaemonError::WatchError)?;

        self.status = WatcherStatus::Running;
        debug!("Config watcher started, watching: {}", watch_dir.display());

        // Process events
        self.process_events(&rx, &config_path)?;

        Ok(())
    }

    /// Create a filesystem watcher
    fn create_watcher(
        &self,
        tx: Sender<notify::Result<Event>>,
    ) -> DaemonResult<notify::RecommendedWatcher> {
        let watcher = notify::recommended_watcher(tx).map_err(DaemonError::WatchError)?;
        Ok(watcher)
    }

    /// Process filesystem events
    fn process_events(
        &mut self,
        rx: &Receiver<notify::Result<Event>>,
        config_path: &Path,
    ) -> DaemonResult<()> {
        loop {
            match rx.recv() {
                Ok(Ok(event)) => {
                    if let Err(e) = self.handle_event(event, config_path) {
                        error!("Error handling config file event: {}", e);
                        self.status = WatcherStatus::Error;
                    }
                }
                Ok(Err(e)) => {
                    error!("Filesystem watch error: {}", e);
                    self.status = WatcherStatus::Error;
                    return Err(DaemonError::WatchError(e));
                }
                Err(e) => {
                    error!("Channel receive error: {}", e);
                    self.status = WatcherStatus::Error;
                    return Err(DaemonError::InternalError(format!("Channel error: {}", e)));
                }
            }
        }
    }

    /// Handle a filesystem event
    fn handle_event(&self, event: Event, config_path: &Path) -> DaemonResult<()> {
        debug!("Config watcher event: {:?}", event.kind);

        // Only process Modify events for the specific config file
        match event.kind {
            EventKind::Modify(_) => {
                for path in event.paths {
                    if path == config_path {
                        info!("Config file changed: {}", path.display());
                        info!("Triggering config reload...");
                        (self.event_callback)();
                    }
                }
            }
            _ => {
                // Ignore other events
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn test_config_watcher_creation() {
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        let watcher = ConfigWatcher::new(move || {
            called_clone.store(true, Ordering::SeqCst);
        });

        assert_eq!(watcher.get_status(), WatcherStatus::Stopped);
        assert!(watcher.filepath.is_none());
    }

    #[test]
    fn test_config_watcher_with_path() {
        let path = PathBuf::from("/tmp/test-config.toml");
        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        let watcher = ConfigWatcher::new_with_path(path.clone(), move || {
            called_clone.store(true, Ordering::SeqCst);
        });

        assert_eq!(watcher.get_status(), WatcherStatus::Stopped);
        assert_eq!(watcher.filepath, Some(path));
    }
}
