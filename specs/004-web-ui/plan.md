# Implementation Plan: Web UI Served by the API

**Branch**: `004-web-ui` | **Date**: 2026-10-25 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/004-web-ui/spec.md`

## Summary

The `daemon` command gains an optional HTTP listener (default `0.0.0.0:15432`) that
serves the existing JSON API and a new browser UI from the daemon's own engine: the same
download manager, watcher and database at `download_dir/.putiosync.db`. The UI is a
framework-free single-page app (plain HTML, CSS and ES modules) embedded in the binary
with `include_bytes!`, so `make ci` and the Docker build stay `cargo`-only.

Hardening comes with it: the API refuses everything except health, the page itself and
sign-in when no token is configured (open mode removed), failed sign-ins are rate
limited, and the permissive CORS layer is dropped.

Four new capabilities back the UI: a single `GET /status` summary, `POST /local/sync`
("sync now", refused while a sync runs), `POST /local/queue` (queue a Put.io file or
folder through the sync's own filter and dedupe path) and a name filter on
`GET /history`. The sync state and the enqueue logic live in `putio-sync-download`, next
to the sync loop they belong to.

Decisions are in [research.md](research.md) (R1–R12).

## Technical Context

**Language/Version**: Rust 1.96 (`rust-toolchain.toml`), edition 2021 (no let-chains).
Browser side: HTML5, CSS, JavaScript ES2020 modules, no transpilation.

**Primary Dependencies**:
- Existing: tokio, axum 0.7, tower-http 0.5 (`trace` only after this feature; the `cors`
  feature is dropped), reqwest 0.12, sqlx (SQLite), tracing, serde.
- Newly used by `putio-sync-api`: `putio-sync-telemetry` (bottom-layer, for the statvfs
  disk helper, R9).
- New dev-dependency for `putio-sync-api`: `mockito` 1.2 (already in the lock file).
- No new runtime crate. No Node.

**Storage**: none new. Settings stay in the TOML file. The browser keeps the token in
`localStorage`. Sync status is in memory.

**Testing**: `cargo test` with mockito for Put.io, temp SQLite databases and
`tower::ServiceExt::oneshot` for the router (R10). A `mock_ui` example binary for manual
browser checks.

**Target Platform**: Linux container on Unraid (nas01), macOS for development.
Browsers: current Safari, Chrome and Firefox on desktop and phone.

**Project Type**: Cargo workspace, one CLI binary with `daemon` and `server` modes; the
UI is static files inside the `api` crate.

**Performance Goals**:
- Status page interactive within 2 s on the LAN (SC-002); the embedded assets total
  under 150 KB uncompressed.
- `GET /status` answers within 200 ms with a 1,000-row queue (it is one aggregate query
  plus in-memory reads).
- Polling: at most one request per second per open page (FR-011).

**Constraints**:
- No open mode; token required (FR-005, Principle VI).
- No secrets in any response, page or log line (FR-008, SC-004).
- Listener failure must not stop the daemon (FR-004).
- `make ci` and `docker build` need only `cargo` (Principle II, VII).

**Scale/Scope**: one operator, one instance, six pages, three new endpoints, two
changed endpoints, one changed middleware.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Gate | Status |
|---|---|---|
| I. Crate Boundaries | New code goes in the crate that owns it; no upward deps; shared types in `core` | ✅ HTTP and the UI in `api`; sync status and `request_sync` in `download`; history filter in `db`; `[api]` settings in `core`; the daemon listener wiring in `cli` (glue). `daemon` does not depend on `api`; `api` gains a dependency on `telemetry` (bottom layer). No new crate. |
| II. Local-First Verification | `make ci` is the gate | ✅ All tests are `cargo test`. No Node, no browser driver. The UI assets compile into the binary, so a release build proves they embed. |
| III. Tests Without Put.io | mock HTTP only; API and UI exercisable against a mocked backend | ✅ mockito + temp DB for every API test; the `mock_ui` example serves the full UI over a mocked Put.io for manual checks (R10). |
| IV. Download Integrity | no false "complete"; destructive ops guarded | ✅ Cancel reuses the existing path (partial file stays partial, never renamed). Put.io delete from the UI is refused server-side for queued/downloading files (R11) and always confirmed. |
| V. Observable by Default | structured logs, metrics | ✅ New endpoints log with structured fields; the auth limiter logs IP and count. Metrics are unchanged; the UI links to the metrics port. |
| VI. Secrets Stay Secret | tokens never in logs/responses; API requires auth except health/metrics | ✅ Open mode removed (FR-005). `/status` reports `token`/`app_password`/`none`, never values. The sweep test (SC-004) greps every response and the captured logs for the test tokens. Public routes are only `/health`, `/`, `/assets/*`, `/auth/status`, `/auth/login`. |
| VII. Simplicity | smallest change; no unjustified deps or knobs | ✅ Zero new runtime crates (R1). Two new config keys (`api.listen`, `api.bind`), both required by FR-003. The dead legacy `/api/*` placeholders are removed. |

**Post-design re-check**: passes. No Complexity Tracking entries are needed. One known
gap outside this feature's requirements is recorded below for honesty.

## Project Structure

### Documentation (this feature)

```text
specs/004-web-ui/
├── spec.md
├── plan.md              # this file
├── research.md          # R1–R11 decisions
├── data-model.md        # settings, sync status, status summary, session, limiter
├── quickstart.md        # validation guide
├── contracts/
│   ├── api.md           # new and changed HTTP routes, auth behaviour, error shapes
│   ├── config.md        # [api] TOML/env settings, startup lines, errors
│   └── ui.md            # pages, what each shows, which routes it calls, polling
├── checklists/requirements.md
└── tasks.md             # /speckit-tasks
```

### Source Code (repository root)

```text
putio-sync-core/src/config.rs             # ApiConfig + listen/bind; resolve_api(); env overrides; validation

putio-sync-download/src/
├── sync_status.rs                        # NEW: SyncStatus, SyncOutcome, SyncRequestError
├── manager.rs                            # tracks SyncStatus in sync_downloads; Notify-driven run_sync_loop; request_sync(); sync_status(); enqueue_tree(root_id) shared by sync and POST /local/queue
└── lib.rs                                # re-exports

putio-sync-db/src/history.rs              # query()/count() gain name: Option<&str>

putio-sync-telemetry/src/gauges.rs        # pub fn disk_usage(path) -> Option<(free, total)>

putio-sync-api/
├── Cargo.toml                            # + putio-sync-telemetry; tower-http features = ["trace"]; dev: mockito
├── ui/                                   # NEW: static UI source, embedded at build time
│   ├── index.html
│   ├── app.css
│   ├── app.js                            # boot, hash router, auth gate, polling scheduler, disconnected banner
│   ├── api.js                            # fetch wrapper: bearer header, JSON, 401 → sign-in, error toasts
│   ├── format.js                         # bytes, durations, relative times
│   └── pages/
│       ├── login.js
│       ├── status.js
│       ├── queue.js                      # queue + active downloads + sync now
│       ├── putio.js                      # folder browser
│       ├── transfers.js
│       ├── history.js
│       └── settings.js
├── examples/mock_ui.rs                   # NEW: mockito Put.io + seeded temp DB + router on a local port
├── src/
│   ├── lib.rs                            # AppState::new, started_at, limiter, router(state), create_app → router; public vs protected mounting; CORS removed; legacy routes removed
│   ├── error.rs                          # + Conflict (409), ServiceUnavailable (503), TooManyRequests (429 + Retry-After), details helper
│   ├── ui.rs                             # NEW: embedded asset table, GET / and GET /assets/{file}
│   ├── middleware/
│   │   ├── mod.rs                        # + ratelimit
│   │   ├── auth.rs                       # no open mode; failure → limiter; 401 body names the setting
│   │   └── ratelimit.rs                  # NEW: per-IP failed-auth limiter (5 per 60 s → 429)
│   └── routes/
│       ├── system/auth.rs                # NEW: GET /auth/status, POST /auth/login
│       ├── system/status.rs              # NEW: GET /status
│       ├── system/history.rs             # name= filter
│       ├── system/stats.rs               # /stats/disk uses disk_usage()
│       ├── local/sync.rs                 # NEW: POST /local/sync
│       ├── local/queue.rs                # POST / (enqueue a Put.io file or folder)
│       └── putio/files.rs                # DELETE refuses queued/downloading ids (409)
└── tests/
    ├── common/mod.rs                     # NEW: test app builder (mockito client, temp DB, token)
    ├── auth.rs                           # NEW: no token configured, missing, wrong, rate limit, login
    ├── status_sync.rs                    # NEW: /status sections, /local/sync 202 and 409
    ├── queue_actions.rs                  # NEW: each existing queue action (FR-021)
    ├── queue_enqueue.rs                  # NEW: POST /local/queue counts
    ├── putio_delete_guard.rs             # NEW: DELETE /putio/files 409 guard
    ├── history_filter.rs                 # NEW
    ├── settings.rs                       # NEW: GET/PUT round trip per group, 400 on bad input
    ├── ui_assets.rs                      # NEW: / and /assets served, index references resolve, content types
    └── secrets_sweep.rs                  # NEW: SC-004 over every route + captured logs

putio-sync-cli/src/
├── api_listener.rs                       # NEW: start(config, state) → bind, serve with connect info, ERROR once on failure
└── commands/daemon.rs                    # build AppState from PutioDaemon getters, call api_listener::start before daemon.start()
└── commands/server.rs                    # unchanged behaviour; uses create_app; connect-info make_service

config.example.toml                       # [api] section (listen, bind, auth_token, remote enabled/url)
README.md                                 # Web UI section: reach, token, port, plain-HTTP warning, env table
Dockerfile                                # EXPOSE 15432 (documentation only; publishing is the operator's step)
```

**Structure Decision**: the existing workspace layout. The only structural additions are
the `ui/` folder and `examples/` in `putio-sync-api`, and `api_listener.rs` in the CLI
(sibling of the feature-003 `telemetry.rs`). Tests go in each crate's `tests/`
directory, matching features 002 and 003.

## Delivery Order (maps to user stories)

1. **Foundational**: `[api]` settings (core), `SyncStatus` + `request_sync` (download),
   `AppState::new` + `router()` + public/protected split + CORS removal (api), auth
   hardening + limiter (api), daemon listener wiring (cli). After this, the daemon serves
   the existing API on 15432 behind a required token, with no UI yet.
2. **US1 (P1, MVP)**: `/status`, disk helper, embedded asset serving, `index.html`,
   `app.js`, `api.js`, `format.js`, the status page, `mock_ui` example.
3. **US2 (P1)**: `/auth/*`, the sign-in page and sign-out, secrets sweep test. (The
   server-side refusal is already in Foundational; US2 adds the UI and the proof.)
4. **US3 (P2)**: `/local/sync`, the queue page (queue, active downloads, actions, sync
   now).
5. **US4 (P2)**: `enqueue_tree` + `POST /local/queue`, Put.io browser page, transfers
   page, delete guard.
6. **US5 (P3)**: history filter and page.
7. **US6 (P3)**: settings page.
8. **Polish**: README, `config.example.toml`, Dockerfile `EXPOSE`, legacy route removal,
   quickstart run.

## Complexity Tracking

No constitution violations. One known gap, deliberately left:

| Item | Why left | Follow-up |
|---|---|---|
| `POST /local/watcher/start|stop` change the API's copy of `watch.enabled` and save the file, but the daemon loop reads `DaemonConfig.watch_enabled`, set at startup. | FR-010 needs only watcher state, folders and "scan now". Wiring start/stop through to the daemon touches `putio-sync-daemon` and is not required by any FR. | The UI does not offer start/stop. A later feature can make the daemon loop read the shared `Config`. |
