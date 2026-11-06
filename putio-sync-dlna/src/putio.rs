//! The Put.io root: one Put.io folder listed and played through putio-sync (feature 022).
//!
//! Playback is proxied, so Put.io links and tokens never reach a player. The MP4 version
//! is played when Put.io has one, the original otherwise.

use crate::content::{self, BrowseFlag, BrowseOut, Fault, ObjectId};
use crate::media::{self, MediaType};
use crate::Shared;
use futures_util::StreamExt;
use putio_sync_client::PutioApiClient;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a folder listing is reused
pub const LISTING_TTL: Duration = Duration::from_secs(60);
/// Folder listings kept
const MAX_LISTINGS: usize = 200;
/// Entries remembered for parent-chain checks
const MAX_KNOWN: usize = 20_000;
/// Longest parent chain followed
/// The longest a player waits on one Put.io call (the client's own retries can take a minute)
const CALL_LIMIT: Duration = Duration::from_secs(10);

/// A folder's entries and when they were fetched
type Listing = (Instant, Arc<Vec<PutioEntry>>);

const MAX_DEPTH: usize = 64;

/// Why a Put.io lookup failed
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PutioError {
    /// The file isn't inside the shared folder
    #[error("{0}")]
    Outside(String),
    /// Put.io didn't answer as expected
    #[error("{0}")]
    Upstream(String),
}

/// A file or folder on Put.io
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PutioEntry {
    pub id: i64,
    pub name: String,
    pub is_folder: bool,
    pub size: i64,
    pub parent: i64,
}

/// Which version of a file is played
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version {
    pub mp4: bool,
    pub size: i64,
}

/// A started stream: headers to pass on, and the body to copy
pub struct Opened {
    pub status: u16,
    pub content_length: Option<String>,
    pub content_range: Option<String>,
    pub mime: String,
    pub mp4: bool,
    pub body: Option<reqwest::Response>,
}

/// The shared Put.io folder
pub struct PutioRoot {
    client: Arc<PutioApiClient>,
    folder: i64,
    ttl: Duration,
    listings: Mutex<HashMap<i64, Listing>>,
    known: Mutex<HashMap<i64, PutioEntry>>,
    versions: Mutex<HashMap<i64, (Instant, Version)>>,
    last_error: Mutex<Option<String>>,
}

fn entry_of(e: &putio_sync_client::models::FileEntry, fallback_parent: i64) -> PutioEntry {
    PutioEntry {
        id: e.id,
        name: e.name.clone(),
        is_folder: e.is_folder,
        size: e.size,
        parent: e.parent_id.unwrap_or(fallback_parent),
    }
}

/// Original files that already play as they are
fn is_mp4_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".mp4") || lower.ends_with(".m4v")
}

impl PutioRoot {
    pub fn new(client: Arc<PutioApiClient>, folder: i64) -> Self {
        Self::with_ttl(client, folder, LISTING_TTL)
    }

    /// The same with another listing lifetime (tests)
    pub fn with_ttl(client: Arc<PutioApiClient>, folder: i64, ttl: Duration) -> Self {
        PutioRoot {
            client,
            folder,
            ttl,
            listings: Mutex::new(HashMap::new()),
            known: Mutex::new(HashMap::new()),
            versions: Mutex::new(HashMap::new()),
            last_error: Mutex::new(None),
        }
    }

    pub fn folder(&self) -> i64 {
        self.folder
    }

    /// The last Put.io failure, until the next success
    pub fn last_error(&self) -> Option<String> {
        self.last_error.lock().unwrap().clone()
    }

    fn fail(&self, what: &str, err: &dyn std::fmt::Display) -> PutioError {
        let reason = format!("{what}: {err}");
        *self.last_error.lock().unwrap() = Some(reason.clone());
        PutioError::Upstream(reason)
    }

    fn ok(&self) {
        *self.last_error.lock().unwrap() = None;
    }

    fn remember(&self, entries: &[PutioEntry]) {
        let mut known = self.known.lock().unwrap();
        if known.len() + entries.len() > MAX_KNOWN {
            known.clear();
        }
        for e in entries {
            known.insert(e.id, e.clone());
        }
    }

