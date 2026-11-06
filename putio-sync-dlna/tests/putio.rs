//! The Put.io root against a mock Put.io (feature 022, US4). Never the real service.

mod common;
use common::Fixture;
use mockito::{Matcher, Server, ServerGuard};
use putio_sync_client::PutioApiClient;
use putio_sync_dlna::Options;
use std::sync::Arc;
use std::time::Duration;

fn file(id: i64, name: &str, parent: i64, size: i64, folder: bool) -> String {
    format!(
        r#"{{"id":{id},"name":"{name}","size":{size},"file_type":"{}","parent_id":{parent},"is_folder":{folder},"is_shared":false}}"#,
        if folder { "FOLDER" } else { "VIDEO" }
    )
}

fn listing(parent: i64, files: &[String]) -> String {
    format!(
        r#"{{"status":"OK","files":[{}],"parent":{},"total":{}}}"#,
        files.join(","),
        file(parent, "Shared", 0, 0, true),
        files.len()
    )
}

async fn start(server: &ServerGuard, ttl: Duration) -> Fixture {
    // Short retry delays so an outage test doesn't wait out the real backoff
    let client = Arc::new(
        PutioApiClient::with_base_url("test_token".to_string(), server.url()).with_retry_policy(
            putio_sync_client::RetryPolicy {
                max_attempts: 2,
                initial_delay: Duration::from_millis(5),
                max_delay: Duration::from_millis(10),
            },
        ),
    );
    Fixture::start_with(
        &["downloads", "putio"],
        Some(client),
        |c| c.dlna.putio_folder = Some(100),
        Options {
            putio_ttl: ttl,
            ..Default::default()
        },
    )
    .await
}

async fn list_mock(server: &mut ServerGuard, parent: i64, files: &[String]) -> mockito::Mock {
    server
        .mock("GET", format!("/files/list?parent_id={parent}").as_str())
        .match_header("authorization", "Bearer test_token")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(listing(parent, files))
        .create_async()
        .await
}

#[tokio::test]
async fn lists_the_shared_folder_and_its_subfolders() {
    let mut server = Server::new_async().await;
    let _a = list_mock(
        &mut server,
        100,
        &[
            file(201, "ep1.mkv", 100, 5000, false),
            file(101, "Season 1", 100, 0, true),
        ],
    )
    .await;
    let _b = list_mock(&mut server, 101, &[file(202, "ep2.mp4", 101, 700, false)]).await;
    let f = start(&server, Duration::from_secs(60)).await;

    assert_eq!(f.children("0").await.titles(), ["Downloads", "Put.io"]);
    let top = f.children("putio/").await;
    assert_eq!(top.titles(), ["Season 1", "ep1.mkv"]);
    let entries = top.entries();
    assert_eq!(entries[0].1, "putio/101");
    assert_eq!(entries[1].1, "putio/201");
    let sub = f.children("putio/101").await;
    assert_eq!(sub.titles(), ["ep2.mp4"]);
    assert!(sub.didl().contains("size=\"700\""));
}

#[tokio::test]
async fn an_item_outside_the_shared_tree_is_refused() {
    let mut server = Server::new_async().await;
    let _a = list_mock(&mut server, 100, &[]).await;
    let _o = server
        .mock("GET", "/files/999")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(format!(
            r#"{{"status":"OK","file":{}}}"#,
            file(999, "private.mkv", 0, 10, false)
        ))
        .create_async()
        .await;
    let f = start(&server, Duration::from_secs(60)).await;
    assert_eq!(f.children("putio/999").await.fault(), Some(701));
    let (status, _) = common::raw_get(f.port, "/media/putio/999").await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn plays_the_mp4_version_with_the_players_range() {
    let mut server = Server::new_async().await;
    let _a = list_mock(&mut server, 100, &[file(201, "ep1.mkv", 100, 5000, false)]).await;
    let _m = server
        .mock("GET", "/files/201/mp4")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"OK","mp4":{"status":"COMPLETED","size":4000}}"#)
        .create_async()
        .await;
    let dl = server
        .mock("GET", "/files/201/mp4/download")
        .match_header("authorization", "Bearer test_token")
        .match_header("range", "bytes=0-9")
        .with_status(206)
        .with_header("content-range", "bytes 0-9/4000")
        .with_body("0123456789")
        .create_async()
        .await;
    let f = start(&server, Duration::from_secs(60)).await;
    f.children("putio/").await;
    let r = reqwest::Client::new()
        .get(format!("{}/media/putio/201", f.base()))
        .header("Range", "bytes=0-9")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 206);
    assert_eq!(r.headers()["content-range"], "bytes 0-9/4000");
    assert_eq!(r.headers()["content-type"], "video/mp4");
    assert_eq!(r.bytes().await.unwrap().as_ref(), b"0123456789");
    dl.assert_async().await;

    // The listing advertises the converted file
    let meta = f.browse("putio/201", "BrowseMetadata", 0, 0).await;
    assert!(meta.didl().contains("video/mp4"));
    assert!(meta.didl().contains("size=\"4000\""));
}

