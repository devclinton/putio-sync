//! The first request of every NAS sync failed because the client reused a pooled connection
//! that a middlebox had silently dropped while idle (≈250/day in the live logs). The test
//! server mimics that drop: it keeps the socket open, then resets it as soon as the client
//! writes to it again.

use putio_sync_client::PutioApiClient;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

const BODY: &str = r#"{"files":[],"parent":{"id":0,"name":"Root","size":0,"file_type":"FOLDER","parent_id":0,"is_folder":true,"is_shared":false},"total":0,"status":"OK"}"#;

#[derive(Default)]
struct Counters {
    connections: AtomicUsize,
    stale_writes: AtomicUsize,
}

async fn read_request(stream: &mut TcpStream) -> bool {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 1024];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return false,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
    true
}

async fn serve(listener: TcpListener, counters: Arc<Counters>) {
    loop {
        let (mut stream, _) = listener.accept().await.unwrap();
        counters.connections.fetch_add(1, Ordering::SeqCst);
        let counters = counters.clone();
        tokio::spawn(async move {
            if !read_request(&mut stream).await {
                return;
            }
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: keep-alive\r\n\r\n{}",
                BODY.len(),
                BODY
            );
            stream.write_all(response.as_bytes()).await.unwrap();

            // The connection is now "dead" in the network: any further request on it gets a reset.
            let mut byte = [0u8; 1];
            if matches!(stream.read(&mut byte).await, Ok(n) if n > 0) {
                counters.stale_writes.fetch_add(1, Ordering::SeqCst);
            }
        });
    }
}

#[tokio::test]
async fn idle_pooled_connection_is_not_reused() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let counters = Arc::new(Counters::default());
    tokio::spawn(serve(listener, counters.clone()));

    let client = PutioApiClient::with_base_url("test_token".to_string(), format!("http://{addr}"))
        .with_pool_idle_timeout(Duration::from_millis(200));

    client.list_files(0).await.expect("first request");
    tokio::time::sleep(Duration::from_millis(600)).await;
    client
        .list_files(0)
        .await
        .expect("second request after idle");

    assert_eq!(
        counters.stale_writes.load(Ordering::SeqCst),
        0,
        "client wrote to an idle connection instead of opening a fresh one"
    );
    assert_eq!(counters.connections.load(Ordering::SeqCst), 2);
}
