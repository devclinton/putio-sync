//! The local files routes (feature 044): roots, listing, delete, rename and re-queue, and
//! that none of them reach outside the configured folders.

mod common;

use axum::http::{Method, StatusCode};
use common::{body_json, test_app, test_app_with, TestApp};
use putio_sync_file_manager::calculate_string_checksum;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// App whose watching is on, with `watch` as its one watch path
async fn watch_app(watch: &Path) -> TestApp {
    let watch = watch.to_path_buf();
    test_app_with(move |config| {
        config.watch.enabled = true;
        config.watch.paths = vec![watch];
    })
    .await
}

fn downloads(app: &TestApp) -> PathBuf {
    app._dir.path().to_path_buf()
}

async fn delete(app: &TestApp, query: &str) -> axum::response::Response {
    app.req(
        Method::DELETE,
        &format!("/local/files/downloads?{query}"),
        Some(&app.token),
        None,
    )
    .await
}

fn names(listing: &Value) -> Vec<String> {
    listing["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["name"].as_str().unwrap().to_string())
        .collect()
}

fn entry<'a>(listing: &'a Value, name: &str) -> &'a Value {
    listing["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["name"] == name)
        .unwrap_or_else(|| panic!("{name} not in {listing}"))
}

// ---- US1: roots and listing ---------------------------------------------------

#[tokio::test]
async fn roots_follow_watch_setting() {
    let app = test_app().await;
    let body = body_json(app.get("/local/files").await).await;
    let roots = body["roots"].as_array().unwrap();
    assert_eq!(roots.len(), 1, "{body}");
    assert_eq!(roots[0]["key"], "downloads");
    assert_eq!(roots[0]["exists"], true);
    assert_eq!(roots[0]["watch"], false);

    let watch = tempfile::tempdir().unwrap();
    let missing = watch.path().join("sonarr");
    let app = test_app_with({
        let paths = vec![watch.path().to_path_buf(), missing.clone()];
        move |config| {
            config.watch.enabled = true;
            config.watch.paths = paths;
        }
    })
    .await;
    let body = body_json(app.get("/local/files").await).await;
    let roots = body["roots"].as_array().unwrap();
    assert_eq!(roots.len(), 3, "{body}");
    assert_eq!(roots[1]["key"], "watch-0");
    assert_eq!(roots[1]["watch"], true);
    assert_eq!(roots[2]["label"], "Watch: sonarr");
    assert_eq!(roots[2]["exists"], false);

    // A missing root says so rather than failing
    let response = app.get("/local/files/watch-1").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    // Watching off hides the watch roots, even when paths are set
    let app = test_app_with({
        let path = watch.path().to_path_buf();
        move |config| config.watch.paths = vec![path]
    })
    .await;
    let response = app.get("/local/files/watch-0").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn listing_marks_partials_and_rejected() {
    let watch = tempfile::tempdir().unwrap();
    std::fs::write(watch.path().join("x.torrent.rejected"), b"d4:infoe").unwrap();
    let app = watch_app(watch.path()).await;
    let root = downloads(&app);
    std::fs::create_dir(root.join("sonarr")).unwrap();
    std::fs::write(root.join("sonarr/.Show.S01E01.mkv.downloading"), b"12").unwrap();
    std::fs::write(root.join("sonarr/Done.mkv"), b"12345").unwrap();
    std::fs::create_dir(root.join("sonarr/Season 1")).unwrap();

    let body = body_json(app.get("/local/files/downloads?path=sonarr").await).await;
    assert_eq!(body["path"], "sonarr");
    assert_eq!(body["truncated"], false);
    assert_eq!(
        names(&body),
        ["Season 1", ".Show.S01E01.mkv.downloading", "Done.mkv"]
    );
    let partial = entry(&body, ".Show.S01E01.mkv.downloading");
    assert_eq!(partial["partial"], "Show.S01E01.mkv");
    assert_eq!(partial["path"], "sonarr/.Show.S01E01.mkv.downloading");
    assert_eq!(partial["in_use"], false);
    let done = entry(&body, "Done.mkv");
    assert_eq!(done["kind"], "file");
    assert_eq!(done["size"], 5);
    assert!(done["modified"].as_str().unwrap().ends_with('Z'), "{done}");
    assert_eq!(entry(&body, "Season 1")["kind"], "dir");

    let body = body_json(app.get("/local/files/watch-0").await).await;
    assert_eq!(entry(&body, "x.torrent.rejected")["rejected"], "x.torrent");
}

#[tokio::test]
async fn listing_shows_active_download() {
    let app = test_app().await;
    let root = downloads(&app);
    std::fs::create_dir(root.join("radarr")).unwrap();
    std::fs::write(root.join("radarr/.Movie.mkv.downloading"), b"1").unwrap();
    let _claim = app
        .state
        .download_manager
        .claim_path(&root.join("radarr/Movie.mkv"))
        .await;

    let body = body_json(app.get("/local/files/downloads?path=radarr").await).await;
    assert_eq!(entry(&body, ".Movie.mkv.downloading")["in_use"], true);
    let body = body_json(app.get("/local/files/downloads").await).await;
    assert_eq!(entry(&body, "radarr")["in_use"], true);

    // FR-005: neither the partial nor the folder holding it can be deleted
    for query in ["path=radarr/.Movie.mkv.downloading", "path=radarr"] {
        let response = delete(&app, query).await;
        assert_eq!(response.status(), StatusCode::CONFLICT, "{query}");
    }
    assert!(root.join("radarr/.Movie.mkv.downloading").exists());
}

#[tokio::test]
async fn database_files_cannot_be_deleted_or_renamed() {
    // The test app's database lives in the download folder, as it can on a NAS
    let app = test_app().await;
    let root = downloads(&app);
    // The journal only exists mid-transaction; a stray one is still the database's
    std::fs::write(root.join(".putiosync.db-journal"), b"").unwrap();

    let body = body_json(app.get("/local/files/downloads").await).await;
    for name in [
        ".putiosync.db",
        ".putiosync.db-wal",
        ".putiosync.db-journal",
    ] {
        assert_eq!(entry(&body, name)["in_use"], true, "{name}");
        let response = delete(&app, &format!("path={name}")).await;
        assert_eq!(response.status(), StatusCode::CONFLICT, "delete {name}");
        let response = app
            .post_json(
                "/local/files/downloads/rename",
                json!({"path": name, "name": "moved"}),
            )
            .await;
        assert_eq!(response.status(), StatusCode::CONFLICT, "rename {name}");
        assert!(root.join(name).exists(), "{name}");
    }
    // Files beside it are not affected
    assert_eq!(entry(&body, "config.toml")["in_use"], false);
}

#[tokio::test]
async fn paths_outside_the_root_refused() {
    let app = test_app().await;
    let root = downloads(&app);
    std::fs::create_dir(root.join("sub")).unwrap();
    for query in [
        "path=..",
        "path=../",
        "path=sub/../..",
        "path=%2Fetc",
        "path=.%2Fsub",
        "path=sub%2F%2E%2E%2F%2E%2E",
        "path=sub%5C..%5C..",
    ] {
        let response = app.get(&format!("/local/files/downloads?{query}")).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "list {query}");
        let response = delete(&app, query).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "delete {query}");
    }
    let response = app.get("/local/files/nope").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    // A file is not a folder to list
    std::fs::write(root.join("f.mkv"), b"1").unwrap();
    let response = app.get("/local/files/downloads?path=f.mkv").await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[cfg(unix)]
