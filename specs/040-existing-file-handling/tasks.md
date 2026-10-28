# Tasks: What to do when a file already exists

**Input**: Design documents from `specs/040-existing-file-handling/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/config-and-cli.md,
contracts/api.md, quickstart.md

**Tests**: Required (Constitution III). They cover the pure decision table, mockito with small
payloads for the download manager (range, resume and failure mid-stream), the database
migrations, and the API routes. The UI is checked by hand through `mock_ui`.

## Phase 1: Setup

- [ ] T001 Add `crc = "3.4"` to `[dependencies]` in `putio-sync-download/Cargo.toml` with a comment `# CRC32 of local files against Put.io's crc32 (feature 040, research R4)`; check `Cargo.lock` gains no new package (`crc` 3.4.0 is already there through `sqlx`)

## Phase 2: Foundational (blocks every story)

- [ ] T002 In `putio-sync-core/src/config.rs`: `enum ExistingFile { Resume, Overwrite, KeepBoth }` with `#[serde(rename_all = "snake_case")]`, `#[serde(alias = "rename")]` on `KeepBoth`, `Default = Resume`, `From<bool>` (`true` → Resume, `false` → Overwrite), `as_str()`; `enum ExistingOutcome { Skipped, Restarted, Resumed, Overwritten, SavedAs, Refused }` (snake case, `as_str()`, `FromStr`)
- [ ] T003 In `putio-sync-core/src/config.rs`: `DownloadConfig.on_existing: Option<ExistingFile>` (`#[serde(default, skip_serializing_if = "Option::is_none")]`) and `DownloadConfig::on_existing() -> ExistingFile` = `on_existing.unwrap_or(skip_existing.into())`; at load, warn once when both `on_existing` and `skip_existing` are set ("on_existing wins"); the builder gets `on_existing(ExistingFile)`, and `skip_existing(bool)` stays
- [ ] T004 In `putio-sync-core/src/config.rs` save path (`edit_table` / `save_keeping_file_credentials`): writing `[download]` or a `[[download.rules]]` entry with `on_existing` set removes `skip_existing` from that table, keeping comments; tests: an old file with `skip_existing = false` loads as Overwrite; a UI save writes `on_existing = "overwrite"` and no `skip_existing`; `rename` loads as KeepBoth; an unknown value fails load and the message lists `resume, overwrite, keep_both`
- [ ] T005 In `putio-sync-core/src/rules.rs`: `FolderRule.on_existing: Option<ExistingFile>` (serde like the other options); `FolderRule::is_empty` checks it; `RuleSet` stores `on_existing: ExistingFile` (from `DownloadConfig::on_existing()`) in place of `skip_existing: bool`; `Effective.on_existing: Sourced<ExistingFile>` replaces `Effective.skip_existing`; `apply` sets it from `rule.on_existing.or(rule.skip_existing.map(Into::into))`; update the unit tests that used `skip_existing`, and add: nearest rule wins, a rule with only the old `skip_existing = false` gives Overwrite from that rule, no rule gives the global value with `from = None`
- [ ] T006 [P] In `putio-sync-db/src/schema.rs` and `putio-sync-db/src/lib.rs` `migrate`: four migrations in the existing `pragma_table_info` → `ALTER TABLE` style: `download_queue.save_as TEXT` (NULL), `download_queue.fixed_path INTEGER NOT NULL DEFAULT 0`, `download_history.local_path TEXT` (NULL), `download_history.outcome TEXT` (NULL); also put the columns in the `CREATE TABLE` strings for new databases; test that an old database gains them and a second `migrate` is a no-op
- [ ] T007 [P] In `putio-sync-db/src/queue.rs` and `putio-sync-db/src/models.rs`: the queue model gains `save_as: Option<String>` and `fixed_path: bool`; `add(...)` takes `fixed_path: bool`, and the upsert keeps 1 once set (`fixed_path = MAX(fixed_path, excluded.fixed_path)`); new `set_save_as(file_id, Option<&str>)`; `save_as` is cleared when the row completes or is removed; update every caller; tests: a second upsert with 0 keeps 1, `set_save_as` round-trips
- [ ] T008 [P] In `putio-sync-db/src/history.rs` and `putio-sync-db/src/models.rs`: `insert` and `insert_download_at` take `local_path: Option<&str>` and `outcome: Option<ExistingOutcome>`; `query` / `query_by_date` return both (old rows `None`); update every caller to pass `None` for now; tests
- [ ] T009 [P] In `putio-sync-telemetry/src/labels.rs`, `putio-sync-telemetry/src/download.rs`, `putio-sync-telemetry/src/lib.rs`: `label_enum!` `ExistingOutcome { Skipped => "skipped", Resumed => "resumed", Restarted => "restarted", Overwritten => "overwritten", SavedAs => "saved_as", Refused => "refused" }`; counter `putio_sync_existing_files_total{outcome}` with every series registered at zero; `telemetry::download::existing(ExistingOutcome)`; exposition test
- [ ] T010 In `putio-sync-download/src/manager.rs`: the manager's `DownloadConfig` replaces `skip_existing: bool` with `on_existing: ExistingFile` and adds `resume_enabled: bool` (default true); `From<&core DownloadConfig>` fills both (`on_existing()`, `resume_enabled`); `effective()` copies `on_existing` from the rule set; fix every use in `putio-sync-download`, `putio-sync-daemon/src/daemon.rs` (including the `download_settings_reach_the_manager` test), `putio-sync-api` and `putio-sync-cli`
- [ ] T011 In `putio-sync-download/src/manager.rs`: `DownloadFile` gains `crc32: Option<String>`, `save_as: Option<String>`, `fixed_path: bool`; the queue worker fills them from `file_info.crc32` and the queue row; the constructors in `putio-sync-api/src/routes/local/downloads.rs`, `putio-sync-cli/src/commands/download.rs`, `putio-sync-cli/src/commands/server.rs` and tests pass `None`, `None`, `false`; `ActiveDownload` gains `phase: &'static str` (`"downloading"` default)
- [ ] T012 In `putio-sync-download/src/files.rs`: `enum LocalState { Missing, Dir, File { size: u64 } }`, `enum SaveAsReason { KeepBoth, Larger, Checksum }`, `enum ExistingAction { Fresh, Skip, Resume { from: u64 }, Overwrite, SaveAs { name: String, reason: SaveAsReason }, Refuse }`, and `plan_existing(local, remote_size, remote_crc, local_crc: impl FnOnce() -> Option<String>, choice, resume_enabled, fixed_path) -> ExistingAction`, implementing the table in research.md R2 exactly; `local_crc` is called only for a same-size file when `remote_crc` is `Some`; with `fixed_path` every `SaveAs` becomes `Overwrite`; with `resume_enabled = false` a smaller file in Resume becomes `Overwrite`; returning `SaveAs` gives only the reason, and the name comes from T013
- [ ] T013 In `putio-sync-download/src/files.rs`: `numbered_name(name, n)` (`Show.S01E01.1080p.mkv`, 1 → `Show.S01E01.1080p (1).mkv`; `README`, 1 → `README (1)`; `.hidden`, 1 → `.hidden (1)`); `next_free_name(dir, name, reserved: &HashSet<PathBuf>)`: the lowest `n ≥ 1` with neither the file, its hidden `.downloading` file, nor a reserved path present; `crc32_file(path) -> io::Result<String>` using `crc::Crc::<u32, crc::Table<16>>::new(&crc::CRC_32_ISO_HDLC)`, 1 MiB reads on `spawn_blocking`, lowercase 8-digit hex, with a progress callback for bytes read; compare with Put.io's value case-insensitively
- [ ] T014 Unit tests in `putio-sync-download/src/files.rs`: one test per cell of the R2 table for all three choices, plus `fixed_path` and `resume_enabled = false` variants; `local_crc` not called when sizes differ or `remote_crc` is `None`; `numbered_name` cases; `next_free_name` skipping a taken name, a hidden file and a reserved path; `crc32_file` on `b"123456789"` = `cbf43926`

