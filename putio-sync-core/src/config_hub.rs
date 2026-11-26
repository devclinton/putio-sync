//! The running config, and the watcher that follows its file.
//!
//! One [`ConfigHub`] holds the config the process runs on. Whoever changes it
//! ([`Source::Api`] after a settings save, [`Source::File`] after a hand edit) publishes
//! the new value; services subscribe and apply what changed. [`spawn_file_watcher`]
//! publishes hand edits and refuses invalid ones, keeping the running config.

use crate::config::Config;
use crate::error::{CoreError, CoreResult};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tokio::sync::watch;

/// Where a published config came from
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The file as it was at startup
    Startup,
    /// A save through the API; its routes have already applied it to the services they own
    Api,
    /// A hand edit of the config file
    File,
}

/// One published config
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// The config
    pub config: Arc<Config>,
    /// 0 at startup, then one more for each publish
    pub generation: u64,
    /// Who published it
    pub source: Source,
}

/// How the last reload went
#[derive(Debug, Clone, Default)]
pub struct HubStatus {
    /// Why the last hand edit was refused; cleared by the next good publish or when the
    /// file matches the running config again
    pub last_error: Option<String>,
    /// When the running config was published
    pub applied_at: Option<SystemTime>,
}

/// The running config and its subscribers
#[derive(Debug)]
pub struct ConfigHub {
    tx: watch::Sender<Snapshot>,
    startup: Arc<Config>,
    status: Mutex<HubStatus>,
}

impl ConfigHub {
    /// A hub whose first snapshot (generation 0) is `config`, from [`Source::Startup`]
    pub fn new(config: Config) -> Self {
        let startup = Arc::new(config);
        let (tx, _) = watch::channel(Snapshot {
            config: startup.clone(),
            generation: 0,
            source: Source::Startup,
        });
        Self {
            tx,
            startup,
            status: Mutex::new(HubStatus {
                last_error: None,
                applied_at: Some(SystemTime::now()),
            }),
        }
    }

    /// The running config
    pub fn current(&self) -> Arc<Config> {
        self.tx.borrow().config.clone()
    }

    /// The config the process started with
    pub fn startup(&self) -> Arc<Config> {
        self.startup.clone()
    }

    /// Saved settings that differ from startup and apply only after a restart, as
    /// `section.key` ([`crate::restart::pending`])
    pub fn pending_restart(&self) -> Vec<String> {
        crate::restart::pending(&self.startup, &self.current())
    }

    /// The running snapshot
    pub fn snapshot(&self) -> Snapshot {
        self.tx.borrow().clone()
    }

    /// A receiver that sees every later publish (only the latest, if several come at
    /// once). Its first `changed()` waits for the next publish.
    pub fn subscribe(&self) -> watch::Receiver<Snapshot> {
        self.tx.subscribe()
    }

    /// Make `config` the running config. Returns its generation. Clears the last error.
    pub fn publish(&self, config: Config, source: Source) -> u64 {
        let mut generation = 0;
        self.tx.send_modify(|snapshot| {
            snapshot.generation += 1;
            snapshot.config = Arc::new(config);
            snapshot.source = source;
            generation = snapshot.generation;
        });
        let mut status = self.lock();
        status.last_error = None;
        status.applied_at = Some(SystemTime::now());
        generation
    }

    /// Record why a hand edit was refused
    pub fn set_error(&self, error: impl Into<String>) {
        self.lock().last_error = Some(error.into());
    }

    /// Forget the last error
    pub fn clear_error(&self) {
        self.lock().last_error = None;
    }

