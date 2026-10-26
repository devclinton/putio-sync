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

/// SQL to create performance indexes
/// These indexes optimize common query patterns
pub const CREATE_INDEXES: &str = r#"
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
