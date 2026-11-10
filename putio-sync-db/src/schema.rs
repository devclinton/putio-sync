//! Database schema definitions and SQL migrations

/// SQL to create the downloads table
pub const CREATE_DOWNLOADS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS downloads (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_id INTEGER NOT NULL,
    filename TEXT NOT NULL,
    file_size INTEGER NOT NULL,
    local_path TEXT NOT NULL,
    downloaded_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    status TEXT NOT NULL CHECK(status IN ('completed', 'in_progress', 'failed')),
    retry_count INTEGER DEFAULT 0,
    UNIQUE(file_id, filename, file_size)
)
"#;

/// SQL to create the download_history table
pub const CREATE_DOWNLOAD_HISTORY_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS download_history (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_id INTEGER NOT NULL,
    filename TEXT NOT NULL,
    file_size INTEGER NOT NULL,
    extension TEXT,
    action TEXT NOT NULL CHECK(action IN ('downloaded', 'deleted')),
    timestamp TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    duration_seconds INTEGER,
    average_speed INTEGER
)
"#;

/// SQL to create the download_stats table
pub const CREATE_DOWNLOAD_STATS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS download_stats (
    id INTEGER PRIMARY KEY CHECK(id = 1),
    total_downloads INTEGER DEFAULT 0,
    total_bytes INTEGER DEFAULT 0,
    total_time_seconds INTEGER DEFAULT 0,
    last_updated TIMESTAMP DEFAULT CURRENT_TIMESTAMP
)
"#;

/// SQL to initialize stats table with a single row
pub const INIT_DOWNLOAD_STATS: &str = r#"
INSERT OR IGNORE INTO download_stats (id, total_downloads, total_bytes, total_time_seconds)
VALUES (1, 0, 0, 0)
"#;

/// SQL to create the download_queue table
pub const CREATE_DOWNLOAD_QUEUE_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS download_queue (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_id INTEGER NOT NULL,
    filename TEXT NOT NULL,
    file_size INTEGER NOT NULL,
    download_url TEXT NOT NULL,
    relative_path TEXT NOT NULL DEFAULT '',
    priority INTEGER DEFAULT 0,
    status TEXT NOT NULL CHECK(status IN ('pending', 'downloading', 'paused', 'completed', 'failed')),
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(file_id)
)
"#;

/// Migration to add relative_path column to download_queue table
/// This is for upgrading existing databases
pub const MIGRATE_ADD_RELATIVE_PATH: &str = r#"
-- Add relative_path column if it doesn't exist
-- SQLite doesn't support IF NOT EXISTS for columns, so we check if it exists first
-- This will be handled by code checking PRAGMA table_info
"#;

/// SQL to create the processed_torrents table
pub const CREATE_PROCESSED_TORRENTS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS processed_torrents (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    file_path TEXT NOT NULL,
    file_checksum TEXT NOT NULL,
    file_size INTEGER NOT NULL,
    magnet_link TEXT,
    processed_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(file_checksum)
)
"#;

/// SQL to create the download_sources table (feature 006): where each queued Put.io
/// file came from. `tracker_host` is a host name only, never a tracker address.
pub const CREATE_DOWNLOAD_SOURCES_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS download_sources (
    file_id INTEGER PRIMARY KEY,
    transfer_id INTEGER,
    source_type TEXT CHECK(source_type IN ('torrent', 'url', 'other')),
    tracker_host TEXT,
    is_private INTEGER NOT NULL DEFAULT 0,
    origin TEXT NOT NULL CHECK(origin IN ('watch', 'manual', 'putio', 'unknown')),
    watch_folder TEXT,
    putio_folder TEXT,
    recorded_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
)
"#;

/// SQL to create the transfer_origins table (feature 006): transfers putio-sync
/// created itself, from a watch folder or the web UI
pub const CREATE_TRANSFER_ORIGINS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS transfer_origins (
    transfer_id INTEGER PRIMARY KEY,
    origin TEXT NOT NULL CHECK(origin IN ('watch', 'manual')),
    watch_folder TEXT,
    recorded_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
)
"#;

