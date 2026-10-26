# Data Model: Put.io Empty-Folder Cleanup

Nothing is persisted; these are in-memory and wire types.

## CleanupConfig (`[cleanup]` in config.toml, `putio-sync-core`)

| Field | Type | Default | Rule |
|-------|------|---------|------|
| `enabled` | bool | `false` | Daily run on/off (daemon only) |
| `run_at` | string `HH:MM` | `"03:00"` | 24-hour local time; `00:00`–`23:59` |
| `min_age_hours` | u64 | `24` | `0`–`8760` |
| `nested` | bool | `true` | Also remove folders whose sub-folders are all empty; `false` removes only folders with nothing in them |

## EmptyFolderOptions (`putio-sync-client`)

- `dry_run: bool`
- `min_age: Duration`
- `protected: HashSet<i64>` (from config: `watch.parent_folder_id`, `sync.remote_parent_id`)
- `now: DateTime<Utc>`
- `nested: bool`

Root (0), shared entries and active transfer targets are added by the walk itself.

## EmptyFolderReport

| Field | Meaning |
|-------|---------|
| `dry_run` | Whether anything was deleted |
| `scanned` | Folders listed |
| `removed` | `[RemovedFolder]`, every folder removed (or that would be), nested ones included, parents after children |
| `kept_protected` | Empty folders kept because protected |
| `kept_too_new` | Empty folders kept because younger than the minimum age or of unknown age |
| `failures` | `[CleanupFailure]`, list or delete errors |

**RemovedFolder**: `id: i64`, `path: String` (from the root, `/`-joined), `age_hours: u64`.

**CleanupFailure**: `id: i64`, `step` (`list` or `delete`), `path: String`, `error: String` (client error text; no
tokens).

A kept folder (protected, too new, failed to list or delete) makes every ancestor non-empty.

## CleanupError (`putio-sync-core`)

- `AlreadyRunning` → API 409
- `Client(ClientError)` → the run could not start (e.g. transfer list or root listing
  failed) → API 502/500 per existing `ApiError` mapping

## Run lifecycle

`idle → running → idle`. A second request while `running` gets `AlreadyRunning`. A finished
non-dry run records metrics: result `success` when `failures` is empty, else `failure`.
