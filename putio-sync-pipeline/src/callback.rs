//! Callback translation (feature 026, research R6-R8): the step sends the subtitle to the
//! user's service and, when the service answers `202`, parks. A parked step stays `running`
//! with its attempt open and a `waiting` job row; the item's task ends, which gives its
//! permit back, and the other branches go on.
//!
//! The result arrives at [`Pipeline::accept_callback`] (the unauthenticated route), which
//! checks it, saves it and finishes the step. When time runs out
//! [`Pipeline::expire_callbacks`] fails the step with attempt result `timeout` and no
//! automatic retry: the service was asked once and the operator can retry.

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use putio_sync_subtitles::srt;
use putio_sync_translation::callback::{self, CallbackClient, JobRequest, Parsed, Reply};
use putio_sync_translation::engine;
use tracing::{info, warn};

use crate::model::{AttemptResult, StepState};
use crate::runner::Ended;
use crate::store::{CallbackJob, ItemRow, StepRow};
use crate::subtitles::video_path;
use crate::translate::{Early, Source};
use crate::Pipeline;

/// Why a callback was not taken. The route answers `404` for [`Self::NotFound`], whatever the
/// cause, so a caller learns nothing about tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcceptError {
    /// Unknown, malformed, used or expired token
    NotFound,
    /// The token was good and is used up; the body could not be taken
    Unprocessable(String),
}

