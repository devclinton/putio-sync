//! `torrent-remove` without ids is refused: Transmission reads a missing `ids` as "every
//! torrent", so a malformed request must never remove anything.

use std::sync::Arc;

use putio_sync_arr::rpc::{handle, Request};
use putio_sync_arr::ArrService;
use putio_sync_client::PutioApiClient;
use putio_sync_core::config::ArrSettings;
use putio_sync_db::{Database, NewArrItem};
use putio_sync_download::{DownloadConfig, DownloadManager};
use serde_json::json;

const HASH: &str = "c12fe1c06bba254a9dc9f519b335aa7c1367a88a";

async fn service(dir: &tempfile::TempDir) -> (ArrService, Arc<Database>) {
    let db = Arc::new(Database::new(dir.path().join(".db")).await.unwrap());
    db.arr()
        .insert(&NewArrItem {
            hash: HASH,
            name: "Movie",
            category: "radarr",
            transfer_id: Some(1),
            putio_status: "COMPLETED",
            total_size: 10,
        })
        .await
        .unwrap();
    let client = Arc::new(PutioApiClient::with_base_url(
        "token".to_string(),
        "http://127.0.0.1:9".to_string(),
    ));
    let downloads = Arc::new(DownloadManager::new(
        client.clone(),
        db.clone(),
        DownloadConfig {
            target_directory: dir.path().join("downloads"),
            ..DownloadConfig::default()
        },
    ));
    let service = ArrService::new(
        client,
        db.clone(),
        downloads,
        ArrSettings {
            enabled: true,
            username: "u".into(),
            password: Some("p".into()),
            servers: Vec::new(),
        },
        dir.path().join("downloads"),
    );
    (service, db)
}

fn remove(arguments: serde_json::Value) -> Request {
    serde_json::from_value(json!({ "method": "torrent-remove", "arguments": arguments })).unwrap()
}

#[tokio::test]
async fn remove_without_ids_is_an_error_and_keeps_every_item() {
    let dir = tempfile::tempdir().unwrap();
    let (service, db) = service(&dir).await;
    for arguments in [
        json!({}),
        json!({ "ids": [] }),
        json!({ "ids": "recently-active" }),
    ] {
        let reply = handle(&service, remove(arguments.clone())).await;
        assert_eq!(reply["result"], "torrent-remove needs ids", "{arguments}");
    }
    assert!(db.arr().get_by_hash(HASH).await.unwrap().is_some());
}
