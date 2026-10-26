---

description: "Task list for feature 011: Radarr/Sonarr Download Client"
---

# Tasks: Radarr/Sonarr Download Client

**Input**: Design documents from `specs/011-arr-download-client/`

**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md),
[data-model.md](data-model.md), [contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: included (Constitution III). Write each test first and see it fail.

## Format: `[ID] [P?] [Story] Description`

## Phase 1: Setup

- [X] T001 Create crate `putio-sync-arr` (`Cargo.toml` with deps client, db, download, core,
  telemetry, tokio, reqwest, serde, serde_json, base64, tracing, thiserror, `sha1 = "0.10"`; dev:
  mockito, tempfile), `src/lib.rs`, and add it to workspace `members` in `Cargo.toml`.

## Phase 2: Foundational (blocks every story)

- [X] T002 [P] Test then add `[arr]` to `putio-sync-core/src/config.rs`:
  - `ArrConfig { enabled=false, username="putio-sync", password: Option, servers: Vec<ArrServer> }`
    and `ArrServer { kind: radarr|sonarr, url, api_key: Option, category }`;
  - env overrides `PUTIO_SYNC_ARR_PASSWORD`, `PUTIO_SYNC_RADARR_API_KEY`,
    `PUTIO_SYNC_SONARR_API_KEY` (the first server of that kind);
  - `Debug` prints `<set>`/`<unset>`;
  - validation: http(s) URL, unique categories, category without `..` or empty segments and
    normalised without a leading or trailing `/`;
  - `save_keeping_file_credentials` keeps `[arr] password` and every `api_key` exactly as on
    disk.
- [X] T003 [P] Test then add `arr_items` and `arr_item_files` to `putio-sync-db/src/schema.rs`.
  Use the exact columns and CHECKs in data-model.md:
  - `state` CHECK IN (`putio`,`local`,`done`,`failed`);
  - `blocklist` DEFAULT `none` CHECK IN (`none`,`pending`,`done`,`gave_up`).

  Wire them into the migration in `putio-sync-db/src/lib.rs`. Add `ArrRepository` in
  `putio-sync-db/src/arr.rs` with:
  - `insert`, `get_by_hash`, `get_by_rowid`, `list`, `update_putio`, `set_files`, `files`,
    `set_state`, `set_blocklist`, `delete` (which also deletes the item's files);
  - `local_progress(hash) -> (completed_bytes, all_completed)`, a join of `arr_item_files` with
    `downloads`;
  - `counts_by_state`.
- [X] T004 [P] Test then add the Put.io calls in `putio-sync-client/src/files.rs`:
  - `create_folder(name, parent_id) -> File` (`POST /files/create-folder`);
  - `upload_bytes(filename, bytes, parent_id) -> UploadResponse`, with `upload_file` refactored
    onto it;
  - `add_transfer_torrent(filename, bytes, parent)` in `transfers.rs`.
- [X] T005 [P] Test then add metrics in `putio-sync-telemetry/src/arr.rs`:
  - `putio_sync_arr_rpc_requests_total{method,outcome}`;
  - `putio_sync_arr_items{state}`;
  - `putio_sync_arr_blocklist_total{outcome}`.

  Use the bounded label enums from research R12. Add the families to
  `putio-sync-telemetry/tests/contract.rs`.
- [X] T006 [P] Test then add `infohash.rs` in `putio-sync-arr/src/`:
  - `from_magnet(&str) -> Option<String>`: hex40 is lowercased; base32 32 → hex;
  - `from_torrent(&[u8]) -> Option<(hash, name)>`: a bencode scanner for the top-level `info`
    byte span, then SHA-1, plus `info.name`.

  Check against a hand-built .torrent whose hash is computed in the test.

## Phase 3: User Story 1 — Releases go straight to putio-sync (P1) 🎯 MVP

**Independent test**: the *arr-style handshake and add (magnet and .torrent) through the router
against mocked Put.io. The item is listed under the *arr's own hash, in the category folder.

- [X] T007 [US1] Test then implement the `ArrService` core in `putio-sync-arr/src/service.rs`:
  - `new(client, db, download_manager, config)`;
  - `add(source: Magnet|Url|Metainfo, download_dir: Option<&str>)` → `Added | Duplicate`, which:
    - validates that `download_dir` is inside the target directory (else the error
      `download-dir must be inside <target>`);
    - resolves or creates each category segment under the Put.io root;
    - creates the transfer (an http(s) .torrent URL is fetched, capped at 10 MB);
    - inserts the row with `state='putio'`;
    - logs `hash`, `name`, `transfer_id` and nothing else.

  Mock Put.io with mockito in `putio-sync-arr/tests/service.rs`.
- [X] T008 [P] [US1] Test then implement `rpc.rs` in `putio-sync-arr/src/`:
  - the request and response types;
  - `dispatch(&ArrService, method, args) -> Value` for `session-get`, `torrent-add`
    (`torrent-added` / `torrent-duplicate`), and the accepted no-op methods, exactly as in
    `contracts/transmission-rpc.md`;
  - records the RPC metric.
- [X] T009 [P] [US1] Implement `contact.rs` in `putio-sync-arr/src/`: the last-contact map by the
  first `User-Agent` token.
- [X] T010 [US1] Test then add the `/transmission/rpc` route in
  `putio-sync-api/src/routes/arr.rs` and wire it in `putio-sync-api/src/lib.rs`:
  - add `AppState.arr: Option<Arc<ArrService>>`, built in `AppState::new` when the client and
    DB exist;
  - make it a public route, applying the gate in order 404 / 503 / 429 / 401 / 409;
  - compare credentials in constant time and count failures on `AuthLimiter`;
  - generate a random session id at startup.

  Tests in `putio-sync-api/tests/arr_rpc.rs`: every gate, a magnet add with download-dir
  `<target>/sonarr` (asserting Put.io got `create-folder sonarr` and `transfers/add` with that
  parent), a .torrent add, a duplicate add, and a download-dir outside the target.

## Phase 4: User Story 2 — Progress, completion, import (P1)

**Independent test**: drive mocked Put.io states and seed `downloads` rows. `torrent-get`
moves through downloading → finished only when all accepted files are complete, with the right
`downloadDir`/`name`.

- [X] T011 [P] [US2] Test then implement `view.rs` in `putio-sync-arr/src/`. It is a pure
  mapping from an item plus its local progress to the Transmission torrent JSON, per the
  data-model "Transmission view" table (status codes 3/4/0, the two-half progress, `isFinished`,
  `errorString`, seed modes 0, `downloadDir` with no trailing slash).
- [X] T012 [US2] Test then add `enqueue_tree_report(root_id) -> TreeReport { stats, files: Vec<{file_id, name, size, relative_path, decision: Queued|Known|Blocked(ext)}>, root_name }`
  in `putio-sync-download/src/manager.rs`. `enqueue_tree` keeps its behaviour by delegating.
- [X] T013 [US2] Test then implement `ArrService::refresh()` (single-flight, at most every
  20 s) in `putio-sync-arr/src/service.rs`, which:
  1. lists transfers once and updates `putio_status`, percent, eta and size;
  2. moves `putio → local` on COMPLETED/SEEDING with a `file_id`: calls
     `enqueue_tree_report(file_id)`, stores the accepted files, and sets `output_name` and
     `name`;
  3. moves `local → done` when every file is completed;
  4. sets `failed` on ERROR (with Put.io's reason), on a missing transfer ("transfer no longer
     on Put.io"), or when no files were accepted (naming the blocked extensions);
  5. moves ERROR back to DOWNLOADING → `putio`;
  6. keeps the last state when Put.io is unreachable;
  7. updates the items gauge.

  Add `list(ids)` for `torrent-get`.
- [X] T014 [US2] Wire `torrent-get` into `rpc.rs`: refresh if stale, then list (filtered by
  hash, case-insensitive, or by numeric id). Add the router test: a full lifecycle, plus
  persistence across a new `ArrService` on the same DB.

## Phase 5: User Story 3 — Remove cleans up Put.io (P2)

**Independent test**: remove a finished and an unfinished item, with and without "delete data",
against mocked Put.io and a temp target dir.

- [X] T015 [US3] Test then implement `ArrService::remove(hash, delete_local)` in
  `putio-sync-arr/src/service.rs`, following research R8:
  1. cancel the transfer;
  2. delete `file_id` on Put.io (finished, or when `delete_local`);
  3. drop pending or paused queue rows and cancel active downloads for the item's files;
  4. delete `<target>/<category>/<output_name>` only when `delete_local`, after checking that
     the path stays inside the target directory and the name has no separators or `..`;
  5. drop the rows even when a Put.io call fails, and log the failure.

  Unknown hashes are a no-op.
- [X] T016 [US3] Wire `torrent-remove` (`ids`, `delete-local-data`) in `rpc.rs`, with a router
  test.

## Phase 6: User Story 4 — Bad releases blocklisted (P2)

**Independent test**: mocked *arr. A failed item triggers one queue lookup by hash and one
`DELETE ...?removeFromClient=true&blocklist=true`.

- [X] T017 [P] [US4] Test then implement `arr_api.rs` in `putio-sync-arr/src/`:
  - `ArrApi { kind, url, api_key }` with `system_status()`, `find_queue_id(hash)` (via
    `/api/v3/queue/details`) and `remove_and_blocklist(id)`;
  - the `X-Api-Key` header, never logged;
  - errors map to `unreachable | unauthorized | not_arr`.

  Tests in `putio-sync-arr/tests/arr_api.rs`.
- [X] T018 [US4] In `refresh()`, set `blocklist='pending'` when an item enters `failed` and a
  server matches its category. Then process pending items: success or not found → `done`;
  error → attempts + 1; at 5 attempts → `gave_up`. Record the blocklist metric. Service test
  with a mocked *arr.

## Phase 7: User Story 5 — Settings card and connection test (P3)

**Independent test**: `GET /arr` and the server test against mocked *arr apps. No secret appears
in any response.

- [X] T019 [US5] Test then add protected `GET /arr` and `POST /arr/servers/{index}/test` in
  `putio-sync-api/src/routes/arr.rs`, per `contracts/api.md`. Tests in
  `putio-sync-api/tests/arr_settings.rs`. Extend `putio-sync-api/tests/secrets_sweep.rs` with the
  arr password and API key.
- [X] T020 [US5] Add the "Radarr / Sonarr" card to `putio-sync-api/ui/pages/settings.js` in its
  own function. It shows:
  - enabled state, host:port (from `location`), URL base, username, and password set/not set;
  - the download folder;
  - last contact per agent;
  - item counts;
  - a server list with Test buttons and the result.

  Add API helpers in `putio-sync-api/ui/api.js` and fixtures in
  `putio-sync-api/examples/mock_ui.rs`.

## Phase 8: Polish

- [X] T021 [P] Document `[arr]` in `config.example.toml` (with Radarr/Sonarr setup steps and the
  env variables).
- [X] T022 [P] Add a README section: setup in Radarr/Sonarr, how it differs from the blackhole,
  path mapping notes, the metrics, and the `/arr` routes.
- [X] T023 Run `TMPDIR=<scratchpad> make ci` and fix everything. Run quickstart's mock UI check.

## Dependencies

- T001 → everything.
- T002–T006 are parallel and block the stories.
- US1 (T007–T010) → US2 (T011–T014) → US3 (T015–T016). US4 needs US2's refresh. US5 needs T002
  and T009, and can run alongside US3 and US4.
- Polish comes last.

## Parallel examples

- Phase 2: T002, T003, T004, T005, T006 together (different crates).
- US1: T008 and T009 alongside T007.
- US2: T011 alongside T012.
- US4: T017 alongside US3.

## Implementation strategy

MVP = US1 + US2: the *arr apps add items and import them when done, which already replaces the
blackhole. US3 then stops Put.io and disk from filling up. US4 fixes the fake-release loop. US5
is setup comfort.
