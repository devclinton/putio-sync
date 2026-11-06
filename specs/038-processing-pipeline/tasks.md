# Tasks: Processing Pipeline and Its View

**Input**: The documents in `/specs/038-processing-pipeline/`:
[plan.md](plan.md), [spec.md](spec.md), [research.md](research.md),
[data-model.md](data-model.md), [contracts/api.md](contracts/api.md) and
[quickstart.md](quickstart.md).

**Base**: Start from 039 (`claude/039-pause-hold`), or from main after 039 merges.

**Tests**: Required (Constitution III). Each story writes its tests first.

## Phase 1: Setup

- [x] T001 Create the crate `putio-sync-pipeline`:
  - deps: `putio-sync-core`, `putio-sync-db`, `putio-sync-hooks`, `tokio`, `sqlx`, `serde`,
    `serde_json`, `tracing`, `prometheus`, `thiserror`;
  - dev-deps: `tempfile`, `wiremock`;
  - add it to the workspace `members` in `Cargo.toml`.
- [ ] T002 [P] Lay out the modules in `putio-sync-pipeline/src/lib.rs` (`model`, `store`,
  `plan`, `runner`, `staging`, `gate`). Add a crate doc comment naming feature 038.

## Phase 2: Foundational

- [x] T003 Add the tables to `putio-sync-db/src/schema.rs` and register them where the 039
  tables are registered:
  - `pipeline_items`, `pipeline_steps` and `pipeline_attempts`, with the exact columns in
    data-model.md;
  - an index on `pipeline_steps(item_key)`, and one on `pipeline_steps(state, next_attempt_at)`.
- [x] T004 [P] Write the stage, step and item types in `putio-sync-pipeline/src/model.rs`:
  - `Stage`, in this order: `pre_download`, `downloading`, `pre_processing`, `subtitles`,
    `translating`, `post_processing`, `notifications`;
  - `StepKind`: `hook`, `download`, `notify`;
  - `StepState`: `waiting`, `running`, `done`, `failed`, `skipped`, `interrupted`;
  - `ItemState`: `running`, `held`, `waiting_for_operator`, `done`, `done_with_warnings`,
    `failed`, `cancelled`, `skipped`;
  - `SkipReason`: `not_configured`, `category`, `operator`, `hook`;
  - DB string round-trip tests.
- [ ] T005 [P] Add the config to `putio-sync-core/src/config.rs`:
  - a `[pipeline]` section:
    - `concurrency`, default 2, "1..=16";
    - `optional_retries`, default `[60, 300, 600]`, "≤ 10 entries";
    - `staging`, `"auto" | "on" | "off"`, default `"auto"`;
  - in `putio-sync-core/src/hooks.rs`, `HookConfig` gains:
    - `stage: Option<HookStage>` (`pre_download`, `pre_processing`, `post_processing`);
    - `required` (default `true`);
    - `categories` (default empty, meaning all);
  - validation: `stage` set means `events` is empty, and `categories` entries are not empty;
  - unit tests for each rule;
  - document all of it in `config.example.toml`.
- [x] T006 [P] Add `trait PipelineGate { fn pending(&self, key: &str) -> Option<String>; }` and
  a `NoGate` impl in `putio-sync-core/src/pipeline_gate.rs`.
- [x] T007 Write the store in `putio-sync-pipeline/src/store.rs`:
  - insert an item with its planned steps;
  - get the timeline;
  - start an attempt and end it, keeping the output tail to "the last 200 lines or 64 KB";
  - mark running steps as interrupted at startup;
  - list due steps;
  - set the item state;
  - prune with 039's retention;
  - tests on an in-memory DB.
- [x] T008 Write the planner in `putio-sync-pipeline/src/plan.rs`. For an item and the config,
  it gives the ordered steps per stage:
  - the hooks of each stage, in config order, filtered by category (a filtered-out hook is
    skipped with reason `category`);
  - one `download` step and one `notify` step;
  - empty stages marked `not_configured`.

  `changes_files` is true for hooks and false for notify. Tests in the same file.

