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
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::OnceLock;

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
    ("select.js", JS, include_bytes!("../ui/select.js")),
    ("theme.js", JS, include_bytes!("../ui/theme.js")),
    ("rules.js", JS, include_bytes!("../ui/rules.js")),
    ("pages/login.js", JS, include_bytes!("../ui/pages/login.js")),
    (
        "pages/status.js",
        JS,
        include_bytes!("../ui/pages/status.js"),
    ),
    (
        "pages/active.js",
        JS,
        include_bytes!("../ui/pages/active.js"),
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
    ("pages/stats.js", JS, include_bytes!("../ui/pages/stats.js")),
    (
        "pages/doctor.js",
        JS,
        include_bytes!("../ui/pages/doctor.js"),
    ),
    (
        "pages/settings.js",
        JS,
        include_bytes!("../ui/pages/settings.js"),
    ),
];

/// `"<16 hex digits>"`: a hash of `bytes`, so the ETag changes whenever the embedded
/// file does, including between builds of the same version
fn etag(bytes: &[u8]) -> HeaderValue {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    HeaderValue::from_str(&format!("\"{:016x}\"", hasher.finish()))
        .expect("hex digits are a valid header value")
}

/// ETags of `INDEX` and then each of `ASSETS`, computed once on first use
fn etags() -> &'static [HeaderValue] {
    static ETAGS: OnceLock<Vec<HeaderValue>> = OnceLock::new();
    ETAGS.get_or_init(|| {
        std::iter::once(INDEX)
            .chain(ASSETS.iter().map(|(_, _, bytes)| *bytes))
            .map(etag)
            .collect()
    })
}

/// GET /
pub async fn index(headers: HeaderMap) -> Response {
    serve(&headers, HTML, INDEX, &etags()[0])
}

/// GET /assets/{file}
pub async fn asset(Path(path): Path<String>, headers: HeaderMap) -> Response {
    match ASSETS.iter().position(|(name, _, _)| *name == path) {
        Some(i) => {
            let (_, content_type, bytes) = ASSETS[i];
            serve(&headers, content_type, bytes, &etags()[i + 1])
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

fn serve(
    headers: &HeaderMap,
    content_type: &'static str,
    bytes: &'static [u8],
    etag: &HeaderValue,
) -> Response {
    let cache = [
        (header::CACHE_CONTROL, HeaderValue::from_static("no-cache")),
        (header::ETAG, etag.clone()),
    ];
    let tag = etag.to_str().unwrap_or_default();
    let fresh = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|t| t.trim() == tag));
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

    #[test]
    fn etag_changes_with_content() {
        assert_eq!(etag(b"body { }"), etag(b"body { }"));
        assert_ne!(etag(b"body { }"), etag(b"body {}"));
        assert_eq!(etags().len(), ASSETS.len() + 1);
    }
}