/// SQL to create the arr_items table (feature 011): releases Radarr/Sonarr added
/// through the Transmission RPC. `id` is the Transmission torrent id.
pub const CREATE_ARR_ITEMS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS arr_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    hash TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    category TEXT NOT NULL,
    transfer_id INTEGER,
    file_id INTEGER,
    output_name TEXT,
    total_size INTEGER NOT NULL DEFAULT 0,
    putio_status TEXT,
    putio_percent INTEGER NOT NULL DEFAULT 0,
    putio_eta INTEGER,
    state TEXT NOT NULL CHECK(state IN ('putio', 'local', 'done', 'failed')),
    error TEXT,
    blocklist TEXT NOT NULL DEFAULT 'none' CHECK(blocklist IN ('none', 'pending', 'done', 'gave_up')),
    blocklist_attempts INTEGER NOT NULL DEFAULT 0,
    added_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
)
"#;

/// SQL to create the arr_item_files table (feature 011): the Put.io files of an item
/// that the sync's filters accepted
pub const CREATE_ARR_ITEM_FILES_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS arr_item_files (
    hash TEXT NOT NULL,
    file_id INTEGER NOT NULL,
    size INTEGER NOT NULL,
    PRIMARY KEY (hash, file_id)
)
"#;

/// SQL to create the rule_decisions table (feature 009): the latest folder rule
/// decision per Put.io file not downloaded. `rule_path` NULL means the global settings.
pub const CREATE_RULE_DECISIONS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS rule_decisions (
    file_id INTEGER PRIMARY KEY,
    filename TEXT NOT NULL,
    folder_path TEXT NOT NULL,
    decision TEXT NOT NULL CHECK(decision IN ('blocked', 'skipped', 'deleted')),
    reason TEXT NOT NULL,
    rule_path TEXT,
    decided_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
)
"#;

/// SQL to create the pipeline_holds table (feature 039): transfers paused while still on
/// Put.io. Their files are queued as `paused` when the sync finds them.
pub const CREATE_PIPELINE_HOLDS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS pipeline_holds (
    transfer_id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    held_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
)
"#;

/// SQL to create the daemon_flags table (feature 039): runtime switches that survive a
/// restart. `value` is "1" or "0"; a missing key means off.
pub const CREATE_DAEMON_FLAGS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS daemon_flags (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
)
"#;

/// SQL to create the pipeline_finished table (feature 039): Recently finished items,
/// pruned after `[transfers] finished_retention_hours`
pub const CREATE_PIPELINE_FINISHED_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS pipeline_finished (
    key TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    size INTEGER NOT NULL,
    outcome TEXT NOT NULL CHECK (outcome IN ('done', 'failed')),
    reason TEXT,
    added_at TIMESTAMP,
    putio_done_at TIMESTAMP,
    queued_at TIMESTAMP,
    done_at TIMESTAMP NOT NULL
)
"#;

/// SQL to create the pipeline_items table (feature 038): one row per item the processing
/// pipeline follows, keyed like the Active page (`transfer:<id>`, `folder:<path>`, `file:<id>`)
pub const CREATE_PIPELINE_ITEMS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS pipeline_items (
    key TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    category TEXT,
    source TEXT NOT NULL,
    dir TEXT NOT NULL DEFAULT '',
    transfer_id INTEGER,
    stage TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('running', 'held', 'waiting_for_operator', 'done', 'done_with_warnings', 'failed', 'cancelled', 'skipped')),
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    finished_at TIMESTAMP
)
"#;

/// SQL to create the pipeline_steps table (feature 038): the planned steps of an item
pub const CREATE_PIPELINE_STEPS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS pipeline_steps (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    item_key TEXT NOT NULL REFERENCES pipeline_items(key) ON DELETE CASCADE,
    stage TEXT NOT NULL,
    branch TEXT,
    position INTEGER NOT NULL DEFAULT 0,
    kind TEXT NOT NULL CHECK (kind IN ('hook', 'download', 'notify', 'subtitle_scan', 'subtitles', 'translate_scan', 'translate')),
    name TEXT NOT NULL,
    required INTEGER NOT NULL DEFAULT 1,
    changes_files INTEGER NOT NULL DEFAULT 0,
    state TEXT NOT NULL CHECK (state IN ('waiting', 'running', 'done', 'failed', 'skipped', 'interrupted')),
    skip_reason TEXT CHECK (skip_reason IS NULL OR skip_reason IN ('not_configured', 'category', 'operator', 'hook', 'no_video')),
    next_attempt_at TIMESTAMP,
    progress REAL
)
"#;

