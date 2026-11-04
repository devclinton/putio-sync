# Tasks: First-Run Setup Wizard

**Input**: Design documents from `specs/013-setup-wizard/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/

Tests are required by the constitution (Principle III: mocked Put.io, run under `make ci`).

## Format: `[ID] [P?] [Story] Description`

## Phase 1: Setup

- [X] T001 Raise the UI bundle cap from 260 KB to 300 KB in `putio-sync-api/tests/ui_assets.rs` (rename the test to `assets_total_under_300kb`; user decision)

## Phase 2: Foundational (blocking)

- [X] T002 [P] Add `UNAUTHORIZED_MESSAGE` and `ClientError::is_unauthorized()` in `putio-sync-client/src/error.rs`; use the constant where the client maps 401 in `putio-sync-client/src/client.rs`; unit test
- [X] T003 [P] Add `[putio] app_id` (`PutioConfig`) and `[download] preset` to `putio-sync-core/src/config.rs` with validation of the preset name; unit tests for parse, defaults and an unknown preset
- [X] T004 Apply `[download] preset` at the root of `RuleSet::resolve` in `putio-sync-core/src/rules.rs` (unset keeps `blocked_extensions`); tests for root preset, folder rule overriding it, and unchanged behaviour when unset
- [X] T005 Create `putio-sync-core/src/setup.rs` (export in `lib.rs`): `SetupReason`, `SetupCheck`, `check_local(path, Option<&Config>)`, `resolve_app_id(&Config)`, `in_container()`, `suggest_download_dir`, `suggest_watch_dir`; unit tests with temp dirs

**Checkpoint**: config, rules and setup reasons available to every crate.

## Phase 3: User Story 1 — setup mode instead of exit (P1) 🎯 MVP

**Goal**: an unconfigured daemon stays up, prints a setup code, serves the UI + setup routes, and continues in-process when setup completes.
**Independent test**: start with an empty config path → `/auth/status` says `setup_required`, `/api/*` → 503, `/setup/unlock` needs the code; writing a complete config ends setup mode.

- [X] T006 [US1] Create `putio-sync-api/src/setup.rs`: `SetupState` (session behind a `Mutex`, `Notify` for finish, `AuthLimiter`), `new_setup_code()`, `new_api_token()`, `setup_router(state)` with public `/health`, `/auth/status` (`setup_required: true`), UI assets, `/setup/status`, gated routes via a setup-auth middleware (code or API token, constant time, limiter), and a 503 fallback; export from `putio-sync-api/src/lib.rs`
- [X] T007 [US1] Add `setup_required: false` to the normal `/auth/status` response in `putio-sync-api/src/routes/system/auth.rs` (or wherever `auth_status` lives)
- [X] T008 [US1] Add `putio_sync_setup_required` gauge in `putio-sync-telemetry` (registration + setter) and update the telemetry metric-name contract test
- [X] T009 [US1] Create `putio-sync-cli/src/commands/setup_mode.rs`: `needs_setup(path) -> Option<(Config, Vec<SetupReason>)>` (local checks + one account call; 401 → `credentials_rejected`, network error → no setup), `run(...)` that prints the code with `eprintln!`, binds the API address (retry on `AddrInUse` up to 5 s), serves `setup_router` with graceful shutdown on finish or when the 5 s config poll passes, logs enter/leave with reasons, sets the gauge; `listen = false` → console message and poll only
- [X] T010 [US1] Wire setup mode into `putio-sync-cli/src/commands/daemon.rs` before the normal startup (load config after setup, start telemetry only once)
- [X] T011 [US1] Tests in `putio-sync-api/tests/setup_routes.rs`: status public; 503 for `/api/status`; 401 without code; code and API token unlock; limiter 429 after 5 failures; `/health` in setup mode
- [X] T012 [US1] Test in `putio-sync-cli` (unit, in `setup_mode.rs`): `needs_setup` for missing config, invalid config, no token, rejected token (mock 401), unwritable folder, and a complete config (mock OK) → none

## Phase 4: User Story 2 — connect Put.io (P1)

**Goal**: link via device code (or paste) and show the account.
**Independent test**: with wiremock Put.io, `POST /setup/putio/link` returns a code, `GET` turns `linked` once the mock returns a token, and the account shows; paste path validates the token.

- [X] T013 [P] [US2] Create `putio-sync-client/src/oob.rs`: `request_code(app_id)`, `check_code(code)` with a base-URL constructor; wiremock tests for pending, linked and error
- [X] T014 [US2] Add routes `POST/GET /setup/putio/link`, `POST /setup/putio/token`, `GET /setup/putio/folders` in `putio-sync-api/src/routes/setup.rs` (Put.io base URL overridable in `SetupState` for tests; 10-minute expiry; first token wins)
- [X] T015 [US2] Tests in `putio-sync-api/tests/setup_routes.rs`: link flow, 409 without app ID, token paste OK and rejected, token never in any response

## Phase 5: User Story 3 — download folder (P1)

**Goal**: suggest and check a folder; block unusable ones.
**Independent test**: `POST /setup/folder/check` on writable, missing-but-creatable, and read-only dirs.

- [X] T016 [P] [US3] Add `FolderCheck` + `check_folder(path, putio_used)` to `putio-sync-core/src/setup.rs` (exists, is_dir, probe write, can_create via nearest parent, free/total via telemetry `disk_usage`, low-space and container warnings); unit tests
- [X] T017 [US3] Add `POST /setup/folder/check`, `PUT /setup/folder` in `putio-sync-api/src/routes/setup.rs`; tests in `setup_routes.rs`

## Phase 6: User Story 4 — what to download (P2)

**Goal**: account-wide preset and per-top-level-folder rules.
**Independent test**: `PUT /setup/download` with a preset and two folder rules, then finish → config has `[download] preset` and the rules.

- [X] T018 [US4] Add `SetupChoices` + `apply(&mut Config)` in `putio-sync-core/src/setup.rs` (token, folder, preset, top-level rules replacing only chosen folders, watch, metrics, api token); unit tests that unrelated settings are kept
- [X] T019 [US4] Add `PUT /setup/download` and the preset list in `GET /setup/session` in `putio-sync-api/src/routes/setup.rs`; tests

## Phase 7: User Story 5 — dry run, then start syncing (P2)

**Goal**: read-only preview with totals and samples; finish writes once and ends setup mode.
**Independent test**: mocked listing → report counts/bytes, samples largest first, already-present detection; finish writes 0600 config with API token and fires shutdown.

- [X] T020 [P] [US5] Create `putio-sync-download/src/dry_run.rs` (`DryRunReport`, `dry_run(client, &DownloadConfig, target, deadline)`) reusing the manager's listing helpers; tests with mockito/wiremock listings, partial on deadline
- [X] T021 [US5] Add `POST /setup/dry-run` and `POST /setup/finish` in `putio-sync-api/src/routes/setup.rs` (validate, apply, `Config::save`, create API token when none in file/env, fire `Notify`; second finish → 409); tests
- [X] T022 [US5] Create `putio-sync-api/ui/pages/setup.js` (steps: unlock, Put.io, folder, download, extras, review, finish; back navigation; code in `sessionStorage`) and styles in `putio-sync-api/ui/app.css`
- [X] T023 [US5] Boot into the wizard from `putio-sync-api/ui/app.js` when `/auth/status` says `setup_required`; after finish store the API token, wait until `setup_required` is false, go to `#/status`
- [X] T024 [US5] Embed `pages/setup.js` in the asset list (`putio-sync-api/src/` UI asset table) and add a UI asset test that it is served

## Phase 8: User Story 7 — CLI `putio-sync setup` (P2)

**Goal**: same flow in the terminal, headless-capable.
**Independent test**: non-interactive run with `PUTIO_SYNC_PUTIO_TOKEN`, `--download-dir`, `--yes` against a mock writes the config; missing required value fails before writing.

- [X] T025 [US7] Add `Setup` subcommand and flags to `putio-sync-cli/src/cli.rs` and dispatch in `putio-sync-cli/src/main.rs`
- [X] T026 [US7] Create `putio-sync-cli/src/commands/setup.rs` with a `Prompter` trait (terminal + scripted for tests), the steps, dry-run print, single save, API token printed once; tests with a scripted prompter and mock Put.io

## Phase 9: User Story 6 — extras (P3)

- [X] T027 [US6] Add `POST /setup/watch/check`, `PUT /setup/extras`, and `POST /setup/arr/{discover,preview,run}` (reusing 018 discovery/setup with a session arr draft) in `putio-sync-api/src/routes/setup.rs`; tests for watch + metrics saved at finish and arr preview reachable
- [X] T028 [US6] Extras step UI in `putio-sync-api/ui/pages/setup.js` (watch, Radarr/Sonarr via the setup routes, metrics)

## Phase 10: Polish

- [ ] T029 [P] Document `[putio] app_id` and `[download] preset` in `config.example.toml`
- [ ] T030 [P] README, INSTALLATION.md and snap README: first run now opens the wizard; `putio-sync setup`
- [ ] T031 CHANGELOG.md entry; remove 013 from ROADMAP.md
- [ ] T032 Merge `origin/main`, run `TMPDIR=<scratchpad> make ci`, report UI size

## Dependencies

- Phase 2 blocks all stories. US1 (T006–T012) blocks the route stories (T014+), which add to `routes/setup.rs` sequentially.
- T013, T016, T020 are independent of the router and can be done in parallel.
- UI tasks (T022–T024, T028) need the routes they call.
- CLI (T025–T026) needs T005, T016, T018, T020.

## Implementation Strategy

MVP = Phases 1–5 + finish (T021): a daemon that waits, links Put.io, checks the folder and starts syncing. Then presets, dry run UI, CLI, extras, docs.
