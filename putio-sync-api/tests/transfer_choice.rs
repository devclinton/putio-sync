//! Pick files inside a transfer (feature 070): the file list of a transfer and of a
//! `.torrent`, saving and forgetting a choice, and adding a transfer with one.

mod common;

use axum::http::{Method, StatusCode};
use base64::Engine;
use common::{body_json, test_app, TestApp};
use putio_sync_db::models::QueueStatus;
use serde_json::{json, Value};

/// A Put.io transfer object
fn transfer(id: i64, file_id: Option<i64>) -> Value {
    json!({
        "id": id, "name": "Show", "size": 1000, "status": "COMPLETED",
        "down_speed": 0, "up_speed": 0, "downloaded": 1000, "uploaded": 0,
        "percent_done": 100, "estimated_time": null, "peers_connected": 0,
        "peers_sending_to_us": 0, "availability": 100, "current_ratio": "0.00",
        "seconds_seeding": 0, "error_message": null,
        "created_at": "2026-10-25T12:00:00Z", "finished_at": "2026-10-25T13:00:00Z",
        "save_parent_id": 0, "file_id": file_id, "is_private": false,
        "magneturi": "", "source": null, "type": "TORRENT"
    })
}

fn entry(id: i64, name: &str, parent_id: i64, folder: bool) -> Value {
    json!({
        "id": id, "name": name, "size": 100,
        "file_type": if folder { "FOLDER" } else { "VIDEO" },
        "parent_id": parent_id, "is_folder": folder, "is_shared": false
    })
}

/// Transfer 7 made the folder Show (10): e01.mkv (11), sample.mkv (12)
async fn mock_show(app: &mut TestApp) -> Vec<mockito::Mock> {
    let root = entry(0, "Root", 0, true);
    let show = entry(10, "Show", 0, true);
    let files = vec![
        entry(11, "e01.mkv", 10, false),
        entry(12, "sample.mkv", 10, false),
    ];
    let mut mocks = vec![
        app.mock_file(10, json!({ "file": show, "status": "OK" }))
            .await,
        app.mock_files_list(
            10,
            json!({ "files": files, "parent": show, "status": "OK" }),
        )
        .await,
        app.mock_files_list(
            0,
            json!({ "files": [show], "parent": root, "status": "OK" }),
        )
        .await,
    ];
    for (path, body) in [
        (
            "/transfers/7",
            json!({ "status": "OK", "transfer": transfer(7, Some(10)) }),
        ),
        (
            "/transfers/list",
            json!({ "status": "OK", "transfers": [transfer(7, Some(10))] }),
        ),
    ] {
        mocks.push(
            app.putio
                .mock("GET", path)
                .with_status(200)
                .with_header("content-type", "application/json")
                .with_body(body.to_string())
                .create_async()
                .await,
        );
    }
    mocks
}

async fn put_choice(app: &TestApp, id: i64, body: Value) -> axum::response::Response {
    app.req(
        Method::PUT,
        &format!("/putio/transfers/{id}/choice"),
        Some(&app.token),
        Some(body),
    )
    .await
}

fn by_path<'a>(files: &'a Value, path: &str) -> &'a Value {
    files["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["path"] == path)
        .unwrap()
}

