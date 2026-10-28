# Contract: `putio-sync archive`

```text
putio-sync [--config <path>] archive <metrics|db|summary> [options]
```

Common options (flag, then env variable):

| Option | Env | Default | Used by |
|---|---|---|---|
| `--dir <path>` | `PUTIO_SYNC_ARCHIVE_DIR` | `<config dir>/archive` (`/config/archive` in Docker) | all |
| `--prometheus <url>` | `PUTIO_SYNC_ARCHIVE_PROMETHEUS_URL` | none | `metrics` (required), `summary` (optional) |
| `--instance <value>` | `PUTIO_SYNC_ARCHIVE_INSTANCE` | the only `instance` with `app="putio-sync"`; error if several | `metrics`, `summary` |
| `--keep-days <n>` | `PUTIO_SYNC_ARCHIVE_KEEP_DAYS` | unset = keep forever | `metrics`, `db` |
| `--require-manifest` | — | off | all; fail if `manifest.jsonl` is missing (guards an unmounted share) |

None of them read or print a Put.io token or an API token. `db` uses the database path from
the config, exactly as the daemon does.

## `archive metrics`

| Option | Default |
|---|---|
| `--since <YYYY-MM-DD>` | 366 days before today (UTC) |

For each finished UTC day from `--since` to yesterday that is not in `manifest.jsonl`, it
fetches `{__name__=~"putio_sync:.+:[a-z]*5m",app="putio-sync",instance="<i>"}[1d]` at
`<day+1>T00:00:00Z`. Samples with timestamp `< day+1` and `>= day` are kept. It then writes
`metrics/putio-sync-metrics-<day>.om.gz` and appends a manifest line. Days with no samples are
logged as `no data` and not written, so a later run tries them again.

Output, one line per day: `saved 2026-11-01 points=288/288` or `saved 2026-11-02 points=201/288 partial`.

## `archive db`

It runs `VACUUM INTO '<dir>/db/.tmp-<uuid>.sqlite'`, gzips that to
`db/putio-sync-db-<today>.sqlite.gz.tmp`, fsyncs, renames it, and deletes the temp copy. It
appends a manifest line with the download-history row count. If today's file already exists,
it prints `already saved <today>` and exits 0. It never writes to the live DB. It runs fine
while the daemon runs.

## `archive summary`

| Option | Default |
|---|---|
| `--from <YYYY-MM-DD>` | first day in the download history |
| `--to <YYYY-MM-DD>` | today (UTC); today's row is marked partial by `resource_coverage` |

It reads download history through `putio-sync-db` (`daily_totals`). With `--prometheus`, it
fetches the 5-minute series per day as `archive metrics` does. Without it, or for days with no
samples, the resource columns are blank. It writes `summary/usage-daily.csv`,
`summary/usage-monthly.csv` and `summary/usage-latest-month.txt` (columns in
[data-model.md](../data-model.md#usage-summary-row)), each through temp and rename. Re-running
for the same inputs gives byte-identical files.

## Files and safety

- **Atomic writes**: every file is written as `<name>.tmp` in the same folder, fsynced, then
  renamed. Leftover `.tmp` files from a crash are deleted at the start of the next run.
- **Pruning**: `--keep-days` deletes only `putio-sync-metrics-*.om.gz` or
  `putio-sync-db-*.sqlite.gz` files older than N days, and their manifest lines. It never
  deletes other files.
- **Missing mount**: the archive dir must already exist, or its parent must be writable and
  the dir is created. If the dir looks like an empty mount point (it is a mount point with no
  `manifest.jsonl` while one is expected, via `--require-manifest`), the command exits with
  error. The Unraid wrapper passes `--require-manifest` after the first run.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Done, including "nothing to do" |
| 1 | Bad arguments or config |
| 2 | Prometheus unreachable or returned an error |
| 3 | Database could not be read or copied |
| 4 | Archive folder missing, not writable, or disk full |

Errors go to stderr as one line, `archive <cmd>: <what failed>: <cause>`, with no secrets.

## Unraid wrapper (`scripts/archive/unraid-user-script.sh`)

The daily User Scripts entry, set to "Custom" at `30 3 * * *`:

```bash
#!/bin/bash
set -euo pipefail
P=http://192.168.1.51:9090
run() { docker exec putio-sync putio-sync --config /config/config.toml archive "$@"; }
run metrics --prometheus "$P" --require-manifest
run summary --prometheus "$P"
[ "$(date +%u)" = 1 ] && run db    # weekly, Mondays, before the backup restart window
```

It exits non-zero if any step fails, which User Scripts shows in its log. The NAS reaches the
monitoring server's `:9090` over the LAN.
