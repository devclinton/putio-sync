//! The 025 rebuild of `pipeline_steps` keeps 038's rows (feature 025, R13)

use putio_sync_db::Database;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::Row;
use std::str::FromStr;

const OLD_ITEMS: &str = "CREATE TABLE pipeline_items (
    key TEXT PRIMARY KEY, name TEXT NOT NULL, category TEXT, source TEXT NOT NULL,
    dir TEXT NOT NULL DEFAULT '', transfer_id INTEGER, stage TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('running', 'held', 'waiting_for_operator', 'done', 'done_with_warnings', 'failed', 'cancelled', 'skipped')),
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP, updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    finished_at TIMESTAMP)";

const OLD_STEPS: &str = "CREATE TABLE pipeline_steps (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    item_key TEXT NOT NULL REFERENCES pipeline_items(key) ON DELETE CASCADE,
    stage TEXT NOT NULL, branch TEXT, position INTEGER NOT NULL DEFAULT 0,
    kind TEXT NOT NULL CHECK (kind IN ('hook', 'download', 'notify')),
    name TEXT NOT NULL, required INTEGER NOT NULL DEFAULT 1, changes_files INTEGER NOT NULL DEFAULT 0,
    state TEXT NOT NULL CHECK (state IN ('waiting', 'running', 'done', 'failed', 'skipped', 'interrupted')),
    skip_reason TEXT CHECK (skip_reason IS NULL OR skip_reason IN ('not_configured', 'category', 'operator', 'hook')),
    next_attempt_at TIMESTAMP, progress REAL)";

const OLD_ATTEMPTS: &str = "CREATE TABLE pipeline_attempts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    step_id INTEGER NOT NULL REFERENCES pipeline_steps(id) ON DELETE CASCADE,
    n INTEGER NOT NULL, started_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP, ended_at TIMESTAMP,
    result TEXT CHECK (result IS NULL OR result IN ('done', 'failed', 'timeout', 'skip', 'interrupted')),
    exit_code INTEGER, output TEXT NOT NULL DEFAULT '')";

async fn seed_old_schema(path: &std::path::Path) {
    let opts = SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))
        .unwrap()
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .unwrap();
    for sql in [OLD_ITEMS, OLD_STEPS, OLD_ATTEMPTS] {
        sqlx::query(sql).execute(&pool).await.unwrap();
    }
    sqlx::query("INSERT INTO pipeline_items (key, name, source, stage, state) VALUES ('transfer:1', 'Show', 'transfer', 'post_processing', 'running')")
        .execute(&pool).await.unwrap();
    for (id, kind, name, state) in [
        (5, "hook", "tidy", "done"),
        (9, "notify", "bell", "waiting"),
    ] {
        sqlx::query("INSERT INTO pipeline_steps (id, item_key, stage, branch, position, kind, name, required, changes_files, state, progress) VALUES (?, 'transfer:1', 'post_processing', 'a', ?, ?, ?, 0, 1, ?, 0.5)")
            .bind(id).bind(id).bind(kind).bind(name).bind(state)
            .execute(&pool).await.unwrap();
    }
    for n in 1..=3 {
        sqlx::query("INSERT INTO pipeline_attempts (step_id, n, result, output) VALUES (5, ?, 'done', 'out')")
            .bind(n)
            .execute(&pool)
            .await
            .unwrap();
    }
    pool.close().await;
}

async fn counts(db: &Database) -> (i64, i64) {
    let steps: i64 = sqlx::query("SELECT COUNT(*) FROM pipeline_steps")
        .fetch_one(db.pool())
        .await
        .unwrap()
        .get(0);
    let attempts: i64 = sqlx::query("SELECT COUNT(*) FROM pipeline_attempts")
        .fetch_one(db.pool())
        .await
        .unwrap()
        .get(0);
    (steps, attempts)
}