/// With no choice every file is wanted; a saved choice marks the file it leaves out,
/// takes it out of the queue, and is forgotten again by DELETE
#[tokio::test]
async fn a_choice_is_listed_saved_and_forgotten() {
    let mut app = test_app().await;
    let _mocks = mock_show(&mut app).await;
    app.seed_queue(&[
        (11, "e01.mkv", QueueStatus::Pending),
        (12, "sample.mkv", QueueStatus::Pending),
    ])
    .await;

    let response = app.get("/putio/transfers/7/files").await;
    assert_eq!(response.status(), StatusCode::OK);
    let listed = body_json(response).await;
    assert_eq!(listed["name"], "Show");
    assert_eq!(listed["ready"], true);
    assert_eq!(listed["choice"], Value::Null);
    assert_eq!(listed["files"].as_array().unwrap().len(), 2);
    assert_eq!(by_path(&listed, "sample.mkv")["wanted"], true);
    assert_eq!(by_path(&listed, "sample.mkv")["state"], "pending");

    let response = put_choice(
        &app,
        7,
        json!({ "others": "download", "files": [{ "path": "sample.mkv", "wanted": false }] }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await["removed"], 1);
    assert!(app.db().queue().get_by_file_id(12).await.unwrap().is_none());

    let listed = body_json(app.get("/putio/transfers/7/files").await).await;
    assert_eq!(
        listed["choice"],
        json!({ "others": "download", "pending": false })
    );
    let sample = by_path(&listed, "sample.mkv");
    assert_eq!(sample["wanted"], false);
    assert_eq!(sample["state"], "none");
    assert_eq!(sample["waiting"]["code"], "not_chosen");
    let kept = by_path(&listed, "e01.mkv");
    assert_eq!(kept["wanted"], true);
    assert!(kept.get("waiting").is_none());

    // The queue says which transfer a row came from only when a source row exists
    let queue = body_json(app.get("/local/queue").await).await;
    assert!(queue["items"][0].get("transfer_id").is_none());

    assert_eq!(forget(&app).await.status(), StatusCode::OK);
    assert_eq!(forget(&app).await.status(), StatusCode::NOT_FOUND);
}

async fn forget(app: &TestApp) -> axum::response::Response {
    let path = "/putio/transfers/7/choice";
    app.req(Method::DELETE, path, Some(&app.token), None).await
}

/// A bad body or an empty path saves nothing
#[tokio::test]
async fn a_bad_choice_is_refused() {
    let app = test_app().await;
    for body in [
        json!({ "others": "maybe" }),
        json!({ "others": "skip", "files": [{ "path": "", "wanted": true }] }),
        json!({ "others": "download", "pending": true }),
        json!({ "others": "skip", "pending": true, "files": [{ "path": "a", "wanted": true }] }),
    ] {
        let response = put_choice(&app, 7, body).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
    assert!(app.db().choices().all().await.unwrap().is_empty());
}

/// A torrent of two files under `Show`, bencoded
const TORRENT: &[u8] = b"d4:infod5:filesld6:lengthi10e4:pathl8:Season 17:e01.mkveed6:lengthi1e4:pathl10:readme.txteee4:name4:Show12:piece lengthi16384e6:pieces0:ee";

/// The preview lists a `.torrent`'s files by the path a choice uses, without Put.io
#[tokio::test]
async fn a_torrent_is_previewed() {
    let app = test_app().await;
    let encode = |bytes: &[u8]| base64::engine::general_purpose::STANDARD.encode(bytes);
    let response = app
        .post_json(
            "/putio/transfers/preview",
            json!({ "file_data": encode(TORRENT) }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        body_json(response).await,
        json!({ "name": "Show", "files": [
            { "path": "Season 1/e01.mkv", "size": 10 },
            { "path": "readme.txt", "size": 1 }
        ]})
    );

    let response = app
        .post_json(
            "/putio/transfers/preview",
            json!({ "file_data": encode(b"not a torrent") }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

/// Adding a magnet "to choose first" stores a waiting choice for the new transfer;
/// adding with a choice stores that choice
#[tokio::test]
async fn a_transfer_is_added_with_a_choice() {
    let mut app = test_app().await;
    let _add = app
        .putio
        .mock("POST", "/transfers/add")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({ "status": "OK", "transfer": transfer(9, None) }).to_string())
        .expect(2)
        .create_async()
        .await;
    let _info = app
        .putio
        .mock("GET", "/transfers/9")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(json!({ "status": "OK", "transfer": transfer(9, None) }).to_string())
        .create_async()
        .await;

    let response = app
        .post_json(
            "/putio/transfers",
            json!({ "url": "magnet:?xt=urn:btih:0123456789abcdef", "choose_first": true }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let stored = app.db().choices().get(9).await.unwrap().unwrap();
    assert!(stored.pending && !stored.others_wanted && stored.files.is_empty());

    let listed = body_json(app.get("/putio/transfers/9/files").await).await;
    assert_eq!(listed["ready"], false);
    assert_eq!(listed["choice"]["pending"], true);

    // The transfer list marks it, and a picker can hold a transfer the same way later
    let _list = app
        .putio
        .mock("GET", "/transfers/list")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            json!({ "status": "OK", "transfers": [transfer(9, None), transfer(8, None)] })
                .to_string(),
        )
        .create_async()
        .await;
    let transfers = body_json(app.get("/putio/transfers").await).await;
    assert_eq!(transfers["items"][0]["choice"], "pending");
    assert_eq!(transfers["items"][1].get("choice"), None);
    let response = put_choice(&app, 8, json!({ "others": "skip", "pending": true })).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(app.db().choices().get(8).await.unwrap().unwrap().pending);

    let response = app
        .post_json(
            "/putio/transfers",
            json!({
                "url": "magnet:?xt=urn:btih:0123456789abcdef",
                "choice": { "others": "skip", "files": [{ "path": "readme.txt", "wanted": true }] }
            }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let stored = app.db().choices().get(9).await.unwrap().unwrap();
    assert!(!stored.pending);
    assert!(stored.wants("readme.txt") && !stored.wants("Season 1/e01.mkv"));
}
