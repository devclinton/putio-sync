//! Crash report routes (feature 058): list, show with the issue links, dismiss, and the
//! token requirement

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app};
use putio_sync_core::crash::{self, PanicDetails};
use serde_json::json;
use std::time::Instant;

fn write(dir: &std::path::Path, message: &str) -> String {
    crash::write_panic(
        &dir.join(crash::DIR_NAME),
        Instant::now(),
        &PanicDetails {
            thread: "main".into(),
            message: message.into(),
            location: None,
        },
        "   0: putio_sync_download::fetch::part\n   1: putio_sync_cli::main\n",
        vec!["INFO a line token=supersecretvalue1".into()],
    )
    .unwrap()
    .id
}

#[tokio::test]
async fn crash_routes_need_a_token() {
    let app = test_app().await;
    for (method, path) in [
        (Method::GET, "/crash-reports"),
        (Method::GET, "/crash-reports/20260101T000000000-000"),
        (
            Method::POST,
            "/crash-reports/20260101T000000000-000/dismiss",
        ),
    ] {
        let res = app.req(method, path, None, None).await;
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{path}");
    }
}

#[tokio::test]
async fn no_reports_is_an_empty_list() {
    let app = test_app().await;
    let res = app.get("/crash-reports").await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(res).await;
    assert_eq!(body["pending"], 0);
    assert_eq!(body["reports"], json!([]));
}

#[tokio::test]
async fn a_report_is_listed_shown_and_dismissed() {
    let app = test_app().await;
    let id = write(app._dir.path(), "it broke");

    let body = body_json(app.get("/crash-reports").await).await;
    assert_eq!(body["pending"], 1);
    assert_eq!(body["reports"][0]["id"], id);
    assert_eq!(body["reports"][0]["message"], "it broke");
    assert_eq!(body["reports"][0]["dismissed"], false);

    let res = app.get(&format!("/crash-reports/{id}")).await;
    assert_eq!(res.status(), StatusCode::OK);
    let detail = body_json(res).await;
    assert_eq!(detail["signature"], "fetch::part < putio_sync_cli::main");
    assert_eq!(detail["title"], "Crash: fetch::part < putio_sync_cli::main");
    assert_eq!(detail["stopped"], true);
    let url = detail["url"].as_str().unwrap();
    assert!(
        url.starts_with("https://github.com/devclinton/putio-sync/issues/new?template=crash.yml")
    );
    assert!(url.chars().count() <= crash::MAX_URL);
    assert!(detail["search_url"].as_str().unwrap().contains("issues?q="));
    let text = detail["text"].as_str().unwrap();
    assert!(text.contains("it broke") && text.contains("Backtrace:"));
    assert!(!text.contains("supersecretvalue1"), "{text}");

    let res = app
        .post_json(&format!("/crash-reports/{id}/dismiss"), json!({}))
        .await;
    assert_eq!(res.status(), StatusCode::OK);
    let body = body_json(app.get("/crash-reports").await).await;
    assert_eq!(body["pending"], 0);
    assert_eq!(body["reports"][0]["dismissed"], true);
}

#[tokio::test]
async fn unknown_and_unsafe_ids_are_not_found() {
    let app = test_app().await;
    for id in ["20200101T000000000-000", "..%2Fconfig", "x"] {
        let res = app.get(&format!("/crash-reports/{id}")).await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND, "{id}");
        let res = app
            .post_json(&format!("/crash-reports/{id}/dismiss"), json!({}))
            .await;
        assert_eq!(res.status(), StatusCode::NOT_FOUND, "{id}");
    }
}

/// The notice shows the report as text, never markup, and opens GitHub without an opener
#[test]
fn the_notice_uses_text_nodes_and_noopener() {
    let js = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/ui/crash.js")).unwrap();
    assert!(
        !js.contains("innerHTML")
            && !js.contains("outerHTML")
            && !js.contains("insertAdjacentHTML")
    );
    assert!(js.contains("text.textContent = report.text"));
    assert!(js.contains("putio-sync hit an internal error"));
    assert!(js.contains(r#"rel: "noopener""#));
    assert!(js.contains(r#"target: "_blank""#));
}
