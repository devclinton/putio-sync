# Quickstart: Validating Feature 002

## 1. Local gate

```bash
make ci
```

Expect exit code 0, plus these new tests:
- `stale_connection` (client)
- concurrent claim (db)
- already-uploaded cleanup (file-manager)
- idle scan log level (file-manager)
- `is_blocked` and blocked sync (download)
- log level source (core)
- download-interval wiring (cli/daemon)

## 2. Targeted runs

```bash
cargo test -p putio-sync-db claim
cargo test -p putio-sync-client --test stale_connection
cargo test -p putio-sync-file-manager already_uploaded
cargo test -p putio-sync-download blocked
```

## 3. Local daemon smoke test (no Put.io)

Point the daemon at a mockito-style local stub, or reuse the integration tests. Then check:

- With a config containing `[watch] paths = [<tmp>]` and `delete_after_upload = true`, the
  startup log shows the real paths and flags. No `Watch paths: []` line appears.
- `--download-interval 30` produces a sync roughly every 30 s, with no "not supported"
  warning.
- With `RUST_LOG=info` set, one line reads `Log level: info (from RUST_LOG)`.

## 4. On the NAS, after the operator deploys

| Check | Command (read-only) | Expect |
|-------|---------------------|--------|
| Stuck watch files cleared | `ls /mnt/user/appdata/torrents/blackhole/{radarr,sonarr} \| wc -l` | Drops from 41 to only new grabs, within one scan |
| No first-request retries | `docker logs --since 24h putio-sync 2>&1 \| grep -c 'retrying'` | < 5 |
| No lock errors | `docker logs --since 168h putio-sync 2>&1 \| grep -c 'database is locked'` | 0 |
| Executables blocked | `docker logs --since 24h putio-sync 2>&1 \| grep 'Blocked file'` | One WARN per blocked file, no repeats |
| Log volume | `docker logs --since 24h putio-sync 2>&1 \| wc -l` | ≤ 40% of the ≈7,000/day baseline |

The NAS also needs WAL sidecar files to be writable next to `/downloads/.putiosync.db`, which
the existing bind mount already allows.
