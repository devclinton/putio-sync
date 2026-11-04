# Data Model: Put.io Storage Management

## Config: `[storage]` (`StorageConfig`, core)

| Field | Type | Default | Rule |
|-------|------|---------|------|
| `warn_free_percent` | u8 | 10 | 1 ≤ warn < target |
| `target_free_percent` | u8 | 20 | warn < target ≤ 90 |
| `auto_free` | bool | false | daily run on/off (daemon only) |
| `run_at` | string | "02:30" | 24h `HH:MM`, local time |

## SQLite

### `storage_runs`

| Column | Type | Notes |
|--------|------|-------|
| `id` | INTEGER PK AUTOINCREMENT | |
| `finished_at` | TIMESTAMP DEFAULT CURRENT_TIMESTAMP | UTC |
| `trigger` | TEXT | `manual` or `schedule` |
| `files` | INTEGER | files deleted |
| `bytes` | INTEGER | sum of deleted sizes |
| `failed` | INTEGER | files that failed to delete |
| `free_after` | INTEGER NULL | Put.io free bytes after the run, when read |

Only runs that deleted at least one file are stored.

### `storage_run_files`

| Column | Type | Notes |
|--------|------|-------|
| `run_id` | INTEGER NOT NULL | → `storage_runs.id` |
| `file_id` | INTEGER NOT NULL | Put.io id |
| `path` | TEXT NOT NULL | Put.io path at plan time |
| `size` | INTEGER NOT NULL | bytes |
| `created_at` | TEXT NULL | Put.io date added |
| `local_path` | TEXT NOT NULL | the verified local copy |

Index `idx_storage_run_files_run` on `run_id`.

### Read from existing tables

- `downloads`: `file_id, file_size, local_path WHERE status = 'completed'`.
- `download_queue`: `file_id WHERE status IN ('pending','downloading','paused')`.
- `download_history`: one `deleted` row per deleted file.

## In memory (storage crate)

- **StorageSummary**: `size, used, free, used_percent, low (bool), warn_free_percent,
  target_free_percent, top: [TopItem]`. `low` is false when `size ≤ 0`.
- **TopItem**: `id, name, size, is_folder, share_percent` (of used).
- **Candidate / PlannedFile**: `id, path, size, created_at, local_path, ancestors`
  (ancestors not serialized).
- **FreePlan**: `id (uuid), created (Instant), free_before, target_bytes, needed_bytes,
  total_bytes, free_after, shortfall_bytes, complete (walk), files: [PlannedFile]`.
  Kept as the service's latest plan for 30 minutes.
- **FreeResult**: `run_id (option), deleted: [PlannedFile], skipped: [{id, path, reason}],
  failed: [{id, path, error}], bytes_deleted, free_after (option)`.
- **StorageRun** (read model): the `storage_runs` row with its `files`.

## State

A plan: created by `plan()` → consumed by `free()` (dropped) or replaced by the next
`plan()` or expired after 30 minutes. A run: one `free()`; recorded when `deleted` is
non-empty.
