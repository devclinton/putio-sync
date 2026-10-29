# Data Model: CPU and Memory Telemetry, Kept Long Term

Nothing here is a new database table. The entities are metric series, files and report rows.

## Process figures (in putio-sync, read at each scrape)

| Figure | Source on Linux | Kind | Missing when |
|---|---|---|---|
| CPU seconds | `/proc/self/stat` fields 14+15 (utime+stime) ÷ clock ticks | counter | not Linux, or stat unreadable |
| Resident memory bytes | `/proc/self/status` `VmRSS` (kB × 1024) | gauge | not Linux |
| Virtual memory bytes | `/proc/self/status` `VmSize` | gauge | not Linux |
| Threads | `/proc/self/status` `Threads` | gauge | not Linux |
| Open files | entries in `/proc/self/fd` | gauge | not Linux, or dir unreadable |
| Open-file limit | `/proc/self/limits`, "Max open files" soft value; absent if `unlimited` | gauge | not Linux, or unlimited |
| Disk read bytes | `/proc/self/io` `read_bytes` | counter | not Linux, or `io` unreadable (some kernels and containers) |
| Disk written bytes | `/proc/self/io` `write_bytes` | counter | as above |
| Sync in progress | set by the sync loop | gauge 0/1 | never (0 at start) |

Rules:
- A figure that fails on one scrape is left out of that scrape only. The startup warning
  (FR-005) lists the figures that failed on the first read.
- Parsers accept the fixture text from a real NAS read and reject malformed input with `None`,
  never a panic.

## Recorded 5-minute series (main Prometheus, remote-written to long-term)

Names follow `putio_sync:<figure>:<aggregation>5m`. Every series keeps the scrape labels `job`,
`instance`, `host` and `app` and adds none. Full expressions are in
[contracts/long-term.md](contracts/long-term.md).

| Series | Meaning |
|---|---|
| `putio_sync:process_cpu_usage:avg5m` | Average cores in use over 5 min |
| `putio_sync:process_cpu_usage:max5m` | Peak 1-minute rate inside the 5 min |
| `putio_sync:process_resident_memory_bytes:avg5m` / `:max5m` | Memory in use |
| `putio_sync:process_virtual_memory_bytes:max5m` | Peak memory reserved |
| `putio_sync:process_threads:avg5m` / `:max5m` | Threads |
| `putio_sync:process_open_fds:avg5m` / `:max5m` | Open files |
| `putio_sync:process_max_fds:last5m` | Open-file limit |
| `putio_sync:process_disk_read_bytes:increase5m` / `putio_sync:process_disk_written_bytes:increase5m` | Disk I/O in the 5 min |
| `putio_sync:downloads_active:avg5m` / `:max5m` | Downloads running |
| `putio_sync:downloaded_bytes:increase5m` | Bytes downloaded in the 5 min |
| `putio_sync:sync_in_progress:avg5m` | Share of the 5 min spent syncing (0–1) |
| `putio_sync:activity_state:5m` | Activity state, below |
| `putio_sync:start_time_seconds:max5m` | Process start; a change means a restart |
| `putio_sync:build_info:max5m` | Keeps `version` and `commit` labels (value 1) |
| `putio_sync:up:min5m` | 1 if every scrape in the 5 min succeeded |

That is about 22 series per instance.

## Activity state

The value of `putio_sync:activity_state:5m` is one of:

| Value | State | Rule (over the 5 min) |
|---|---|---|
| `N ≥ 1` | downloading with N at once | `round(avg downloads_active) ≥ 1`; N = that rounded value |
| `-1` | syncing | not downloading, and `avg sync_in_progress ≥ 0.5` |
| `0` | idle | otherwise |

Downloading wins over syncing. N is bounded by the configured worker count, but is not
clamped to it, so a worker-count change keeps the meaning.

## Metrics archive day

- **File**: `<dir>/metrics/putio-sync-metrics-YYYY-MM-DD.om.gz`, holding the UTC day
  `[00:00, 24:00)`.
- **Content**: OpenMetrics text with every `putio_sync:.+:5m` sample of that day, one
  `# TYPE <name> gauge` per family, ending `# EOF`.
- **Manifest line** (`<dir>/manifest.jsonl`):
  `{"kind":"metrics","day":"2026-11-01","file":"metrics/...","saved_at":"...","points":288,"expected":288,"partial":false}`.
  - `points` is the sample count of `putio_sync:up:min5m`.
  - `partial` is `points < expected`.
- **State**:
  - The day is still running, so it is skipped.
  - The day is finished and missing from the manifest, so it is written.
  - The day is listed in the manifest, so it is skipped for good, even if partial.
- A day with no samples at all is not written. It is logged as "no data".

## Database archive

- **File**: `<dir>/db/putio-sync-db-YYYY-MM-DD.sqlite.gz`, from `VACUUM INTO` a temp file, then
  gzip.
- **Manifest line**: `{"kind":"db","day":..., "file":..., "saved_at":..., "rows": <download history rows>}`.
- A second run on the same UTC day replaces nothing. It reports "already saved today".

## Usage summary row

One row per UTC day (`usage-daily.csv`) and per calendar month (`usage-monthly.csv`).

| Column | From | Notes |
|---|---|---|
| `period` | — | `YYYY-MM-DD` or `YYYY-MM` |
| `downloads`, `bytes`, `download_seconds`, `avg_speed_bytes_per_s` | download history (`daily_totals`) | always filled |
| `idle_s`, `syncing_s`, `downloading_s` | count of 5-min points per state × 300 | blank if no thinned data |
| `downloading_1_s` … `downloading_<max>_s` | same, per N | `<max>` is the largest N seen in the file's range; columns are fixed per run |
| `cpu_avg_<state>`, `cpu_max_<state>` | `cpu_usage:avg5m` / `:max5m` paired with each point's state | cores; average weights each point equally |
| `mem_avg_<state>_bytes`, `mem_max_<state>_bytes` | `resident_memory_bytes:avg5m` / `:max5m` | |
| `cpu_per_mbps`, `mem_per_mbps_bytes` | averaged over downloading points: cpu ÷ (bytes/300/1e6) | points with 0 bytes skipped |
| `restarts` | distinct `start_time_seconds:max5m` values in the period, minus one; plus one if the period's first value differs from the previous period's last | |
| `versions` | distinct `version` labels of `build_info:max5m`, joined with `;` | quoted CSV field |
| `resource_coverage` | thinned points ÷ expected points | 0–1; blank resource columns when 0 |

`<state>` is one of `idle`, `syncing` or `downloading`, plus `downloading_<N>` for each N.
Blank, never 0, when there is no data (spec US4 scenario 6). The output is deterministic for
the same inputs (FR-015a).

`usage-latest-month.txt` gives the same figures for the latest full or current month as short
lines, for example `Downloading 2 at once: 31 h, CPU avg 0.12 cores (peak 0.80), memory avg
48 MB (peak 95 MB)`.
