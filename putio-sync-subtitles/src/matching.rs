//! The OpenSubtitles pass (feature 050): for the languages still missing after Put.io, search
//! by the file's fingerprint, then by the IMDb or TMDb id the Radarr/Sonarr history gives, then
//! by the title parsed from the file name. Each result is scored; the best one at or above the
//! threshold is saved. Stops at the first level that fills every language.

use std::collections::BTreeMap;

use putio_sync_core::media::{MediaInfo, MediaKind, MediaLookup, MediaRequest};
use tracing::{info, warn};

use crate::engine::{
    bounded, EngineError, Outcome, SavedSubtitle, SkipNote, SkipReason, VideoRequest,
};
use crate::naming::{self, Existing, Slot, SlotKind, WriteOutcome};
use crate::opensubtitles::{Hit, SearchError, SubtitleSearch};
use crate::release::ReleaseInfo;
use crate::score::{self, Facts, Level, Score};
use crate::srt;

/// What the OpenSubtitles pass needs besides the video
#[derive(Clone, Copy)]
pub struct Matching<'a> {
    /// The search and download client
    pub search: &'a dyn SubtitleSearch,
    /// The IDs the Radarr/Sonarr history holds, when an *arr is configured
    pub media: Option<&'a dyn MediaLookup>,
    /// The item's category, which names the *arr
    pub category: Option<&'a str>,
    /// Results scoring below this are not saved
    pub min_confidence: u8,
}

impl std::fmt::Debug for Matching<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Matching")
            .field("category", &self.category)
            .field("min_confidence", &self.min_confidence)
            .finish_non_exhaustive()
    }
}

/// Search with the hash, then the ids, then the name, for the downloading languages with no
/// main subtitle yet. Saves into `out` and `existing`; a transient failure with nothing saved
/// is an error so the step is retried.
pub(crate) async fn fill(
    req: &VideoRequest<'_>,
    matching: &Matching<'_>,
    out: &mut Outcome,
    existing: &mut Vec<Existing>,
) -> Result<(), EngineError> {
    let missing: Vec<String> = req
        .modes
        .download_languages()
        .into_iter()
        .filter(|l| !naming::has_slot(existing, &Slot::main(l)))
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    let name = req
        .video
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut pass = Pass {
        req,
        m: matching,
        video: ReleaseInfo::parse(&name),
        missing,
        out,
        existing,
        best_below: BTreeMap::new(),
        no_ids: false,
        stop: false,
        quota: false,
        transient: None,
    };
    if matching.search.quota().exhausted {
        pass.quota = true;
    } else {
        pass.by_hash().await;
        pass.by_ids().await;
        pass.by_name().await;
    }
    pass.finish()
}

struct Pass<'a, 'r> {
    req: &'a VideoRequest<'r>,
    m: &'a Matching<'a>,
    video: ReleaseInfo,
    missing: Vec<String>,
    out: &'a mut Outcome,
    existing: &'a mut Vec<Existing>,
    /// The best score per language that was under the threshold
    best_below: BTreeMap<String, u8>,
    no_ids: bool,
    stop: bool,
    quota: bool,
    transient: Option<String>,
}

/// What happened to one candidate
enum Next {
    Saved,
    TryNext,
    Stop,
}

impl Pass<'_, '_> {
    async fn by_hash(&mut self) {
        if self.done() {
            return;
        }
        let path = self.req.video.to_path_buf();
        let hash = tokio::task::spawn_blocking(move || crate::hash::opensubtitles_hash(&path))
            .await
            .ok()
            .and_then(|r| match r {
                Ok(h) => h,
                Err(e) => {
                    warn!(error = %e, "Could not read the video to fingerprint it");
                    None
                }
            });
        let Some(hash) = hash else { return };
        let params = vec![("moviehash".to_string(), hash)];
        self.level(params, |h| {
            if h.moviehash_match {
                score::score_hash(&facts_of(h))
            } else {
                Score::zero(Level::Hash)
            }
        })
        .await;
    }

    async fn by_ids(&mut self) {
        if self.done() {
            return;
        }
        let Some(params) = self.id_params().await else {
            self.no_ids = true;
            return;
        };
        let video = self.video.clone();
        self.level(params, move |h| score::score_ids(&video, &facts_of(h)))
            .await;
    }

