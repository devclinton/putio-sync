//! The chat client against a mock server (feature 026, research R2). No real model is used.

use std::sync::Arc;
use std::time::Duration;

use mockito::{Matcher, Server};
use putio_sync_translation::chat::ChatClient;
use putio_sync_translation::chunk::{AttemptError, CallError, Fatal};
use tokio::sync::Semaphore;

fn client(base: &str, key: &str) -> ChatClient {
    ChatClient::new(base, "qwen2.5:14b", key, 5, Arc::new(Semaphore::new(2)))
        .unwrap()
        .with_min_wait(Duration::from_millis(1))
}

fn ok(content: &str) -> String {
    serde_json::json!({"choices":[{"finish_reason":"stop","message":{"content":content}}]})
        .to_string()
}

#[tokio::test]
async fn an_empty_path_posts_to_v1_chat_completions_with_the_documented_body() {
    let mut s = Server::new_async().await;
    let m = s
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", Matcher::Missing)
        .match_body(Matcher::PartialJson(serde_json::json!({
            "model": "qwen2.5:14b", "stream": false, "temperature": 0,
            "messages": [{"role":"system","content":"sys"},{"role":"user","content":"1| Hi"}]
        })))
        .with_body(ok("1| Hola"))
        .create_async()
        .await;
    let reply = client(&s.url(), "").send("sys", "1| Hi").await.unwrap();
    assert_eq!(reply, "1| Hola");
    m.assert_async().await;
}

#[tokio::test]
async fn base_urls_with_a_path_resolve() {
    let mut s = Server::new_async().await;
    let a = s
        .mock("POST", "/v1/chat/completions")
        .with_body(ok("a"))
        .expect(2)
        .create_async()
        .await;
    let b = s
        .mock("POST", "/api/chat/completions")
        .with_body(ok("b"))
        .create_async()
        .await;
    assert_eq!(
        client(&format!("{}/v1", s.url()), "")
            .send("s", "u")
            .await
            .unwrap(),
        "a"
    );
    assert_eq!(
        client(&format!("{}/v1/chat/completions", s.url()), "")
            .send("s", "u")
            .await
            .unwrap(),
        "a"
    );
    assert_eq!(
        client(&format!("{}/api", s.url()), "")
            .send("s", "u")
            .await
            .unwrap(),
        "b"
    );
    a.assert_async().await;
    b.assert_async().await;
}

#[tokio::test]
async fn the_key_is_sent_as_a_bearer_token_only_when_set() {
    let mut s = Server::new_async().await;
    let m = s
        .mock("POST", "/v1/chat/completions")
        .match_header("authorization", "Bearer sk-test-1234")
        .with_body(ok("x"))
        .create_async()
        .await;
    client(&s.url(), "sk-test-1234")
        .send("s", "u")
        .await
        .unwrap();
    m.assert_async().await;
}

