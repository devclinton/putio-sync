---
description: "Task list for 047 Log viewer in the UI"
---

# Tasks: Log Viewer in the UI

**Input**: Design documents from `specs/047-log-viewer/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/logs-api.md

**Tests**: Requested. FR-009 asks for redaction tests, and constitution III wants tests for
API behaviour.

## Format: `[ID] [P?] [Story] Description`

## Phase 1: Setup

- [X] T001 Add `futures-util = "0.3"` (already in Cargo.lock) to `putio-sync-api/Cargo.toml`, with a comment saying it is for the SSE stream (research R5)
- [X] T002 Add `buffer_lines: usize` to `LoggingConfig` in `putio-sync-core/src/logging.rs`: `#[serde(default = "default_buffer_lines")]`, default 2000, valid "100 to 50,000". Add `validate_filters` rejection outside the range, and document it in `config.example.toml` (`example-config.toml` has no `[logging]` section)

---

## Phase 2: Foundational

- [X] T003 [P] Create `putio-sync-telemetry/src/logs.rs` with `putio_sync_log_lines_dropped_total{reason}` (`evicted`, `lagged`, both created at zero) and `record_dropped(reason, n)`. Register it in `putio-sync-telemetry/src/lib.rs` and add a test that both series render
- [X] T004 [P] Create `putio-sync-core/src/log_redact.rs`: `set_secrets(Vec<String>)` (keep values ≥ 6 chars, longest first), `redact(&str) -> String` (known values → `********`, sensitive `key=`/`key:` values, `Bearer`/`Basic` credentials, then `redact::text` for URLs), and `redact_field(name, value)` (a sensitive name masks the whole value unless it is `true`/`false`/empty). Register it in `putio-sync-core/src/lib.rs`
- [X] T005 [P] Unit tests in `putio-sync-core/src/log_redact.rs` for each secret kind: Put.io token, API token, app secret (`PUTIO_CLIENT_SECRET`), arr password and API key, notification webhook URL, hook URL, header and env values, plugin secret, `Authorization: Bearer …`, `Cookie:`, `token=` in a query, a field named `api_key`, `Some("…")` Debug output. Also check that a plain `putio_sync_core::download` path and `has_token = true` are left alone
- [X] T006 Add `Config::secret_values(&self) -> Vec<String>` in `putio-sync-core/src/config.rs`, covering the sources listed in research R3 (config values plus env values), with a test that each source is included
- [X] T007 Create `putio-sync-core/src/log_capture.rs`:
  - `LogLine` (seq, time, level, target, message, fields, truncated; `Serialize`).
  - The global buffer: `set_capacity` (clamped to 100..=50000, drops the oldest), `push`
    (counts `evicted` through telemetry), `query(&LogQuery)`, `targets()`, `subscribe()`,
    `snapshot_after(seq)`, and `stats()` (last_seq, oldest_seq, capacity, evicted).
  - `CaptureLayer`, which visits fields, redacts, cuts to 4 KB, then pushes. It must not
    log.
  - Register the module in `putio-sync-core/src/lib.rs`.
- [X] T008 Unit tests in `putio-sync-core/src/log_capture.rs`:
  - the ring drops the oldest and counts it;
  - shrinking capacity drops lines;
  - each query filter works (after, level at or above, target prefix with `::`, `q` ignoring case);
  - a long line is cut and marked;
  - the layer with a local subscriber captures fields and redacts a registered secret;
  - a debug event is not captured under an info filter.
- [X] T009 Add the capture layer to every branch of `init_logging` in `putio-sync-core/src/logging.rs`, right after the reload filter, and call `log_capture::set_capacity(config.buffer_lines)` there
- [X] T010 Call `log_redact::set_secrets(config.secret_values())` before `init_logging` in `putio-sync-cli/src/main.rs`, and in the daemon/server reload poller callbacks call `log_capture::set_capacity(logging.buffer_lines)` (`putio-sync-cli/src/commands/daemon.rs`, `putio-sync-cli/src/commands/server.rs`)

**Checkpoint**: lines are captured and redacted in-process.

---

## Phase 3: User Story 1 - Read recent logs in the browser (P1) 🎯 MVP