    async fn by_name(&mut self) {
        if self.done() {
            return;
        }
        if self.video.title.is_empty() {
            return;
        }
        let mut params = vec![("query".to_string(), self.video.title.to_lowercase())];
        match (self.video.season, self.video.episode) {
            (Some(s), Some(e)) => {
                params.push(("type".into(), "episode".into()));
                params.push(("season_number".into(), s.to_string()));
                params.push(("episode_number".into(), e.to_string()));
            }
            _ => {
                params.push(("type".into(), "movie".into()));
                if let Some(y) = self.video.year {
                    params.push(("year".into(), y.to_string()));
                }
            }
        }
        let video = self.video.clone();
        self.level(params, move |h| score::score_name(&video, &facts_of(h)))
            .await;
    }

    fn done(&self) -> bool {
        self.stop || self.missing.is_empty()
    }

    /// The query for the ids level, or `None` when the *arr gives nothing to search by
    async fn id_params(&mut self) -> Option<Vec<(String, String)>> {
        let media = self.m.media?;
        let file_id = self.req.file_id?;
        let info = media
            .lookup(MediaRequest {
                putio_file_id: Some(file_id),
                category: self.m.category,
            })
            .await?;
        if !info.has_ids() {
            return None;
        }
        let mut params = Vec::new();
        if info.kind == Some(MediaKind::Episode) {
            let (season, episode) = self.pick_episode(&info)?;
            self.video.season = Some(season);
            self.video.episode = Some(episode);
            match (&info.imdb_id, info.tmdb_id) {
                (Some(imdb), _) => params.push(("parent_imdb_id".to_string(), imdb.clone())),
                (None, Some(tmdb)) => params.push(("parent_tmdb_id".to_string(), tmdb.to_string())),
                (None, None) => return None,
            }
            params.push(("season_number".into(), season.to_string()));
            params.push(("episode_number".into(), episode.to_string()));
        } else {
            match (&info.imdb_id, info.tmdb_id) {
                (Some(imdb), _) => params.push(("imdb_id".to_string(), imdb.clone())),
                (None, Some(tmdb)) => params.push(("tmdb_id".to_string(), tmdb.to_string())),
                (None, None) => return None,
            }
        }
        Some(params)
    }

    /// The season and episode this file is: the pair in its name that the *arr also lists, or
    /// the only pair the *arr lists when the name has none
    fn pick_episode(&self, info: &MediaInfo) -> Option<(u16, u16)> {
        match (self.video.season, self.video.episode) {
            (Some(s), Some(e)) => info.episodes.contains(&(s, e)).then_some((s, e)),
            _ => match info.episodes.as_slice() {
                [only] => Some(*only),
                _ => None,
            },
        }
    }

    /// One search for every missing language, then the best accepted result per language
    async fn level(&mut self, mut params: Vec<(String, String)>, scorer: impl Fn(&Hit) -> Score) {
        let mut langs = self.missing.clone();
        langs.sort();
        params.push(("languages".to_string(), langs.join(",")));
        for exclude in ["ai_translated", "hearing_impaired", "machine_translated"] {
            params.push((exclude.to_string(), "exclude".to_string()));
        }
        params.sort();
        let hits = match self.m.search.search(&params).await {
            Ok(h) => h,
            Err(e) => return self.failed(e),
        };
        for lang in self.missing.clone() {
            if self.stop {
                return;
            }
            let mut scored: Vec<(Score, &Hit)> = hits
                .iter()
                .filter(|h| h.language == lang)
                // A main subtitle is not machine made, for the hard of hearing or forced
                .filter(|h| !h.machine_translated && !h.hearing_impaired && !h.foreign_parts_only)
                .map(|h| (scorer(h), h))
                .filter(|(s, _)| s.total > 0)
                .collect();
            if let Some(best) = scored.iter().map(|(s, _)| s.total).max() {
                if !scored.iter().any(|(s, _)| s.accepts(self.m.min_confidence)) {
                    let e = self.best_below.entry(lang.clone()).or_insert(0);
                    *e = (*e).max(best);
                }
            }
            scored.retain(|(s, _)| s.accepts(self.m.min_confidence));
            scored.sort_by_key(|(s, _)| std::cmp::Reverse(s.rank()));
            for (score, hit) in scored {
                match self.save(&lang, hit, &score).await {
                    Next::Saved | Next::Stop => break,
                    Next::TryNext => {}
                }
            }
        }
    }

