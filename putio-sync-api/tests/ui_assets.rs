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
        "files",
        "transfers",
        "history",
        "stats",
        "doctor",
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
        let text = etag.to_str().unwrap();
        let hex = text.trim_matches('"');
        assert!(
            text.len() == hex.len() + 2 && hex.len() == 16,
            "{path}: {text}"
        );
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()), "{path}: {text}");

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

/// The ETag is a hash of each asset's content, not the crate version, so a rebuild
/// with changed files is never served stale
#[tokio::test]
async fn etag_follows_content() {
    let app = test_app().await;
    let mut tags = BTreeSet::new();
    let mut paths = all_assets(&app).await;
    paths.insert("/".to_string());
    for path in &paths {
        let first = public_get(&app, path).await;
        let second = public_get(&app, path).await;
        let tag = first.headers()[header::ETAG].to_str().unwrap().to_string();
        assert_eq!(second.headers()[header::ETAG], tag.as_str(), "{path}");
        assert_ne!(tag, format!("\"{}\"", env!("CARGO_PKG_VERSION")), "{path}");
        tags.insert(tag);
    }
    assert_eq!(tags.len(), paths.len(), "every asset has its own ETag");
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
async fn assets_total_under_480kb() {
    let app = test_app().await;
    let mut total = body_text(public_get(&app, "/").await).await.len();
    for path in all_assets(&app).await {
        total += body_text(public_get(&app, &path).await).await.len();
    }
    assert!(total < 480 * 1024, "UI is {total} bytes");
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

// ---- Theme contrast (feature 012) ---------------------------------------------

/// `#rrggbb` → relative luminance (WCAG 2)
fn luminance(hex: &str) -> f64 {
    let channel = |i: usize| {
        let v = f64::from(u8::from_str_radix(&hex[i..i + 2], 16).unwrap()) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(1) + 0.7152 * channel(3) + 0.0722 * channel(5)
}

fn contrast(a: &str, b: &str) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Every `--name: light-dark(#light, #dark)` in the rules of `css` for `:root` and
/// `[data-accent="..."]`, keyed by selector; a rule with a selector list counts for each
fn theme_rules(
    css: &str,
) -> std::collections::BTreeMap<String, std::collections::BTreeMap<String, [String; 2]>> {
    let mut rules = std::collections::BTreeMap::new();
    for block in css.split('}') {
        let Some((selector, body)) = block.split_once('{') else {
            continue;
        };
        let selector = selector.rsplit("*/").next().unwrap();
        let mut tokens = std::collections::BTreeMap::new();
        for decl in body.split(';') {
            let Some((name, value)) = decl.split_once(':') else {
                continue;
            };
            let Some(args) = value.trim().strip_prefix("light-dark(") else {
                continue;
            };
            let (light, dark) = args.trim_end_matches(')').split_once(',').unwrap();
            let (light, dark) = (light.trim(), dark.trim());
            if light.len() == 7 && light.starts_with('#') && dark.len() == 7 {
                let name = name.trim().trim_start_matches("--").to_string();
                tokens.insert(name, [light.to_string(), dark.to_string()]);
            }
        }
        for part in selector.split(',').map(str::trim) {
            if part == ":root" || part.starts_with("[data-accent=") {
                let rule: &mut std::collections::BTreeMap<_, _> =
                    rules.entry(part.to_string()).or_default();
                rule.extend(tokens.clone());
            }
        }
    }
    rules
}

#[tokio::test]
async fn theme_contrast_meets_aa() {
    let app = test_app().await;
    let css = body_text(public_get(&app, "/assets/app.css").await).await;
    let rules = theme_rules(&css);
    let root = rules.get(":root").expect(":root tokens");
    let accents: Vec<&String> = rules.keys().filter(|k| *k != ":root").collect();
    assert!(accents.len() >= 6, "accents: {accents:?}");

    let mut failures = Vec::new();
    for selector in std::iter::once(&":root".to_string()).chain(accents) {
        let mut theme = root.clone();
        theme.extend(rules[selector].clone());
        for (mode, i) in [("light", 0), ("dark", 1)] {
            let get = |name: &str| -> &str {
                &theme
                    .get(name)
                    .unwrap_or_else(|| panic!("{selector} has no --{name}"))[i]
            };
            let mut pairs = Vec::new();
            for fg in ["text", "muted", "accent", "ok", "warn", "bad"] {
                for bg in ["bg", "surface", "surface-2"] {
                    pairs.push((fg, bg));
                }
            }
            pairs.push(("on-accent", "accent"));
            for (fg, bg) in pairs {
                let ratio = contrast(get(fg), get(bg));
                if ratio < 4.5 {
                    failures.push(format!("{selector} {mode}: --{fg} on --{bg} = {ratio:.2}"));
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "below WCAG AA:\n{}",
        failures.join("\n")
    );
}
