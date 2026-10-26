# Research: Web UI Served by the API

Decisions for feature 004. Each one lists what was chosen, why, and what else was
considered.

## R1. Frontend approach

- **Decision**: a hand-written single-page UI with no framework and no build step: one
  `index.html`, one stylesheet and a handful of ES-module JavaScript files under
  `putio-sync-api/ui/`. They are embedded in the binary with `include_bytes!` and served
  by a small `ui` module in `putio-sync-api`. Routing inside the page is hash-based
  (`/#/queue`), so the server serves exactly one HTML document. Live data comes from
  `fetch` polling (R6).
- **Rationale**:
  - `make ci` and the Docker build stay `cargo` only. No Node, npm or bundler is needed,
    on the developer machine or in the `rust:alpine` builder stage (Principle II, VII).
  - The UI is six pages of tables and forms over existing JSON. That is around 1,500
    lines of plain JavaScript; a framework would be larger than the application.
  - `include_bytes!` is in the standard library. No new crate for asset embedding.
  - Everything is served from the LAN, so there is no CDN and the page works offline.
  - Plain CSS with a single-column layout under 700 px handles the phone requirement
    (FR-019).
- **Alternatives considered**:
  - **Vite + React/Svelte/Solid**: good tooling, but it puts Node in the CI gate and in
    the Docker build, or forces committing a built bundle. Rejected on the stated
    priorities.
  - **`rust-embed`**: convenient folder embedding with debug-time reload, but it is a new
    dependency plus a proc macro for what six `include_bytes!` lines do.
  - **htmx or a vendored micro-framework (Preact + htm)**: a vendored 15–30 KB file is
    acceptable in principle, but htmx pushes rendering into the server (templates in
    Rust, another dependency) and Preact adds a second idiom for little gain at this size.
  - **Server-side templates (askama/minijinja)**: no scripting on the client, but live
    refresh every few seconds then means full-page reloads, and a template crate is a new
    dependency.
  - **Leptos/Yew (Rust → WASM)**: keeps one language, but adds a `wasm32` target, a
    bundler (`trunk`) and multi-megabyte downloads. Far heavier than the need.

## R2. Where the UI lives and how it is served

- **Decision**: `putio-sync-api/ui/` holds the source files; `putio-sync-api/src/ui.rs`
  holds a static table of `(path, content type, bytes)` and two routes:
  - `GET /` → `index.html`;
  - `GET /assets/{file}` → the matching entry, or 404.

  Responses carry `Cache-Control: no-cache` and an `ETag` equal to the crate version, so
  browsers revalidate cheaply and a new release is never served stale. Both routes are
  mounted outside the auth layer: the page itself is public (it only contains the sign-in
  form and rendering code), and every piece of data behind it is protected.
- **Rationale**: the UI must be served "by the same process and port as the JSON API"
  (FR-001). The `api` crate owns HTTP (Principle I). Keeping the assets next to the routes
  that feed them means a route change and its UI change land in one crate.
- **Alternatives considered**: a separate `putio-sync-ui` crate (nothing would depend on
  it except `api`; an extra crate for a folder of static files is not justified); serving
  from a directory on disk (breaks FR-001's "no files to deploy").

## R3. Sign-in, session and auth hardening

- **Decision**:
  - The session is the configured `api.auth_token`, held in `localStorage` by the page and
    sent as `Authorization: Bearer <token>` on every data request. There is no server-side
    session store. Sign-out clears the stored value.
  - `POST /auth/login` with `{ "token": "..." }` validates the token with the existing
    constant-time compare and returns 204 or 401. The UI calls it before storing the token
    so a typo is caught immediately. `GET /auth/status` (public) returns
    `{ "token_configured": bool }` so the sign-in page can say "set `api.auth_token`" when
    nothing is configured.
  - **Open mode is removed**: when `api.auth_token` is unset, `require_auth` returns 401
    with a body naming the setting, and the process logs one WARN at startup
    (`API token not configured (set [api] auth_token or PUTIO_SYNC_API_TOKEN); all API and UI requests will be refused`).
  - **Rate limit**: a small in-memory limiter keyed by client IP counts failed
    authentications (a wrong token on a protected route, or a failed `/auth/login`). A
    request with no token gets 401 without being counted, so a browser fetching
    `/robots.txt` or a touch icon cannot lock its operator out. After 5 failures within 60 s, further requests from that IP get 429
    with `Retry-After` until the window ends. The log line records the IP and the count,
    never the attempted value. Client IP comes from `ConnectInfo<SocketAddr>` (the servers
    use `into_make_service_with_connect_info`); when it's absent (router tests using
    `oneshot`) the limiter keys on `127.0.0.1`.
  - **CORS**: `CorsLayer::permissive()` is removed and nothing replaces it. Without CORS
    headers browsers refuse cross-origin reads, which is FR-009. Same-origin UI requests
    and non-browser clients (curl, the CLI remote mode) are unaffected.
