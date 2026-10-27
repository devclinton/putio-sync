//! The *arr v3 API calls against a mocked Radarr/Sonarr

use putio_sync_arr::{ArrApi, ArrApiError};
use putio_sync_core::config::ArrKind;

fn api(kind: ArrKind, url: &str) -> ArrApi {
    ArrApi::new(kind, url, "secret-key", reqwest::Client::new())
}

#[tokio::test]
async fn status_checks_key_and_app() {
    let mut server = mockito::Server::new_async().await;
    let mock = server
        .mock("GET", "/api/v3/system/status")
        .match_header("x-api-key", "secret-key")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"appName":"Sonarr","version":"4.0.9.2244","branch":"main"}"#)
        .expect(2)
        .create_async()
        .await;

    let status = api(ArrKind::Sonarr, &format!("{}/", server.url()))
        .system_status()
        .await
        .unwrap();
    assert_eq!(status.app_name, "Sonarr");
    assert_eq!(status.version, "4.0.9.2244");

    let err = api(ArrKind::Radarr, &server.url())
        .system_status()
        .await
        .unwrap_err();
    assert_eq!(err.kind(), "not_arr");
    assert_eq!(err.to_string(), "not Radarr: found Sonarr");
    mock.assert_async().await;
}

#[tokio::test]
async fn unauthorized_and_unreachable() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/api/v3/system/status")
        .with_status(401)
        .create_async()
        .await;
    let err = api(ArrKind::Sonarr, &server.url())
        .system_status()
        .await
        .unwrap_err();
    assert_eq!(err, ArrApiError::Unauthorized);
    assert!(!err.to_string().contains("secret-key"));

    let err = api(ArrKind::Sonarr, "http://127.0.0.1:9")
        .system_status()
        .await
        .unwrap_err();
    assert_eq!(err.kind(), "unreachable");
    assert!(!err.to_string().contains("secret-key"));

    server
        .mock("GET", "/html/api/v3/system/status")
        .with_status(200)
        .with_body("<html>login</html>")
        .create_async()
        .await;
    let err = api(ArrKind::Sonarr, &format!("{}/html", server.url()))
        .system_status()
        .await
        .unwrap_err();
    assert_eq!(err.kind(), "not_arr");
}

#[tokio::test]
async fn queue_lookup_and_blocklist() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/api/v3/queue/details")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"[{"id":1},{"id":2,"downloadId":"ABCDEF"}]"#)
        .create_async()
        .await;
    let delete = server
        .mock(
            "DELETE",
            "/api/v3/queue/2?removeFromClient=true&blocklist=true",
        )
        .match_header("x-api-key", "secret-key")
        .with_status(200)
        .create_async()
        .await;

    let api = api(ArrKind::Radarr, &server.url());
    assert_eq!(api.find_queue_id("abcdef").await.unwrap(), Some(2));
    assert_eq!(api.find_queue_id("123456").await.unwrap(), None);
    api.remove_and_blocklist(2).await.unwrap();
    delete.assert_async().await;
}
