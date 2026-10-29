---

description: "Task list for feature 042: Release Pre-check"
---

# Tasks: Release Pre-check

**Input**: Design documents from `specs/042-release-precheck/`

**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md),
[data-model.md](data-model.md), [contracts/api.md](contracts/api.md), [quickstart.md](quickstart.md)

**Tests**: included (Constitution III). Write each test first and see it fail. No test touches the
network: Put.io, the caches, indexer URLs and the *arr API are mocked with mockito, and the
.torrents are synthetic.

## Format: `[ID] [P?] [Story] Description`

## Phase 1: Setup

- [X] T001 Create the crate `putio-sync-precheck`:
  - `Cargo.toml` deps: putio-sync-core, putio-sync-client, putio-sync-telemetry, tokio, reqwest (workspace, no new
    features), serde, serde_json, sha1 = "0.10", thiserror, tracing. Dev deps: mockito, tokio
    (macros).
  - `src/lib.rs`, and add the crate to the workspace `members` in `Cargo.toml`.
  - Add it as a dependency of `putio-sync-arr`, `putio-sync-file-manager` and `putio-sync-api`.

## Phase 2: Foundational (blocks every story)

- [X] T002 [P] Test, then move the bencode reader and `from_torrent` from
  `putio-sync-arr/src/infohash.rs` into `putio-sync-precheck/src/metainfo.rs`.
  - Add `Metainfo { hash, name, files: Vec<ReleaseFile { path, size }>, private, tracker_hosts }`.
  - Read files for: single-file (`name` + `length`), multi-file (`files[]` with `length` and
    `path[]` joined with `/` under `name`), and v2 `file tree` when `files` is absent. Hybrid
    torrents use `files`.
  - Ignore padding files (`attr` contains `p`, or a path starting `.pad`).
  - Refuse more than 100,000 files, and keep the existing depth limit.
  - Tracker hosts come from `announce` and `announce-list` through
    `putio_sync_client::models::tracker_host`, host names only.
  - `putio-sync-arr/src/infohash.rs` keeps `from_magnet` and re-exports the hash from precheck.
    011's existing infohash tests must still pass unchanged.
  - Tests: `putio-sync-precheck/tests/metainfo.rs`, one fixture per layout above, plus
    truncated/garbage input returning `None`.
- [X] T003 [P] Test, then add the pure rule in `putio-sync-precheck/src/rule.rs`.
  - Types: `Policy { enabled, wanted: BTreeSet<String>, wanted_from: Option<String>, min_size,
    allow_archives }`, `Verdict::Pass { largest_wanted }` | `Verdict::Reject(Reason)`, and
    `Reason::NoWanted { found }` | `SmallWanted { largest, min }` | `Invalid` | `Gone`.
  - The extension is the last one, lowercased, as 009's `rules::extension` reads it.
  - Archive table: `7z, zip, rar, tar, gz, bz2, xz, zst, cab, iso, img`, plus split parts
    `\.r\d\d$`, `\.\d{3}$` and `\.part\d+\.rar$`. It is added to `wanted` only when
    `allow_archives` is on.
  - `Reason::message(stage)` produces the exact strings in contracts/api.md, for example
    `rejected before Put.io: no video file (found .exe, .txt)` and
    `rejected before Put.io: only a small video (12 MB, minimum 50 MB)`.
  - Tests in `tests/rule.rs`, covering:
    - rejected: `.exe` only, `.7z` only, `.r00` parts, a 12 MB `.mkv` alone, and
      `Episode.mkv.exe`;
    - passed: a 2 GB `.mkv` + `.exe`, a season pack with one big episode, and an audio policy
      with `.flac`;
    - with `allow_archives`: a `.rar` set passes.