**Goal**: a signed-in operator sees the kept lines, filtered.

**Independent Test**: `GET /logs` returns the lines, filters apply, and there's a 401 without a token.

- [ ] T011 [US1] Create `putio-sync-api/src/routes/system/logs.rs` with `GET /logs`, following contracts/logs-api.md (query: after, level, target, q, limit; response: lines, last_seq, oldest_seq, capacity, evicted, targets; 400 for a bad level). Register it in `routes/system/mod.rs` and nest it at `/logs` in the protected router in `putio-sync-api/src/lib.rs`
- [ ] T012 [US1] Refresh redaction secrets in `AppState::new` and after every settings save and import in `putio-sync-api/src/routes/system/settings.rs` (the shared `update` helper), using `Config::secret_values`
- [ ] T013 [US1] Create `putio-sync-api/tests/logs.rs`: 401 without a token, lines returned after logging, level, target and q filters, a bad level → 400, and 503 from the setup router (FR-012)
- [ ] T014 [US1] Create `putio-sync-api/ui/pages/logs.js`:
  - a list of lines (time, level, target, message, fields);
  - level select, module input with a datalist of `targets`, and search, all kept in the hash query;
  - an initial load from `GET /logs`.
  - Add the nav link in `putio-sync-api/ui/index.html`, add the page to PAGES/TITLES in `putio-sync-api/ui/app.js`, and add a few rules in `putio-sync-api/ui/app.css` (budget ≤ 8 KB JS, ≤ 1 KB CSS)

**Checkpoint**: MVP. The Logs page shows recent lines with filters.

---

## Phase 4: User Story 2 - Follow the logs live (P1)

**Independent Test**: the stream sends new lines, resumes from `after` without duplicates, and reports skipped lines.

- [ ] T015 [US2] Add `GET /logs/stream` to `putio-sync-api/src/routes/system/logs.rs`:
  - SSE over `futures_util::stream::unfold`;
  - first the backlog after `after` (default last_seq), then the broadcast;
  - on `Lagged`, backfill from the buffer, send `event: skipped` for lines gone, and count `lagged`;
  - same filters as `GET /logs`; keep-alive every 15 s.
- [ ] T016 [US2] Tests in `putio-sync-api/tests/logs.rs`: the stream sends the backlog after `after` and then a newly logged line; a stream from an evicted seq sends `skipped`
- [ ] T017 [US2] Live tail in `putio-sync-api/ui/pages/logs.js`:
  - `fetch` the stream with an auth header and parse `event:`/`data:` lines;
  - Pause/Resume with a waiting count, and auto-scroll unless scrolled up;
  - reconnect with `after=<last seq>` and a status note;
  - a skipped marker; restart on a filter change; abort on unmount.

---

## Phase 5: User Story 3 - Change the log level from the UI (P1)

**Independent Test**: set a module to debug for 30 minutes and its debug lines are kept; expire, and it is back to base.

- [X] T018 [US3] Override state in `putio-sync-core/src/logging.rs`:
  - `LevelState` (base, source, global, modules ≤ 20) and `Override { level, until }`;
  - composition into the effective directive (research R4) and `level_status() -> LevelStatus`;
  - `set_override(target, level, minutes)` (level is one of off/error/warn/info/debug/trace;
    target matches `[A-Za-z_][A-Za-z0-9_]*(::[A-Za-z_][A-Za-z0-9_]*)*`; minutes 1..=1440);
  - `clear_override(target)` and `expire(now)`;
  - `apply_level` now sets the base and keeps overrides;
  - one info line `Log level changed` with old/new/reason per change;
  - for timed overrides, spawn a tokio expiry task when a runtime is available.
- [X] T019 [US3] Unit tests in `putio-sync-core/src/logging.rs`:
  - composition (global replaces the bare base level, a module override replaces the same base target);
  - expiry with an explicit `now`;
  - the base change keeps overrides;
  - each validation error, and the 21st module refused;
  - `verbose` detection.
