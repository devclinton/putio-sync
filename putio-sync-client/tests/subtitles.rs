//! Put.io subtitle endpoints against a mock server (feature 025).

use std::time::Duration;

use mockito::{Matcher, Server};
use putio_sync_client::{PutioApiClient, RetryPolicy};

fn client(server: &Server) -> PutioApiClient {
    PutioApiClient::with_base_url("tok".to_string(), server.url()).with_retry_policy(RetryPolicy {
        max_attempts: 2,
        initial_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(2),
    })
}

const LIST: &str = r#"{"status":"OK","default":"sub-1","subtitles":[
  {"format":"srt","key":"sub-1","language":"English","language_code":"en","name":"English","source":"opensubtitles","url":"https://api.put.io/subtitles/1"},
  {"key":"sub-2","extra":"ignored"}]}"#;

#[tokio::test]
async fn list_sends_languages_csv_and_parses_leniently() {
    let mut server = Server::new_async().await;
    let m = server
        .mock("GET", "/files/42/subtitles")
        .match_header("authorization", "Bearer tok")
        .match_query(Matcher::UrlEncoded("languages".into(), "en,es".into()))
        .with_status(200)
        .with_body(LIST)
        .create_async()
        .await;
    let list = client(&server)
        .list_subtitles(42, &["en".to_string(), "es".to_string()])
        .await
        .unwrap();
    m.assert_async().await;
    assert_eq!(list.default.as_deref(), Some("sub-1"));
    assert_eq!(list.subtitles.len(), 2);
    assert_eq!(list.subtitles[0].language_code, "en");
    assert_eq!(list.subtitles[0].source, "opensubtitles");
    assert_eq!(list.subtitles[1].key, "sub-2");
    assert_eq!(list.subtitles[1].language_code, "");
}

#[tokio::test]
async fn list_without_languages_has_no_query_and_null_default_is_fine() {
    let mut server = Server::new_async().await;
    let m = server
        .mock("GET", "/files/7/subtitles")
        .match_query(Matcher::Missing)
        .with_status(200)
        .with_body(r#"{"status":"OK","default":null,"subtitles":[]}"#)
        .create_async()
        .await;
    let list = client(&server).list_subtitles(7, &[]).await.unwrap();
    m.assert_async().await;
    assert!(list.default.is_none() && list.subtitles.is_empty());
}

#[tokio::test]
async fn list_missing_fields_still_parse() {
    let mut server = Server::new_async().await;
    server
        .mock("GET", "/files/7/subtitles")
        .with_status(200)
        .with_body(r#"{"status":"OK"}"#)
        .create_async()
        .await;
    let list = client(&server).list_subtitles(7, &[]).await.unwrap();
    assert!(list.subtitles.is_empty());
}

#[tokio::test]
async fn unavailable_statuses_are_classified() {
    let mut server = Server::new_async().await;
    for (id, status) in [(1, 404), (2, 402), (3, 401)] {
        server
            .mock("GET", format!("/files/{id}/subtitles").as_str())
            .with_status(status)
            .with_body(r#"{"status":"ERROR","error_type":"x"}"#)
            .create_async()
            .await;
        let e = client(&server).list_subtitles(id, &[]).await.unwrap_err();
        assert!(e.is_subtitles_unavailable(), "{status}: {e}");
    }
    server
        .mock("GET", "/files/4/subtitles")
        .with_status(400)
        .with_body("{}")
        .create_async()
        .await;
    let e = client(&server).list_subtitles(4, &[]).await.unwrap_err();
    assert!(!e.is_subtitles_unavailable(), "{e}");
}

#[tokio::test]
async fn server_errors_retry_then_fail_as_a_real_failure() {
    let mut server = Server::new_async().await;
    let m = server
        .mock("GET", "/files/9/subtitles")
        .with_status(503)
        .expect(2)
        .create_async()
        .await;
    let e = client(&server).list_subtitles(9, &[]).await.unwrap_err();
    m.assert_async().await;
    assert!(!e.is_subtitles_unavailable());
}

#[tokio::test]
async fn download_returns_the_body_bytes() {
    let mut server = Server::new_async().await;
    let body: Vec<u8> = b"1\n00:00:01,000 --> 00:00:02,000\nCaf\xe9\n".to_vec();
    let m = server
        .mock("GET", "/files/42/subtitles/sub%201")
        .match_query(Matcher::UrlEncoded("format".into(), "srt".into()))
        .match_header("authorization", "Bearer tok")
        .with_status(200)
        .with_body(body.clone())
        .create_async()
        .await;
    let got = client(&server)
        .download_subtitle(42, "sub 1", None)
        .await
        .unwrap();
    m.assert_async().await;
    assert_eq!(got, body);
}

#[tokio::test]
async fn download_falls_back_to_the_item_url_on_404() {
    let mut server = Server::new_async().await;
    server
        .mock("GET", "/files/42/subtitles/k")
        .match_query(Matcher::Any)
        .with_status(404)
        .create_async()
        .await;
    let m = server
        .mock("GET", "/subtitles/1")
        .match_header("authorization", "Bearer tok")
        .with_status(200)
        .with_body("hello")
        .create_async()
        .await;
    let url = format!("{}/subtitles/1", server.url());
    let got = client(&server)
        .download_subtitle(42, "k", Some(&url))
        .await
        .unwrap();
    m.assert_async().await;
    assert_eq!(got, b"hello");
}

#[tokio::test]
async fn download_does_not_send_the_token_to_other_hosts() {
    let mut server = Server::new_async().await;
    server
        .mock("GET", "/files/42/subtitles/k")
        .match_query(Matcher::Any)
        .with_status(404)
        .create_async()
        .await;
    let e = client(&server)
        .download_subtitle(42, "k", Some("http://evil.example/steal"))
        .await
        .unwrap_err();
    assert!(e.is_not_found(), "{e}");
}
