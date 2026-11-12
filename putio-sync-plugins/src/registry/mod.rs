//! The plugin registry (059): a list of reviewed plugins from `[plugin_registry] url`,
//! cached for 6 hours in `plugin-data/registry-index.json`, and installs from it. A
//! release is downloaded and checked in memory ([`archive`]), then written through the
//! 029 installer with an install record (`.registry.json`) beside the manifest. Nothing is
//! fetched while the registry is off, and `GET /plugins` never fetches.

pub mod archive;
pub mod index;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, Utc};
use putio_sync_core::plugins::{valid_link, PluginRegistryConfig};
use serde::{Deserialize, Serialize};

use index::{Entry, Hosts, Index, Offer, MAX_INDEX_BYTES};

/// How long a fetched list is used before fetching again
pub const CACHE_FOR: Duration = Duration::from_secs(6 * 3600);

/// The install record's file in a plugin's folder
pub const RECORD_FILE: &str = ".registry.json";

const CACHE_FILE: &str = "registry-index.json";
const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_REDIRECTS: usize = 5;

/// What putio-sync this is, for `min_putio_sync`
pub const RUNNING: &str = env!("CARGO_PKG_VERSION");

/// The fetched list as kept on disk
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Cache {
    url: String,
    fetched_at: DateTime<Utc>,
    index: Index,
    #[serde(default)]
    skipped: usize,
}

/// Where a registry plugin was installed from (`.registry.json`)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstallRecord {
    /// The index it came from
    pub registry: String,
    pub version: String,
    /// Of the release file
    pub sha256: String,
    pub installed_at: DateTime<Utc>,
}

impl InstallRecord {
    /// The record in the plugin folder `folder`, if any
    pub fn read(folder: &Path) -> Option<InstallRecord> {
        let text = std::fs::read(folder.join(RECORD_FILE)).ok()?;
        serde_json::from_slice(&text).ok()
    }
}

/// One entry as More plugins shows it
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Listed {
    pub name: String,
    pub description: String,
    pub author: String,
    pub license: String,
    pub repository: String,
    pub docs: Option<String>,
    pub homepage: Option<String>,
    pub issues: Option<String>,
    pub runtime: String,
    pub events: Vec<String>,
    pub hooks: Vec<String>,
    pub hosts: Hosts,
    /// The release offered, when there is one
    pub version: Option<String>,
    pub notes: Option<String>,
    /// `available`, `installed`, `update`, `needs_newer` or `name_in_use`
    pub state: &'static str,
    /// The putio-sync version the oldest release needs, for `needs_newer`
    pub needs: Option<String>,
    pub withdrawn: Option<String>,
}

/// The list for the UI
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Listing {
    pub enabled: bool,
    pub url: String,
    pub fetched_at: Option<DateTime<Utc>>,
    /// Why the last fetch failed; the list is then the cached one
    pub error: Option<String>,
    /// Entries dropped as invalid
    pub skipped: usize,
    pub plugins: Vec<Listed>,
}

/// Why a registry install could not happen
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    /// No such entry, or the registry is off
    NotFound(String),
    /// The entry's state does not allow it (name in use, needs a newer putio-sync, ...)
    Conflict(String),
    /// The download or a check failed; nothing was written
    Refused(String),
    /// Writing the plugins folder failed
    Io(String),
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RegistryError::NotFound(e)
            | RegistryError::Conflict(e)
            | RegistryError::Refused(e)
            | RegistryError::Io(e) => write!(f, "{}", e),
        }
    }
}

/// The registry's cache and install locks
pub struct Registry {
    data_dir: Option<PathBuf>,
    cache: Mutex<Option<Cache>>,
    fetching: tokio::sync::Mutex<()>,
    installing: Mutex<HashSet<String>>,
}

/// Holds an install's name until dropped
pub(crate) struct Installing<'a> {
    registry: &'a Registry,
    name: String,
}

impl Drop for Installing<'_> {
    fn drop(&mut self) {
        self.registry.installing.lock().unwrap().remove(&self.name);
    }
}

