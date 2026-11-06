//! The file name level of the OpenSubtitles pass (feature 050): used when the fingerprint and
//! ids give nothing, scored by title and release. Fake search; no network.

mod common;

use common::*;
use putio_sync_core::media::MediaLookup;
use putio_sync_core::subtitles::SubtitleMode;
use putio_sync_subtitles::engine::{self, Outcome, SkipReason, VideoRequest};
use putio_sync_subtitles::matching::Matching;
use putio_sync_subtitles::modes::ModePlan;
use putio_sync_subtitles::NoSource;

async fn run(name: &str, search: &FakeSearch, min: u8) -> (Outcome, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let v = video(dir.path(), name);
    let langs = vec!["en".to_string()];
    let modes = ModePlan::uniform(&langs, SubtitleMode::DownloadOnly);
    let none: Option<&dyn MediaLookup> = None;
    let m = Matching {
        search,
        media: none,
        category: None,
        min_confidence: min,
    };
    // No Put.io file id: this is the manual path, where only the name is known
    let req = VideoRequest::new(&v, None, &langs, &modes).with_matching(m);
    (engine::fetch_for_video(&NoSource, &req).await.unwrap(), dir)
}

fn named(file_id: i64, title: &str, release: &str) -> putio_sync_subtitles::opensubtitles::Hit {
    let mut h = hit(file_id, "en", release);
    h.title = title.to_string();
    h.year = Some(2024);
    h
}

#[tokio::test]
async fn a_movie_is_searched_by_title_and_year() {
    let search = FakeSearch::new(|p| {
        Ok(if has(p, "query").is_some() {
            vec![named(1, "Movie", "Movie.2024.1080p.WEB-DL.x264-GRP")]
        } else {
            vec![]
        })
    });
    let (out, dir) = run("Movie.2024.1080p.WEB-DL.x264-GRP.mkv", &search, 65).await;
    assert_eq!(out.saved[0].source, "os_name");
    assert!(dir
        .path()
        .join("Movie.2024.1080p.WEB-DL.x264-GRP.en.srt")
        .exists());
    let q = search.searches().pop().unwrap();
    assert_eq!(has(&q, "query").as_deref(), Some("movie"));
    assert_eq!(has(&q, "type").as_deref(), Some("movie"));
    assert_eq!(has(&q, "year").as_deref(), Some("2024"));
}

#[tokio::test]
async fn an_episode_is_searched_by_season_and_episode() {
    let search = FakeSearch::hits(vec![]);
    run("Show.Name.S02E05.1080p.WEB-DL.x264-GRP.mkv", &search, 65).await;
    let q = search.searches().pop().unwrap();
    assert_eq!(has(&q, "query").as_deref(), Some("show name"));
    assert_eq!(has(&q, "type").as_deref(), Some("episode"));
    assert_eq!(has(&q, "season_number").as_deref(), Some("2"));
    assert_eq!(has(&q, "episode_number").as_deref(), Some("5"));
}

#[tokio::test]
async fn the_name_level_never_reaches_a_fingerprint_confidence() {
    let search = FakeSearch::hits(vec![named(1, "Movie", "Movie.2024.1080p.WEB-DL.x264-GRP")]);
    let (out, _dir) = run("Movie.2024.1080p.WEB-DL.x264-GRP.mkv", &search, 65).await;
    let detail = &out.saved[0].detail;
    let n: u32 = detail
        .split(|c: char| !c.is_ascii_digit())
        .find(|s| !s.is_empty())
        .unwrap()
        .parse()
        .unwrap();
    assert!(n <= 90, "{detail}");
}

#[tokio::test]
async fn a_different_title_is_not_saved() {
    let search = FakeSearch::hits(vec![named(
        1,
        "Completely Different Film",
        "Completely.Different.Film.2024.1080p.WEB-DL.x264-GRP",
    )]);
    let (out, _dir) = run("Movie.2024.1080p.WEB-DL.x264-GRP.mkv", &search, 65).await;
    assert!(out.saved.is_empty());
    assert!(search.downloads.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_weak_release_match_falls_below_the_threshold() {
    let search = FakeSearch::hits(vec![named(1, "Movie", "Movie.2024.720p.HDTV.xvid-OTHER")]);
    let (out, _dir) = run("Movie.2024.1080p.WEB-DL.x264-GRP.mkv", &search, 85).await;
    assert!(out.saved.is_empty());
    assert!(matches!(
        out.skipped[0].reason,
        SkipReason::BelowThreshold(_)
    ));
}
