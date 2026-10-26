//! The embedded web UI: `GET /` and `GET /assets/{file}` (feature 004, R2).

mod common;

use axum::body::Body;
use axum::http::{header, Method, Request, StatusCode};
use common::{body_text, test_app, TestApp};
use std::collections::BTreeSet;

/// Every `"/assets/..."` value quoted in `text`
fn asset_refs(text: &str) -> Vec<String> {
    text.split('"')
        .filter(|s| s.starts_with("/assets/"))
        .map(str::to_string)
        .collect()
}

/// Relative module imports (`from "./x.js"`, `from "../x.js"`) in `text`, resolved
/// against the asset path `base` (e.g. `/assets/pages/status.js`)
fn imports(base: &str, text: &str) -> Vec<String> {
    let dir = &base[..base.rfind('/').unwrap()];
    text.split("from \"")
        .skip(1)
        .filter_map(|rest| rest.split('"').next())
        .filter(|spec| spec.starts_with("./") || spec.starts_with("../"))
        .map(|spec| {
            let mut parts: Vec<&str> = dir.split('/').collect();
            for seg in spec.split('/') {
                match seg {
                    "." => {}
                    ".." => {
                        parts.pop();
                    }
                    s => parts.push(s),
                }
            }
            parts.join("/")
        })
        .collect()
}

async fn public_get(app: &TestApp, path: &str) -> axum::response::Response {
    app.req(Method::GET, path, None, None).await
}

/// Every asset reachable from the index, through script imports
async fn all_assets(app: &TestApp) -> BTreeSet<String> {
    let index = body_text(public_get(app, "/").await).await;
    let mut seen = BTreeSet::new();
    let mut todo = asset_refs(&index);
    while let Some(path) = todo.pop() {
        if !seen.insert(path.clone()) {
            continue;
        }
        if path.ends_with(".js") {
            let text = body_text(public_get(app, &path).await).await;
            todo.extend(imports(&path, &text));
        }
    }
    seen
}

#[tokio::test]
async fn index_served_public() {
    let app = test_app().await;
    let response = public_get(&app, "/").await;
    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response.headers()[header::CONTENT_TYPE].to_str().unwrap();
    assert!(content_type.starts_with("text/html"), "{content_type}");
    let body = body_text(response).await;
    assert!(body.contains("<script type=\"module\""), "{body}");
    assert!(body.contains("<noscript>"), "{body}");
}

#[tokio::test]
async fn every_referenced_asset_resolves() {
    let app = test_app().await;
    let assets = all_assets(&app).await;
    for page in [
        "status",
        "queue",
        "putio",
        "transfers",
        "history",
        "settings",
        "login",
    ] {
        let path = format!("/assets/pages/{page}.js");
        assert!(assets.contains(&path), "{path} not reachable: {assets:?}");
    }
    assert!(assets.contains("/assets/app.css"), "{assets:?}");

    for path in &assets {
        let response = public_get(&app, path).await;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        let content_type = response.headers()[header::CONTENT_TYPE].to_str().unwrap();
        let expected = if path.ends_with(".css") {
            "text/css"
        } else {
            "text/javascript"
        };
        assert!(content_type.starts_with(expected), "{path}: {content_type}");
    }
}

#[tokio::test]
async fn unknown_asset_404() {
    let app = test_app().await;
    for path in [
        "/assets/nope.js",
        "/assets/../Cargo.toml",
        "/assets/index.html",
    ] {
        let response = public_get(&app, path).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
}

#[tokio::test]
async fn etag_304() {
    let app = test_app().await;
    for path in ["/", "/assets/app.js"] {
        let first = public_get(&app, path).await;
        let etag = first.headers()[header::ETAG].clone();
        assert_eq!(
            etag.to_str().unwrap(),
            format!("\"{}\"", env!("CARGO_PKG_VERSION"))
        );

        let request = Request::builder()
            .uri(path)
            .header(header::IF_NONE_MATCH, etag.clone())
            .body(Body::empty())
            .unwrap();
        let second = app.send(request).await;
        assert_eq!(second.status(), StatusCode::NOT_MODIFIED, "{path}");
        assert_eq!(second.headers()[header::ETAG], etag);
        assert!(body_text(second).await.is_empty());
    }
}

#[tokio::test]
async fn no_cache_header() {
    let app = test_app().await;
    for path in ["/", "/assets/app.css"] {
        let response = public_get(&app, path).await;
        assert_eq!(
            response.headers()[header::CACHE_CONTROL],
            "no-cache",
            "{path}"
        );
    }
}

#[tokio::test]
async fn assets_total_under_150kb() {
    let app = test_app().await;
    let mut total = body_text(public_get(&app, "/").await).await.len();
    for path in all_assets(&app).await {
        total += body_text(public_get(&app, &path).await).await.len();
    }
    assert!(total < 150 * 1024, "UI is {total} bytes");
}

#[tokio::test]
async fn index_has_no_inline_script() {
    let app = test_app().await;
    let body = body_text(public_get(&app, "/").await).await;
    for chunk in body.split("<script").skip(1) {
        let open_tag = chunk.split('>').next().unwrap();
        assert!(
            open_tag.contains("src="),
            "inline script: <script{open_tag}>"
        );
    }
}
