# Contract: Config, CLI and Log Changes

## TOML config

```toml
[download]
# File extensions (without dot, case-insensitive) that are never downloaded.
# Omit to use the default list; set [] to disable the guard.
blocked_extensions = ["exe", "msi", "bat", "cmd", "com", "scr", "pif", "vbs",
                      "js", "jar", "ps1", "lnk", "apk", "dmg", "pkg"]
```

- The setting is documented in `config.example.toml`.
- Hot-reloadable.
- Exposed read and write through `GET`/`PUT /settings` as `blocked_extensions: string[]`.

## CLI

| Flag | Before | After |
|------|--------|-------|
| `daemon --download-interval <secs>` | Parsed, then ignored with a WARN | Sets the Put.io sync interval. Default 300 s. |

## Log lines (operator-visible)

| Event | Level | Shape |
|-------|-------|-------|
| Network retry | WARN | `Put.io request failed, retrying` with fields `url`, `attempt`, `max_attempts`, `delay_ms`, `error` (full cause chain) |
| Final network failure | ERROR | Same fields as a retry, without `delay_ms` |
| Idle watcher scan | DEBUG | `Scan completed …` |
| Active watcher scan | INFO | `Scan completed …` including `cleaned_up` |
| Cleaned-up leftover watch file | INFO | `Removed already-uploaded watch file` with field `path` |
| Post-upload action failure | WARN, once per path per run | Fields `path`, `error` |
| Invalid watch file | WARN, once per path per run | Fields `path`, `reason` |
| Blocked download | WARN, once per file per run | `Blocked file not downloaded` with fields `file_id`, `name`, `extension` |
| Sync summary | INFO | `Sync complete - Queued: N, Skipped: N, Blocked: N, Failed: N` |
| Startup log level | INFO, once | `Log level: <directive> (from RUST_LOG \| config)` |
| Startup config | INFO | Effective settings, printed after the config merge |

Removed: the pre-config `eprintln!` banner lines (`⚙️ Watch paths: …`, `Delete after: …`).

No log line may contain the OAuth or API token.
