# Data Model: Update Checker and Self-Update (055)

## Config: `[updates]` in `config.toml`

| Key | Type | Default | Meaning |
|-----|------|---------|---------|
| `check` | bool | `true` | Check GitHub Releases about a minute after start and every 24 h (FR-001, FR-002) |
| `auto_install` | bool | `false` | Install automatically when idle; only honoured for the GitHub archive channel (FR-020) |

Documented in `config.example.toml`. Hot reload: both keys apply at the next scheduled check,
with no restart needed.

## Table `update_state` (one row, `id = 1`), owned by `putio-sync-db`

| Column | Type | Notes |
|--------|------|-------|
| `last_attempt_at` | TEXT (RFC 3339) | null before the first check |
| `last_success_at` | TEXT | |
| `last_error` | TEXT | short, no URLs with tokens (there are none) |
| `etag` | TEXT | `If-None-Match` for R1 |
| `releases_json` | TEXT | the releases newer than the running version, as fetched (version, date, url, body, asset names) |
| `dismissed_version` | TEXT | FR-007 |
| `last_rolled_back` | TEXT | version that was rolled back, so it is never installed automatically again |
| `rollback_reason` | TEXT | shown once in the UI |

Created with `CREATE TABLE IF NOT EXISTS`, the way `migrate()` does it today.

## `update-pending.json` in the config folder (R8)

Written before the swap and read before the database opens, so it is a file, not a row.

```json
{ "from": "0.3.0", "to": "0.4.0", "old_path": "/opt/putio-sync/putio-sync.old",
  "db_backup": "/config/putio-sync.db.pre-0.4.0", "started_at": "…", "starts": 0 }
```

After a rollback it becomes `{ "rolled_back": "0.4.0", "reason": "…" }`. The old binary moves
that into `update_state` and deletes the file.

## In-memory types (`putio-sync-update`)

- **`Version`**: `(u32, u32, u32)`, parsed from `vX.Y.Z` or `X.Y.Z`, ordered.
- **`ReleaseInfo`**: `version`, `published_at`, `html_url`, `body`, `assets: Vec<String>`.
- **`Channel`**: `Snap | Flatpak | Unraid | CasaOs | Umbrel | Container { image, variant } |
  Homebrew | Deb | Winget | MacPkg | WindowsSetup | GithubArchive | Unknown`, plus
  `fn mode(&self) -> UpdateMode` with `Swap | Installer | Instructions`, and `fn instruction(&self,
  to: Version, arch) -> Instruction { commands: Vec<String>, steps: Option<String>, link: Option<Url> }`.
- **`Target`**: `linux-x86_64 | linux-aarch64 | macos-arm64 | windows-x64 | windows-arm64`,
  from `cfg!(target_os, target_arch)`. Intel macOS has no target (spec Assumptions).
- **`Attempt`**: `from`, `to`, `kind: Patch | Full | Installer`, `state`.

### Attempt states

```text
idle → waiting_for_idle (auto, transfers running) → downloading → verifying →
  ├─ (Swap)      swapping → restarting → [new process] trial → done
  │                                             └→ rolled_back
  ├─ (Installer) opened (the installer takes over) | downloaded (no desktop session: show path)
  └─ failed (any step before swapping; nothing replaced)
```

Only one attempt at a time. A second "Update now" gets `409`.

## Release assets added by 055 (see [contracts/release-assets.md](contracts/release-assets.md))

`putio-sync-<ver>-<target>.xz`, `putio-sync-<from>-to-<ver>-<target>.bsdiff.xz`, the
`.bin` hash lines in `SHA256SUMS`, and `SHA256SUMS.minisig`.
