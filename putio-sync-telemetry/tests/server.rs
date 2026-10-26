//! Listener test: content type, 404, bind failure, and scrape latency under load.

use std::net::SocketAddr;
use std::time::{Duration, Instant};

use putio_sync_telemetry::server::{self, MetricsSettings, CONTENT_TYPE_TEXT};
use putio_sync_telemetry::Telemetry;

fn leak() -> &'static Telemetry {
    let t: &'static Telemetry = Box::leak(Box::new(Telemetry::new()));
    t.init(2, "1.2.3", "abc1234");
    t
}

async fn start(t: &'static Telemetry) -> SocketAddr {
    let settings = MetricsSettings {
        bind: "127.0.0.1:0".parse().unwrap(),
        ..MetricsSettings::default()
    };
    let listener = server::bind(&settings).await.unwrap();
    let addr = listener.local_addr().unwrap();
    server::serve(listener, &settings.path, t);
    addr
}

#[tokio::test]
async fn scrape_returns_text_format() {
    let addr = start(leak()).await;
    let resp = reqwest::get(format!("http://{}/metrics", addr))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(
        resp.headers()["content-type"].to_str().unwrap(),
        CONTENT_TYPE_TEXT
    );
    let body = resp.text().await.unwrap();
    assert!(body.contains("putio_sync_build_info{commit=\"abc1234\",version=\"1.2.3\"} 1"));
}

#[tokio::test]
async fn other_paths_are_not_found() {
    let addr = start(leak()).await;
    let resp = reqwest::get(format!("http://{}/nope", addr)).await.unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn bind_in_use_returns_err() {
    let taken = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let settings = MetricsSettings {
        bind: taken.local_addr().unwrap(),
        ..MetricsSettings::default()
    };
    assert!(server::bind(&settings).await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn scrape_is_fast_while_recording() {
    let t = leak();
    let addr = start(t).await;
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut writers = Vec::new();
    for _ in 0..2000 {
        let stop = stop.clone();
        writers.push(tokio::spawn(async move {
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                t.download_bytes(1);
                tokio::task::yield_now().await;
            }
        }));
    }
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Best of several scrapes, each while all 2000 writers record. A scrape that has to
    // wait for recording to stop never finishes here (the writers only stop afterwards),
    // and one that contends with every write takes far longer than the bound. A single
    // scrape also measures how long the 4 worker threads, shared with 2000 busy tasks,
    // take to reach the listener: under a load average of 11-30 (other test binaries in
    // parallel) that alone reached ~1.1 s. The best of five filters out those stalls,
    // and the 2 s bound leaves room for a machine that stays loaded throughout.
    let mut best = Duration::MAX;
    for _ in 0..5 {
        let started = Instant::now();
        let resp = reqwest::get(format!("http://{}/metrics", addr))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        resp.text().await.unwrap();
        best = best.min(started.elapsed());
    }

    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    for w in writers {
        w.await.unwrap();
    }
    assert!(
        best < Duration::from_secs(2),
        "fastest of 5 scrapes took {:?}",
        best
    );
}
