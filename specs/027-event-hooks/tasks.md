# Tasks: Event Hooks

**Input**: Design documents from `specs/027-event-hooks/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/

Tests come first in each task where behaviour changes (Constitution III).

## Phase 1: Setup

- [X] T001 Create the `putio-sync-hooks` crate (`Cargo.toml` with core, telemetry, tokio, reqwest, serde, serde_json, tracing, chrono; `src/lib.rs`), add it to the workspace `Cargo.toml` and to the crate lists in `Dockerfile`; add `uuid = { version = "1.6", features = ["v4"] }` to `putio-sync-core/Cargo.toml`

## Phase 2: Foundational

- [X] T002 [P] `putio-sync-core/src/redact.rs`: `pub fn url(&str) -> String` keeping scheme, host and port and the first path segment, then `…` when there is more ("no query string or user info"); an unparsable URL gives `<invalid url>`; unit tests including `https://user:pw@h/a/b?token=x` → `https://h/a/…`
- [X] T003 `putio-sync-core/src/events.rs` + `putio-sync-core/src/events/schema.json`: `SCHEMA_VERSION = 1`, `EventType` (`ALL`, `as_str`, `FromStr`), `Source`, `Event` with flattened `Payload` (`#[serde(tag = "type", content = "data")]`), payload structs per `contracts/events.md`, `emit(Payload) -> Arc<Event>` and `subscribe()` on a `OnceLock<broadcast::Sender<Arc<Event>>>` (capacity 4 096, send errors ignored), host per research R9, `error` fields cut to 4 KiB ("no event is larger than 64 KiB"); tests: wire form of each type, every sample validates against `schema.json` with the in-test validator (R8), `EventType::ALL` equals the schema's types, a subscriber receives an emitted event
- [X] T004 [P] `putio-sync-telemetry/src/hooks.rs` + `lib.rs`: `putio_sync_events_total{type}` (zero-initialised for the event type list), `putio_sync_hook_runs_total{hook,event,result}`, `putio_sync_hook_run_duration_seconds{hook}` (histogram, buckets 0.05–600 s), `putio_sync_hook_events_dropped_total{hook}`; global recorders `event_emitted(type)`, `hook_run(hook, event, result, duration)`, `hook_dropped(hook, n)`, `hook_configured(hook)`; add to `putio-sync-telemetry/tests/contract.rs`; `emit` in T003 calls `event_emitted`
- [X] T005 `putio-sync-core/src/hooks.rs` + `config.rs`: `HookConfig` per data-model ("`name` `[A-Za-z0-9_-]{1,40}`", "`timeout_secs` default 30, 1..=600", "at most 32 hooks", "at most 32 headers"), `validate` with every rule and message in `contracts/config.md`, `matches(EventType)` (`*` matches all but `hook.test`), `kind`, `target_label`, `MASK`, `mask`, `unmask(new, current)`; `Config.hooks: Vec<HookConfig>` (`#[serde(default, skip_serializing_if = "Vec::is_empty")]`) checked in `Config::validate`; fix any `Config { .. }` literals; unit tests per rule and for mask/unmask

**Checkpoint**: events, config and metrics tested on their own.

## Phase 3: User Story 1 - Run my script when a download lands (P1) 🎯 MVP

**Goal**: a command hook receives `download.finished`.

**Independent Test**: hooks crate test with a shell script that writes stdin to a file.

