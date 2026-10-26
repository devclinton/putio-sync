//! Queue actions behind the queue page (feature 004, US3, FR-012, FR-021).
//!
//! The order is the one `GET /local/queue` lists, which is the order the workers claim
//! pending rows in (highest priority first).

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app, TestApp};
use putio_sync_db::models::QueueStatus;
use serde_json::{json, Value};

/// Three pending rows `a`, `b`, `c` with priorities 0, 1, 2 (so listed c, b, a) and a
/// completed row `d` listed last
async fn seeded() -> TestApp {
    let app = test_app().await;
    let queue = app.db().queue();
    for (file_id, name, priority) in [(1, "a", 0), (2, "b", 1), (3, "c", 2), (4, "d", -1)] {
        queue
            .add(file_id, name, 1024, "http://unused", "", priority)
            .await
            .unwrap();
    }
    queue
        .update_status(4, QueueStatus::Completed)
        .await
        .unwrap();
    app
}

/// Filenames in the order `GET /local/queue` lists them
async fn order(app: &TestApp) -> Vec<String> {
    let response = app.get("/local/queue").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;
    body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["filename"].as_str().unwrap().to_string())
        .collect()
}

async fn stats(app: &TestApp) -> Value {
    body_json(app.get("/local/queue/stats").await).await
}

async fn action(app: &TestApp, path: &str) -> (StatusCode, Value) {
    let response = app.post_json(path, json!({})).await;
    let status = response.status();
    (status, body_json(response).await)
}

#[tokio::test]
async fn queue_lists_in_worker_order() {
    let app = seeded().await;
    assert_eq!(order(&app).await, ["c", "b", "a", "d"]);
    let next = app
        .db()
        .queue()
        .claim_next_pending()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(next.filename, "c");
}

#[tokio::test]
async fn move_up() {
    let app = seeded().await;
    let (status, body) = action(&app, "/local/queue/2/move_up").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body["message"].is_string(), "{body}");
    assert_eq!(order(&app).await, ["b", "c", "a", "d"]);

    let (status, body) = action(&app, "/local/queue/2/move_up").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "Item is already first");
}

#[tokio::test]
async fn move_down() {
    let app = seeded().await;
    let (status, body) = action(&app, "/local/queue/3/move_down").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(order(&app).await, ["b", "c", "a", "d"]);

    let (status, body) = action(&app, "/local/queue/4/move_down").await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(body["error"], "Item is already last");
}

#[tokio::test]
async fn move_first() {
    let app = seeded().await;
    let (status, body) = action(&app, "/local/queue/1/move_first").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(order(&app).await, ["a", "c", "b", "d"]);
    // The workers pick the new order
    let next = app
        .db()
        .queue()
        .claim_next_pending()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(next.filename, "a");
}

#[tokio::test]
async fn move_last() {
    let app = seeded().await;
    let (status, body) = action(&app, "/local/queue/3/move_last").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(order(&app).await, ["b", "a", "d", "c"]);
}

/// The order rule (tasks.md Implementation Notes, US3): a move renumbers the queue but
/// keeps everyone's relative order, so a row that sorts first (say one queued with a
/// raised priority to be resumed first) stays ahead of everything nobody moved above it,
/// and the operator's explicit move above it wins. The worker claims in that order.
#[tokio::test]
async fn moves_keep_a_raised_row_ahead_unless_moved_above() {
    let app = test_app().await;
    let queue = app.db().queue();
    for (file_id, name, priority) in [(1, "a", 0), (2, "b", 0), (3, "resume", 1000)] {
        queue
            .add(file_id, name, 1024, "http://unused", "", priority)
            .await
            .unwrap();
    }
    assert_eq!(order(&app).await, ["resume", "a", "b"]);

    // Moving another row around it leaves it first, for the list and the worker
    assert_eq!(
        action(&app, "/local/queue/2/move_up").await.0,
        StatusCode::OK
    );
    assert_eq!(order(&app).await, ["resume", "b", "a"]);
    assert_eq!(
        action(&app, "/local/queue/2/move_down").await.0,
        StatusCode::OK
    );
    assert_eq!(order(&app).await, ["resume", "a", "b"]);
    // Rows the sync adds later (priority 0) still queue behind it
    queue
        .add(4, "new", 1024, "http://unused", "", 0)
        .await
        .unwrap();
    assert_eq!(order(&app).await, ["resume", "a", "b", "new"]);
    let next = queue.claim_next_pending().await.unwrap().unwrap();
    assert_eq!(next.filename, "resume");
    queue.update_status(3, QueueStatus::Pending).await.unwrap();

    // The operator moves a row above it: that row goes first
    assert_eq!(
        action(&app, "/local/queue/1/move_first").await.0,
        StatusCode::OK
    );
    assert_eq!(order(&app).await, ["a", "resume", "b", "new"]);
    let next = queue.claim_next_pending().await.unwrap().unwrap();
    assert_eq!(next.filename, "a");
}

