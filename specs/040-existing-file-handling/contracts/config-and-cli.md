# Contract: config and CLI (feature 040)

## `config.toml`

```toml
[download]
# What to do when a file already exists at its local path and the download history
# does not list it as done:
#   resume     skip complete files, continue partial ones (default)
#   overwrite  download again from the start, replace the old file once the new one is complete
#   keep_both  keep the old file, save the new one as "name (1).ext", "name (2).ext", ...
# Complete means same size as on Put.io, and same CRC32 when Put.io has one.
# Radarr/Sonarr download-client items never get a numbered name; they overwrite instead.
# Replaces skip_existing (true = resume, false = overwrite), which is still read.
on_existing = "resume"

# Continue interrupted and partial downloads instead of starting over (default true).
resume_enabled = true

[[download.rules]]
path = "backups"
on_existing = "overwrite"
```

- Allowed values: `resume`, `overwrite`, `keep_both`; `rename` is read as `keep_both`.
- `on_existing` absent → `skip_existing` decides; both absent → `resume`.
- A save from the web UI writes `on_existing` and removes `skip_existing` from the same table.
- No environment override, like the other `[download]` keys.

## CLI

```text
putio-sync download [...] [--on-existing <resume|overwrite|keep-both>] [--skip-existing] [--no-resume]
putio-sync daemon   [...] [--on-existing <resume|overwrite|keep-both>] [--skip-existing]
```

- `--on-existing` overrides `[download] on_existing` for this run. Folder rules still override it
  per folder.
- `--skip-existing` is kept and means `--on-existing resume`. Passing it together with
  `--on-existing` exits with a usage error.
- `--no-resume` turns `resume_enabled` off for the run (it did nothing before).

## Log lines

One `info` event per existing-file outcome (target `putio_sync_download`), with structured
fields:

| Field | Always | Example |
|---|---|---|
| `file_id` | yes | `1234` |
| `name` | yes | `Movie.mkv` |
| `path` | yes | `/downloads/movies/Movie.mkv` |
| `choice` | yes | `resume` |
| `outcome` | yes | `resumed` |
| `from_bytes` | resumed | `734003200` |
| `saved_as` | saved_as | `Movie (1).mkv` |
| `reason` | saved_as, restarted, overwritten (fixed path) | `larger`, `checksum`, `keep_both`, `fixed_path` |

`refused` logs at `error` with the same fields. No log line is written for a file with nothing
at its path.

## Metric

```text
# TYPE putio_sync_existing_files_total counter
putio_sync_existing_files_total{outcome="skipped|resumed|restarted|overwritten|saved_as|refused"}
```
