//! `GET /stats/releases` behind the Stats page's release tiles (042 US5, contracts/api.md).

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app, test_app_without_db, TestApp};
use putio_sync_db::{CheckState, NewCheck, NewReport, ReportKind};
use serde_json::json;

const GB: i64 = 1024 * 1024 * 1024;

async fn check(app: &TestApp, check: NewCheck<'_>) -> i64 {
    app.db().releases().insert_check(&check).await.unwrap()
}

/// What Put.io downloaded for a fallback, as `resolve_check` records it
async fn putio_bytes(app: &TestApp, id: i64, bytes: i64) {
    sqlx::query("UPDATE release_checks SET putio_bytes = ? WHERE id = ?")
        .bind(bytes)
        .bind(id)
        .execute(app.db().pool())
        .await
        .unwrap();
}

async fn report(app: &TestApp, kind: ReportKind, types: &str, trackers: &str) {
    app.db()
        .releases()
        .insert_report(&NewReport {
            kind,
            name: "Movie.2024",
            types,
            tracker_hosts: trackers,
            ..NewReport::default()
        })
        .await
        .unwrap();
}

/// A rejection before Put.io, a fake magnet fallback, a real fallback, a gone fallback,
/// a magnet found in a cache, a passed .torrent, and one reject from last month
async fn seed(app: &TestApp) {
    let rejected = |source, stage, reason, size| NewCheck {
        name: "Movie.2024",
        source,
        stage,
        state: Some(CheckState::Rejected),
        reason: Some(reason),
        total_size: size,
        ..NewCheck::default()
    };
    check(app, rejected("torrent", "before", "no_wanted", 2 * GB)).await;
    check(app, rejected("watch_torrent", "before", "small_wanted", GB)).await;
    let fake = check(
        app,
        NewCheck {
            lookup: Some("not_found"),
            ..rejected("magnet", "putio", "no_wanted", 3 * GB)
        },
    )
    .await;
    putio_bytes(app, fake, 3 * GB).await;
    check(
        app,
        NewCheck {
            lookup: Some("timeout"),
            ..rejected("watch_magnet", "putio", "gone", 0)
        },
    )
    .await;
    let real = check(
        app,
        NewCheck {
            name: "Show.S01",
            source: "magnet",
            stage: "putio",
            state: Some(CheckState::Passed),
            lookup: Some("not_found"),
            ..NewCheck::default()
        },
    )
    .await;
    putio_bytes(app, real, GB).await;
    check(
        app,
        NewCheck {
            name: "Show.S02",
            source: "magnet",
            stage: "before",
            state: Some(CheckState::Passed),
            lookup: Some("found"),
            lookup_cache: Some("itorrents.net"),
            ..NewCheck::default()
        },
    )
    .await;
    check(
        app,
        NewCheck {
            name: "Show.S03",
            source: "torrent",
            stage: "before",
            state: Some(CheckState::Passed),
            ..NewCheck::default()
        },
    )
    .await;
    let old = check(app, rejected("url", "before", "invalid", 5 * GB)).await;
    sqlx::query("UPDATE release_checks SET checked_at = datetime('now', '-20 days'), resolved_at = datetime('now', '-20 days') WHERE id = ?")
        .bind(old)
        .execute(app.db().pool())
        .await
        .unwrap();

    let exe = r#"[{"ext":"exe","count":1,"largest":100},{"ext":"txt","count":1,"largest":1}]"#;
    report(app, ReportKind::Rejected, exe, r#"["tracker.example.org"]"#).await;
    report(
        app,
        ReportKind::Rejected,
        r#"[{"ext":"EXE","count":2,"largest":9}]"#,
        "[]",
    )
    .await;
    report(
        app,
        ReportKind::FlaggedFake,
        r#"[{"ext":"7z","count":1,"largest":9}]"#,
        r#"["tracker.example.org"]"#,
    )
    .await;
    // A wrong rejection's types don't count as a fake's
    report(
        app,
        ReportKind::FlaggedNotFake,
        r#"[{"ext":"mkv","count":1,"largest":9}]"#,
        "[]",
    )
    .await;
}

#[tokio::test]
async fn requires_token() {
    let app = test_app().await;
    let response = app.req(Method::GET, "/stats/releases", None, None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn rejects_a_bad_range() {
    let app = test_app().await;
    for query in ["range=2d", "tz=abc"] {
        let response = app.get(&format!("/stats/releases?{query}")).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{query}");
        assert_eq!(body_json(response).await["error"], "Invalid query");
    }
}

#[tokio::test]
async fn without_a_database_is_an_error() {
    let app = test_app_without_db().await;
    let response = app.get("/stats/releases").await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn empty_database_has_every_section() {
    let app = test_app().await;
    let body = body_json(app.get("/stats/releases").await).await;
    assert_eq!(body["range"]["preset"], "7d");
    assert!(body["range"]["from"].is_i64());
    assert_eq!(
        body["rejected"],
        json!({"count": 0, "bytes": 0,
               "by_stage": {"before": {"count": 0, "bytes": 0}, "putio": {"count": 0, "bytes": 0}},
               "by_reason": []})
    );
    assert_eq!(
        body["magnets"],
        json!({"checked": 0, "found": 0, "fell_back": 0, "fallback_fake": 0,
               "fallback_putio_bytes": 0, "caches": []})
    );
    assert_eq!(
        body["reports"],
        json!({"rejected": 0, "flagged_fake": 0, "flagged_not_fake": 0,
               "top_types": [], "top_trackers": []})
    );
}

#[tokio::test]
async fn counts_the_week() {
    let app = test_app().await;
    seed(&app).await;
    let response = app.get("/stats/releases?range=7d&tz=60").await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_json(response).await;

    // Rejections on Put.io count their size as avoided on the NAS
    assert_eq!(
        body["rejected"],
        json!({"count": 4, "bytes": 6 * GB,
               "by_stage": {"before": {"count": 2, "bytes": 3 * GB},
                            "putio": {"count": 2, "bytes": 3 * GB}},
               "by_reason": [{"reason": "no_wanted", "count": 2},
                             {"reason": "gone", "count": 1},
                             {"reason": "small_wanted", "count": 1}]})
    );
    // A gone fallback isn't a fake; a passed one cost Put.io bytes but isn't counted
    assert_eq!(
        body["magnets"],
        json!({"checked": 4, "found": 1, "fell_back": 3, "fallback_fake": 1,
               "fallback_putio_bytes": 3 * GB,
               "caches": [
                   {"cache": "itorrents.net", "found": 1, "not_found": 0, "timeout": 0,
                    "wrong_hash": 0, "error": 0},
                   {"cache": null, "found": 0, "not_found": 2, "timeout": 1,
                    "wrong_hash": 0, "error": 0}]})
    );
    assert_eq!(
        body["reports"],
        json!({"rejected": 2, "flagged_fake": 1, "flagged_not_fake": 1,
               "top_types": [{"ext": "exe", "count": 2}, {"ext": "7z", "count": 1},
                             {"ext": "txt", "count": 1}],
               "top_trackers": [{"host": "tracker.example.org", "count": 2}]})
    );
}

#[tokio::test]
async fn longer_range_takes_older_checks() {
    let app = test_app().await;
    seed(&app).await;
    let body = body_json(app.get("/stats/releases?range=30d").await).await;
    assert_eq!(body["rejected"]["count"], 5);
    assert_eq!(body["rejected"]["bytes"], 11 * GB);
    let body = body_json(app.get("/stats/releases?range=all").await).await;
    assert_eq!(body["range"]["from"], serde_json::Value::Null);
    assert_eq!(body["rejected"]["count"], 5);
}

// ---- GET /releases/reports --------------------------------------------------

#[tokio::test]
async fn reports_require_token_and_a_valid_query() {
    let app = test_app().await;
    let response = app.req(Method::GET, "/releases/reports", None, None).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    for query in ["range=2d", "format=xml"] {
        let response = app.get(&format!("/releases/reports?{query}")).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{query}");
    }
}

#[tokio::test]
async fn reports_as_json_newest_first() {
    let app = test_app().await;
    seed(&app).await;
    let response = app.get("/releases/reports?range=7d").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-disposition"],
        "attachment; filename=\"putio-sync-releases-7d.json\""
    );
    let body = body_json(response).await;
    let rows = body.as_array().unwrap();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0]["kind"], "flagged_not_fake");
    assert_eq!(rows[3]["kind"], "rejected");
    assert_eq!(
        rows[3]["types"],
        json!([{"ext": "exe", "count": 1, "largest": 100}, {"ext": "txt", "count": 1, "largest": 1}])
    );
    assert_eq!(rows[3]["tracker_hosts"], json!(["tracker.example.org"]));
    assert_eq!(rows[3]["is_private"], false);
    assert!(rows[3]["created_at"].is_string());
}

#[tokio::test]
async fn reports_as_csv() {
    let app = test_app().await;
    app.db()
        .releases()
        .insert_report(&NewReport {
            kind: ReportKind::Rejected,
            source: Some("torrent"),
            stage: Some("before"),
            name: "=Movie, \"2024\"",
            types: r#"[{"ext":"exe","count":1,"largest":1503238553},{"ext":"txt","count":1,"largest":120}]"#,
            tracker_hosts: r#"["a.example","b.example"]"#,
            reason: Some("no_wanted"),
            ..NewReport::default()
        })
        .await
        .unwrap();
    let response = app.get("/releases/reports?format=csv").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-disposition"],
        "attachment; filename=\"putio-sync-releases-all.csv\""
    );
    let text = common::body_text(response).await;
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].starts_with("id,kind,check_id,"));
    assert!(lines[1].contains(",rejected,"), "{}", lines[1]);
    assert!(
        lines[1].contains(",\"'=Movie, \"\"2024\"\"\","),
        "{}",
        lines[1]
    );
    assert!(
        lines[1].contains(",exe:1:1503238553;txt:1:120,"),
        "{}",
        lines[1]
    );
    assert!(lines[1].contains(",a.example;b.example,"), "{}", lines[1]);
}
