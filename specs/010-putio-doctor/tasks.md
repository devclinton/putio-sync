---

description: "Task list for the Put.io Doctor (feature 010)"
---

# Tasks: Put.io Doctor

**Input**: Design documents from `specs/010-putio-doctor/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/

**Tests**: Required. Constitution III requires them, and so does the task brief. Put.io is
mocked with `mockito`, and nothing touches the live account.

**Organization**: Tasks are grouped by user story:

- US1: CLI report (P1)
- US2: UI and fixes (P1)
- US3: metrics and schedule (P2)
- US4: CLI fixes (P3)

## Format: `[ID] [P?] [Story] Description`

## Phase 1: Setup

- [X] T001 Create crate `putio-sync-doctor` (Cargo.toml depending on core, client, db, download, telemetry, tokio, serde, serde_json, chrono, thiserror, tracing, futures; dev: mockito, tempfile, tokio test-util) and add it to workspace `members` in Cargo.toml
- [X] T002 [P] Add `serde_ignored` to `putio-sync-core/Cargo.toml` (R3)
- [X] T003 [P] Add `DoctorConfig { enabled: bool = true, interval_minutes: u64 = 60 }` under `[doctor]` to `putio-sync-core/src/config.rs` (serde defaults, a missing section uses the defaults, validation: interval_minutes ≥ 5) and document it in `config.example.toml`

## Phase 2: Foundational (blocks every story)

- [X] T004 Report types in `putio-sync-doctor/src/report.rs`:
  - Status `ok|warning|problem|skipped` (metric values 0/1/2/-1);
  - Area, CheckId (17 ids in data-model order), Item, FixId (6 ids, with `destructive()`), FixInfo, CheckResult (items capped at 100, with `item_count` for the total), Report (`overall` = the worst non-skipped status), FixPreview, FixRequest, FixOutcome;
  - serde `snake_case`;
  - unit tests for the overall status and the item cap.
- [X] T005 [P] `PutioApiClient::probe_account()` in new `putio-sync-client/src/probe.rs`:
  - one `GET /account/info` with no retries;
  - returns `AccountProbe { result: Result<AccountInfo, ClientError>, http_status, elapsed, server_date: Option<DateTime<Utc>>, rate_limit: Option<(u64 limit, u64 remaining)> }`;
  - mockito tests: ok, 401, 429, headers present and absent.
- [X] T006 [P] `PutioApiClient::walk_files(concurrency, deadline)` in new `putio-sync-client/src/walk.rs`:
  - BFS from root over `list_files`, at most 4 listings at a time;
  - returns `FileWalk { files: Vec<WalkedFile{id,name,size,parent_id,path,is_folder,extension}>, complete: bool, folders_listed, errors }`;
  - stops at the deadline with `complete = false`;
  - mockito tests: a nested tree, a listing error, and a deadline.
- [X] T007 [P] Move the re-add logic into `PutioApiClient::readd_transfer(id) -> Result<Transfer, ReaddError{NoMagnet, Client, AddFailed{cancelled_id,name,magnet,source}}>` in `putio-sync-client/src/transfers.rs`. Make `putio-sync-api/src/handlers/transfers.rs::readd_transfer` call it and map the errors exactly as before. The existing `putio-sync-api/tests/transfer_readd.rs` must still pass unchanged.
- [X] T008 [P] DB queries, with tests on a temp DB:
  - `Database::quick_check() -> Vec<String>` (`PRAGMA quick_check`; `["ok"]` means healthy) in `putio-sync-db/src/lib.rs`;
  - `QueueRepository::failed()` and `claimed()` (pending, downloading or paused rows) in `putio-sync-db/src/queue.rs`;
  - `DownloadRepository::failing(min_retries)` (rows with status failed or retry_count ≥ n) in `putio-sync-db/src/downloads.rs`;
  - `processed_torrent_paths()` in `putio-sync-db/src/torrents.rs`.
- [X] T009 [P] `config::lint(text: &str) -> Vec<ConfigWarning{key, message}>` in `putio-sync-core/src/config.rs`:
  - `serde_ignored` collects the ignored paths, each giving an "unknown key" warning;
  - an explicit `download.skip_existing = false` gives a "re-downloads files already on disk" warning;
  - tests: a clean file, a typo key, a nested typo, and the explicit skip_existing.
- [X] T010 `Doctor` service skeleton in `putio-sync-doctor/src/lib.rs`:
  - `Doctor::new(config, client, Option<db>, cleanup, Option<stall_tracker>)` and `configure(&Config)`;
  - `run()` is single-flight: a waiter returns the report that finished while it waited (R7);
  - `latest()`;
  - `run()` runs the checks with a 5-minute deadline;
  - checks are dispatched in `putio-sync-doctor/src/checks/mod.rs`, with one shared walk per run and the probe passed to the auth, storage and net checks;
  - a check that panics or errors becomes a Problem "could not complete: …".

**Checkpoint**: The crate builds and the report types and building blocks are tested.

## Phase 3: User Story 1 — Run the doctor from the command line (P1) 🎯 MVP

**Goal**: `putio-sync doctor [--json]` prints every check, with exit codes 0, 1, 2 or 3.

**Independent Test**: Seed faults in a temp config, DB and download dir, with Put.io mocked.
Every check returns the expected status, and the JSON and the exit code match
contracts/cli.md.

### Tests and implementation (write each check's failing test first)

- [X] T011 [P] [US1] Put.io checks in `putio-sync-doctor/src/checks/putio.rs`, each with mockito tests for the OK and fault cases:
  - `auth`: Problem on 401 or a missing token, plus a login hint. Every other Put.io check is Skipped when auth fails.
  - `storage`: Warning at 90% used or more, Problem at 98% or more.
  - `transfers`: errored transfers, each item with fix `transfers.retry`; stalled transfers from the tracker with a magnet, each with fix `transfers.readd`. With no tracker, the summary notes that stall detection needs the daemon.
  - `empty_folders`: the cleanup dry run.
  - `duplicates`: `(name, size)` groups with at least 2 members, of files ≥ 1 MiB.
  - `orphaned_torrents`: `.torrent` files whose stem matches no transfer name, case-insensitive.
  - `stuck_watch_files`: Skipped when watch is off. Otherwise `.torrent` or `.magnet` files in the watch paths with mtime > 1 h that are not in `processed_torrents`.
- [X] T012 [P] [US1] Sync checks in `putio-sync-doctor/src/checks/sync.rs`, with tests:
  - `dead_queue_rows`: rows missing from the walk that `get_file_details` confirms with a 404. Skipped when the walk is incomplete.
  - `failing_downloads`: failed queue rows, plus download rows with retry_count ≥ 3.
  - `orphan_partials`: `.X.downloading` files under the target directory that no claimed row maps to through `create_temp_path`, and whose mtime is > 1 h.
- [X] T013 [P] [US1] Local checks in `putio-sync-doctor/src/checks/local.rs`, with tests:
  - `download_dir`: create and remove a probe file. Problem if the directory is missing or read-only.
  - `disk_space`: `telemetry::gauges::disk_usage`. Warning when free space is < 10% or less than the pending queue's bytes; Problem when < 2%.
  - `database`: `quick_check`. Skipped when there is no DB.
  - `config`: `lint` on the config file text. Skipped when there is no file.
- [X] T014 [P] [US1] Connectivity checks in `putio-sync-doctor/src/checks/net.rs`, with tests:
  - `reachability`: Problem on a transport error; Warning when elapsed > 2 s, with the elapsed time injectable for tests.
  - `rate_limit`: Warning on a 429 or when remaining < 10%; OK "not reported" when there are no headers.
  - `clock_skew`: from the `Date` header. Warning at > 60 s, Problem at > 300 s, Skipped when there is no header.
- [X] T015 [US1] Full-run test in `putio-sync-doctor/tests/run.rs`:
  - a healthy mock gives all OK;
  - a seeded-fault mock gives the expected statuses;
  - with auth failing, the Put.io checks are Skipped and the local checks still run;
  - two concurrent `run()` calls make one set of Put.io calls (mock `expect(1)`).
- [X] T016 [US1] CLI subcommand `Doctor { json, fix, dry_run, yes }` in `putio-sync-cli/src/cli.rs` and dispatch in `putio-sync-cli/src/main.rs`.
- [X] T017 [US1] `putio-sync-cli/src/commands/doctor.rs` and its export in `commands/mod.rs`:
  - load the config and build the client and DB the way `clean_empty.rs` does;
  - print the readable report grouped by area: ✓, ! or ✗ per check, up to 10 items each, the summary line and the available fixes;
  - `--json` prints only the Report to stdout;
  - exit codes per contracts/cli.md (`std::process::exit` after flushing).
- [X] T018 [US1] CLI tests in `putio-sync-cli/tests/doctor.rs`, running the binary against a temp config with the Put.io base URL pointed at mockito:
  - `--json` parses and `overall` matches;
  - exit code 2 with a seeded problem, 0 when healthy.

  If the CLI cannot take a base URL override, test the `commands::doctor` functions directly.

**Checkpoint**: The MVP works from a terminal with no daemon running.

## Phase 4: User Story 2 — Doctor page in the web UI with fixes (P1)

**Goal**: Authenticated `/doctor` routes and a Doctor page with Run now, Retry and Requeue,
and preview-and-confirm for destructive fixes.

**Independent Test**: The API tests in `putio-sync-api/tests/doctor.rs`, plus a browser check
against `mock_ui`.

- [X] T019 [US2] Fixes in `putio-sync-doctor/src/fixes.rs`:
  - `preview(fix)` detects again and lists every item, with no cap;
  - `apply(fix, req)` validates the request (known fix, non-empty items, ≤ 1000, destructive requires `confirm`) and checks each item again;
  - `transfers.retry`: `retry_transfer`, only for transfers still in error.
  - `transfers.readd`: `readd_transfer`, only when the transfer is still stalled.
  - `dead_queue_rows.remove`: remove the row after confirming the 404 again.
  - `failing_downloads.requeue`: set the queue row to pending; download rows with no queue row are skipped ("not in the queue").
  - `orphan_partials.delete`: delete the file only if it still exists, still has the same size, is unclaimed and is older than 1 h.
  - `empty_folders.clean`: do a fresh dry run, and return the "list changed" error if it is not a subset of `items`; otherwise do the real run.
  - Each apply logs `fix`, `items`, `done`, `skipped` and `failed`, then re-runs the one check and updates that entry in `latest`.
- [X] T020 [US2] Tests for every fix in `putio-sync-doctor/tests/fixes.rs`:
  - a destructive fix without `confirm` is refused and makes no mock calls;
  - stale items are skipped;
  - the empty-folder subset rule holds;
  - a partial changed after the preview is not deleted.
- [X] T021 [US2] `AppState.doctor: Arc<Doctor>` built in `AppState::new`, plus `with_doctor()`, in `putio-sync-api/src/lib.rs`. Settings saves call `doctor.configure`, next to `cleanup.configure`.
- [X] T022 [US2] Routes in `putio-sync-api/src/routes/system/doctor.rs`, nested at `/doctor` inside the protected router:
  - `GET /doctor` returns 404 when nothing has run yet;
  - `POST /doctor/run`;
  - `POST /doctor/fixes/:fix/preview`;
  - `POST /doctor/fixes/:fix/apply`;
  - errors map to 400, 404 and 409 per contracts/http-api.md.
- [X] T023 [US2] API tests in `putio-sync-api/tests/doctor.rs`, using the `tests/common` helpers:
  - 401 without a token;
  - run, then GET returns the report;
  - preview and apply of `dead_queue_rows.remove`;
  - a destructive fix without confirm gives 400;
  - an unknown fix gives 404.

  Also extend `putio-sync-api/tests/secrets_sweep.rs` to the doctor endpoints.
- [X] T024 [US2] UI page `putio-sync-api/ui/pages/doctor.js` (≤ 9 KB), using `h`, `confirm`, `toast` and `toastError` from app.js and `get`/`post` from api.js:
  - a summary bar with counts, the time it ran and Run now;
  - sections per area, each check row with a status pill, title, summary and suggestion;
  - `<details>` holding the items, with "and N more" when `item_count` > 100;
  - fix buttons. A non-destructive fix applies the previewed items straight away. A destructive fix previews, then `confirm` lists up to 20 items plus "and N more", with the verb taken from the fix label.
  - After a fix, toast the done, skipped and failed counts, then refresh.
- [X] T025 [US2] Register the page: `doctor` in `PAGES` and `TITLES` in `putio-sync-api/ui/app.js`, the nav link in `putio-sync-api/ui/index.html`, and a small `.doctor` style block (status pills reuse the existing colour tokens) in `putio-sync-api/ui/app.css`. Update the asset test list in `putio-sync-api/tests/ui_assets.rs` if it enumerates pages.
- [X] T026 [US2] Seed doctor faults in `putio-sync-api/examples/mock_ui.rs`:
  - account info at 93% used;
  - an errored transfer;
  - a duplicate pair and an orphaned `.torrent` in the mocked tree;
  - a dead queue row (file id with a 404 mock);
  - a failed queue row;
  - an orphan partial file with an old mtime in the temp download dir;
  - an unknown key appended to the saved config.
- [X] T027 [US2] Browser check: run `mock_ui`, open Doctor, run through the quickstart §2 steps, and fix anything found.

**Checkpoint**: The UI and API work against the mock backend.

## Phase 5: User Story 3 — Alert on doctor results (P2)

**Goal**: Scheduled runs in the daemon and `putio_sync_doctor_*` gauges.

**Independent Test**: A daemon-level test runs the doctor once and reads the gauges with
`Telemetry::sample`.

- [X] T028 [P] [US3] `putio-sync-telemetry/src/doctor.rs`:
  - families `doctor_check_status{check}`, `doctor_check_items{check}`, `doctor_last_run_timestamp_seconds` and `doctor_last_run_duration_seconds`, registered in `lib.rs`;
  - `record(&[(check, status, items)], finished, duration)` and a global helper;
  - a unit test that samples them.
- [X] T029 [US3] `Doctor::run` records the gauges after every run (`putio-sync-doctor/src/lib.rs`). Fix re-runs update their check's gauges too.
- [X] T030 [US3] Daemon schedule in `putio-sync-daemon/src/daemon.rs`:
  - `with_doctor(Arc<Doctor>)`;
  - a task that waits 2 minutes, then runs every `interval_minutes` while `enabled`, reading the config at each tick;
  - a startup log line next to the other tasks.

  In `putio-sync-cli/src/commands/daemon.rs`, build one `Doctor` (with the daemon's stall tracker and cleanup) and pass it to the daemon and `AppState::with_doctor`.
- [X] T031 [US3] Test (done in `putio-sync-doctor/tests/schedule.rs`, since the loop lives in the doctor crate as `run_scheduled` and its test support is there) that one scheduled tick (paused tokio time) runs the doctor and sets `putio_sync_doctor_check_status{check="dead_queue_rows"}` to 1, and that `enabled = false` makes no run.

## Phase 6: User Story 4 — Apply fixes from the command line (P3)

**Goal**: `putio-sync doctor --fix ID [--dry-run] [--yes]`.

**Independent Test**: CLI tests show that `--dry-run` changes nothing, that answering no
changes nothing, and that `--yes` removes exactly the previewed rows.

- [X] T032 [US4] Fix mode in `putio-sync-cli/src/commands/doctor.rs`, per contracts/cli.md:
  - preview and print;
  - `--dry-run` stops there;
  - a destructive fix prompts `[y/N]` unless `--yes`;
  - apply, then print the outcome and the re-run check;
  - an unknown id lists the valid ids and exits 3;
  - `--json` with a destructive fix and no `--yes` exits 3.
- [X] T033 [US4] Tests in `putio-sync-cli/tests/doctor.rs`: `dead_queue_rows.remove` with `--dry-run` (the row remains), with `--yes` (the row is removed), and an unknown id (exit 3).

## Phase 7: Polish

- [X] T034 [P] README: a "Doctor" section covering the checks table, the CLI, the UI page, the `[doctor]` config and the metrics with an example alert. Also add `putio-sync-doctor` to the crate list in README.md.
- [X] T035 Run `TMPDIR=<scratchpad> make ci` (fmt, clippy -D warnings, tests, release build) and fix everything.
- [X] T036 Leave a hand-over note in the shared notes folder, covering the files touched for rebases: app.js, index.html, app.css, lib.rs, daemon.rs and the CLI files.

## Dependencies & Execution Order

- Setup (T001–T003) comes first, then Foundational (T004–T010).
- US1 (T011–T018) needs Foundational. T011–T014 can run in parallel.
- US2 needs the US1 checks (T011–T015), because fixes reuse their detection.
- US3 needs T010 only; it can run in parallel with US2.
- US4 needs T019 (the fixes) and T017.
- Polish comes last.

## Parallel Examples

- Foundational: T005, T006, T007, T008 and T009 touch different crates.
- US1: T011, T012, T013 and T014 are separate files in `checks/`.
- US3's T028 can run while US2's UI work (T024–T026) is in progress.

## Implementation Strategy

MVP = Setup, Foundational and US1 (the CLI report). Then US2 (the UI and fixes, where most of
the value is), then US3 (alerting), then US4. Each phase ends with `cargo test -p` for the
crates it touched. `make ci` runs at the end.
