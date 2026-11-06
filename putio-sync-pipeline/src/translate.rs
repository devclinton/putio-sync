//! The Translating stage (feature 026): a scan step that finds which wanted languages a video
//! lacks and adds one `translate` step for each (branch = language code), and the step that
//! makes one subtitle.
//!
//! Like the Subtitles stage, these are ordinary pipeline steps, so retries, cancel, the gate,
//! staging and the timeline come with them. `putio-sync-translation` does the translating.

use std::path::{Path, PathBuf};
use std::time::Instant;

use putio_sync_core::log_redact;
use putio_sync_core::translation::TranslatorKind;
use putio_sync_subtitles::naming::{self, Slot, SlotKind, WriteOutcome};
use putio_sync_subtitles::srt;
use putio_sync_translation::chunk::{LogCtx, TranslateError};
use putio_sync_translation::engine::{self, Job};
use putio_sync_translation::source::{self, Candidate, Origin};
use tracing::{info, warn};

use crate::model::{AttemptResult, SkipReason, Stage, StepKind, StepState};
use crate::plan::{PlannedStep, VIDEO_POSITION};
use crate::runner::{Ended, Outcome};
use crate::store::{ItemRow, Provenance, StepRow};
use crate::subtitles::video_path;
use crate::Pipeline;

/// Source files that can be read as a subtitle
fn readable(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("srt") || e.eq_ignore_ascii_case("vtt"))
}

/// The label for a language in metrics: a wanted language, else `other`
pub(crate) fn language_label(language: &str, wanted: &[String]) -> String {
    if wanted.iter().any(|w| w == language) {
        language.to_string()
    } else {
        "other".to_string()
    }
}

/// What the step needs before it translates
pub(crate) struct Source {
    pub language: String,
    pub cues: Vec<srt::Cue>,
    /// The file the cues were read from, and the hash of its bytes
    pub path: PathBuf,
    pub sha256: String,
}

/// Why the step ends before translating
pub(crate) enum Early {
    /// Passes with this message (the file is there already, or there is nothing to do)
    Pass(String),
    /// Fails with this message
    Fail(String),
    /// The callback service accepted the job: the step stays `running` with its attempt
    /// open until the callback or the time limit ends it
    Parked,
}

impl Pipeline {
    /// Whether translation is off for this item now (settings changed since the plan)
    fn translation_off(&self, item: &ItemRow) -> Option<SkipReason> {
        let settings = self.settings();
        if !settings.translation.is_configured() || !settings.subtitles.is_configured() {
            Some(SkipReason::NotConfigured)
        } else if item
            .category
            .as_deref()
            .is_some_and(|c| settings.translation.excludes(c))
        {
            Some(SkipReason::Category)
        } else {
            None
        }
    }

    /// The main-slot sidecars of `video` that could be the source, with where they came from
    pub(crate) async fn source_candidates(&self, video: &Path, target: &str) -> Vec<Candidate> {
        let mut out = Vec::new();
        for e in naming::existing_sidecars(video) {
            if e.kind != SlotKind::Main || e.language == target || !readable(&e.path) {
                continue;
            }
            let size = std::fs::metadata(&e.path).map(|m| m.len() as i64).ok();
            let origin = match self
                .inner
                .store
                .provenance(&e.path.to_string_lossy(), size)
                .await
            {
                Ok(Some(p)) if p.source == "translated" => Origin::Translated,
                Ok(Some(p)) if p.source == "transcribed" => Origin::Transcribed,
                _ => Origin::Other,
            };
            out.push(Candidate {
                language: e.language,
                path: e.path,
                origin,
            });
        }
        out
    }