- **Rationale**: it is the smallest design that meets FR-005–FR-009 and Principle VI.
  The token already exists; a cookie session would need a session store, CSRF defence and
  a second secret for the same single operator. `localStorage` keeps the operator signed
  in across reloads (US2 scenario 3); it's readable by scripts on the same origin only,
  which is the UI itself.
- **Alternatives considered**: HttpOnly cookie sessions (more code, CSRF handling, no
  benefit for one operator on a LAN); HTTP Basic auth (browser-cached credentials, no
  sign-out); keeping open mode for loopback binds (two code paths and one more thing to
  get wrong; the NAS binds to all interfaces anyway).

## R4. Serving the UI from the daemon (clarification Q1, option A)

- **Decision**:
  - `putio-sync-api` exposes `AppState::new(config, config_path, client, database,
    download_manager, file_watcher)` and `pub fn router(state: AppState) -> Router`.
    `create_app` (used by `server`) keeps building its own engine and then calls
    `router`, so `server` is unchanged in behaviour apart from the auth/CORS hardening.
  - In `putio-sync-cli/src/commands/daemon.rs`, after `daemon.initialize()` and before
    `daemon.start()`, the CLI builds an `AppState` from `daemon.get_client()`,
    `get_database()`, `get_download_manager()` and `get_file_watcher()` plus the loaded
    `Config` and its path, resolves `[api]` (R8), binds the listener and spawns
    `axum::serve` as a task. A bind failure is logged once at ERROR and the daemon carries
    on (FR-004), mirroring `cli/src/telemetry.rs`.
  - `PutioDaemon` already keeps `Arc`s of every engine piece and exposes getters, so the
    daemon crate needs no change for this. The `daemon` crate still does not depend on
    `api`; the CLI is the glue, which is what Principle I allows glue to do.
- **Rationale**: one engine, one database (`download_dir/.putiosync.db`), one watcher.
  The queue the UI reorders is the queue the workers read. The settings routes'
  `download_manager.update_config` and `file_watcher.update_config` now act on the live
  daemon.
- **Known gap, out of scope**: the API's `POST /local/watcher/start|stop` flips
  `config.watch.enabled` in the API's `Config` and saves the file, but the daemon loop
  reads `DaemonConfig.watch_enabled`, which is set once at startup. The UI therefore shows
  the watcher's state and offers "scan now", not start/stop. Wiring those together is
  follow-up work; it's recorded in the plan's Complexity Tracking as a non-issue for this
  feature's requirements (FR-010 asks only for state and folders).
- **Alternatives considered**: options B and C from clarify (rejected by the user); having
  `putio-sync-daemon` build the router itself (would make `daemon` depend on `api`, an
  upward dependency).

## R5. Status summary and "sync now"

