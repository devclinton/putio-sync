//! The web UI's language catalogs (feature 051): one per language in `ui/i18n.js`, served
//! from `/assets/i18n/<code>.json`, made from `ui/i18n/en.json` and covering most of it.

mod common;

use axum::http::{header, Method, StatusCode};
use common::{body_text, test_app, TestApp};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// SC-003: the share of en.json each catalog must translate
const MIN_COVERAGE: f64 = 0.9;

fn ui_dir() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/ui"))
}

fn english() -> BTreeMap<String, String> {
    let text = std::fs::read_to_string(ui_dir().join("i18n/en.json")).unwrap();
    serde_json::from_str(&text).expect("en.json is an object of strings")
}

async fn get(app: &TestApp, path: &str) -> axum::response::Response {
    app.req(Method::GET, path, None, None).await
}

/// The codes in `LANGUAGES = [...]` in the served i18n.js
async fn languages(app: &TestApp) -> Vec<String> {
    let js = body_text(get(app, "/assets/i18n.js").await).await;
    let list = js
        .split("LANGUAGES = [")
        .nth(1)
        .unwrap()
        .split(']')
        .next()
        .unwrap();
    list.split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .collect()
}

/// `{0}`, `{1}`... in `text`
fn holes(text: &str) -> BTreeSet<String> {
    text.match_indices('{')
        .filter_map(|(i, _)| {
            let rest = &text[i + 1..];
            let end = rest.find('}')?;
            rest[..end]
                .chars()
                .all(|c| c.is_ascii_digit())
                .then(|| text[i..i + end + 2].to_string())
        })
        .collect()
}

/// Languages not written in Latin script, where any English word left in is untranslated
const NON_LATIN: &[&str] = &[
    "ar", "he", "hi", "ja", "ko", "ru", "th", "uk", "zh", "zh-TW",
];

/// `out` is the English `en` unchanged, and `en` has two lower-case words in a row (or, for a
/// language not written in Latin script, any lower-case word)
fn sent_back(code: &str, en: &str, out: &str) -> bool {
    if en != out {
        return false;
    }
    if NON_LATIN.contains(&code) {
        return en
            .split(|c: char| !c.is_ascii_lowercase())
            .any(|w| w.len() >= 3);
    }
    en.split_whitespace()
        .collect::<Vec<_>>()
        .windows(2)
        .any(|w| {
            w.iter()
                .all(|word| word.len() > 1 && word.chars().all(|c| c.is_ascii_lowercase()))
        })
}

#[test]
fn english_maps_each_string_to_itself() {
    let en = english();
    assert!(en.len() > 500, "{} strings", en.len());
    for (key, value) in &en {
        assert_eq!(key, value);
    }
}

#[tokio::test]
async fn every_language_has_a_served_catalog() {
    let app = test_app().await;
    let en = english();
    for code in languages(&app).await {
        let path = format!("/assets/i18n/{code}.json");
        let response = get(&app, &path).await;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        let content_type = response.headers()[header::CONTENT_TYPE].to_str().unwrap();
        assert!(
            content_type.starts_with("application/json"),
            "{path}: {content_type}"
        );
        let catalog: BTreeMap<String, String> = serde_json::from_str(&body_text(response).await)
            .unwrap_or_else(|e| panic!("{path}: {e}"));

        let unknown: Vec<_> = catalog.keys().filter(|k| !en.contains_key(*k)).collect();
        assert!(
            unknown.is_empty(),
            "{path} has strings en.json lacks: {unknown:?}"
        );
        // A translation that lost a {n} is not used, and words sent back in English (the
        // service had no model) aren't translated, so neither counts
        let usable = catalog
            .iter()
            .filter(|(k, v)| {
                !v.trim().is_empty() && holes(k) == holes(v) && !sent_back(&code, k, v)
            })
            .count();
        let coverage = usable as f64 / en.len() as f64;
        assert!(
            coverage >= MIN_COVERAGE,
            "{path} translates {usable} of {} strings; run `make i18n-translate`",
            en.len()
        );
    }
}

#[tokio::test]
async fn catalog_files_match_the_language_list() {
    let app = test_app().await;
    let mut listed: BTreeSet<String> = languages(&app).await.into_iter().collect();
    listed.insert("en".into());
    let on_disk: BTreeSet<String> = std::fs::read_dir(ui_dir().join("i18n"))
        .unwrap()
        .map(|e| {
            let name = e.unwrap().file_name().into_string().unwrap();
            name.strip_suffix(".json")
                .expect("only .json in ui/i18n")
                .to_string()
        })
        .collect();
    assert_eq!(on_disk, listed);
}

/// translate.js is imported only for another language, so the size budget doesn't count it
#[tokio::test]
async fn translator_is_served() {
    let app = test_app().await;
    let response = get(&app, "/assets/translate.js").await;
    assert_eq!(response.status(), StatusCode::OK);
    let i18n = body_text(get(&app, "/assets/i18n.js").await).await;
    assert!(i18n.contains("import(\"./translate.js\")"));
}

#[tokio::test]
async fn english_needs_no_catalog() {
    let app = test_app().await;
    let response = get(&app, "/assets/i18n/en.json").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