## Phase 3: User Story 1 – See where an item is (P1) 🎯 MVP

- [ ] T009 [P] [US1] Write `putio-sync-pipeline/tests/stages.rs`:
  - with no hooks, the timeline is download done, notify done, and the rest not configured;
  - stages run in the FR-001 order;
  - a restart while a step is running marks it interrupted and runs it again, while done
    steps are not run again (SC-005);
  - an item whose download failed has its later steps marked not reached;
  - the branch API: two branches in one stage run at the same time, and the stage is done
    when both are.
- [ ] T010 [US1] Write the `Pipeline` service in `putio-sync-pipeline/src/lib.rs`:
  - `on_tick(&download::Pipeline)` (research R1):
    - new keys become items;
    - an item whose NAS stage is done moves to `pre_processing`;
    - a failed download becomes `failed`;
  - a worker that takes due steps, under a semaphore of `concurrency` permits;
  - `configure(Config)`;
  - `startup()`, which marks running steps as interrupted.
- [ ] T011 [US1] Call `Pipeline::on_tick` from `pipeline_tick` in
  `putio-sync-daemon/src/daemon.rs`, after `record_finished`. Build the `Pipeline` at start
  and hand it to the API state.
- [ ] T012 [US1] Add `step` to the `/active` rows, the `processing` filter, and
  `GET /active/{key}/timeline` in `putio-sync-api/src/routes/active.rs`, following
  contracts/api.md. Add API tests in `putio-sync-api/tests/active.rs`.
- [ ] T013 [US1] Build the timeline panel in `putio-sync-api/ui/` (the Active page, which
  Recently finished rows link to):
  - stages run left to right, and stack on narrow screens;
  - skipped stages fold under "show skipped";
  - branches sit side by side;
  - each attempt expands to its output;
  - the current step label goes on the row;
  - Node tests next to the 039 Active tests.

## Phase 4: User Story 2 – My own steps (P1)

- [ ] T014 [P] [US2] Write `putio-sync-pipeline/tests/hooks.rs`:
  - the environment holds `PUTIO_SYNC_ITEM_KEY`, `_NAME`, `_CATEGORY`, `_SOURCE`, `_DIR`,
    `_STAGE` and `PUTIO_SYNC_ITEM_JSON`, and the JSON shape matches research R3;
  - exit 0 means done, exit 3 means failed with the output kept, and running past the timeout
    means failed (`timeout`);
  - exit 10 in pre_download skips the item, and anywhere else counts as done;
  - URL hook: 2xx means done, 204 with `{"action":"skip"}` means skip, and 500 means failed;
  - hooks in one stage run in order;
  - an event hook (no `stage`) is never run as a step.
- [ ] T015 [US2] Add `run_step(hook, &ItemPayload) -> StepOutcome` to
  `putio-sync-hooks/src/command.rs` and `putio-sync-hooks/src/webhook.rs`, reusing their
  timeout, env, header and output capture. The JSON is written to a temp file and removed
  after the run. `HooksService` must not fire hooks that have a `stage`.
- [ ] T016 [US2] Write the runner in `putio-sync-pipeline/src/runner.rs`. It runs a hook step,
  redacts the output through `putio_sync_core::log_redact`, and records the attempt. It logs
  start and end with `key`, `stage`, `step`, `attempt`, `result` and `duration_ms`, and
  updates the metrics from contracts/api.md.
- [ ] T017 [US2] Add the pre-download gate in `putio-sync-pipeline/src/lib.rs`. When a new
  item has pre-download hooks and no file of it has started, place a 039 hold with reason
  `pre-download`. Release the hold when the hooks pass. A hook skip ends the item as
  `skipped`, and its queued files are removed through the existing queue cancel.
- [ ] T018 [US2] Extend the hook editor in `putio-sync-api/ui/` and the settings route in
  `putio-sync-api/src/routes/system/settings.rs` (`apply_hooks`):
  - "Runs as" (event hook, or a stage), Required, Categories;
  - order within a stage;
  - tests.

