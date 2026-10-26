//! `GET /stats/summary` behind the Stats page (feature 006, contracts/api.md).

mod common;

use axum::http::{Method, StatusCode};
use chrono::{Duration, Utc};
use common::{body_json, test_app, test_app_without_db};
use putio_sync_db::models::{DownloadSource, Origin, SourceType};

#[tokio::test]
async fn requires_token() {
    let app = test_app().await;
    let response = app.req(Method::GET, "/stats/summary", None, None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn rejects_bad_parameters() {
    let app = test_app().await;
    let long = "x".repeat(65);
    for (query, param) in [
        ("range=2d", "range"),
        ("tz=abc", "tz"),
        ("tz=841", "tz"),
        ("category=movies", "category"),
        ("source=dht", "source"),
        ("private=yes", "private"),
        ("origin=upload", "origin"),
        (&format!("ext={long}") as &str, "ext"),
    ] {
        let response = app.get(&format!("/stats/summary?{query}")).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{query}");
        let body = body_json(response).await;
        assert_eq!(body["error"], "Invalid query", "{query}");
        assert!(
            body["details"].as_str().unwrap().starts_with(param),
            "{query}: {body}"
        );
    }
}

#[tokio::test]
async fn empty_database_has_every_section() {
    let app = test_app().await;
    let response = app.get("/stats/summary?range=&tz=-300").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;

    assert_eq!(body["range"]["range"], "7d");
    assert_eq!(body["range"]["bucket"], "6h");
    assert_eq!(body["range"]["tz"], -300);
    assert_eq!(body["totals"]["files"], 0);
    assert!(body["series"].as_array().unwrap().len() >= 28);
    assert_eq!(body["heatmap"]["files"].as_array().unwrap().len(), 7);
    assert_eq!(body["types"], serde_json::json!([]));
    for key in [
        "types",
        "trackers",
        "privacy",
        "origins",
        "watch_folders",
        "folders",
    ] {
        assert_eq!(body["sources"][key], serde_json::json!([]), "{key}");
    }
    for key in ["extensions", "trackers", "watch_folders", "folders"] {
        assert_eq!(body["options"][key], serde_json::json!([]), "{key}");
    }
}

#[tokio::test]
async fn seeded_range_totals_and_filters() {
    let app = test_app().await;
    let history = app.db().history();
    let now = Utc::now();
    for (file_id, name, size, days) in [
        (1, "a.mkv", 4_000, 1),
        (2, "b.mkv", 2_000, 10),
        (3, "c.srt", 10, 20),
        (4, "old.mkv", 99, 45),
    ] {
        history
            .insert_download_at(
                file_id,
                name,
                size,
                Some(2),
                None,
                now - Duration::days(days),
            )
            .await
            .unwrap();
    }
    app.db()
        .sources()
        .record(
            1,
            &DownloadSource {
                transfer_id: Some(5),
                source_type: Some(SourceType::Torrent),
                tracker_host: Some("tracker.example.org".into()),
                is_private: true,
                origin: Origin::Watch,
                watch_folder: Some("/watch/tv".into()),
                putio_folder: Some("TV".into()),
            },
        )
        .await
        .unwrap();

    let body = body_json(app.get("/stats/summary?range=30d").await).await;
    assert_eq!(body["range"]["bucket"], "1d");
    assert!((30..=31).contains(&body["series"].as_array().unwrap().len()));
    assert_eq!(body["totals"]["files"], 3);
    assert_eq!(body["totals"]["bytes"], 6_010);
    assert_eq!(body["totals"]["seconds"], 6);
    let series_bytes: i64 = body["series"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["bytes"].as_i64().unwrap())
        .sum();
    assert_eq!(series_bytes, 6_010);
    assert_eq!(body["types"][0]["category"], "video");
    assert_eq!(body["options"]["extensions"][0], "mkv");
    assert_eq!(body["options"]["trackers"][0], "tracker.example.org");
    assert_eq!(body["sources"]["origins"][0]["key"], "watch");

    let body = body_json(app.get("/stats/summary?range=30d&category=subtitle").await).await;
    assert_eq!(body["totals"]["bytes"], 10);
    // Filter menus don't shrink to the selection
    assert_eq!(body["options"]["extensions"].as_array().unwrap().len(), 2);

    let body = body_json(app.get("/stats/summary?range=30d&private=true").await).await;
    assert_eq!(body["totals"]["bytes"], 4_000);
    let body = body_json(
        app.get("/stats/summary?range=30d&watch=%2Fwatch%2Ftv")
            .await,
    )
    .await;
    assert_eq!(body["totals"]["files"], 1);
    let body = body_json(app.get("/stats/summary?range=30d&source=unknown").await).await;
    assert_eq!(body["totals"]["files"], 2);
    let body = body_json(app.get("/stats/summary?range=all").await).await;
    assert_eq!(body["totals"]["files"], 4);
}

#[tokio::test]
async fn without_database_is_500() {
    let app = test_app_without_db().await;
    let response = app.get("/stats/summary").await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(body_json(response).await["error"], "Database not available");
}