    /// The scan step: for each video and wanted language with no file, add a `translate` step
    pub(crate) async fn run_translate_scan(&self, item: &ItemRow, step: &StepRow) -> Outcome {
        if let Some(why) = self.translation_off(item) {
            return self.skip_step(step, why).await;
        }
        let store = &self.inner.store;
        let Ok((attempt_id, n)) = store.start_attempt(step.id).await else {
            return Outcome::Stopped;
        };
        let started = Instant::now();
        let scan = self.scan_videos(item).await;
        if scan.videos.is_empty() {
            let _ = store
                .end_attempt(attempt_id, AttemptResult::Skip, None, "no videos")
                .await;
            putio_sync_telemetry::steps::step(
                step.stage.as_str(),
                "translate_scan",
                "skip",
                started.elapsed(),
            );
            return self.skip_step(step, SkipReason::NoVideo).await;
        }
        let wanted = self.settings().subtitles.wanted();
        let mut per_language: Vec<(String, Vec<PlannedStep>)> = Vec::new();
        let mut total = 0;
        let sources = scan.videos.clone();
        for language in &wanted {
            let mut steps = Vec::new();
            for relative in &sources {
                let video = video_path(item, relative);
                let have = naming::existing_sidecars(&video);
                if naming::has_slot(&have, &Slot::main(language)) {
                    continue;
                }
                // Nothing to translate from now means nothing later either
                if source::pick(
                    &self.source_candidates(&video, language).await,
                    &wanted,
                    language,
                )
                .is_none()
                {
                    continue;
                }
                steps.push(translate_step(language, relative));
            }
            total += steps.len();
            per_language.push((language.clone(), steps));
        }
        let message = if total == 0 {
            "nothing to translate".to_string()
        } else {
            format!("{total} subtitles to translate")
        };
        for (_, steps) in &per_language {
            if steps.is_empty() {
                continue;
            }
            if let Err(e) = store.insert_steps(&item.key, steps).await {
                return self
                    .conclude(
                        item,
                        step,
                        Ended {
                            attempt_id,
                            n,
                            kind: "translate_scan",
                            result: AttemptResult::Failed,
                            code: None,
                            output: e.to_string(),
                            error: Some(e.to_string()),
                            took: started.elapsed(),
                        },
                    )
                    .await;
            }
        }
        self.conclude(
            item,
            step,
            Ended {
                attempt_id,
                n,
                kind: "translate_scan",
                result: AttemptResult::Done,
                code: None,
                output: message,
                error: None,
                took: started.elapsed(),
            },
        )
        .await
    }

    /// Choose and read the subtitle to translate from
    pub(crate) async fn read_source(&self, video: &Path, target: &str) -> Result<Source, Early> {
        let wanted = self.settings().subtitles.wanted();
        let candidates = self.source_candidates(video, target).await;
        let Some(pick) = source::pick(&candidates, &wanted, target) else {
            return Err(Early::Pass("no source subtitle".to_string()));
        };
        let path = pick.path.clone();
        let language = pick.language.clone();
        let read = tokio::task::spawn_blocking({
            let path = path.clone();
            move || std::fs::read(&path)
        })
        .await;
        let bytes = match read {
            Ok(Ok(b)) => b,
            Ok(Err(e)) => {
                return Err(Early::Fail(format!(
                    "could not read the source subtitle: {e}"
                )))
            }
            Err(e) => {
                return Err(Early::Fail(format!(
                    "could not read the source subtitle: {e}"
                )))
            }
        };
        let text = srt::decode_bytes(&bytes, &language)
            .map_err(|e| Early::Fail(format!("the source subtitle is not readable: {e}")))?;
        let cues = srt::parse(&text);
        if cues.is_empty() {
            return Err(Early::Fail("the source subtitle has no cues".to_string()));
        }
        let sha256 = putio_sync_translation::callback::sha256_hex(&bytes);
        Ok(Source {
            language,
            cues,
            path,
            sha256,
        })
    }