/// SQL to create the pipeline_deferred_deletes table (feature 025): Put.io files whose
/// post-download delete waits for the Subtitles stage. Persisted so a restart neither loses
/// the delete nor skips the wait.
pub const CREATE_PIPELINE_DEFERRED_DELETES_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS pipeline_deferred_deletes (
    file_id INTEGER PRIMARY KEY,
    item_key TEXT NOT NULL,
    created_at INTEGER NOT NULL
)
"#;

/// SQL to create the translation_callbacks table (feature 026): one row per translation job
/// sent to a callback service. The token is never stored, only its SHA-256.
pub const CREATE_TRANSLATION_CALLBACKS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS translation_callbacks (
    token_hash TEXT PRIMARY KEY,
    job_id TEXT NOT NULL UNIQUE,
    step_id INTEGER NOT NULL REFERENCES pipeline_steps(id) ON DELETE CASCADE,
    attempt_id INTEGER NOT NULL,
    source_lang TEXT NOT NULL,
    target_lang TEXT NOT NULL,
    source_path TEXT NOT NULL,
    source_sha256 TEXT NOT NULL,
    target_path TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('waiting', 'done', 'failed', 'expired')),
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    finished_at INTEGER
)
"#;

/// SQL to create the subtitle_provenance table (features 025 and 026): where each saved
/// sidecar came from, so translation can prefer a transcribed or original source
pub const CREATE_SUBTITLE_PROVENANCE_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS subtitle_provenance (
    path TEXT PRIMARY KEY,
    language TEXT NOT NULL,
    source TEXT NOT NULL CHECK (source IN ('putio_embedded', 'putio_folder', 'opensubtitles', 'transcribed', 'translated')),
    bytes INTEGER NOT NULL,
    created_at INTEGER NOT NULL
)
"#;

/// SQL to create the pipeline_attempts table (feature 038): every run of a step, with the
/// redacted tail of its output
pub const CREATE_PIPELINE_ATTEMPTS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS pipeline_attempts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    step_id INTEGER NOT NULL REFERENCES pipeline_steps(id) ON DELETE CASCADE,
    n INTEGER NOT NULL,
    started_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    ended_at TIMESTAMP,
    result TEXT CHECK (result IS NULL OR result IN ('done', 'failed', 'timeout', 'skip', 'interrupted')),
    exit_code INTEGER,
    output TEXT NOT NULL DEFAULT ''
)
"#;

/// SQL to create the release_checks table (feature 042): one row per release pre-check
pub const CREATE_RELEASE_CHECKS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS release_checks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    hash TEXT,
    name TEXT NOT NULL,
    source TEXT NOT NULL CHECK(source IN ('torrent', 'url', 'magnet', 'watch_torrent', 'watch_magnet')),
    category TEXT,
    watch_folder TEXT,
    stage TEXT NOT NULL CHECK(stage IN ('before', 'putio')),
    state TEXT NOT NULL CHECK(state IN ('passed', 'rejected', 'pending')),
    lookup TEXT CHECK(lookup IN ('found', 'not_found', 'timeout', 'wrong_hash', 'error', 'off')),
    lookup_cache TEXT,
    transfer_id INTEGER,
    total_size INTEGER NOT NULL DEFAULT 0,
    putio_bytes INTEGER NOT NULL DEFAULT 0,
    reason TEXT,
    checked_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    resolved_at TIMESTAMP
)
"#;

/// SQL to create the release_reports table (feature 042): one row per rejection and per
/// operator flag. Never a magnet, URL, API key or .torrent bytes.
pub const CREATE_RELEASE_REPORTS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS release_reports (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL CHECK(kind IN ('rejected', 'flagged_fake', 'flagged_not_fake')),
    check_id INTEGER,
    arr_hash TEXT,
    file_id INTEGER,
    transfer_id INTEGER,
    source TEXT,
    category TEXT,
    watch_folder TEXT,
    stage TEXT,
    name TEXT NOT NULL,
    hash TEXT,
    total_size INTEGER,
    file_count INTEGER,
    types TEXT NOT NULL DEFAULT '[]',
    largest_wanted INTEGER,
    min_size INTEGER,
    tracker_hosts TEXT NOT NULL DEFAULT '[]',
    is_private INTEGER NOT NULL DEFAULT 0,
    reason TEXT,
    note TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
)
"#;

