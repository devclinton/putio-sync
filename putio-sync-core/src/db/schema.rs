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

/// SQL to create the download_chunks table
pub const CREATE_DOWNLOAD_CHUNKS_TABLE: &str = r#"
CREATE TABLE IF NOT EXISTS download_chunks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    download_id INTEGER NOT NULL,
    chunk_index INTEGER NOT NULL,
    start_byte INTEGER NOT NULL,
    end_byte INTEGER NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('pending', 'downloading', 'completed')),
    FOREIGN KEY(download_id) REFERENCES downloads(id) ON DELETE CASCADE,
    UNIQUE(download_id, chunk_index)
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
    priority INTEGER DEFAULT 0,
    status TEXT NOT NULL CHECK(status IN ('pending', 'downloading', 'paused', 'completed', 'failed')),
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    UNIQUE(file_id)
)
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

/// SQL to create performance indexes
/// These indexes optimize common query patterns
pub const CREATE_INDEXES: &str = r#"
-- Index for finding downloads by file_id
CREATE INDEX IF NOT EXISTS idx_downloads_file_id ON downloads(file_id);

-- Index for finding downloads by status
CREATE INDEX IF NOT EXISTS idx_downloads_status ON downloads(status);

-- Index for download_chunks queries by download_id
CREATE INDEX IF NOT EXISTS idx_chunks_download_id ON download_chunks(download_id);

-- Index for finding pending chunks
CREATE INDEX IF NOT EXISTS idx_chunks_status ON download_chunks(status);

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
    fn test_schema_constants_not_empty() {
        assert!(!CREATE_DOWNLOADS_TABLE.is_empty());
        assert!(!CREATE_DOWNLOAD_CHUNKS_TABLE.is_empty());
        assert!(!CREATE_DOWNLOAD_HISTORY_TABLE.is_empty());
        assert!(!CREATE_DOWNLOAD_STATS_TABLE.is_empty());
        assert!(!INIT_DOWNLOAD_STATS.is_empty());
        assert!(!CREATE_INDEXES.is_empty());
    }

    #[test]
    fn test_schema_contains_create_table() {
        assert!(CREATE_DOWNLOADS_TABLE.contains("CREATE TABLE"));
        assert!(CREATE_DOWNLOAD_CHUNKS_TABLE.contains("CREATE TABLE"));
        assert!(CREATE_DOWNLOAD_HISTORY_TABLE.contains("CREATE TABLE"));
        assert!(CREATE_DOWNLOAD_STATS_TABLE.contains("CREATE TABLE"));
    }

    #[test]
    fn test_indexes_contain_create_index() {
        assert!(CREATE_INDEXES.contains("CREATE INDEX"));
        assert!(CREATE_INDEXES.contains("idx_downloads_file_id"));
        assert!(CREATE_INDEXES.contains("idx_downloads_status"));
        assert!(CREATE_INDEXES.contains("idx_chunks_download_id"));
        assert!(CREATE_INDEXES.contains("idx_chunks_status"));
        assert!(CREATE_INDEXES.contains("idx_history_timestamp"));
        assert!(CREATE_INDEXES.contains("idx_history_action"));
        assert!(CREATE_INDEXES.contains("idx_history_extension"));
        assert!(CREATE_INDEXES.contains("idx_history_action_timestamp"));
    }
}
