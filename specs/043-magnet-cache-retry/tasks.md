---

description: "Task list for feature 043: Hold Magnets and Ask the Cache Again"
---

# Tasks: Hold Magnets and Ask the Cache Again

**Input**: Design documents from `specs/043-magnet-cache-retry/`

**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md),
[data-model.md](data-model.md), [contracts/api.md](contracts/api.md), [quickstart.md](quickstart.md).
042 must be merged into the base branch first; US4 also needs 042 US4 (T018–T019 there).

**Tests**: included (Constitution III, spec FR-019). Write each test first and see it fail. No
test touches the network: the caches and Put.io are mockito servers, the .torrents are
synthetic, and the hold pass takes `now` as an argument so no test sleeps.

## Format: `[ID] [P?] [Story] Description`

## Phase 1: Setup

- [ ] T001 Rebase the branch onto the base branch once 042 is merged, and check that
  `TMPDIR=<scratchpad> make ci` passes before any change.

## Phase 2: Foundational (blocks every story)

- [X] T002 [P] Test, then add three keys to `PrecheckConfig` in `putio-sync-core/src/config.rs`:
  - `hold_magnet_secs: u64` default `300`, range `0..=3600`, `0` = holding off;
  - `hold_retry_secs: u64` default `30`, range `10..=600`;
  - `first_lookup_secs: u64` default `3`, range `1..=lookup_timeout_secs`;
  - validation messages in 042's format, for example
    `precheck.first_lookup_secs must be between 1 and lookup_timeout_secs (10)`;
  - a helper `holding(&self) -> bool` (`enabled && magnet_lookup && hold_magnet_secs > 0`);
  - no env overrides (data-model.md).
  - Tests next to 042's `[precheck]` config tests: defaults, each range edge, and
    `first_lookup_secs > lookup_timeout_secs` rejected.
- [X] T003 [P] Test, then add the `held_magnets` table and `HeldRepository`:
  - `putio-sync-db/src/schema.rs`: `CREATE_HELD_MAGNETS_TABLE` exactly as data-model.md
    (`hash TEXT PRIMARY KEY`, `magnet TEXT NOT NULL`,
    `source TEXT NOT NULL CHECK(source IN ('magnet', 'url', 'watch_magnet'))`, `category`,
    `watch_folder`, `check_id`, `attempts INTEGER NOT NULL DEFAULT 1`, `last_lookup`,
    `next_at TIMESTAMP NOT NULL`, `until_at TIMESTAMP NOT NULL`,
    `held_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP`) and the `held_magnets_next` index; created in
    `putio-sync-db/src/lib.rs` `migrate` after the 042 tables.
  - `putio-sync-db/src/held.rs`: `NewHeld`, `Held`, and `insert`, `due(now, limit)` (oldest
    `next_at` first), `reschedule(hash, next_at, last_lookup)` (also `attempts += 1`), `get`,
    `delete`, `list`, `count`. Expose as `Database::held()`.
  - The `magnet` column is never in a `Debug` impl that gets logged: redact it in `Held`'s
    `Debug`.
  - Tests in `held.rs`: insert/due ordering, reschedule, delete, a duplicate hash refused.
- [X] T004 [P] Test, then add two settle methods to `putio-sync-db/src/releases.rs`:
  - `settle_held(id, state, lookup, lookup_cache, total_size, reason, transfer_id)`: sets the
    fields and `resolved_at`, only `WHERE id = ? AND state = 'pending'`;
  - `set_check_transfer(id, transfer_id)`: leaves the row `pending`, so 042's
    `pending_checks` picks it up.
  - Tests in the module's test block, next to `pending_checks_resolve_once`.
- [X] T005 [P] Add the metric families from contracts/api.md in
  `putio-sync-telemetry/src/lib.rs`:
  - `putio_sync_precheck_held_total{result}` with `result` in `found_passed`,
    `found_rejected`, `ran_out`, `removed`, `unchecked`;
  - `putio_sync_precheck_held` gauge;
  - `putio_sync_precheck_held_seconds{result}` histogram, buckets 30, 60, 120, 180, 300, 600;
  - an `ask` label (`first`, `retry`) on 042's `putio_sync_precheck_cache_lookups_total`,
    updating its existing call sites to pass `first`.
  - Add all of them to the telemetry contract test.
- [X] T006 Factor the tail of `ArrService::add` in `putio-sync-arr/src/service.rs` (category
  folder, add transfer, record) into `send_to_putio(item, magnet_or_torrent, check)`, with no
  behaviour change. 042's tests in `putio-sync-arr/tests/service.rs` must pass unchanged.

## Phase 3: User Story 1 - A magnet the cache doesn't have yet is still checked before Put.io (P1) 🎯 MVP

**Goal**: a magnet that misses the first ask is held, re-asked, and checked before Put.io.

**Independent test**: a mockito cache answering 404 for a hash first and a synthetic .torrent
later. The add makes no Put.io call; after `release_held(now + 30 s)` a fake is failed and
blocklisted with no Put.io call, and a real release is added to Put.io.