impl Registry {
    /// A registry keeping its cache in `data_dir`, loading what an earlier run kept
    pub fn new(data_dir: Option<PathBuf>) -> Registry {
        let cache = data_dir
            .as_ref()
            .and_then(|d| std::fs::read(d.join(CACHE_FILE)).ok())
            .and_then(|bytes| serde_json::from_slice(&bytes).ok());
        Registry {
            data_dir,
            cache: Mutex::new(cache),
            fetching: tokio::sync::Mutex::new(()),
            installing: Mutex::new(HashSet::new()),
        }
    }

    /// The list for `config`, fetched when the cache is older than 6 hours, is for another
    /// address, or `refresh` is set. A failed fetch keeps the cached list and says why.
    pub async fn listing(
        &self,
        config: &PluginRegistryConfig,
        refresh: bool,
        plugins_dir: Option<&Path>,
    ) -> Listing {
        let mut listing = Listing {
            enabled: config.enabled,
            url: config.url.clone(),
            fetched_at: None,
            error: None,
            skipped: 0,
            plugins: Vec::new(),
        };
        if !config.enabled {
            return listing;
        }
        let (cache, error) = self.index(config, refresh).await;
        listing.error = error;
        if let Some(cache) = cache {
            listing.fetched_at = Some(cache.fetched_at);
            listing.skipped = cache.skipped;
            listing.plugins = cache
                .index
                .plugins
                .iter()
                .map(|e| listed(e, plugins_dir))
                .filter(|l| l.withdrawn.is_none() || matches!(l.state, "installed" | "update"))
                .collect();
        }
        listing
    }

    /// The cached index for `config.url`, without fetching
    pub fn cached(&self, config: &PluginRegistryConfig) -> Option<Index> {
        let cache = self.cache.lock().unwrap();
        cache
            .as_ref()
            .filter(|c| config.enabled && c.url == config.url)
            .map(|c| c.index.clone())
    }

    /// The index, fresh enough or fetched now, and the fetch's error
    async fn index(
        &self,
        config: &PluginRegistryConfig,
        refresh: bool,
    ) -> (Option<Cache>, Option<String>) {
        let _one = self.fetching.lock().await;
        let current = self
            .cache
            .lock()
            .unwrap()
            .clone()
            .filter(|c| c.url == config.url);
        let fresh = current.as_ref().is_some_and(|c| {
            (Utc::now() - c.fetched_at)
                .to_std()
                .is_ok_and(|age| age < CACHE_FOR)
        });
        if fresh && !refresh {
            return (current, None);
        }
        let fetched = get_limited(&config.url, MAX_INDEX_BYTES)
            .await
            .and_then(|bytes| Index::parse(&bytes));
        match fetched {
            Ok((index, skipped)) => {
                let cache = Cache {
                    url: config.url.clone(),
                    fetched_at: Utc::now(),
                    index,
                    skipped,
                };
                if let Some(dir) = &self.data_dir {
                    let saved = std::fs::create_dir_all(dir).and_then(|_| {
                        std::fs::write(
                            dir.join(CACHE_FILE),
                            serde_json::to_vec(&cache).unwrap_or_default(),
                        )
                    });
                    if let Err(e) = saved {
                        tracing::warn!("plugin registry: cannot keep the list: {}", e);
                    }
                }
                *self.cache.lock().unwrap() = Some(cache.clone());
                (Some(cache), None)
            }
            Err(e) => {
                tracing::warn!("plugin registry: {}", e);
                (current, Some(e))
            }
        }
    }