**Checkpoint**: the setting loads, resolves per folder and persists; the decision is tested; downloads do not use it yet.

## Phase 3: User Story 1 - A partial file is finished, a complete one is left alone (P1) 🎯 MVP

**Goal**: The default "resume" skips complete files (size, plus CRC32 when Put.io has one) and continues partial files at the real name.

**Independent test**: mockito Put.io and a temporary folder holding half of file A and all of file B; one sync completes A with one range request and makes no request for B.

- [ ] T015 [US1] In `putio-sync-download/src/manager.rs` `download_file`: replace the `skip_existing && target_path.exists()` check with `LocalState` from `symlink_metadata`, then `plan_existing` (`local_crc` = `crc32_file` with `ActiveDownload.phase = "checking"` while it runs), and carry out `Fresh` (today's path), `Skip` (write the `downloads` row `completed` with `target_path` and the retry count 0, no bytes, success result) and `Resume { from }` (rename the real file to `create_temp_path(&target_path)`, then `download_streaming_with_resume(file, cb, from)`)
- [ ] T016 [US1] In `putio-sync-download/src/manager.rs`: after a successful `Resume` and before the final rename, when `file.crc32` is `Some`, compute `crc32_file` of the hidden file; on a mismatch delete it and run `download_streaming` once (outcome `restarted`), and fail the file with "checksum mismatch after re-download" when the second copy also mismatches; move the final rename after the check so a mismatching file never reaches the real name (FR-006)
- [ ] T017 [US1] In `putio-sync-download/src/manager.rs`: honour `resume_enabled`; when off, skip `check_resume` (delete any hidden `.downloading` file and start over), and pass it to `plan_existing`
- [ ] T018 [US1] In `putio-sync-download/src/manager.rs`: `fn record_existing(&self, file, path, choice, outcome, detail)` logs one `info!` with structured fields `file_id`, `name`, `path`, `choice`, `outcome`, and `from_bytes` / `saved_as` / `reason` where set (`error!` for `Refused`), and calls `telemetry::download::existing`; call it for `Skip`, `Resume` and `Restarted`; nothing is logged for `Fresh`
- [ ] T019 [US1] In `putio-sync-cli/src/commands/download.rs`: pass the computed `resume_enabled` (`!no_resume && config.resume_enabled`) into the manager's `DownloadConfig` instead of only logging it
- [ ] T020 [US1] New `putio-sync-download/tests/existing_files.rs` (mockito, small payloads, temporary folder), the US1 scenarios: half a file at the real name → exactly one range request from that offset, and the file matches; a resumed file with a wrong `crc32` → one full download then success, outcome `restarted`; both copies wrong → failure and nothing at the real name; same size and matching `crc32` → no download request and a `completed` row; same size and no `crc32` → skipped; a hidden `.downloading` file still resumes (extend `tests/interrupted_resume.rs`); `resume_enabled = false` → full download for both a hidden and a real-name partial
- [ ] T021 [US1] In `putio-sync-download/tests/existing_files.rs`: US1 scenarios 5 and 6 (a larger local file, and a same-size file with a different `crc32`) → the old file is unchanged and the new file lands at `name (1).ext` (needs T023; mark these tests `#[ignore]` until US2 lands, or run US1 and US2 together)

**Checkpoint**: MVP. With default settings, truncated files are fixed and complete ones skipped.

## Phase 4: User Story 2 - The operator chooses overwrite or keep both (P1)

**Goal**: "overwrite" and "keep both" work end to end, and can be chosen in Settings, in the config file and on the CLI.

**Independent test**: a folder holding `Movie.mkv` and `Movie (1).mkv`; "keep both" saves `Movie (2).mkv`; "overwrite" makes `Movie.mkv` match Put.io.

- [ ] T022 [US2] In `putio-sync-download/src/manager.rs`: carry out `Overwrite`: download into the hidden file with no resume offset (delete a stale hidden file first), and rename over the real file only on success, so the old file stays until then; outcome `overwritten` (`reason = "fixed_path"` when it came from a fixed path)
- [ ] T023 [US2] In `putio-sync-download/src/manager.rs`: carry out `SaveAs`: if `file.save_as` is set and is a single path segment, use it; otherwise pick `next_free_name` with the manager's reserved set, and store it with `queue.set_save_as(file_id, Some(name))` before the first byte; download to `<folder>/<name>` through its own hidden file (resuming it after an interruption); write `downloads.local_path` as the numbered path; outcome `saved_as` with `saved_as` and `reason`
- [ ] T024 [US2] In `putio-sync-download/src/manager.rs`: a manager-wide `Mutex<HashSet<PathBuf>>` of target paths in use; take the target path before `plan_existing` and release it on every exit (a guard); a second worker for the same path waits (poll every 500 ms) for Resume and Overwrite, and treats the path as taken when picking a numbered name; carry out `Refuse` as a failed result with error "target path is a folder: <path>" and outcome `refused`
- [ ] T025 [US2] In `putio-sync-api/src/routes/system/settings.rs`: the download settings view replaces `skip_existing` with `on_existing: ExistingFile`; `DOWNLOAD_LIVE` lists `on_existing` and `resume_enabled` in place of `skip_existing`; a PUT body that still has `skip_existing` and no `on_existing` is mapped; an unknown value is 400 and changes nothing; route tests
- [ ] T026 [US2] In `putio-sync-api/ui/pages/settings.js`: replace the `skip_existing` row with `["on_existing", "When a file already exists", ..., [...]]`, labelled Resume / Overwrite / Keep both, with a help line per value that updates on change ("Keep both" shows `Movie (1).mkv`); change the "Resume" help to "Continue partial and interrupted downloads instead of starting over"
- [ ] T027 [US2] In `putio-sync-cli/src/cli.rs` and `putio-sync-cli/src/main.rs`: `--on-existing <resume|overwrite|keep-both>` (clap `ValueEnum` mapping to `ExistingFile`) on `Download` and `Daemon`, `conflicts_with = "skip_existing"`; `--skip-existing` maps to Resume; pass the value to `putio-sync-cli/src/commands/download.rs` and the daemon start; a test that both flags together give a usage error
- [ ] T028 [US2] Tests in `putio-sync-download/tests/existing_files.rs`: overwrite replaces and matches Put.io; overwrite failing mid-stream (mockito closes early) leaves the old file byte-identical; keep both with `(1)` taken gives `(2)` and touches neither old file; keep both with a complete local file skips with no request; keep both interrupted, then a new manager on the same database resumes into the same `name (1).ext`; several dots and no extension; two queue rows for one path never write at the same time; target path is a folder → refused, folder untouched; un-ignore the T021 tests

**Checkpoint**: all three choices work globally.

## Phase 5: User Story 3 - Per-folder choice with folder rules (P2)

**Goal**: Folder rules override the choice per folder, and the UI shows where the value comes from.

**Independent test**: global "resume", a rule for `backups/` set to "overwrite"; only the file under `backups/` is downloaded again.

- [ ] T029 [US3] In `putio-sync-api/src/routes/system/rules.rs`: `RuleBody.on_existing: Option<ExistingFile>` replaces `skip_existing` (a body with only `skip_existing` is mapped); `EffectiveView.on_existing: SourcedView<ExistingFile>`; the global summary reports `on_existing`; route tests for set, clear and the inherited source
- [ ] T030 [P] [US3] In `putio-sync-api/ui/rules.js` and `putio-sync-api/ui/pages/putio.js`: the rule editor's "Skip existing" control becomes a select of "Use global" / Resume / Overwrite / Keep both; the rule card and the Settings global summary show the value and its source ("this folder", "from Movies", "global")
- [ ] T031 [US3] Test in `putio-sync-download/tests/folder_rules.rs`: the US3 independent test, plus a rule with only the old `skip_existing = false` acting as Overwrite

## Phase 6: User Story 4 - Radarr/Sonarr items keep a predictable path (P2)

**Goal**: *arr item files never get a numbered name, so `<reported folder>/<item name>` holds the new download.

**Independent test**: "keep both" globally; an *arr item whose target exists with different content; it finishes with the new file at the output path and no numbered copy.

- [ ] T032 [US4] In `putio-sync-download/src/manager.rs`: `enqueue_tree_report(root_id, fixed_path: bool)` passes `fixed_path` to `queue.add`; `enqueue_tree` calls it with `false`; update callers and tests
- [ ] T033 [US4] In `putio-sync-arr/src/service.rs`: call `enqueue_tree_report(root, true)`
- [ ] T034 [US4] Tests: in `putio-sync-download/tests/existing_files.rs`, a `fixed_path` row with "keep both" and a different local file overwrites, with `reason = "fixed_path"` and no `(1)` file; the larger-file and checksum-mismatch cases in "resume" also overwrite; in `putio-sync-arr` tests, the item reports finished only after the overwrite completes, and its output path holds the new bytes

## Phase 7: User Story 5 - The operator can see what happened (P3)

**Goal**: Outcomes show in History, with the saved path, and in the Queue while a file is checked.

**Independent test**: one sync hits each outcome; the live log has one line per outcome; History shows the outcome and the saved path.

- [ ] T035 [US5] In `putio-sync-download/src/manager.rs`: every history insert passes `local_path` (the path actually written) and `outcome` (`None` for `Fresh`); `Skip` writes a history row with action `downloaded`, no duration and outcome `skipped`
- [ ] T036 [P] [US5] In `putio-sync-api/src/routes/system/history.rs`: `HistoryEntry` gains `local_path: Option<String>` and `outcome: Option<String>`; route test with an old row and a `saved_as` row
- [ ] T037 [P] [US5] In `putio-sync-api/src/routes/local/downloads.rs`: active downloads report `phase`
- [ ] T038 [P] [US5] In `putio-sync-api/ui/pages/history.js`: show the saved path under the name when it is not `<folder>/<filename>`, and a badge for `resumed`, `overwritten`, `saved_as`, `restarted`; old rows unchanged; works at 375 px and in dark mode
- [ ] T039 [P] [US5] In `putio-sync-api/ui/pages/queue.js`: an active download with `phase = "checking"` shows "Checking" in place of the speed and ETA
- [ ] T040 [US5] In `putio-sync-api/examples/mock_ui.rs`: seed history rows with outcomes `saved_as` (path `.../Movie (1).mkv`) and `resumed`, one active download in phase `checking`, and a config with `on_existing`
- [ ] T041 [US5] Test in `putio-sync-download/tests/existing_files.rs`: one sync that hits skipped, resumed, restarted, overwritten, saved_as and refused gives one counter increment per outcome and the matching `outcome` / `local_path` on each history row

## Phase 8: Polish & Cross-Cutting

- [ ] T042 [P] `config.example.toml`: add `on_existing` with the comment from contracts/config-and-cli.md; mark `skip_existing` as replaced (still read); fix the `resume_enabled` comment; update the folder-rule comment (`delete_after_download / on_existing`) and the examples that set `skip_existing`
- [ ] T043 [P] `README.md`: document the setting, the three choices, the checksum check, the Radarr/Sonarr exception, `--on-existing`, the `--no-resume` behaviour, and the new metric
- [ ] T044 [P] `CHANGELOG.md`: one user-facing entry for feature 040 (resume by default, overwrite or keep both; `resume_enabled` now works)
- [ ] T045 Run `TMPDIR=<scratchpad> make ci` and fix any failures (fmt, clippy `-D warnings`, tests, release build)
- [ ] T046 Run quickstart.md section 2 against `mock_ui` (Settings, rule card, History, Queue, 375 px, dark mode) and section 3

## Dependencies

- Phase 1 → Phase 2 → stories.
- US1 (Phase 3) needs T002–T018 but not US2; T021 needs T023 from US2.
- US2 needs Phase 2; it shares `download_file` with US1, so do US1 first (both P1).
- US3 needs Phase 2 (T005) and US2's settings work only for the UI summary; it can run after US1.
- US4 needs Phase 2 (T007) and US2's `SaveAs` / `Overwrite` (T022, T023).
- US5 needs Phase 2 (T008) and the outcomes from US1 and US2.
- Polish last.

## Parallel opportunities

- Phase 2: T006, T007, T008 and T009 touch different crates and can run together once T002 is done; T012–T014 (download `files.rs`) can run alongside them.
- US3: T030 (UI) alongside T029 (route).
- US5: T036, T037, T038 and T039 are in different files.
- Polish: T042, T043 and T044 together.

## Implementation strategy

1. MVP: Phases 1–3. The default becomes real resume, which fixes the main problem. Ship behind the existing default, since configs with `skip_existing = true` now resume.
2. Add US2 (overwrite and keep both, Settings, CLI), then US4 straight after, since keep both must not reach *arr items before it ships. Release US2 and US4 together.
3. US3 and US5 next; then polish and `make ci`.
