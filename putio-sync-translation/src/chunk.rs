//! The chunk driver (research R4, R5): split the translatable cues into chunks, send them
//! concurrently, validate every reply and, when a chunk keeps failing, retry it, then split it
//! in half, to a depth of 3. Results are placed by cue number, so reply order never matters.
//!
//! The driver does not talk HTTP. It calls a [`ChatCall`], which the chat client implements
//! (and which also owns the in-flight limit) and tests replace with a fake.

use std::ops::Range;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use putio_sync_subtitles::BoxFut;
use tokio::task::JoinSet;

use crate::prompt::{self, Numbered, ReplyError};

/// Source cues of context before each piece
pub const CONTEXT_CUES: usize = 3;
/// Deepest split: 40, 20, 10, 5
pub const MAX_DEPTH: u8 = 3;
/// Tries of one piece before it is split or fails
pub const ATTEMPTS: u32 = 2;

/// One request: source context (not to be translated) and the lines to translate
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// Up to [`CONTEXT_CUES`] source lines before the first line
    pub context: Vec<Numbered>,
    /// The lines to translate
    pub lines: Vec<Numbered>,
}

/// Errors that retrying cannot fix: the translation fails at once
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Fatal {
    /// 401 or 403
    #[error("the server rejected the API key")]
    Key,
    /// 404 naming the model
    #[error("model not found: {0}")]
    ModelNotFound(String),
    /// 404 otherwise
    #[error("{0}")]
    EndpointNotFound(String),
    /// Any other 400
    #[error("the server refused the request: {0}")]
    BadRequest(String),
    /// 429 for longer than the budget
    #[error("rate limited for {0} s")]
    RateLimited(u64),
}

