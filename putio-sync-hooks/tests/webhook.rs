//! URL hooks (feature 027, T010).

mod common;

use std::time::Duration;

use common::*;
use putio_sync_hooks::{webhook, RunResult};

const FAST: [Duration; 2] = [Duration::from_millis(10), Duration::from_millis(10)];

#[tokio::test]
async fn header_and_body_arrive() {
    let mut server = mockito::Server::new_async().await;
    let event = test_event();
    let mock = server
        .mock("POST", "/hook")
        .match_query(mockito::Matcher::UrlEncoded("token".into(), "abc".into()))
        .match_header("authorization", "Bearer s3cret")
        .match_header("content-type", "application/json")
        .match_header(
            "user-agent",
            mockito::Matcher::Regex("^putio-sync/".to_string()),
        )
        .match_body(mockito::Matcher::PartialJsonString(format!(
            "{{\"id\":\"{}\",\"type\":\"hook.test\"}}",
            event.id
        )))
        .with_status(200)
        .with_body("ok")
        .create_async()
        .await;
    let mut hook = url_hook("ha", &format!("{}/hook?token=abc", server.url()));
    hook.headers
        .insert("Authorization".to_string(), "Bearer s3cret".to_string());

    let run = webhook::run(&webhook::client(), &hook, &event, &FAST).await;

    mock.assert_async().await;
    assert_eq!(run.result, RunResult::Success, "{:?}", run);
    assert_eq!(run.http_status, Some(200));
    assert_eq!(run.attempts, 1);
    assert_eq!(run.stdout, "ok");
}

#[tokio::test]
async fn a_500_is_retried() {
    let mut server = mockito::Server::new_async().await;
    let fail = server
        .mock("POST", "/hook")
        .with_status(500)
        .expect(1)
        .create_async()
        .await;
    let hook = url_hook("ha", &format!("{}/hook", server.url()));
    let client = webhook::client();
    let event = test_event();
    let handle = tokio::spawn({
        let hook = hook.clone();
        async move {
            webhook::run(
                &client,
                &hook,
                &event,
                &[Duration::from_millis(300), Duration::from_millis(300)],
            )
            .await
        }
    });
    tokio::time::sleep(Duration::from_millis(150)).await;
    fail.assert_async().await;
    fail.remove_async().await;
    server
        .mock("POST", "/hook")
        .with_status(204)
        .create_async()
        .await;
    let run = handle.await.unwrap();
    assert_eq!(run.result, RunResult::Success, "{:?}", run);
    assert_eq!(run.attempts, 2);
    assert_eq!(run.error, None);
}

#[tokio::test]
async fn a_400_is_not_retried() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("POST", mockito::Matcher::Regex("^/hook/abc".to_string()))
        .with_status(400)
        .expect(1)
        .create_async()
        .await;
    let hook = url_hook("ha", &format!("{}/hook/abc?token=zzz", server.url()));
    let run = webhook::run(&webhook::client(), &hook, &test_event(), &FAST).await;
    mock.assert_async().await;
    assert_eq!(run.result, RunResult::Failure);
    assert_eq!(run.attempts, 1);
    let error = run.error.unwrap();
    assert!(
        error.starts_with("HTTP 400 Bad Request from http://"),
        "{}",
        error
    );
    assert!(
        !error.contains("zzz") && !error.contains("abc"),
        "{}",
        error
    );
}

#[tokio::test]
async fn a_redirect_to_another_host_is_not_followed() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/hook")
        .with_status(307)
        .with_header("location", "http://example.invalid/steal")
        .create_async()
        .await;
    let mut hook = url_hook("ha", &format!("{}/hook", server.url()));
    hook.headers
        .insert("Authorization".to_string(), "Bearer s3cret".to_string());
    let run = webhook::run(&webhook::client(), &hook, &test_event(), &FAST).await;
    assert_eq!(run.result, RunResult::Failure);
    assert_eq!(run.http_status, Some(307));
}

#[tokio::test]
async fn a_same_host_redirect_is_followed() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("POST", "/old")
        .with_status(307)
        .with_header("location", "/new")
        .create_async()
        .await;
    let new = server
        .mock("POST", "/new")
        .with_status(200)
        .create_async()
        .await;
    let hook = url_hook("ha", &format!("{}/old", server.url()));
    let run = webhook::run(&webhook::client(), &hook, &test_event(), &FAST).await;
    new.assert_async().await;
    assert_eq!(run.result, RunResult::Success);
}

#[tokio::test]
async fn a_slow_server_times_out_without_leaking_the_url() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let mut held = Vec::new();
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            held.push(socket);
        }
    });
    let mut hook = url_hook("ha", &format!("http://{}/api/hook?token=zzz", addr));
    hook.timeout_secs = 1;
    let run = webhook::run(&webhook::client(), &hook, &test_event(), &[]).await;
    assert_eq!(run.result, RunResult::Timeout, "{:?}", run);
    let error = run.error.unwrap();
    assert_eq!(
        error,
        format!("timed out after 1 s calling http://{}/api/…", addr)
    );
}

#[tokio::test]
async fn a_refused_connection_is_retried_then_fails() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let hook = url_hook("ha", &format!("http://{}/hook?token=zzz", addr));
    let run = webhook::run(&webhook::client(), &hook, &test_event(), &FAST).await;
    assert_eq!(run.result, RunResult::Failure);
    assert_eq!(run.attempts, 3);
    assert_eq!(
        run.error.as_deref(),
        Some(format!("could not connect to http://{}/hook…", addr).as_str())
    );
}
