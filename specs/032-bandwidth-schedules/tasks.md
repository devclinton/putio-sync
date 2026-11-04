# Tasks: Bandwidth Schedules

**Input**: Design documents from `specs/032-bandwidth-schedules/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/

Tests come first in each task where behaviour changes (Constitution III).

## Phase 1: Setup

- [ ] T001 Add `pub mod bandwidth;` to `putio-sync-core/src/lib.rs` with an empty module file `putio-sync-core/src/bandwidth.rs`

## Phase 2: Foundational

- [ ] T002 In `putio-sync-core/src/bandwidth.rs`: `Limit` (`"unlimited"`, `"paused"`, a number; serde both ways; ordered `Paused < Rate(a) < Rate(b) < Unlimited`), `ScheduleEntry { days, start, end, limit }`, the strict parse to `Window` per research R5 ("`start` `00:00`–`23:59`, `end` `00:01`–`24:00`", "`start == end` is refused", "at most 50 entries", a numeric limit must be > 0) with the messages in `contracts/config.md`, and `validate(&DownloadConfig)`; unit tests for each message, every day form (`mon`, `monday`, `mon-fri`, `fri-mon`, `daily`, `weekdays`, `weekends`, commas, any case)
- [ ] T003 In `putio-sync-core/src/bandwidth.rs`: `Override { mode: pause|slow, until }`, `Policy::from_config`, `Policy::at(local, utc, override) -> Current { limit, source, entry }` (first entry wins, wrap past midnight names the start day, slow is `min(slow, schedule)`), `Policy::next_change` (window boundaries over 8 days plus the override end) and `warnings(&DownloadConfig)` (overlap, paused all week, below 64 KiB/s) with unit tests on fixed `NaiveDateTime`s, including Friday 22:00–06:00 at Saturday 03:00 and `24:00` ends
- [ ] T004 In `putio-sync-core/src/config.rs`: `[download] bandwidth_schedule: Vec<ScheduleEntry>` (default empty, skipped when empty) and `slow_limit_bytes_per_sec: u64` (default 1 048 576); `Config::validate` calls `bandwidth::validate`; refuse `bandwidth_limit_bytes_per_sec = 0` and `slow_limit_bytes_per_sec = 0`; fix every `DownloadConfig { .. }` literal in the workspace
- [ ] T005 [P] In `putio-sync-db/src/pipeline.rs`: `BANDWIDTH_OVERRIDE` key, `FlagsRepository::get_text`, `set_text`, `remove`; tests in `putio-sync-db/tests/pipeline.rs`
- [ ] T006 [P] In `putio-sync-telemetry/src/{lib,download}.rs`: `putio_sync_download_bandwidth_limit_bytes_per_second` (Gauge, starts `+Inf`) and `putio_sync_download_bandwidth_override` (IntGauge), setters and globals; add both to `putio-sync-telemetry/tests/contract.rs` with a sample test

**Checkpoint**: core logic tested on its own.

## Phase 3: User Story 1 - Speed limits by time of day and day of week (P1) 🎯 MVP

**Goal**: one shared limiter that follows the schedule and applies to running downloads.

**Independent Test**: limiter tests with an injected clock; total across two consumers.

- [ ] T007 [US1] Rework `putio-sync-download/src/rate_limiter.rs`: shared `RateLimiter` (`Clone`, `&self consume`), token bucket per research R2, holding a `Policy`, an optional `Override` and a clock; `new(bps)` keeps a fixed default; `set_policy`, `set_override`, `current`, `status`, `is_paused`; recompute at most once a second and on change, setting the gauges and logging `limit`/`source`; tests: total of two concurrent consumers, a lower limit applies to a waiting consumer within ~1 s, paused blocks until resumed, unlimited returns at once
- [ ] T008 [US1] `putio-sync-download/src/stream.rs`: `stream_to_temp` and `stream_download` take `Option<&RateLimiter>` instead of `bandwidth_limit_bytes_per_sec`; update the stream tests
- [ ] T009 [US1] `putio-sync-download/src/manager.rs`: `DownloadConfig` gains `bandwidth_schedule` and `slow_limit_bytes_per_sec` (from `[download]`); the manager owns one `RateLimiter` built from the policy, passes it to `stream_to_temp`, swaps the policy in `update_config`, and `process_queue` claims nothing while paused; `WorkerStats.bandwidth_limit` is `None`; `pub fn bandwidth(&self) -> &RateLimiter`
- [ ] T010 [US1] Settings: `putio-sync-api/src/routes/system/settings.rs` `DownloadSettings` gains `bandwidth_schedule` and `slow_limit_bytes_per_sec` (view, apply, `DOWNLOAD_LIVE`); route test in `putio-sync-api/tests/settings.rs` that a bad entry is refused with the entry and field and a good one is saved and live
- [ ] T011 [US1] `putio-sync-api/src/routes/local/workers.rs`: per-worker `bandwidth_limit_bps` null, `total_bandwidth_limit_bps` the current limit (null unlimited, 0 paused)

**Checkpoint**: schedules work from the config file and Settings API.

## Phase 4: User Story 2 - Pause or slow for an hour (P2)

- [ ] T012 [US2] `putio-sync-download/src/manager.rs`: `set_bandwidth_override(mode, minutes)` ("1 to 1440 minutes"), `clear_bandwidth_override()`, `restore_bandwidth_override()` (expired ignored), persisted as JSON in the `bandwidth_override` flag when there is a database; tests in `putio-sync-download/tests/bandwidth.rs`
- [ ] T013 [US2] Call `restore_bandwidth_override()` where the daemon and `server` build their manager (`putio-sync-daemon/src/daemon.rs`, `putio-sync-cli/src/commands/server.rs` as needed)
- [ ] T014 [US2] New `putio-sync-api/src/routes/system/bandwidth.rs`: `GET /bandwidth`, `PUT /bandwidth/override` (`{ mode, minutes = 60 }`, 400 for bad values), `DELETE /bandwidth/override`, registered in `putio-sync-api/src/lib.rs`; tests in `putio-sync-api/tests/bandwidth.rs` including 503 in setup mode

## Phase 5: User Story 3 - See the current limit and why (P2)

- [ ] T015 [US3] `putio-sync-api/src/routes/system/status.rs`: `bandwidth` section (the `GET /bandwidth` body); test in `putio-sync-api/tests/bandwidth.rs`
- [ ] T016 [US3] New `putio-sync-api/ui/bandwidth.js`: `rate()` formatting (MB/s, Unlimited, Paused), the Status card (limit, source, next change, daemon time and offset, Pause 1 h / Slow 1 h / Resume schedule buttons), wired into `putio-sync-api/ui/pages/status.js`; api helpers in `putio-sync-api/ui/api.js`

## Phase 6: User Story 4 - Watch it from monitoring (P3)

- [ ] T017 [US4] Gauges are set by the limiter (T007); a download test checks the limit gauge reads 0 while paused and the override gauge 1 while an override is in force

## Phase 7: User Story 5 - Mistakes are caught before they bite (P3)

- [ ] T018 [US5] `putio-sync-core/src/assistant.rs` `risks_in`: `bandwidth_overlap`, `bandwidth_paused_week`, `bandwidth_low` (Level::Warning) from `bandwidth::warnings`; unit test
- [ ] T019 [US5] Doctor: `CheckId::Bandwidth` ("bandwidth", "Bandwidth limit", Area::Sync) in `putio-sync-doctor/src/report.rs`, dispatch in `checks/mod.rs`, check in `checks/sync.rs` (limit and source from config plus stored override, time zone offset; Warning while paused by an override); update the `ALL` count test in `putio-sync-doctor/tests/run.rs` and any check lists in the UI or README
- [ ] T020 [US5] Settings UI: a Bandwidth schedule editor (rows of days, start, end, limit in MB/s or Unlimited/Paused, add, move, remove) and the slow limit field in the Download group of `putio-sync-api/ui/pages/settings.js` using helpers from `putio-sync-api/ui/bandwidth.js`; node test `putio-sync-api/tests/ui/bandwidth.test.mjs` for the MB/s conversions and row serialisation

## Phase 8: Polish

- [ ] T021 [P] README (schedule section with the daemon-local-time note and Docker `TZ`, HTTP API table, metrics table, doctor checks), `config.example.toml`, `example-config.toml`
- [ ] T022 [P] `CHANGELOG.md` Unreleased: Added (schedules, quick actions, Status card, gauges, doctor check), Changed (the limit is now the total, applied to running downloads), Fixed (`/local/workers` per-worker share)
- [ ] T023 Remove 032 from `ROADMAP.md`; merge `origin/main`; `TMPDIR=<scratch> make ci`; push

## Dependencies

T001 → T002 → T003 → T004 → (T005, T006) → T007 → T008 → T009 → T010, T011 → T012 → T013, T014 → T015 → T016 → T017 → T018, T019, T020 → T021, T022 → T023.

## Parallel examples

- T005 and T006 touch different crates.
- T018 and T019 touch core and doctor.
- T021 and T022 are docs.

## Implementation strategy

MVP is Phase 3: the shared limiter following the schedule. Then the quick actions and the
Status card, then monitoring and validation surfaces, then docs.