- [ ] T020 [US3] Add `GET /logs/level`, `PUT /logs/level` and `DELETE /logs/level?target=` to `putio-sync-api/src/routes/system/logs.rs`, following the contract (400 on validation, 409 when unavailable)
- [ ] T021 [US3] Tests in `putio-sync-api/tests/logs.rs`: an invalid target or level → 400; set, then GET shows it; clear → back to none
- [ ] T022 [US3] Level controls in `putio-sync-api/ui/pages/logs.js`:
  - the effective level and its source;
  - module (empty = everything), level, and "30 minutes" / "Keep (until restart)";
  - Apply, plus a list of overrides with their end time and a Revert button for each, and Revert all.

---

## Phase 6: User Story 4 - Secrets never reach the viewer (P1)

**Independent Test**: every configured secret kind logged at trace is absent from `/logs` and the stream.

- [ ] T023 [US4] Add a secrets test to `putio-sync-api/tests/logs.rs`:
  - configure a token, API token, arr password, hook header and URL, plugin secret and
    notification webhook through `test_app`'s config;
  - log lines holding each one in the message and in fields;
  - assert that none of the values appear in `GET /logs` or the stream body, and that
    `Authorization: Bearer x` is masked.

---

## Phase 7: User Story 5 - Copy, download and follow links (P2)

- [ ] T024 [US5] In `putio-sync-api/ui/pages/logs.js`:
  - Copy (the shown lines as text via `navigator.clipboard`, then a toast with the count);
  - Download (a Blob `putio-sync-logs-YYYYMMDD-HHMMSS.txt`) and Clear view;
  - links for `file_id` (app.put.io, new tab), `folder_id`/`parent_id` (`#/putio/<id>`),
    `transfer_id` (`#/transfers`), `hook`/`plugin` (`#/settings`) and `run_id` (`#/history`),
    only for digit ids or valid names.

---

## Phase 8: User Story 6 - Don't leave verbose logging on (P3)

- [ ] T025 [US6] Add `CheckId::LogLevel` ("log_level", "Log level", area Local) to `putio-sync-doctor/src/report.rs`, plus the check in `putio-sync-doctor/src/checks/local.rs`:
  - use the live `level_status()` when it is available, else config + `RUST_LOG`;
  - warn on debug/trace with the source, any end time, and a hint;
  - add a unit test for ok and warning.
- [X] T026 [US6] Check that the dropped-lines metric counts `evicted` from the buffer, with a test in `putio-sync-core/src/log_capture.rs` that reads `putio_sync_telemetry` output

---

## Phase 9: Polish

- [ ] T027 Settings › Logging: add `buffer_lines` to `LoggingSettings` (view and apply, 400 outside 100..=50000) in `putio-sync-api/src/models` / `routes/system/settings.rs`, and apply `set_capacity` on save. Add one field `["buffer_lines", "Lines kept for the Logs page", "100 to 50,000", "n", 100, 50000]` in `putio-sync-api/ui/pages/settings.js` (the only change to that file)
- [ ] T028 [P] Document the Logs page, `buffer_lines`, the `/logs` API and the metric in `README.md`, and add a `CHANGELOG.md` entry under Unreleased › Added
- [ ] T029 Run `cargo test -p putio-sync-api --test ui_assets` (384 KB cap), then check the page in the built-in browser against a local daemon (quickstart.md)
- [ ] T030 Merge `origin/main`, then run `TMPDIR=<scratch> CARGO_PROFILE_RELEASE_STRIP=none make ci`, push, and write the hand-over note

## Dependencies & Execution Order

- Phase 1 → Phase 2 → stories. T004/T003 are independent; T007 needs T003 and T004; T009 needs T007.
- US1 (T011–T014) needs Phase 2. US2 needs T011. US3's core (T018–T019) needs only Phase 2; its API part (T020) needs T011's module. US4 needs US1 and US2. US5 needs T014. US6's T025 needs T018.
- Polish comes last.

## Parallel Opportunities

- T003, T004 and T005 (telemetry, redaction and its tests) touch different files.
- T018/T019 (core logging) can go alongside T011–T014 (api/ui).

## Implementation Strategy

MVP = Phases 1 to 3 (read and filter). Then live tail, level changes, the secrets test, links,
doctor and polish. One person works through them in order. Each phase ends with `cargo test`
for the crates it touched.
