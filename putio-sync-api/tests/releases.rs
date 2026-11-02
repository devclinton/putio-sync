//! `GET /stats/releases` behind the Stats page's release tiles (042 US5), and the flags
//! and reports under `/releases` (042 US6), as in contracts/api.md.

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app, test_app_with, test_app_without_db, TestApp};
use putio_sync_core::config::ArrConfig;
use putio_sync_db::models::{DownloadSource, HistoryAction, Origin, SourceType};
use putio_sync_db::{CheckState, NewArrItem, NewCheck, NewReport, ReportKind};
use serde_json::{json, Value};

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
    assert_eq!(
        body["held"],
        json!({"now": 0, "found": 0, "ran_out": 0, "removed": 0, "median_found_secs": null})
    );
}

/// How holds ended (043 US5)
#[tokio::test]
async fn counts_how_holds_ended() {
    let app = test_app().await;
    let db = app.state.database.as_ref().unwrap();
    let now = chrono::Utc::now().timestamp();
    for (hash, result, held_for) in [
        ("a", "found_passed", 60),
        ("b", "found_rejected", 240),
        ("c", "ran_out", 300),
        ("d", "removed", 10),
        ("e", "found_passed", 120),
    ] {
        db.held()
            .insert(&putio_sync_db::NewHeld {
                hash,
                magnet: "magnet:?xt=urn:btih:x",
                source: "magnet",
                category: Some("sonarr"),
                watch_folder: None,
                check_id: None,
                last_lookup: Some("not_found"),
                held_at: now - held_for,
                // Not due, so the app's hold ticker leaves it to this test
                next_at: now + 300,
                until_at: now + 300,
            })
            .await
            .unwrap();
        db.held().finish(hash, result, now).await.unwrap();
    }
    let body = body_json(app.get("/stats/releases").await).await;
    assert_eq!(
        body["held"],
        json!({"now": 0, "found": 3, "ran_out": 1, "removed": 1, "median_found_secs": 120})
    );
    // The magnet never shows
    assert!(!body.to_string().contains("magnet:?"));
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

// ---- Flags (042 US6) --------------------------------------------------------------

const HASH: &str = "c12fe1c06bba254a9dc9f519b335aa7c1367a88a";

/// `GET /files/<id>` for one file
async fn putio_file(app: &mut TestApp, id: i64, name: &str, size: i64) -> mockito::Mock {
    app.mock_file(
        id,
        json!({ "status": "OK", "file": {
            "id": id, "name": name, "size": size, "file_type": "VIDEO", "parent_id": 0,
            "is_folder": false, "is_shared": false,
        }}),
    )
    .await
}

async fn flag(app: &TestApp, body: Value) -> (StatusCode, Value) {
    let response = app.post_json("/releases/flags", body).await;
    let status = response.status();
    (status, body_json(response).await)
}

/// The one report saved, as `/releases/reports` lists it
async fn only_report(app: &TestApp) -> Value {
    let rows = body_json(app.get("/releases/reports").await).await;
    assert_eq!(rows.as_array().unwrap().len(), 1, "{rows}");
    rows[0].clone()
}

#[tokio::test]
async fn flags_refuse_bad_bodies() {
    let app = test_app().await;
    let response = app
        .req(Method::POST, "/releases/flags", None, Some(json!({})))
        .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let long = "x".repeat(501);
    for body in [
        json!({ "target": { "path": "/x" }, "kind": "fake" }),
        json!({ "target": { "file_id": 1, "arr_hash": HASH }, "kind": "fake" }),
        json!({ "target": { "file_id": 1 }, "kind": "maybe" }),
        json!({ "target": { "file_id": 1 }, "kind": "fake", "note": long }),
        json!({ "target": { "file_id": 1 }, "kind": "not_fake", "blocklist": true }),
    ] {
        let (status, _) = flag(&app, body.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    }
    let (status, _) = flag(
        &app,
        json!({ "target": { "history_id": 9 }, "kind": "fake" }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = flag(
        &app,
        json!({ "target": { "arr_hash": HASH }, "kind": "fake" }),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        app.req(Method::DELETE, "/releases/flags/9", Some(&app.token), None)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn flag_a_file_fake_then_undo() {
    let mut app = test_app().await;
    let _file = putio_file(&mut app, 31, "Movie.2024.mkv", 1503238553).await;
    app.db()
        .sources()
        .record(
            31,
            &DownloadSource {
                transfer_id: Some(5),
                source_type: Some(SourceType::Torrent),
                tracker_host: Some("tracker.example.org".to_string()),
                is_private: true,
                origin: Origin::Watch,
                watch_folder: Some("/watch/movies".to_string()),
                putio_folder: None,
            },
        )
        .await
        .unwrap();

    let body = json!({ "target": { "file_id": 31 }, "kind": "fake", "note": "password .7z" });
    let (status, created) = flag(&app, body).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["kind"], "flagged_fake");
    assert_eq!(created["name"], "Movie.2024.mkv");
    assert_eq!(created["blocklisted"], false);

    let row = only_report(&app).await;
    assert_eq!(row["kind"], "flagged_fake");
    assert_eq!(row["file_id"], 31);
    assert_eq!(row["transfer_id"], 5);
    assert_eq!(row["source"], "watch");
    assert_eq!(row["watch_folder"], "/watch/movies");
    assert_eq!(row["stage"], Value::Null);
    assert_eq!(row["file_count"], 1);
    assert_eq!(row["total_size"], 1503238553_i64);
    assert_eq!(row["largest_wanted"], 1503238553_i64);
    assert_eq!(
        row["types"],
        json!([{ "ext": "mkv", "count": 1, "largest": 1503238553_i64 }])
    );
    assert_eq!(row["tracker_hosts"], json!(["tracker.example.org"]));
    assert_eq!(row["is_private"], true);
    assert_eq!(row["note"], "password .7z");

    // A second "Fake" replaces the note; the row on screen shows it
    let body = json!({ "target": { "file_id": 31 }, "kind": "fake", "note": "exe inside" });
    let (_, again) = flag(&app, body).await;
    assert_eq!(again["id"], created["id"]);
    let flags = body_json(app.get("/releases/flags?file_id=31&file_id=32").await).await;
    assert_eq!(
        flags,
        json!({ "flags": [{
            "id": created["id"], "kind": "flagged_fake", "target": { "file_id": 31 },
            "note": "exe inside",
        }], "arr_file_ids": [] })
    );

    // Not fake needs a rejection first
    let (status, _) = flag(
        &app,
        json!({ "target": { "file_id": 31 }, "kind": "not_fake" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    // Neither a file of no *arr item can be blocklisted
    let body = json!({ "target": { "file_id": 31 }, "kind": "fake", "blocklist": true });
    assert_eq!(flag(&app, body).await.0, StatusCode::BAD_REQUEST);

    let path = format!("/releases/flags/{}", created["id"]);
    let undo = app.req(Method::DELETE, &path, Some(&app.token), None).await;
    assert_eq!(undo.status(), StatusCode::NO_CONTENT);
    let flags = body_json(app.get("/releases/flags?file_id=31").await).await;
    assert_eq!(flags, json!({ "flags": [], "arr_file_ids": [] }));
    let undo = app.req(Method::DELETE, &path, Some(&app.token), None).await;
    assert_eq!(undo.status(), StatusCode::NOT_FOUND);
}

/// Put.io no longer has the file: the history row gives the traits
#[tokio::test]
async fn flag_a_history_row() {
    let app = test_app().await;
    let id = app
        .db()
        .history()
        .insert(
            41,
            "Show.S01E01.exe",
            1200,
            HistoryAction::Downloaded,
            Some(1),
            None,
        )
        .await
        .unwrap();
    let (status, created) = flag(
        &app,
        json!({ "target": { "history_id": id }, "kind": "fake" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let row = only_report(&app).await;
    assert_eq!(row["name"], "Show.S01E01.exe");
    assert_eq!(row["file_id"], 41);
    assert_eq!(
        row["types"],
        json!([{ "ext": "exe", "count": 1, "largest": 1200 }])
    );
    assert_eq!(row["largest_wanted"], 0);

    let path = format!("/releases/flags?history_id={id}");
    let flags = body_json(app.get(&path).await).await;
    assert_eq!(flags["flags"][0]["target"], json!({ "history_id": id }));
}

#[tokio::test]
async fn flag_a_rejection_not_fake() {
    let app = test_app().await;
    let rejected = app
        .db()
        .releases()
        .insert_report(&NewReport {
            kind: ReportKind::Rejected,
            source: Some("watch_torrent"),
            stage: Some("before"),
            name: "Movie.2024",
            hash: Some(HASH),
            total_size: Some(1000),
            types: r#"[{"ext":"mkv","count":1,"largest":1000}]"#,
            min_size: Some(52428800),
            reason: Some("small_wanted"),
            ..NewReport::default()
        })
        .await
        .unwrap();

    let target = json!({ "report_id": rejected });
    let (status, _) = flag(&app, json!({ "target": target, "kind": "fake" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, created) = flag(&app, json!({ "target": target, "kind": "not_fake" })).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["kind"], "flagged_not_fake");

    let rows = body_json(app.get("/releases/reports").await).await;
    let not_fake = &rows[0];
    assert_eq!(not_fake["kind"], "flagged_not_fake");
    assert_eq!(not_fake["hash"], HASH);
    assert_eq!(
        not_fake["types"],
        json!([{ "ext": "mkv", "count": 1, "largest": 1000 }])
    );
    assert_eq!(not_fake["min_size"], 52428800);
    assert_eq!(not_fake["stage"], Value::Null);

    let flags = body_json(
        app.get(&format!("/releases/flags?report_id={rejected}"))
            .await,
    )
    .await;
    assert_eq!(flags["flags"][0]["id"], created["id"]);

    // The rejection itself can't be undone
    let path = format!("/releases/flags/{rejected}");
    let undo = app.req(Method::DELETE, &path, Some(&app.token), None).await;
    assert_eq!(undo.status(), StatusCode::CONFLICT);

    // A bad .torrent has no hash: its check keeps the flag on it
    let check = app
        .db()
        .releases()
        .insert_check(&NewCheck {
            name: "Movie.2021",
            source: "watch_torrent",
            stage: "before",
            ..NewCheck::default()
        })
        .await
        .unwrap();
    let invalid = app
        .db()
        .releases()
        .insert_report(&NewReport {
            kind: ReportKind::Rejected,
            check_id: Some(check),
            source: Some("watch_torrent"),
            stage: Some("before"),
            name: "Movie.2021",
            reason: Some("invalid"),
            ..NewReport::default()
        })
        .await
        .unwrap();
    let target = json!({ "report_id": invalid });
    let (_, first) = flag(&app, json!({ "target": target, "kind": "not_fake" })).await;
    let (_, again) = flag(
        &app,
        json!({ "target": target, "kind": "not_fake", "note": "real" }),
    )
    .await;
    assert_eq!(first["id"], again["id"]);
    let path = format!("/releases/flags?report_id={invalid}&report_id={rejected}");
    let flags = body_json(app.get(&path).await).await;
    assert_eq!(flags["flags"].as_array().unwrap().len(), 2, "{flags}");
    assert_eq!(flags["flags"][0]["note"], "real");
}

async fn arr_app(dry_run: bool) -> TestApp {
    let app = test_app_with(|config| {
        config.arr = ArrConfig {
            enabled: true,
            password: Some("arr-password".to_string()),
            ..ArrConfig::default()
        };
        config.precheck.magnet_lookup = false;
        config.precheck.dry_run = dry_run;
    })
    .await;
    app.db()
        .arr()
        .insert(&NewArrItem {
            hash: HASH,
            name: "Movie.2024",
            category: "radarr",
            transfer_id: Some(5),
            putio_status: "COMPLETED",
            total_size: 1000,
        })
        .await
        .unwrap();
    sqlx::query("UPDATE arr_items SET file_id = 50")
        .execute(app.db().pool())
        .await
        .unwrap();
    app
}

async fn deletes(app: &mut TestApp, expect: usize) -> mockito::Mock {
    app.putio
        .mock("POST", "/files/delete")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"status":"OK"}"#)
        .expect(expect)
        .create_async()
        .await
}

#[tokio::test]
async fn fail_and_blocklist_under_dry_run_changes_nothing() {
    let mut app = arr_app(true).await;
    let _file = putio_file(&mut app, 50, "Movie.2024.mkv", 1000).await;
    let delete = deletes(&mut app, 0).await;
    let flags = body_json(app.get("/releases/flags?file_id=50&file_id=51").await).await;
    assert_eq!(flags["arr_file_ids"], json!([50]));
    let body = json!({ "target": { "file_id": 50 }, "kind": "fake", "blocklist": true });
    let (status, created) = flag(&app, body).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created["blocklisted"], false);
    assert_eq!(created["dry_run"], true);
    delete.assert_async().await;

    let item = app.db().arr().get_by_hash(HASH).await.unwrap().unwrap();
    assert_ne!(item.state, putio_sync_db::ArrState::Failed);
    let row = only_report(&app).await;
    assert_eq!(row["arr_hash"], HASH);
    assert_eq!(row["category"], "radarr");
}

#[tokio::test]
async fn fail_and_blocklist_fails_the_item_and_deletes_it() {
    let mut app = arr_app(false).await;
    let _file = putio_file(&mut app, 50, "Movie.2024.mkv", 1000).await;
    let delete = deletes(&mut app, 1).await;
    let body = json!({ "target": { "arr_hash": HASH }, "kind": "fake", "blocklist": true });
    let (status, created) = flag(&app, body).await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    assert_eq!(created.get("dry_run"), None);
    delete.assert_async().await;

    let item = app.db().arr().get_by_hash(HASH).await.unwrap().unwrap();
    assert_eq!(item.state, putio_sync_db::ArrState::Failed);
    assert_eq!(item.error.as_deref(), Some("flagged fake by the operator"));

    // A plain fake flag on the same item only records it
    let mut quiet = arr_app(false).await;
    let delete = deletes(&mut quiet, 0).await;
    let (status, _) = flag(
        &quiet,
        json!({ "target": { "arr_hash": HASH }, "kind": "fake" }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    delete.assert_async().await;
}

#[tokio::test]
async fn flags_query_is_bounded() {
    let app = test_app().await;
    let many: Vec<String> = (0..201).map(|i| format!("file_id={i}")).collect();
    let response = app
        .get(&format!("/releases/flags?{}", many.join("&")))
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response = app.get("/releases/flags?file_id=x").await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let flags = body_json(app.get("/releases/flags").await).await;
    assert_eq!(flags, json!({ "flags": [], "arr_file_ids": [] }));
}
