---

description: "Task list for feature 004: Web UI Served by the API"
---

# Tasks: Web UI Served by the API

**Input**: Design documents from `specs/004-web-ui/`

**Prerequisites**:
- [plan.md](plan.md)
- [spec.md](spec.md)
- [research.md](research.md) (R1–R12)
- [data-model.md](data-model.md)
- contracts: [api.md](contracts/api.md), [config.md](contracts/config.md), [ui.md](contracts/ui.md)
- [quickstart.md](quickstart.md)

**Tests**: included. FR-021 and Constitution III require every API capability to be
exercised against a mocked Put.io and a temporary database; the constitution requires
`make ci` green at every step (Principle II). Within each story, write the tests first and
confirm they fail before implementing.

**Organization**: tasks are grouped by user story. The phases follow spec priority: US1
(P1, MVP), US2 (P1), US3 (P2), US4 (P2), US5 (P3), US6 (P3).

## Format: `[ID] [P?] [Story] Description`

- **[P]**: can run in parallel (different files, and no dependency on an unfinished task)
- **[Story]**: US1–US6 from spec.md
- Paths are relative to the repo root.

## Ground rules for every task

- Rust edition 2021, so no let-chains. `make ci` = fmt, clippy `-D warnings`, test and a
  release build. Run `cargo fmt` and `cargo clippy -p <crate> -- -D warnings` before
  calling a task done.
- Route shapes, status codes and bodies come from [contracts/api.md](contracts/api.md);
  settings and log lines from [contracts/config.md](contracts/config.md); page behaviour
  from [contracts/ui.md](contracts/ui.md). Don't invent fields.
- Never log, return or embed a token value. Log `token=set`/`token=unset` only
  (Principle VI). Tests that need a token use the literal `test-api-token` and a Put.io
  token `test-putio-token`, so the secrets sweep (T031) can grep for them.
- Tests never reach the network: Put.io is a `mockito::Server`, the database is a
  `tempfile::TempDir` SQLite file. Build the router through the helper from T011.
- The UI has no build step. Files under `putio-sync-api/ui/` are served byte-for-byte.
  Plain ES modules (`<script type="module">`), no bundler, no external URLs, no inline
  scripts in `index.html`.
- Keep the UI small: total of `ui/` under 150 KB. No icon fonts or images.
- Commit nothing; the coordinator handles commits.

---

## Phase 1: Setup

**Purpose**: dependencies and the test scaffold every later task uses.

- [X] T001 Update `putio-sync-api/Cargo.toml`:
  - Add `putio-sync-telemetry = { path = "../putio-sync-telemetry" }` under
    `[dependencies]` (for the disk helper, R9).
  - Change `tower-http = { version = "0.5", features = ["cors", "trace"] }` to
    `features = ["trace"]` (R3: CORS layer removed).
  - Add `mockito = "1.2"` under `[dev-dependencies]`. `tower` (with `util`, for
    `oneshot`) and `tracing-subscriber` (T031 captures logs) are already regular
    dependencies and so available to tests and examples.
  - Run `cargo build -p putio-sync-api` to confirm it resolves (it will fail to compile
    until T008 removes the `CorsLayer` import; that is expected, so do T008 right after).
- [X] T002 [P] Add error variants in `putio-sync-api/src/error.rs`:
  - `Conflict(String)` → 409; `ServiceUnavailable(String)` → 503;
    `TooManyRequests { retry_after_seconds: u64 }` → 429 with a `Retry-After` header
    whose value is the seconds, body `{"error":"Too many failed authentication attempts"}`.
  - `Unauthorized` must set `WWW-Authenticate: Bearer` on its response.
  - Add a `details: Option<String>` path: `Conflict` and `Unauthorized` messages that
    contain `": "` are not split; instead add `ApiError::with_details(self, details)`
    that fills `ErrorResponse.details`. Keep the `Display` impl exhaustive.
  - Unit-test each new variant's status and headers in the file's `mod tests`.

---

## Phase 2: Foundational (blocks every story)

**Purpose**: settings, sync status, the router split, auth hardening and the daemon
listener. After this phase the daemon serves the existing JSON API on port 15432 behind a
required token. No UI yet.

### `[api]` settings (core)

- [X] T003 Write failing tests in `putio-sync-core/src/config.rs` (`mod tests`, next to
  the `resolve_metrics` tests, reusing their `with_env` helper):
  - `api_defaults`: `Config::default().resolve_api()` gives `listen == true`,
    `bind == "0.0.0.0:15432"`, `token == None`, `source == LevelSource::Config`.
  - `api_env_overrides`: with `PUTIO_SYNC_API_LISTEN=0`, `PUTIO_SYNC_API_BIND=127.0.0.1:1`
    and `PUTIO_SYNC_API_TOKEN=abc` set, the resolved values follow the env and
    `source == LevelSource::Env`.
  - `api_invalid_values`: `listen = "maybe"` in TOML, `PUTIO_SYNC_API_BIND=nope`, and
    `auth_token = "has space"` each fail with `invalid api.<setting> '<value>': expected
    ...` (for the token: `invalid api.auth_token: expected a non-empty value without
    whitespace`, no value echoed).
  - `api_token_never_in_debug`: `format!("{:?}", settings)` with a token set does not
    contain the token text.
- [X] T004 Implement `[api]` settings in `putio-sync-core/src/config.rs` (R8,
  [contracts/config.md](contracts/config.md)):
  - Add `listen: Option<bool>` (TOML `listen`) and `bind: Option<String>` to the
    existing `ApiConfig` with `#[serde(default)]`; keep `enabled`, `url`, `auth_token`.
    Accept `listen` as bool or the strings `true/false/1/0` the same way `metrics.enabled`
    is parsed.
  - Add `pub struct ApiSettings { pub listen: bool, pub bind: SocketAddr, pub token:
    Option<String> }` with a manual `Debug` that prints `token: <set>`/`<unset>`.
  - Add `pub fn resolve_api(&self) -> CoreResult<Resolved<ApiSettings>>` modelled on
    `resolve_metrics`: env wins (`PUTIO_SYNC_API_LISTEN`, `PUTIO_SYNC_API_BIND`,
    `PUTIO_SYNC_API_TOKEN`), validation errors use the `invalid <setting> '<value>':
    expected ...` wording, token errors never echo the value.
  - Make T003 pass.

### Sync status and enqueue (download)