/// The folder sync queues every file with priority 0; moves must still reorder them
#[tokio::test]
async fn moves_reorder_equal_priorities() {
    let app = test_app().await;
    let queue = app.db().queue();
    for (file_id, name) in [(1, "a"), (2, "b"), (3, "c")] {
        queue
            .add(file_id, name, 1024, "http://unused", "", 0)
            .await
            .unwrap();
    }
    assert_eq!(order(&app).await, ["a", "b", "c"]);

    assert_eq!(
        action(&app, "/local/queue/2/move_up").await.0,
        StatusCode::OK
    );
    assert_eq!(order(&app).await, ["b", "a", "c"]);
    assert_eq!(
        action(&app, "/local/queue/2/move_down").await.0,
        StatusCode::OK
    );
    assert_eq!(order(&app).await, ["a", "b", "c"]);
    assert_eq!(
        action(&app, "/local/queue/3/move_first").await.0,
        StatusCode::OK
    );
    assert_eq!(order(&app).await, ["c", "a", "b"]);
    assert_eq!(
        action(&app, "/local/queue/3/move_first").await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        action(&app, "/local/queue/1/move_last").await.0,
        StatusCode::OK
    );
    assert_eq!(order(&app).await, ["c", "b", "a"]);
    let next = app
        .db()
        .queue()
        .claim_next_pending()
        .await
        .unwrap()
        .unwrap();
    assert_eq!(next.filename, "c");
}

#[tokio::test]
async fn move_unknown_is_404() {
    let app = seeded().await;
    for verb in ["move_up", "move_down", "move_first", "move_last", "skip"] {
        let (status, body) = action(&app, &format!("/local/queue/999/{verb}")).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{verb}: {body}");
    }
    assert_eq!(order(&app).await, ["c", "b", "a", "d"]);
}

#[tokio::test]
async fn skip_removes_the_row() {
    let app = seeded().await;
    let (status, body) = action(&app, "/local/queue/2/skip").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(order(&app).await, ["c", "a", "d"]);
    assert_eq!(stats(&app).await["pending"], 2);
}

#[tokio::test]
async fn pause_and_resume_all() {
    let app = seeded().await;
    let (status, body) = action(&app, "/local/queue/pause").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Paused 3 items in queue");
    let s = stats(&app).await;
    assert_eq!(
        (s["pending"].clone(), s["paused"].clone()),
        (json!(0), json!(3))
    );
    assert_eq!(s["completed"], 1, "completed rows are not paused");
    assert!(app
        .db()
        .queue()
        .claim_next_pending()
        .await
        .unwrap()
        .is_none());

    let (status, body) = action(&app, "/local/queue/resume").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Resumed 3 items in queue");
    let s = stats(&app).await;
    assert_eq!(
        (s["pending"].clone(), s["paused"].clone()),
        (json!(3), json!(0))
    );
    // Order is kept
    assert_eq!(order(&app).await, ["c", "b", "a", "d"]);
}

#[tokio::test]
async fn clear_removes_completed() {
    let app = seeded().await;
    let (status, body) = action(&app, "/local/queue/clear").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["message"], "Cleared 1 completed items from queue");
    assert_eq!(order(&app).await, ["c", "b", "a"]);
    assert_eq!(stats(&app).await["completed"], 0);
}

#[tokio::test]
async fn cancel_unknown_download_is_404() {
    let app = seeded().await;
    let response = app
        .req(
            Method::DELETE,
            "/local/downloads/999",
            Some(&app.token),
            None,
        )
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = body_json(response).await;
    assert!(body["error"].is_string(), "{body}");
}

#[tokio::test]
async fn cancel_queued_download_returns_it_to_pending() {
    let app = seeded().await;
    app.db()
        .queue()
        .update_status(3, QueueStatus::Downloading)
        .await
        .unwrap();
    let response = app
        .req(Method::DELETE, "/local/downloads/3", Some(&app.token), None)
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(stats(&app).await["downloading"], 0);
    assert_eq!(stats(&app).await["pending"], 3);
}

#[tokio::test]
async fn queue_states_are_plain_names() {
    let app = seeded().await;
    let body = body_json(app.get("/local/queue").await).await;
    let states: Vec<&str> = body["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["status"].as_str().unwrap())
        .collect();
    assert_eq!(states, ["pending", "pending", "pending", "completed"]);
}