- [X] T007 [US1] (Built in a 043 section of `putio-sync-arr/tests/service.rs`, sharing its
  harness.) Write the tests in a new `putio-sync-arr/tests/hold.rs`, reusing the harness
  from `putio-sync-arr/tests/service.rs` (move shared helpers into `tests/common/mod.rs` if both
  files need them). Each must fail first:
  - holding on, first ask 404: `torrent-add` succeeds, no `transfers/add` request, one
    `held_magnets` row, one `pending` check with no transfer, the arr item `putio`/`HELD`;
  - re-ask serves a passing .torrent: one `transfers/add`, the check `passed` with
    `lookup = found` and the transfer id, the hold row gone;
  - re-ask serves a fake: the item `failed` with 042's reason, blocklist `pending`, a
    `release_reports` row, no `transfers/add`, the check `rejected`;
  - re-ask still 404: `attempts = 2`, `next_at` moved by `hold_retry_secs`;
  - `now >= until_at`: one `transfers/add`, the check stays `pending` with its `transfer_id`
    set;
  - a re-ask with the wrong hash counts as a miss;
  - `dry_run`: a fake found on re-ask goes to Put.io and is logged "would reject";
  - holding off (`hold_magnet_secs = 0`): 042's behaviour, no hold row.
- [X] T008 [US1] Add the `Held` branch to `ArrService::add` and `precheck` in
  `putio-sync-arr/src/service.rs`: on `Lookup::Missed` with `holding()` and the release eligible
  (FR-002: category checked, not private), insert the arr item (`transfer_id = None`,
  `state = Putio`, `putio_status = "HELD"`), the pending check (`stage = before`, no transfer,
  `lookup` = the first result) and the hold row (`next_at = now + hold_retry_secs`,
  `until_at = now + hold_magnet_secs`), then answer as for any add. A URL that redirected to a
  magnet stores the magnet, `source = url`.
- [X] T009 [US1] Add `ArrService::release_held(now)` in `putio-sync-arr/src/service.rs`,
  following the table in plan.md "Hold pass": take `held().due(now, …)` oldest first and
  handle one at a time; re-ask with `lookup(hash, lookup_timeout_secs)`; on found, run
  `check_meta` and either `send_to_putio` (pass or dry-run reject) or the 042 reject tail on the
  existing item (`set_state(Failed)`, blocklist, report); settle the check with `settle_held`; on
  `until_at`, `send_to_putio` and `set_check_transfer`. Count each outcome (T005) and log
  `hash`, `name`, `attempt`, `result`, never the magnet.
- [X] T010 [US1] A refused Put.io add during the hold pass fails the item with Put.io's message
  and deletes the hold row (FR-014). Add the test to `tests/hold.rs` first.
