//! A subtitle that comes back from a callback service is checked against the source before
//! anything is saved (feature 026, research R9).

use putio_sync_subtitles::srt::{self, Cue};
use putio_sync_translation::chunk::TranslateError;
use putio_sync_translation::engine::check_result;

fn source() -> Vec<Cue> {
    srt::parse(
        "1\n00:00:01,000 --> 00:00:02,500\nHello.\n\n2\n00:00:03,000 --> 00:00:04,000\nBye now.\n\n3\n00:00:05,000 --> 00:00:06,000\n[music]\n",
    )
}

#[test]
fn equal_times_pass_unchanged() {
    let text = "1\n00:00:01,000 --> 00:00:02,500\nHola.\n\n2\n00:00:03,000 --> 00:00:04,000\nAdios.\n\n3\n00:00:05,000 --> 00:00:06,000\n[musica]\n";
    let checked = check_result(&source(), text).unwrap();
    assert_eq!(checked.realigned, 0);
    assert_eq!(checked.cues.len(), 3);
    assert_eq!(checked.cues[0].text, "Hola.");
}

#[test]
fn other_times_with_the_same_count_take_the_sources_times() {
    let text = "1\n00:00:01,200 --> 00:00:02,700\nHola.\n\n2\n00:00:03,100 --> 00:00:04,100\nAdios.\n\n3\n00:00:05,000 --> 00:00:06,000\n[musica]\n";
    let checked = check_result(&source(), text).unwrap();
    assert_eq!(checked.realigned, 2);
    for (c, s) in checked.cues.iter().zip(source()) {
        assert_eq!((c.start_ms, c.end_ms), (s.start_ms, s.end_ms));
    }
    assert_eq!(checked.cues[1].text, "Adios.");
}

#[test]
fn a_different_count_is_refused_with_both_counts() {
    let text = "1\n00:00:01,000 --> 00:00:02,500\nHola.\n";
    match check_result(&source(), text) {
        Err(TranslateError::Refused(m)) => {
            assert_eq!(m, "result has 1 cues, the source has 3");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn text_that_is_not_a_subtitle_is_refused() {
    for text in ["", "just some words", "{\"x\": 1}"] {
        assert!(
            matches!(
                check_result(&source(), text),
                Err(TranslateError::Refused(_))
            ),
            "{text}"
        );
    }
}

#[test]
fn a_vtt_result_is_accepted() {
    let text = "WEBVTT\n\n00:00:01.000 --> 00:00:02.500\nHola.\n\n00:00:03.000 --> 00:00:04.000\nAdios.\n\n00:00:05.000 --> 00:00:06.000\n[musica]\n";
    let checked = check_result(&source(), text).unwrap();
    assert_eq!(checked.cues.len(), 3);
}
