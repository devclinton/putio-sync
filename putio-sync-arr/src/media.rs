//! What the *arr knows about a download (feature 050): the IMDb and TMDb ids and the episodes,
//! read from the Radarr/Sonarr history by the download's info hash. The subtitles crate only
//! sees the [`MediaLookup`] trait from core, so the two crates stay apart.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use putio_sync_core::media::{BoxFut, MediaInfo, MediaLookup, MediaRequest};
use tracing::{debug, warn};

use crate::service::ArrService;

/// How long an answer, found or not, is kept. The history for a finished download does not
/// change, and a miss must not be asked again for every file of a season pack.
const TTL: Duration = Duration::from_secs(60 * 60);

/// Answers [`MediaLookup`] from the *arr history
pub struct ArrMedia {
    service: Arc<ArrService>,
    cache: Mutex<HashMap<String, (Instant, Option<MediaInfo>)>>,
}

impl ArrMedia {
    pub fn new(service: Arc<ArrService>) -> Self {
        Self {
            service,
            cache: Mutex::new(HashMap::new()),
        }
    }

    fn cached(&self, hash: &str) -> Option<Option<MediaInfo>> {
        let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache
            .get(hash)
            .filter(|(at, _)| at.elapsed() < TTL)
            .map(|(_, info)| info.clone())
    }

    fn keep(&self, hash: &str, info: Option<MediaInfo>) {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.retain(|_, (at, _)| at.elapsed() < TTL);
        cache.insert(hash.to_string(), (Instant::now(), info));
    }
}

impl MediaLookup for ArrMedia {
    fn lookup<'a>(&'a self, req: MediaRequest<'a>) -> BoxFut<'a, Option<MediaInfo>> {
        Box::pin(async move {
            let file_id = req.putio_file_id?;
            let item = self.service.item_for_file(file_id).await?;
            if let Some(hit) = self.cached(&item.hash) {
                return hit;
            }
            let category = req.category.unwrap_or(&item.category);
            let api = self.service.blocklist_server(category)?;
            let info = match api.history_by_download(&item.hash).await {
                Ok(info) if info.kind.is_some() => Some(info),
                Ok(_) => {
                    debug!(hash = %item.hash, "The *arr history has no record of this download");
                    None
                }
                // An unreachable *arr is not remembered: the next video asks again
                Err(e) => {
                    warn!(hash = %item.hash, error = %e, "Could not read the *arr history");
                    return None;
                }
            };
            self.keep(&item.hash, info.clone());
            info
        })
    }
}