## Phase 5: User Story 3 – Radarr and Sonarr import only finished items (P1)

- [ ] T019 [P] [US3] Write the tests in `putio-sync-arr/src/view.rs`: when the gate reports
  pending for a locally done item, `percentDone` is 0.99, `leftUntilDone` is
  `max(1, total/100)`, `status` is downloading and `errorString` is empty. When the gate
  clears, it reads 1.0 and done. With `NoGate`, the output is the same as today.
- [ ] T020 [P] [US3] Write `putio-sync-pipeline/tests/staging.rs`:
  - files go into `<category>/.putio-sync-staging/<item>/`;
  - the category listing never shows the item until one rename;
  - an existing target is renamed to `<item> (2)`;
  - a different `st_dev` is refused (use a tmpfs or a second temp volume when one is
    available; otherwise test the check function directly).
- [ ] T021 [US3] Write the `PipelineGate` impl in `putio-sync-pipeline/src/gate.rs`. It
  answers from an in-memory map that is refreshed on every step change, with no DB query per
  `torrent-get`. Pass it to `putio-sync-arr` (`view::torrent` and its service) in the daemon
  wiring.
- [ ] T022 [US3] Write staging in `putio-sync-pipeline/src/staging.rs` and in
  `putio-sync-download/src/pipeline.rs`:
  - give the target dir for watch-folder items when staging is in effect;
  - rename into place after the last file-changing step;
  - check `st_dev` at start and on config save, which a `[pipeline] staging` save refuses
    with a field error.
- [ ] T023 [US3] Make a hold (039) pause the pipeline between steps: the item becomes `held`.
  Releasing the hold returns it to its last state. Test this in
  `putio-sync-pipeline/tests/stages.rs`.

## Phase 6: User Story 4 – Retry, skip, give up (P2)

- [ ] T024 [P] [US4] Write `putio-sync-pipeline/tests/retry.rs`:
  - a failed required step moves the item to `waiting_for_operator`. Retry after a fix makes
    the item complete. Skip records `operator` and goes on;
  - an optional step that keeps failing, with `optional_retries = [1, 1, 1]` and paused tokio
    time, gives up after 3 retries. The item ends `done_with_warnings`, and one
    `pipeline.completed` event lists the step;
  - each attempt is listed on the timeline.
- [ ] T025 [US4] Write retry, skip and give-up in `putio-sync-pipeline/src/lib.rs`, using
  `next_attempt_at`. Emit the events `pipeline.step_failed` and `pipeline.completed` through
  the core events bus, and document them in `EVENTS.md`.
- [ ] T026 [US4] Add `POST /active/{key}/steps/{id}/retry` and `/skip` (202, 409, 404) in
  `putio-sync-api/src/routes/active.rs`, with tests. In the UI timeline, failed steps get
  Retry and Skip buttons, and Skip asks for confirmation.

## Phase 7: User Story 5 – Per category (P3)

- [ ] T027 [US5] Show the category skip reason ("not for this category") in the timeline. Add
  a planner hook point in `putio-sync-pipeline/src/plan.rs` that later step kinds (025, 026)
  use to opt out per category. Add tests.

## Phase 8: Polish

- [ ] T028 [P] Add the `GET`/`PUT /settings/pipeline` routes and the Settings › Pipeline group
  in `putio-sync-api/ui/`.
- [ ] T029 [P] Update `README.md` (Pipeline section), `EVENTS.md` (pipeline hooks: input, exit
  codes, the idempotency note) and `CHANGELOG.md` (Unreleased › Added).
- [ ] T030 Run `make ci`, and run quickstart §2–§5 locally.

## Dependencies

- Setup, then Foundational, then US1.
- US2 needs T010 and T016.
- US3 needs T010.
- US4 needs T016.
- US5 needs T008.

## Parallel examples

- In Foundational, T004, T005 and T006 can run in parallel.
- T009 with T014. T019 with T020.

## Implementation strategy

- The MVP is US1 + US2 + US3, which together make the pipeline useful and safe for the arrs.
- Then US4, then US5.