    /// How the last reload went
    pub fn status(&self) -> HubStatus {
        self.lock().clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HubStatus> {
        self.status.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// `config` as a TOML table, for telling whether two configs differ
fn table(config: &Config) -> Option<toml::Table> {
    toml::Table::try_from(config).ok()
}

/// True when `a` and `b` serialise alike. A config that doesn't serialise is never equal.
pub fn same_config(a: &Config, b: &Config) -> bool {
    match (table(a), table(b)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

type Stamp = (SystemTime, u64);

async fn stamp(path: &Path) -> std::io::Result<Stamp> {
    let meta = tokio::fs::metadata(path).await?;
    Ok((meta.modified()?, meta.len()))
}

fn hash_of(contents: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    contents.hash(&mut hasher);
    hasher.finish()
}

/// Follow the config file at `path` and publish hand edits to `hub` as [`Source::File`].
///
/// Every `interval` it looks at the file's modified time and length; only when they change
/// does it read the file. A change is taken once two reads in a row agree, so half-written
/// saves are not applied. The text is parsed ([`Config::from_toml`]) and passed to
/// `prepare`, which does what startup does to a config (validation, environment
/// overrides). A result equal to the running config is dropped without a word: that is how
/// the API's own saves pass through. A file that is missing, unparsable or refused by
/// `prepare` leaves the running config in place, is recorded with [`ConfigHub::set_error`],
/// and is logged once per distinct content.
pub fn spawn_file_watcher(
    path: PathBuf,
    interval: Duration,
    hub: Arc<ConfigHub>,
    prepare: impl Fn(Config) -> CoreResult<Config> + Send + Sync + 'static,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        // The state at startup is the one already running.
        ticker.tick().await;
        let mut last: Option<Stamp> = stamp(&path).await.ok();
        // The hash of the read that waits for a second, agreeing read
        let mut pending: Option<u64> = None;
        // The hash of the content last dealt with (applied, equal or refused)
        let mut dealt_with: Option<u64> = None;
        let mut missing = false;

        loop {
            ticker.tick().await;
            let current = match stamp(&path).await {
                Ok(current) => current,
                Err(e) => {
                    if !missing {
                        tracing::error!(path = %path.display(), error = %e, "Config file unreadable; keeping the running settings");
                        hub.set_error(format!("The config file can't be read: {e}"));
                        missing = true;
                    }
                    last = None;
                    pending = None;
                    dealt_with = None;
                    continue;
                }
            };
            missing = false;
            if Some(current) == last {
                continue;
            }
            let contents = match tokio::fs::read_to_string(&path).await {
                Ok(contents) => contents,
                // Between the stat and the read: look again next time
                Err(_) => continue,
            };
            let hash = hash_of(&contents);
            if pending != Some(hash) {
                // Wait for a second read that agrees
                pending = Some(hash);
                continue;
            }
            pending = None;
            last = Some(current);
            if dealt_with == Some(hash) {
                continue;
            }
            dealt_with = Some(hash);

            let candidate = Config::from_toml(&contents).and_then(&prepare);
            match candidate {
                Ok(config) => {
                    if same_config(&config, &hub.current()) {
                        hub.clear_error();
                        continue;
                    }
                    let generation = hub.publish(config, Source::File);
                    tracing::info!(generation, "Config file changed; applying the new settings");
                }
                Err(e) => {
                    tracing::error!(path = %path.display(), error = %e, "Config file change refused; keeping the running settings");
                    hub.set_error(match e {
                        CoreError::ConfigError(message) => message,
                        other => other.to_string(),
                    });
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_counts_generations_and_wakes_subscribers() {
        let hub = ConfigHub::new(Config::default());
        let mut rx = hub.subscribe();
        assert_eq!(hub.snapshot().generation, 0);
        assert_eq!(hub.snapshot().source, Source::Startup);
        assert!(!rx.has_changed().unwrap());

        let mut config = Config::default();
        config.watch.refresh_cycle = 7;
        assert_eq!(hub.publish(config, Source::Api), 1);
        assert!(rx.has_changed().unwrap());
        let seen = rx.borrow_and_update().clone();
        assert_eq!(seen.generation, 1);
        assert_eq!(seen.source, Source::Api);
        assert_eq!(seen.config.watch.refresh_cycle, 7);
        assert_eq!(hub.current().watch.refresh_cycle, 7);

        // Two publishes before a look: only the latest is seen
        hub.publish(Config::default(), Source::File);
        hub.publish(Config::default(), Source::File);
        assert_eq!(rx.borrow_and_update().generation, 3);
    }

    #[test]
    fn a_publish_clears_the_error() {
        let hub = ConfigHub::new(Config::default());
        hub.set_error("bad");
        assert_eq!(hub.status().last_error.as_deref(), Some("bad"));
        hub.publish(Config::default(), Source::Api);
        assert!(hub.status().last_error.is_none());
        assert!(hub.status().applied_at.is_some());
    }

    #[test]
    fn pending_restart_compares_with_startup() {
        let hub = ConfigHub::new(Config::default());
        assert!(hub.pending_restart().is_empty());
        let mut config = Config::default();
        config.logging.console_enabled = !config.logging.console_enabled;
        hub.publish(config, Source::File);
        assert_eq!(hub.pending_restart(), vec!["logging.console_enabled"]);
        // Put back as it was: nothing waits for a restart
        hub.publish(Config::default(), Source::Api);
        assert!(hub.pending_restart().is_empty());
    }

    #[test]
    fn same_config_sees_a_changed_value() {
        let a = Config::default();
        let mut b = Config::default();
        assert!(same_config(&a, &b));
        b.watch.refresh_cycle += 1;
        assert!(!same_config(&a, &b));
    }
}
