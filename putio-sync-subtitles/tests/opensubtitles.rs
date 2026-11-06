//! The OpenSubtitles client (feature 050) against a mock server. No real service is used.

use std::time::Duration;

use mockito::{Matcher, Server};
use putio_sync_subtitles::opensubtitles::{OpenSubtitles, SearchError, SubtitleSearch};

const KEY: &str = "KEY-SECRET-0123";
const PASSWORD: &str = "PASSWORD-SECRET-0123";
const TOKEN: &str = "JWT-TOKEN-VALUE";

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/opensubtitles/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn client(server: &Server, user: &str) -> OpenSubtitles {
    OpenSubtitles::new(
        &server.url(),
        KEY,
        user,
        if user.is_empty() { "" } else { PASSWORD },
        "putio-sync test",
        Duration::ZERO,
    )
}

fn params(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[tokio::test]
async fn a_search_sends_the_headers_and_the_sorted_query_and_reads_the_results() {
    let mut server = Server::new_async().await;
    let m = server
        .mock("GET", "/subtitles")
        .match_header("api-key", KEY)
        .match_header("user-agent", "putio-sync test")
        .match_header("accept", "application/json")
        .match_query(Matcher::Exact(
            "languages=en%2Ces&moviehash=0123456789abcdef".to_string(),
        ))
        .with_status(200)
        .with_body(fixture("search_hash.json"))
        .expect(1)
        .create_async()
        .await;
    let hits = client(&server, "")
        .search(&params(&[
            ("languages", "en,es"),
            ("moviehash", "0123456789abcdef"),
        ]))
        .await
        .unwrap();
    m.assert_async().await;
    assert_eq!(hits.len(), 1);
    let h = &hits[0];
    assert_eq!(h.file_id, 98765);
    assert_eq!(h.language, "en");
    assert!(h.moviehash_match);
    assert_eq!(h.parent_title, "Show Name");
    assert_eq!((h.season, h.episode), (Some(2), Some(5)));
    assert_eq!(h.downloads, 4200);
}

#[tokio::test]
async fn an_empty_answer_is_no_hits() {
    let mut server = Server::new_async().await;
    server
        .mock("GET", "/subtitles")
        .with_status(200)
        .with_body(fixture("search_empty.json"))
        .create_async()
        .await;
    assert!(client(&server, "").search(&[]).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_download_asks_for_a_link_and_then_fetches_it_without_the_key() {
    let mut server = Server::new_async().await;
    let link = format!("{}/files/abc.srt", server.url());
    let body = fixture("download.json").replace("LINK", &link);
    let ask = server
        .mock("POST", "/download")
        .match_header("api-key", KEY)
        .match_body(Matcher::Json(
            serde_json::json!({"file_id": 98765, "sub_format": "srt"}),
        ))
        .with_status(200)
        .with_body(body)
        .expect(1)
        .create_async()
        .await;
    let file = server
        .mock("GET", "/files/abc.srt")
        .match_header("api-key", Matcher::Missing)
        .with_status(200)
        .with_body("1\n00:00:01,000 --> 00:00:02,000\nHi\n")
        .expect(1)
        .create_async()
        .await;
    let c = client(&server, "");
    let bytes = c.download(98765).await.unwrap();
    ask.assert_async().await;
    file.assert_async().await;
    assert!(String::from_utf8(bytes).unwrap().contains("Hi"));
    assert_eq!(c.quota().remaining, Some(197));
    assert!(!c.quota().exhausted);
}

#[tokio::test]
async fn a_login_is_made_once_and_its_token_is_reused() {
    let mut server = Server::new_async().await;
    let login = server
        .mock("POST", "/login")
        .match_body(Matcher::Json(
            serde_json::json!({"username": "me", "password": PASSWORD}),
        ))
        .with_status(200)
        .with_body(fixture("login.json"))
        .expect(1)
        .create_async()
        .await;
    let search = server
        .mock("GET", "/subtitles")
        .match_header("authorization", format!("Bearer {TOKEN}").as_str())
        .with_status(200)
        .with_body(fixture("search_empty.json"))
        .expect(2)
        .create_async()
        .await;
    let c = client(&server, "me");
    c.search(&[]).await.unwrap();
    c.search(&[]).await.unwrap();
    login.assert_async().await;
    search.assert_async().await;
    assert_eq!(c.quota().allowed, Some(200));
}

#[tokio::test]
async fn a_refused_key_stops_every_later_call_and_names_no_secret() {
    let mut server = Server::new_async().await;
    let m = server
        .mock("GET", "/subtitles")
        .with_status(401)
        .with_body(format!("{{\"message\":\"bad key {KEY}\"}}"))
        .expect(1)
        .create_async()
        .await;
    let c = client(&server, "");
    let err = c.search(&[]).await.unwrap_err();
    assert_eq!(err, SearchError::KeyRejected);
    // Not tried again: the mock allows one request
    assert_eq!(c.search(&[]).await.unwrap_err(), SearchError::KeyRejected);
    m.assert_async().await;
    let shown = format!("{err} {err:?} {c:?}");
    assert!(!shown.contains(KEY), "{shown}");
}

#[tokio::test]
async fn a_failed_login_is_a_rejected_key_without_the_password() {
    let mut server = Server::new_async().await;
    server
        .mock("POST", "/login")
        .with_status(401)
        .with_body(format!("{{\"message\":\"wrong {PASSWORD}\"}}"))
        .create_async()
        .await;
    let err = client(&server, "me").search(&[]).await.unwrap_err();
    assert_eq!(err, SearchError::KeyRejected);
    assert!(!format!("{err:?}").contains(PASSWORD));
}

#[tokio::test]
async fn the_quota_running_out_is_remembered_and_no_further_request_is_made() {
    let mut server = Server::new_async().await;
    let m = server
        .mock("POST", "/download")
        .with_status(406)
        .with_body(
            r#"{"message":"You have downloaded your allowed 20 subtitles","reset_time":"23h 12m"}"#,
        )
        .expect(1)
        .create_async()
        .await;
    let c = client(&server, "");
    assert_eq!(c.download(1).await.unwrap_err(), SearchError::Quota);
    assert!(c.quota().exhausted);
    assert_eq!(c.download(2).await.unwrap_err(), SearchError::Quota);
    m.assert_async().await;
}

#[tokio::test]
async fn a_zero_remaining_without_a_link_is_the_quota_too() {
    let mut server = Server::new_async().await;
    server
        .mock("POST", "/download")
        .with_status(200)
        .with_body(r#"{"remaining":0,"message":"quota"}"#)
        .create_async()
        .await;
    let c = client(&server, "");
    assert_eq!(c.download(1).await.unwrap_err(), SearchError::Quota);
}

#[tokio::test]
async fn a_429_waits_for_retry_after_once() {
    let mut server = Server::new_async().await;
    let first = server
        .mock("GET", "/subtitles")
        .with_status(429)
        .with_header("retry-after", "0")
        .expect(1)
        .create_async()
        .await;
    let c = client(&server, "");
    // The second answer is also 429: one wait, then the error
    let second = server
        .mock("GET", "/subtitles")
        .with_status(429)
        .with_header("retry-after", "0")
        .expect(1)
        .create_async()
        .await;
    assert_eq!(c.search(&[]).await.unwrap_err(), SearchError::RateLimited);
    first.assert_async().await;
    second.assert_async().await;
}

#[tokio::test]
async fn a_429_that_clears_gives_the_results() {
    let mut server = Server::new_async().await;
    let limited = server
        .mock("GET", "/subtitles")
        .with_status(429)
        .with_header("retry-after", "0")
        .expect(1)
        .create_async()
        .await;
    let ok = server
        .mock("GET", "/subtitles")
        .with_status(200)
        .with_body(fixture("search_hash.json"))
        .expect(1)
        .create_async()
        .await;
    let hits = client(&server, "").search(&[]).await.unwrap();
    assert_eq!(hits.len(), 1);
    limited.assert_async().await;
    ok.assert_async().await;
}

#[tokio::test]
async fn a_server_error_is_a_transient_failure() {
    let mut server = Server::new_async().await;
    server
        .mock("GET", "/subtitles")
        .with_status(503)
        .create_async()
        .await;
    assert!(matches!(
        client(&server, "").search(&[]).await.unwrap_err(),
        SearchError::Failed(_)
    ));
}

#[tokio::test]
async fn requests_keep_the_pace() {
    let mut server = Server::new_async().await;
    server
        .mock("GET", "/subtitles")
        .with_status(200)
        .with_body(fixture("search_empty.json"))
        .expect(3)
        .create_async()
        .await;
    let c = OpenSubtitles::new(&server.url(), KEY, "", "", "t", Duration::from_millis(120));
    let start = std::time::Instant::now();
    for _ in 0..3 {
        c.search(&[]).await.unwrap();
    }
    assert!(
        start.elapsed() >= Duration::from_millis(230),
        "{:?}",
        start.elapsed()
    );
}

#[tokio::test]
async fn the_test_button_logs_in_searches_and_reports_the_allowance() {
    let mut server = Server::new_async().await;
    server
        .mock("POST", "/login")
        .with_status(200)
        .with_body(fixture("login.json"))
        .create_async()
        .await;
    let m = server
        .mock("GET", "/subtitles")
        .match_query(Matcher::Exact("query=test&type=movie".to_string()))
        .with_status(200)
        .with_body(fixture("search_empty.json"))
        .expect(1)
        .create_async()
        .await;
    let quota = client(&server, "me").check().await.unwrap();
    m.assert_async().await;
    assert_eq!(quota.allowed, Some(200));
}
