# Data Model: Download Verification

## Config (`[download]`, putio-sync-core `DownloadConfig`)

| Key | Type | Default | Rule |
|-----|------|---------|------|
| `verify` | bool | `true` | — |
| `verify_max_redownloads` | u32 | `2` | 0..=5 |
| `verify_read_limit_mb_per_sec` | u64 | `50` | 1..=10000 (MB = 1 000 000 bytes) |
| `verify_follow_bandwidth_pause` | bool | `true` | — |

Errors name the key, for example `download.verify_max_redownloads must be 0..=5 (got 9)`.

## Columns added

| Table | Column | Type | Meaning |
|-------|--------|------|---------|
| `downloads` | `crc32` | TEXT NULL | Put.io's CRC32 when the file was downloaded |
| `downloads` | `verify` | TEXT NULL | `crc32`, `size` or `none` (verification off) |
| `downloads` | `verified_at` | TIMESTAMP NULL | Last pass (download or run) |
| `download_history` | `verify` | TEXT NULL | `crc32`, `size`, `none`; NULL for rows before 034 |
| `download_history` | `redownloads` | INTEGER NULL | Re-downloads this download took |
| `download_queue` | `redownload` | INTEGER NOT NULL DEFAULT 0 | 1: overwrite whatever is at the path (queued by a run) |

## `verify_runs`

| Column | Type | Notes |
|--------|------|-------|
| `id` | INTEGER PK | |
| `scope` | TEXT | `everything` or `paths` |
| `report_only` | INTEGER | 0/1 |
| `state` | TEXT | `running` while going; `finished`, `stopped` or `failed` once over |
| `error` | TEXT NULL | why a run failed (walk failed) |
| `started_at`, `ended_at` | TIMESTAMP | `ended_at` NULL while going |
| `files_total`, `files_done` | INTEGER | |
| `bytes_total`, `bytes_done` | INTEGER | |
| `verified`, `mismatch`, `missing`, `not_on_putio`, `busy`, `unreadable`, `not_checked` | INTEGER | counts |

A row left `running` at startup (the daemon stopped mid-run) is set to `stopped`. Only
the latest 10 runs are kept.

## `verify_problems`

| Column | Type | Notes |
|--------|------|-------|
| `id` | INTEGER PK | |
| `run_id` | INTEGER NULL | NULL for a download's failure |
| `source` | TEXT | `download` or `run` |
| `file_id` | INTEGER | Put.io id |
| `name` | TEXT | |
| `local_path` | TEXT NULL | |
| `result` | TEXT | `mismatch`, `missing`, `not_on_putio`, `busy`, `unreadable`, `not_checked`, `failed` (download gave up) |
| `detail` | TEXT NULL | expected/actual CRC32, error text (token-free) |
| `queued` | INTEGER | 1 when queued to download again |
| `at` | TIMESTAMP | |
| `fixed_at` | TIMESTAMP NULL | set when the file id later passes |

## Run state (in memory, `VerifyStatus`)

```text
idle ──start──▶ running ──pause──▶ paused ──resume──▶ running
                  │  ▲                                   
     bandwidth    │  │ pause ends                        
     paused       ▼  │                                   
                waiting                                  
running|paused|waiting ──stop──▶ stopping ──▶ idle (run row: stopped)
running ──last file──▶ idle (run row: finished)
```

`VerifyStatus { state, run_id, scope, report_only, files_total, files_done, bytes_total,
bytes_done, counts, current (name), waiting_reason, read_limit_bytes_per_sec }`.

## Verification of one download

```text
transfer ok ─▶ method?
  crc32 known & (verify on | resumed) ─▶ compare
       match ─▶ passed(crc32) ─▶ rename ─▶ verify.passed ─▶ download.finished
       mismatch ─▶ verify.failed ─▶ n < max ? verify.redownload, delete hidden, transfer fresh
                                        : fail(integrity) + verify_problems(failed)
  no crc32 / verify off ─▶ size already checked ─▶ passed(size | none)
```