#[tokio::test]
async fn symlinks_that_escape_are_not_followed() {
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.mkv"), b"keep").unwrap();
    let app = test_app().await;
    let root = downloads(&app);
    std::os::unix::fs::symlink(outside.path(), root.join("escape")).unwrap();

    let body = body_json(app.get("/local/files/downloads").await).await;
    assert_eq!(entry(&body, "escape")["kind"], "link");

    for path in ["escape", "escape/"] {
        let response = app
            .get(&format!("/local/files/downloads?path={path}"))
            .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{path}");
    }
    let response = delete(&app, "path=escape/secret.mkv").await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response = app
        .post_json(
            "/local/files/downloads/rename",
            json!({"path": "escape/secret.mkv", "name": "gone.mkv"}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);

    // Deleting the link removes the link only
    let response = delete(&app, "path=escape").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!root.join("escape").exists());
    assert!(outside.path().join("secret.mkv").exists());
}

#[tokio::test]
async fn routes_need_the_token() {
    let app = test_app().await;
    for (method, path, body) in [
        (Method::GET, "/local/files", None),
        (Method::GET, "/local/files/downloads", None),
        (Method::GET, "/local/files/downloads/summary?path=x", None),
        (Method::DELETE, "/local/files/downloads?path=x", None),
        (
            Method::POST,
            "/local/files/downloads/rename",
            Some(json!({"path": "x", "name": "y"})),
        ),
        (
            Method::POST,
            "/local/files/downloads/requeue",
            Some(json!({"path": "x"})),
        ),
    ] {
        let response = app.req(method.clone(), path, None, body).await;
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{method} {path}"
        );
    }
}

// ---- US2: delete --------------------------------------------------------------