#[tokio::test]
async fn old_rows_survive_and_new_kinds_are_accepted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.db");
    seed_old_schema(&path).await;

    let db = Database::new(&path).await.unwrap();
    assert_eq!(counts(&db).await, (2, 3), "attempts must not cascade away");

    // the old rows read back unchanged
    let row = sqlx::query("SELECT kind, name, required, changes_files, branch, progress FROM pipeline_steps WHERE id = 5")
        .fetch_one(db.pool()).await.unwrap();
    assert_eq!(row.get::<String, _>("kind"), "hook");
    assert_eq!(row.get::<String, _>("name"), "tidy");
    assert_eq!(row.get::<i64, _>("changes_files"), 1);
    assert_eq!(row.get::<f64, _>("progress"), 0.5);

    // the new kinds and skip reason are accepted, ids keep counting from the old maximum
    sqlx::query("INSERT INTO pipeline_steps (item_key, stage, kind, name, state) VALUES ('transfer:1', 'subtitles', 'subtitle_scan', 'videos', 'waiting')")
        .execute(db.pool()).await.unwrap();
    sqlx::query("INSERT INTO pipeline_steps (item_key, stage, kind, name, state, skip_reason) VALUES ('transfer:1', 'subtitles', 'subtitles', 'a.mkv', 'skipped', 'no_video')")
        .execute(db.pool()).await.unwrap();
    let max: i64 = sqlx::query("SELECT MAX(id) FROM pipeline_steps")
        .fetch_one(db.pool())
        .await
        .unwrap()
        .get(0);
    assert_eq!(max, 11);

    // still validated
    assert!(sqlx::query("INSERT INTO pipeline_steps (item_key, stage, kind, name, state) VALUES ('transfer:1', 's', 'bogus', 'x', 'waiting')")
        .execute(db.pool()).await.is_err());

    // foreign keys are on again for the pool, and the attempts still cascade from steps
    let fk: i64 = sqlx::query("PRAGMA foreign_keys")
        .fetch_one(db.pool())
        .await
        .unwrap()
        .get(0);
    assert_eq!(fk, 1);
    sqlx::query("DELETE FROM pipeline_steps WHERE id = 5")
        .execute(db.pool())
        .await
        .unwrap();
    assert_eq!(counts(&db).await.1, 0);
}

#[tokio::test]
async fn migration_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.db");
    seed_old_schema(&path).await;
    {
        let db = Database::new(&path).await.unwrap();
        assert_eq!(counts(&db).await, (2, 3));
    }
    let db = Database::new(&path).await.unwrap();
    assert_eq!(counts(&db).await, (2, 3));
}

#[tokio::test]
async fn a_fresh_database_has_the_new_checks_and_the_deferred_table() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::new(dir.path().join("new.db")).await.unwrap();
    sqlx::query("INSERT INTO pipeline_items (key, name, source, stage, state) VALUES ('k', 'n', 'transfer', 'subtitles', 'running')")
        .execute(db.pool()).await.unwrap();
    sqlx::query("INSERT INTO pipeline_steps (item_key, stage, kind, name, state) VALUES ('k', 'subtitles', 'subtitles', 'a.mkv', 'waiting')")
        .execute(db.pool()).await.unwrap();

    let d = db.deferred_deletes();
    d.insert(7, "transfer:1", 100).await.unwrap();
    d.insert(7, "transfer:other", 200).await.unwrap();
    d.insert(3, "folder:/x", 50).await.unwrap();
    let rows = d.list().await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].file_id, 3);
    assert_eq!(rows[1].item_key, "transfer:1", "the first deferral wins");
    assert!(d.delete(7).await.unwrap());
    assert!(!d.delete(7).await.unwrap());
    assert_eq!(d.list().await.unwrap().len(), 1);
}

// Feature 026: a second rebuild adds the translation kinds, and the callback table is
// not cascade-deleted by it

/// The table as 025 left it: the subtitle kinds but not the translation ones
const SUBTITLE_STEPS: &str = "CREATE TABLE pipeline_steps (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    item_key TEXT NOT NULL REFERENCES pipeline_items(key) ON DELETE CASCADE,
    stage TEXT NOT NULL, branch TEXT, position INTEGER NOT NULL DEFAULT 0,
    kind TEXT NOT NULL CHECK (kind IN ('hook', 'download', 'notify', 'subtitle_scan', 'subtitles')),
    name TEXT NOT NULL, required INTEGER NOT NULL DEFAULT 1, changes_files INTEGER NOT NULL DEFAULT 0,
    state TEXT NOT NULL CHECK (state IN ('waiting', 'running', 'done', 'failed', 'skipped', 'interrupted')),
    skip_reason TEXT CHECK (skip_reason IS NULL OR skip_reason IN ('not_configured', 'category', 'operator', 'hook', 'no_video')),
    next_attempt_at TIMESTAMP, progress REAL)";

async fn seed_025_schema(path: &std::path::Path) {
    let opts = SqliteConnectOptions::from_str(&format!("sqlite:{}", path.display()))
        .unwrap()
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(opts)
        .await
        .unwrap();
    for sql in [OLD_ITEMS, SUBTITLE_STEPS, OLD_ATTEMPTS] {
        sqlx::query(sql).execute(&pool).await.unwrap();
    }
    sqlx::query("INSERT INTO pipeline_items (key, name, source, stage, state) VALUES ('transfer:1', 'Show', 'transfer', 'subtitles', 'running')")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO pipeline_steps (id, item_key, stage, kind, name, state) VALUES (4, 'transfer:1', 'subtitles', 'subtitles', 'a.mkv', 'done')")
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO pipeline_attempts (step_id, n, result) VALUES (4, 1, 'done')")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
}

