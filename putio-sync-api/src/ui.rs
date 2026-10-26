//! Embedded web UI (feature 004, R2)
//!
//! The files under `putio-sync-api/ui/` are compiled into the binary and served as they
//! are on disk, with no build step:
//! - GET  /               - `index.html`
//! - GET  /assets/{file}  - stylesheet and ES modules
//!
//! Both routes are public: the page holds only the sign-in form and rendering code;
//! every piece of data behind it comes from protected routes.

use axum::{
    extract::Path,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};

const HTML: &str = "text/html; charset=utf-8";
const CSS: &str = "text/css; charset=utf-8";
const JS: &str = "text/javascript; charset=utf-8";

/// The page itself, served at `/`
const INDEX: &[u8] = include_bytes!("../ui/index.html");

/// `(path under /assets/, content type, bytes)`
const ASSETS: &[(&str, &str, &[u8])] = &[
    ("app.css", CSS, include_bytes!("../ui/app.css")),
    ("app.js", JS, include_bytes!("../ui/app.js")),
    ("api.js", JS, include_bytes!("../ui/api.js")),
    ("format.js", JS, include_bytes!("../ui/format.js")),
    ("pages/login.js", JS, include_bytes!("../ui/pages/login.js")),
    (
        "pages/status.js",
        JS,
        include_bytes!("../ui/pages/status.js"),
    ),
    ("pages/queue.js", JS, include_bytes!("../ui/pages/queue.js")),
    ("pages/putio.js", JS, include_bytes!("../ui/pages/putio.js")),
    (
        "pages/transfers.js",
        JS,
        include_bytes!("../ui/pages/transfers.js"),
    ),
    (
        "pages/history.js",
        JS,
        include_bytes!("../ui/pages/history.js"),
    ),
    (
        "pages/settings.js",
        JS,
        include_bytes!("../ui/pages/settings.js"),
    ),
];

/// The ETag of every asset: the crate version, so a new release is never served stale
const ETAG: &str = concat!("\"", env!("CARGO_PKG_VERSION"), "\"");

/// GET /
pub async fn index(headers: HeaderMap) -> Response {
    serve(&headers, HTML, INDEX)
}

/// GET /assets/{file}
pub async fn asset(Path(path): Path<String>, headers: HeaderMap) -> Response {
    match ASSETS.iter().find(|(name, _, _)| *name == path) {
        Some((_, content_type, bytes)) => serve(&headers, content_type, bytes),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

fn serve(headers: &HeaderMap, content_type: &'static str, bytes: &'static [u8]) -> Response {
    let cache = [
        (header::CACHE_CONTROL, HeaderValue::from_static("no-cache")),
        (header::ETAG, HeaderValue::from_static(ETAG)),
    ];
    let fresh = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|tag| tag.trim() == ETAG));
    if fresh {
        return (StatusCode::NOT_MODIFIED, cache).into_response();
    }
    (
        [(header::CONTENT_TYPE, HeaderValue::from_static(content_type))],
        cache,
        bytes,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_paths_are_unique() {
        for (i, (a, _, _)) in ASSETS.iter().enumerate() {
            assert!(
                ASSETS[i + 1..].iter().all(|(b, _, _)| a != b),
                "duplicate asset {a}"
            );
        }
    }
}