    /// The entries of a folder, from the cache while it's fresh. Folders first, then by
    /// name, case-insensitive.
    pub async fn list(&self, folder: i64) -> Result<Arc<Vec<PutioEntry>>, PutioError> {
        if let Some((at, list)) = self.listings.lock().unwrap().get(&folder) {
            if at.elapsed() < self.ttl {
                return Ok(list.clone());
            }
        }
        let response = tokio::time::timeout(CALL_LIMIT, self.client.list_files(folder))
            .await
            .map_err(|_| self.fail("listing a folder", &"timed out"))?
            .map_err(|e| self.fail("listing a folder", &e))?;
        self.ok();
        let mut entries: Vec<PutioEntry> =
            response.files.iter().map(|f| entry_of(f, folder)).collect();
        entries.sort_by(|a, b| {
            b.is_folder
                .cmp(&a.is_folder)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        self.remember(&entries);
        let list = Arc::new(entries);
        let mut listings = self.listings.lock().unwrap();
        if listings.len() >= MAX_LISTINGS {
            if let Some(oldest) = listings
                .iter()
                .min_by_key(|(_, (at, _))| *at)
                .map(|(k, _)| *k)
            {
                listings.remove(&oldest);
            }
        }
        listings.insert(folder, (Instant::now(), list.clone()));
        Ok(list)
    }

    /// One entry, looked up when not already seen
    pub async fn entry(&self, id: i64) -> Result<PutioEntry, PutioError> {
        if let Some(e) = self.known.lock().unwrap().get(&id) {
            return Ok(e.clone());
        }
        let response = tokio::time::timeout(CALL_LIMIT, self.client.get_file_details(id))
            .await
            .map_err(|_| self.fail("reading a file", &"timed out"))?
            .map_err(|e| self.fail("reading a file", &e))?;
        self.ok();
        let entry = entry_of(&response.file, 0);
        self.remember(std::slice::from_ref(&entry));
        Ok(entry)
    }

    /// Whether `id` is the shared folder or below it
    pub async fn is_inside(&self, id: i64) -> Result<bool, PutioError> {
        let mut current = id;
        for _ in 0..MAX_DEPTH {
            if current == self.folder {
                return Ok(true);
            }
            if current == 0 {
                return Ok(false);
            }
            current = self.entry(current).await?.parent;
        }
        Ok(false)
    }

    /// The version of a file that is played, and its size
    pub async fn version(&self, entry: &PutioEntry) -> Version {
        let original = Version {
            mp4: false,
            size: entry.size,
        };
        if is_mp4_name(&entry.name) {
            return original;
        }
        if let Some((at, v)) = self.versions.lock().unwrap().get(&entry.id) {
            if at.elapsed() < self.ttl {
                return *v;
            }
        }
        let status = tokio::time::timeout(CALL_LIMIT, self.client.mp4_status(entry.id)).await;
        let version = match status {
            Ok(Ok(s)) if s.is_ready() => Version {
                mp4: true,
                size: s.size.unwrap_or(entry.size),
            },
            _ => original,
        };
        let mut versions = self.versions.lock().unwrap();
        if versions.len() > MAX_KNOWN {
            versions.clear();
        }
        versions.insert(entry.id, (Instant::now(), version));
        version
    }

    /// Start playing a file: a request to Put.io with the player's `Range` (none for HEAD)
    pub async fn open(
        &self,
        id: i64,
        range: Option<&str>,
        head: bool,
    ) -> Result<Opened, PutioError> {
        if !self.is_inside(id).await? {
            return Err(PutioError::Outside(format!(
                "{id} is not in the shared folder"
            )));
        }
        let entry = self.entry(id).await?;
        if entry.is_folder {
            return Err(PutioError::Outside(format!("{id} is a folder")));
        }
        let kind = media::classify(&entry.name)
            .ok_or_else(|| PutioError::Outside(format!("{} is not media", entry.name)))?;
        let version = self.version(&entry).await;
        let mime = if version.mp4 {
            "video/mp4".to_string()
        } else {
            kind.mime
        };
        if head {
            return Ok(Opened {
                status: 200,
                content_length: Some(version.size.to_string()),
                content_range: None,
                mime,
                mp4: version.mp4,
                body: None,
            });
        }
        let url = if version.mp4 {
            self.client.mp4_download_url(id)
        } else {
            self.client.get_download_url(id)
        };
        let mut request = self
            .client
            .authenticated_download_request(&url)
            .map_err(|e| self.fail("starting a stream", &e))?;
        if let Some(range) = range {
            request = request.header(reqwest::header::RANGE, range);
        }
        let response = tokio::time::timeout(CALL_LIMIT, request.send())
            .await
            .map_err(|_| self.fail("starting a stream", &"timed out"))?
            .map_err(|e| self.fail("starting a stream", &redact(&e)))?;
        let status = response.status().as_u16();
        if !matches!(status, 200 | 206 | 416) {
            return Err(self.fail("starting a stream", &format!("Put.io answered {status}")));
        }
        self.ok();
        let header = |name| {
            response
                .headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        Ok(Opened {
            status,
            content_length: header(reqwest::header::CONTENT_LENGTH),
            content_range: header(reqwest::header::CONTENT_RANGE),
            mime,
            mp4: version.mp4,
            body: Some(response),
        })
    }
}

/// A reqwest error without the URL it carries
fn redact(e: &reqwest::Error) -> String {
    if e.is_timeout() {
        "timed out".to_string()
    } else if e.is_connect() {
        "could not connect".to_string()
    } else {
        "request failed".to_string()
    }
}

/// Browse inside the Put.io root. `rel` is empty for the shared folder, else a file id.
pub async fn browse(
    shared: &Shared,
    base: &str,
    key: &str,
    rel: &str,
    flag: BrowseFlag,
    start: usize,
    count: usize,
) -> Result<BrowseOut, Fault> {
    let root = shared
        .putio
        .as_ref()
        .ok_or_else(|| Fault::NoSuchObject("Put.io is not shared".to_string()))?;
    let id = if rel.is_empty() {
        root.folder
    } else {
        rel.parse::<i64>()
            .map_err(|_| Fault::NoSuchObject("not a Put.io id".to_string()))?
    };
    if !root.is_inside(id).await.map_err(content::putio_fault)? {
        return Err(Fault::NoSuchObject(format!(
            "{id} is not in the shared folder"
        )));
    }
    let title = shared
        .root(key)
        .map(|r| r.title.clone())
        .unwrap_or_else(|| "Put.io".to_string());
    let this = if id == root.folder {
        PutioEntry {
            id,
            name: title,
            is_folder: true,
            size: 0,
            parent: 0,
        }
    } else {
        root.entry(id).await.map_err(content::putio_fault)?
    };
    let id_of = |e: &PutioEntry| {
        if e.id == root.folder {
            ObjectId::node(key, "").format()
        } else {
            ObjectId::node(key, &e.id.to_string()).format()
        }
    };
    let this_id = id_of(&this);
    let parent_id = if this.id == root.folder {
        "0".to_string()
    } else {
        id_of(&PutioEntry {
            id: this.parent,
            name: String::new(),
            is_folder: true,
            size: 0,
            parent: 0,
        })
    };

    match flag {
        BrowseFlag::Metadata => {
            let media = media_for(root, &this).await;
            let element = content::putio_didl(key, &parent_id, &this, media, base)
                .ok_or_else(|| Fault::NoSuchObject("not media".to_string()))?;
            // A folder is addressed by its own id form (the root folder has no number)
            Ok(BrowseOut {
                didl: content::didl(&[element]),
                returned: 1,
                total: 1,
            })
        }
        BrowseFlag::DirectChildren => {
            if !this.is_folder {
                return Ok(BrowseOut {
                    didl: content::didl(&[]),
                    returned: 0,
                    total: 0,
                });
            }
            let listed = root.list(id).await.map_err(content::putio_fault)?;
            let visible: Vec<PutioEntry> = listed
                .iter()
                .filter(|e| e.is_folder || media::classify(&e.name).is_some())
                .cloned()
                .collect();
            let total = visible.len();
            let shown = content::paged(&visible, start, count);
            let this_id = &this_id;
            let elements: Vec<Option<String>> = futures_util::stream::iter(shown)
                .map(|e| async move {
                    let media = media_for(root, &e).await;
                    content::putio_didl(key, this_id, &e, media, base)
                })
                .buffered(8)
                .collect()
                .await;
            let elements: Vec<String> = elements.into_iter().flatten().collect();
            Ok(BrowseOut {
                returned: elements.len(),
                total,
                didl: content::didl(&elements),
            })
        }
    }
}

/// The class, mime and size an item is shown with
async fn media_for(root: &PutioRoot, e: &PutioEntry) -> Option<(MediaType, Option<u64>)> {
    if e.is_folder {
        return None;
    }
    let mut kind = media::classify(&e.name)?;
    let version = root.version(e).await;
    if version.mp4 {
        kind.mime = "video/mp4".to_string();
    }
    Some((kind, u64::try_from(version.size).ok()))
}
