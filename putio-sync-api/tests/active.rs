//! `GET /active`: the Active transfers pipeline (feature 039, contracts/api.md)

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app, test_app_without_db, TestApp};
use putio_sync_db::models::{DownloadSource, Origin, QueueStatus};
use serde_json::{json, Value};
use std::time::Duration;

/// A Put.io transfer object
fn transfer(id: i64, name: &str, status: &str) -> Value {
    json!({
        "id": id, "name": name, "size": 1000, "status": status,
        "down_speed": 50, "up_speed": 0, "downloaded": 250, "uploaded": 0,
        "percent_done": 25, "estimated_time": 15, "peers": 3, "availability": 100,
        "current_ratio": 0.0, "seconds_seeding": 0, "error_message": null,
        "created_at": "2026-10-25T12:00:00", "finished_at": null,
        "save_parent_id": 0, "file_id": null, "is_private": false
    })
}

async fn mock_transfers(app: &mut TestApp, transfers: Vec<Value>) -> mockito::Mock {
    app.putio
        .mock("GET", "/transfers/list")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({ "status": "OK", "transfers": transfers }).to_string())
        .create_async()
        .await
}

/// Queue `file_id` as a file of `transfer_id`
async fn queue_file(app: &TestApp, file_id: i64, transfer_id: i64, status: QueueStatus) {
    app.seed_queue(&[(file_id, &format!("f{file_id}.mkv"), status)])
        .await;
    app.db()
        .sources()
        .record(
            file_id,
            &DownloadSource {
                transfer_id: Some(transfer_id),
                source_type: None,
                tracker_host: None,
                is_private: false,
                origin: Origin::Putio,
                watch_folder: None,
                putio_folder: None,
            },
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn active_returns_the_contract_shape() {
    let mut app = test_app().await;
    let _list = mock_transfers(
        &mut app,
        vec![
            transfer(1, "Downloading film", "DOWNLOADING"),
            transfer(2, "Show S01", "COMPLETED"),
        ],
    )
    .await;
    queue_file(&app, 20, 2, QueueStatus::Pending).await;
    queue_file(&app, 21, 2, QueueStatus::Pending).await;

    let response = app.get("/active").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;

    let items = body["items"].as_array().expect("items");
    assert_eq!(items.len(), 2, "{body}");
    let putio = items.iter().find(|i| i["key"] == "transfer:1").unwrap();
    assert_eq!(putio["stage"], "putio", "{putio}");
    assert_eq!(putio["state"], "downloading");
    assert_eq!(putio["speed_bps"], 50);
    assert_eq!(putio["links"]["transfers"], "#/transfers?id=1");
    assert_eq!(putio["links"]["queue"], "#/queue?transfer=1");
    let nas = items.iter().find(|i| i["key"] == "transfer:2").unwrap();
    assert_eq!(nas["stage"], "nas", "{nas}");
    assert_eq!(nas["state"], "queued");
    assert_eq!(nas["files"]["total"], 2);
    assert_eq!(nas["putio_status"], "COMPLETED");

    for key in ["held", "paused", "size", "done_bytes", "percent", "seeding"] {
        assert!(putio.get(key).is_some(), "{key} missing: {putio}");
    }
    let counts = &body["counts"];
    assert_eq!(counts["putio"]["downloading"], 1, "{counts}");
    assert_eq!(counts["putio"]["stalled"], 0, "zeros are listed: {counts}");
    assert_eq!(counts["putio"]["on_hold"], 0, "{counts}");
    assert_eq!(counts["nas"]["queued"], 1, "{counts}");
    assert_eq!(counts["nas"]["failed"], 0, "{counts}");
    assert_eq!(body["nas_paused"], false);
    assert_eq!(body["putio_stale"], false);
    assert!(
        body["putio_as_of"].as_str().unwrap().ends_with('Z'),
        "{body}"
    );
    assert!(body["recently_finished"].is_array(), "{body}");
}

/// Recently finished rows come back newest first, with the time spent in each stage;
/// a time is null when either end is unknown (feature 039, T048)
#[tokio::test]
async fn recently_finished_lists_rows_with_stage_times() {
    use putio_sync_db::pipeline::{FinishedOutcome, FinishedRow};
    let mut app = test_app().await;
    let _list = mock_transfers(&mut app, vec![]).await;
    let finished = app.db().finished();
    finished
        .upsert(&FinishedRow {
            key: "transfer:799".into(),
            name: "Some.Movie".into(),
            size: 31_000,
            outcome: FinishedOutcome::Done,
            reason: None,
            added_at: Some("2026-10-28 06:00:00".into()),
            putio_done_at: Some("2026-10-28 06:50:00".into()),
            queued_at: Some("2026-10-28 06:57:00".into()),
            done_at: "2026-10-28 07:30:00".into(),
        })
        .await
        .unwrap();
    finished
        .upsert(&FinishedRow {
            key: "folder:Photos".into(),
            name: "Photos".into(),
            size: 10,
            outcome: FinishedOutcome::Failed,
            reason: Some("nothing downloaded".into()),
            added_at: None,
            putio_done_at: None,
            queued_at: Some("2026-10-28 08:00:00".into()),
            done_at: "2026-10-28 09:00:00".into(),
        })
        .await
        .unwrap();

    let body = body_json(app.get("/active").await).await;
    let rows = body["recently_finished"].as_array().unwrap();
    assert_eq!(rows.len(), 2, "{body}");
    // Newest first; a row with unknown ends has null times
    assert_eq!(rows[0]["key"], "folder:Photos");
    assert_eq!(rows[0]["outcome"], "failed");
    assert_eq!(rows[0]["reason"], "nothing downloaded");
    assert_eq!(rows[0]["done_at"], "2026-10-28T09:00:00Z");
    for field in ["total_seconds", "putio_seconds", "waiting_seconds"] {
        assert!(rows[0][field].is_null(), "{field}: {body}");
    }
    // queued_at and done_at are known, so the NAS time is too
    assert_eq!(rows[0]["nas_seconds"], 3600);
    assert_eq!(rows[1]["key"], "transfer:799");
    assert_eq!(rows[1]["outcome"], "done");
    assert_eq!(rows[1]["size"], 31_000);
    assert_eq!(rows[1]["total_seconds"], 5400);
    assert_eq!(rows[1]["putio_seconds"], 3000);
    assert_eq!(rows[1]["waiting_seconds"], 420);
    assert_eq!(rows[1]["nas_seconds"], 1980);
}

/// SC-005: 200 transfers and 5,000 queue rows build the pipeline in under 200 ms
#[tokio::test]
async fn active_with_200_transfers_and_5000_queue_rows_is_fast() {
    let mut app = test_app().await;
    let transfers: Vec<Value> = (1..=200)
        .map(|id| transfer(id, &format!("Transfer {id}"), "COMPLETED"))
        .collect();
    let _list = mock_transfers(&mut app, transfers).await;
    // 25 files per transfer, in a mix of states, inserted in one transaction
    let mut tx = app.db().pool().begin().await.unwrap();
    for file_id in 1..=5000_i64 {
        let transfer_id = (file_id - 1) / 25 + 1;
        let status = match file_id % 5 {
            0 => "downloading",
            1 => "paused",
            _ => "pending",
        };
        sqlx::query(
            "INSERT INTO download_queue (file_id, filename, file_size, download_url, relative_path, status) \
             VALUES (?, ?, 100, 'http://x', ?, ?)",
        )
        .bind(file_id)
        .bind(format!("f{file_id}.mkv"))
        .bind(format!("Transfer {transfer_id}"))
        .bind(status)
        .execute(&mut *tx)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO download_sources (file_id, transfer_id, origin) VALUES (?, ?, 'putio')",
        )
        .bind(file_id)
        .bind(transfer_id)
        .execute(&mut *tx)
        .await
        .unwrap();
    }
    tx.commit().await.unwrap();

    // The first request fetches the list from Put.io; time the ones after it, which is
    // what each page poll costs (best of three, so a busy test machine does not fail it)
    let first = app.get("/active").await;
    assert_eq!(first.status(), StatusCode::OK);
    let mut best = Duration::MAX;
    for _ in 0..3 {
        let started = std::time::Instant::now();
        let response = app.get("/active").await;
        let elapsed = started.elapsed();
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_json(response).await;
        assert_eq!(body["items"].as_array().unwrap().len(), 200);
        best = best.min(elapsed);
    }
    assert!(
        best < Duration::from_millis(200),
        "GET /active took {best:?}"
    );
}

#[tokio::test]
async fn active_requires_auth() {
    let app = test_app().await;
    let response = app.req(Method::GET, "/active", None, None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn active_without_a_database_is_503() {
    let app = test_app_without_db().await;
    let response = app.get("/active").await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    let body = body_json(response).await;
    assert!(
        !body["error"].as_str().unwrap_or_default().is_empty(),
        "{body}"
    );
}

/// Put.io failing keeps the last list, marked stale, while NAS figures stay current.
/// A 400 stands in for the outage: the client retries a 503 with backoff for ~30 s.
#[tokio::test]
async fn putio_down_serves_the_last_list_marked_stale() {
    let mut app = test_app().await;
    let list = mock_transfers(&mut app, vec![transfer(2, "Show S01", "COMPLETED")]).await;
    queue_file(&app, 20, 2, QueueStatus::Pending).await;
    let body = body_json(app.get("/active").await).await;
    assert_eq!(body["putio_stale"], false, "{body}");
    let as_of = body["putio_as_of"].clone();

    list.remove_async().await;
    let _down = app
        .putio
        .mock("GET", "/transfers/list")
        .with_status(400)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"ERROR","error_type":"Down","error_message":"down"}"#)
        .create_async()
        .await;
    // Force the refetch now instead of waiting out the 10 s cache
    app.state
        .download_manager
        .transfers_snapshot(Duration::ZERO)
        .await
        .unwrap();
    queue_file(&app, 21, 2, QueueStatus::Pending).await;

    let response = app.get("/active").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(body["putio_stale"], true, "{body}");
    assert_eq!(body["putio_as_of"], as_of);
    let item = &body["items"][0];
    assert_eq!(item["key"], "transfer:2", "{body}");
    assert_eq!(item["files"]["total"], 2, "NAS figures are current: {item}");
}

fn results(body: &Value) -> Vec<(String, String)> {
    body["results"]
        .as_array()
        .unwrap_or_else(|| panic!("results: {body}"))
        .iter()
        .map(|r| {
            (
                r["key"].as_str().unwrap().to_string(),
                r["result"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

fn pair(key: &str, result: &str) -> (String, String) {
    (key.to_string(), result.to_string())
}

#[tokio::test]
async fn pause_and_resume_answer_per_key_in_order() {
    let mut app = test_app().await;
    let _list = mock_transfers(
        &mut app,
        vec![
            transfer(1, "Downloading film", "DOWNLOADING"),
            transfer(2, "Show S01", "COMPLETED"),
        ],
    )
    .await;
    queue_file(&app, 20, 2, QueueStatus::Pending).await;
    queue_file(&app, 21, 2, QueueStatus::Pending).await;

    let keys = json!({ "keys": ["transfer:2", "transfer:1", "file:5531", "transfer:2"] });
    let response = app.post_json("/active/pause", keys.clone()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(
        results(&body),
        vec![
            pair("transfer:2", "paused"),
            pair("transfer:1", "held"),
            pair("file:5531", "not_found"),
            pair("transfer:2", "already"),
        ],
        "{body}"
    );
    assert!(app.db().holds().is_held(1).await.unwrap());
    for id in [20, 21] {
        let row = app.db().queue().get_by_file_id(id).await.unwrap().unwrap();
        assert_eq!(row.status, "paused");
    }

    let response = app.post_json("/active/resume", keys).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    assert_eq!(
        results(&body),
        vec![
            pair("transfer:2", "resumed"),
            pair("transfer:1", "resumed"),
            pair("file:5531", "not_found"),
            pair("transfer:2", "already"),
        ],
        "{body}"
    );
    assert!(!app.db().holds().is_held(1).await.unwrap());
    let row = app.db().queue().get_by_file_id(20).await.unwrap().unwrap();
    assert_eq!(row.status, "pending");
}

#[tokio::test]
async fn pause_and_resume_refuse_no_keys_and_too_many() {
    let mut app = test_app().await;
    let _list = mock_transfers(&mut app, vec![]).await;
    for path in ["/active/pause", "/active/resume"] {
        let response = app.post_json(path, json!({ "keys": [] })).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
        let keys: Vec<String> = (0..501).map(|n| format!("file:{n}")).collect();
        let response = app.post_json(path, json!({ "keys": keys })).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
        let keys: Vec<String> = (0..500).map(|n| format!("file:{n}")).collect();
        let response = app.post_json(path, json!({ "keys": keys })).await;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
    }
}

#[tokio::test]
async fn pause_and_resume_require_auth() {
    let app = test_app().await;
    for path in ["/active/pause", "/active/resume"] {
        let response = app
            .req(
                Method::POST,
                path,
                None,
                Some(json!({ "keys": ["file:1"] })),
            )
            .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED, "{path}");
    }
}