- [X] T004 [P] Test, then add `[precheck]` to `putio-sync-core/src/config.rs`.
  - Struct: `PrecheckConfig { enabled=true, dry_run=false, min_size_mb=50, allow_archives=false,
    magnet_lookup=true, lookup_timeout_secs=10,
    caches=["https://itorrents.net/torrent/{HASH}.torrent"],
    categories: BTreeMap<String, PrecheckOverride>, watch: BTreeMap<String, PrecheckOverride> }`.
    `PrecheckOverride` has optional `enabled`, `min_size_mb` and `allow_archives`.
  - Validation:
    - `min_size_mb` must be 0..=100000;
    - `lookup_timeout_secs` must be 1..=30;
    - each cache must be http(s) and contain `{HASH}` or `{hash}`;
    - a `watch` key not in `[watch].paths` gives a warning, not an error.
  - Env overrides: `PUTIO_SYNC_PRECHECK_ENABLED` and `PUTIO_SYNC_PRECHECK_MAGNET_LOOKUP`.
  - Saves from the UI keep comments (the same `toml_edit` path as 009).
  - Test that the default `blocked_extensions` list is unchanged by this feature (FR-006): no
    `7z`, `zip` or `rar` added.
- [X] T005 [P] Test, then add `putio-sync-precheck/src/policy.rs`:
  `Policy::for_folder(cfg: &PrecheckConfig, rules: &RuleSet, putio_folder: &str,
  override_key: Key::Category(&str) | Key::Watch(&Path))`.
  - Wanted types: `RuleSet::resolve(putio_folder).filter.value.only` minus `blocked`, when
    `only` is `Some`. Set `wanted_from` to that filter's `from`. Otherwise use
    `rules::builtin("video")`'s list.
  - Overrides are applied last.
  - Tests:
    - no rules: video;
    - a `lidarr` folder with the `audio` preset: audio, `wanted_from = "lidarr"`;
    - a `no-binaries` block-list rule: still video;
    - a category override that turns the check off.
- [X] T006 [P] Test, then add `release_checks` and `release_reports` to
  `putio-sync-db/src/schema.rs`, with the exact columns, CHECKs and defaults from data-model.md:
  - `release_checks.source` CHECK IN
    (`torrent`,`url`,`magnet`,`watch_torrent`,`watch_magnet`);
  - `stage` CHECK IN (`before`,`putio`);
  - `state` CHECK IN (`passed`,`rejected`,`pending`);
  - `lookup` CHECK IN (`found`,`not_found`,`timeout`,`wrong_hash`,`error`,`off`);
  - `release_reports.kind` CHECK IN (`rejected`,`flagged_fake`,`flagged_not_fake`),
    `types`/`tracker_hosts` TEXT NOT NULL DEFAULT `'[]'`, and `note` at most 500 characters
    (checked in the repository);
  - indexes `(state) WHERE state='pending'`, `(checked_at)` and `(transfer_id)` on
    `release_checks`.

  Wire both into the migration in `putio-sync-db/src/lib.rs`. Add `ReleasesRepository` in
  `putio-sync-db/src/releases.rs` with:
  - `insert_check`, `resolve_check(id, state, reason, putio_bytes)`, `pending_checks()`,
    `has_pending()`, `pending_transfer_ids()`;
  - `insert_report`, `upsert_flag` (one active flag per target and kind; a second one replaces
    the note), `delete_flag(id) -> Deleted | NotFound | NotAFlag`, `flags_for(targets)`;
  - `reports(range)`, `release_stats(range)` returning the contracts/api.md `/stats/releases`
    shape;
  - *MVP note*: `upsert_flag`, `delete_flag` and `flags_for` landed with T023, and
    `release_stats` with T020, and `reports` with T022. A flag's release is the `arr_hash`,
    else the `file_id`, else the `hash`, else the `check_id` (for a rejection with none).
  - `prune(older_than)`.
