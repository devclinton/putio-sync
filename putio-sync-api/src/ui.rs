//! Embedded web UI (feature 004, R2)
//!
//! The files under `putio-sync-api/ui/` are compiled into the binary and served as they
//! are on disk, with no build step:
//! - GET  /               - `index.html`
//! - GET  /assets/{file}  - stylesheet, ES modules and language catalogs (feature 051)
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
const JSON: &str = "application/json; charset=utf-8";

/// The page itself, served at `/`
const INDEX: &[u8] = include_bytes!("../ui/index.html");

/// A language's catalog (feature 051), fetched only when that language is in use
macro_rules! catalog {
    ($code:literal) => {
        (
            concat!("i18n/", $code, ".json"),
            JSON,
            include_bytes!(concat!("../ui/i18n/", $code, ".json")),
        )
    };
}

/// `(path under /assets/, content type, bytes)`
const ASSETS: &[(&str, &str, &[u8])] = &[
    ("app.css", CSS, include_bytes!("../ui/app.css")),
    ("app.js", JS, include_bytes!("../ui/app.js")),
    ("api.js", JS, include_bytes!("../ui/api.js")),
    ("format.js", JS, include_bytes!("../ui/format.js")),
    ("select.js", JS, include_bytes!("../ui/select.js")),
    ("query.js", JS, include_bytes!("../ui/query.js")),
    ("listctl.js", JS, include_bytes!("../ui/listctl.js")),
    ("theme.js", JS, include_bytes!("../ui/theme.js")),
    ("i18n.js", JS, include_bytes!("../ui/i18n.js")),
    ("translate.js", JS, include_bytes!("../ui/translate.js")),
    ("rules.js", JS, include_bytes!("../ui/rules.js")),
    ("flags.js", JS, include_bytes!("../ui/flags.js")),
    ("storage.js", JS, include_bytes!("../ui/storage.js")),
    ("tailscale.js", JS, include_bytes!("../ui/tailscale.js")),
    ("bandwidth.js", JS, include_bytes!("../ui/bandwidth.js")),
    ("network.js", JS, include_bytes!("../ui/network.js")),
    (
        "media_server.js",
        JS,
        include_bytes!("../ui/media_server.js"),
    ),
    ("media_text.js", JS, include_bytes!("../ui/media_text.js")),
    ("organise.js", JS, include_bytes!("../ui/organise.js")),
    (
        "organise_text.js",
        JS,
        include_bytes!("../ui/organise_text.js"),
    ),
    ("stream_mode.js", JS, include_bytes!("../ui/stream_mode.js")),
    ("hooks.js", JS, include_bytes!("../ui/hooks.js")),
    ("timeline.js", JS, include_bytes!("../ui/timeline.js")),
    ("plugins.js", JS, include_bytes!("../ui/plugins.js")),
    ("notify.js", JS, include_bytes!("../ui/notify.js")),
    ("verify.js", JS, include_bytes!("../ui/verify.js")),
    (
        "transcription.js",
        JS,
        include_bytes!("../ui/transcription.js"),
    ),
    ("subtitles.js", JS, include_bytes!("../ui/subtitles.js")),
    (
        "subtitle_modes.js",
        JS,
        include_bytes!("../ui/subtitle_modes.js"),
    ),
    (
        "settings/media.js",
        JS,
        include_bytes!("../ui/settings/media.js"),
    ),
    ("translation.js", JS, include_bytes!("../ui/translation.js")),
    ("pages/login.js", JS, include_bytes!("../ui/pages/login.js")),
    (
        "pages/signin.js",
        JS,
        include_bytes!("../ui/pages/signin.js"),
    ),
    ("signin.js", JS, include_bytes!("../ui/signin.js")),
    ("signin_text.js", JS, include_bytes!("../ui/signin_text.js")),
    (
        "settings/signin.js",
        JS,
        include_bytes!("../ui/settings/signin.js"),
    ),
    ("pages/setup.js", JS, include_bytes!("../ui/pages/setup.js")),
    ("pages/news.js", JS, include_bytes!("../ui/pages/news.js")),
    (
        "pages/update.js",
        JS,
        include_bytes!("../ui/pages/update.js"),
    ),
    ("pages/logs.js", JS, include_bytes!("../ui/pages/logs.js")),
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
    ("pages/files.js", JS, include_bytes!("../ui/pages/files.js")),
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
    (
        "settings/sections.js",
        JS,
        include_bytes!("../ui/settings/sections.js"),
    ),
    (
        "settings/form.js",
        JS,
        include_bytes!("../ui/settings/form.js"),
    ),
    (
        "settings/general.js",
        JS,
        include_bytes!("../ui/settings/general.js"),
    ),
    (
        "settings/putio.js",
        JS,
        include_bytes!("../ui/settings/putio.js"),
    ),
    (
        "settings/downloads.js",
        JS,
        include_bytes!("../ui/settings/downloads.js"),
    ),
    (
        "settings/filters.js",
        JS,
        include_bytes!("../ui/settings/filters.js"),
    ),
    (
        "settings/integrations.js",
        JS,
        include_bytes!("../ui/settings/integrations.js"),
    ),
    (
        "settings/plugins.js",
        JS,
        include_bytes!("../ui/settings/plugins.js"),
    ),
    (
        "settings/notifications.js",
        JS,
        include_bytes!("../ui/settings/notifications.js"),
    ),
    (
        "settings/plugin_install.js",
        JS,
        include_bytes!("../ui/settings/plugin_install.js"),
    ),
    (
        "settings/registry.js",
        JS,
        include_bytes!("../ui/settings/registry.js"),
    ),
    (
        "settings/workbench.js",
        JS,
        include_bytes!("../ui/settings/workbench.js"),
    ),
    ("workbench.js", JS, include_bytes!("../ui/workbench.js")),
    (
        "settings/subtitles.js",
        JS,
        include_bytes!("../ui/settings/subtitles.js"),
    ),
    (
        "settings/translation.js",
        JS,
        include_bytes!("../ui/settings/translation.js"),
    ),
    (
        "settings/arr.js",
        JS,
        include_bytes!("../ui/settings/arr.js"),
    ),
    (
        "settings/advanced.js",
        JS,
        include_bytes!("../ui/settings/advanced.js"),
    ),
    catalog!("ar"),
    catalog!("da"),
    catalog!("de"),
    catalog!("es"),
    catalog!("fi"),
    catalog!("fr"),
    catalog!("he"),
    catalog!("hi"),
    catalog!("id"),
    catalog!("it"),
    catalog!("ja"),
    catalog!("ko"),
    catalog!("ms"),
    catalog!("nb"),
    catalog!("nl"),
    catalog!("pl"),
    catalog!("pt"),
    catalog!("pt-PT"),
    catalog!("ru"),
    catalog!("sv"),
    catalog!("th"),
    catalog!("tr"),
    catalog!("uk"),
    catalog!("vi"),
    catalog!("zh"),
    catalog!("zh-TW"),
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
