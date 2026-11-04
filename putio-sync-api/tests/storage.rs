//! Put.io storage routes (feature 033): the summary, a preview that deletes nothing, a
//! free-up that deletes exactly the previewed files and records the run and History, and
//! the error answers. Put.io is mocked; nothing here talks to a real account.

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app, test_app_without_db, TestApp};
use putio_sync_db::models::{DownloadStatus, HistoryAction};
use serde_json::{json, Value};

const GB: i64 = 1 << 30;

fn file(id: i64, name: &str, size: i64, folder: bool, created: &str) -> Value {
    json!({
        "id": id, "name": name, "size": size,
        "file_type": if folder { "FOLDER" } else { "VIDEO" }, "parent_id": 0,
        "created_at": created, "is_folder": folder, "is_shared": false
    })
}

fn listing(files: Vec<Value>) -> Value {
    json!({"status": "OK", "files": files, "parent": file(0, "root", 0, true, "2020-01-01T00:00:00")})
}

async fn account(app: &mut TestApp, size: i64, used: i64) -> mockito::Mock {
    app.putio
        .mock("GET", "/account/info")
        .with_header("content-type", "application/json")
        .with_body(
            json!({"status": "OK", "info": {"user_id": 1, "username": "u", "mail": "m",
                "account_active": true, "disk": {"size": size, "used": used, "avail": size - used}}})
            .to_string(),
        )
        .create_async()
        .await
}

/// 100 GB with 95 used; the root holds three old files, two of them downloaded with a
/// local copy (ids 1 and 2) and one not (3)
async fn nearly_full(app: &mut TestApp, local: &tempfile::TempDir) -> Vec<mockito::Mock> {
    let mut mocks = vec![
        account(app, 100 * GB, 95 * GB).await,
        app.mock_files_list(
            0,
            listing(vec![
                file(1, "Old.mkv", 8 * GB, false, "2026-01-01T00:00:00"),
                file(2, "Older.mkv", 8 * GB, false, "2025-12-01T00:00:00"),
                file(3, "Oldest.mkv", 8 * GB, false, "2025-11-01T00:00:00"),
            ]),
        )
        .await,
        app.putio
            .mock("GET", "/transfers/list")
            .with_header("content-type", "application/json")
            .with_body(r#"{"status":"OK","transfers":[]}"#)
            .create_async()
            .await,
    ];
    for (id, name) in [(1, "Old.mkv"), (2, "Older.mkv")] {
        let path = local.path().join(name);
        std::fs::File::create(&path)
            .unwrap()
            .set_len(8 * GB as u64)
            .unwrap();
        app.db()
            .downloads()
            .insert(
                id,
                name,
                8 * GB,
                &path.to_string_lossy(),
                DownloadStatus::Completed,
            )
            .await
            .unwrap();
        mocks.push(
            app.mock_file(
                id,
                json!({"status": "OK", "file": file(id, name, 8 * GB, false, "2026-01-01T00:00:00")}),
            )
            .await,
        );
    }
    mocks
}

#[tokio::test]
async fn summary_needs_a_token_and_lists_largest_first() {
    let mut app = test_app().await;
    let response = app.req(Method::GET, "/putio/storage", None, None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let _account = account(&mut app, 100 * GB, 80 * GB).await;
    let _root = app
        .mock_files_list(
            0,
            listing(vec![
                file(10, "Movies", 50 * GB, true, "2026-01-01T00:00:00"),
                file(11, "TV", 25 * GB, true, "2026-01-01T00:00:00"),
                file(12, "Books", 5 * GB, true, "2026-01-01T00:00:00"),
            ]),
        )
        .await;
    let response = app.get("/putio/storage").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["size"], json!(100 * GB));
    assert_eq!(body["free"], json!(20 * GB));
    assert_eq!(body["used_percent"], json!(80.0));
    assert_eq!(body["low"], json!(false));
    assert_eq!(body["warn_free_percent"], json!(10));
    let top: Vec<i64> = body["top"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["id"].as_i64().unwrap())
        .collect();
    assert_eq!(top, vec![10, 11, 12]);
    assert_eq!(body["top"][0]["share_percent"], json!(62.5));
}

#[tokio::test]
async fn preview_then_free_deletes_exactly_the_previewed_files() {
    let mut app = test_app().await;
    let local = tempfile::TempDir::new().unwrap();
    let _mocks = nearly_full(&mut app, &local).await;
    let no_delete = app
        .putio
        .mock("POST", "/files/delete")
        .expect(0)
        .create_async()
        .await;

    let response = app.post_json("/putio/storage/plan", json!({})).await;
    assert_eq!(response.status(), StatusCode::OK);
    let plan = body_json(response).await;
    // 5 GB free, 20 GB target: 15 needed; oldest downloaded first, not the undownloaded 3
    let ids: Vec<i64> = plan["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["id"].as_i64().unwrap())
        .collect();
    assert_eq!(ids, vec![2, 1]);
    assert_eq!(plan["needed_bytes"], json!(15 * GB));
    assert_eq!(plan["shortfall_bytes"], json!(0));
    assert!(plan["files"][0]["local_path"]
        .as_str()
        .unwrap()
        .ends_with("Older.mkv"));
    // A preview deletes nothing
    no_delete.assert_async().await;
    no_delete.remove_async().await;

    let delete = app
        .putio
        .mock("POST", "/files/delete")
        .match_body(mockito::Matcher::Json(json!({"file_ids": "2,1"})))
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"OK"}"#)
        .expect(1)
        .create_async()
        .await;
    let response = app
        .post_json(
            "/putio/storage/free",
            json!({"plan_id": plan["id"], "file_ids": [2, 1]}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    delete.assert_async().await;
    let result = body_json(response).await;
    assert_eq!(result["deleted"].as_array().unwrap().len(), 2);
    assert_eq!(result["bytes_deleted"], json!(16 * GB));
    assert!(result["run_id"].is_i64());

    let runs = body_json(app.get("/putio/storage/runs").await).await;
    assert_eq!(runs.as_array().unwrap().len(), 1);
    assert_eq!(runs[0]["trigger"], json!("manual"));
    assert_eq!(runs[0]["deleted"].as_array().unwrap().len(), 2);
    let deleted = app
        .db()
        .history()
        .count(Some(HistoryAction::Deleted), None)
        .await
        .unwrap();
    assert_eq!(deleted, 2);

    // The preview was used up
    let response = app
        .post_json(
            "/putio/storage/free",
            json!({"plan_id": plan["id"], "file_ids": [2]}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn bad_bodies_and_unknown_previews_are_refused() {
    let app = test_app().await;
    for body in [
        json!({}),
        json!({"plan_id": "x"}),
        json!({"plan_id": 1, "file_ids": []}),
    ] {
        let response = app.post_json("/putio/storage/free", body.clone()).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{body}");
    }
    let response = app
        .post_json(
            "/putio/storage/free",
            json!({"plan_id": "x", "file_ids": []}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response = app
        .post_json(
            "/putio/storage/free",
            json!({"plan_id": "x", "file_ids": [1]}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert!(body_json(response).await["error"]
        .as_str()
        .unwrap()
        .contains("Preview again"));
}

#[tokio::test]
async fn without_a_database_there_is_no_preview_and_no_runs() {
    let mut app = test_app_without_db().await;
    let _account = account(&mut app, 100 * GB, 95 * GB).await;
    let response = app.post_json("/putio/storage/plan", json!({})).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let runs = body_json(app.get("/putio/storage/runs").await).await;
    assert_eq!(runs, json!([]));
}
