//! The OpenSubtitles pass of the engine (feature 050): fingerprint, ids, scores, the
//! threshold, quota and failures. A fake search service and a fake *arr; no network.

mod common;

use common::*;
use putio_sync_core::media::{MediaInfo, MediaKind};
use putio_sync_subtitles::engine::{self, EngineError, Outcome, SkipReason, VideoRequest};
use putio_sync_subtitles::matching::Matching;
use putio_sync_subtitles::modes::ModePlan;
use putio_sync_subtitles::opensubtitles::SearchError;
use putio_sync_subtitles::NoSource;

const MOVIE: &str = "Movie.2024.1080p.WEB-DL.x264-GRP.mkv";
const SAME_RELEASE: &str = "Movie.2024.1080p.WEB-DL.x264-GRP";

async fn run(
    dir: &std::path::Path,
    name: &str,
    search: &FakeSearch,
    media: Option<&FakeMedia>,
    langs: &[&str],
    min: u8,
) -> Result<Outcome, EngineError> {
    let v = video(dir, name);
    let langs: Vec<String> = langs.iter().map(|l| l.to_string()).collect();
    let modes = ModePlan::uniform(
        &langs,
        putio_sync_core::subtitles::SubtitleMode::DownloadOnly,
    );
    let m = Matching {
        search,
        media: media.map(|m| m as &dyn putio_sync_core::media::MediaLookup),
        category: Some("radarr"),
        min_confidence: min,
    };
    let req = VideoRequest::new(&v, Some(9), &langs, &modes).with_matching(m);
    engine::fetch_for_video(&NoSource, &req).await
}

fn movie_info() -> MediaInfo {
    MediaInfo {
        kind: Some(MediaKind::Movie),
        imdb_id: Some("903747".into()),
        ..Default::default()
    }
}

#[tokio::test]
async fn a_fingerprint_match_saves_at_full_confidence_and_stops() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = hit(5, "en", "Something.Else");
    h.moviehash_match = true;
    let search = FakeSearch::hits(vec![h]);
    let media = FakeMedia(Some(movie_info()));
    let out = run(dir.path(), MOVIE, &search, Some(&media), &["en"], 65)
        .await
        .unwrap();
    assert_eq!(out.saved.len(), 1);
    assert_eq!(out.saved[0].source, "os_hash");
    assert!(dir
        .path()
        .join("Movie.2024.1080p.WEB-DL.x264-GRP.en.srt")
        .exists());
    let calls = search.searches();
    assert_eq!(calls.len(), 1, "later levels are not searched");
    assert!(has(&calls[0], "moviehash").is_some());
    assert_eq!(has(&calls[0], "languages").as_deref(), Some("en"));
    assert_eq!(
        has(&calls[0], "machine_translated").as_deref(),
        Some("exclude")
    );
    assert_eq!(
        has(&calls[0], "hearing_impaired").as_deref(),
        Some("exclude")
    );
}

#[tokio::test]
async fn ids_are_searched_when_the_fingerprint_finds_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let search = FakeSearch::new(|p| {
        Ok(if has(p, "imdb_id").is_some() {
            vec![hit(6, "en", SAME_RELEASE)]
        } else {
            vec![]
        })
    });
    let media = FakeMedia(Some(movie_info()));
    let out = run(dir.path(), MOVIE, &search, Some(&media), &["en"], 65)
        .await
        .unwrap();
    assert_eq!(out.saved[0].source, "os_ids");
    assert!(out.saved[0].detail.contains("100") || !out.saved[0].detail.is_empty());
    let calls = search.searches();
    assert_eq!(calls.len(), 2);
    assert_eq!(has(&calls[1], "imdb_id").as_deref(), Some("903747"));
}

