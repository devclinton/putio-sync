//! Fetching a .torrent from an indexer URL (T009)

use std::time::Duration;

use putio_sync_precheck::fetch::{client, fetch, FetchError, Fetched, MAX_TORRENT_BYTES};
use tokio::time::Instant;

fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(14)
}

#[tokio::test]
async fn ok_torrent() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("GET", "/t/1")
        .with_body(b"d4:infod4:name1:aee".as_slice())
        .create_async()
        .await;
    let got = fetch(&client(), &format!("{}/t/1", server.url()), deadline()).await;
    assert_eq!(got, Ok(Fetched::Torrent(b"d4:infod4:name1:aee".to_vec())));
}

#[tokio::test]
async fn follows_a_redirect() {
    let mut server = mockito::Server::new_async().await;
    let _a = server
        .mock("GET", "/a")
        .with_status(302)
        .with_header("location", "/b")
        .create_async()
        .await;
    let _b = server
        .mock("GET", "/b")
        .with_body("body")
        .create_async()
        .await;
    let got = fetch(&client(), &format!("{}/a", server.url()), deadline()).await;
    assert_eq!(got, Ok(Fetched::Torrent(b"body".to_vec())));
}

#[tokio::test]
async fn redirect_to_magnet() {
    let magnet = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=x";
    let mut server = mockito::Server::new_async().await;
    let _a = server
        .mock("GET", "/a")
        .with_status(302)
        .with_header("location", magnet)
        .create_async()
        .await;
    let got = fetch(&client(), &format!("{}/a", server.url()), deadline()).await;
    assert_eq!(got, Ok(Fetched::Magnet(magnet.to_string())));
}

#[tokio::test]
async fn six_hops_fail() {
    let mut server = mockito::Server::new_async().await;
    let mut mocks = Vec::new();
    for i in 0..6 {
        mocks.push(
            server
                .mock("GET", format!("/{i}").as_str())
                .with_status(302)
                .with_header("location", &format!("/{}", i + 1))
                .create_async()
                .await,
        );
    }
    let _end = server
        .mock("GET", "/6")
        .with_body("body")
        .create_async()
        .await;
    let got = fetch(&client(), &format!("{}/0", server.url()), deadline()).await;
    assert_eq!(got, Err(FetchError::Redirects));
}

#[tokio::test]
async fn five_hops_pass() {
    let mut server = mockito::Server::new_async().await;
    let mut mocks = Vec::new();
    for i in 0..5 {
        mocks.push(
            server
                .mock("GET", format!("/{i}").as_str())
                .with_status(302)
                .with_header("location", &format!("/{}", i + 1))
                .create_async()
                .await,
        );
    }
    let _end = server
        .mock("GET", "/5")
        .with_body("body")
        .create_async()
        .await;
    let got = fetch(&client(), &format!("{}/0", server.url()), deadline()).await;
    assert_eq!(got, Ok(Fetched::Torrent(b"body".to_vec())));
}

#[tokio::test]
async fn too_large() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("GET", "/big")
        .with_body(vec![b'x'; MAX_TORRENT_BYTES + 1])
        .create_async()
        .await;
    let got = fetch(&client(), &format!("{}/big", server.url()), deadline()).await;
    assert_eq!(got, Err(FetchError::TooLarge));
}

#[tokio::test]
async fn html_is_fetched() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("GET", "/login")
        .with_header("content-type", "text/html")
        .with_body("<html>log in</html>")
        .create_async()
        .await;
    let got = fetch(&client(), &format!("{}/login", server.url()), deadline()).await;
    assert_eq!(got, Ok(Fetched::Torrent(b"<html>log in</html>".to_vec())));
}

#[tokio::test]
async fn bad_status_and_scheme() {
    let mut server = mockito::Server::new_async().await;
    let _m = server
        .mock("GET", "/gone")
        .with_status(404)
        .create_async()
        .await;
    let got = fetch(&client(), &format!("{}/gone", server.url()), deadline()).await;
    assert_eq!(got, Err(FetchError::Status));
    let got = fetch(&client(), "ftp://example.com/x", deadline()).await;
    assert_eq!(got, Err(FetchError::Scheme));
}