- **Decision**:
  - `putio-sync-download` gains a `SyncStatus` value tracked by `DownloadManager`:
    `in_progress`, `last_started_at`, `last_finished_at`, `last_duration`, `last_outcome`
    (`Success` summary or a token-free `Failure { error }`), `next_due_at`, and
    `interval`. `sync_downloads` updates it on entry and exit. It lives in a new
    `putio-sync-download/src/sync_status.rs` and is read through
    `DownloadManager::sync_status()`.
  - `run_sync_loop` waits on `tokio::select!` between the interval sleep and a
    `tokio::sync::Notify`. `DownloadManager::request_sync()` returns
    `Err(SyncRequestError::InProgress)` when a sync is running, otherwise notifies the loop
    and returns `Ok(())`. If no loop is running (the `server` dev mode), the request runs
    `sync_downloads` directly on a spawned task, so `POST /local/sync` behaves the same in
    both modes.
  - `GET /status` in `putio-sync-api/src/routes/system/status.rs` composes: version,
    uptime from `AppState.started_at`, `sync_status()`, `db.queue().counts_by_status()`,
    `get_active_downloads()`, `active_workers()` and the configured count, the watcher's
    `get_stats()` plus configured paths, disk free/total of the download folder, database
    reachability (the counts query succeeded) and Put.io reachability (the last sync
    result was `Ok`, `unknown` before the first sync). Each section is independently
    `null`-able with a reason, so a missing database doesn't blank the page (US1 scenario
    3).
  - `POST /local/sync` returns 202 with the new status, or 409 when a sync is in
    progress.
- **Rationale**: sync state belongs with the sync loop (Principle I: `download` owns
  it). `Notify` wakes the existing loop without a second scheduler. Deriving Put.io
  reachability from the last sync avoids one Put.io request per 3 s poll.
- **Alternatives considered**: a dedicated "sync now" task that calls `sync_downloads`
  concurrently with the loop (two syncs could overlap, which the spec forbids); pinging
  Put.io on each status request (needless API traffic and metric noise).

## R6. Live updates

- **Decision**: polling with `fetch`. The status page polls `/status` every 3 s; the queue
  page polls `/local/queue` and `/local/downloads` every 3 s; transfers every 5 s; other
  pages on demand. Polling pauses while the tab is hidden (`visibilitychange`) and backs
  off to 10 s after a network error, showing a "disconnected" banner until a request
  succeeds (edge case: API unreachable). After an action the page refreshes at once.
- **Rationale**: within FR-011 (≤5 s, ≤1 request/s/page) with no server-side push code.
  The daemon is one process on a LAN with one operator; the cost of a request every 3 s
  is negligible.
- **Alternatives considered**: Server-Sent Events (needs a broadcast of every state change
  from the engine, plus reconnect logic in the page); WebSockets (same, plus a protocol).
  Either can replace polling later without changing the page structure.

## R7. History name filter