#[tokio::test]
async fn tmdb_is_used_without_an_imdb_id_and_episodes_use_the_series_ids() {
    let dir = tempfile::tempdir().unwrap();
    let search = FakeSearch::hits(vec![]);
    let info = MediaInfo {
        kind: Some(MediaKind::Episode),
        tmdb_id: Some(1396),
        episodes: vec![(2, 5)],
        ..Default::default()
    };
    let media = FakeMedia(Some(info));
    run(
        dir.path(),
        "Show.Name.S02E05.1080p.WEB-DL.x264-GRP.mkv",
        &search,
        Some(&media),
        &["en"],
        65,
    )
    .await
    .unwrap();
    let calls = search.searches();
    let ids = &calls[1];
    assert_eq!(has(ids, "parent_tmdb_id").as_deref(), Some("1396"));
    assert_eq!(has(ids, "season_number").as_deref(), Some("2"));
    assert_eq!(has(ids, "episode_number").as_deref(), Some("5"));
}

#[tokio::test]
async fn a_result_under_the_threshold_is_not_saved_and_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    // Ids match but nothing about the release does: 60
    let search = FakeSearch::new(|p| {
        Ok(if has(p, "imdb_id").is_some() {
            vec![hit(6, "en", "Other.720p.HDTV.xvid-NOPE")]
        } else {
            vec![]
        })
    });
    let media = FakeMedia(Some(movie_info()));
    let out = run(dir.path(), MOVIE, &search, Some(&media), &["en"], 65)
        .await
        .unwrap();
    assert!(out.saved.is_empty());
    assert_eq!(out.missing, ["en"] as [&str; 1]);
    assert!(matches!(
        out.skipped[0].reason,
        SkipReason::BelowThreshold(s) if s >= 60 && s < 65
    ));
    assert!(
        out.message().contains("below threshold"),
        "{}",
        out.message()
    );
    // The same result passes a lower threshold
    let out = run(dir.path(), MOVIE, &search, Some(&media), &["en"], 50)
        .await
        .unwrap();
    assert_eq!(out.saved.len(), 1);
}

#[tokio::test]
async fn machine_made_and_hearing_impaired_results_are_never_saved() {
    let dir = tempfile::tempdir().unwrap();
    let mut a = hit(1, "en", SAME_RELEASE);
    a.machine_translated = true;
    let mut b = hit(2, "en", SAME_RELEASE);
    b.hearing_impaired = true;
    let mut c = hit(3, "en", SAME_RELEASE);
    c.foreign_parts_only = true;
    let good = hit(4, "en", SAME_RELEASE);
    let search = FakeSearch::hits(vec![a, b, c, good]);
    let media = FakeMedia(Some(movie_info()));
    let out = run(dir.path(), MOVIE, &search, Some(&media), &["en"], 65)
        .await
        .unwrap();
    assert_eq!(*search.downloads.lock().unwrap(), [4]);
    assert_eq!(out.saved.len(), 1);
}

#[tokio::test]
async fn an_unusable_download_falls_to_the_next_best() {
    let dir = tempfile::tempdir().unwrap();
    let mut first = hit(1, "en", SAME_RELEASE);
    first.downloads = 9000;
    let second = hit(2, "en", "Movie.2024.1080p.WEB-DL.x264-GRP");
    let mut search = FakeSearch::hits(vec![first, second]);
    search.broken = vec![1];
    let media = FakeMedia(Some(movie_info()));
    let out = run(dir.path(), MOVIE, &search, Some(&media), &["en"], 65)
        .await
        .unwrap();
    assert_eq!(*search.downloads.lock().unwrap(), [1, 2]);
    assert_eq!(out.saved.len(), 1);
    assert_eq!(out.warnings.len(), 1);
}