- [X] T007 [P] Test, then add the metrics in `putio-sync-telemetry/src/precheck.rs`, with the
  exact families and bounded label enums from research R12:
  - `putio_sync_precheck_total{source,stage,outcome}`;
  - `putio_sync_precheck_avoided_bytes_total{stage}`;
  - `putio_sync_precheck_cache_lookups_total{cache,result}`;
  - `putio_sync_precheck_cache_lookup_seconds`;
  - `putio_sync_precheck_fallback_total{result}`;
  - `putio_sync_precheck_fallback_putio_bytes_total`;
  - `putio_sync_release_flags_total{kind}`.

  Add the families to `putio-sync-telemetry/tests/contract.rs`.
- [X] T008 Add `Prechecker` in `putio-sync-precheck/src/lib.rs` (depends on T002, T003, T005):
  - `check_bytes(bytes, policy) -> Checked { meta: Option<Metainfo>, verdict }`;
  - `Outcome::{Passed(Metainfo), Rejected(Option<Metainfo>, Reason), Unknown { hash, name }}`;
  - `dry_run` turns `Rejected` into `Passed` after logging and counting it as rejected, with
    `dry_run = true` in the log.

  Each check logs `hash`, `name`, `source`, `stage`, `outcome` and `reason`, never the bytes, URL
  or magnet.

**Checkpoint**: the rule and file lists work offline. No behaviour has changed yet.

## Phase 3: User Story 1 - A fake .torrent never reaches Put.io (P1) 🎯 MVP

**Goal**: .torrent contents and indexer URLs are checked locally. Fakes become failed *arr items
with no Put.io transfer, and are then blocklisted.

**Independent test**: against a mocked Put.io and *arr API, add three .torrents: an `.exe` only,
a `.7z` only, and a 2 GB `.mkv`. Only the third reaches `transfers/add`. The other two are
listed failed with the reason, and each is blocklisted once.

- [X] T009 [P] [US1] Test, then move `ArrService::fetch_torrent` into
  `putio-sync-precheck/src/fetch.rs` as `fetch(url) -> Fetched::Torrent(bytes) | Magnet(String)`.
  - Keep the 10 MB cap, http(s) only, and that the URL is never logged. Replace the 30 s timeout
    with a `deadline: Instant` argument: the add's 14 s budget (research R6, "Time budget").
  - Use a custom redirect policy: at most 5 hops, and stop when the next URL's scheme is
    `magnet`, then read `Location` from the 3xx response (research R6).
  - Tests in `tests/lookup.rs`:
    - 200 .torrent;
    - 302 → 200;
    - 302 → `magnet:?xt=…`;
    - 6 hops gives an error;
    - over 10 MB gives an error;
    - an HTML body is fetched (and the parse then fails in T010).
- [X] T010 [US1] Test first, in `putio-sync-arr/tests/service.rs`, then change `ArrService::add`
  in `putio-sync-arr/src/service.rs`:
  - resolve the policy for the category's Put.io folder (T005);
  - run the check for `Metainfo`, and for `Url` via T009;
  - on reject: insert the `arr_items` row with `state = failed`, `transfer_id = NULL`,
    `error = reason.message(Before)`, and `blocklist = pending` when `blocklist_server(category)`
    is set. Write `release_checks` (`state=rejected`, `stage=before`) and a `release_reports`
    row of kind `rejected` with the traits from FR-025;
  - return `AddOutcome::Added(item)` so `torrent-add` answers `torrent-added` (research R7);
  - on pass: proceed as today, and write `release_checks` with `state=passed`;
  - count the metrics from T007; avoided bytes = the sum of the file sizes.
  - Tests:
    - the three .torrents from the independent test;
    - the same through `Url` with a mocked indexer;
    - a category with the check off;
    - `dry_run`.
- [X] T011 [US1] Test, then add guards in `putio-sync-arr/src/service.rs` for a NULL
  `transfer_id`:
  - `refresh` skips the item;
  - `remove` skips the cancel and delete calls but still drops the rows;
  - `remove` of an item whose check is `pending` resolves that row as `rejected` with reason
    `Gone` (data-model "States"), so the sync gate is released.

  Also update the `transfer_id` note in `specs/011-arr-download-client/data-model.md` to "NULL for
  items rejected before Put.io (042)".