/// Seconds since the epoch
pub(crate) fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Pipeline {
    /// Send the subtitle to the callback service. `Ok` is a finished translation (an
    /// immediate `200`); `Err(Early::Parked)` means the service will call back.
    pub(crate) async fn translate_callback(
        &self,
        step: &StepRow,
        attempt_id: i64,
        source: &Source,
        target: &str,
        video: &Path,
    ) -> Result<(Vec<srt::Cue>, usize), Early> {
        let store = &self.inner.store;
        let config = self.inner.translator.config();
        let c = &config.callback;
        let client = CallbackClient::new(&c.url, &c.headers, config.timeout_secs)
            .map_err(|e| Early::Fail(format!("the callback settings are not usable: {e}")))?;
        let token = callback::new_token();
        let hash = callback::hash_token(&token);
        let job_id = callback::new_job_id();
        let url = callback::callback_url(&c.callback_base_url, &token);
        let now = unix_now();
        let job = CallbackJob {
            token_hash: hash.clone(),
            job_id: job_id.clone(),
            step_id: step.id,
            attempt_id,
            source_lang: source.language.clone(),
            target_lang: target.to_string(),
            source_path: source.path.to_string_lossy().into_owned(),
            source_sha256: source.sha256.clone(),
            target_path: video.to_string_lossy().into_owned(),
            state: "waiting".to_string(),
            created_at: now,
            expires_at: now + c.timeout_secs as i64,
            finished_at: None,
        };
        // The job exists before the request goes out: a service may call back before it
        // has finished answering
        store
            .insert_job(&job)
            .await
            .map_err(|e| Early::Fail(format!("could not record the job: {e}")))?;
        self.refresh_callback_gauge().await;
        let srt_text = srt::render(&source.cues);
        let video_name = video
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        info!(
            job = %job_id, video = %video_name, language = %target,
            "translation job sent to the callback service"
        );
        let reply = client
            .submit(&JobRequest {
                job_id: &job_id,
                source_lang: &source.language,
                target_lang: target,
                video_name: &video_name,
                srt: &srt_text,
                callback_url: &url,
            })
            .await;
        match reply {
            Reply::Immediate(text) => {
                let _ = store.finish_job(&hash, "done", unix_now()).await;
                self.refresh_callback_gauge().await;
                let checked = engine::check_result(&source.cues, &text)
                    .map_err(|e| Early::Fail(e.to_string()))?;
                Ok((checked.cues, checked.realigned))
            }
            Reply::Failed(message) => {
                let _ = store.finish_job(&hash, "failed", unix_now()).await;
                self.refresh_callback_gauge().await;
                Err(Early::Fail(message))
            }
            Reply::Accepted => {
                // The callback may have come in before this reply was read; then the step
                // is finished already and must not be parked
                match store.job(&hash).await {
                    Ok(Some(j)) if j.state == "waiting" => {
                        info!(job = %job_id, "translation accepted, waiting for the callback");
                        Err(Early::Parked)
                    }
                    _ => Err(Early::Parked),
                }
            }
        }
    }

    /// The service called back. Returns when the step is finished or failed; the item goes
    /// on by itself.
    pub async fn accept_callback(
        &self,
        token: &str,
        content_type: Option<&str>,
        body: &str,
    ) -> Result<(), AcceptError> {
        let store = &self.inner.store;
        if !callback::is_well_formed(token) {
            putio_sync_telemetry::translation::callback_refused("malformed");
            return Err(AcceptError::NotFound);
        }
        let hash = callback::hash_token(token);
        let now = unix_now();
        let job = match store.claim(&hash, now).await {
            Ok(Some(job)) => job,
            Ok(None) => {
                let reason = match store.job(&hash).await {
                    Ok(None) | Err(_) => "unknown",
                    Ok(Some(j)) if j.state == "waiting" || j.state == "expired" => "expired",
                    Ok(Some(_)) => "used",
                };
                putio_sync_telemetry::translation::callback_refused(reason);
                return Err(AcceptError::NotFound);
            }
            Err(e) => {
                warn!(error = %e, "Could not look up a translation callback");
                return Err(AcceptError::NotFound);
            }
        };
        self.refresh_callback_gauge().await;
        let Some((item, step)) = self.job_owner(&job).await else {
            let _ = store.finish_job(&hash, "failed", now).await;
            putio_sync_telemetry::translation::callback_refused("unknown");
            return Err(AcceptError::NotFound);
        };
        let took = Duration::from_secs((now - job.created_at).max(0) as u64);
        let n = self.attempt_number(&job).await;

        // The service reported a failure itself
        let text = match callback::parse_body(content_type, body) {
            Ok(Parsed::Srt(text)) => text,
            Ok(Parsed::Error(message)) => {
                let message = format!("the service said: {}", self.scrub_service(&message));
                self.fail_job(&item, &step, &job, n, took, message).await;
                return Ok(());
            }
            Err(message) => {
                putio_sync_telemetry::translation::callback_refused("bad_body");
                self.fail_job(&item, &step, &job, n, took, message.clone())
                    .await;
                return Err(AcceptError::Unprocessable(message));
            }
        };

        // The result is for the subtitle that was sent
        let source = match self.reread_source(&job).await {
            Ok(cues) => cues,
            Err(message) => {
                putio_sync_telemetry::translation::callback_refused("source_changed");
                self.fail_job(&item, &step, &job, n, took, message.clone())
                    .await;
                return Err(AcceptError::Unprocessable(message));
            }
        };
        let checked = match engine::check_result(&source, &text) {
            Ok(c) => c,
            Err(e) => {
                let message = e.to_string();
                putio_sync_telemetry::translation::callback_refused("bad_body");
                self.fail_job(&item, &step, &job, n, took, message.clone())
                    .await;
                return Err(AcceptError::Unprocessable(message));
            }
        };
        let video = video_path(&item, &step.name);
        let saved = self
            .save_translation(&video, &job.target_lang, &checked.cues)
            .await;
        let (path, write) = match saved {
            Ok(saved) => saved,
            Err(message) => {
                self.fail_job(&item, &step, &job, n, took, message.clone())
                    .await;
                return Err(AcceptError::Unprocessable(message));
            }
        };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let message = match write {
            putio_sync_subtitles::naming::WriteOutcome::Written => format!(
                "{}: saved {name} from {} ({} cues, realigned {})",
                job.target_lang,
                job.source_lang,
                checked.cues.len(),
                checked.realigned
            ),
            putio_sync_subtitles::naming::WriteOutcome::AlreadyExists => {
                format!("{}: already exists", job.target_lang)
            }
        };
        let wanted = self.settings().subtitles.wanted();
        putio_sync_telemetry::translation::done(
            "callback",
            "done",
            &crate::translate::language_label(&job.target_lang, &wanted),
            took,
        );
        info!(
            key = %item.key, job = %job.job_id, language = %job.target_lang,
            "translation received from the callback service"
        );
        self.conclude(
            &item,
            &step,
            Ended {
                attempt_id: job.attempt_id,
                n,
                kind: "translate",
                result: AttemptResult::Done,
                code: None,
                output: message,
                error: None,
                took,
            },
        )
        .await;
        self.inner.wake.notify_one();
        Ok(())
    }

    /// Jobs whose time is up: the step fails (attempt `timeout`) and is not retried
    pub async fn expire_callbacks(&self) {
        let store = &self.inner.store;
        let jobs = match store.expire_due(unix_now()).await {
            Ok(jobs) => jobs,
            Err(e) => {
                warn!(error = %e, "Could not expire translation callbacks");
                return;
            }
        };
        let wanted = self.settings().subtitles.wanted();
        for job in jobs {
            let Some((item, step)) = self.job_owner(&job).await else {
                continue;
            };
            if step.state != StepState::Running {
                continue;
            }
            let n = self.attempt_number(&job).await;
            let waited = job.expires_at - job.created_at;
            let message = format!("no callback within {waited} s");
            let _ = store
                .end_attempt(job.attempt_id, AttemptResult::Timeout, None, &message)
                .await;
            putio_sync_telemetry::steps::step(
                step.stage.as_str(),
                "translate",
                "timeout",
                Duration::from_secs(waited.max(0) as u64),
            );
            putio_sync_telemetry::translation::done(
                "callback",
                "timeout",
                &crate::translate::language_label(&job.target_lang, &wanted),
                Duration::from_secs(waited.max(0) as u64),
            );
            warn!(
                key = %item.key, job = %job.job_id, language = %job.target_lang,
                "translation callback did not arrive in time"
            );
            self.give_up(&item, &step, n, message).await;
            self.inner.wake.notify_one();
        }
        self.refresh_callback_gauge().await;
    }

    /// The step of these jobs will not be answered (the item was cancelled, the step was
    /// stopped): the jobs end and the attempts are closed as interrupted
    pub(crate) async fn abandon_jobs(&self, jobs: Vec<CallbackJob>) {
        let store = &self.inner.store;
        for job in jobs {
            let _ = store
                .end_attempt(job.attempt_id, AttemptResult::Interrupted, None, "")
                .await;
            if let Ok(Some(step)) = store.step(job.step_id).await {
                if step.state == StepState::Running {
                    let _ = store
                        .set_step(step.id, StepState::Interrupted, None, None)
                        .await;
                }
            }
        }
        self.refresh_callback_gauge().await;
    }

    pub(crate) async fn refresh_callback_gauge(&self) {
        let n = self
            .inner
            .store
            .waiting_jobs()
            .await
            .map(|j| j.len())
            .unwrap_or(0);
        putio_sync_telemetry::translation::set_callbacks_waiting(n);
    }

    async fn job_owner(&self, job: &CallbackJob) -> Option<(ItemRow, StepRow)> {
        let store = &self.inner.store;
        let step = store.step(job.step_id).await.ok()??;
        let item = store.item(&step.item_key).await.ok()??;
        Some((item, step))
    }

    async fn attempt_number(&self, job: &CallbackJob) -> i64 {
        self.inner
            .store
            .attempts(job.step_id)
            .await
            .unwrap_or_default()
            .iter()
            .find(|a| a.id == job.attempt_id)
            .map(|a| a.n)
            .unwrap_or(1)
    }

    fn scrub_service(&self, text: &str) -> String {
        let config = self.inner.translator.config();
        let secrets: Vec<String> = config.callback.headers.values().cloned().collect();
        callback::scrub(text, &secrets, callback::MAX_ERROR_CHARS)
    }

    /// The claimed job could not be taken: its step fails like any failed attempt (with the
    /// pipeline's retries, which send a new job)
    async fn fail_job(
        &self,
        item: &ItemRow,
        step: &StepRow,
        job: &CallbackJob,
        n: i64,
        took: Duration,
        message: String,
    ) {
        let store = &self.inner.store;
        let _ = store
            .finish_job(&job.token_hash, "failed", unix_now())
            .await;
        let wanted = self.settings().subtitles.wanted();
        putio_sync_telemetry::translation::done(
            "callback",
            "failed",
            &crate::translate::language_label(&job.target_lang, &wanted),
            took,
        );
        warn!(
            key = %item.key, job = %job.job_id, language = %job.target_lang,
            error = %message, "translation from the callback service failed"
        );
        self.conclude(
            item,
            step,
            Ended {
                attempt_id: job.attempt_id,
                n,
                kind: "translate",
                result: AttemptResult::Failed,
                code: None,
                output: message.clone(),
                error: Some(message),
                took,
            },
        )
        .await;
        self.inner.wake.notify_one();
    }

    /// The subtitle that was sent, if it is still the same file
    async fn reread_source(&self, job: &CallbackJob) -> Result<Vec<srt::Cue>, String> {
        let path = job.source_path.clone();
        let bytes = tokio::task::spawn_blocking(move || std::fs::read(path))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|_| "the source subtitle is gone".to_string())?;
        if callback::sha256_hex(&bytes) != job.source_sha256 {
            return Err("the source subtitle changed while it was being translated".to_string());
        }
        let text = srt::decode_bytes(&bytes, &job.source_lang)
            .map_err(|e| format!("the source subtitle is not readable: {e}"))?;
        Ok(srt::parse(&text))
    }
}
