//! The work for one video: the Put.io pass (existing sidecars, list, pick, download, convert,
//! write) and, when it finds nothing, the transcription pass.
//!
//! Nothing here touches the database or the pipeline. It returns an [`Outcome`] the step
//! records, or an [`EngineError`] the pipeline retries.

use std::path::{Path, PathBuf};

use tracing::{info, warn};

use crate::candidates::{self, Candidate, SourceKind};
use crate::naming::{self, Slot, SlotKind, WriteOutcome};
use crate::srt;
use crate::{SourceError, SubtitleSource};

/// What the engine needs to know about one video
#[derive(Debug, Clone)]
pub struct VideoRequest<'a> {
    /// The video on disk (inside the staging folder when staging is on)
    pub video: &'a Path,
    /// The Put.io file id, when the file is still on Put.io and the id is known
    pub file_id: Option<i64>,
    /// Wanted languages, canonical codes, in order
    pub languages: &'a [String],
}

/// A subtitle file the engine wrote
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedSubtitle {
    pub path: PathBuf,
    pub language: String,
    pub kind: SlotKind,
    /// The metrics label: `putio_embedded`, `putio_folder`, `opensubtitles` or `transcribed`
    pub source: &'static str,
    pub bytes: usize,
    /// Whether `language` is one of the wanted ones
    pub wanted: bool,
}

/// What happened to one video
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Files written now
    pub saved: Vec<SavedSubtitle>,
    /// Wanted languages that had a main subtitle already
    pub present: Vec<String>,
    /// Wanted languages with no main subtitle at the end
    pub missing: Vec<String>,
    /// Candidates that failed and were passed over, and other notes
    pub warnings: Vec<String>,
}

impl Outcome {
    /// Nothing was found for any wanted language and nothing was saved
    pub fn none_found(&self) -> bool {
        self.saved.is_empty() && self.present.is_empty()
    }

    /// The line shown on the step, such as `en: opensubtitles; es: missing`
    pub fn message(&self) -> String {
        let mut parts = Vec::new();
        for s in &self.saved {
            let slot = match s.kind {
                SlotKind::Main => String::new(),
                SlotKind::Forced => " forced".to_string(),
                SlotKind::Sdh => " sdh".to_string(),
            };
            parts.push(format!("{}{}: {}", s.language, slot, s.source));
        }
        for l in &self.present {
            parts.push(format!("{l}: already present"));
        }
        for l in &self.missing {
            parts.push(format!("{l}: missing"));
        }
        if parts.is_empty() {
            "no subtitles wanted".to_string()
        } else {
            parts.join("; ")
        }
    }
}

/// A failure worth a retry
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EngineError {
    /// Put.io or the disk failed in a way that may pass on a later attempt
    #[error("{0}")]
    Failed(String),
}

/// The Put.io pass for one video
pub async fn fetch_for_video(
    source: &dyn SubtitleSource,
    req: &VideoRequest<'_>,
) -> Result<Outcome, EngineError> {
    let mut out = Outcome::default();
    let mut existing = naming::existing_sidecars(req.video);
    let had: Vec<String> = req
        .languages
        .iter()
        .filter(|l| naming::has_slot(&existing, &Slot::main(l)))
        .cloned()
        .collect();

    let mut all: Vec<Candidate> = Vec::new();
    if let Some(id) = req.file_id {
        if !req.languages.is_empty() {
            match source.list(id, req.languages).await {
                Ok(list) => all = candidates::from_list(&list),
                Err(SourceError::Unavailable(why)) => {
                    info!(file_id = id, %why, "Put.io has no subtitles for this file");
                }
                Err(SourceError::Failed(why)) => return Err(EngineError::Failed(why)),
            }
        }
    }

    let mut transient: Option<String> = None;
    for lang in req.languages {
        for kind in [SlotKind::Main, SlotKind::Forced, SlotKind::Sdh] {
            let slot = Slot {
                language: lang.clone(),
                kind,
            };
            if naming::has_slot(&existing, &slot) {
                continue;
            }
            let (Some(id), true) = (req.file_id, !all.is_empty()) else {
                continue;
            };
            for cand in candidates::select(&all, lang, kind) {
                match try_candidate(source, id, cand, req, &slot).await {
                    Ok(Some(saved)) => {
                        existing.push(naming::Existing {
                            language: lang.clone(),
                            kind,
                            path: saved.path.clone(),
                        });
                        out.saved.push(saved);
                        break;
                    }
                    // Another process wrote the file first: it stays
                    Ok(None) => break,
                    Err(CandidateError::Bad(why)) => {
                        warn!(language = %lang, key = %cand.raw.key, %why, "Subtitle candidate failed");
                        out.warnings.push(format!("{lang}: {why}"));
                    }
                    Err(CandidateError::Transient(why)) => {
                        warn!(language = %lang, key = %cand.raw.key, %why, "Subtitle download failed");
                        transient.get_or_insert(why.clone());
                        out.warnings.push(format!("{lang}: {why}"));
                    }
                }
            }
        }
    }

    for lang in req.languages {
        if had.contains(lang) {
            out.present.push(lang.clone());
        } else if !naming::has_slot(&existing, &Slot::main(lang)) {
            out.missing.push(lang.clone());
        }
    }
    if let Some(why) = transient {
        if !out.missing.is_empty() && out.saved.is_empty() {
            return Err(EngineError::Failed(why));
        }
    }
    Ok(out)
}

enum CandidateError {
    /// The subtitle is unusable (empty, no cues, bad encoding): try the next
    Bad(String),
    /// Put.io failed while giving it: the step may succeed later
    Transient(String),
}

async fn try_candidate(
    source: &dyn SubtitleSource,
    file_id: i64,
    cand: &Candidate,
    req: &VideoRequest<'_>,
    slot: &Slot,
) -> Result<Option<SavedSubtitle>, CandidateError> {
    let bytes = source
        .download(file_id, &cand.raw)
        .await
        .map_err(|e| match e {
            SourceError::Unavailable(w) => CandidateError::Bad(w),
            SourceError::Failed(w) => CandidateError::Transient(w),
        })?;
    let text = srt::convert(&bytes, &slot.language)
        .map_err(|e| CandidateError::Bad(format!("unusable subtitle: {e}")))?;
    let target = naming::sidecar_path(req.video, slot);
    match naming::write_new(&target, text.as_bytes()) {
        Ok(WriteOutcome::Written) => {
            let label = metric_source(cand.source);
            putio_sync_telemetry::subtitles::saved(label, &bounded(&slot.language, req.languages));
            info!(
                language = %slot.language, source = label, path = %target.display(),
                "Subtitle saved"
            );
            Ok(Some(SavedSubtitle {
                path: target,
                language: slot.language.clone(),
                kind: slot.kind,
                source: label,
                bytes: text.len(),
                wanted: req.languages.contains(&slot.language),
            }))
        }
        Ok(WriteOutcome::AlreadyExists) => Ok(None),
        Err(e) => Err(CandidateError::Transient(format!("could not write: {e}"))),
    }
}

fn metric_source(kind: SourceKind) -> &'static str {
    kind.as_metric()
}

/// The language label: a wanted language, else `other`
pub fn bounded(language: &str, wanted: &[String]) -> String {
    if wanted.iter().any(|w| w == language) {
        language.to_string()
    } else {
        "other".to_string()
    }
}
