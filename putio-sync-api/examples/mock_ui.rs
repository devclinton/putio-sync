//! Serve the web UI over a mocked Put.io and a seeded temp database, so every page can
//! be clicked through offline (feature 004, R10).
//!
//! ```text
//! cargo run -p putio-sync-api --example mock_ui [port]
//! ```
//!
//! Prints the URL and the API token to sign in with, then runs until Ctrl+C. The port
//! defaults to a free one. Nothing reaches the real Put.io through `state.client`.
//! The history holds 64 rows (two pages); 7 of them match `ubuntu`.

use putio_sync_api::{extract_download_config, router, AppState};
use putio_sync_client::PutioApiClient;
use putio_sync_core::config::{AuthConfig, Config};
use putio_sync_db::models::{HistoryAction, QueueStatus};
use putio_sync_db::Database;
use putio_sync_download::DownloadManager;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Arc;

/// The API token to sign in with; a fixed test value, not a secret
const API_TOKEN: &str = "mock-ui-token";

/// The Put.io token the mocked client sends; mockito ignores it
const PUTIO_TOKEN: &str = "mock-putio-token";

const MOVIES: i64 = 100;
const SHOWS: i64 = 200;

fn file(id: i64, name: &str, size: i64, file_type: &str, parent_id: i64) -> Value {
    json!({
        "id": id,
        "name": name,
        "size": size,
        "file_type": file_type,
        "parent_id": parent_id,
        "is_folder": file_type == "FOLDER",
        "is_shared": false,
        "created_at": "2026-10-20T09:30:00",
        "content_type": if file_type == "FOLDER" { "application/x-directory" } else { "video/x-matroska" },
    })
}

fn listing(parent: Value, files: Vec<Value>) -> String {
    let total = files.len();
    json!({ "status": "OK", "files": files, "parent": parent, "total": total }).to_string()
}

fn transfer(id: i64, name: &str, status: &str, percent: i64) -> Value {
    json!({
        "id": id,
        "name": name,
        "size": 2_147_483_648_i64,
        "status": status,
        "down_speed": 3_145_728,
        "up_speed": 0,
        "downloaded": 2_147_483_648_i64 * percent / 100,
        "uploaded": 0,
        "percent_done": percent,
        "estimated_time": 420,
        "peers": 12,
        "availability": 100,
        "current_ratio": 0.0,
        "seconds_seeding": 0,
        "error_message": null,
        "created_at": "2026-10-26T08:00:00Z",
        "finished_at": null,
        "save_parent_id": 0,
        "file_id": null,
        "is_private": false
    })
}