/// SQL to create the held_magnets table (feature 043): magnets accepted but not yet sent
/// to Put.io, waiting for a cache to have their file list. Times are Unix seconds. The
/// magnet is never logged or returned by the API.
pub const CREATE_HELD_MAGNETS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS held_magnets (
    hash TEXT PRIMARY KEY,
    magnet TEXT NOT NULL,
    source TEXT NOT NULL CHECK(source IN ('magnet', 'url', 'watch_magnet')),
    category TEXT,
    watch_folder TEXT,
    check_id INTEGER,
    attempts INTEGER NOT NULL DEFAULT 1,
    last_lookup TEXT,
    next_at INTEGER NOT NULL,
    until_at INTEGER NOT NULL,
    held_at INTEGER NOT NULL
)
"#;

/// SQL to create the hold_ends table (feature 043): how each hold ended, for the Stats
/// page. `result` is a `HoldResult` label; times are Unix seconds.
pub const CREATE_HOLD_ENDS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS hold_ends (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    hash TEXT NOT NULL,
    result TEXT NOT NULL,
    held_secs INTEGER NOT NULL,
    ended_at INTEGER NOT NULL
)
"#;

/// SQL to create the whats_new_seen table (feature 017): the ids of the changelog entries
/// this install has shown. Rows are only added.
pub const CREATE_WHATS_NEW_SEEN_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS whats_new_seen (
    id TEXT PRIMARY KEY,
    seen_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
)
"#;

/// SQL to create the storage_runs table (feature 033): each free-up run that deleted
/// files. `trigger` is `manual` or `schedule`.
pub const CREATE_STORAGE_RUNS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS storage_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    finished_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    trigger TEXT NOT NULL,
    files INTEGER NOT NULL,
    bytes INTEGER NOT NULL,
    failed INTEGER NOT NULL DEFAULT 0,
    free_after INTEGER
)
"#;

/// SQL to create the storage_run_files table (feature 033): the files a run deleted
pub const CREATE_STORAGE_RUN_FILES_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS storage_run_files (
    run_id INTEGER NOT NULL REFERENCES storage_runs(id),
    file_id INTEGER NOT NULL,
    path TEXT NOT NULL,
    size INTEGER NOT NULL,
    created_at TEXT,
    local_path TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_storage_run_files_run ON storage_run_files(run_id)
"#;

/// SQL to create the verify_runs table (feature 034): each "verify everything" run.
/// `state` is `running` while it goes, then `finished`, `stopped` or `failed`.
pub const CREATE_VERIFY_RUNS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS verify_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    scope TEXT NOT NULL,
    report_only INTEGER NOT NULL DEFAULT 0,
    state TEXT NOT NULL,
    error TEXT,
    started_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    ended_at TIMESTAMP,
    files_total INTEGER NOT NULL DEFAULT 0,
    files_done INTEGER NOT NULL DEFAULT 0,
    bytes_total INTEGER NOT NULL DEFAULT 0,
    bytes_done INTEGER NOT NULL DEFAULT 0,
    verified INTEGER NOT NULL DEFAULT 0,
    mismatch INTEGER NOT NULL DEFAULT 0,
    missing INTEGER NOT NULL DEFAULT 0,
    not_on_putio INTEGER NOT NULL DEFAULT 0,
    busy INTEGER NOT NULL DEFAULT 0,
    unreadable INTEGER NOT NULL DEFAULT 0,
    not_checked INTEGER NOT NULL DEFAULT 0
)
"#;