    /// Download and check the release of `name` the cached list offers. Takes the name's
    /// install lock, held until the result is dropped.
    pub(crate) async fn release(
        &self,
        config: &PluginRegistryConfig,
        name: &str,
        plugins_dir: Option<&Path>,
    ) -> Result<(Entry, archive::Checked, Installing<'_>), RegistryError> {
        let index = self
            .cached(config)
            .ok_or_else(|| RegistryError::NotFound("open More plugins first".to_string()))?;
        let entry = index
            .get(name)
            .cloned()
            .ok_or_else(|| RegistryError::NotFound(format!("{} is not in the registry", name)))?;
        let shown = listed(&entry, plugins_dir);
        match shown.state {
            "name_in_use" => {
                return Err(RegistryError::Conflict(format!(
                    "a plugin named {} that is not from the registry is installed",
                    name
                )))
            }
            "needs_newer" => {
                return Err(RegistryError::Conflict(format!(
                    "{} needs putio-sync {} or newer",
                    name,
                    shown.needs.unwrap_or_default()
                )))
            }
            _ => {}
        }
        if let Some(reason) = &entry.withdrawn {
            return Err(RegistryError::Conflict(format!(
                "{} was withdrawn: {}",
                name, reason
            )));
        }
        if !self.installing.lock().unwrap().insert(name.to_string()) {
            return Err(RegistryError::Conflict(format!(
                "{} is being installed",
                name
            )));
        }
        let guard = Installing {
            registry: self,
            name: name.to_string(),
        };
        let Offer::Release(release) = entry.offered(RUNNING) else {
            unreachable!("needs_newer is refused above")
        };
        let bytes = get_limited(&release.url, archive::MAX_ARCHIVE_BYTES)
            .await
            .map_err(RegistryError::Refused)?;
        let checked = archive::check(&bytes, &entry, release).map_err(RegistryError::Refused)?;
        Ok((entry, checked, guard))
    }
}

/// An entry with its state against the plugins folder
fn listed(entry: &Entry, plugins_dir: Option<&Path>) -> Listed {
    let offer = entry.offered(RUNNING);
    let release = match &offer {
        Offer::Release(r) => Some(*r),
        Offer::NeedsNewer(_) => None,
    };
    let folder = plugins_dir.map(|d| d.join(&entry.name));
    let record = folder.as_deref().and_then(InstallRecord::read);
    let state = match (&record, folder.as_ref().is_some_and(|f| f.exists())) {
        (Some(r), _) => match release {
            Some(rel) if index::version(&rel.version) > index::version(&r.version) => "update",
            _ => "installed",
        },
        (None, true) => "name_in_use",
        (None, false) if release.is_none() => "needs_newer",
        (None, false) => "available",
    };
    Listed {
        name: entry.name.clone(),
        description: entry.description.clone(),
        author: entry.author.clone(),
        license: entry.license.clone(),
        repository: entry.repository.clone(),
        docs: entry.docs.clone(),
        homepage: entry.homepage.clone(),
        issues: entry.issues.clone(),
        runtime: entry.runtime.clone(),
        events: entry.events.clone(),
        hooks: entry.hooks.clone(),
        hosts: entry.hosts.clone(),
        version: release.map(|r| r.version.clone()),
        notes: release.and_then(|r| r.notes.clone()),
        state,
        needs: match offer {
            Offer::NeedsNewer(v) => Some(v),
            Offer::Release(_) => None,
        },
        withdrawn: entry.withdrawn.clone(),
    }
}

