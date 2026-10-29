# Data Model: What to do when a file already exists

## ExistingFile (core, config)

`putio-sync-core::config::ExistingFile`, serialised in snake case.

| Value | TOML | Meaning |
|---|---|---|
| Resume | `"resume"` (default) | Skip complete files, continue partial ones |
| Overwrite | `"overwrite"` | Download again from the start, replace when complete |
| KeepBoth | `"keep_both"` (alias `"rename"`) | Keep the old file, save the new one as `name (n).ext` |

- `From<bool>` for compatibility: `true` → Resume, `false` → Overwrite.
- `DownloadConfig.on_existing: Option<ExistingFile>` and `DownloadConfig::on_existing()` →
  `on_existing.unwrap_or(skip_existing.into())`.
- `FolderRule.on_existing: Option<ExistingFile>`. The rule's effective value is
  `on_existing.or(skip_existing.map(Into::into))`. `FolderRule::is_empty` checks both.
- `Effective.on_existing: Sourced<ExistingFile>` replaces `Effective.skip_existing`.
- `RuleSet` stores the resolved global choice in place of `skip_existing: bool`.

## ExistingOutcome (core)

`putio-sync-core::config::ExistingOutcome`: `skipped`, `resumed`, `restarted`, `overwritten`,
`saved_as`, `refused`. Stored as text in `download_history.outcome`, and mirrored by a closed
label enum in `putio-sync-telemetry`.

`SaveAsReason`: `keep_both`, `larger`, `checksum`. Logged only, not stored.

## ExistingAction (download, internal)

Output of `files::plan_existing`. See the table in [research.md](research.md) R2.

| Action | Then |
|---|---|
| `Fresh` | Download as today; no outcome recorded |
| `Skip` | Record `downloads` row `completed` and history `skipped`; no bytes fetched |
| `Resume { from }` | Rename the real file to `.<name>.downloading`, range request from `from`, checksum check (R3) |
| `Overwrite` | Download into `.<name>.downloading`, rename over the real file when complete |
| `SaveAs { path, reason }` | Store `save_as` on the queue row, download to `path` through its hidden file |
| `Refuse` | Fail the file with "target path is a folder"; touch nothing |

Inputs:

- `LocalState`: `Missing`, `Dir`, or `File { size }`.
- `Remote`: `size`, `crc32: Option<String>`.
- `choice: ExistingFile`, `resume_enabled: bool`, `fixed_path: bool`.
- A lazily computed local CRC32, only for a same-size file when `crc32` is `Some`.

After `Resume`, a checksum mismatch becomes `restarted`: delete the hidden file, download once
from the start, then fail on a second mismatch.

## DownloadFile (download)

New fields:

| Field | Type | Source |
|---|---|---|
| `crc32` | `Option<String>` | `get_file_details` in the queue worker; `None` elsewhere |
| `save_as` | `Option<String>` | `download_queue.save_as` |
| `fixed_path` | `bool` | `download_queue.fixed_path` |

The CLI, API and server constructors set `None` / `None` / `false`.

## ActiveDownload (download)

New field `phase`: `"downloading"` (default) or `"checking"`. While checking, `bytes_downloaded`
is the bytes read for the checksum.

## Manager DownloadConfig (download)

- `skip_existing: bool` → `on_existing: ExistingFile` (global, from `DownloadConfig::on_existing()`).
- New `resume_enabled: bool`.
- `effective(path).on_existing` gives the per-folder value.

## Database (db)

Added by migrations in `Database::migrate` (check `pragma_table_info`, then `ALTER TABLE`):

| Table | Column | Type | Notes |
|---|---|---|---|
| `download_queue` | `save_as` | `TEXT NULL` | Numbered file name chosen for "keep both"; kept across retries |
| `download_queue` | `fixed_path` | `INTEGER NOT NULL DEFAULT 0` | 1 for Radarr/Sonarr item files; upsert keeps 1 |
| `download_history` | `local_path` | `TEXT NULL` | Path the file was saved to; NULL for old rows and deletes |
| `download_history` | `outcome` | `TEXT NULL` | `ExistingOutcome`; NULL for a plain new download |

`downloads.local_path` already holds the written path, which is now the numbered one for
"keep both".

## Metric (telemetry)

`putio_sync_existing_files_total{outcome}`: counter, one series per `ExistingOutcome`, all
pre-registered at zero.

## Validation

- An unknown `on_existing` value fails config load with the allowed values in the message, as
  other enums do.
- `on_existing` and `skip_existing` both set in one table: `on_existing` wins, with one warning
  at load.
- `save_as` must be a single path segment. A row whose `save_as` fails this is cleared and
  re-decided.