async fn insert_job(db: &Database, step: i64, hash: &str, job: &str, state: &str) -> bool {
    sqlx::query("INSERT INTO translation_callbacks (token_hash, job_id, step_id, attempt_id, source_lang, target_lang, source_path, source_sha256, target_path, state, created_at, expires_at) VALUES (?, ?, ?, 1, 'en', 'es', '/m/a.en.srt', 'abc', '/m/a.es.srt', ?, 100, 200)")
        .bind(hash).bind(job).bind(step).bind(state)
        .execute(db.pool()).await.is_ok()
}

#[tokio::test]
async fn the_translation_kinds_are_added_to_a_025_database() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v025.db");
    seed_025_schema(&path).await;

    let db = Database::new(&path).await.unwrap();
    assert_eq!(counts(&db).await, (1, 1), "attempts must not cascade away");
    for kind in ["translate_scan", "translate"] {
        sqlx::query("INSERT INTO pipeline_steps (item_key, stage, kind, name, state, branch) VALUES ('transfer:1', 'translating', ?, 'a.mkv', 'waiting', 'es')")
            .bind(kind).execute(db.pool()).await.unwrap();
    }
    assert!(sqlx::query("INSERT INTO pipeline_steps (item_key, stage, kind, name, state) VALUES ('transfer:1', 's', 'bogus', 'x', 'waiting')")
        .execute(db.pool()).await.is_err());
    drop(db);

    // idempotent
    let db = Database::new(&path).await.unwrap();
    assert_eq!(counts(&db).await, (3, 1));
}

#[tokio::test]
async fn the_translation_kinds_are_added_to_a_038_database_in_one_rebuild() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v038.db");
    seed_old_schema(&path).await;
    let db = Database::new(&path).await.unwrap();
    sqlx::query("INSERT INTO pipeline_steps (item_key, stage, kind, name, state) VALUES ('transfer:1', 'translating', 'translate', 'a.mkv', 'waiting')")
        .execute(db.pool()).await.unwrap();
    assert_eq!(counts(&db).await, (3, 3));
}

#[tokio::test]
async fn translation_callbacks_are_checked_unique_and_cascade() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("v025.db");
    seed_025_schema(&path).await;
    let db = Database::new(&path).await.unwrap();

    assert!(insert_job(&db, 4, "h1", "job-1", "waiting").await);
    assert!(
        !insert_job(&db, 4, "h1", "job-2", "waiting").await,
        "token_hash is the key"
    );
    assert!(
        !insert_job(&db, 4, "h2", "job-1", "waiting").await,
        "job_id is unique"
    );
    assert!(
        !insert_job(&db, 4, "h3", "job-3", "bogus").await,
        "state is checked"
    );
    for (i, state) in ["done", "failed", "expired"].iter().enumerate() {
        assert!(insert_job(&db, 4, &format!("x{i}"), &format!("j{i}"), state).await);
    }
    assert!(
        !insert_job(&db, 999, "h9", "job-9", "waiting").await,
        "the step must exist"
    );

    // a rebuild of pipeline_steps (a reopen) keeps the jobs, and deleting the step removes them
    drop(db);
    let db = Database::new(&path).await.unwrap();
    let n: i64 = sqlx::query("SELECT COUNT(*) FROM translation_callbacks")
        .fetch_one(db.pool())
        .await
        .unwrap()
        .get(0);
    assert_eq!(n, 4);
    sqlx::query("DELETE FROM pipeline_steps WHERE id = 4")
        .execute(db.pool())
        .await
        .unwrap();
    let n: i64 = sqlx::query("SELECT COUNT(*) FROM translation_callbacks")
        .fetch_one(db.pool())
        .await
        .unwrap()
        .get(0);
    assert_eq!(n, 0);
}

#[tokio::test]
async fn subtitle_provenance_checks_its_source() {
    let dir = tempfile::tempdir().unwrap();
    let db = Database::new(dir.path().join("new.db")).await.unwrap();
    for source in [
        "putio_embedded",
        "putio_folder",
        "opensubtitles",
        "transcribed",
        "translated",
    ] {
        sqlx::query("INSERT INTO subtitle_provenance (path, language, source, bytes, created_at) VALUES (?, 'en', ?, 10, 1)")
            .bind(format!("/m/{source}.srt")).bind(source)
            .execute(db.pool()).await.unwrap();
    }
    assert!(sqlx::query("INSERT INTO subtitle_provenance (path, language, source, bytes, created_at) VALUES ('/m/x.srt', 'en', 'bogus', 10, 1)")
        .execute(db.pool()).await.is_err());
}