/// The bytes at `url`, at most `max`: https, or http to a local host, as is every redirect
async fn get_limited(url: &str, max: usize) -> Result<Vec<u8>, String> {
    valid_link(url).map_err(|e| format!("{}: {}", url, e))?;
    let client = reqwest::Client::builder()
        .timeout(TIMEOUT)
        .user_agent(format!("putio-sync/{}", RUNNING))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= MAX_REDIRECTS {
                attempt.error("too many redirects")
            } else if valid_link(attempt.url().as_str()).is_err() {
                attempt.error("redirected to an address that is not https")
            } else {
                attempt.follow()
            }
        }))
        .build()
        .map_err(|e| e.to_string())?;
    let failed = |e: reqwest::Error| {
        let e = e.without_url();
        let cause = std::error::Error::source(&e)
            .map(|s| format!("{}: {}", e, s))
            .unwrap_or_else(|| e.to_string());
        format!("cannot download {}: {}", url, cause)
    };
    let response = client.get(url).send().await.map_err(failed)?;
    if !response.status().is_success() {
        return Err(format!(
            "cannot download {}: HTTP {}",
            url,
            response.status().as_u16()
        ));
    }
    let mut response = response;
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(failed)? {
        if bytes.len() + chunk.len() > max {
            return Err(format!(
                "cannot download {}: larger than {} MB",
                url,
                max / (1024 * 1024)
            ));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::archive::tests::{entry_for, good};
    use super::*;
    use crate::{PluginsOptions, PluginsService};

    /// A registry server listing `hook` 1.0.0 with `archive`'s SHA-256
    async fn served(archive: Vec<u8>) -> (mockito::ServerGuard, PluginRegistryConfig) {
        served_as(&archive.clone(), archive).await
    }

    /// A registry server listing `listed`'s SHA-256 and serving `archive`
    async fn served_as(
        listed: &[u8],
        archive: Vec<u8>,
    ) -> (mockito::ServerGuard, PluginRegistryConfig) {
        let mut server = mockito::Server::new_async().await;
        let (mut entry, mut release) = entry_for(listed);
        release.url = format!("{}/hook-1.0.0.tar.gz", server.url());
        entry.releases = vec![release];
        let index = serde_json::json!({ "schema_version": 1, "plugins": [entry] });
        server
            .mock("GET", "/index.json")
            .with_body(index.to_string())
            .create_async()
            .await;
        server
            .mock("GET", "/hook-1.0.0.tar.gz")
            .with_body(archive)
            .create_async()
            .await;
        let config = PluginRegistryConfig {
            enabled: true,
            url: format!("{}/index.json", server.url()),
        };
        (server, config)
    }

    fn service(root: &Path) -> PluginsService {
        PluginsService::new(PluginsOptions {
            dir: Some(root.join("plugins")),
            data_dir: Some(root.join("plugin-data")),
            ..Default::default()
        })
    }

    #[tokio::test]
    async fn a_listed_plugin_installs_disabled_with_its_record() {
        let root = tempfile::tempdir().unwrap();
        let (_server, config) = served(good()).await;
        let plugins = service(root.path());

        let listing = plugins.registry(&config, false).await;
        assert_eq!(listing.error, None);
        assert_eq!(listing.plugins.len(), 1);
        assert_eq!(listing.plugins[0].state, "available");
        assert!(root.path().join("plugin-data").join(CACHE_FILE).exists());

        assert_eq!(
            plugins.install_from_registry(&config, "hook").await,
            Ok("hook".to_string())
        );
        let status = plugins.status();
        let hook = status.iter().find(|p| p.name == "hook").unwrap();
        assert_eq!(hook.state, crate::PluginState::Disabled);
        let record = hook.registry.as_ref().expect("install record");
        assert_eq!(record.version, "1.0.0");
        assert_eq!(record.registry, config.url);
        assert_eq!(
            plugins.registry(&config, false).await.plugins[0].state,
            "installed"
        );
    }

    #[tokio::test]
    async fn a_changed_archive_writes_nothing() {
        let root = tempfile::tempdir().unwrap();
        let (_server, config) = served_as(&good(), b"not the archive".to_vec()).await;
        let plugins = service(root.path());
        plugins.registry(&config, false).await;
        let refused = plugins.install_from_registry(&config, "hook").await;
        assert!(
            matches!(&refused, Err(RegistryError::Refused(e)) if e.contains("SHA-256")),
            "{:?}",
            refused
        );
        assert!(!root.path().join("plugins/hook").exists());
    }

    #[tokio::test]
    async fn a_hand_installed_plugin_of_the_same_name_is_left_alone() {
        let root = tempfile::tempdir().unwrap();
        let (_server, config) = served(good()).await;
        std::fs::create_dir_all(root.path().join("plugins/hook")).unwrap();
        let plugins = service(root.path());
        assert_eq!(
            plugins.registry(&config, false).await.plugins[0].state,
            "name_in_use"
        );
        assert!(matches!(
            plugins.install_from_registry(&config, "hook").await,
            Err(RegistryError::Conflict(_))
        ));
    }

    #[tokio::test]
    async fn an_off_registry_fetches_nothing_and_a_failed_fetch_keeps_the_list() {
        let root = tempfile::tempdir().unwrap();
        let (server, mut config) = served(good()).await;
        let plugins = service(root.path());
        config.enabled = false;
        let off = plugins.registry(&config, true).await;
        assert!(!off.enabled && off.plugins.is_empty());

        config.enabled = true;
        plugins.registry(&config, false).await;
        drop(server);
        let kept = plugins.registry(&config, true).await;
        assert!(kept.error.is_some());
        assert_eq!(kept.plugins.len(), 1);
    }

    #[tokio::test]
    async fn only_https_or_local_addresses_are_fetched() {
        let err = get_limited("http://example.com/index.json", 10)
            .await
            .unwrap_err();
        assert!(err.contains("https"), "{}", err);
    }
}
