//! The daemon's metrics listener follows a published config (hot reload, part 3): a new
//! bind list, a new path, off and on again, all without a restart.

use putio_sync_cli::telemetry;
use putio_sync_core::config::Config;
use putio_sync_core::listen::BindList;
use std::net::SocketAddr;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn config(enabled: bool, port: u16, path: &str) -> Config {
    let mut c = Config::default();
    c.metrics.enabled = enabled;
    c.metrics.bind = BindList::from(format!("127.0.0.1:{port}").as_str());
    c.metrics.path = path.to_string();
    c
}

async fn status(port: u16, path: &str) -> Option<u16> {
    reqwest::Client::new()
        .get(format!("http://127.0.0.1:{port}{path}"))
        .timeout(Duration::from_secs(2))
        .send()
        .await
        .ok()
        .map(|r| r.status().as_u16())
}

async fn until(what: &str, mut ok: impl FnMut() -> bool) {
    for _ in 0..200 {
        if ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out waiting for {what}");
}

#[tokio::test]
async fn follows_bind_path_and_enabled() {
    let (a, b) = (free_port(), free_port());
    let dir = tempfile::tempdir().unwrap();
    let metrics = telemetry::start(
        &config(true, a, "/metrics"),
        Arc::new(OnceLock::new()),
        2,
        dir.path().to_path_buf(),
    )
    .await
    .unwrap();
    let bound = metrics.bound();
    let at = |p: u16| SocketAddr::from(([127, 0, 0, 1], p));
    assert_eq!(bound.get(), vec![at(a)]);
    assert_eq!(status(a, "/metrics").await, Some(200));

    // A new bind list moves it
    metrics.apply(&config(true, b, "/metrics")).await;
    until("the new port", || bound.get() == vec![at(b)]).await;
    assert_eq!(status(b, "/metrics").await, Some(200));
    assert_eq!(status(a, "/metrics").await, None);

    // A new path is served on the same addresses
    metrics.apply(&config(true, b, "/scrape")).await;
    until("the new path", || bound.get() == vec![at(b)]).await;
    assert_eq!(status(b, "/scrape").await, Some(200));
    assert_eq!(status(b, "/metrics").await, Some(404));

    // Off closes it; on opens it again
    metrics.apply(&config(false, b, "/scrape")).await;
    assert!(bound.get().is_empty());
    assert_eq!(status(b, "/scrape").await, None);
    metrics.apply(&config(true, a, "/scrape")).await;
    until("on again", || bound.get() == vec![at(a)]).await;
    assert_eq!(status(a, "/scrape").await, Some(200));

    // An invalid list changes nothing
    let mut bad = config(true, b, "/scrape");
    bad.metrics.bind = BindList::from("not an address");
    metrics.apply(&bad).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(bound.get(), vec![at(a)]);
}

#[tokio::test]
async fn off_at_start_turns_on_later() {
    let port = free_port();
    let dir = tempfile::tempdir().unwrap();
    let metrics = telemetry::start(
        &config(false, port, "/metrics"),
        Arc::new(OnceLock::new()),
        2,
        dir.path().to_path_buf(),
    )
    .await
    .unwrap();
    assert!(metrics.bound().get().is_empty());
    metrics.apply(&config(true, port, "/metrics")).await;
    let bound = metrics.bound();
    until("the listener", || !bound.get().is_empty()).await;
    assert_eq!(status(port, "/metrics").await, Some(200));
}