    /// Save a finished translation next to the video, never over a file, and remember it
    pub(crate) async fn save_translation(
        &self,
        video: &Path,
        target: &str,
        cues: &[srt::Cue],
    ) -> Result<(PathBuf, WriteOutcome), String> {
        let path = naming::sidecar_path(video, &Slot::main(target));
        let data = srt::render(cues).into_bytes();
        let bytes = data.len() as i64;
        let write = tokio::task::spawn_blocking({
            let path = path.clone();
            move || naming::write_new(&path, &data)
        })
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("could not write {}: {e}", path.display()))?;
        if write == WriteOutcome::Written {
            let row = Provenance {
                path: path.to_string_lossy().into_owned(),
                language: target.to_string(),
                source: "translated".to_string(),
                bytes,
            };
            if let Err(e) = self.inner.store.upsert_provenance(&row).await {
                warn!(error = %e, "Could not record where a translation came from");
            }
        }
        Ok((path, write))
    }

    /// One translation: one video, one target language (the step's branch)
    pub(crate) async fn run_translate(&self, item: &ItemRow, step: &StepRow) -> Outcome {
        if let Some(why) = self.translation_off(item) {
            return self.skip_step(step, why).await;
        }
        let store = &self.inner.store;
        let Ok((attempt_id, n)) = store.start_attempt(step.id).await else {
            return Outcome::Stopped;
        };
        let started = Instant::now();
        let target = step.branch.clone().unwrap_or_default();
        let video = video_path(item, &step.name);
        let kind = self.inner.translator.kind();
        let label = kind.as_str();
        let wanted = self.settings().subtitles.wanted();
        let cancel = self.cancel_of(&item.key);

        let run = async {
            if target.is_empty() {
                return Err(Early::Fail("the step has no target language".to_string()));
            }
            if !video.is_file() {
                return Err(Early::Fail(format!(
                    "the video is not there: {}",
                    step.name
                )));
            }
            let have = naming::existing_sidecars(&video);
            if naming::has_slot(&have, &Slot::main(&target)) {
                return Err(Early::Pass(format!("{target}: already exists")));
            }
            let source = self.read_source(&video, &target).await?;
            let log = LogCtx {
                key: item.key.clone(),
                video: step.name.clone(),
            };
            let (cues, detail) = match kind {
                TranslatorKind::Chat => {
                    let t = self
                        .translate_chat(step, &source, &target, log)
                        .await
                        .map_err(|e| Early::Fail(log_redact::redact(&e.to_string())))?;
                    let detail = format!(
                        "{} chunks, {} retries, {} splits",
                        t.stats.chunks, t.stats.retries, t.stats.splits
                    );
                    (t.cues, detail)
                }
                TranslatorKind::Callback => {
                    let (cues, realigned) = self
                        .translate_callback(step, attempt_id, &source, &target, &video)
                        .await?;
                    let detail = format!("{} cues, realigned {realigned}", cues.len());
                    (cues, detail)
                }
                TranslatorKind::None => {
                    return Err(Early::Fail("no translator is selected".to_string()))
                }
            };
            let (path, write) = self
                .save_translation(&video, &target, &cues)
                .await
                .map_err(Early::Fail)?;
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            Ok(match write {
                WriteOutcome::Written => {
                    format!("{target}: saved {name} from {} ({detail})", source.language)
                }
                WriteOutcome::AlreadyExists => format!("{target}: already exists"),
            })
        };
        let run = tokio::select! {
            run = run => Some(run),
            _ = cancel.notified() => None,
        };
        let took = started.elapsed();
        let Some(run) = run else {
            if let Ok(jobs) = store
                .expire_for_step(step.id, crate::callback::unix_now())
                .await
            {
                self.abandon_jobs(jobs).await;
            }
            let _ = store
                .end_attempt(attempt_id, AttemptResult::Interrupted, None, "")
                .await;
            let _ = store
                .set_step(step.id, StepState::Interrupted, None, None)
                .await;
            putio_sync_telemetry::steps::step(
                step.stage.as_str(),
                "translate",
                "interrupted",
                took,
            );
            return Outcome::Stopped;
        };
        if matches!(run, Err(Early::Parked)) {
            // The attempt stays open and the step `running`; the callback or the time
            // limit ends them. This task is done, which frees the item's permit.
            return Outcome::Continue;
        }
        let language = language_label(&target, &wanted);
        let (result, output, error) = match run {
            Ok(message) => {
                putio_sync_telemetry::translation::done(label, "done", &language, took);
                info!(
                    key = %item.key, video = %step.name, language = %target, kind = label,
                    duration_ms = took.as_millis() as u64, "translation done"
                );
                (AttemptResult::Done, message, None)
            }
            Err(Early::Parked) => unreachable!("handled above"),
            Err(Early::Pass(message)) => (AttemptResult::Done, message, None),
            Err(Early::Fail(why)) => {
                putio_sync_telemetry::translation::done(label, "failed", &language, took);
                warn!(
                    key = %item.key, video = %step.name, language = %target, kind = label,
                    error = %why, "translation failed"
                );
                (AttemptResult::Failed, why.clone(), Some(why))
            }
        };
        self.conclude(
            item,
            step,
            Ended {
                attempt_id,
                n,
                kind: "translate",
                result,
                code: None,
                output,
                error,
                took,
            },
        )
        .await
    }

    /// Translate through the chat server, writing progress to the step
    async fn translate_chat(
        &self,
        step: &StepRow,
        source: &Source,
        target: &str,
        log: LogCtx,
    ) -> Result<engine::Translation, TranslateError> {
        let translator = &self.inner.translator;
        let config = translator.config();
        let client = translator.chat_client().map_err(TranslateError::Refused)?;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<f64>();
        let store = self.inner.store.clone();
        let id = step.id;
        let writer = tokio::spawn(async move {
            while let Some(p) = rx.recv().await {
                let _ = store.set_progress(id, Some(p)).await;
            }
        });
        let progress: engine::Progress = Box::new(move |done, total| {
            let _ = tx.send(done as f64 / total.max(1) as f64);
        });
        let result = engine::translate_with_chat(
            client,
            Job {
                source: &source.cues,
                source_code: &source.language,
                target_code: target,
                instruction: &config.instruction,
                chunk_cues: config.chunk_cues,
                log,
            },
            progress,
        )
        .await;
        let _ = writer.await;
        let _ = self.inner.store.set_progress(id, None).await;
        result
    }
}

/// A `translate` step for one video and language
pub(crate) fn translate_step(language: &str, relative: &str) -> PlannedStep {
    PlannedStep {
        stage: Stage::Translating,
        branch: Some(language.to_string()),
        position: VIDEO_POSITION,
        kind: StepKind::Translate,
        name: relative.to_string(),
        required: false,
        changes_files: true,
        state: StepState::Waiting,
        skip_reason: None,
    }
}
