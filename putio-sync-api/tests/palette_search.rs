//! The command palette's file searches (feature 061): `GET /putio/files/search` and
//! `GET /local/files/search` need sign-in, check `q`, and return the contract's shapes.

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app, test_app_with};
use serde_json::json;

#[tokio::test]
async fn both_searches_need_sign_in() {
    let app = test_app().await;
    for path in ["/putio/files/search?q=show", "/local/files/search?q=show"] {
        let response = app.req(Method::GET, path, None, None).await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
    }
}

#[tokio::test]
async fn both_searches_refuse_short_and_long_text() {
    let app = test_app().await;
    let long = "x".repeat(201);
    for base in ["/putio/files/search", "/local/files/search"] {
        for q in ["", "a", "%20a%20", long.as_str()] {
            let response = app.get(&format!("{base}?q={q}")).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{base}?q={q}");
        }
        let response = app.get(base).await;
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "{base} without q"
        );
    }
}

#[tokio::test]
async fn putio_search_returns_the_matches() {
    let mut app = test_app().await;
    let mock = app
        .putio
        .mock("GET", "/files/search")
        .match_query(mockito::Matcher::AllOf(vec![
            mockito::Matcher::UrlEncoded("query".into(), "the show".into()),
            mockito::Matcher::UrlEncoded("per_page".into(), "50".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({ "status": "OK", "cursor": null, "total": 2, "files": [
                { "id": 7, "name": "The Show", "size": 0, "file_type": "FOLDER", "parent_id": 0 },
                { "id": 8, "name": "The Show S01E01.mkv", "size": 734003200, "file_type": "VIDEO", "parent_id": 7 },
            ] })
            .to_string(),
        )
        .create_async()
        .await;

    let response = app.get("/putio/files/search?q=%20the%20show%20").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    mock.assert_async().await;
    assert_eq!(
        body,
        json!({ "files": [
            { "id": 7, "name": "The Show", "parent_id": 0, "is_folder": true, "size": 0 },
            { "id": 8, "name": "The Show S01E01.mkv", "parent_id": 7, "is_folder": false, "size": 734003200 },
        ] })
    );
}

#[tokio::test]
async fn putio_search_failure_is_a_bad_gateway() {
    let mut app = test_app().await;
    let _mock = app
        .putio
        .mock("GET", "/files/search")
        .match_query(mockito::Matcher::Any)
        .with_status(400)
        .with_body(
            json!({ "status": "ERROR", "error_type": "BadRequest", "error_message": "nope" })
                .to_string(),
        )
        .create_async()
        .await;

    let response = app.get("/putio/files/search?q=show").await;
    assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    let body = body_json(response).await;
    assert_eq!(body["error"], "Put.io search failed");
}

#[tokio::test]
async fn local_search_covers_downloads_and_watch_folders() {
    let watch = tempfile::tempdir().unwrap();
    std::fs::write(watch.path().join("show.torrent"), b"x").unwrap();
    let app = test_app_with({
        let paths = vec![watch.path().to_path_buf()];
        move |config| {
            config.watch.enabled = true;
            config.watch.paths = paths;
        }
    })
    .await;
    let downloads = app._dir.path();
    std::fs::create_dir_all(downloads.join("TV")).unwrap();
    std::fs::write(downloads.join("TV/The.Show.S01E01.mkv"), b"12345").unwrap();
    std::fs::write(downloads.join(".The.Show.S01E02.mkv.downloading"), b"x").unwrap();

    let body = body_json(app.get("/local/files/search?q=show").await).await;
    assert_eq!(body["truncated"], false, "{body}");
    let mut hits: Vec<_> = body["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| {
            (
                h["root"].as_str().unwrap().to_string(),
                h["path"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    hits.sort();
    assert_eq!(
        hits,
        vec![
            (
                "downloads".to_string(),
                "TV/The.Show.S01E01.mkv".to_string()
            ),
            ("watch-0".to_string(), "show.torrent".to_string()),
        ]
    );
    let file = body["hits"]
        .as_array()
        .unwrap()
        .iter()
        .find(|h| h["root"] == "downloads")
        .unwrap();
    assert_eq!(file["root_label"], "Downloads");
    assert_eq!(file["name"], "The.Show.S01E01.mkv");
    assert_eq!(file["kind"], "file");
    assert_eq!(file["size"], 5);
}