- [X] T005 Create `putio-sync-download/src/sync_status.rs` and export it from
  `putio-sync-download/src/lib.rs` (`pub mod sync_status; pub use sync_status::{SyncStatus,
  SyncOutcome, SyncRequestError};`):
  - `#[derive(Clone, Debug, Default)] pub struct SyncStatus { pub in_progress: bool, pub
    last_started_at: Option<SystemTime>, pub last_finished_at: Option<SystemTime>, pub
    last_duration: Option<Duration>, pub last_outcome: Option<SyncOutcome>, pub
    next_due_at: Option<SystemTime>, pub interval: Option<Duration> }`.
  - `pub enum SyncOutcome { Success { found: usize, queued: usize, skipped: usize,
    blocked: usize, failed: usize }, Failure { error: String } }` with
    `impl From<&SyncStats> for SyncOutcome`.
  - `#[derive(Debug, thiserror::Error)] pub enum SyncRequestError { #[error("a sync is
    already in progress")] InProgress }`.
  - Methods on `SyncStatus`: `begin(&mut self)`, `finish(&mut self, outcome: SyncOutcome)`
    (sets finished, duration, in_progress = false, and `next_due_at = now + interval` when
    `interval` is set), `schedule(&mut self, interval: Duration)`.
  - Unit tests in the file for the three transitions from
    [data-model.md](data-model.md#transitions).
- [X] T006 Track status and add "sync now" in `putio-sync-download/src/manager.rs`
  (R5):
  - Fields on `DownloadManager`: `sync_status: Arc<RwLock<SyncStatus>>`,
    `sync_notify: Arc<tokio::sync::Notify>`. Initialise in `new` and
    `new_without_database`.
  - `sync_downloads`: call `begin()` before `sync_downloads_inner`, `finish(outcome)`
    after, where a failure's `error` is `error_chain(&e)` (already token-free). Keep the
    telemetry wrapper as is.
  - `run_sync_loop(interval_secs)`: call `schedule(interval)` once, then each cycle wait
    with `tokio::select! { _ = tokio::time::sleep(interval) => {}, _ =
    self.sync_notify.notified() => {} }` after the sync. A notify during a sync is
    consumed by `notified()` and ignored because `request_sync` already refused it.
  - `pub async fn request_sync(&self) -> Result<(), SyncRequestError>`: `InProgress` if
    `in_progress`; else if `interval.is_some()` (a loop runs) `notify_one()`; else spawn
    `sync_downloads` on a clone (`DownloadManager` must be `Clone` or wrapped; use the
    existing `Arc` fields so a cheap clone works, or take `self: &Arc<Self>`).
  - `pub async fn sync_status(&self) -> SyncStatus`.
  - Tests in the file's `mod tests` or `putio-sync-download/tests/sync_status.rs`:
    with a mockito Put.io listing zero files and a temp DB, `request_sync` → `Ok`, then
    `sync_status()` eventually shows `last_outcome == Some(Success{..})`; a second
    `request_sync` while a sync is blocked (mockito `with_delay`) returns `InProgress`.
- [X] T007 Extract `enqueue_tree` in `putio-sync-download/src/manager.rs` (R12, after
  T006, same file):
  - `pub async fn enqueue_tree(&self, root_id: i64) -> DownloadResult<SyncStats>` = the
    body of today's `sync_downloads_inner` with `list_all_files(root_id)`. For a root
    that is a file (not a folder), `list_all_files` returns it as the single entry;
    confirm and handle if it does not (fetch `get_file_details(root_id)` first and build
    a one-element list).
  - `sync_downloads_inner` becomes `self.enqueue_tree(0).await`.
  - Test: seed mockito with a folder containing `a.mkv`, `b.exe` (blocked by config) and
    `c.mkv` already in the queue; `enqueue_tree(folder)` returns `total_files 3, queued
    1, blocked 1, skipped 1` and the queue holds `a.mkv` and `c.mkv` only.

### Router, state and auth (api)

- [X] T008 Restructure `putio-sync-api/src/lib.rs` (R4, [contracts/api.md](contracts/api.md#route-classes)):
  - Add `started_at: std::time::Instant` to `AppState`.
  - `pub fn AppState::new(config: Config, config_path: Option<PathBuf>, client:
    Arc<PutioApiClient>, database: Option<Arc<Database>>, download_manager:
    Arc<DownloadManager>, file_watcher: Option<Arc<TorrentWatcher>>) -> AppState`.
  - `pub fn router(state: AppState) -> Router`: protected routes nested as today, then
    `.layer(from_fn_with_state(state.clone(), middleware::auth::require_auth))`, then
    `.with_state(state.clone())`, then `.merge(public_routes(state))` where
    `public_routes` holds `/health` (and later `/`, `/assets/*`, `/auth/*`), then
    `.layer(TraceLayer::new_for_http())`. Remove `CorsLayer` and its import. Remove the
    three `/api/*` legacy nests and the `create_legacy_*` functions (delete
    `routes/legacy*` files if they exist and nothing else uses them).
  - `create_app(...)` keeps its signature and body up to the state, then returns
    `Ok(router(AppState::new(...)))`.
  - Add `limiter: Arc<middleware::ratelimit::AuthLimiter>` to `AppState`, created in
    `new()` with the limits from T009.
  - `cargo build -p putio-sync-api` and `cargo test -p putio-sync-api` must pass (the
    existing `tests/server_bind.rs` still uses `create_app`).
- [X] T009 [P] Create `putio-sync-api/src/middleware/ratelimit.rs` and add `pub mod
  ratelimit;` to `putio-sync-api/src/middleware/mod.rs`
  ([data-model.md](data-model.md#failed-auth-limiter-in-memory-api)):
  - `pub struct AuthLimiter { limit: u32, window: Duration, inner: Mutex<HashMap<IpAddr,
    (Instant, u32)>> }`, `AuthLimiter::new(limit, window)`.
  - `pub fn check(&self, ip: IpAddr) -> Result<(), u64>`: Err(seconds left) when the
    window is live and failures ≥ limit. `pub fn record_failure(&self, ip: IpAddr) ->
    u32` returns the new count, resets the window when expired, prunes expired entries.
  - `pub fn client_ip(request: &Request) -> IpAddr`: `ConnectInfo<SocketAddr>` from
    extensions, else `127.0.0.1`.
  - Unit tests: 5 failures allowed, 6th refused with `Retry-After` 1..=60; after the
    window a new failure is allowed; two IPs are independent.
- [X] T010 Harden `putio-sync-api/src/middleware/auth.rs` (R3, after T008 and T009):
  - Remove the "no token configured → allow" branch. When `config.api.auth_token` is
    `None`: `record_failure` and return `ApiError::Unauthorized("API token not
    configured".into()).with_details("Set [api] auth_token in the config file or
    PUTIO_SYNC_API_TOKEN")`.
  - Before comparing: `state.limiter.check(ip)` → on `Err(secs)` return
    `ApiError::TooManyRequests { retry_after_seconds: secs }` and log
    `warn!(ip = %ip, retry_after_seconds = secs, "Authentication rate limited")`.
  - On mismatch or missing header: `let n = record_failure(ip)`; `warn!(ip = %ip,
    failures = n, "Authentication failed")`; return `Unauthorized("Unauthorized")`.
  - Keep `validate_token` constant-time. Export `pub fn token_matches(provided: &str,
    expected: &str) -> bool` for `/auth/login` (T030).
- [X] T011 Create `putio-sync-api/tests/common/mod.rs` (R10):
  - `pub struct TestApp { pub router: Router, pub state: AppState, pub putio:
    mockito::ServerGuard, pub db: Arc<Database>, pub _dir: tempfile::TempDir, pub token:
    String }`.
  - `pub async fn test_app() -> TestApp`: `mockito::Server::new_async()`, `Config`
    with `api.auth_token = Some("test-api-token")`, `putio.oauth_token =
    Some("test-putio-token")` (or whichever field holds it), `download.target_directory`
    = the temp dir, `PutioApiClient::with_base_url("test-putio-token", &server.url())`,
    `Database::new(temp .putiosync.db)`, `DownloadManager::new(client, db, config)`,
    `AppState::new(...)`, `router(state)`.
  - `pub async fn test_app_without_token()`, `pub async fn test_app_without_db()`.
  - Helpers: `get(&self, path) -> Response`, `post_json(&self, path, body)`,
    `req(method, path, Option<token>, Option<Json>)` using `tower::ServiceExt::oneshot`;
    `body_json(response) -> serde_json::Value`; `seed_queue(&self, rows)` via
    `db.queue().add(file_id, name, size, url, rel_path, priority)`.
  - Mockito helpers: `mock_files_list(&mut self, parent_id, json)`,
    `mock_file(&mut self, id, json)` for `/v2/files/list` and `/v2/files/{id}` (check
    the paths the client really calls in `putio-sync-client/src/files.rs`).
- [X] T012 Write failing tests in `putio-sync-api/tests/auth.rs` (uses T011):
  - `health_is_public`; `no_token_configured_refuses_with_setting_name` (401, body
    `error == "API token not configured"`, `details` names `PUTIO_SYNC_API_TOKEN`);
    `missing_token_401_with_www_authenticate`; `wrong_token_401`;
    `sixth_failure_429_with_retry_after`; `legacy_api_routes_gone` (`/api/queue` → 404 or
    401 but not 200); `no_cors_headers` (a request with `Origin: http://evil` gets no
    `access-control-allow-origin`).
  - Make them pass (T008–T010 exist; fix what the tests reveal).

### Listener wiring (cli)

- [X] T013 Create `putio-sync-cli/src/api_listener.rs`, add `pub mod api_listener;` to
  `putio-sync-cli/src/lib.rs` (pattern: `putio-sync-cli/src/telemetry.rs`):
  - `pub async fn start(config: &Config, state: AppState) -> Result<Option<SocketAddr>,
    CoreError>`: `config.resolve_api()?`; if `!listen` → `info!(source = ?source, "API
    listener disabled")`, `Ok(None)`; if token is `None` → `warn!("API token not
    configured (set [api] auth_token or PUTIO_SYNC_API_TOKEN); all API and UI requests
    will be refused")`; `TcpListener::bind(bind)`:
    on error `error!(bind = %bind, error = %e, "API listener failed to start")` and
    `Ok(None)`; on success spawn `axum::serve(listener,
    router(state).into_make_service_with_connect_info::<SocketAddr>())`, log
    `info!(bind = %addr, token = if set {"set"} else {"unset"}, source = ?source, "API
    listener enabled")`, return `Ok(Some(addr))`.
  - Wait: `config.api.auth_token` must reflect the resolved token (env override). Write
    the resolved token back into the `Config` passed to `AppState::new` so the
    middleware sees it.
- [X] T014 Wire the listener in `putio-sync-cli/src/commands/daemon.rs` (R4, after T013):
  - After `daemon.initialize()` and the `queue_db.set`, build the state when
    `daemon.get_client()`, `get_download_manager()` are `Some`:
    `AppState::new(config.clone() /* with resolved token */, Some(config_file_path),
    client, daemon.get_database(), download_manager, daemon.get_file_watcher())`, then
    `crate::api_listener::start(&config, state).await?` (an invalid setting is a startup
    error like metrics; a bind failure is not).
  - Then `daemon.start().await` as today. The listener task lives as long as the
    runtime.
  - Confirm `putio-sync-daemon` does not gain a dependency on `putio-sync-api` (it
    doesn't need one; only the CLI glue touches both).
- [X] T015 [P] Update `putio-sync-cli/src/commands/server.rs` (after T008):
  - Resolve `[api]` for the token only (`resolve_api()?`, copy `token` into
    `config.api.auth_token` before `create_app`); log the WARN line when unset.
  - Change `axum::serve(listener, app)` to `axum::serve(listener,
    app.into_make_service_with_connect_info::<SocketAddr>())` so the limiter sees IPs.
  - `--bind`/`--port` keep winning; `api.listen`/`api.bind` are ignored here (say so in
    the module doc comment).
- [X] T016 Run `make ci`. Start `cargo run -- daemon` against a throwaway config with
  `[api] auth_token = "x"` and confirm with curl: `/health` 200, `/local/queue` 401
  without a token and 200 with it, and the `API listener enabled bind=... token=set`
  log line. Then the same with `PUTIO_SYNC_API_LISTEN=0` (one `API listener disabled`
  line, no port open).

**Checkpoint**: the daemon serves the existing API behind a required token. Every story
below builds on this.

---

## Phase 3: User Story 1 – See the daemon's status at a glance (Priority: P1) 🎯 MVP

**Goal**: open `http://nas:15432/`, sign in, see one status page that answers "is it
working".

**Independent test**: with the `mock_ui` example running, the status page shows version,
uptime, last/next sync, queue counts, active downloads, workers, watcher, disk and
reachability, and updates within 3 s after a queue change made with curl.

### Tests for User Story 1 ⚠️ write first, then confirm they fail

- [X] T017 [P] [US1] Create `putio-sync-api/tests/status_sync.rs` with the `/status`
  cases ([contracts/api.md](contracts/api.md#get-status-new-protected)):
  - `status_full`: seed two pending and one completed queue rows; `GET /status` → 200,
    `build.version == env!("CARGO_PKG_VERSION")`, `uptime_seconds >= 0`,
    `queue == {pending 2, completed 1, others 0}`, `downloads == []`, `workers.configured
    == config.download.workers`, `disk.total_bytes > 0`, `reachability.database ==
    "ok"`, `reachability.putio == "unknown"`, `sync.last_outcome == null`,
    `auth.api_token == true`, `unavailable == []`.
  - `status_without_database`: `test_app_without_db()` → 200, `queue == null`,
    `reachability.database == "unavailable"`, `unavailable` contains `section ==
    "queue"`.
  - `status_never_contains_tokens`: response body has neither `test-api-token` nor
    `test-putio-token`.
  - `status_after_sync`: mock an empty Put.io listing, call
    `state.download_manager.sync_downloads()`, then `/status` shows
    `sync.last_outcome.result == "success"` and `reachability.putio == "ok"`.
- [X] T018 [P] [US1] Create `putio-sync-api/tests/ui_assets.rs`:
  - `index_served_public`: `GET /` without a token → 200, `content-type` starts with
    `text/html`, body contains `<script type="module"`.
  - `every_referenced_asset_resolves`: parse `src="..."` and `href="..."` values from
    the index body (a simple regex over `/assets/[^"]+`), `GET` each → 200 with
    `text/css` or `text/javascript`; also fetch every `import ... from "./x.js"` found
    in `app.js` and `pages/*.js` relative to `/assets/`.
  - `unknown_asset_404`, `etag_304`: second request with `If-None-Match` from the first
    → 304. `no_cache_header`: `Cache-Control: no-cache`.
  - `assets_total_under_150kb`: sum of all asset bodies < 150 × 1024.

### Implementation for User Story 1

- [X] T019 [P] [US1] Add `pub fn disk_usage(dir: &Path) -> std::io::Result<(u64, u64)>`
  (free, total bytes) to `putio-sync-telemetry/src/gauges.rs`, extracted from
  `refresh_disk` so both use it (R9). Unit test on the temp dir: total ≥ free > 0.
- [X] T020 [US1] Use it in `putio-sync-api/src/routes/system/stats.rs` `disk_stats`:
  replace `sys_info::disk_info()` with
  `putio_sync_telemetry::gauges::disk_usage(Path::new(download_dir))`; keep the response
  shape. Drop `sys_info` from `putio-sync-api/Cargo.toml` if nothing else uses it (grep
  first).
- [X] T021 [US1] Create `putio-sync-api/src/routes/system/status.rs` and mount it in
  `putio-sync-api/src/lib.rs` as `.route("/status", get(...))` under the protected
  routes (after T008, T019):
  - Compose the JSON exactly as in
    [contracts/api.md](contracts/api.md#get-status-new-protected): `build` from
    `env!("CARGO_PKG_VERSION")` and `option_env!("PUTIO_SYNC_GIT_SHA")`; `uptime_seconds`
    from `state.started_at`; `sync` from `download_manager.sync_status()` (SystemTime →
    RFC 3339 via `chrono::DateTime<Utc>::from`); `queue` from
    `db.queue().counts_by_status()` mapped onto the six fixed keys with 0 defaults;
    `downloads` from `get_active_downloads()`; `workers` from `active_workers()` and
    `config.download.workers`; `watcher` from `file_watcher.get_stats()` else
    `{configured: config.watch.enabled, paths: config.watch.paths, files_processed:
    null}`; `disk` from `disk_usage(target_directory)`; `reachability` and `auth`
    (`putio`: `token` if an OAuth token is configured, `app_password` if app-password
    auth is, else `none`); `unavailable` list.
  - Any failing section becomes `null` plus an `unavailable` entry; the route itself
    never fails because of one section.
  - Make T017 pass.
- [X] T022 [US1] Create `putio-sync-api/src/ui.rs`, add `pub mod ui;` in `lib.rs`, and
  mount `GET /` and `GET /assets/*path` in `public_routes` (R2, after T008):
  - A `const ASSETS: &[(&str, &str, &[u8])]` table of `(path, content_type, bytes)` using
    `include_bytes!("../ui/index.html")`, `../ui/app.css`, `../ui/app.js`,
    `../ui/api.js`, `../ui/format.js`, `../ui/pages/{login,status,queue,putio,transfers,
    history,settings}.js`. Add every file now with placeholder content (an empty module
    `export {};` for pages not yet written) so the crate compiles; later tasks fill them.
  - Handlers set `Content-Type`, `Cache-Control: no-cache`, `ETag: "<CARGO_PKG_VERSION>"`
    and answer 304 on a matching `If-None-Match`. Unknown path → 404.
- [X] T023 [P] [US1] Write `putio-sync-api/ui/index.html` and `putio-sync-api/ui/app.css`
  ([contracts/ui.md](contracts/ui.md#shell-indexhtml-appjs-appcss)):
  - `index.html`: `<meta name="viewport" content="width=device-width, initial-scale=1">`,
    `<link rel="stylesheet" href="/assets/app.css">`, a `<header>` with the product
    name, `<nav>` links for the six pages, a sign-out button, a metrics link slot, a
    `<div id="banner" hidden>` for disconnected, `<main id="view">`, `<div id="toasts">`,
    `<dialog id="confirm">` with title, text, Cancel and a confirm button, and
    `<script type="module" src="/assets/app.js"></script>`, and a `<noscript>` block
    inside `#view` saying a current browser with JavaScript is required (spec edge case
    "browser without scripting"). No inline scripts.
  - `app.css`: system font stack, CSS variables for light/dark via
    `prefers-color-scheme`, a `.table` that is a real table ≥ 601 px and stacked cards
    (`display: grid`, `data-label` pseudo-headers) at ≤ 600 px, buttons with 44 px touch
    targets, `.banner`, `.toast`, progress bars, no horizontal overflow (`overflow-x:
    hidden` on `body`, `word-break: break-word` on names).
- [X] T024 [P] [US1] Write `putio-sync-api/ui/api.js` and `putio-sync-api/ui/format.js`:
  - `api.js` exports `request(method, path, body)`, `get(path)`, `post(path, body)`,
    `put`, `del`, `login(token)`, `logout()`, `getToken()`, `onAuthExpired(fn)`.
    `request` adds `Authorization: Bearer <token>` when stored under
    `localStorage['putio-sync.token']`, sends JSON, returns parsed JSON (or `null` for
    204), throws `{status, error, details, retryAfter}` from the error body, and on 401
    clears the token and calls the registered handler. Wrap `localStorage` access in
    try/catch (private mode).
  - `format.js` exports `bytes(n)` (B/KB/MB/GB/TB, 1 decimal), `duration(seconds)`
    (`1h 02m`, `45s`), `relative(iso)` (`3 s ago`, `in 42 s`), `percent(x)`,
    `dateTime(iso)` (local time).
- [X] T025 [US1] Write `putio-sync-api/ui/app.js` (after T023, T024):
  - Hash router: `#/status` default, `#/queue`, `#/putio[/<id>]`, `#/transfers`,
    `#/history`, `#/settings`, `#/login`; each page module exports `{ mount(el,
    params), unmount() }`; `app.js` imports them from `./pages/<name>.js`.
  - Auth gate: no token → render login; `onAuthExpired` → login with "session
    expired". Sign-out button → `logout()` → login.
  - Poll scheduler `poll(fn, ms)` returned to pages: pauses on `document.hidden`, resumes
    on `visibilitychange`, backs off to 10 000 ms after a thrown network error and shows
    the banner, restores the page's interval and hides the banner on the next success.
  - `toast(text, kind)`, `confirm({title, text, verb}) -> Promise<bool>` using the
    `<dialog>`, Cancel focused by default.
  - Remember the last route in `localStorage['putio-sync.page']` (try/catch).
- [X] T026 [US1] Write `putio-sync-api/ui/pages/status.js`
  ([contracts/ui.md](contracts/ui.md#pages) Status row):
  - Poll `GET /status` every 3 000 ms. Render cards: Build & uptime; Sync (last run,
    outcome summary or error text, next due, in-progress badge, a "Sync now" button
    that `POST /local/sync` and toasts 202 "sync started" or 409 "sync already
    running"); Queue counts; Active downloads table (name, done/size, %, speed, ETA);
    Workers busy/configured; Watcher configured + paths + processed; Disk free/total
    with a bar; Reachability database/Put.io as coloured dots; Auth method.
  - A section that is `null` renders its `unavailable` reason in place of the card.
- [X] T027 [US1] Fill the remaining page placeholders from T022 with a minimal "coming
  soon" `mount` that renders the page title, so navigation works before US3–US6 (edit
  `putio-sync-api/ui/pages/{queue,putio,transfers,history,settings,login}.js`; the login
  page gets its real content in T031).
- [X] T028 [US1] Create `putio-sync-api/examples/mock_ui.rs` (R10):
  - Builds the same pieces as `tests/common/mod.rs` but as an example binary (examples
    can't import `tests/common`, so copy the ~40 lines or move the builder into a
    `pub mod testing` behind `#[cfg(any(test, feature = "testing"))]`; prefer the copy).
  - Mockito mocks: `/v2/files/list?parent_id=0` with two folders and one file,
    `parent_id=<folder>` with two files (one `.exe`), `/v2/files/{id}` for each,
    `/v2/transfers/list` with one transfer, `/v2/files/delete`, `/v2/transfers/add` and
    `/v2/transfers/cancel` returning success (check exact paths in
    `putio-sync-client/src/`). Mocks must be `.expect_at_least(0)` and persistent.
  - Seed the queue with four rows (two pending, one completed, one paused), bind
    `127.0.0.1:0`, serve with connect info, print `Open http://127.0.0.1:<port>/ and sign
    in with token: mock-ui-token`, run until Ctrl+C.
  - Add `[[example]] name = "mock_ui"` to `putio-sync-api/Cargo.toml` if required
    (dev-dependencies are available to examples by default).
- [X] T029 [US1] Run `make ci`, then quickstart §2 steps 1–2 and §3 in a real browser
  (desktop and 375 px). Fix layout overflow before moving on. Make T017 and T018 green.

**Checkpoint**: MVP. The NAS can show its status page.

---

## Phase 4: User Story 2 – Protected access (Priority: P1)

**Goal**: sign-in page, sign-out, and proof that nothing leaks.

**Independent test**: an unauthenticated browser sees only the sign-in page; a wrong
token is refused; six wrong tokens are rate-limited; after sign-in a reload stays signed
in; no response or log contains the token.

### Tests for User Story 2 ⚠️ write first

- [X] T030 [US2] Add to `putio-sync-api/tests/auth.rs` (same file as T012, so not [P]):
  `auth_status_public` (`GET /auth/status` → `{token_configured: true}` without a token;
  `false` with `test_app_without_token`); `login_204_on_match`; `login_401_on_mismatch`
  (and the failure counts toward the limiter: 5 bad logins then a 6th → 429);
  `login_401_when_not_configured`; `login_body_never_logged` is covered by T031.
- [X] T031 [P] [US2] Create `putio-sync-api/tests/secrets_sweep.rs` (SC-004, FR-008):
  - Install a `tracing_subscriber` with a writer that appends to a shared
    `Arc<Mutex<Vec<u8>>>` (`tracing_subscriber::fmt().with_writer(...)`, `try_init`).
  - Walk every route in [contracts/api.md](contracts/api.md) plus the existing ones
    (`/putio/files`, `/putio/transfers`, `/local/queue`, `/local/downloads`,
    `/local/watcher/status`, `/local/workers`, `/settings/*`, `/stats/*`, `/history`,
    `/history/stats`, `/status`, `/`, `/assets/app.js`) with and without the token, plus
    a failed `/auth/login`. Assert no response body and no captured log line contains
    `test-api-token` or `test-putio-token`.
  - `every_protected_route_refused_without_token` (SC-003): the same route list minus
    the public ones, without a token, every status is 401 (a missing token is not
    counted by the limiter, so it never trips to 429); repeat with
    `test_app_without_token()` and assert 401 with the "not configured" body.

### Implementation for User Story 2

- [X] T032 [US2] Create `putio-sync-api/src/routes/system/auth.rs`, add it to
  `routes/system/mod.rs`, and mount `/auth/status` and `/auth/login` in `public_routes`
  in `putio-sync-api/src/lib.rs` (after T022, same file):
  - `GET /auth/status` → `{ "token_configured": config.api.auth_token.is_some() }`.
  - `POST /auth/login` with `{ "token": String }`: limiter `check(ip)` first (429 as in
    the middleware); no token configured → 401 "API token not configured" with details;
    `token_matches` → 204; else `record_failure`, WARN `Authentication failed`, 401
    `Unauthorized`. Never log the body. Derive nothing with `Debug` on the request.
  - Make T030 pass.
- [X] T033 [US2] Write `putio-sync-api/ui/pages/login.js`
  ([contracts/ui.md](contracts/ui.md#pages) Login row):
  - On mount `GET /auth/status`; if `token_configured` is false render the "Set `[api]
    auth_token` or `PUTIO_SYNC_API_TOKEN` and restart" notice, no form.
  - Otherwise a form with one password-type field and "Sign in"; on submit
    `login(token)`; 204 → navigate to the remembered route or `#/status`; 401 → inline
    "Token not accepted"; 429 → "Too many attempts, try again in N s" with a countdown
    from `retryAfter`; network error → "Can't reach the daemon".
  - Autofocus the field; `autocomplete="current-password"` so phone keyboards behave.
- [X] T034 [US2] Make T031 pass: fix any log line or response that leaks (expected
  suspects: the client's `Debug` output in error chains, `info!` lines in
  `server.rs`/`daemon.rs` that print the config). Run `make ci`, then quickstart §2 step
  1 and 7 and §4.

**Checkpoint**: US1 + US2 is a shippable release.

---

## Phase 5: User Story 3 – Manage the queue and downloads (Priority: P2)

**Goal**: the queue page with every action and "sync now".

**Independent test**: with `mock_ui`, every queue action changes the list within 3 s;
"Sync now" starts a sync and a second click during it is refused.

### Tests for User Story 3 ⚠️ write first

- [X] T035 [US3] Add to `putio-sync-api/tests/status_sync.rs` (same file as T017):
  `sync_now_202_then_409` (mockito listing delayed 500 ms: first `POST /local/sync` → 202
  with `in_progress == true`; immediate second → 409 `"A sync is already in progress"`;
  after completion `/status` shows the outcome), `sync_now_503_without_db`.
- [X] T036 [P] [US3] Create `putio-sync-api/tests/queue_actions.rs`: seed three pending
  rows with priorities 0,1,2 and one completed; assert each of `move_up`, `move_down`,
  `move_first`, `move_last`, `skip`, `pause` (pending → paused, counts via
  `/local/queue/stats`), `resume` (paused → pending), `clear` (completed row gone) and
  `DELETE /local/downloads/{id}` for an unknown id (404 or the existing error, not 500)
  behaves as `GET /local/queue` shows. These routes exist; the tests document them for
  FR-021.

### Implementation for User Story 3

- [X] T037 [US3] Create `putio-sync-api/src/routes/local/sync.rs`, add to
  `routes/local/mod.rs`, mount `POST /local/sync` in `putio-sync-api/src/lib.rs` (after
  T032, same file): no database → `ServiceUnavailable("Database unavailable")`;
  `download_manager.request_sync()` → `Err(InProgress)` → `Conflict("A sync is already
  in progress")`; `Ok` → 202 with the `sync` object (reuse the serializer from
  `status.rs`; move it to a `pub(crate) fn sync_json(&SyncStatus)` in `status.rs`).
  Make T035 pass.
- [X] T038 [US3] Write `putio-sync-api/ui/pages/queue.js`
  ([contracts/ui.md](contracts/ui.md#pages) Queue row):
  - Poll `GET /local/queue` and `GET /local/downloads` every 3 000 ms (two requests per
    3 s is within FR-011's one per second).
  - Active downloads table first (name, done/size, bar, speed, ETA, Cancel with
    confirmation "Cancel download of <name>?" → `DELETE /local/downloads/{id}`).
  - Queue table: name, size, path, state badge, priority, age (`relative(added_at)`),
    and per-row ↑ ↓ ⤒ ⤓ Skip buttons (`POST /local/queue/{file_id}/move_up|move_down|
    move_first|move_last|skip`). Toolbar: Pause all, Resume all, Clear completed
    (confirm "Remove N completed items from the queue?" with N from the current list),
    Sync now (same handler as the status page; factor it into `api.js` as `syncNow()`).
  - After every action, refresh immediately; toast the server's message.
- [X] T039 [US3] Run `make ci`, quickstart §2 step 3; make T036 green.

---

## Phase 6: User Story 4 – Browse Put.io and manage transfers (Priority: P2)

**Goal**: browse folders, queue a file or folder, delete with guards, manage transfers.

**Independent test**: with `mock_ui`, browsing shows the mocked tree; "Download" on the
folder toasts `found 2, queued 1, blocked 1`; delete is hidden for a queued file and
confirmed for another; add/start/stop/remove a transfer.

### Tests for User Story 4 ⚠️ write first

- [X] T040 [P] [US4] Create `putio-sync-api/tests/queue_enqueue.rs`: mock a folder with
  `a.mkv`, `b.exe` (config `blocked_extensions = ["exe"]`) and `c.mkv` already queued;
  `POST /local/queue {file_id: folder}` → 202 `{found 3, queued 1, skipped 1, blocked 1,
  failed 0}`; `POST` with an unknown id (mockito 404) → 404; without db → 503; a single
  file id → `{found 1, queued 1, ...}`.
- [X] T041 [P] [US4] Create `putio-sync-api/tests/putio_delete_guard.rs`: seed file 7
  as `pending` and file 8 as `completed`; mock `/v2/files/delete` to succeed;
  `DELETE /putio/files/7` → 409 `"File is queued or downloading"` and the mock was not
  hit (`expect(0)` + `assert_async`); `DELETE /putio/files/8` → 200. Also `paused` → 409
  and `downloading` → 409.

### Implementation for User Story 4

- [X] T042 [US4] Add `POST /` to `putio-sync-api/src/routes/local/queue.rs` (R12): body
  `{ file_id: i64 }`; no db → 503; `download_manager.enqueue_tree(file_id)` →
  `Err` whose chain contains a client 404 → `NotFound("File not found")`, other errors →
  500; `Ok(stats)` → 202 `{found: total_files, queued: files_downloaded, skipped:
  files_skipped, blocked: files_blocked, failed: files_failed}`. Make T040 pass.
- [X] T043 [P] [US4] Guard `DELETE /putio/files/{id}` in
  `putio-sync-api/src/routes/putio/files.rs` (R11): when `state.database` is `Some`,
  `queue().get_by_file_id(id)`; status `pending`, `downloading` or `paused` →
  `Conflict("File is queued or downloading").with_details("Skip or cancel it first")`.
  Make T041 pass.
- [X] T044 [P] [US4] Write `putio-sync-api/ui/pages/putio.js`
  ([contracts/ui.md](contracts/ui.md#pages) Put.io row): route param = folder id
  (default 0); `GET /putio/files?parent_id=<id>`; breadcrumb built while descending
  (keep a stack in module state; a direct link to `#/putio/<id>` shows `… / <id>`);
  filter input narrows rows client-side (case-insensitive substring on name); folder
  rows navigate; file rows open a details panel (`GET /putio/files/{id}`); Download
  button on files and folders → `POST /local/queue` → toast "found N, queued N, skipped
  N, blocked N"; Delete button hidden when the id is in the last `GET /local/queue`
  result with status pending/downloading/paused (fetch the queue once on mount and
  after each action), else confirm "Delete <name> from Put.io? This cannot be undone." →
  `DELETE /putio/files/{id}`; a 409 from the server is toasted. When a Put.io request
  fails, render the error text with a Retry button in place of the listing (spec edge
  case "Put.io unreachable").
- [X] T045 [P] [US4] Write `putio-sync-api/ui/pages/transfers.js`
  ([contracts/ui.md](contracts/ui.md#pages) Transfers row): poll `GET /putio/transfers`
  every 5 000 ms; table name, status, %, size, speed; add form with a text field
  (magnet or URL → `POST /putio/transfers {url}`) and a file input (`.torrent` read with
  `FileReader.readAsDataURL`, strip the prefix, send `{file_data: <base64>}`); Start /
  Stop buttons → `POST /putio/transfers/{id}/start|stop`; Remove → confirm "Remove
  transfer <name>?" → `DELETE /putio/transfers/{id}`. A failed list shows the error and
  a Retry button; polling continues.
- [X] T046 [US4] Extend `putio-sync-api/examples/mock_ui.rs` (same file as T028) so the
  transfer mocks cover add/start/stop/cancel, then run `make ci` and quickstart §2 steps
  4–5.

---

## Phase 7: User Story 5 – Review history (Priority: P3)

**Goal**: history page with action and name filters, paging and totals.

**Independent test**: with `mock_ui` seeded history, filtering by `ubuntu` shows only
matching rows and the total updates; paging works.

### Tests for User Story 5 ⚠️ write first

- [X] T047 [P] [US5] Add tests in `putio-sync-db/src/history.rs` `mod tests` (or
  `putio-sync-db/tests/history_filter.rs`): insert `Ubuntu.iso`, `debian.iso`,
  `100%_done.mkv`; `query(None, 10, 0, Some("ubuntu"))` returns one row;
  `Some("%")` returns only `100%_done.mkv`; `Some("_done")` returns one row (underscore
  literal); `count(None, Some("iso"))` is 2.
- [X] T048 [P] [US5] Create `putio-sync-api/tests/history_filter.rs`: seed history via
  `db.history().add(...)` (check the method name), `GET /history?name=ubu` → one entry,
  `total == 1`; `?action=deleted&name=x` combine; `?name=` (empty) behaves as no filter.

### Implementation for User Story 5

- [X] T049 [US5] Add `name: Option<&str>` to `query` and `count` in
  `putio-sync-db/src/history.rs` (R7): when `Some` and non-empty, append
  `AND LOWER(filename) LIKE ? ESCAPE '\'` with the fragment escaped (`\` → `\\`, `%` →
  `\%`, `_` → `\_`), lower-cased and wrapped in `%…%`. Update every caller in the
  workspace (`grep -rn "history().query\|history().count"`), passing `None`. Make T047
  pass.
- [X] T050 [US5] Add `name: Option<String>` to `HistoryQuery` in
  `putio-sync-api/src/routes/system/history.rs` and pass it to `query` and `count`
  (empty string → `None`). Make T048 pass.
- [X] T051 [P] [US5] Write `putio-sync-api/ui/pages/history.js`
  ([contracts/ui.md](contracts/ui.md#pages) History row): filters (action select
  all/downloaded/deleted, name text with 300 ms debounce), page size 50 with Prev/Next
  and "x–y of total", totals line from `GET /history/stats`; rows: time
  (`dateTime`), action badge, name, size, path. No polling.
- [X] T052 [US5] Seed history rows in `putio-sync-api/examples/mock_ui.rs` (same file as
  T046), run `make ci` and quickstart §2 step 6 (history half).

---

## Phase 8: User Story 6 – Adjust settings (Priority: P3)

**Goal**: settings page for the four groups.

**Independent test**: with `mock_ui`, each group loads, a save shows the API's message,
an invalid value shows the validation error, and the saved value is in the temp config
file.

### Tests for User Story 6 ⚠️ write first

- [X] T053 [P] [US6] Create `putio-sync-api/tests/settings.rs`: for each of
  `/settings/download`, `/settings/watcher`, `/settings/logging`,
  `/settings/notifications`: `GET` → 200 with the documented fields; `PUT` with the same
  body and one changed field → 200 with a `message`, then `GET` shows the change and the
  temp config file (`config_path` passed to `AppState::new`) contains it; `PUT` with an
  invalid value (e.g. `workers: 0`, `level: "loud"`) → 400 with a non-empty `error`.
  These routes exist; fix any that return 500 instead of 400 on bad input.

### Implementation for User Story 6

- [X] T054 [US6] Write `putio-sync-api/ui/pages/settings.js`
  ([contracts/ui.md](contracts/ui.md#pages) Settings row): four `<form>`s built from
  the `GET` bodies (booleans → checkbox, numbers → `type=number`, arrays of strings →
  textarea one per line, `Option` → empty allowed, `progress_style`/`sort_order`/
  `level`/`console_format`/`backend` → `<select>` with the values the API accepts;
  read them from `putio-sync-core/src/config.rs` validation); Save per group → `PUT`
  → toast the `message` verbatim (it says whether a restart is needed); 400 → show
  `error` under the form. Never render the `[api]` or Put.io auth settings.
- [X] T055 [US6] Run `make ci` and quickstart §2 step 6 (settings half); make T053
  green.

---

## Phase 9: Polish & Cross-Cutting Concerns

- [X] T056 [P] Update `config.example.toml`: add the `[api]` block from
  [contracts/config.md](contracts/config.md#toml) with comments, `auth_token` commented out
  (`# auth_token = "change-me"`) with the "required for the UI" note; an empty string fails
  startup per contracts/config.md, `enabled`/`url` commented out.
- [X] T057 [P] Update `README.md` (FR-022): a "Web UI" section: URL
  (`http://<nas>:15432/`), that `[api] auth_token` or `PUTIO_SYNC_API_TOKEN` is required
  and open mode is gone (breaking change note), the port to publish, the plain-HTTP LAN
  warning, the env table from contracts/config.md, `PUTIO_SYNC_API_LISTEN=0` to turn it
  off, the "sync now" and name-filter additions, and that the legacy `/api/*` routes are
  removed. Keep the existing API docs accurate: add `GET /status`, `POST /local/sync`,
  `POST /local/queue`, `GET /history?name=`, `/auth/*`.
- [X] T058 [P] Add `EXPOSE 15432` to `Dockerfile` next to the metrics `EXPOSE`.
- [X] T059 Final gate: `make ci`; `cargo build --release` and confirm the binary serves
  `/` (assets embedded); run quickstart §2–§4 end to end; measure the `ui/` total
  (`du -ck putio-sync-api/ui`) and record it in research.md R1 if it moved.

---

## Dependencies & Execution Order

### Phase dependencies

- **Setup (T001–T002)**: no dependencies; T001 and T002 touch different files.
- **Foundational (T003–T016)**: needs Setup, blocks every story.
  - core: T003 → T004 (same file).
  - download: T005 → T006 → T007 (T006 and T007 share `manager.rs`).
  - api: T008 first (everything in `lib.rs` after it). T009 in parallel with T008.
    T010 after both. T011 after T008. T012 after T010 and T011.
  - cli: T013 after T008; T014 after T013; T015 after T008 (parallel with T013/T014).
  - T016 last.
- **US1 (T017–T029)**: needs Foundational. T017 and T018 first (fail). T019 → T020 →
  T021. T022 after T008 (edits `lib.rs`), before T023–T027 are served but they can be
  written in parallel with it. T025 after T023 and T024. T026 after T025. T027 after
  T022. T028 after T021 and T022. T029 last.
- **US2 (T030–T034)**: needs US1 (login page uses `app.js`; `lib.rs` chain). T030 after
  T012 (same file). T032 after T022 (same `lib.rs`). T033 after T025.
- **US3 (T035–T039)**: needs US1 and T006. T035 after T017 (same file). T037 after T032
  (same `lib.rs`). T038 after T025.
- **US4 (T040–T046)**: needs US1 and T007. T042 after T040; T043 after T041. T046 after
  T028 (same file).
- **US5 (T047–T052)**: needs Foundational only for the db/API parts; the page needs US1.
  T049 after T047; T050 after T049 and T048. T052 after T046.
- **US6 (T053–T055)**: needs US1.
- **Polish (T056–T059)**: after the chosen stories.

### Same-file sequencing (not [P])

| File | Tasks, in order |
|---|---|
| `putio-sync-core/src/config.rs` | T003 → T004 |
| `putio-sync-download/src/manager.rs` | T006 → T007 |
| `putio-sync-api/src/lib.rs` | T008 → T021 → T022 → T032 → T037 |
| `putio-sync-api/src/middleware/auth.rs` | T010 only |
| `putio-sync-api/tests/auth.rs` | T012 → T030 |
| `putio-sync-api/tests/status_sync.rs` | T017 → T035 |
| `putio-sync-api/ui/app.js` | T025 only (pages import it; they do not edit it) |
| `putio-sync-api/ui/pages/login.js` | T027 → T033 |
| `putio-sync-api/examples/mock_ui.rs` | T028 → T046 → T052 |
| `putio-sync-api/src/routes/local/queue.rs` | T042 only |
| `putio-sync-db/src/history.rs` | T047 → T049 |
| `putio-sync-api/src/routes/system/history.rs` | T050 only |
| `putio-sync-cli/src/commands/daemon.rs` | T014 only |
| `putio-sync-cli/src/commands/server.rs` | T015 only |

## Parallel Examples

```text
# Setup:
T001 api/Cargo.toml  |  T002 api/error.rs

# Foundational, three crates at once:
T003→T004 core/config.rs  |  T005→T006→T007 download  |  T008 api/lib.rs + T009 ratelimit.rs

# Foundational, after T008:
T010 auth.rs  |  T011 tests/common  |  T013 cli/api_listener.rs  |  T015 cli/server.rs

# US1 tests, then implementation:
T017 tests/status_sync.rs  |  T018 tests/ui_assets.rs
T019 gauges.rs  |  T023 index.html+app.css  |  T024 api.js+format.js

# US4, four files:
T040 tests/queue_enqueue.rs  |  T041 tests/putio_delete_guard.rs  |  T044 putio.js  |  T045 transfers.js

# Polish:
T056 config.example.toml  |  T057 README.md  |  T058 Dockerfile
```

## Implementation Strategy

### MVP (US1 only)

1. Setup and Foundational (T001–T016). The daemon serves the API behind a token.
2. US1 (T017–T029). The status page works against `mock_ui` and on the NAS.
3. **Stop and validate**: `make ci`, quickstart §2 steps 1–2, §3, §4.

US2's server side (required token, limiter) is already in Foundational, so the MVP is
safe to expose on the LAN; US2 adds the sign-in page polish and the leak proof.

### Incremental delivery

1. US2 (sign-in page, secrets sweep). First release candidate.
2. US3 (queue page, sync now). Replaces the SSH-and-sqlite sessions.
3. US4 (Put.io browser, transfers, enqueue, delete guard).
4. US5 (history), US6 (settings).
5. Polish (T056–T059).

Each step leaves `make ci` green and can ship on its own.

## Implementation Notes

- `AppState::new` takes the `Config` whose `api.auth_token` must already hold the
  resolved token (env override applied). Both `api_listener::start` and `server.rs` do
  that before building the state; `tests/common` sets it directly.
- `ConnectInfo<SocketAddr>` is only present when the server is started with
  `into_make_service_with_connect_info`. `client_ip()` falls back to `127.0.0.1`; tests
  therefore share one limiter bucket per `TestApp`, which is fine because each test
  builds its own app.
- `SystemTime` is used in `putio-sync-download` to avoid adding `chrono` there; the API
  converts with `chrono::DateTime::<Utc>::from(system_time).to_rfc3339()`.
- The UI pages must not import each other; shared helpers live in `api.js`,
  `format.js` and the `app.js` exports (`poll`, `toast`, `confirm`, `navigate`).
- When a task finds that an existing route returns 500 for an expected client error
  (bad input, unknown id), fix it to the right 4xx as part of that task and add the
  case to the test; that is within FR-021's "each queue action".

Phase notes, recorded at the end of the feature (T059). Each records what differs from
the task text above.

Setup and Foundational (T001–T016):

- The `/putio/*` route handlers use the shared `AppState` client instead of building
  their own, so `mock_ui` and the tests can point them at mockito.
- The limiter counts only real failures: a protected request that presents a non-empty,
  wrong token, or a wrong `POST /auth/login`. A request with no token is refused (401)
  but not counted, and nothing is counted while no token is configured, so stray
  browser requests (`/robots.txt`, touch icons) can't lock the operator out.
  contracts/api.md says so.
- The 429 check runs before the token compare, so a locked-out IP is refused even with
  the right token until 60 s after its first failure.
- Removed as planned: legacy `/api/*` routes, open mode, the permissive `CorsLayer`.

US1 and US2 (T017–T034):

- Committed at 73f9001. No deviations beyond the Foundational ones above.

US3 (T035–T039):

- Queue order is `priority DESC, created_at ASC`: higher priority downloads first.
  The folder sync queues every file at the same priority, so swapping two rows did
  nothing. Every move (`move_up/down/first/last`) now renumbers the whole queue in one
  transaction (`QueueRepository::set_order`, N down to 1). Order rule (pre-deploy
  review): a move keeps everyone's relative order, so a row that sorts first stays ahead
  of everything nobody moved above it, and only the operator's explicit move above it
  wins; rows the sync adds later (priority 0) queue behind every renumbered row. There
  is no +1000 resume boost in the current code to lose: it belonged to the old job-based
  pool and went with 8440a79. Today a worker told to stop (fewer `workers`) finishes its
  current file first, and a partial `.<name>.downloading` file resumes from its bytes
  whenever its row is next claimed, whatever its priority. Test:
  `queue_actions.rs::moves_keep_a_raised_row_ahead_unless_moved_above`. The README
  "Worker Scaling" section and the spec's "Worker count lowered" edge case were
  rewritten to this behaviour; the old 75 % / re-queue-first rule is gone.
- Fixed while testing: `GET /local/queue` returned `status` as a quoted string
  (`"\"pending\""`); it is now plain. `DELETE /local/downloads/{id}` for an id not in
  the queue is 404, not a success.

US4 (T040–T046):

- `ClientError::is_not_found()` identifies a Put.io 404, so `POST /local/queue` maps it
  to 404 `File not found` instead of matching message text in the API.
- `.torrent` uploads go to `<base_url>/files/upload` when the client has a custom base
  URL (`DEFAULT_UPLOAD_URL` otherwise), so the mock and tests receive them instead of
  the real upload host. Production behaviour is unchanged.
- Transfers Start/Stop: Put.io has no start/stop call, so both routes are server-side
  no-ops that return "Transfer N is managed automatically by Put.io"; the page toasts it.
  FR-016's start/stop are therefore present but do nothing on Put.io.

US5 (T047–T052):

- The db tests live in `putio-sync-db/src/history.rs` `mod tests`; the API test is
  `putio-sync-api/tests/history_filter.rs`. `mock_ui` seeds 64 rows (two pages).

US6 (T053–T055):

- Settings PUTs validate a copy of the whole config and keep it only if it passes, so a
  refused PUT changes nothing in memory or on disk. Invalid values are 400 with a JSON
  `error`; a malformed or mistyped body is also 400 (axum's default was 422).
- `logging::validate_filters` refuses a bad level or module filter before it is saved,
  instead of the reload poller rejecting it later.
- Enabling the watcher requires every watch folder to exist (existing `validate`).
- What a save applies live was checked in the code (README "Settings from the UI"):
  - `daemon`: logging `level`/`module_filters` within 10 s via the 003 poller; download
    `workers` (pool scales), bandwidth, buffer, retries, `skip_existing`,
    `delete_after_download`, `blocked_extensions` and `target_directory` on the next
    download (the daemon's queue database and disk figures stay on the startup folder);
    watcher `paths`, `parent_folder_id`, `archive_directory` at the next scan, only if
    the daemon started with a watch folder. Restart: other logging and download fields,
    watcher `enabled` (also `POST /local/watcher/start|stop`, which only save the flag),
    `delete_after_upload`, and all notifications. `refresh_cycle` is unused by the
    daemon, which scans on `--watch-interval`.
  - `server`: logging as above if the config file existed at startup; download changes
    reach only the API's own download manager, not the `--download` thread; watcher and
    notifications need a restart.
  - Closed in the pre-deploy review: each PUT diffs the group's GET body before and
    after and names the changed fields as "Applies …" or "Needs a restart", for the
    engine (`AppState::engine`, set to `Daemon` by `AppState::on_daemon` in the daemon
    command) and for whether a reload poller runs (`AppState::logging_reload`, true when
    the config file existed at startup). A save that changed nothing says so;
    `target_directory`, `refresh_cycle` under `daemon` and `level` while `RUST_LOG` is
    set get a sentence of their own. `POST /local/watcher/start|stop` now say the flag
    applies after a restart. The UI shows the message as is; the static "Applies within
    10 s" hint on the level field is gone.

Polish (T056–T059):

- T056: `config.example.toml` gains an `[api]` section with `# auth_token = "change-me"`
  commented out and the "required for the UI" note (an empty string fails startup),
  `listen`/`bind` at their defaults, and `enabled`/`url` commented out. The TOML block in
  contracts/config.md showed `auth_token = ""`, which contradicts its own Errors table;
  it now shows the commented line.
- T057: README gains a "Web UI" section (pages, token and breaking change, lockout,
  plain-HTTP warning, env table, `PUTIO_SYNC_API_LISTEN=0`, port, live vs restart
  table, route table, `/api/*` and CORS removal), plus Features, Architecture, "Live
  configuration changes" and Docker env/ports updates. The README had no API route
  docs before, so the route table is new rather than an update.
- T058: the Dockerfile had no metrics `EXPOSE` either, so it now has `EXPOSE 9742 15432`.
  Nothing else is needed: the UI is embedded with `include_bytes!` and `ui/` is copied
  with the crate (`.dockerignore` doesn't exclude it).
- T059: UI total 82,206 bytes (about 82 KB, 12 files) against the 150 KB budget;
  recorded in research.md R1.
- The `AppState` doc comment no longer says logging changes need a restart.
- Gate (2026-10-26): `make ci` passes (469 tests passed, 0 failed, 2 ignored);
  `docker build` succeeds and the image exposes 9742/tcp and 15432/tcp. The release
  binary (`server --port 15498`, no token) serves `/` and `/assets/*` (content-hash
  ETag), logs one WARN "API token not configured", and answers `/status` with 401 "API
  token not configured".
- Quickstart run locally (2026-10-26, mock_ui on port 15499 since 15432 is taken here):
  - §2: all seven steps pass. Wrong token, then lockout message after the fifth failure;
    status; queue move, resume all, sync now (second click "already running"); Put.io
    browse, name filter, download, delete confirmation (cancelled); transfers add,
    remove with confirmation (stop toasts the DELETE hint, since start/stop are no-ops);
    history action filter and paging (50 + 14); settings load, browser-side validation
    of `workers = 0`, a valid save message; reload keeps the session; sign out clears
    the token from `localStorage`.
  - §3: at 375 px every page has `scrollWidth` 375 (no horizontal scroll) and table
    rows render as cards.
  - §4: `/health` 200; `/status` and an unknown path 401 without a token; wrong token
    401 ×5 then 429, also with the right token; the token appears 0 times in `/status`
    and `/settings`; `PUT /settings/download` with `workers: 0` or a malformed body 400;
    `/api/status` 404. The daemon variant of the no-token and port-taken checks was not
    run (it would call the real Put.io); `tests/server_bind.rs` covers the bind failure.
  - The breadcrumb shows "Put.io / … / 100" when a folder URL is opened directly
    (cosmetic).

Operator steps still pending on the NAS (004, plus the open 003 steps):

1. In the Unraid template, publish container port 15432 (and 9742 if the 003 metrics
   port was never published).
2. Set `PUTIO_SYNC_API_TOKEN` in the template to a new random token
   (`openssl rand -hex 24`); don't put it in config.toml.
3. Pull the new image and restart; expect `API listener enabled bind=0.0.0.0:15432
   token=set`.
4. Run quickstart §5–§6 (desktop and phone sign-in, live status, sync now, token grep
   of the container log = 0).
5. Still pending from 003: `deploy-monitoring.yml --limit monitoring_server` and 003
   quickstart §6.
6. Before the first UI settings save on the NAS, check that config.toml has no
   `auth_token` line unless you put one there (the pre-deploy fix below keeps env-only
   tokens out of the file from now on).

Pre-deploy review (2026-10-26):

- Env-only token: settings PUTs and `POST /local/watcher/start|stop` write with
  `Config::save_keeping_file_credentials`, which re-reads the file and writes `[auth]`
  and `api.auth_token` as they are there (absent stays absent). Memory keeps the
  resolved token for the auth middleware. A file that no longer parses is left alone
  and the 500 names no content. The save happens before the in-memory swap, so a failed
  write changes nothing. No Put.io token has an env override; `[auth]` is kept from disk
  so a `login` run while the daemon runs isn't reverted. Tests: core
  `save_keeping_file_credentials_tests`, API `settings.rs::env_only_api_token_is_not_written`,
  `file_tokens_are_kept`, `watcher_start_stop_keep_tokens_and_say_restart`.
- Save messages: see US6 above. Tests: `settings.rs` `*_messages`, `unchanged_save_says_so`.
- Queue order: see US3 above.
- `archive_directory`: the daemon ignored it at startup (with a warning) while a live
  save applied it. The README's config-file setup and `example-config.toml` document it,
  so the daemon now honours it at startup: `DaemonConfig::archive_dir` reaches the
  `TorrentWatcher` (`WatcherConfig::archive_dir`), the startup log prints it, and
  `Config::load` expands `~` in it like the other paths. `--archive-dir` is still ignored
  in daemon mode. Tests: daemon `watcher_starts_with_archive_directory`, core
  `archive_directory_is_expanded_on_load`.
