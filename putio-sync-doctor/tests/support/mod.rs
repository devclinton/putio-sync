//! A doctor wired to a mocked Put.io, a temp database and a temp download folder

#![allow(dead_code)]

use mockito::{Mock, Server, ServerGuard};
use putio_sync_client::{PutioApiClient, RetryPolicy};
use putio_sync_core::cleanup::EmptyFolderCleanup;
use putio_sync_core::config::Config;
use putio_sync_db::Database;
use putio_sync_doctor::Doctor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

pub const OLD: &str = "2020-01-01T00:00:00";

pub struct Env {
    pub server: ServerGuard,
    pub dir: tempfile::TempDir,
    pub db: Arc<Database>,
    pub doctor: Doctor,
}

impl Env {
    pub async fn new() -> Self {
        let server = Server::new_async().await;
        let dir = tempfile::tempdir().unwrap();
        let mut config = Config::default();
        config.download.target_directory = dir.path().join("downloads");
        std::fs::create_dir_all(&config.download.target_directory).unwrap();
        let client = Arc::new(
            PutioApiClient::with_base_url("t".into(), server.url()).with_retry_policy(
                RetryPolicy {
                    max_attempts: 1,
                    ..RetryPolicy::default()
                },
            ),
        );
        let db = Arc::new(
            Database::new(dir.path().join(".putiosync.db"))
                .await
                .unwrap(),
        );
        let cleanup = Arc::new(EmptyFolderCleanup::from_config(client.clone(), &config));
        let doctor = Doctor::new(config, client, Some(db.clone()), cleanup);
        Self {
            server,
            dir,
            db,
            doctor,
        }
    }

    pub fn downloads(&self) -> PathBuf {
        self.dir.path().join("downloads")
    }

    pub async fn account(&mut self, status: usize) -> Mock {
        self.server
            .mock("GET", "/account/info")
            .with_status(status)
            .with_header("x-ratelimit-limit", "1000")
            .with_header("x-ratelimit-remaining", "990")
            .with_body(
                r#"{"status":"OK","info":{"user_id":1,"username":"u","mail":"u@example.com",
                "account_active":true,"disk":{"size":1000000000000,"used":100000000000,
                "avail":900000000000}}}"#,
            )
            .create_async()
            .await
    }

    pub async fn transfers(&mut self, list: &[(i64, &str, &str)]) -> Mock {
        let items: Vec<String> = list
            .iter()
            .map(|(id, name, status)| {
                format!(
                    r#"{{"id":{id},"name":"{name}","status":"{status}","created_at":"{OLD}",
                    "save_parent_id":0,"is_private":false,"downloaded":10,
                    "magneturi":"magnet:?xt=urn:btih:{id}"}}"#
                )
            })
            .collect();
        self.server
            .mock("GET", "/transfers/list")
            .with_body(format!(
                r#"{{"status":"OK","transfers":[{}]}}"#,
                items.join(",")
            ))
            .create_async()
            .await
    }

    /// Folder `parent` holds `entries`: (id, name, size, is_folder)
    pub async fn folder(&mut self, parent: i64, entries: &[(i64, &str, i64, bool)]) -> Mock {
        let files: Vec<String> = entries
            .iter()
            .map(|(id, name, size, folder)| {
                format!(
                    r#"{{"id":{id},"name":"{name}","size":{size},"file_type":"{}",
                    "parent_id":{parent},"is_folder":{folder},"is_shared":false,
                    "created_at":"{OLD}"}}"#,
                    if *folder { "FOLDER" } else { "VIDEO" }
                )
            })
            .collect();
        self.server
            .mock("GET", format!("/files/list?parent_id={parent}").as_str())
            .with_body(format!(
                r#"{{"status":"OK","files":[{}],"parent":{{"id":{parent},"name":"f","size":0,
                "file_type":"FOLDER","parent_id":null,"is_folder":true}}}}"#,
                files.join(",")
            ))
            .create_async()
            .await
    }

    pub async fn gone(&mut self, id: i64) -> Mock {
        self.server
            .mock("GET", format!("/files/{id}").as_str())
            .with_status(404)
            .with_body(r#"{"status":"ERROR","error_type":"NotFound","status_code":404}"#)
            .create_async()
            .await
    }

    /// A healthy account: one big file at the root, one finished transfer
    pub async fn healthy(&mut self) {
        self.account(200).await;
        self.transfers(&[(1, "Done", "COMPLETED")]).await;
        self.folder(0, &[(10, "movie.mkv", 2_000_000, false)]).await;
    }
}

/// A partial download at `rel` under `root`, last touched `age` ago
pub fn partial(root: &Path, rel: &str, age: Duration) -> PathBuf {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, b"partial").unwrap();
    let when = filetime::FileTime::from_system_time(SystemTime::now() - age);
    filetime::set_file_mtime(&path, when).unwrap();
    path
}
