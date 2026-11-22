//! A hardened HTTP accept loop shared by the API, DLNA and metrics listeners.
//!
//! `axum::serve` has no connection cap and no header-read timeout, so a client that opens
//! many idle or trickling connections can use up the file descriptors. This loop caps open
//! connections and gives each request a time limit to send its headers. It sets no limit on
//! the whole request or response, so long video streams are not cut off.

use axum::extract::connect_info::ConnectInfo;
use axum::Router;
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use hyper_util::server::conn::auto::Builder;
use hyper_util::server::graceful::GracefulShutdown;
use hyper_util::service::TowerToHyperService;
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;

/// Open connections allowed per listener
pub const MAX_CONNECTIONS: usize = 512;
/// How long a client has to send the headers of a request (also bounds idle keep-alive)
pub const HEADER_READ_TIMEOUT: Duration = Duration::from_secs(30);
/// How long open connections get to finish after a stop is asked
const SHUTDOWN_GRACE: Duration = Duration::from_secs(5);

/// Limits for [`serve_limited`]
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// Most connections open at once; more wait in the accept queue
    pub max_connections: usize,
    /// Longest a client may take to send request headers
    pub header_read_timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_connections: MAX_CONNECTIONS,
            header_read_timeout: HEADER_READ_TIMEOUT,
        }
    }
}

/// Serve `app` on `listener` until `shutdown` resolves. Handlers can take
/// `ConnectInfo<SocketAddr>`.
pub async fn serve_limited(
    listener: TcpListener,
    app: Router,
    limits: Limits,
    shutdown: impl Future<Output = ()>,
) {
    let permits = Arc::new(Semaphore::new(limits.max_connections.max(1)));
    let graceful = GracefulShutdown::new();
    tokio::pin!(shutdown);
    loop {
        // Take a slot first, so a flood waits in the kernel queue instead of using fds
        let permit = tokio::select! {
            p = permits.clone().acquire_owned() => match p {
                Ok(p) => p,
                Err(_) => break,
            },
            _ = &mut shutdown => break,
        };
        let (stream, peer) = tokio::select! {
            r = listener.accept() => match r {
                Ok(v) => v,
                Err(e) => {
                    tracing::debug!(error = %e, "accept failed");
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
            },
            _ = &mut shutdown => break,
        };
        let _ = stream.set_nodelay(true);
        let service = TowerToHyperService::new(
            app.clone()
                .layer(axum::Extension(ConnectInfo::<SocketAddr>(peer))),
        );
        let mut builder = Builder::new(TokioExecutor::new());
        builder
            .http1()
            .timer(TokioTimer::new())
            .header_read_timeout(limits.header_read_timeout);
        builder.http2().timer(TokioTimer::new());
        let conn = graceful.watch(
            builder
                .serve_connection_with_upgrades(TokioIo::new(stream), service)
                .into_owned(),
        );
        tokio::spawn(async move {
            let _ = conn.await;
            drop(permit);
        });
    }
    drop(listener);
    let _ = tokio::time::timeout(SHUTDOWN_GRACE, graceful.shutdown()).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::routing::get;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    async fn start(limits: Limits) -> (SocketAddr, tokio::sync::watch::Sender<bool>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, mut rx) = tokio::sync::watch::channel(false);
        let app = Router::new().route(
            "/",
            get(|ConnectInfo(p): ConnectInfo<SocketAddr>| async move { p.ip().to_string() }),
        );
        tokio::spawn(serve_limited(listener, app, limits, async move {
            let _ = rx.wait_for(|s| *s).await;
        }));
        (addr, tx)
    }

    async fn get_ok(addr: SocketAddr) -> String {
        let mut s = TcpStream::connect(addr).await.unwrap();
        s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).await.unwrap();
        out
    }

    #[tokio::test]
    async fn serves_with_connect_info() {
        let (addr, _stop) = start(Limits::default()).await;
        let out = get_ok(addr).await;
        assert!(out.starts_with("HTTP/1.1 200"), "{out}");
        assert!(out.ends_with("127.0.0.1"), "{out}");
    }

    #[tokio::test]
    async fn a_client_that_never_finishes_its_headers_is_dropped() {
        let (addr, _stop) = start(Limits {
            max_connections: 8,
            header_read_timeout: Duration::from_millis(300),
        })
        .await;
        let mut s = TcpStream::connect(addr).await.unwrap();
        s.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n").await.unwrap();
        let mut buf = Vec::new();
        let read = tokio::time::timeout(Duration::from_secs(5), s.read_to_end(&mut buf)).await;
        assert!(read.is_ok(), "the server kept a stalled connection open");
    }

    #[tokio::test]
    async fn connections_over_the_cap_wait_for_a_free_slot() {
        let (addr, _stop) = start(Limits {
            max_connections: 1,
            header_read_timeout: Duration::from_secs(30),
        })
        .await;
        // The first connection holds the only slot without sending anything
        let _held = TcpStream::connect(addr).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
        let second = tokio::time::timeout(Duration::from_millis(500), get_ok(addr)).await;
        assert!(second.is_err(), "the cap did not hold");
        drop(_held);
        let third = tokio::time::timeout(Duration::from_secs(5), get_ok(addr)).await;
        assert!(third.is_ok(), "the slot was not freed");
    }
}
