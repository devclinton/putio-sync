# Data Model: Put.io Doctor

All entities are in-memory and serializable (serde, `snake_case`). Nothing new is persisted.

## Status

`ok | warning | problem | skipped`. Severity order: `ok < warning < problem`, with `skipped`
left out of the overall status. Metric value: ok 0, warning 1, problem 2, skipped -1.

## Area

`putio | sync | local | connectivity`

## CheckId (closed set, run order)

| id | area | Fixes |
|----|------|-------|
| `auth` | putio | none |
| `storage` | putio | none |
| `transfers` | putio | `transfers.retry`, `transfers.readd` (destructive) |
| `empty_folders` | putio | `empty_folders.clean` (destructive) |
| `duplicates` | putio | none |
| `orphaned_torrents` | putio | none |
| `stuck_watch_files` | putio | none |
| `dead_queue_rows` | sync | `dead_queue_rows.remove` (destructive) |
| `failing_downloads` | sync | `failing_downloads.requeue` |
| `orphan_partials` | sync | `orphan_partials.delete` (destructive) |
| `download_dir` | local | none |
| `disk_space` | local | none |
| `database` | local | none |
| `config` | local | none |
| `reachability` | connectivity | none |
| `rate_limit` | connectivity | none |
| `clock_skew` | connectivity | none |

## Report

| field | type | notes |
|-------|------|-------|
| `started_at` | RFC 3339 UTC | |
| `duration_ms` | u64 | |
| `overall` | Status | the worst of the non-skipped results; `ok` if every check was skipped |
| `counts` | `{ok, warning, problem, skipped}` | |
| `checks` | `[CheckResult]` | in CheckId order |

## CheckResult

| field | type | notes |
|-------|------|-------|
| `id` | CheckId | |
| `area` | Area | |
| `title` | string | e.g. "Put.io storage" |
| `status` | Status | |
| `summary` | string | one plain sentence, e.g. "Using 912 GB of 1 TB (91%)." |
| `suggestion` | string? | present when status is not ok |
| `item_count` | u64 | total affected |
| `items` | `[Item]` | at most 100 |
| `fixes` | `[FixInfo]` | only fixes that apply now (`item_count > 0`) |

## Item

| field | type | notes |
|-------|------|-------|
| `key` | string | stable id a fix uses: transfer id, file id, queue file id, local path relative to the download dir, config key path |
| `label` | string | what to show: name, path or key |
| `detail` | string? | e.g. "failed 4 times", "1.2 GB, 3 days old", "availability 92%" |
| `fix` | FixId? | for `transfers`, which of the two fixes applies to this item |

## FixInfo

`{ id: FixId, label: string, destructive: bool }`

## FixPreview

`{ fix: FixId, destructive: bool, items: [Item], item_count: u64 }`. The items are
recomputed at preview time; destructive previews list every item (no 100 cap) so confirm acts
on exactly what was shown.

## FixRequest

`{ items: [string] (keys), confirm: bool }`.

Validation:

- the fix id must be known;
- `items` must be non-empty;
- a destructive fix needs `confirm = true`;
- `items` is capped at 1000.

## FixOutcome

`{ fix, done: [Item], skipped: [{item, reason}], failed: [{item, error}], check: CheckResult }`

`check` is the affected check, run again after the fix (FR-053).

## State held by `Doctor`

- `latest: RwLock<Option<Report>>`
- `running: tokio::Mutex<()>`, the single-flight guard (R7)
- `config: RwLock<Config>`, refreshed by `configure()` after a settings save, like the cleanup
  service
- shared handles: the client, the database, the stall tracker (optional) and the cleanup service

## Thresholds (constants)

| name | value |
|------|-------|
| storage warning / problem | 90% / 98% used |
| disk warning / problem | < 10% free or < pending queue bytes / < 2% free |
| failing-download retries | ≥ 3 |
| watch-file and partial-file age | > 1 h |
| duplicate minimum size | 1 MiB |
| latency warning | > 2 s |
| rate-limit warning | remaining < 10% of limit, or a 429 |
| clock skew warning / problem | > 60 s / > 300 s |
| run deadline | 5 min |
| walk concurrency | 4 |
| items per check | 100 |
