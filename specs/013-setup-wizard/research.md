# Research: First-Run Setup Wizard

All Technical Context items were known; this file records the decisions behind the design.

## R1. How the daemon serves setup without a Put.io client

- **Decision**: a separate `SetupState` + `setup_router()` in `putio-sync-api::setup`, served by
  the CLI on the normal API address while setup is needed. `AppState` is untouched.
- **Rationale**: `AppState` requires a `PutioApiClient` and a `DownloadManager`; making them
  optional would touch every handler. The setup surface is small (≈15 routes) and has its own auth.
- **Alternatives**: `Option<…>` fields in `AppState` (wide blast radius); a second port for setup
  (Docker users would need to publish another port).

## R2. Leaving setup mode in-process

- **Decision**: `execute_daemon` loops: `check` → if setup needed, `setup_mode::run(...)` serves
  the setup router with `with_graceful_shutdown` on a `Notify` fired by `/setup/finish` or by the
  config poller (every 5 s re-runs the check on the file); then reloads the config and runs the
  normal startup. Rebinding the same address retries on `AddrInUse` for up to 5 s.
- **Rationale**: the user's browser stays on the same URL; no container restart (FR-004).
- **Alternatives**: exit and rely on the container restart policy (snap/plain binary would stop).

## R3. Setup gate

- **Decision**: a 6-group code (`XXXX-XXXX-XXXX`, 60 bits, uuid v4 bytes mapped to an unambiguous
  alphabet) generated per setup-mode run, printed with `eprintln!` together with the UI URL.
  `/setup/*` (except `/setup/status`) accept `Authorization: Bearer <code>` or the configured API
  token; comparison is constant time (reuse `token_matches`). Failures count in the same
  `AuthLimiter` (5/min/IP).
- **Rationale**: user decision; console works for Docker (`docker logs`), snap (`snap logs`)
  and terminals. Printing outside `tracing` keeps it out of log files and the telemetry archive.
- **Alternatives**: code file in the config dir (needs a shell into the container).

## R4. Put.io device link

- **Decision**: `GET /v2/oauth2/oob/code?app_id=<id>` → `{code, qr_code_url}`; then
  `GET /v2/oauth2/oob/code/<code>` every 3 s → `{oauth_token: null|string}`. Codes are treated as
  expired after 10 minutes (the UI offers a new one). Implemented in `putio-sync-client::oob`
  with a base URL parameter so tests use wiremock.
- **App ID**: `[putio] app_id` > `PUTIO_SYNC_APP_ID` (runtime env) > built-in
  `option_env!("PUTIO_SYNC_APP_ID")` at build time. The built-in is unset in this repo until the
  user supplies one; with no ID the wizard shows the token-paste path only.
- **Alternatives**: redirect OAuth (needs a client secret and a reachable redirect URI; not
  possible from a NAS on a LAN address).

## R5. Rejected token detection

- **Decision**: add `ClientError::is_unauthorized()` matching the existing 401 `ApiError`
  message (constant `UNAUTHORIZED_MESSAGE`), like `is_not_found()`. At startup the daemon calls
  `get_account_info()` once: a 401 means setup (reason `credentials_rejected`); a network error
  does **not** enter setup (the daemon starts and retries as today), per FR-001 edge case.

## R6. Folder checks

- **Decision**: `core::setup::check_folder(path)`: exists or can be created (`create_dir_all`
  only when the user asks: `create: true`), is a directory, writable (create+remove a probe file
  `.putio-sync-write-test-<pid>`), free/total bytes via `putio_sync_telemetry::gauges::disk_usage`.
  Warnings: free < Put.io used, free < 10 GiB. Container detection: `/.dockerenv` or
  `/run/.containerenv` exists, or `container` env var set.
- **Suggestion order**: configured target (when not the default `./putio-downloads`) →
  `/downloads` if it exists → `~/putio-downloads`.

## R7. Account-wide preset

- **Decision**: new `[download] preset = "<name>"` (optional). `RuleSet::resolve` starts from
  `TypeFilter::from_preset(preset)` with `Sourced::global(Some(name))` when set, otherwise from
  `blocked_extensions` exactly as today. Validation: name must be a built-in or `[download.presets]`
  key. The wizard offers the built-in presets with "No binaries" preselected.
- **Rationale**: per-folder rules already exist (009); this is the only missing piece for "what
  to download" at the account level.

## R8. Dry run

- **Decision**: `putio_sync_download::dry_run(client, &DownloadConfig, target_dir, deadline)`:
  walks the tree with the existing listing helpers (read-only), applies `config.effective(path)
  .decide(name)`, checks whether the local file exists at the destination with the same size, and
  totals: would download (count, bytes), already present, skipped by rules. Samples: 20 largest
  to-download files. Deadline 60 s → `partial: true` with counts so far.
- **Alternatives**: running the real sync with a "no-op" downloader (risk of side effects).

## R9. CLI `putio-sync setup`

- **Decision**: plain stdin/stdout prompts (no new crate). Flags as in contracts/cli.md; the
  Put.io token from `PUTIO_SYNC_PUTIO_TOKEN`, a device link or paste prompt (terminal), or the
  first stdin line (no terminal). Uses the same `core::setup` engine and the same
  dry run. Writes once with `Config::save`. Skips *arr (stated at the end).

## R10. Dependencies

- **Decision**: none added. The setup code and the API token are generated in the api crate with
  `uuid` v4 (already a dependency); `putio_sync_api::setup::{new_setup_code, new_api_token}` are
  public so the CLI (which depends on api) uses the same functions. Core stays free of randomness.

## R11. UI

- **Decision**: `pages/setup.js` (one module, steps as functions); `app.js` boot asks
  `/auth/status` and mounts the setup page when `setup_required`. The setup code lives in
  `sessionStorage` (`putio-sync.setup-code`). On finish the returned API token goes to the normal
  `localStorage` key and the router navigates to `#/status` after the daemon is back (poll
  `/auth/status` until `setup_required: false`).
- **Size**: bundle cap raised to 300 KB (user decision); the final size is reported.

## R12. Metric

- **Decision**: `putio_sync_setup_required` gauge, 1 while in setup mode, 0 after. Telemetry only
  starts during setup mode when the existing config enables metrics; the normal start path
  skips `telemetry::start` if it already ran.
