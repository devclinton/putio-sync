//! Put.io API metrics from `execute_with_retry` (T012). Tests in this file share the
//! global registry, so they run one at a time and compare before/after values.

use std::time::Duration;

use putio_sync_client::{PutioApiClient, RetryPolicy};
use putio_sync_telemetry::global;

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

const LIST_BODY: &str = r#"{"files":[],"parent":{"id":0,"name":"Root","size":0,"file_type":"FOLDER","parent_id":0,"is_folder":true,"is_shared":false},"total":0,"status":"OK"}"#;

fn fast(max_attempts: u32) -> RetryPolicy {
    RetryPolicy {
        max_attempts,
        initial_delay: Duration::from_millis(1),
        max_delay: Duration::from_millis(5),
    }
}

fn value(name: &str, labels: &[(&str, &str)]) -> f64 {
    global().sample(name, labels).unwrap_or(0.0)
}

fn requests(outcome: &str) -> f64 {
    value(
        "putio_sync_api_requests_total",
        &[("operation", "list_files"), ("outcome", outcome)],
    )
}

fn retries(reason: &str) -> f64 {
    value("putio_sync_api_retries_total", &[("reason", reason)])
}

fn attempts() -> f64 {
    value("putio_sync_api_request_duration_seconds", &[])
}

#[tokio::test]
async fn server_errors_then_success() {
    let _serial = SERIAL.lock().await;
    let mut server = mockito::Server::new_async().await;
    let fail = server
        .mock("GET", "/v2/files/list")
        .match_query(mockito::Matcher::Any)
        .with_status(500)
        .expect(2)
        .create_async()
        .await;
    let (err0, ok0, retry0, n0) = (
        requests("server_error"),
        requests("success"),
        retries("server_error"),
        attempts(),
    );

    // Mocks match in the order they are defined; the 500s run out first.
    let ok = server
        .mock("GET", "/v2/files/list")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(LIST_BODY)
        .expect(1)
        .create_async()
        .await;
    let client = PutioApiClient::with_base_url("tok_SECRET".into(), format!("{}/v2", server.url()))
        .with_retry_policy(fast(3));

    client.list_files(0).await.unwrap();
    fail.assert_async().await;
    ok.assert_async().await;

    assert_eq!(requests("server_error") - err0, 2.0);
    assert_eq!(requests("success") - ok0, 1.0);
    assert_eq!(retries("server_error") - retry0, 2.0);
    assert_eq!(attempts() - n0, 3.0);
}

#[tokio::test]
async fn rate_limited_counts_as_client_error() {
    let _serial = SERIAL.lock().await;
    let mut server = mockito::Server::new_async().await;
    let _limited = server
        .mock("GET", "/v2/files/list")
        .match_query(mockito::Matcher::Any)
        .with_status(429)
        .with_header("retry-after", "0")
        .create_async()
        .await;
    let (client0, rate0) = (requests("client_error"), retries("rate_limited"));

    let client = PutioApiClient::with_base_url("tok_SECRET".into(), format!("{}/v2", server.url()))
        .with_retry_policy(fast(2));
    assert!(client.list_files(0).await.is_err());

    assert_eq!(requests("client_error") - client0, 2.0);
    assert_eq!(retries("rate_limited") - rate0, 1.0);
}

#[tokio::test]
async fn refused_connection_is_network_error() {
    let _serial = SERIAL.lock().await;
    // Bind then drop, so the port refuses connections.
    let addr = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let (net0, retry0) = (requests("network_error"), retries("network"));

    let client = PutioApiClient::with_base_url("tok_SECRET".into(), format!("http://{}/v2", addr))
        .with_retry_policy(fast(2));
    assert!(client.list_files(0).await.is_err());

    assert_eq!(requests("network_error") - net0, 2.0);
    assert_eq!(retries("network") - retry0, 1.0);
}