/// Persistent mocks for every Put.io call the UI can trigger
async fn mock_putio(server: &mut mockito::ServerGuard) -> Vec<mockito::Mock> {
    let root = file(0, "Your Files", 0, "FOLDER", 0);
    let movies = file(MOVIES, "Movies", 9_000_000_000, "FOLDER", 0);
    let shows = file(SHOWS, "Shows", 4_000_000_000, "FOLDER", 0);
    let readme = file(300, "README.txt", 2_048, "TEXT", 0);
    let big = file(
        101,
        "Big.Buck.Bunny.2008.2160p.mkv",
        8_589_934_592,
        "VIDEO",
        MOVIES,
    );
    let setup = file(102, "setup.exe", 52_428_800, "FILE", MOVIES);
    // Not queued, so "Download" on Movies toasts found 3, queued 1, skipped 1, blocked 1
    let sintel = file(103, "Sintel.2010.1080p.mkv", 1_288_490_189, "VIDEO", MOVIES);
    let episode = file(
        201,
        "Example.Show.S01E01.1080p.mkv",
        1_610_612_736,
        "VIDEO",
        SHOWS,
    );

    let mut json_mock = |method: &str, path: String, body: String| {
        server
            .mock(method, path.as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .expect_at_least(0)
    };

    let mut mocks = vec![
        json_mock(
            "GET",
            "/files/list?parent_id=0".into(),
            listing(root.clone(), vec![movies.clone(), shows.clone(), readme.clone()]),
        ),
        json_mock(
            "GET",
            format!("/files/list?parent_id={MOVIES}"),
            listing(movies.clone(), vec![big.clone(), setup.clone(), sintel.clone()]),
        ),
        json_mock(
            "GET",
            "/transfers/list".into(),
            json!({ "status": "OK", "transfers": [transfer(1, "Sintel.2010.1080p", "DOWNLOADING", 42)] })
                .to_string(),
        ),
        json_mock(
            "POST",
            "/transfers/add".into(),
            json!({ "status": "OK", "transfer": transfer(2, "New transfer", "WAITING", 0) })
                .to_string(),
        ),
        // A `.torrent` file goes through the upload endpoint, which follows the base URL
        json_mock(
            "POST",
            "/files/upload".into(),
            json!({ "status": "OK", "transfer": transfer(3, "Uploaded torrent", "WAITING", 0) })
                .to_string(),
        ),
        json_mock("POST", "/files/delete".into(), json!({ "status": "OK" }).to_string()),
        // Remove; start and stop answer from the API without calling Put.io
        json_mock("POST", "/transfers/cancel".into(), json!({ "status": "OK" }).to_string()),
        json_mock("POST", "/transfers/retry".into(), json!({ "status": "OK" }).to_string()),
    ];
    for f in [&movies, &shows, &readme, &big, &setup, &sintel, &episode] {
        mocks.push(json_mock(
            "GET",
            format!("/files/{}", f["id"]),
            json!({ "status": "OK", "file": f }).to_string(),
        ));
    }

    // The Shows listing answers after 3 s, so a "sync now" (which walks every folder)
    // stays in progress long enough to see it and to try a second one
    let shows_listing = listing(shows.clone(), vec![episode.clone()]);
    mocks.push(
        server
            .mock("GET", format!("/files/list?parent_id={SHOWS}").as_str())
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_chunked_body(move |w| {
                std::thread::sleep(std::time::Duration::from_secs(3));
                w.write_all(shows_listing.as_bytes())
            })
            .expect_at_least(0),
    );

    // `#/putio/999` is a folder that no longer exists: the page shows the error and Retry
    mocks.push(
        server
            .mock("GET", "/files/list?parent_id=999")
            .with_status(404)
            .with_header("content-type", "application/json")
            .with_body(r#"{"status":"ERROR","error_type":"NotFound"}"#)
            .expect_at_least(0),
    );

    let mut created = Vec::with_capacity(mocks.len());
    for mock in mocks {
        created.push(mock.create_async().await);
    }
    created
}

/// 64 history rows, two pages of 50: every fourth one a deletion, every tenth one an
/// `ubuntu` image, so the name filter has something to find
async fn seed_history(db: &Database) -> Result<(), Box<dyn std::error::Error>> {
    const NAMES: [&str; 5] = [
        "Big.Buck.Bunny.2008.1080p.mkv",
        "Sintel.2010.720p.mkv",
        "debian-12.7.0-amd64-netinst.iso",
        "Cosmos.Laundromat.2015.mkv",
        "Spring.2019.2160p.mkv",
    ];
    let history = db.history();
    for i in 0..64_i64 {
        let name = if i % 10 == 3 {
            format!("Ubuntu-24.04.{i}-desktop-amd64.iso")
        } else {
            format!("{:02}.{}", i, NAMES[i as usize % NAMES.len()])
        };
        let size = 104_857_600 * (i % 7 + 1);
        if i % 4 == 0 {
            history
                .insert(500 + i, &name, size, HistoryAction::Deleted, None, None)
                .await?;
        } else {
            let seconds = 30 + i * 5;
            history
                .insert(
                    500 + i,
                    &name,
                    size,
                    HistoryAction::Downloaded,
                    Some(seconds),
                    Some(size / seconds),
                )
                .await?;
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,sqlx=warn".into()),
        )
        .init();

    let port: u16 = match std::env::args().nth(1) {
        Some(arg) => arg.parse()?,
        None => 0,
    };

    let mut putio = mockito::Server::new_async().await;
    let _mocks = mock_putio(&mut putio).await;

    let dir = tempfile::tempdir()?;
    let mut config = Config {
        auth: Some(AuthConfig::with_token(PUTIO_TOKEN.to_string())),
        ..Default::default()
    };
    config.download.target_directory = dir.path().to_path_buf();
    config.download.blocked_extensions = vec!["exe".to_string()];
    config.api.auth_token = Some(API_TOKEN.to_string());
    let config_path = dir.path().join("config.toml");
    config.save(&config_path)?;

    let db = Arc::new(Database::new(dir.path().join(".putiosync.db")).await?);
    let queue = db.queue();
    let rows = [
        (
            101,
            "Big.Buck.Bunny.2008.2160p.mkv",
            8_589_934_592,
            QueueStatus::Pending,
        ),
        (
            201,
            "Example.Show.S01E01.1080p.mkv",
            1_610_612_736,
            QueueStatus::Pending,
        ),
        (
            401,
            "Elephants.Dream.2006.1080p.mkv",
            1_073_741_824,
            QueueStatus::Completed,
        ),
        (
            402,
            "Tears.of.Steel.2012.720p.mkv",
            734_003_200,
            QueueStatus::Paused,
        ),
    ];
    // Listed (and downloaded) highest priority first, so the first row gets the highest
    let top = rows.len() as i64;
    for (index, (file_id, name, size, status)) in rows.into_iter().enumerate() {
        queue
            .add(file_id, name, size, "http://unused", "", top - index as i64)
            .await?;
        if status != QueueStatus::Pending {
            queue.update_status(file_id, status).await?;
        }
    }

    seed_history(&db).await?;

    let client = Arc::new(PutioApiClient::with_base_url(
        PUTIO_TOKEN.to_string(),
        putio.url(),
    ));
    let manager =
        DownloadManager::new(client.clone(), db.clone(), extract_download_config(&config));
    let state = AppState::new(
        config,
        Some(config_path),
        client,
        Some(db),
        Arc::new(manager),
        None,
    );

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await?;
    let addr = listener.local_addr()?;
    println!("Open http://{addr}/ and sign in with token: {API_TOKEN}");
    println!("Mocked Put.io at {}; press Ctrl+C to stop.", putio.url());

    axum::serve(
        listener,
        router(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = tokio::signal::ctrl_c().await;
    })
    .await?;
    Ok(())
}
