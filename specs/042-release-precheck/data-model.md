# Data Model: Release Pre-check (042)

## Table `release_checks` (new)

One row per check: every *arr add and every watch-folder file while the check is on.

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PK AUTOINCREMENT | |
| `hash` | TEXT | Info hash, 40 lower-case hex characters. NULL when the .torrent couldn't be parsed |
| `name` | TEXT NOT NULL | Release name, from the .torrent, the magnet `dn`, or the watch file name |
| `source` | TEXT NOT NULL CHECK IN (`torrent`,`url`,`magnet`,`watch_torrent`,`watch_magnet`) | |
| `category` | TEXT | The *arr category, or NULL for the watch folder |
| `watch_folder` | TEXT | The watch path, or NULL for *arr items |
| `stage` | TEXT NOT NULL CHECK IN (`before`,`putio`) | `putio` once a fallback is checked there |
| `state` | TEXT NOT NULL CHECK IN (`passed`,`rejected`,`pending`) | `pending` means waiting for the check on Put.io |
| `lookup` | TEXT CHECK IN (`found`,`not_found`,`timeout`,`wrong_hash`,`error`,`off`) | Magnets only; the best result across the caches |
| `lookup_cache` | TEXT | Host name of the cache that answered `found` |
| `transfer_id` | INTEGER | The Put.io transfer, when one was created |
| `total_size` | INTEGER NOT NULL DEFAULT 0 | From the file list, or from Put.io for fallbacks |
| `putio_bytes` | INTEGER NOT NULL DEFAULT 0 | Bytes Put.io downloaded. Only for `stage = putio` |
| `reason` | TEXT | For `rejected` |
| `checked_at` | TIMESTAMP DEFAULT CURRENT_TIMESTAMP | When the row was created |
| `resolved_at` | TIMESTAMP | When a `pending` row became `passed` or `rejected` |

Indexes: `(state) WHERE state = 'pending'`, `(checked_at)`, `(transfer_id)`.

**States**

```text
add / scan ──▶ passed                       (file list known, rule passed)
           ──▶ rejected                     (file list known, rule failed)
           ──▶ pending ──▶ passed  (stage = putio)
                       ──▶ rejected (stage = putio; cancel + delete on Put.io)
```

The sync gate (research R8) skips files whose transfer has a `pending` row, so every pending row
must resolve. A pending row resolves to `rejected` with reason `Gone` ("transfer no longer on
Put.io") when any of these happens:

- its transfer has disappeared from Put.io;
- its transfer is in `ERROR`;
- its transfer is cancelled;
- its *arr item is removed.

These rows are not counted as fake.

## Table `release_reports` (new)

One row per rejection and per flag (FR-025).

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PK AUTOINCREMENT | Returned to the UI so a flag can be undone |
| `kind` | TEXT NOT NULL CHECK IN (`rejected`,`flagged_fake`,`flagged_not_fake`) | |
| `check_id` | INTEGER | → `release_checks.id`, for `rejected` rows and for flags on a checked release |
| `arr_hash` | TEXT | → `arr_items.hash`, when the release is an *arr item |
| `file_id` | INTEGER | Put.io file or folder flagged, if any |
| `transfer_id` | INTEGER | |
| `source` | TEXT | As `release_checks.source`, plus `putio` and `manual` for releases from plain sync |
| `category` / `watch_folder` | TEXT | |
| `stage` | TEXT | `before`, `putio`, or NULL for a flag |
| `name` | TEXT NOT NULL | |
| `hash` | TEXT | |
| `total_size` | INTEGER | |
| `file_count` | INTEGER | |
| `types` | TEXT NOT NULL DEFAULT `'[]'` | JSON: `[{"ext":"exe","count":1,"largest":1503238553}]`, sorted by largest. An empty `ext` means no extension |
| `largest_wanted` | INTEGER | Size of the largest wanted file, 0 when none |
| `min_size` | INTEGER | The minimum in force, for rejections |
| `tracker_hosts` | TEXT NOT NULL DEFAULT `'[]'` | JSON list of host names, never full announce addresses |
| `is_private` | INTEGER NOT NULL DEFAULT 0 | |
| `reason` | TEXT | |
| `note` | TEXT | The operator's note, at most 500 characters |
| `created_at` | TIMESTAMP DEFAULT CURRENT_TIMESTAMP | |

Rules:

- No magnet link, URL, API key or .torrent bytes is ever stored here (FR-025).
- One active flag per release and kind. A second "Fake" flag on the same `arr_hash`/`file_id`
  replaces the note.
- Undo deletes the flag row. `rejected` rows cannot be undone. A "Not fake" flag goes next to them.

Both tables are pruned after 400 days (research R10).

## Changes to `arr_items` (011)

- `transfer_id` may be NULL for an item rejected before Put.io. The column was already nullable,
  so no schema change is needed.
- A rejected item goes straight into `state = failed`, with
  `blocklist = pending|none` as for any failure.
- `refresh` and `remove` skip the Put.io calls when `transfer_id` is NULL.

## Configuration (`[precheck]`, new, in `putio-sync-core`)

```toml
[precheck]
enabled = true              # default for every *arr category and the watch folder
dry_run = false             # check, log and count, but never reject or delete (Constitution IV)
min_size_mb = 50
allow_archives = false
magnet_lookup = true
lookup_timeout_secs = 10    # total time for all caches; 1..=30
caches = ["https://itorrents.net/torrent/{HASH}.torrent"]

[precheck.categories.software]    # key: the *arr category (folder below the download dir)
enabled = false

[precheck.watch."/watch/tv"]      # key: a path from [watch].paths
min_size_mb = 100
```

Validation at load and on save:

- `min_size_mb` must be 0..=100000;
- every cache must be http(s) and contain `{HASH}` or `{hash}`;
- a `watch` key must match one of `[watch].paths`. This is a warning, not an error, so a removed
  path doesn't break startup.

Environment overrides: `PUTIO_SYNC_PRECHECK_ENABLED` and `PUTIO_SYNC_PRECHECK_MAGNET_LOOKUP`.

## In-memory types (`putio-sync-precheck`)

- `ReleaseFile { path, size }`
- `Metainfo { hash, name, files: Vec<ReleaseFile>, private, tracker_hosts }`
- `Policy { enabled, wanted: BTreeSet<String>, wanted_from: Option<String> /* 009 rule path */,
  min_size, allow_archives }`
- `Verdict::Pass { largest_wanted }` | `Verdict::Reject(Reason)`, where `Reason` is
  `NoWanted { found: Vec<String> }`, `SmallWanted { largest, min }`, `Invalid`, or `Gone` (a
  pending fallback whose transfer can no longer be checked; never counted as fake)
- `Lookup` outcome: `Found(Metainfo, cache_host)` | `Missed(LookupResult)`
