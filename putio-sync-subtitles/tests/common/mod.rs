//! Fakes for the OpenSubtitles pass: a search service and an *arr lookup that answer from
//! memory. Nothing here touches the network.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use putio_sync_core::media::{MediaInfo, MediaLookup, MediaRequest};
use putio_sync_subtitles::opensubtitles::{Hit, Quota, SearchError, SubtitleSearch};
use putio_sync_subtitles::BoxFut;

pub type Params = Vec<(String, String)>;
type Answer = Box<dyn Fn(&[(String, String)]) -> Result<Vec<Hit>, SearchError> + Send + Sync>;

pub const SRT: &str = "1\n00:00:01,000 --> 00:00:02,000\nHello\n";

pub struct FakeSearch {
    answer: Answer,
    pub calls: Mutex<Vec<Params>>,
    pub downloads: Mutex<Vec<i64>>,
    /// File ids whose download gives this instead of a good subtitle
    pub broken: Vec<i64>,
    pub download_error: Option<SearchError>,
    pub exhausted: bool,
}

impl FakeSearch {
    pub fn new(
        answer: impl Fn(&[(String, String)]) -> Result<Vec<Hit>, SearchError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            answer: Box::new(answer),
            calls: Mutex::default(),
            downloads: Mutex::default(),
            broken: Vec::new(),
            download_error: None,
            exhausted: false,
        }
    }

    /// Always the same hits
    pub fn hits(hits: Vec<Hit>) -> Self {
        Self::new(move |_| Ok(hits.clone()))
    }

    pub fn searches(&self) -> Vec<Params> {
        self.calls.lock().unwrap().clone()
    }
}

pub fn has(params: &[(String, String)], key: &str) -> Option<String> {
    params
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
}

impl SubtitleSearch for FakeSearch {
    fn search<'a>(
        &'a self,
        params: &'a [(String, String)],
    ) -> BoxFut<'a, Result<Vec<Hit>, SearchError>> {
        self.calls.lock().unwrap().push(params.to_vec());
        let r = (self.answer)(params);
        Box::pin(async move { r })
    }

    fn download<'a>(&'a self, file_id: i64) -> BoxFut<'a, Result<Vec<u8>, SearchError>> {
        self.downloads.lock().unwrap().push(file_id);
        let r = if let Some(e) = &self.download_error {
            Err(e.clone())
        } else if self.broken.contains(&file_id) {
            Ok(b"not a subtitle".to_vec())
        } else {
            Ok(SRT.as_bytes().to_vec())
        };
        Box::pin(async move { r })
    }

    fn quota(&self) -> Quota {
        Quota {
            exhausted: self.exhausted,
            ..Default::default()
        }
    }
}

pub struct FakeMedia(pub Option<MediaInfo>);

impl MediaLookup for FakeMedia {
    fn lookup<'a>(&'a self, _: MediaRequest<'a>) -> BoxFut<'a, Option<MediaInfo>> {
        let r = self.0.clone();
        Box::pin(async move { r })
    }
}

/// A result for `lang` with sensible defaults
pub fn hit(file_id: i64, lang: &str, release: &str) -> Hit {
    Hit {
        file_id,
        language: lang.to_string(),
        release: release.to_string(),
        ..Default::default()
    }
}

/// A video file big enough to be fingerprinted
pub fn video(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    std::fs::write(&p, vec![7u8; 200 * 1024]).unwrap();
    p
}