- [X] T012 [P] [US1] Test (it can be written alongside T009–T011, but only passes once T010 is
  done), through the router in `putio-sync-api/tests/arr_rpc.rs`:
  - `torrent-add` with a fake `metainfo` returns `torrent-added`;
  - `torrent-get` then shows `error: 3`, the exact `errorString`, and `isFinished: false`.

**Checkpoint**: the MVP. Most Sonarr/Radarr grabs (.torrent and URL) are covered.

## Phase 4: User Story 2 - Magnet links are checked using a few KB (P1)

**Goal**: look up a magnet's .torrent by hash in the configured caches within the time limit,
and check it. A miss falls back to Put.io.

**Independent test**: with mocked caches, a magnet whose cached .torrent is an `.exe` is
rejected, and a real one passes. A miss creates a Put.io transfer and a `pending` check row. A
wrong-hash body is ignored.

- [X] T013 [P] [US2] Test, then add `putio-sync-precheck/src/lookup.rs`: `lookup(hash, cfg) ->
  Found(Metainfo, cache_host) | Missed(LookupResult)`.
  - Try the caches in order within `lookup_timeout_secs` in total, with each body capped at
    2 MB.
  - Send `Accept-Encoding: identity`, a plain `User-Agent`, no cookies and no referer, and only
    the hash in the path (FR-010).
  - A body starting with the gzip magic or `<` counts as `error`.
  - A parsed hash that doesn't equal the magnet's counts as `wrong_hash`, and the next cache is
    tried.
  - `magnet_lookup = false` makes no request and returns `off`.
  - Record `cache_lookups_total{cache=host,result}` and `cache_lookup_seconds`.
  - Tests: found; 404 then found on the second cache; HTML; gzip; wrong hash; a slow cache past
    the limit (`timeout`, and the call returns within limit + 1 s); off.
  - For SC-002, each test asserts the mock's request count: one GET per cache tried and nothing
    else. A found lookup reads only the body it parses.
- [X] T014 [US2] Test, then handle magnets in `ArrService::add` (`Magnet`, `Url` starting with
  `magnet:`, and `Fetched::Magnet`):
  - found: check as in T010 with `source=magnet`, recording `lookup` and `lookup_cache`;
  - missed: add to Put.io as today, insert `release_checks` with `state=pending`, the `lookup`
    result and `transfer_id`, and count `precheck_total{outcome=unknown}`.
  - The lookup's limit is the smaller of `lookup_timeout_secs` and the time left before the add's
    deadline. Under 1 s left: skip it as `timeout` (research R6).
  - A re-add of a hash that was already rejected returns the existing failed item (011's
    duplicate rule), before any cache request.
  - Tests in `putio-sync-arr/tests/service.rs`:
    - a found fake, a found real release, and a miss;
    - the add answers within 15 s while every cache hangs (SC-003);
    - a URL whose indexer takes 12 s and then redirects to a magnet, with hanging caches: the
      add still answers within 15 s, as a fallback;
    - a rejected hash re-added makes no cache request.

## Phase 5: User Story 3 - Unchecked releases are caught before the NAS downloads them (P2)

**Goal**: a fallback is checked when Put.io shows its files, and the regular sync can't download
it first.

**Independent test**: a pending *arr magnet whose completed transfer holds only a `.7z`:
- it is cancelled on Put.io, its files are deleted, and it is failed and blocklisted;
- nothing is queued locally, even when a regular sync runs in between.