/// A failed attempt: the chunk is tried again, then split
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AttemptError {
    /// The reply did not hold what was asked (also a cut-off reply or a context-length 400)
    #[error("{0}")]
    Invalid(#[from] ReplyError),
    /// 5xx and other answers worth trying again
    #[error("HTTP {status}: {message}")]
    Http {
        /// Status code
        status: u16,
        /// Scrubbed message
        message: String,
    },
    /// No answer in time
    #[error("timeout after {0} s")]
    Timeout(u64),
    /// Connection refused, DNS, reset
    #[error("unreachable: {0}")]
    Unreachable(String),
}

/// What one call can end in
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CallError {
    /// Give up now
    #[error("{0}")]
    Fatal(#[from] Fatal),
    /// Try again, then split
    #[error("{0}")]
    Attempt(#[from] AttemptError),
}

/// A chat server, as the driver sees it. Returns the model's reply text.
pub trait ChatCall: Send + Sync {
    /// Send one request and wait for the reply text
    fn call<'a>(&'a self, request: &'a Request) -> BoxFut<'a, Result<String, CallError>>;
}

/// Why a translation did not finish
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TranslateError {
    /// See [`Fatal`]
    #[error("{0}")]
    Fatal(#[from] Fatal),
    /// A piece failed its attempts at the deepest split
    #[error("{reason} (lines {first} to {last})")]
    Chunk {
        /// The last attempt's reason
        reason: String,
        /// First cue number of the piece
        first: usize,
        /// Last cue number of the piece
        last: usize,
    },
    /// There is nothing usable to translate from
    #[error("{0}")]
    Source(String),
    /// A result was refused
    #[error("{0}")]
    Refused(String),
}

/// Words for the logs
#[derive(Debug, Clone, Default)]
pub struct LogCtx {
    /// Pipeline item key
    pub key: String,
    /// The video, relative path
    pub video: String,
}

/// What the driver reports
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Stats {
    /// Top-level chunks
    pub chunks: u32,
    /// Repeated requests
    pub retries: u32,
    /// Pieces that were split in half
    pub splits: u32,
}

/// The chunk ranges over `count` translatable cues
pub fn plan(count: usize, chunk_cues: usize) -> Vec<Range<usize>> {
    let size = chunk_cues.max(1);
    (0..count)
        .step_by(size)
        .map(|start| start..(start + size).min(count))
        .collect()
}

struct Shared {
    call: Arc<dyn ChatCall>,
    /// Every cue's source text, by position
    texts: Vec<String>,
    /// Positions (0-based) of the cues that are sent
    items: Vec<usize>,
    log: LogCtx,
    retries: AtomicUsize,
    splits: AtomicUsize,
    done: AtomicUsize,
    progress: Box<dyn Fn(usize, usize) + Send + Sync>,
}

impl Shared {
    fn request(&self, range: &Range<usize>) -> Request {
        let first_pos = self.items[range.start];
        let context = (first_pos.saturating_sub(CONTEXT_CUES)..first_pos)
            .map(|p| (p + 1, self.texts[p].clone()))
            .collect();
        let lines = range
            .clone()
            .map(|i| (self.items[i] + 1, self.texts[self.items[i]].clone()))
            .collect();
        Request { context, lines }
    }

    async fn attempt(&self, range: &Range<usize>) -> Result<Vec<String>, CallError> {
        let request = self.request(range);
        let reply = self.call.call(&request).await?;
        let expected: Vec<usize> = request.lines.iter().map(|(n, _)| *n).collect();
        let context: Vec<usize> = request.context.iter().map(|(n, _)| *n).collect();
        let texts = prompt::validate(&expected, &context, prompt::parse_reply(&reply))
            .map_err(AttemptError::Invalid)?;
        Ok(texts)
    }

    fn solve<'a>(
        &'a self,
        range: Range<usize>,
        depth: u8,
    ) -> BoxFut<'a, Result<Vec<String>, TranslateError>> {
        Box::pin(async move {
            let mut last: Option<AttemptError> = None;
            for attempt in 1..=ATTEMPTS {
                match self.attempt(&range).await {
                    Ok(texts) => {
                        if depth == 0 {
                            putio_sync_telemetry::translation::chunk("ok");
                        }
                        let done = self.done.fetch_add(texts.len(), Ordering::SeqCst) + texts.len();
                        (self.progress)(done, self.items.len());
                        return Ok(texts);
                    }
                    Err(CallError::Fatal(f)) => {
                        putio_sync_telemetry::translation::chunk("failed");
                        return Err(TranslateError::Fatal(f));
                    }
                    Err(CallError::Attempt(e)) => {
                        let detail: String = e.to_string().chars().take(200).collect();
                        tracing::debug!(
                            key = %self.log.key,
                            video = %self.log.video,
                            chunk = %format!("{}-{}", self.items[range.start] + 1, self.items[range.end - 1] + 1),
                            attempt,
                            depth,
                            reason = %reason_of(&e),
                            detail = %detail,
                            "translation chunk retry"
                        );
                        if attempt < ATTEMPTS {
                            self.retries.fetch_add(1, Ordering::SeqCst);
                            putio_sync_telemetry::translation::chunk_retry();
                        }
                        last = Some(e);
                    }
                }
            }
            let reason = last.map(|e| e.to_string()).unwrap_or_default();
            if range.len() > 1 && depth < MAX_DEPTH {
                self.splits.fetch_add(1, Ordering::SeqCst);
                putio_sync_telemetry::translation::chunk("split");
                let mid = range.start + range.len() / 2;
                let (a, b) = tokio::join!(
                    self.solve(range.start..mid, depth + 1),
                    self.solve(mid..range.end, depth + 1)
                );
                let mut out = a?;
                out.extend(b?);
                return Ok(out);
            }
            putio_sync_telemetry::translation::chunk("failed");
            Err(TranslateError::Chunk {
                reason,
                first: self.items[range.start] + 1,
                last: self.items[range.end - 1] + 1,
            })
        })
    }
}

/// A short fixed word for the log
fn reason_of(e: &AttemptError) -> &'static str {
    match e {
        AttemptError::Invalid(ReplyError::WrongLineCount { .. }) => "line_count",
        AttemptError::Invalid(ReplyError::BadNumbering { .. }) => "numbering",
        AttemptError::Invalid(ReplyError::EmptyText { .. }) => "empty_text",
        AttemptError::Invalid(ReplyError::Truncated) => "truncated",
        AttemptError::Invalid(ReplyError::NoContent) => "no_content",
        AttemptError::Http { .. } => "http",
        AttemptError::Timeout(_) => "timeout",
        AttemptError::Unreachable(_) => "unreachable",
    }
}

/// What [`translate_chunks`] gives back
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Translated {
    /// The translation of each item, in item order
    pub texts: Vec<String>,
    /// How it went
    pub stats: Stats,
}

/// Translate the cues at `items` (0-based positions in `texts`, ascending) in chunks of
/// `chunk_cues`, all chunks at once (the call limits how many requests run together).
/// `progress(done, total)` is called after each piece that succeeds.
pub async fn translate_chunks(
    call: Arc<dyn ChatCall>,
    texts: Vec<String>,
    items: Vec<usize>,
    chunk_cues: usize,
    log: LogCtx,
    progress: Box<dyn Fn(usize, usize) + Send + Sync>,
) -> Result<Translated, TranslateError> {
    let ranges = plan(items.len(), chunk_cues);
    let shared = Arc::new(Shared {
        call,
        texts,
        items,
        log,
        retries: AtomicUsize::new(0),
        splits: AtomicUsize::new(0),
        done: AtomicUsize::new(0),
        progress,
    });
    let mut set = JoinSet::new();
    for (index, range) in ranges.iter().cloned().enumerate() {
        let shared = shared.clone();
        set.spawn(async move { (index, shared.solve(range, 0).await) });
    }
    let mut parts: Vec<Option<Vec<String>>> = vec![None; ranges.len()];
    let mut failure: Option<TranslateError> = None;
    while let Some(joined) = set.join_next().await {
        match joined {
            Ok((index, Ok(texts))) => parts[index] = Some(texts),
            Ok((_, Err(e))) => {
                // a fatal error beats a chunk failure: it is the thing to fix
                let better = match (&failure, &e) {
                    (None, _) => true,
                    (Some(TranslateError::Chunk { .. }), TranslateError::Fatal(_)) => true,
                    _ => false,
                };
                if better {
                    failure = Some(e);
                }
                set.abort_all();
            }
            Err(e) if e.is_cancelled() => {}
            Err(e) => {
                failure.get_or_insert(TranslateError::Refused(format!("internal: {e}")));
                set.abort_all();
            }
        }
    }
    if let Some(e) = failure {
        return Err(e);
    }
    let texts: Vec<String> = parts.into_iter().flatten().flatten().collect();
    Ok(Translated {
        texts,
        stats: Stats {
            chunks: ranges.len() as u32,
            retries: shared.retries.load(Ordering::SeqCst) as u32,
            splits: shared.splits.load(Ordering::SeqCst) as u32,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Translates every line to `T<text>`; the script can break chosen calls
    struct Fake {
        calls: Mutex<Vec<Request>>,
        /// (call index) -> reply override
        script: Box<dyn Fn(usize, &Request) -> Option<Result<String, CallError>> + Send + Sync>,
        in_flight: AtomicUsize,
        max_in_flight: AtomicUsize,
    }

    impl Fake {
        fn new(
            script: impl Fn(usize, &Request) -> Option<Result<String, CallError>>
                + Send
                + Sync
                + 'static,
        ) -> Arc<Self> {
            Arc::new(Self {
                calls: Mutex::new(Vec::new()),
                script: Box::new(script),
                in_flight: AtomicUsize::new(0),
                max_in_flight: AtomicUsize::new(0),
            })
        }
        fn count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }
    }

    fn echo(request: &Request) -> String {
        request
            .lines
            .iter()
            .map(|(n, t)| format!("{n}| T{}", prompt::encode_text(t)))
            .collect::<Vec<_>>()
            .join("\n")
    }

    impl ChatCall for Fake {
        fn call<'a>(&'a self, request: &'a Request) -> BoxFut<'a, Result<String, CallError>> {
            Box::pin(async move {
                let index = {
                    let mut calls = self.calls.lock().unwrap();
                    calls.push(request.clone());
                    calls.len() - 1
                };
                let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
                self.max_in_flight.fetch_max(now, Ordering::SeqCst);
                tokio::task::yield_now().await;
                let out = (self.script)(index, request).unwrap_or_else(|| Ok(echo(request)));
                self.in_flight.fetch_sub(1, Ordering::SeqCst);
                out
            })
        }
    }

    fn texts(n: usize) -> Vec<String> {
        (1..=n).map(|i| format!("line {i}")).collect()
    }

    async fn run(fake: Arc<Fake>, n: usize, chunk: usize) -> Result<Translated, TranslateError> {
        translate_chunks(
            fake,
            texts(n),
            (0..n).collect(),
            chunk,
            LogCtx::default(),
            Box::new(|_, _| {}),
        )
        .await
    }

    #[test]
    fn the_plan_cuts_chunks_of_the_given_size() {
        assert_eq!(plan(0, 40), Vec::<Range<usize>>::new());
        assert_eq!(plan(40, 40), vec![0..40]);
        assert_eq!(plan(41, 40), vec![0..40, 40..41]);
        assert_eq!(plan(100, 40), vec![0..40, 40..80, 80..100]);
        assert_eq!(plan(3, 0), vec![0..1, 1..2, 2..3]);
    }

    #[tokio::test]
    async fn chunks_carry_three_cues_of_source_context() {
        let fake = Fake::new(|_, _| None);
        let out = run(fake.clone(), 10, 4).await.unwrap();
        assert_eq!(out.texts.len(), 10);
        assert_eq!(out.texts[0], "Tline 1");
        assert_eq!(out.texts[9], "Tline 10");
        assert_eq!(
            out.stats,
            Stats {
                chunks: 3,
                retries: 0,
                splits: 0
            }
        );
        let mut calls = fake.calls.lock().unwrap().clone();
        calls.sort_by_key(|r| r.lines[0].0);
        assert!(calls[0].context.is_empty());
        assert_eq!(calls[1].lines[0].0, 5);
        assert_eq!(
            calls[1].context.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
            vec![2, 3, 4]
        );
        // context is the source text, never a translation
        assert_eq!(calls[1].context[0].1, "line 2");
        assert_eq!(calls[2].context.last().unwrap().0, 8);
    }

    #[tokio::test]
    async fn context_can_hold_cues_that_are_not_sent() {
        // cue 2 is a music note and is not sent
        let fake = Fake::new(|_, _| None);
        let mut t = texts(5);
        t[1] = "♪".to_string();
        let items = vec![0, 2, 3, 4];
        let out = translate_chunks(
            fake.clone(),
            t,
            items,
            2,
            LogCtx::default(),
            Box::new(|_, _| {}),
        )
        .await
        .unwrap();
        assert_eq!(out.texts, vec!["Tline 1", "Tline 3", "Tline 4", "Tline 5"]);
        let calls = fake.calls.lock().unwrap().clone();
        let first = calls.iter().find(|r| r.lines[0].0 == 1).unwrap();
        assert_eq!(
            first.lines.iter().map(|l| l.0).collect::<Vec<_>>(),
            vec![1, 3]
        );
        let second = calls.iter().find(|r| r.lines[0].0 == 4).unwrap();
        assert_eq!(
            second
                .context
                .iter()
                .map(|c| c.1.clone())
                .collect::<Vec<_>>(),
            vec!["line 1", "♪", "line 3"]
        );
    }

    #[tokio::test]
    async fn a_bad_first_attempt_is_retried_with_the_same_chunk() {
        let fake = Fake::new(|i, _| (i == 0).then(|| Ok("1| only one".to_string())));
        let out = run(fake.clone(), 4, 4).await.unwrap();
        assert_eq!(out.texts.len(), 4);
        assert_eq!(fake.count(), 2);
        assert_eq!(out.stats.retries, 1);
        assert_eq!(out.stats.splits, 0);
        let calls = fake.calls.lock().unwrap();
        assert_eq!(calls[0], calls[1]);
    }

    #[tokio::test]
    async fn two_failures_split_the_chunk_in_half() {
        // the first two calls (the whole chunk) are wrong, then everything works
        let fake = Fake::new(|i, _| (i < 2).then(|| Ok("garbage".to_string())));
        let out = run(fake.clone(), 8, 8).await.unwrap();
        assert_eq!(out.texts.len(), 8);
        assert_eq!(out.stats.splits, 1);
        assert_eq!(fake.count(), 4);
        let calls = fake.calls.lock().unwrap();
        let mut sizes: Vec<usize> = calls[2..].iter().map(|r| r.lines.len()).collect();
        sizes.sort();
        assert_eq!(sizes, vec![4, 4]);
    }

    #[tokio::test]
    async fn splitting_stops_at_depth_three_and_the_translation_fails() {
        let fake = Fake::new(|_, _| Some(Ok("nope".to_string())));
        let err = run(fake.clone(), 40, 40).await.unwrap_err();
        assert!(matches!(err, TranslateError::Chunk { .. }), "{err:?}");
        // 40 -> 20 -> 10 -> 5: at most 30 requests for the chunk
        assert!(fake.count() <= 30, "{}", fake.count());
        let calls = fake.calls.lock().unwrap();
        assert!(calls.iter().all(|r| r.lines.len() >= 5));
    }

    #[tokio::test]
    async fn a_failing_chunk_is_retried_and_split_before_it_fails() {
        let fake = Fake::new(|_, _| Some(Ok("nope".to_string())));
        // the error ends the join early on the first failing piece, so count what a full
        // failing tree costs by letting every piece fail with no early exit: use 40 cues
        let _ = run(fake.clone(), 40, 40).await;
        assert!(fake.count() >= 2 + 2 + 2 + 2, "{}", fake.count());
    }

    #[tokio::test]
    async fn a_single_cue_that_fails_twice_fails_the_translation() {
        let fake = Fake::new(|_, _| Some(Ok("".to_string())));
        let err = run(fake.clone(), 1, 40).await.unwrap_err();
        assert_eq!(fake.count(), 2);
        match err {
            TranslateError::Chunk { first, last, .. } => assert_eq!((first, last), (1, 1)),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn transport_failures_count_as_failed_attempts() {
        let fake = Fake::new(|i, _| {
            (i == 0).then(|| {
                Err(CallError::Attempt(AttemptError::Http {
                    status: 503,
                    message: "busy".into(),
                }))
            })
        });
        let out = run(fake.clone(), 3, 10).await.unwrap();
        assert_eq!(out.stats.retries, 1);
        assert_eq!(fake.count(), 2);
    }

    #[tokio::test]
    async fn fatal_errors_skip_every_retry() {
        for fatal in [
            Fatal::Key,
            Fatal::ModelNotFound("qwen".into()),
            Fatal::EndpointNotFound("endpoint not found".into()),
            Fatal::BadRequest("nope".into()),
            Fatal::RateLimited(300),
        ] {
            let f = fatal.clone();
            let fake = Fake::new(move |_, _| Some(Err(CallError::Fatal(f.clone()))));
            let err = run(fake.clone(), 100, 10).await.unwrap_err();
            assert_eq!(err, TranslateError::Fatal(fatal));
            // the other chunks may have started, but nothing was retried or split
            assert!(fake.count() <= 10, "{}", fake.count());
        }
    }

    #[tokio::test]
    async fn results_are_placed_by_cue_number_whatever_the_order_of_replies() {
        // earlier chunks answer later
        let fake = Fake::new(|_, _| None);
        let out = run(fake, 25, 5).await.unwrap();
        let expected: Vec<String> = (1..=25).map(|i| format!("Tline {i}")).collect();
        assert_eq!(out.texts, expected);
    }

    #[tokio::test]
    async fn echoed_context_and_commentary_do_not_break_a_chunk() {
        let fake = Fake::new(|_, r| {
            let mut s = String::from("Sure! Here you go:\n");
            for (n, t) in &r.context {
                s.push_str(&format!("{n}| ctx {t}\n"));
            }
            s.push_str(&echo(r));
            s.push_str("\nLet me know if you need more.");
            Some(Ok(s))
        });
        let out = run(fake.clone(), 6, 3).await.unwrap();
        assert_eq!(out.stats.retries, 0);
        assert_eq!(out.texts[5], "Tline 6");
    }

    #[tokio::test]
    async fn progress_reports_the_share_done() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        let fake = Fake::new(|_, _| None);
        translate_chunks(
            fake,
            texts(10),
            (0..10).collect(),
            5,
            LogCtx::default(),
            Box::new(move |done, total| s.lock().unwrap().push((done, total))),
        )
        .await
        .unwrap();
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen.last(), Some(&(10, 10)));
        assert!(seen.iter().all(|(_, t)| *t == 10));
    }

    #[tokio::test]
    async fn all_chunks_are_started_together() {
        let fake = Fake::new(|_, _| None);
        run(fake.clone(), 50, 5).await.unwrap();
        // the driver itself does not limit; the call does
        assert!(fake.max_in_flight.load(Ordering::SeqCst) > 1);
    }

    #[tokio::test]
    async fn no_cues_give_no_calls() {
        let fake = Fake::new(|_, _| None);
        let out = run(fake.clone(), 0, 40).await.unwrap();
        assert!(out.texts.is_empty());
        assert_eq!(fake.count(), 0);
    }
}
