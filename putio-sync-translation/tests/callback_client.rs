//! The outbound request to a callback service (feature 026, research R6), against mockito.

use std::collections::BTreeMap;

use mockito::Matcher;
use putio_sync_translation::callback::{CallbackClient, JobRequest, Reply};
use serde_json::json;

const SECRET: &str = "Bearer very-secret-header";

fn job() -> JobRequest<'static> {
    JobRequest {
        job_id: "5b1c0f5e-7b3b-4e6c-9d0f-2a8f1c2d9e11",
        source_lang: "en",
        target_lang: "es",
        video_name: "Movie.2024.mkv",
        srt: "1\n00:00:01,000 --> 00:00:02,500\nHello.\n",
        callback_url: "http://nas.lan:8088/translate/callback/abc",
    }
}

fn headers() -> BTreeMap<String, String> {
    BTreeMap::from([("Authorization".to_string(), SECRET.to_string())])
}

fn client(url: &str) -> CallbackClient {
    CallbackClient::new(url, &headers(), 10).unwrap()
}

#[tokio::test]
async fn the_body_has_the_contract_fields_and_the_headers_go_along() {
    let mut server = mockito::Server::new_async().await;
    let m = server
        .mock("POST", "/translate")
        .match_header("authorization", SECRET)
        .match_header("content-type", "application/json")
        .match_header("user-agent", Matcher::Regex("^putio-sync/".to_string()))
        .match_body(Matcher::Json(json!({
            "job_id": "5b1c0f5e-7b3b-4e6c-9d0f-2a8f1c2d9e11",
            "source_lang": "en",
            "target_lang": "es",
            "video_name": "Movie.2024.mkv",
            "srt": "1\n00:00:01,000 --> 00:00:02,500\nHello.\n",
            "callback_url": "http://nas.lan:8088/translate/callback/abc",
        })))
        .with_status(202)
        .expect(1)
        .create_async()
        .await;
    let reply = client(&format!("{}/translate", server.url()))
        .submit(&job())
        .await;
    assert_eq!(reply, Reply::Accepted);
    m.assert_async().await;
}

#[tokio::test]
async fn a_200_with_text_or_json_is_the_subtitle() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/text")
        .with_status(200)
        .with_header("content-type", "text/plain")
        .with_body("1\n00:00:01,000 --> 00:00:02,500\nHola.\n")
        .create_async()
        .await;
    server
        .mock("POST", "/json")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({"srt": "1\n00:00:01,000 --> 00:00:02,500\nHola.\n"}).to_string())
        .create_async()
        .await;
    server
        .mock("POST", "/error")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({"error": "language not supported"}).to_string())
        .create_async()
        .await;
    for path in ["/text", "/json"] {
        match client(&format!("{}{path}", server.url()))
            .submit(&job())
            .await
        {
            Reply::Immediate(srt) => assert!(srt.contains("Hola."), "{path}"),
            other => panic!("{path}: {other:?}"),
        }
    }
    match client(&format!("{}/error", server.url()))
        .submit(&job())
        .await
    {
        Reply::Failed(m) => assert!(m.contains("language not supported"), "{m}"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn redirects_are_not_followed_and_errors_are_scrubbed() {
    let mut server = mockito::Server::new_async().await;
    let target = server
        .mock("POST", "/elsewhere")
        .with_status(200)
        .expect(0)
        .create_async()
        .await;
    server
        .mock("POST", "/moved")
        .with_status(302)
        .with_header("location", "/elsewhere")
        .create_async()
        .await;
    server
        .mock("POST", "/denied")
        .with_status(401)
        .with_body(format!("{{\"error\":\"bad credentials {SECRET}\"}}"))
        .create_async()
        .await;
    server
        .mock("POST", "/boom")
        .with_status(503)
        .with_body("upstream down")
        .create_async()
        .await;
    server
        .mock("POST", "/created")
        .with_status(201)
        .create_async()
        .await;

    match client(&format!("{}/moved", server.url()))
        .submit(&job())
        .await
    {
        Reply::Failed(m) => assert!(m.contains("redirects are not followed"), "{m}"),
        other => panic!("{other:?}"),
    }
    target.assert_async().await;
    match client(&format!("{}/denied", server.url()))
        .submit(&job())
        .await
    {
        Reply::Failed(m) => {
            assert!(m.starts_with("HTTP 401"), "{m}");
            assert!(!m.contains("very-secret-header"), "{m}");
        }
        other => panic!("{other:?}"),
    }
    match client(&format!("{}/boom", server.url()))
        .submit(&job())
        .await
    {
        Reply::Failed(m) => assert!(m.contains("503") && m.contains("upstream down"), "{m}"),
        other => panic!("{other:?}"),
    }
    match client(&format!("{}/created", server.url()))
        .submit(&job())
        .await
    {
        Reply::Failed(m) => assert!(m.contains("unexpected HTTP 201"), "{m}"),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn an_unreachable_service_fails_without_the_url_or_header_values() {
    let c = client("http://127.0.0.1:9/translate?token=abcd1234");
    match c.submit(&job()).await {
        Reply::Failed(m) => {
            assert!(m.starts_with("unreachable"), "{m}");
            assert!(
                !m.contains("abcd1234") && !m.contains("very-secret-header"),
                "{m}"
            );
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_slow_service_times_out() {
    use tokio::io::AsyncReadExt;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut s, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf).await;
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            });
        }
    });
    let c = CallbackClient::new(&format!("http://{addr}/t"), &BTreeMap::new(), 1).unwrap();
    match c.submit(&job()).await {
        Reply::Failed(m) => assert_eq!(m, "timeout after 1 s"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn bad_header_names_are_refused_when_the_client_is_made() {
    let h = BTreeMap::from([("bad name".to_string(), "v".to_string())]);
    assert!(CallbackClient::new("http://x/", &h, 10).is_err());
}