#[tokio::test]
async fn think_blocks_are_removed_and_a_length_stop_is_an_invalid_reply() {
    let mut s = Server::new_async().await;
    s.mock("POST", "/v1/chat/completions")
        .with_body(ok("<think>let me see</think>\n1| Hola"))
        .create_async()
        .await;
    assert_eq!(
        client(&s.url(), "").send("s", "u").await.unwrap(),
        "1| Hola"
    );

    let mut s = Server::new_async().await;
    s.mock("POST", "/v1/chat/completions")
        .with_body(r#"{"choices":[{"finish_reason":"length","message":{"content":"1| Ho"}}]}"#)
        .create_async()
        .await;
    let e = client(&s.url(), "").send("s", "u").await.unwrap_err();
    assert!(
        matches!(e, CallError::Attempt(AttemptError::Invalid(_))),
        "{e:?}"
    );
}

#[tokio::test]
async fn a_rejected_key_is_fatal_and_the_key_is_not_in_the_message() {
    let mut s = Server::new_async().await;
    for status in [401, 403] {
        let m = s
            .mock("POST", "/v1/chat/completions")
            .with_status(status)
            .with_body(r#"{"error":{"message":"bad key sk-secret-9999"}}"#)
            .create_async()
            .await;
        let e = client(&s.url(), "sk-secret-9999")
            .send("s", "u")
            .await
            .unwrap_err();
        assert_eq!(e, CallError::Fatal(Fatal::Key));
        assert!(!e.to_string().contains("sk-secret"));
        m.remove_async().await;
    }
}

#[tokio::test]
async fn a_404_is_a_missing_model_or_a_wrong_url() {
    for body in [
        r#"{"error":{"message":"model \"x\" not found, try pulling it first","type":"api_error"}}"#,
        r#"{"error":{"message":"The model `x` does not exist","code":"model_not_found"}}"#,
        r#"{"error":"model 'x' not found"}"#,
    ] {
        let mut s = Server::new_async().await;
        s.mock("POST", "/v1/chat/completions")
            .with_status(404)
            .with_body(body)
            .create_async()
            .await;
        let e = client(&s.url(), "").send("s", "u").await.unwrap_err();
        assert!(
            matches!(e, CallError::Fatal(Fatal::ModelNotFound(_))),
            "{body}: {e:?}"
        );
    }
    let mut s = Server::new_async().await;
    s.mock("POST", "/v1/chat/completions")
        .with_status(404)
        .with_body("404 page not found")
        .create_async()
        .await;
    let c = client(&s.url(), "");
    let e = c.send("s", "u").await.unwrap_err();
    match e {
        CallError::Fatal(Fatal::EndpointNotFound(m)) => assert!(m.contains(c.endpoint())),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn a_400_about_context_splits_and_any_other_400_is_fatal() {
    let mut s = Server::new_async().await;
    s.mock("POST", "/v1/chat/completions")
        .with_status(400)
        .with_body(r#"{"error":{"message":"This model's maximum context length is 4096 tokens"}}"#)
        .create_async()
        .await;
    let e = client(&s.url(), "").send("s", "u").await.unwrap_err();
    assert!(matches!(e, CallError::Attempt(_)), "{e:?}");

    let mut s = Server::new_async().await;
    s.mock("POST", "/v1/chat/completions")
        .with_status(400)
        .with_body(r#"{"error":{"message":"unknown field foo"}}"#)
        .create_async()
        .await;
    let e = client(&s.url(), "").send("s", "u").await.unwrap_err();
    assert!(matches!(e, CallError::Fatal(Fatal::BadRequest(_))), "{e:?}");
}

#[tokio::test]
async fn a_429_waits_and_tries_again_without_using_an_attempt() {
    let mut s = Server::new_async().await;
    let limited = s
        .mock("POST", "/v1/chat/completions")
        .with_status(429)
        .with_header("retry-after", "0")
        .expect(2)
        .create_async()
        .await;
    let c = client(&s.url(), "");
    let task = tokio::spawn({
        let c = c.clone();
        async move { c.send("s", "u").await }
    });
    tokio::time::sleep(Duration::from_millis(100)).await;
    limited.remove_async().await;
    s.mock("POST", "/v1/chat/completions")
        .with_body(ok("done"))
        .create_async()
        .await;
    assert_eq!(task.await.unwrap().unwrap(), "done");
}

#[tokio::test]
async fn a_429_wait_gives_the_permit_back() {
    let mut s = Server::new_async().await;
    // One permit only: the first caller is rate limited and keeps retrying, the second must
    // still get through while the first sleeps.
    let limited = s
        .mock("POST", "/v1/chat/completions")
        .match_body(Matcher::Regex("first".into()))
        .with_status(429)
        .with_header("retry-after", "30")
        .create_async()
        .await;
    s.mock("POST", "/v1/chat/completions")
        .match_body(Matcher::Regex("second".into()))
        .with_body(ok("2"))
        .create_async()
        .await;
    let c = ChatClient::new(&s.url(), "m", "", 5, Arc::new(Semaphore::new(1))).unwrap();
    let first = tokio::spawn({
        let c = c.clone();
        async move { c.send("s", "first").await }
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    let second = tokio::time::timeout(Duration::from_secs(3), c.send("s", "second"))
        .await
        .expect("the second request was blocked by the sleeping first one");
    assert_eq!(second.unwrap(), "2");
    limited.assert_async().await;
    first.abort();
}

#[tokio::test]
async fn a_5xx_is_a_failed_attempt() {
    let mut s = Server::new_async().await;
    s.mock("POST", "/v1/chat/completions")
        .with_status(503)
        .with_body("overloaded")
        .create_async()
        .await;
    let e = client(&s.url(), "").send("s", "u").await.unwrap_err();
    assert!(
        matches!(
            e,
            CallError::Attempt(AttemptError::Http { status: 503, .. })
        ),
        "{e:?}"
    );
}

#[tokio::test]
async fn a_slow_server_times_out_and_a_closed_port_is_unreachable() {
    // Nothing listens on this port
    let e = client("http://127.0.0.1:9", "")
        .send("s", "u")
        .await
        .unwrap_err();
    assert!(
        matches!(e, CallError::Attempt(AttemptError::Unreachable(_))),
        "{e:?}"
    );

    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let c = ChatClient::new(
        &format!("http://{addr}"),
        "m",
        "",
        1,
        Arc::new(Semaphore::new(1)),
    )
    .unwrap();
    let e = c.send("s", "u").await.unwrap_err();
    assert!(
        matches!(e, CallError::Attempt(AttemptError::Timeout(1))),
        "{e:?}"
    );
    drop(listener);
}