/// SQL to create the verify_problems table (feature 034): files that did not verify,
/// from a run (`run_id` set) or a download that gave up (`source = 'download'`)
pub const CREATE_VERIFY_PROBLEMS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS verify_problems (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id INTEGER REFERENCES verify_runs(id),
    source TEXT NOT NULL,
    file_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    local_path TEXT,
    result TEXT NOT NULL,
    detail TEXT,
    queued INTEGER NOT NULL DEFAULT 0,
    at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    fixed_at TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_verify_problems_run ON verify_problems(run_id);
CREATE INDEX IF NOT EXISTS idx_verify_problems_file ON verify_problems(file_id)
"#;

/// SQL to create the update_state table (feature 055): one row with the last GitHub
/// Releases check, its ETag and releases, the dismissed version, and the last rollback.
/// Times are RFC 3339.
pub const CREATE_UPDATE_STATE_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS update_state (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    last_attempt_at TEXT,
    last_success_at TEXT,
    last_error TEXT,
    etag TEXT,
    releases_json TEXT,
    dismissed_version TEXT,
    last_rolled_back TEXT,
    rollback_reason TEXT
)
"#;

/// SQL to create performance indexes
/// These indexes optimize common query patterns
pub const CREATE_INDEXES: &str = r#"
-- Processing pipeline steps by item and by what is due (feature 038)
CREATE INDEX IF NOT EXISTS idx_pipeline_steps_item ON pipeline_steps(item_key);
CREATE INDEX IF NOT EXISTS idx_pipeline_steps_due ON pipeline_steps(state, next_attempt_at);
CREATE INDEX IF NOT EXISTS idx_pipeline_attempts_step ON pipeline_attempts(step_id);
CREATE INDEX IF NOT EXISTS idx_translation_callbacks_due ON translation_callbacks(state, expires_at);
CREATE INDEX IF NOT EXISTS idx_translation_callbacks_step ON translation_callbacks(step_id);
CREATE INDEX IF NOT EXISTS idx_subtitle_provenance_created ON subtitle_provenance(created_at);

-- Index for finding downloads by file_id
CREATE INDEX IF NOT EXISTS idx_downloads_file_id ON downloads(file_id);

-- Index for finding downloads by status
CREATE INDEX IF NOT EXISTS idx_downloads_status ON downloads(status);

-- Index for history queries by timestamp (for date range filtering)
CREATE INDEX IF NOT EXISTS idx_history_timestamp ON download_history(timestamp);

-- Index for history queries by action type
CREATE INDEX IF NOT EXISTS idx_history_action ON download_history(action);

-- Index for stats by extension queries
CREATE INDEX IF NOT EXISTS idx_history_extension ON download_history(extension);

-- Composite index for common history query pattern (action + timestamp)
CREATE INDEX IF NOT EXISTS idx_history_action_timestamp ON download_history(action, timestamp);

-- Index for processed torrents by checksum (primary lookup)
CREATE INDEX IF NOT EXISTS idx_processed_torrents_checksum ON processed_torrents(file_checksum);

-- Index for processed torrents by timestamp
CREATE INDEX IF NOT EXISTS idx_processed_torrents_timestamp ON processed_torrents(processed_at);

-- Index for the newest rule decisions
CREATE INDEX IF NOT EXISTS idx_rule_decisions_decided_at ON rule_decisions(decided_at);

-- Indexes for the pipeline file lookup (feature 039)
CREATE INDEX IF NOT EXISTS idx_download_sources_transfer_id ON download_sources(transfer_id);
CREATE INDEX IF NOT EXISTS idx_downloads_downloaded_at ON downloads(downloaded_at);

-- Release pre-check (feature 042): the sync gate's pending lookup, ranges and transfers
CREATE INDEX IF NOT EXISTS idx_release_checks_pending ON release_checks(state) WHERE state = 'pending';
CREATE INDEX IF NOT EXISTS idx_release_checks_checked_at ON release_checks(checked_at);
CREATE INDEX IF NOT EXISTS idx_release_checks_transfer_id ON release_checks(transfer_id);
CREATE INDEX IF NOT EXISTS idx_held_magnets_next_at ON held_magnets(next_at);
CREATE INDEX IF NOT EXISTS idx_hold_ends_ended_at ON hold_ends(ended_at);
CREATE INDEX IF NOT EXISTS idx_release_reports_created_at ON release_reports(created_at);
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_schema_contains_create_table() {
        assert!(CREATE_DOWNLOADS_TABLE.contains("CREATE TABLE"));
        assert!(CREATE_DOWNLOAD_HISTORY_TABLE.contains("CREATE TABLE"));
        assert!(CREATE_DOWNLOAD_STATS_TABLE.contains("CREATE TABLE"));
        assert!(CREATE_DOWNLOAD_SOURCES_TABLE.contains("CREATE TABLE"));
        assert!(CREATE_TRANSFER_ORIGINS_TABLE.contains("CREATE TABLE"));
        assert!(CREATE_RULE_DECISIONS_TABLE.contains("CREATE TABLE"));
    }

    #[test]
    fn test_indexes_contain_create_index() {
        assert!(CREATE_INDEXES.contains("CREATE INDEX"));
        assert!(CREATE_INDEXES.contains("idx_downloads_file_id"));
        assert!(CREATE_INDEXES.contains("idx_downloads_status"));
        assert!(CREATE_INDEXES.contains("idx_history_timestamp"));
        assert!(CREATE_INDEXES.contains("idx_history_action"));
        assert!(CREATE_INDEXES.contains("idx_history_extension"));
        assert!(CREATE_INDEXES.contains("idx_history_action_timestamp"));
    }
}