#[tokio::test]
async fn delete_file_and_folder() {
    let app = test_app().await;
    let root = downloads(&app);
    std::fs::write(root.join("left.mkv"), b"1").unwrap();
    std::fs::create_dir_all(root.join("sonarr/Show/Season 1")).unwrap();
    std::fs::write(root.join("sonarr/Show/Season 1/e1.mkv"), b"123").unwrap();
    std::fs::write(root.join("sonarr/Show/e0.nfo"), b"12").unwrap();

    let body = body_json(
        app.get("/local/files/downloads/summary?path=sonarr/Show")
            .await,
    )
    .await;
    assert_eq!(
        body,
        json!({"files": 2, "folders": 2, "bytes": 5, "in_use": false})
    );

    let response = delete(&app, "path=left.mkv").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!root.join("left.mkv").exists());

    let response = delete(&app, "path=sonarr/Show").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await["message"], "Deleted sonarr/Show");
    assert!(!root.join("sonarr/Show").exists());
    assert!(root.join("sonarr").exists());

    let response = delete(&app, "path=left.mkv").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn root_itself_cannot_be_deleted() {
    let app = test_app().await;
    for query in ["path=", "", "path=%2F"] {
        let response = delete(&app, query).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{query:?}");
    }
    let response = app.get("/local/files/downloads/summary").await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(downloads(&app).exists());
}

// ---- US3: re-queue ------------------------------------------------------------

#[tokio::test]
async fn requeue_rejected_magnet() {
    let watch = tempfile::tempdir().unwrap();
    let magnet = "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567";
    std::fs::write(
        watch.path().join("x.magnet.rejected"),
        format!("{magnet}\n"),
    )
    .unwrap();
    let app = watch_app(watch.path()).await;
    let checksum = calculate_string_checksum(magnet);
    let processed = app.db().processed_torrents();
    processed
        .record_processed("x.magnet", &checksum, 10, Some(magnet))
        .await
        .unwrap();

    let response = app
        .post_json(
            "/local/files/watch-0/requeue",
            json!({"path": "x.magnet.rejected"}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await["path"], "x.magnet");
    assert!(watch.path().join("x.magnet").exists());
    assert!(!watch.path().join("x.magnet.rejected").exists());
    assert!(!processed.is_processed(&checksum).await.unwrap());
}

#[tokio::test]
async fn requeue_refusals() {
    let watch = tempfile::tempdir().unwrap();
    std::fs::write(watch.path().join("a.torrent.rejected"), b"new").unwrap();
    std::fs::write(watch.path().join("a.torrent"), b"old").unwrap();
    std::fs::write(watch.path().join("b.torrent"), b"x").unwrap();
    std::fs::write(watch.path().join("c.txt.rejected"), b"x").unwrap();
    let app = watch_app(watch.path()).await;
    std::fs::write(downloads(&app).join("d.torrent.rejected"), b"x").unwrap();

    let requeue = |root: &'static str, path: &'static str| {
        let app = &app;
        async move {
            app.post_json(
                &format!("/local/files/{root}/requeue"),
                json!({ "path": path }),
            )
            .await
            .status()
        }
    };
    assert_eq!(
        requeue("watch-0", "a.torrent.rejected").await,
        StatusCode::CONFLICT
    );
    assert_eq!(
        std::fs::read(watch.path().join("a.torrent")).unwrap(),
        b"old"
    );
    assert_eq!(
        requeue("watch-0", "b.torrent").await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        requeue("watch-0", "c.txt.rejected").await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        requeue("downloads", "d.torrent.rejected").await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        requeue("watch-0", "gone.torrent.rejected").await,
        StatusCode::NOT_FOUND
    );
}

// ---- US4: rename --------------------------------------------------------------

#[tokio::test]
async fn rename_in_place() {
    let app = test_app().await;
    let root = downloads(&app);
    std::fs::create_dir(root.join("radarr")).unwrap();
    std::fs::write(root.join("radarr/Movie.mkv"), b"1").unwrap();
    std::fs::write(root.join("radarr/Taken.mkv"), b"2").unwrap();

    let rename = |name: &'static str| {
        let app = &app;
        async move {
            app.post_json(
                "/local/files/downloads/rename",
                json!({"path": "radarr/Movie.mkv", "name": name}),
            )
            .await
        }
    };
    for bad in ["", ".", "..", "a/b", "a\\b", "../x"] {
        assert_eq!(
            rename(bad).await.status(),
            StatusCode::BAD_REQUEST,
            "{bad:?}"
        );
    }
    assert_eq!(rename("Taken.mkv").await.status(), StatusCode::CONFLICT);
    assert_eq!(std::fs::read(root.join("radarr/Taken.mkv")).unwrap(), b"2");

    let response = rename("Movie (2024).mkv").await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(body_json(response).await["path"], "radarr/Movie (2024).mkv");
    assert!(root.join("radarr/Movie (2024).mkv").exists());
    assert!(!root.join("radarr/Movie.mkv").exists());
}
