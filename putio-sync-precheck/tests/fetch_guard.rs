//! A .torrent URL may not lead to this machine's own services or to cloud metadata (F-04);
//! the private LAN stays reachable because indexers live there.

use putio_sync_core::net_guard::Reach;
use putio_sync_precheck::fetch::{self, FetchError};
use tokio::time::{Duration, Instant};

fn soon() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

#[tokio::test]
async fn a_loopback_url_is_refused_by_default() {
    let mut server = mockito::Server::new_async().await;
    let hit = server
        .mock("GET", "/a.torrent")
        .with_body("d4:infod4:name1:xee")
        .expect(0)
        .create_async()
        .await;
    let url = format!("{}/a.torrent", server.url());
    let client = fetch::client_with(Reach::Lan);
    let error = fetch::fetch_with(&client, &url, soon(), Reach::Lan)
        .await
        .unwrap_err();
    assert_eq!(error, FetchError::Blocked);
    hit.assert_async().await;
}

#[tokio::test]
async fn a_redirect_to_metadata_or_loopback_is_refused_on_each_hop() {
    let mut server = mockito::Server::new_async().await;
    for (path, to) in [
        ("/meta", "http://169.254.169.254/latest/meta-data/"),
        ("/v6", "http://[fd00:ec2::254]/"),
        ("/own", "http://127.0.0.1:9/admin"),
    ] {
        let _hop = server
            .mock("GET", path)
            .with_status(302)
            .with_header("location", to)
            .create_async()
            .await;
        // The first host is let in (Local reach for loopback), the hop is checked again
        let client = fetch::client_with(Reach::Local);
        let url = format!("{}{}", server.url(), path);
        let error = fetch::fetch_with(&client, &url, soon(), Reach::Local)
            .await
            .unwrap_err();
        let want = if path == "/own" {
            // Local reach allows loopback, so this one reaches the closed port
            FetchError::Request
        } else {
            FetchError::Blocked
        };
        assert_eq!(error, want, "{path}");
    }
}
