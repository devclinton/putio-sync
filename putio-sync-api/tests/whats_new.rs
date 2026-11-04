//! What's new (feature 017): the built-in changelog with new entries marked, the first-run
//! baseline, and recording entries as seen

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app, test_app_without_db, TestApp};
use serde_json::{json, Value};

/// Every `(id, new)` in a `GET /whats-new` body
fn entries(body: &Value) -> Vec<(String, bool)> {
    body["releases"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|r| r["kinds"].as_array().unwrap())
        .flat_map(|k| k["entries"].as_array().unwrap())
        .map(|e| {
            (
                e["id"].as_str().unwrap().to_string(),
                e["new"].as_bool().unwrap(),
            )
        })
        .collect()
}

async fn whats_new(app: &TestApp) -> Value {
    let res = app.get("/whats-new").await;
    assert_eq!(res.status(), StatusCode::OK);
    body_json(res).await
}

async fn forget(app: &TestApp, id: &str) {
    sqlx::query("DELETE FROM whats_new_seen WHERE id = ?")
        .bind(id)
        .execute(app.db().pool())
        .await
        .unwrap();
}

#[tokio::test]
async fn first_run_records_every_entry_as_seen() {
    let app = test_app().await;
    let body = whats_new(&app).await;
    assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(body["releases"][0]["name"], "Unreleased");
    assert_eq!(body["new_count"], 0);
    let all = entries(&body);
    assert!(!all.is_empty());
    assert!(all.iter().all(|(_, new)| !new));
    let seen = app.db().whats_new().seen_ids().await.unwrap();
    assert!(all.iter().all(|(id, _)| seen.contains(id)));
}

#[tokio::test]
async fn an_entry_not_seen_is_new_until_marked() {
    let app = test_app().await;
    let first = entries(&whats_new(&app).await)[0].0.clone();
    forget(&app, &first).await;

    let body = whats_new(&app).await;
    assert_eq!(body["new_count"], 1);
    let new: Vec<_> = entries(&body).into_iter().filter(|(_, n)| *n).collect();
    assert_eq!(new, [(first.clone(), true)]);

    let res = app
        .post_json("/whats-new/seen", json!({ "ids": [first, "0000000000000000"] }))
        .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(whats_new(&app).await["new_count"], 0);
    let seen = app.db().whats_new().seen_ids().await.unwrap();
    assert!(!seen.contains("0000000000000000"), "unknown ids are ignored");
}

#[tokio::test]
async fn too_many_ids_are_refused() {
    let app = test_app().await;
    let ids: Vec<String> = (0..1001).map(|i| format!("{i:016x}")).collect();
    let res = app
        .post_json("/whats-new/seen", json!({ "ids": ids }))
        .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn both_routes_need_the_token() {
    let app = test_app().await;
    let res = app.req(Method::GET, "/whats-new", None, None).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    let res = app
        .req(
            Method::POST,
            "/whats-new/seen",
            None,
            Some(json!({ "ids": [] })),
        )
        .await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn without_a_database_nothing_is_new() {
    let app = test_app_without_db().await;
    let body = whats_new(&app).await;
    assert_eq!(body["new_count"], 0);
    assert!(entries(&body).iter().all(|(_, new)| !new));
    let res = app
        .post_json("/whats-new/seen", json!({ "ids": [] }))
        .await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
}
