//! Test app builder shared by the API integration tests (feature 004, R10).
//!
//! Put.io is a `mockito` server, the database a SQLite file in a temp dir, and the
//! router is driven with `tower::ServiceExt::oneshot`, so nothing reaches the network.

// Each test file uses a different subset of these helpers
#![allow(dead_code)]

use axum::body::Body;
use axum::http::{Method, Request};
use axum::response::Response;
use axum::Router;
use putio_sync_api::{extract_download_config, router, AppState};
use putio_sync_client::PutioApiClient;
use putio_sync_core::config::{AuthConfig, Config};
use putio_sync_db::models::QueueStatus;
use putio_sync_db::Database;
use putio_sync_download::DownloadManager;
use putio_sync_file_manager::watcher::TorrentWatcher;
use serde_json::Value;
use std::sync::Arc;
use tower::ServiceExt;

/// The API token every test app is configured with
pub const API_TOKEN: &str = "test-api-token";

/// The Put.io token every test client uses
pub const PUTIO_TOKEN: &str = "test-putio-token";

pub struct TestApp {
    pub router: Router,
    pub state: AppState,
    pub putio: mockito::ServerGuard,
    /// `None` only for `test_app_without_db`
    pub db: Option<Arc<Database>>,
    pub _dir: tempfile::TempDir,
    pub token: String,
}

/// Config pointing downloads and the config file at `dir`
pub fn test_config(dir: &tempfile::TempDir, api_token: Option<&str>) -> Config {
    let mut config = Config {
        auth: Some(AuthConfig::with_token(PUTIO_TOKEN.to_string())),
        ..Default::default()
    };
    config.download.target_directory = dir.path().to_path_buf();
    config.api.auth_token = api_token.map(str::to_string);
    config
}

/// Which engine the test app stands in for
#[derive(Clone, Copy, PartialEq)]
enum Mode {
    /// `server`: the API has its own download manager and no watcher
    Server,
    /// `daemon`, started without a watch folder
    Daemon,
    /// `daemon`, started with a watch folder (the API shares its watcher)
    DaemonWithWatcher,
}

async fn build(api_token: Option<&str>, with_db: bool) -> TestApp {
    build_with(api_token, with_db, Mode::Server, |_| {}).await
}

async fn build_with(
    api_token: Option<&str>,
    with_db: bool,
    mode: Mode,
    customize: impl FnOnce(&mut Config),
) -> TestApp {
    let putio = mockito::Server::new_async().await;
    let dir = tempfile::tempdir().unwrap();
    let mut config = test_config(&dir, api_token);
    customize(&mut config);
    let config_path = dir.path().join("config.toml");
    config.save(&config_path).unwrap();

    let client = Arc::new(PutioApiClient::with_base_url(
        PUTIO_TOKEN.to_string(),
        putio.url(),
    ));
    let download_config = extract_download_config(&config);
    let (db, manager) = if with_db {
        let db = Arc::new(
            Database::new(dir.path().join(".putiosync.db"))
                .await
                .unwrap(),
        );
        let manager = DownloadManager::new(client.clone(), db.clone(), download_config);
        (Some(db), manager)
    } else {
        let manager = DownloadManager::new_without_database(client.clone(), download_config);
        (None, manager)
    };

    let watcher = (mode == Mode::DaemonWithWatcher).then(|| {
        Arc::new(TorrentWatcher::new(
            vec![dir.path().to_path_buf()],
            None,
            60,
        ))
    });
    let mut state = AppState::new(
        config,
        Some(config_path),
        client,
        db.clone(),
        Arc::new(manager),
        watcher,
    );
    if mode != Mode::Server {
        state = state.on_daemon();
    }
    TestApp {
        router: router(state.clone()),
        state,
        putio,
        db,
        _dir: dir,
        token: api_token.unwrap_or_default().to_string(),
    }
}

/// App with a token, a database and a mocked Put.io
pub async fn test_app() -> TestApp {
    build(Some(API_TOKEN), true).await
}

/// App standing in for the daemon's listener; `with_watcher` when the daemon started
/// with a watch folder
pub async fn test_daemon_app(with_watcher: bool) -> TestApp {
    let mode = if with_watcher {
        Mode::DaemonWithWatcher
    } else {
        Mode::Daemon
    };
    build_with(Some(API_TOKEN), true, mode, |_| {}).await
}

/// App with a token, a database, and `customize` applied to its config
pub async fn test_app_with(customize: impl FnOnce(&mut Config)) -> TestApp {
    build_with(Some(API_TOKEN), true, Mode::Server, customize).await
}

/// App with no API token configured
pub async fn test_app_without_token() -> TestApp {
    build(None, true).await
}

/// App with a token but no database
pub async fn test_app_without_db() -> TestApp {
    build(Some(API_TOKEN), false).await
}

impl TestApp {
    pub fn db(&self) -> &Database {
        self.db.as_ref().expect("this test app has no database")
    }

    /// Send a request; `token` goes in `Authorization: Bearer`, `body` as JSON
    pub async fn req(
        &self,
        method: Method,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> Response {
        let mut builder = Request::builder().method(method).uri(path);
        if let Some(token) = token {
            builder = builder.header("Authorization", format!("Bearer {token}"));
        }
        let body = match body {
            Some(json) => {
                builder = builder.header("Content-Type", "application/json");
                Body::from(json.to_string())
            }
            None => Body::empty(),
        };
        self.send(builder.body(body).unwrap()).await
    }

    /// Send a prepared request through the router
    pub async fn send(&self, request: Request<Body>) -> Response {
        self.router.clone().oneshot(request).await.unwrap()
    }

    /// `GET` with the app's token
    pub async fn get(&self, path: &str) -> Response {
        self.req(Method::GET, path, Some(&self.token), None).await
    }

    /// `POST` JSON with the app's token
    pub async fn post_json(&self, path: &str, body: Value) -> Response {
        self.req(Method::POST, path, Some(&self.token), Some(body))
            .await
    }

    /// Add queue rows `(file_id, filename, status)`; priority follows the slice order
    pub async fn seed_queue(&self, rows: &[(i64, &str, QueueStatus)]) {
        let queue = self.db().queue();
        for (priority, (file_id, name, status)) in rows.iter().enumerate() {
            queue
                .add(
                    *file_id,
                    name,
                    1024,
                    "http://unused",
                    "",
                    priority as i64,
                    QueueStatus::Pending,
                )
                .await
                .unwrap();
            if *status != QueueStatus::Pending {
                queue.update_status(*file_id, status.clone()).await.unwrap();
            }
        }
    }

    /// Mock `GET /files/list?parent_id=<parent_id>`; keep the mock alive while needed
    pub async fn mock_files_list(&mut self, parent_id: i64, json: Value) -> mockito::Mock {
        self.putio
            .mock("GET", format!("/files/list?parent_id={parent_id}").as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(json.to_string())
            .create_async()
            .await
    }

    /// Mock `GET /files/<id>`; keep the mock alive while needed
    pub async fn mock_file(&mut self, id: i64, json: Value) -> mockito::Mock {
        self.putio
            .mock("GET", format!("/files/{id}").as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(json.to_string())
            .create_async()
            .await
    }
}

/// Read a response body as JSON
pub async fn body_json(response: Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

/// Read a response body as text
pub async fn body_text(response: Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}
