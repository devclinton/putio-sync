//! Translating a whole subtitle through a mock chat server (feature 026, SC-001, R3 to R5).

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::*;
use putio_sync_subtitles::srt::Cue;
use putio_sync_translation::chat::ChatClient;
use putio_sync_translation::chunk::{LogCtx, TranslateError};
use putio_sync_translation::engine::{translate_with_chat, Job, Translation};
use tokio::sync::Semaphore;

fn cues(n: usize) -> Vec<Cue> {
    (0..n)
        .map(|i| Cue {
            start_ms: i as u64 * 2000 + 17,
            end_ms: i as u64 * 2000 + 1500,
            text: format!("Line number {}", i + 1),
        })
        .collect()
}

async fn run(
    mock: &Mock,
    source: &[Cue],
    permits: usize,
    progress: Box<dyn Fn(usize, usize) + Send + Sync>,
) -> Result<Translation, TranslateError> {
    let client =
        ChatClient::new(&mock.url, "m", "", 10, Arc::new(Semaphore::new(permits))).unwrap();
    translate_with_chat(
        client,
        Job {
            source,
            source_code: "en",
            target_code: "es",
            instruction: "",
            chunk_cues: 40,
            log: LogCtx::default(),
        },
        progress,
    )
    .await
}

fn quiet() -> Box<dyn Fn(usize, usize) + Send + Sync> {
    Box::new(|_, _| {})
}

#[tokio::test]
async fn three_hundred_cues_keep_numbers_and_timestamps_in_several_requests() {
    let mock = serve(Duration::ZERO, echo("ES ")).await;
    let source = cues(300);
    let out = run(&mock, &source, 2, quiet()).await.unwrap();
    assert_eq!(out.cues.len(), 300);
    for (s, o) in source.iter().zip(&out.cues) {
        assert_eq!((s.start_ms, s.end_ms), (o.start_ms, o.end_ms));
        assert_eq!(o.text, format!("ES {}", s.text));
    }
    assert!(mock.count() >= 8, "{} requests", mock.count());
    // later chunks carry source context that is not translated again
    let later = mock
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|u| u.starts_with("Context, for reference only"));
    assert!(later);
    assert_eq!(out.stats.chunks, 8);
}

#[tokio::test]
async fn a_wrong_line_count_is_retried_then_split_until_it_works() {
    // Any request for more than 10 lines comes back one line short
    let mock = serve(Duration::ZERO, |user, _| {
        let lines = lines_to_translate(user);
        let keep = if lines.len() > 10 {
            lines.len() - 1
        } else {
            lines.len()
        };
        let out: Vec<String> = lines[..keep]
            .iter()
            .map(|(n, t)| format!("{n}| ES {t}"))
            .collect();
        reply(&out.join("\n"))
    })
    .await;
    let source = cues(40);
    let out = run(&mock, &source, 2, quiet()).await.unwrap();
    assert_eq!(out.cues.len(), 40);
    assert!(out.cues.iter().all(|c| c.text.starts_with("ES ")));
    assert!(
        out.stats.splits >= 1 && out.stats.retries >= 1,
        "{:?}",
        out.stats
    );
}

#[tokio::test]
async fn a_reply_that_is_never_right_fails_and_names_the_lines() {
    let mock = serve(Duration::ZERO, |_, _| reply("I cannot help with that.")).await;
    let err = run(&mock, &cues(5), 2, quiet()).await.unwrap_err();
    match err {
        TranslateError::Chunk { first, last, .. } => {
            assert!(first >= 1 && last <= 5);
        }
        other => panic!("{other:?}"),
    }
    // 2 attempts per piece, split to the end: bounded, not endless
    assert!(mock.count() <= 30, "{} requests", mock.count());
}

#[tokio::test]
async fn commentary_around_the_lines_is_ignored() {
    let mock = serve(Duration::ZERO, |user, _| {
        let out: Vec<String> = lines_to_translate(user)
            .iter()
            .map(|(n, t)| format!("{n}| ES {t}"))
            .collect();
        reply(&format!(
            "Here is the translation:\n{}\nHope this helps!",
            out.join("\n")
        ))
    })
    .await;
    let out = run(&mock, &cues(3), 2, quiet()).await.unwrap();
    assert_eq!(out.cues[2].text, "ES Line number 3");
}

#[tokio::test]
async fn cues_without_letters_are_copied_and_never_sent() {
    let mut source = cues(4);
    source[1].text = "♪".into();
    source[2].text = "...".into();
    let mock = serve(Duration::ZERO, echo("ES ")).await;
    let out = run(&mock, &source, 2, quiet()).await.unwrap();
    assert_eq!(out.cues[1].text, "♪");
    assert_eq!(out.cues[2].text, "...");
    let sent = mock.requests.lock().unwrap().join("\n");
    assert!(!sent.contains("2| ♪") && !sent.contains("3| ..."));
}

#[tokio::test]
async fn wrapping_tags_the_model_dropped_come_back_and_breaks_survive() {
    let mut source = cues(2);
    source[0].text = "<i>Hello there</i>".into();
    source[1].text = "First part\nSecond part".into();
    let mock = serve(Duration::ZERO, |user, _| {
        // the model drops the italics and keeps <br>
        let out: Vec<String> = lines_to_translate(user)
            .iter()
            .map(|(n, t)| format!("{n}| ES {}", t.replace("<i>", "").replace("</i>", "")))
            .collect();
        reply(&out.join("\n"))
    })
    .await;
    let out = run(&mock, &source, 2, quiet()).await.unwrap();
    assert_eq!(out.cues[0].text, "<i>ES Hello there</i>");
    assert_eq!(out.cues[1].text, "ES First part\nSecond part");
}

#[tokio::test]
async fn right_to_left_and_utf8_text_pass_through() {
    let mock = serve(Duration::ZERO, |user, _| {
        let out: Vec<String> = lines_to_translate(user)
            .iter()
            .map(|(n, _)| format!("{n}| مرحبا بالعالم ñ 你好"))
            .collect();
        reply(&out.join("\n"))
    })
    .await;
    let out = run(&mock, &cues(2), 2, quiet()).await.unwrap();
    assert_eq!(out.cues[0].text, "مرحبا بالعالم ñ 你好");
}

#[tokio::test]
async fn at_most_max_in_flight_requests_run_at_once() {
    let mock = serve(Duration::from_millis(80), echo("ES ")).await;
    run(&mock, &cues(240), 2, quiet()).await.unwrap();
    assert_eq!(mock.peak(), 2, "peak {}", mock.peak());
}

#[tokio::test]
async fn progress_reports_the_share_done() {
    let mock = serve(Duration::ZERO, echo("ES ")).await;
    let seen = Arc::new(Mutex::new(Vec::new()));
    let last = Arc::new(AtomicUsize::new(0));
    let (s, l) = (seen.clone(), last.clone());
    run(
        &mock,
        &cues(100),
        2,
        Box::new(move |done, total| {
            s.lock().unwrap().push((done, total));
            l.fetch_max(done, Ordering::SeqCst);
        }),
    )
    .await
    .unwrap();
    assert!(seen.lock().unwrap().iter().all(|(_, t)| *t == 100));
    assert_eq!(last.load(Ordering::SeqCst), 100);
}