#[tokio::test]
async fn follows_a_redirect_from_put_io() {
    let mut server = Server::new_async().await;
    let _a = list_mock(&mut server, 100, &[file(202, "movie.mp4", 100, 6, false)]).await;
    let _r = server
        .mock("GET", "/files/202/download")
        .with_status(302)
        .with_header("location", format!("{}/blob/202", server.url()).as_str())
        .create_async()
        .await;
    let _b = server
        .mock("GET", "/blob/202")
        .with_status(200)
        .with_body("abcdef")
        .create_async()
        .await;
    let f = start(&server, Duration::from_secs(60)).await;
    f.children("putio/").await;
    let r = f.get("/media/putio/202").await;
    assert_eq!(r.status(), 200);
    assert_eq!(r.bytes().await.unwrap().as_ref(), b"abcdef");
}

#[tokio::test]
async fn plays_the_original_when_there_is_no_mp4() {
    let mut server = Server::new_async().await;
    let _a = list_mock(&mut server, 100, &[file(201, "ep1.mkv", 100, 5, false)]).await;
    let _m = server
        .mock("GET", "/files/201/mp4")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"OK","mp4":{"status":"NOT_AVAILABLE"}}"#)
        .create_async()
        .await;
    let dl = server
        .mock("GET", "/files/201/download")
        .with_status(200)
        .with_body("orig!")
        .create_async()
        .await;
    let f = start(&server, Duration::from_secs(60)).await;
    f.children("putio/").await;
    let r = f.get("/media/putio/201").await;
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()["content-type"], "video/x-matroska");
    assert_eq!(r.bytes().await.unwrap().as_ref(), b"orig!");
    dl.assert_async().await;
}

#[tokio::test]
async fn a_put_io_outage_is_a_fault_and_a_502_while_downloads_still_work() {
    let mut server = Server::new_async().await;
    let _a = server
        .mock("GET", Matcher::Any)
        .with_status(500)
        .create_async()
        .await;
    let f = start(&server, Duration::from_secs(60)).await;
    f.write("local.mp4", b"x");
    let r = f.children("putio/").await;
    assert_eq!(r.fault(), Some(720));
    let (status, _) = common::raw_get(f.port, "/media/putio/201").await;
    assert_eq!(status, 502);
    assert_eq!(f.children("downloads/").await.titles(), ["local.mp4"]);
    assert!(matches!(
        f.server.status().putio,
        putio_sync_dlna::PutioHealth::Error(_)
    ));
}

#[tokio::test]
async fn listings_are_reused_until_the_ttl_passes() {
    let mut server = Server::new_async().await;
    let mock = list_mock(&mut server, 100, &[file(201, "ep1.mkv", 100, 5, false)])
        .await
        .expect(2);
    let _m = server
        .mock("GET", "/files/201/mp4")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"OK","mp4":{"status":"NOT_AVAILABLE"}}"#)
        .create_async()
        .await;
    let f = start(&server, Duration::from_millis(600)).await;
    f.children("putio/").await;
    f.children("putio/").await;
    tokio::time::sleep(Duration::from_millis(800)).await;
    f.children("putio/").await;
    mock.assert_async().await;
}

#[tokio::test]
async fn no_token_or_put_io_link_reaches_a_player() {
    let mut server = Server::new_async().await;
    let _a = list_mock(&mut server, 100, &[file(201, "ep1.mkv", 100, 5, false)]).await;
    let f = start(&server, Duration::from_secs(60)).await;
    let r = f.children("putio/").await;
    let didl = r.didl();
    let desc = f.get("/description.xml").await.text().await.unwrap();
    for text in [&didl, &desc] {
        assert!(!text.contains("test_token"));
        assert!(!text.contains(&server.url()));
        assert!(!text.contains("/files/201/download"));
    }
    assert!(didl.contains(&format!("http://127.0.0.1:{}/media/putio/201", f.port)));
}

#[tokio::test]
async fn nothing_is_asked_of_put_io_when_it_is_not_shared() {
    let mut server = Server::new_async().await;
    let any = server
        .mock("GET", Matcher::Any)
        .with_status(500)
        .expect(0)
        .create_async()
        .await;
    let client = Arc::new(PutioApiClient::with_base_url(
        "test_token".into(),
        server.url(),
    ));
    let f = Fixture::start_with(
        &["downloads"],
        Some(client),
        |c| c.dlna.putio_folder = Some(100),
        Options::default(),
    )
    .await;
    f.write("a.mp4", b"x");
    assert_eq!(f.children("0").await.titles(), ["Downloads"]);
    assert_eq!(f.children("downloads/").await.titles(), ["a.mp4"]);
    assert_eq!(f.children("putio/").await.fault(), Some(701));
    let (status, _) = common::raw_get(f.port, "/media/putio/201").await;
    assert_eq!(status, 404);
    any.assert_async().await;
}