- [X] T006 [US1] `putio-sync-hooks/src/command.rs`: run per research R3 (absolute program, args, `env_clear` + `PATH`, `HOME`, `LANG`, `TZ`, `PUTIO_SYNC_EVENT_TYPE`, `PUTIO_SYNC_EVENT_ID`, `PUTIO_SYNC_HOOK_NAME`, hook env; JSON on stdin, `BrokenPipe` ignored; stdout/stderr "cut to 4 KiB each" with a `… (N bytes cut)` note; kill on timeout); returns `HookRun`; tests in `putio-sync-hooks/tests/command.rs` (stdin and env reach the script, a `PUTIO_SYNC_TOKEN` set in the test's env does not, exit 3 is a failure with code 3, a `sleep 30` with a 1 s timeout is a timeout and is killed, a missing program fails with "not found", 10 KiB of output is cut)
- [X] T007 [US1] `putio-sync-hooks/src/lib.rs` + `service.rs`: `HookRun`, `RunResult`, `HookStatus`; `HooksService::new(Vec<HookConfig>)`, `start()` (dispatcher from `events::subscribe()`, one worker per hook, queue of 1 000 dropping oldest with `hook_dropped`, `Lagged(n)` counted), `configure(Vec<HookConfig>)` (keeps queue and the 20-run history by name), `status()`, `wants(EventType)`, `test(name)`; each run logged with `hook`, `event_type`, `event_id`, `result`, `duration_ms`, `exit_code`/`http_status` and the output excerpts, and recorded with `hook_run`; tests: filter, a hook with an empty queue starts within 2 s (SC-001), order of ten events, a hanging hook does not block a second hook or `emit`, queue overflow drops oldest, reconfigure keeps history
- [X] T008 [US1] `putio-sync-download/src/manager.rs`: `download_file` emits `download.finished` (`file_id`, `name`, `relative_path`, `local_path`, `size_bytes`, `duration_seconds`, `average_bytes_per_second`, `resumed`) after the record is written and `download.failed` (`error`, `reason` from `fail_reason()`, `attempts`) on failure; not on skip or stop; test in `putio-sync-download/tests/` with a mockito download and `events::subscribe()` (filtering by file id, since the bus is shared)
- [X] T009 [US1] Wiring: `putio-sync-api/src/lib.rs` `AppState.hooks: Arc<HooksService>` (default unstarted from config in `new`) and `with_hooks`; `putio-sync-cli/src/commands/{daemon,server}.rs` build one service from the config, `start()` it, pass it to `AppState` (and the doctor in T021); add the crate to `putio-sync-api` and `putio-sync-cli` `Cargo.toml`

**Checkpoint**: a command hook fires on real downloads.

## Phase 4: User Story 2 - Call a webhook (P1)

- [X] T010 [US2] `putio-sync-hooks/src/webhook.rs` per research R4: POST, JSON, headers, `User-Agent: putio-sync/<version>`, per-attempt timeout, retries after 5 s and 30 s (configurable delays for tests) on connect/timeout/429/5xx, none on other 4xx, same-host redirects up to 3, errors built from status or error kind plus `redact::url`; response body cut to 4 KiB into `stdout`; tests in `putio-sync-hooks/tests/webhook.rs` with mockito: header and body arrive, 500 then 200 succeeds with attempts 2, 400 fails with attempts 1, a redirect to another host is not followed, a timeout, and no run field or log line holds the query token or the header value
- [X] T011 [US2] Hook into the service: `kind() == Url` runs the webhook; `target_label` in `HookStatus` and logs uses `redact::url`

## Phase 5: User Story 3 - Know what each event looks like (P1)

- [X] T012 [US3] `EVENTS.md`: envelope, delivery (stdin and env, webhook body and headers), versioning rule, one section per event type with a field table and a JSON example; a core test checks every `EventType` has an `EVENTS.md` heading and that each example parses and validates against `schema.json`
- [X] T013 [P] [US3] `putio-sync-api/src/routes/system/hooks.rs`: `GET /events/schema` (`application/schema+json`), registered in `putio-sync-api/src/lib.rs`; test in `putio-sync-api/tests/hooks.rs`

## Phase 6: User Story 4 - Set hooks up from Settings and test them (P2)

- [X] T014 [US4] `putio-sync-api/src/routes/system/hooks.rs`: `GET /hooks` (status list, contract `api.md`) and `POST /hooks/test` (`{name}`; 404 unknown; 409 while a test of that hook runs); tests in `putio-sync-api/tests/hooks.rs` with a script hook, including 503 in setup mode
- [ ] T015 [US4] Settings group `hooks` in `putio-sync-api/src/routes/system/settings.rs` (`HooksSettings { hooks }`, view masked with `MASK`, apply with `unmask`, live: `state.hooks.configure`), assistant `groups()` and `AFFECTS` in `routes/system/assistant.rs`, export masks and import refuses unresolved masks (`putio-sync-core/src/assistant.rs` `export_toml` / import path); extend `GROUPS` in `putio-sync-api/tests/settings_assistant.rs`; tests in `putio-sync-api/tests/settings.rs`: masked view, a save sending the mask keeps the value, a bad hook is refused naming hook and field, a save is live
- [ ] T016 [US4] `putio-sync-core/src/assistant.rs` risks `hook_plain_http`, `hook_disabled`, `hook_shared_dir`, `notifications_unused` per `contracts/config.md`; unit tests
- [ ] T017 [P] [US4] `putio-sync-api/ui/hooks.js` (DOM-free: `blankHook`, `targetLabel`, `eventChoices`, `runLabel(run)` like "200 OK in 40 ms" / "exit 3 in 1.2 s" / "timed out after 30 s", `toSettings`/`fromSettings` for the editor rows) with `tests/ui/hooks.test.mjs`; add to `putio-sync-api/src/ui.rs` assets
- [ ] T018 [US4] `putio-sync-api/ui/pages/settings.js`: Hooks group (`extra` editor): per hook name, enabled, kind (command/URL), program and arguments (one per line) or URL, headers or env as key/value rows with masked values shown as "set" and a Replace button, event checkboxes plus "All events", timeout; add and remove; each saved hook shows its last run and a "Send test event" button that calls `POST /hooks/test` and shows the result; keep the bundle under 384 KB

## Phase 7: User Story 5 - Transfers, sync runs, low storage (P2)

- [ ] T019 [US5] `putio-sync-download/src/transfer_events.rs`: `TransferClass`, `TransferEvents::observe` per research R6 (baseline without stored map; added → also finished/failed when already so; active → finished/failed; failed → finished); pure unit tests; `putio-sync-db/src/pipeline.rs` key `TRANSFER_EVENTS_SEEN`; `putio-sync-db/src/arr.rs` lookup of an arr item's app (radarr/sonarr) by Put.io transfer id; `manager.rs` `transfers_snapshot` runs the tracker on each fresh list, resolves origin (`transfer_origins`, arr, else `putio`), emits `transfer.*` and stores the map when it changed
- [ ] T020 [US5] `sync.completed` in `DownloadManager::sync_downloads` (`trigger` from an `AtomicBool` set by `request_sync`, `result`, `error`, `started_at`, `duration_seconds`, counts) with a test; `storage.low` edge in `putio-sync-storage/src/lib.rs` `refresh` (R7) with a mockito test (low → event, still low → none, recovered then low → event); `server` mode ticker in `putio-sync-cli/src/commands/server.rs`: every 30 s `transfers_snapshot` when `hooks.wants` any `transfer.*`, and `storage.refresh()` every 15 minutes

## Phase 8: User Story 6 - Doctor and monitoring (P3)

- [ ] T021 [US6] Doctor `Hooks` check: `CheckId::Hooks` ("hooks", Area::Local, title "Event hooks") in `putio-sync-doctor/src/report.rs` (ALL count 21), `putio-sync-doctor/src/checks/hooks.rs` (no hooks → pass "none set up"; missing or non-executable program → Problem naming the hook; a hook whose last run failed in the last 24 h → Warning with the error and time), `Doctor::with_hooks(Arc<HooksService>)`, dispatch in `checks/mod.rs`; wire in `putio-sync-cli/src/commands/daemon.rs` and `putio-sync-api/src/lib.rs`; unit tests; README doctor table row
- [ ] T022 [US6] Secrets sweep: extend `putio-sync-api/tests/secrets_sweep.rs` with a URL hook carrying `?token=` and an `Authorization` header and an env value; assert none appears in `/hooks`, `/settings/hooks`, `/settings/export`, `/doctor` or `/metrics`

## Phase 9: Polish

- [ ] T023 [P] Docs: README "Event hooks" section (config, events summary pointing at `EVENTS.md`, security: daemon user, clean environment, masking, retries, timeouts, queue), API, metrics, Settings and doctor tables; `config.example.toml` and `example-config.toml` commented `[[hooks]]` examples; `examples/hooks/on-download.py`; `CHANGELOG.md` Unreleased › Added
- [ ] T024 Remove 027 from `ROADMAP.md` (and the 027 mention in the pipeline notes stays only as a reference to hooks); merge `origin/main`; `TMPDIR=<scratch> make ci`; push

## Dependencies

- T001 → T002–T005 → US1 (T006–T009) → US2 (T010–T011) → US3–US6 in any order → Polish.
- T017 before T018. T009 before T014, T015 and T021.

## Parallel examples

- T002 and T004 touch different crates.
- T013 and T017 are independent of each other once T009 is done.

## Implementation Strategy

MVP is US1 (a command hook on `download.finished`). Then webhooks, the schema docs, the
UI, the other events, and the doctor, each committed on its own.