- **Decision**: `HistoryRepository::query` and `count` take `name: Option<&str>`; when
  set, add `AND filename LIKE ? ESCAPE '\'` with `%`, `_` and `\` escaped in the input.
  `GET /history` gains `name=`. Matching is case-insensitive (SQLite `LIKE` default for
  ASCII).
- **Rationale**: the smallest change for FR-017; the table is small (thousands of rows).
- **Alternatives considered**: client-side filtering (would need the whole table every
  time); FTS (overkill).

## R8. `[api]` settings and environment overrides

- **Decision**: the existing `ApiConfig` (`enabled`, `url`, `auth_token`) keeps its
  fields and their meaning (`enabled` + `url` = the daemon delegates downloads to a remote
  API, an existing but unrelated mode). Two fields are added:

  | Setting | Type | Default | Env override |
  |---|---|---|---|
  | `api.listen` | bool | `true` | `PUTIO_SYNC_API_LISTEN` (`true`/`false`/`1`/`0`) |
  | `api.bind` | `host:port` | `0.0.0.0:15432` | `PUTIO_SYNC_API_BIND` |
  | `api.auth_token` | string | unset | `PUTIO_SYNC_API_TOKEN` |

  `Config::resolve_api()` mirrors `resolve_metrics()`: env wins, the source is logged, and
  an invalid value fails startup with the same `invalid <setting> '<value>': expected …`
  message. The `server` command's `--bind`/`--port` flags still win over `[api]` in that
  mode. The `daemon` command reads `[api]` only.
- **Rationale**: it is the same shape as `[metrics]` from feature 003, so operators learn
  one pattern. `listen` rather than reusing `enabled` avoids changing the meaning of an
  existing setting.
- **Alternatives considered**: renaming `enabled` to `remote` and reusing `enabled` for
  the listener (a silent behaviour change for any config that has it).

## R9. Disk space in the status summary

- **Decision**: `GET /status` and the existing `GET /stats/disk` use `statvfs` on the
  download folder through the helper that feature 003 added for the disk gauges
  (`putio_sync_telemetry::gauges`), made `pub` and given a plain
  `disk_usage(path) -> Option<(free, total)>` signature. `putio-sync-api` adds
  `putio-sync-telemetry` as a dependency (it's a bottom-layer crate, so no upward edge).
- **Rationale**: `sys_info::disk_info()` reports the root filesystem, which inside the
  container is the image layer, not the bind-mounted download share. The status page
  would say the disk is nearly full when the share has terabytes free. Reusing the
  telemetry helper means the UI and the metrics agree.
- **Alternatives considered**: keep `sys-info` (wrong numbers in Docker); add `rustix`
  to `api` directly (duplicates the helper).

## R10. Testing strategy (Principle III)

- **Decision**:
  - **API integration tests** in `putio-sync-api/tests/` build the router through
    `AppState::new` with a `PutioApiClient` pointed at a `mockito` server, a temp SQLite
    database and a real `DownloadManager`, then drive it with `tower::ServiceExt::oneshot`
    (the `server_bind.rs` pattern). They cover auth (no token configured, missing, wrong,
    rate limit, correct), `/status`, `/local/sync`, history filter, asset serving and the
    no-secret-in-any-response sweep (SC-004).
  - **Crate tests**: `putio-sync-download/tests/sync_status.rs` (status transitions,
    `request_sync` while idle and while busy, loop wake-up), `putio-sync-db` history
    filter, `putio-sync-core` `[api]` resolution.
  - **UI**: the JavaScript stays thin (render functions over API JSON) and is checked
    two ways: an automated test that every `src`/`href` in `index.html` resolves to an
    embedded asset and that each asset is served with the right content type; and a
    manual click-through in the quickstart against `cargo run -p putio-sync-api --example
    mock_ui`, an example binary that starts a mockito Put.io with fixtures, a temp
    database with seeded queue and history, and the full router on a local port. That
    example is how the UI is "exercisable against a mocked backend" without a browser
    automation stack.
  - `mockito` is added as a dev-dependency of `putio-sync-api` (already used by `client`
    and `download`).
- **Rationale**: browser automation (Playwright, WebDriver) would bring Node or a driver
  binary into `make ci`, which R1 rules out. Keeping logic in the API and testing it there
  gives the constitution's mock-only guarantee for everything the UI does; the remaining
  risk is rendering, which the mock example makes a two-minute manual check.
- **Alternatives considered**: headless Chrome via `chromiumoxide`/`fantoccini` (a browser
  binary in CI); JavaScript unit tests with Node (rejected with R1).

## R11. Destructive-action guards and confirmations

- **Decision**: confirmations are rendered by the page (a modal naming the target) for:
  Put.io delete, transfer remove, cancel download, clear completed. The Put.io file list
  marks entries whose `file_id` is in the queue with state pending or downloading and
  hides the delete action for them; the server also refuses `DELETE /putio/files/{id}`
  with 409 when the queue has that id pending or downloading, so a stale page can't
  delete it either.
- **Rationale**: FR-020 and Principle IV. The server-side check keeps the guarantee even if
  two tabs disagree.

## R12. Queueing a Put.io file or folder from the UI (FR-014)

- **Decision**: `POST /local/downloads` stays as it is (it downloads one file at once,
  outside the queue, and does not recurse). The UI's "download" action is a new
  `POST /local/queue { file_id }` that calls a new
  `DownloadManager::enqueue_tree(root_id) -> SyncStats`. `enqueue_tree` is the body of
  today's `sync_downloads_inner` with the root id as a parameter: it lists the tree with
  `list_all_files(root_id)`, applies the blocked-extension and pattern filters, skips
  files already downloaded or queued, and adds the rest with `queue().add`. A single file
  id goes through the same path (a one-element tree). `sync_downloads` becomes
  `enqueue_tree(0)` plus the telemetry wrapper, so the two can't drift.
- **Rationale**: FR-014 asks for recursion and the same filters as the sync; reusing the
  sync's code is the only way to guarantee "the same". Principle I puts the logic in
  `download`, where the sync already is; the API route is glue.
- **Alternatives considered**: recursing in the API handler with the client's
  `list_files` (duplicates the filter and dedupe logic in the wrong crate); looping
  `POST /local/downloads` from the page (bypasses the queue, no filters, N requests).