    fn failed(&mut self, e: SearchError) {
        self.stop = true;
        match e {
            SearchError::KeyRejected => {
                warn!("OpenSubtitles refused the key or login");
                self.out.warnings.push(e.to_string());
            }
            SearchError::Quota => self.quota = true,
            SearchError::RateLimited | SearchError::Failed(_) => {
                warn!(error = %e, "OpenSubtitles call failed");
                self.transient.get_or_insert(e.to_string());
                self.out.warnings.push(e.to_string());
            }
        }
    }

    async fn save(&mut self, lang: &str, hit: &Hit, score: &Score) -> Next {
        let bytes = match self.m.search.download(hit.file_id).await {
            Ok(b) => b,
            Err(SearchError::Failed(why)) => {
                warn!(language = lang, %why, "OpenSubtitles download failed");
                self.transient
                    .get_or_insert(format!("opensubtitles: {why}"));
                self.out.warnings.push(format!("{lang}: {why}"));
                return Next::TryNext;
            }
            Err(e) => {
                self.failed(e);
                return Next::Stop;
            }
        };
        let text = match srt::convert(&bytes, lang) {
            Ok(t) => t,
            Err(e) => {
                warn!(language = lang, error = %e, "OpenSubtitles subtitle unusable");
                self.out
                    .warnings
                    .push(format!("{lang}: unusable subtitle: {e}"));
                return Next::TryNext;
            }
        };
        let slot = Slot::main(lang);
        let target = naming::sidecar_path(self.req.video, &slot);
        match naming::write_new(&target, text.as_bytes()) {
            Ok(WriteOutcome::Written) => {
                let label = score.level.label();
                putio_sync_telemetry::subtitles::saved(label, &bounded(lang, self.req.languages));
                info!(language = lang, source = label, score = score.total, path = %target.display(), "Subtitle saved");
                self.existing.push(Existing {
                    language: lang.to_string(),
                    kind: SlotKind::Main,
                    path: target.clone(),
                });
                self.out.saved.push(SavedSubtitle {
                    path: target,
                    language: lang.to_string(),
                    kind: SlotKind::Main,
                    source: label,
                    bytes: text.len(),
                    wanted: true,
                    detail: score.describe(),
                });
                self.missing.retain(|l| l != lang);
                Next::Saved
            }
            // Another process wrote it first: it stays
            Ok(WriteOutcome::AlreadyExists) => {
                self.missing.retain(|l| l != lang);
                Next::Saved
            }
            Err(e) => {
                self.transient
                    .get_or_insert(format!("could not write: {e}"));
                Next::Stop
            }
        }
    }

    /// Notes for the languages left, and the retry for a failure that may pass
    fn finish(self) -> Result<(), EngineError> {
        for lang in &self.missing {
            let reason = if self.quota {
                SkipReason::Quota
            } else if let Some(best) = self.best_below.get(lang) {
                SkipReason::BelowThreshold(*best)
            } else if self.stop {
                SkipReason::NoResult
            } else if self.no_ids && self.video.title.is_empty() {
                SkipReason::NoName
            } else if self.no_ids {
                SkipReason::NoIds
            } else {
                SkipReason::NoResult
            };
            putio_sync_telemetry::subtitles::skipped(reason.metric());
            self.out.skipped.push(SkipNote {
                language: lang.clone(),
                reason,
            });
        }
        self.out.quota_used_up |= self.quota;
        if let Some(why) = self.transient {
            if !self.missing.is_empty() && self.out.saved.is_empty() {
                return Err(EngineError::Failed(why));
            }
        }
        Ok(())
    }
}

/// What a result says about itself, for scoring
fn facts_of(h: &Hit) -> Facts {
    let release = ReleaseInfo::parse(&h.release);
    let titles = [
        h.title.as_str(),
        h.parent_title.as_str(),
        release.title.as_str(),
    ]
    .into_iter()
    .filter(|t| !t.is_empty())
    .map(str::to_string)
    .collect();
    Facts {
        titles,
        year: h.year,
        season: h.season,
        episode: h.episode,
        release,
        downloads: h.downloads,
    }
}
