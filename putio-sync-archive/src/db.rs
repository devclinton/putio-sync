//! `archive db`: a gzipped, consistent copy of the database (contracts/archive-cli.md)

use crate::error::{ArchiveError, ArchiveResult};
use crate::files::{write_atomic, ArchiveDir, ManifestEntry};
use chrono::{DateTime, SecondsFormat, Utc};
use flate2::write::GzEncoder;
use flate2::Compression;
use putio_sync_db::Database;
use std::io::Write;
use std::path::PathBuf;
use tracing::info;

/// Options of `archive db`
pub struct DbOptions {
    pub dir: PathBuf,
    /// The live database, as the daemon uses it
    pub db_path: PathBuf,
    pub keep_days: Option<u32>,
    pub require_manifest: bool,
    pub now: DateTime<Utc>,
}

/// Save today's copy unless it exists, writing one line to `out`
pub async fn run(opts: &DbOptions, out: &mut dyn Write) -> ArchiveResult<()> {
    let today = opts.now.date_naive();
    let dir = ArchiveDir::open(&opts.dir, opts.require_manifest)?;
    let folder = dir.subdir("db")?;
    let name = format!("putio-sync-db-{today}.sqlite.gz");
    let target = folder.join(&name);

    if target.exists() {
        let _ = writeln!(out, "already saved {today}");
    } else {
        // Read-only: no migration, no new file, and the daemon can keep writing
        let db = Database::open_read_only(&opts.db_path).await.map_err(|e| {
            ArchiveError::database(&format!("cannot open {}", opts.db_path.display()), e)
        })?;
        let copy = folder.join(format!(".tmp-{}.sqlite", uuid::Uuid::new_v4()));
        let rows = db.snapshot_to(&copy).await;
        let _ = db.close().await;
        let saved = rows
            .map_err(|e| ArchiveError::database("cannot copy the database", e))
            .and_then(|rows| {
                write_atomic(&target, |file| {
                    let mut gz = GzEncoder::new(file, Compression::default());
                    std::io::copy(&mut std::fs::File::open(&copy)?, &mut gz)?;
                    gz.finish().map(|_| ())
                })
                .map(|()| rows)
            });
        let _ = std::fs::remove_file(&copy);
        let rows = saved?;

        dir.append_manifest(&ManifestEntry::Db {
            day: today,
            file: format!("db/{name}"),
            saved_at: opts.now.to_rfc3339_opts(SecondsFormat::Secs, true),
            rows,
        })?;
        info!(%today, rows, "Saved a database copy");
        let _ = writeln!(out, "saved {today} rows={rows}");
    }

    if let Some(keep) = opts.keep_days {
        let removed = dir.prune(keep, today)?;
        if removed > 0 {
            let _ = writeln!(out, "pruned {removed} old files");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::files::ManifestEntry;
    use flate2::read::GzDecoder;
    use putio_sync_db::models::HistoryAction;
    use putio_sync_db::Database;
    use std::io::Read;

    fn at(text: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(text)
            .unwrap()
            .with_timezone(&Utc)
    }

    async fn live_db(dir: &std::path::Path, rows: i64) -> Database {
        let db = Database::new(dir.join("live.db")).await.unwrap();
        for id in 1..=rows {
            db.history()
                .insert(
                    id,
                    "a.mkv",
                    10,
                    HistoryAction::Downloaded,
                    Some(1),
                    Some(10),
                )
                .await
                .unwrap();
        }
        db
    }

    fn opts(dir: &std::path::Path, live: &std::path::Path) -> DbOptions {
        DbOptions {
            dir: dir.join("archive"),
            db_path: live.to_path_buf(),
            keep_days: None,
            require_manifest: false,
            now: at("2026-11-02T03:30:00Z"),
        }
    }

    async fn rows_of_gz(path: &std::path::Path, scratch: &std::path::Path) -> i64 {
        let copy = scratch.join("unzipped.sqlite");
        let mut bytes = Vec::new();
        GzDecoder::new(std::fs::File::open(path).unwrap())
            .read_to_end(&mut bytes)
            .unwrap();
        std::fs::write(&copy, bytes).unwrap();
        let db = Database::open_read_only(&copy).await.unwrap();
        db.history().count(None, None).await.unwrap()
    }

    #[tokio::test]
    async fn saves_a_gzipped_copy_with_its_row_count() {
        let dir = tempfile::tempdir().unwrap();
        let db = live_db(dir.path(), 5).await;
        let o = opts(dir.path(), db.path());

        let mut out = Vec::new();
        run(&o, &mut out).await.unwrap();

        assert_eq!(String::from_utf8(out).unwrap(), "saved 2026-11-02 rows=5\n");
        let file = o.dir.join("db/putio-sync-db-2026-11-02.sqlite.gz");
        assert_eq!(rows_of_gz(&file, dir.path()).await, 5);
        // Only the final file is left in db/
        assert_eq!(std::fs::read_dir(o.dir.join("db")).unwrap().count(), 1);
        let manifest = crate::files::ArchiveDir::open(&o.dir, false)
            .unwrap()
            .read_manifest()
            .unwrap();
        assert_eq!(
            manifest,
            vec![ManifestEntry::Db {
                day: "2026-11-02".parse().unwrap(),
                file: "db/putio-sync-db-2026-11-02.sqlite.gz".to_string(),
                saved_at: "2026-11-02T03:30:00Z".to_string(),
                rows: 5,
            }]
        );
    }

    #[tokio::test]
    async fn a_second_run_the_same_day_does_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let db = live_db(dir.path(), 1).await;
        let o = opts(dir.path(), db.path());
        run(&o, &mut Vec::new()).await.unwrap();
        db.history()
            .insert(9, "b.mkv", 1, HistoryAction::Downloaded, None, None)
            .await
            .unwrap();

        let mut out = Vec::new();
        run(&o, &mut out).await.unwrap();

        assert_eq!(
            String::from_utf8(out).unwrap(),
            "already saved 2026-11-02\n"
        );
        let file = o.dir.join("db/putio-sync-db-2026-11-02.sqlite.gz");
        assert_eq!(rows_of_gz(&file, dir.path()).await, 1);
    }

    #[tokio::test]
    async fn it_runs_while_the_daemon_writes() {
        let dir = tempfile::tempdir().unwrap();
        let db = std::sync::Arc::new(live_db(dir.path(), 3).await);
        let o = opts(dir.path(), db.path());
        let writer = {
            let db = db.clone();
            tokio::spawn(async move {
                for id in 100..140 {
                    db.history()
                        .insert(id, "w.mkv", 1, HistoryAction::Downloaded, None, None)
                        .await
                        .unwrap();
                }
            })
        };
        let mut out = Vec::new();
        run(&o, &mut out).await.unwrap();
        writer.await.unwrap();

        let file = o.dir.join("db/putio-sync-db-2026-11-02.sqlite.gz");
        let rows = rows_of_gz(&file, dir.path()).await;
        assert!((3..=43).contains(&rows), "{rows}");
        assert_eq!(
            String::from_utf8(out).unwrap(),
            format!("saved 2026-11-02 rows={rows}\n")
        );
    }

    #[tokio::test]
    async fn the_live_database_files_are_not_changed() {
        let dir = tempfile::tempdir().unwrap();
        // The daemon's handle stays open, as it would be while the daemon runs
        let db = live_db(dir.path(), 4).await;
        // The -shm index is shared memory that every reader updates; the data files are not
        let read_files = || {
            db.files()
                .iter()
                .filter(|f| !f.to_string_lossy().ends_with("-shm"))
                .map(|f| std::fs::read(f).ok())
                .collect::<Vec<_>>()
        };
        let before = read_files();

        run(&opts(dir.path(), db.path()), &mut Vec::new())
            .await
            .unwrap();

        assert!(read_files() == before, "the live database files changed");
    }

    #[tokio::test]
    async fn a_missing_database_is_exit_3() {
        let dir = tempfile::tempdir().unwrap();
        let o = opts(dir.path(), &dir.path().join("nope.db"));
        let err = run(&o, &mut Vec::new()).await.unwrap_err();
        assert_eq!(err.exit_code(), 3);
        assert!(!dir.path().join("nope.db").exists());
        assert_eq!(std::fs::read_dir(o.dir.join("db")).unwrap().count(), 0);
    }
}
