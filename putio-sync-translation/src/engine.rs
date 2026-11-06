//! Translate one subtitle (research R4, R9): the cues that have words go to the chunk driver,
//! the rest are copied, and the result takes exactly the source's timings.

use std::sync::Arc;

use putio_sync_core::lang;
use putio_sync_subtitles::srt::Cue;

use crate::align;
use crate::chat::{ChatClient, ChatSession};
use crate::chunk::{self, ChatCall, LogCtx, Stats, TranslateError};
use crate::prompt;

/// Progress reports: `(done, total)` translatable cues
pub type Progress = Box<dyn Fn(usize, usize) + Send + Sync>;

/// A finished translation
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Translation {
    /// The cues to save, with the source's numbers and times
    pub cues: Vec<Cue>,
    /// Requests, retries and splits
    pub stats: Stats,
}

/// What to translate and how
pub struct Job<'a> {
    /// The source subtitle
    pub source: &'a [Cue],
    /// Source language code
    pub source_code: &'a str,
    /// Target language code
    pub target_code: &'a str,
    /// The operator's extra instruction
    pub instruction: &'a str,
    /// Cues per request
    pub chunk_cues: usize,
    /// For the logs
    pub log: LogCtx,
}

fn language_name(code: &str) -> String {
    lang::name(code).unwrap_or_else(|| code.to_string())
}

/// Translate through a chat server
pub async fn translate_with_chat(
    client: ChatClient,
    job: Job<'_>,
    progress: Progress,
) -> Result<Translation, TranslateError> {
    let system = prompt::system_prompt(
        &language_name(job.source_code),
        job.source_code,
        &language_name(job.target_code),
        job.target_code,
        job.instruction,
    );
    let call: Arc<dyn ChatCall> = Arc::new(ChatSession { client, system });
    translate_with_call(call, job, progress).await
}

/// Translate through any [`ChatCall`]
pub async fn translate_with_call(
    call: Arc<dyn ChatCall>,
    job: Job<'_>,
    progress: Progress,
) -> Result<Translation, TranslateError> {
    if job.source.is_empty() {
        return Err(TranslateError::Source(
            "the source subtitle has no cues".to_string(),
        ));
    }
    let texts: Vec<String> = job.source.iter().map(|c| c.text.clone()).collect();
    let items: Vec<usize> = texts
        .iter()
        .enumerate()
        .filter(|(_, t)| prompt::is_translatable(t))
        .map(|(i, _)| i)
        .collect();
    let translated = if items.is_empty() {
        chunk::Translated {
            texts: Vec::new(),
            stats: Stats::default(),
        }
    } else {
        chunk::translate_chunks(
            call,
            texts.clone(),
            items.clone(),
            job.chunk_cues,
            job.log,
            progress,
        )
        .await?
    };
    let mut cues: Vec<Cue> = job.source.to_vec();
    for (position, text) in items.iter().zip(translated.texts) {
        cues[*position].text = prompt::restore_wrapping(&texts[*position], &text);
    }
    align::verify_same_timing(job.source, &cues)
        .map_err(|e| TranslateError::Refused(e.to_string()))?;
    Ok(Translation {
        cues,
        stats: translated.stats,
    })
}

/// A subtitle that came back from a callback service (research R6, R9): parsed, held to the
/// source's cue count and given exactly the source's times. The count of cues whose times
/// the service changed is `realigned`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    /// The cues to save
    pub cues: Vec<Cue>,
    /// Cues that had other times in the result than in the source
    pub realigned: usize,
}

/// Check a callback service's SRT or VTT text against the `source` cues
pub fn check_result(source: &[Cue], text: &str) -> Result<Checked, TranslateError> {
    let result = putio_sync_subtitles::srt::parse(text);
    let aligned =
        align::realign(source, &result).map_err(|e| TranslateError::Refused(e.to_string()))?;
    align::verify_same_timing(source, &aligned.cues)
        .map_err(|e| TranslateError::Refused(e.to_string()))?;
    Ok(Checked {
        cues: aligned.cues,
        realigned: aligned.realigned,
    })
}