#[tokio::test]
async fn one_search_serves_every_missing_language() {
    let dir = tempfile::tempdir().unwrap();
    let search = FakeSearch::hits(vec![hit(1, "en", SAME_RELEASE), hit(2, "es", SAME_RELEASE)]);
    let media = FakeMedia(Some(movie_info()));
    let out = run(dir.path(), MOVIE, &search, Some(&media), &["es", "en"], 65)
        .await
        .unwrap();
    assert_eq!(out.saved.len(), 2);
    // The hash level found no fingerprint match, so ids ran once for both languages
    let ids: Vec<_> = search
        .searches()
        .into_iter()
        .filter(|p| has(p, "imdb_id").is_some())
        .collect();
    assert_eq!(ids.len(), 1);
    assert_eq!(has(&ids[0], "languages").as_deref(), Some("en,es"));
}

#[tokio::test]
async fn an_existing_subtitle_is_left_alone_and_not_searched_for() {
    let dir = tempfile::tempdir().unwrap();
    let existing = dir.path().join("Movie.2024.1080p.WEB-DL.x264-GRP.en.srt");
    std::fs::write(&existing, "mine").unwrap();
    let search = FakeSearch::hits(vec![hit(1, "en", SAME_RELEASE)]);
    let out = run(dir.path(), MOVIE, &search, None, &["en"], 65)
        .await
        .unwrap();
    assert!(out.saved.is_empty());
    assert!(search.searches().is_empty());
    assert_eq!(std::fs::read_to_string(existing).unwrap(), "mine");
}

#[tokio::test]
async fn a_used_up_quota_searches_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mut search = FakeSearch::hits(vec![hit(1, "en", SAME_RELEASE)]);
    search.exhausted = true;
    let out = run(dir.path(), MOVIE, &search, None, &["en"], 65)
        .await
        .unwrap();
    assert!(search.searches().is_empty());
    assert!(out.quota_used_up);
    assert_eq!(out.skipped[0].reason, SkipReason::Quota);
    assert!(out.message().contains("quota used up"), "{}", out.message());
}

#[tokio::test]
async fn the_quota_running_out_on_download_stops_the_pass() {
    let dir = tempfile::tempdir().unwrap();
    let mut search = FakeSearch::hits(vec![hit(1, "en", SAME_RELEASE)]);
    search.download_error = Some(SearchError::Quota);
    let media = FakeMedia(Some(movie_info()));
    let out = run(dir.path(), MOVIE, &search, Some(&media), &["en"], 65)
        .await
        .unwrap();
    assert!(out.quota_used_up);
    assert_eq!(out.skipped[0].reason, SkipReason::Quota);
}

#[tokio::test]
async fn a_rejected_key_warns_once_and_stops() {
    let dir = tempfile::tempdir().unwrap();
    let search = FakeSearch::new(|_| Err(SearchError::KeyRejected));
    let media = FakeMedia(Some(movie_info()));
    let out = run(dir.path(), MOVIE, &search, Some(&media), &["en"], 65)
        .await
        .unwrap();
    assert_eq!(search.searches().len(), 1);
    assert!(out.warnings[0].contains("rejected"), "{:?}", out.warnings);
}

#[tokio::test]
async fn a_server_failure_with_nothing_saved_fails_the_step_so_it_retries() {
    let dir = tempfile::tempdir().unwrap();
    let search = FakeSearch::new(|_| Err(SearchError::Failed("HTTP 503".into())));
    let err = run(dir.path(), MOVIE, &search, None, &["en"], 65)
        .await
        .unwrap_err();
    assert!(err.to_string().contains("503"), "{err}");
}

#[tokio::test]
async fn no_ids_and_no_name_are_told_apart() {
    let dir = tempfile::tempdir().unwrap();
    // A name that is only markers has no title; there is no *arr either
    let search = FakeSearch::hits(vec![]);
    let out = run(dir.path(), "1080p.WEB-DL.mkv", &search, None, &["en"], 65)
        .await
        .unwrap();
    assert_eq!(out.skipped[0].reason, SkipReason::NoName);
    // With a title but no ids and nothing found by name, the missing ids are the news
    let out = run(dir.path(), MOVIE, &search, None, &["en"], 65)
        .await
        .unwrap();
    assert_eq!(out.skipped[0].reason, SkipReason::NoIds);
}
