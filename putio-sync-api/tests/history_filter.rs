//! `GET /history` name filter behind the history page (feature 004, US5, FR-017, R7).

mod common;

use axum::http::StatusCode;
use common::{body_json, test_app, TestApp};
use putio_sync_db::models::HistoryAction;
use serde_json::Value;

async fn seeded() -> TestApp {
    let app = test_app().await;
    let history = app.db().history();
    for (file_id, name, action) in [
        (1, "Ubuntu-24.04.iso", HistoryAction::Downloaded),
        (2, "debian.iso", HistoryAction::Downloaded),
        (3, "xubuntu.iso", HistoryAction::Deleted),
        (4, "box.mkv", HistoryAction::Deleted),
    ] {
        history
            .insert(file_id, name, 1024, action, None, None)
            .await
            .unwrap();
    }
    app
}

async fn history(app: &TestApp, query: &str) -> Value {
    let response = app.get(&format!("/history{query}")).await;
    assert_eq!(response.status(), StatusCode::OK);
    body_json(response).await
}

fn names(body: &Value) -> Vec<&str> {
    body["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["filename"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn name_filter_matches_case_insensitive_substring() {
    let app = seeded().await;
    let body = history(&app, "?name=UBU").await;
    assert_eq!(body["total"], 2);
    let mut got = names(&body);
    got.sort();
    assert_eq!(got, ["Ubuntu-24.04.iso", "xubuntu.iso"]);

    let body = history(&app, "?name=ubuntu-24").await;
    assert_eq!(body["total"], 1);
    assert_eq!(names(&body), ["Ubuntu-24.04.iso"]);
}

#[tokio::test]
async fn name_filter_combines_with_action() {
    let app = seeded().await;
    let body = history(&app, "?action=deleted&name=x").await;
    assert_eq!(body["total"], 2);
    let mut got = names(&body);
    got.sort();
    assert_eq!(got, ["box.mkv", "xubuntu.iso"]);

    let body = history(&app, "?action=downloaded&name=ubu").await;
    assert_eq!(body["total"], 1);
    assert_eq!(names(&body), ["Ubuntu-24.04.iso"]);
}

#[tokio::test]
async fn total_counts_filtered_rows_across_pages() {
    let app = seeded().await;
    let body = history(&app, "?name=iso&limit=1&offset=1").await;
    assert_eq!(body["total"], 3);
    assert_eq!(body["entries"].as_array().unwrap().len(), 1);
    assert_eq!(body["offset"], 1);
}

#[tokio::test]
async fn empty_name_is_no_filter() {
    let app = seeded().await;
    let body = history(&app, "?name=").await;
    assert_eq!(body["total"], 4);
    assert_eq!(names(&body).len(), 4);
}

#[tokio::test]
async fn wildcards_match_literally() {
    let app = seeded().await;
    // `%25` is `%`; unescaped it would match every row
    let body = history(&app, "?name=%25").await;
    assert_eq!(body["total"], 0);
    let body = history(&app, "?name=b_x").await;
    assert_eq!(body["total"], 0);
}

/// Feature 040: a row saved under a new name reports its path and outcome; older rows
/// report neither
#[tokio::test]
async fn entries_carry_local_path_and_outcome() {
    let app = seeded().await;
    let rows = app.db().history();
    let id = rows
        .insert(5, "Movie.mkv", 1024, HistoryAction::Downloaded, None, None)
        .await
        .unwrap();
    rows.set_outcome(id, Some("/downloads/Movie (1).mkv"), Some("saved_as"))
        .await
        .unwrap();

    let body = history(&app, "?name=Movie").await;
    let entry = &body["entries"][0];
    assert_eq!(entry["local_path"], "/downloads/Movie (1).mkv");
    assert_eq!(entry["outcome"], "saved_as");

    let body = history(&app, "?name=debian").await;
    let entry = &body["entries"][0];
    assert_eq!(entry["local_path"], Value::Null);
    assert_eq!(entry["outcome"], Value::Null);
}

/// Feature 034: a row says how it was checked and how many re-downloads it took; older
/// rows say neither
#[tokio::test]
async fn entries_carry_verify_and_redownloads() {
    let app = seeded().await;
    let rows = app.db().history();
    let id = rows
        .insert(
            6,
            "Checked.mkv",
            1024,
            HistoryAction::Downloaded,
            None,
            None,
        )
        .await
        .unwrap();
    rows.set_verify(id, "crc32", 1).await.unwrap();

    let body = history(&app, "?name=Checked").await;
    let entry = &body["entries"][0];
    assert_eq!(entry["verify"], "crc32");
    assert_eq!(entry["redownloads"], 1);

    let body = history(&app, "?name=debian").await;
    let entry = &body["entries"][0];
    assert_eq!(entry["verify"], Value::Null);
    assert_eq!(entry["redownloads"], Value::Null);
}

/// Feature 038, FR-024: a row whose file a kept pipeline item downloaded names that item,
/// and loses it when the item is pruned
#[tokio::test]
async fn entries_link_to_their_pipeline_item() {
    let app = seeded().await;
    let pool = app.db().pool();
    sqlx::query("INSERT INTO pipeline_items (key, name, source, stage, state) VALUES ('transfer:9', 'Ubuntu', 'sync', 'notifications', 'done')")
        .execute(pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO pipeline_item_files (item_key, file_id) VALUES ('transfer:9', 1)")
        .execute(pool)
        .await
        .unwrap();

    let body = history(&app, "?name=ubuntu-24").await;
    assert_eq!(body["entries"][0]["pipeline_key"], "transfer:9");
    let body = history(&app, "?name=debian").await;
    assert_eq!(body["entries"][0]["pipeline_key"], Value::Null);

    sqlx::query("DELETE FROM pipeline_items")
        .execute(pool)
        .await
        .unwrap();
    let body = history(&app, "?name=ubuntu-24").await;
    assert_eq!(body["entries"][0]["pipeline_key"], Value::Null);
}
