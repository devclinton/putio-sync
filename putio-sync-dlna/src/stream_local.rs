//! Streaming media bytes to players (feature 022): local files through `ServeFile`, and the
//! byte counting shared with the Put.io proxy.

use crate::media;
use crate::metrics;
use axum::body::Body;
use axum::http::{
    header, HeaderMap, HeaderName, HeaderValue, Method, Request, Response, StatusCode,
};
use bytes::Bytes;
use futures_util::Stream;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;
use tower::ServiceExt;
use tower_http::services::ServeFile;

/// Streams being sent right now, across both root kinds
pub type Active = Arc<AtomicUsize>;

/// What is kept about one open stream; its end is logged and counted when it is dropped
pub struct StreamGuard {
    root_kind: &'static str,
    root: String,
    version: Option<&'static str>,
    size: Option<u64>,
    started: Instant,
    bytes: u64,
    finished: bool,
    failed: bool,
    active: Active,
}

impl StreamGuard {
    pub fn start(
        root_kind: &'static str,
        root: &str,
        version: Option<&'static str>,
        size: Option<u64>,
        active: Active,
    ) -> StreamGuard {
        active.fetch_add(1, Ordering::Relaxed);
        metrics::stream_started(root_kind);
        StreamGuard {
            root_kind,
            root: root.to_string(),
            version,
            size,
            started: Instant::now(),
            bytes: 0,
            finished: false,
            failed: false,
            active,
        }
    }
}

impl Drop for StreamGuard {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::Relaxed);
        metrics::stream_ended(self.root_kind);
        let result = if self.failed {
            "error"
        } else if self.finished {
            "done"
        } else {
            "client_closed"
        };
        tracing::info!(
            root_kind = self.root_kind,
            root = %self.root,
            version = self.version.unwrap_or("-"),
            size = self.size.unwrap_or(0),
            bytes_sent = self.bytes,
            duration_ms = self.started.elapsed().as_millis() as u64,
            result,
            "DLNA stream ended"
        );
    }
}

/// A body stream that counts what it sends
pub struct Counted<S> {
    inner: Pin<Box<S>>,
    guard: StreamGuard,
}

impl<S> Counted<S> {
    pub fn new(inner: S, guard: StreamGuard) -> Self {
        Counted {
            inner: Box::pin(inner),
            guard,
        }
    }
}

impl<S, E> Stream for Counted<S>
where
    S: Stream<Item = Result<Bytes, E>>,
    E: std::error::Error + Send + Sync + 'static,
{
    type Item = Result<Bytes, E>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = &mut *self;
        match this.inner.as_mut().poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                this.guard.bytes += chunk.len() as u64;
                metrics::bytes_sent(this.guard.root_kind, chunk.len() as u64);
                Poll::Ready(Some(Ok(chunk)))
            }
            Poll::Ready(Some(Err(e))) => {
                this.guard.failed = true;
                Poll::Ready(Some(Err(e)))
            }
            Poll::Ready(None) => {
                this.guard.finished = true;
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

fn dlna_headers(headers: &mut HeaderMap, mime: &str) {
    if let Ok(v) = HeaderValue::from_str(mime) {
        headers.insert(header::CONTENT_TYPE, v);
    }
    headers.insert(
        HeaderName::from_static("transfermode.dlna.org"),
        HeaderValue::from_static("Streaming"),
    );
    headers.insert(
        HeaderName::from_static("contentfeatures.dlna.org"),
        HeaderValue::from_static(media::content_features()),
    );
    headers.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
}

/// The headers every media answer carries, for the Put.io proxy
pub fn media_headers(headers: &mut HeaderMap, mime: &str) {
    dlna_headers(headers, mime);
}

/// An empty response with a status (refusals send no body)
pub fn empty(status: StatusCode) -> Response<Body> {
    let mut r = Response::new(Body::empty());
    *r.status_mut() = status;
    r
}

/// Send a file that `content::local_path` has already checked
pub async fn serve(
    path: PathBuf,
    root: &str,
    method: &Method,
    request_headers: &HeaderMap,
    mime: &str,
    active: Active,
) -> Response<Body> {
    let mut builder = Request::builder().method(method.clone()).uri("/");
    for name in [header::RANGE, header::IF_RANGE] {
        if let Some(v) = request_headers.get(&name) {
            builder = builder.header(name, v);
        }
    }
    let Ok(request) = builder.body(Body::empty()) else {
        return empty(StatusCode::BAD_REQUEST);
    };
    let mime_parsed: mime_guess::mime::Mime = mime
        .parse()
        .unwrap_or(mime_guess::mime::APPLICATION_OCTET_STREAM);
    let response = match ServeFile::new_with_mime(&path, &mime_parsed)
        .oneshot(request)
        .await
    {
        Ok(r) => r,
        Err(_) => return empty(StatusCode::INTERNAL_SERVER_ERROR),
    };
    let (mut parts, body) = response.into_parts();
    if parts.status == StatusCode::NOT_FOUND {
        return empty(StatusCode::NOT_FOUND);
    }
    dlna_headers(&mut parts.headers, mime);
    let counted = *method == Method::GET
        && matches!(parts.status, StatusCode::OK | StatusCode::PARTIAL_CONTENT);
    if !counted {
        return Response::from_parts(parts, Body::new(body));
    }
    let size = parts
        .headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok());
    let guard = StreamGuard::start("local", root, None, size, active);
    let stream = Counted::new(Body::new(body).into_data_stream(), guard);
    Response::from_parts(parts, Body::from_stream(stream))
}