- [X] T015 [P] [US3] Test, then add the sync gate in `putio-sync-download/src/manager.rs` (tree
  enqueue loop, research R8):
  - when `releases().has_pending()` is true, load the transfers before the loop;
  - skip files whose `matching_transfer` id is in `pending_transfer_ids()`, with
    `decide(TreeDecision::Known)`, no queue add, and no rule decision recorded.
  - Test: one pending transfer and one normal transfer in the same tree; only the normal one's
    files are queued.
- [X] T016 [US3] Test, then add `check_on_putio(transfer, tree)` in
  `putio-sync-precheck/src/lib.rs`.
  - Input: a read-only file list from `PutioApiClient` (the walk in `putio-sync-client/src/walk.rs`
    for the transfer's `file_id`).
  - Returns a verdict with stage `putio`.
- [X] T017 [US3] Test, then in `ArrService::refresh` in `putio-sync-arr/src/service.rs`, run the
  check for items whose check is pending, when the transfer has a `file_id`, before
  `move_to_local`:
  - pass: resolve the row as `passed`, and continue as today;
  - reject:
    - cancel the transfer, delete the Put.io files, and remove the transfer (research R4);
    - fail the item with `reason.message(OnPutio)`, which ends with "(checked on Put.io)";
    - resolve the row as `rejected` with `putio_bytes = transfer.downloaded`;
    - write a `rejected` report;
    - count `fallback_total{result}`, `fallback_putio_bytes_total`, and `avoided_bytes{stage=putio}`
      (counted as NAS bytes only);
  - a transfer that is gone, in `ERROR`, or cancelled: resolve as `rejected` with reason `Gone`,
    not counted as fake. Test each of the three, and that `has_pending()` is then false;
  - `dry_run`: log and count, but don't cancel or delete;
  - Put.io delete failures are logged with the hash, name and transfer id, and retried next pass.
  - Test in `putio-sync-arr/tests/service.rs`.

## Phase 6: User Story 4 - Watch-folder files are checked too (P2)

**Goal**: `.torrent`/`.magnet` files in watch folders are checked before upload. Fakes are
renamed `.rejected`.

**Independent test**: a temporary watch folder holding a fake and a real `.torrent`. After one
scan, only the real one is uploaded, and the fake is `<name>.torrent.rejected`.

- [X] T018 [US4] Test, then in `process_torrent_file` in
  `putio-sync-file-manager/src/watcher.rs`, before upload:
  - `.torrent`: read the bytes, check them with `Policy::for_folder(cfg, rules,
    <upload folder path>, Key::Watch(dir))`;
  - `.magnet`: look it up as in T013;
  - reject:
    - don't upload; rename to `<file>.rejected`, which `find_torrent_files` must ignore;
    - mark the file processed (checksum) so it isn't retried;
    - write the check row and the report, and log a `warn!` with the path, hash and reason;
    - add a new `ProcessOutcome::Rejected(reason)`, handled in `scan_directories` with no
      archive/delete step;
  - pass: upload as today, and write the check row (`passed`);
  - magnet miss: upload, and write a `pending` row with the new `transfer_id`.
  - Pass the policy inputs (config, `RuleSet`, `Prechecker`, DB) through the existing
    constructor arguments rather than globals.
  - Test: `putio-sync-file-manager/tests/watch_precheck.rs`.
- [X] T019 [US4] Test, then add a check-on-Put.io pass at the start of `scan_directories`, for
  pending rows with `source = watch_magnet`:
  - list the transfers once;
  - for each completed one with a `file_id`, run T016;
  - on reject, cancel, delete and remove the transfer, resolve the row, and write the report;
  - a transfer that is gone, in `ERROR`, or cancelled resolves as `Gone`, as in T017;
  - the counting and `dry_run` handling are the same as T017.
  - Test with a mocked Put.io.

## Phase 7: User Story 5 - See what the check saved (P3)

**Goal**: the Stats page shows rejections, bytes avoided and magnet fallback rates.

**Independent test**: seeded checks in the mock UI. The tiles' totals and their splits by stage,
reason and cache match the seed.

- [X] T020 [P] [US5] Test, then add `GET /stats/releases` in
  `putio-sync-api/src/routes/system/stats.rs`, using `StatsQuery`'s range parsing and
  `ReleasesRepository::release_stats`.
  - The response is exactly the contracts/api.md shape.
  - Test: `putio-sync-api/tests/releases.rs`.
- [X] T021 [US5] In `putio-sync-api/ui/pages/stats.js`, add a "Rejected releases" tile (count,
  bytes, stage split, top reasons) and a "Magnet lookups" tile (found vs fell back, fallbacks that
  were fake and the bytes they cost on Put.io, per-cache table).
  - Use their own render functions, the page's range control, and text alternatives for any
    chart (006 FR-013).
  - Add the API helper in `putio-sync-api/ui/api.js` and seed a week of checks in
    `putio-sync-api/examples/mock_ui.rs`.
- [X] T022 [P] [US5] On rejected items in Queue (`putio-sync-api/ui/pages/queue.js`) and History
  (`history.js`), show the reason and the stage.
  - *Note*: rejected releases never become queue or history items, so this is a
    "Rejected releases" view in History, read from `GET /releases/reports`. It shows
    the time, stage, name, reason (with the types or sizes seen) and size. The Queue is
    unchanged. `GET /releases/reports` (JSON and CSV) and `ReleasesRepository::reports`
    landed here, ahead of T023.

## Phase 8: User Story 6 - Flag fakes the check missed, and wrong rejections (P3)

**Goal**: one-click "Fake" and "Not fake" flags with traits. "Fail and blocklist" for *arr
items. A CSV/JSON export.

**Independent test**: in the mock UI:
- flag a passed item "Fake" and a rejected one "Not fake";
- both reports hold the FR-025 traits;
- the blocklist runs only when chosen;
- the export lists both.

- [X] T023 [US6] Test, then add `putio-sync-api/src/routes/releases.rs`, mounted at `/releases`
  in `putio-sync-api/src/lib.rs`, with the bodies and status codes exactly as in
  contracts/api.md:
  - `POST /releases/flags`;
  - `DELETE /releases/flags/{id}`;
  - `GET /releases/flags`;
  - `GET /releases/reports?format=json|csv` (done with T022).

  Trait gathering (research R11) lives in `putio-sync-precheck/src/report.rs`:
  `traits_for(target)`, using the transfer from `transfers/list`, a read-only tree walk, or the
  local history rows, with missing fields left empty.

  "Fail and blocklist" calls a new `ArrService::flag_fake(hash)`: fail with "flagged fake by the
  operator", delete the Put.io files, and set `blocklist = pending`. This is the second recorded
  Constitution IV deviation (plan.md Complexity Tracking): log each delete with the hash, name
  and transfer id, and under `precheck.dry_run` record the flag and log the delete without
  making it or failing the item.

  Tests in `putio-sync-api/tests/releases.rs`:
  - each 400/404/409 case;
  - a note over 500 chars gives 400;
  - fail and blocklist under `dry_run`: the flag is saved, no Put.io delete call is made;
  - undo;
  - CSV flattening (`exe:1:1503238553;txt:1:120`);
  - extend `tests/secrets_sweep.rs` so no magnet, indexer URL or API key appears in reports,
    exports or responses.

  *Note*: two contract additions, both in contracts/api.md. A `{report_id}` target lets
  History's rejected rows be marked "Not fake" when the release never reached Put.io. `GET
  /releases/flags` also returns `arr_file_ids`, so the UI offers "also fail and blocklist" only
  on files an *arr item holds. A `file_id` an *arr item holds is flagged as that item.
- [X] T024 [US6] UI: add a "Flag" menu to rows on the Put.io page, Queue and History
  (`putio-sync-api/ui/pages/{putio,queue,history}.js`).
  - The menu offers "Fake" with an optional note and, for *arr items, "also fail and blocklist",
    plus "Not fake" on rejected rows.
  - Flagged rows get a badge and an undo button. The undo dialog says a blocklist already sent
    is not undone.
  - Load the flags for visible rows with one `GET /releases/flags` per page render.
  - Add a reports summary and download buttons to the Stats page.
  - Add mock fixtures.

  *Note*: the controls live in `ui/flags.js` (`loadFlags`, `flagControl`). Rows get a "Flag"
  button rather than a menu, since each row has one choice: "Fake", or "Not fake" on rejected
  rows. The Queue polls, so it reloads flags only when its rows or a flag change. The Stats card
  downloads through `downloadReports()` in `ui/api.js`, because the export needs the token.

## Phase 9: Polish

- [X] T025 [P] Add a "Release check" card to `putio-sync-api/ui/pages/settings.js`, in its own
  render function, with the `precheck` block from `GET/PUT /settings`
  (`putio-sync-api/src/routes/system/settings.rs`):
  - global settings, including a `dry_run` toggle labelled "Check only (don't reject or delete)",
    the cache list, and per-category and per-watch overrides;
  - a read-only `effective` table showing where the wanted types come from (FR-020).
  - Test that a save keeps the file's comments.
  - Done as its own route, `GET/PUT /settings/precheck`, like the other groups. Watch rows have
    `wanted: null` (only Put.io can name the upload folder's rule). The card extends `form()`
    with an `extra` hook for the override tables.
- [X] T026 [P] In `putio-sync-daemon/src/daemon.rs`, prune both release tables at startup and
  daily (rows older than 400 days).
  - The first tick runs at startup, then every 24 hours.
- [X] T027 [P] Document `[precheck]` in `config.example.toml` (each key, the cache list, the
  privacy note, `dry_run`), and add a README section covering the rule, the sources, the metrics,
  the `/stats/releases` and `/releases` routes, and how to flag.
- [X] T028 [P] Add a `CHANGELOG.md` entry, and a ROADMAP.md note on the 042 entry if the roadmap
  lists it.
  - ROADMAP.md has no 042 entry, so only the CHANGELOG changed.
- [X] T029 Run `TMPDIR=<scratchpad> make ci` and fix everything. Run the quickstart's mock UI
  steps 1–4.

## Dependencies

- T001 → everything.
- T002–T007 run in parallel. T008 needs T002, T003 and T005. All of Phase 2 blocks the stories.
- US1 (T009–T012) is the MVP.
- US2 (T013–T014) needs T010's add path.
- US3:
  - T015 can start after Phase 2;
  - T016 and T017 need US2, which is what creates pending rows.
- US4: T018 needs T013 for magnets. T019 needs T016.
- US5 needs the rows from US1 and US2. T022 can start after US1.
- US6 needs Phase 2 and T010. The "fail and blocklist" part needs 011's `fail` path.
- Polish comes last. T025 can start after T004.

## Parallel examples

- Phase 2: T002, T003, T004, T006 and T007 together (different crates), then T005 and T008.
- US1: T009 alongside T011 and T012's test scaffolding.
- US2 and US3: T013 alongside T015.
- US5 and US6: T020 alongside T023's route tests. T022 alongside T021.
- Polish: T025, T026, T027 and T028 together.

## Implementation strategy

1. **MVP = US1.** .torrent and URL fakes from Radarr/Sonarr never reach Put.io and get
   blocklisted. This is most grabs, and it needs no network beyond the indexer fetch already
   done today.
2. **US2 + US3** cover magnets: the cache lookup first, then the check on Put.io, so no fake
   reaches the NAS. Ship US3 with US2, because without the sync gate a fallback could be
   downloaded.
3. **US4** brings the same check to blackhole users.
4. **US5 and US6** give the data to judge the fallback rate and tune the rule.

To roll out on the NAS, deploy with `dry_run = true` for a few days, compare the counted
rejections with the fakes in the download folder, then turn it off.
