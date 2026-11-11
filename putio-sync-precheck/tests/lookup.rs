//! Fetching a .torrent from an indexer URL (T009) and looking magnets up in caches (T013)

mod common;

use std::time::Duration;

use common::{file, multi, MB};
use putio_sync_precheck::lookup::{self, Lookup, Missed};
use putio_sync_precheck::metainfo::info_hash;
use putio_sync_telemetry::LookupResult;

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

// --- Cache lookups (T013) ---

fn release() -> (Vec<u8>, String) {
    let bytes = multi("Show.S01E03", vec![file(&["Show.S01E03.mkv"], 2048 * MB)]);
    let hash = info_hash(&bytes).unwrap().0;
    (bytes, hash)
}

fn cache(server: &mockito::ServerGuard, path: &str) -> String {
    format!("{}{path}", server.url())
}

const LIMIT: Duration = Duration::from_secs(10);

#[tokio::test]
async fn found_in_the_first_cache() {
    let (bytes, hash) = release();
    let mut server = mockito::Server::new_async().await;
    let m = server
        .mock(
            "GET",
            format!("/torrent/{}.torrent", hash.to_uppercase()).as_str(),
        )
        .match_header("accept-encoding", "identity")
        .match_header("user-agent", "putio-sync")
        .match_header("cookie", mockito::Matcher::Missing)
        .match_header("referer", mockito::Matcher::Missing)
        .with_header("content-type", "application/x-bittorrent")
        .with_body(bytes)
        .expect(1)
        .create_async()
        .await;
    let caches = vec![cache(&server, "/torrent/{HASH}.torrent")];
    let got = lookup::lookup(&lookup::client(), &hash, &caches, LIMIT).await;
    let Lookup::Found { meta, cache } = got else {
        panic!("expected found, got {got:?}");
    };
    assert_eq!(meta.hash, hash);
    assert_eq!(cache, "127.0.0.1");
    m.assert_async().await;
}

#[tokio::test]
async fn a_404_then_the_second_cache() {
    let (bytes, hash) = release();
    let mut server = mockito::Server::new_async().await;
    let first = server
        .mock("GET", format!("/a/{hash}").as_str())
        .with_status(404)
        .expect(1)
        .create_async()
        .await;
    let second = server
        .mock("GET", format!("/b/{hash}").as_str())
        .with_body(bytes)
        .expect(1)
        .create_async()
        .await;
    let caches = vec![cache(&server, "/a/{hash}"), cache(&server, "/b/{hash}")];
    let got = lookup::lookup(&lookup::client(), &hash, &caches, LIMIT).await;
    assert!(matches!(got, Lookup::Found { .. }), "{got:?}");
    first.assert_async().await;
    second.assert_async().await;
}

#[tokio::test]
async fn html_gzip_and_wrong_hash_are_skipped() {
    let (_, hash) = release();
    let other = multi("Other", vec![file(&["other.mkv"], 2048 * MB)]);
    let mut server = mockito::Server::new_async().await;
    let html = server
        .mock("GET", "/html")
        .with_body("<html>captcha</html>")
        .expect(1)
        .create_async()
        .await;
    let gzip = server
        .mock("GET", "/gzip")
        .with_body([0x1f, 0x8b, 8, 0, 0, 0])
        .expect(1)
        .create_async()
        .await;
    let wrong = server
        .mock("GET", "/wrong")
        .with_body(other)
        .expect(1)
        .create_async()
        .await;
    let caches = vec![
        cache(&server, "/html"),
        cache(&server, "/gzip"),
        cache(&server, "/wrong"),
    ];
    let got = lookup::lookup(&lookup::client(), &hash, &caches, LIMIT).await;
    assert_eq!(got, Lookup::Missed(Missed::Failed(LookupResult::WrongHash)));
    html.assert_async().await;
    gzip.assert_async().await;
    wrong.assert_async().await;

    let got = lookup::lookup(&lookup::client(), &hash, &caches[..2], LIMIT).await;
    assert_eq!(got, Lookup::Missed(Missed::Failed(LookupResult::Error)));
}

/// A server that accepts connections and never answers
async fn hanging() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            held.push(socket);
        }
    });
    format!("http://{addr}/{{HASH}}")
}

#[tokio::test]
async fn slow_cache_times_out_within_the_limit() {
    let (_, hash) = release();
    let mut server = mockito::Server::new_async().await;
    let after = server
        .mock("GET", mockito::Matcher::Any)
        .expect(0)
        .create_async()
        .await;
    let caches = vec![hanging().await, cache(&server, "/x/{hash}")];
    // Build the client before the clock starts: loading TLS roots is slow on a busy machine
    let http = lookup::client();
    let started = std::time::Instant::now();
    let got = lookup::lookup(&http, &hash, &caches, Duration::from_secs(1)).await;
    assert_eq!(got, Lookup::Missed(Missed::Failed(LookupResult::Timeout)));
    // The hanging cache never answers, so any bound shows the limit was kept
    assert!(started.elapsed() < Duration::from_secs(3));
    after.assert_async().await;
}

#[tokio::test]
async fn off_makes_no_request() {
    use putio_sync_core::config::PrecheckConfig;
    use putio_sync_core::rules::RuleSet;
    use putio_sync_precheck::Prechecker;

    let (_, hash) = release();
    let mut server = mockito::Server::new_async().await;
    let none = server
        .mock("GET", mockito::Matcher::Any)
        .expect(0)
        .create_async()
        .await;
    let config = PrecheckConfig {
        magnet_lookup: false,
        caches: vec![cache(&server, "/{hash}")],
        ..PrecheckConfig::default()
    };
    let checker = Prechecker::new(
        config,
        RuleSet::from_config(&putio_sync_core::config::DownloadConfig::default()),
    );
    assert_eq!(
        checker.lookup(&hash, LIMIT).await,
        Lookup::Missed(Missed::Off)
    );
    // Under a second left: a timeout, unasked
    let on = Prechecker::new(
        PrecheckConfig {
            caches: vec![cache(&server, "/{hash}")],
            ..PrecheckConfig::default()
        },
        RuleSet::from_config(&putio_sync_core::config::DownloadConfig::default()),
    );
    assert_eq!(
        on.lookup(&hash, Duration::from_millis(500)).await,
        Lookup::Missed(Missed::Failed(LookupResult::Timeout))
    );
    none.assert_async().await;
}