- [X] T011 [US1] Spawn the hold ticker in `putio-sync-api/src/lib.rs` where `ArrRpc` is built:
  a `tokio::time::interval(10 s)` loop calling `service.release_held(Utc::now())`, first tick
  immediately (covers FR-009's expired-during-restart case). Keep it to glue; the logic stays
  in `arr`.

## Phase 4: User Story 2 - The first ask doesn't waste the add's time (P1)

**Goal**: with holding on, the first ask is limited to `first_lookup_secs`.

**Independent test**: a mockito cache that stalls; the add returns in about 3 s with holding
on, and after the full `lookup_timeout_secs` with holding off.

- [X] T012 [US2] Tests in `putio-sync-arr/tests/hold.rs`, each failing first: a stalling cache
  with holding on returns within `first_lookup_secs` + 1 s and holds the magnet with
  `last_lookup = timeout`; holding off keeps 042's limit; a fast hit with holding on is checked
  at add time as in 042. Use small test values (`first_lookup_secs = 1`,
  `lookup_timeout_secs = 3`) so the suite stays fast.
- [X] T013 [US2] In `ArrService::precheck` (`putio-sync-arr/src/service.rs`), use
  `min(first_lookup_secs, lookup_timeout_secs, left)` as the lookup limit when `holding()`,
  else 042's limit. Record the lookup with `ask = first`; `release_held` uses `ask = retry`.

## Phase 5: User Story 3 - Held items are handled like any other item (P2)

**Goal**: held items show in the *arr queue and the UI, can be removed, and survive restarts.

**Independent test**: hold a magnet, build a new `ArrService` on the same database, and see the
hold continue; remove a held item through the Transmission API and see no Put.io call.

- [X] T014 [P] [US3] Test, then map `putio_status == "HELD"` in `putio-sync-arr/src/view.rs`
  to `status: 3`, `percentDone: 0`, `error: 0`, `errorString: ""`, `eta: -1`,
  `isFinished: false` (contracts/api.md). Unit test in the module.
- [X] T015 [US3] Tests in `putio-sync-arr/tests/hold.rs`, then the fix in `ArrService::remove`:
  removing a held item makes no Put.io call, deletes the hold row, closes the check `gone`, and
  counts `removed`. A second `torrent-add` of a held hash is 042's duplicate.
- [X] T016 [US3] Restart test in `putio-sync-arr/tests/hold.rs`: hold, drop the service, build a
  new one on the same database; `release_held` continues the schedule, and a hold whose
  `until_at` passed is sent to Put.io on the first pass.
- [X] T017 [US3] Test, then FR-013 in `release_held`: if precheck is now off, or the category is
  no longer checked, `send_to_putio` and settle the check `passed` with `reason = unchecked`,
  counted `unchecked`.
- [X] T018 [P] [US3] (Built as a `held` list on `GET /arr`, which has no item list; see
  contracts/api.md.) Add the `held` object (`since`, `until`, `attempts`, `last_lookup`) to each
  held item in `GET /arr` (`putio-sync-api/src/routes/arr.rs`), never the magnet. Route test.
- [X] T019 [P] [US3] In `putio-sync-api/ui/pages/queue.js`, show held items as "Waiting for file
  list (n min left)". Seed one in `putio-sync-api/examples/mock_ui.rs`.

## Phase 6: User Story 4 - Watch-folder magnets are held too (P3)

**Goal**: `.magnet` files that miss the first ask are held the same way.

**Independent test**: as 042 US4's watch test, with a cache that misses first and hits later.

**Blocked until 042 US4 (042 T018–T019) is merged.**

- [ ] T020 [US4] Tests in `putio-sync-file-manager/tests/watch_precheck.rs` (042's file), each
  failing first: a miss holds the file (`source = watch_magnet`, `watch_folder` set) and leaves
  it unprocessed; a later hit is checked and processed as 042 US4 does; a deleted file ends the
  hold with no Put.io call; the hold ending falls back as 042 US4 does.
- [ ] T021 [US4] Implement in `putio-sync-file-manager/src/watcher.rs`: hold on a miss, and
  re-ask the due watch holds at the start of `scan_directories`, sharing `HeldRepository`.

## Phase 7: User Story 5 - The operator can see whether holding pays off (P3)

**Goal**: Stats and metrics show how holds end and how long they take.

**Independent test**: drive holds to each outcome, then read `/stats/releases` and `/metrics`.

- [ ] T022 **Blocked until 042 T020 (`/stats/releases`) is built.** [US5] Test, then add the `held` block (`now`, `found`, `ran_out`, `removed`,
  `median_found_secs`) to `GET /stats/releases` in
  `putio-sync-api/src/routes/system/stats.rs`, read from `release_checks` (settled rows with a
  `stage = before` and a `checked_at`→`resolved_at` gap) and `held().count()`. Add the query to
  `putio-sync-db/src/releases.rs`.
- [ ] T023 **Blocked until 042 T021 (the stats tiles) is built.** [P] [US5] Add a "Held magnets" tile in `putio-sync-api/ui/pages/stats.js`, in its
  own render function next to 042's "Magnet lookups" tile.

## Phase 8: Polish

- [ ] T024 **Blocked until 042 T025 (the Release check card) is built.** [P] Add the three keys to the `precheck` block of `GET/PUT /settings`
  (`putio-sync-api/src/routes/system/settings.rs`) and to 042's "Release check" card in
  `putio-sync-api/ui/pages/settings.js`. Test that a save keeps the file's comments.
- [ ] T025 **Blocked until 042 T027 (the `[precheck]` docs) is built.** [P] Document the keys in `config.example.toml` and the README's pre-check section:
  what holding does, the delay it adds to magnet-only releases, and that it contacts nothing new.
- [X] T026 [P] Add a `CHANGELOG.md` entry, and mark 043 in `ROADMAP.md` as shipped (move it out
  of the queue).
- [ ] T027 Run `TMPDIR=<scratchpad> make ci` and fix everything. Run the quickstart's manual
  steps on the NAS after deploy.

## Dependencies

- T001 → everything (042 merged).
- T002–T005 run in parallel; T006 needs nothing but T001. All of Phase 2 blocks the stories.
- US1 (T007–T011) is the MVP. T008 needs T002, T003, T004. T009 needs T006 and T008.
- US2 (T012–T013) needs T008.
- US3: T014 can start after Phase 2; T015–T017 need T009; T018–T019 need T003.
- US4 needs US1 and 042 US4.
- US5 needs US1's outcomes. T023 needs T022.
- Polish comes last; T024 can start after T002.

## Parallel examples

- Phase 2: T002, T003, T004 and T005 together (different crates), alongside T006.
- US3: T014, T018 and T019 together while T015–T017 go in sequence.
- Polish: T024, T025 and T026 together.

## Implementation strategy

1. **MVP = US1 + US2.** Ship them together: holding without the shorter first ask still costs
   10 s per miss, and the shorter ask without holding loses the add's only chance.
2. **US3** before turning it on for real use, so held items are visible and removable.
3. **US5** gives the numbers that decide whether the deferred peer lookup is ever needed.
4. **US4** follows 042 US4.

Roll out with `dry_run = true` as 042 does, then watch `putio_sync_precheck_held_total` for a
week: `found_*` should be most of it (SC-002).
