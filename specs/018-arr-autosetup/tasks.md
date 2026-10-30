---

description: "Task list for feature 018: Radarr/Sonarr Settings and Auto-Setup"
---

# Tasks: Radarr/Sonarr Settings and Auto-Setup

**Input**: Design documents from `specs/018-arr-autosetup/`

**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md),
[data-model.md](data-model.md), [contracts/](contracts/), [quickstart.md](quickstart.md)

**Tests**: included (Constitution III). Write each test first and watch it fail. Mock the *arr
APIs with mockito ([contracts/arr-api.md](contracts/arr-api.md)). Test the routes through the
axum router (`putio-sync-api/tests/common`).

## Format: `[ID] [P?] [Story] Description`

## Phase 1: Setup

No new crate and no new dependency (plan.md, Technical Context).

- [X] T001 Create the empty modules `clients.rs`, `discovery.rs`, `setup.rs` and `path_check.rs`
  in `putio-sync-arr/src/`, and declare them in `putio-sync-arr/src/lib.rs`.

## Phase 2: Foundational (blocks every story)

- [X] T002 [P] Config shape (R3). Test first, then change `putio-sync-core/src/config.rs`:
  - add `ArrServer.name: Option<String>`;
  - make `category` an `Option<String>` with
    `#[serde(default, skip_serializing_if = "Option::is_none")]`; it's loaded but ignored;
  - in `ArrConfig::validate`, drop the category rules; require an http(s) URL and unique
    normalized URLs; refuse `enabled = true` without a password from the file or env, with
    "Set or generate a password first";
  - add a test that an old config with `category` still loads.
- [X] T003 [P] `[arr]` writer (R1). Test first, then add `Config::save_arr_section(path,
  &ArrConfig)` to `putio-sync-core/src/config.rs`:
  - use `toml_edit` to replace only `[arr]` and `[[arr.servers]]`, keeping every other table,
    its comments and secrets byte-for-byte;
  - update the comment in `settings_table()`, which keeps dropping `arr`;
  - on a write error, return an error that the caller maps to `config_write_failed`.
- [X] T004 [P] Env flags. Test first, then make `resolve_arr` in `putio-sync-core/src/config.rs`
  report `password_from_env` and, for each server, `api_key_from_env`. Env values must never
  reach `save_arr_section`. Add `name` to `ArrSettings`, with the default "Radarr"/"Sonarr" and
  " (2)" etc. on a clash.
- [X] T005 Live settings (R2). Test first, then change `putio-sync-arr/src/service.rs`:
  - hold `settings: RwLock<Arc<ArrSettings>>`;
  - `settings()` returns an `Arc` clone;
  - add `apply(ArrSettings)`, which swaps only the settings, leaving tracked items and the
    refresh state alone.

  Depends on T004.
- [X] T006 Gate per request (R2). In `putio-sync-api/src/lib.rs`, build `ArrService` whenever a
  database exists, whether or not `enabled` is set. In `putio-sync-api/src/routes/arr.rs`,
  `transmission_rpc` reads `enabled` (off → 404) and the password (none → 503) from
  `service.settings()` on each request. Add `arr_rpc.rs` tests: off returns 404, and after
  `apply(on)` the same router answers the 409 handshake. Depends on T005.
- [X] T007 [P] Error kinds (data-model "Failure reasons"). Extend `ArrApiError` in
  `putio-sync-arr/src/arr_api.rs` with `TimedOut`, `UntrustedCertificate`,
  `UnsupportedVersion{found}`, `TestFailed{messages}` and `Refused{messages}`:
  - map `reqwest` `is_timeout()` to `TimedOut`, and a certificate error in the source chain to
    `UntrustedCertificate`;
  - `kind()` returns exactly `unreachable`, `timed_out`, `untrusted_certificate`,
    `unauthorized`, `not_arr`, `unsupported_version`, `invalid_category`, `arr_cannot_reach`,
    `arr_test_failed`, `arr_refused` or `config_write_failed`;
  - `system_status()` refuses a major version below 3 (R12);
  - test that no `Display` output contains the key.
- [X] T008 [P] Download-client calls (contracts/arr-api.md). Test first, then add these to
  `ArrApi` in `putio-sync-arr/src/arr_api.rs`:
  - `download_clients()`, `download_client_schema()`, `test_download_client(&body)`,
    `create_download_client(&body)`, `update_download_client(id, &body, force_save)` and
    `delete_download_client(id)`;
  - a `DownloadClient` type that keeps unknown fields, so a PUT round-trips;
  - field helpers `get_field` and `set_field`;
  - parse a 400 body `[{propertyName, errorMessage}]` into `TestFailed` or `Refused`.
- [X] T009 Putio-sync's own client (R6). Test first, then in `putio-sync-arr/src/clients.rs` add
  `points_at_putio_sync(client, port, candidate_hosts) -> bool`. It's true when
  `implementation == "Transmission"`, the normalized `urlBase == "/transmission/"`, `port`
  matches, and either `host` is a candidate or `name` starts with `putio-sync`. Depends on T008.
- [X] T010 Candidate hosts (R5). Test first, then in `putio-sync-arr/src/setup.rs` add
  `candidate_hosts(browser_host: Option<&str>, override: Option<&str>) -> Vec<String>`:
  - the override alone if given; otherwise `putio-sync`, then `/etc/hostname` or `HOSTNAME`,
    then the browser host without its port, then the default gateway from `/proc/net/route`
    (skipped when missing), then the UDP `local_addr()` toward `192.0.2.1`;
  - drop duplicates, and stop at 5;
  - put the route-file parser behind a function that takes `&str`, so it can be tested.
- [X] T011 [P] Metric (R13). Add the counter `putio_sync_arr_setup_total{kind, result}` in
  `putio-sync-telemetry/src/arr.rs`. Labels are an enum: `kind` is `radarr` or `sonarr`;
  `result` is `created`, `updated`, `unchanged` or a failure reason.

**Checkpoint**: settings apply at runtime, and the *arr client API and host candidates exist.

## Phase 3: User Story 3 - Manage the download client in the UI (P1)

**Goal**: on/off, username and password, with Generate, saved without a restart.

**Independent test**: turn it on, generate a password and save. The file is updated, the RPC
takes the new credentials, and no response returns the password.

- [X] T012 [US3] Write the tests first in `putio-sync-api/tests/arr_settings.rs` for
  `PUT /arr/client` (contracts/api.md):
  - an empty password keeps the current one;
  - turning it on with no password returns 400 "Set or generate a password first";
  - an env-set password is ignored and reported in `env.password`;
  - `password_changed` is set correctly;
  - a write failure returns 500 "Could not write the config file", and `service.settings()`
    is unchanged;
  - other config tables are unchanged.
- [X] T013 [US3] Implement `PUT /arr/client` in `putio-sync-api/src/routes/arr.rs`:
  1. clone the file config and merge the password;
  2. validate;
  3. `save_arr_section`;
  4. swap `state.config`;
  5. `resolve_arr`, then `service.apply`;
  6. log with `tracing` fields `enabled`, `username` and `password_changed` (never the value).

  Depends on T003, T006.
- [X] T014 [US3] Change `GET /arr` in `putio-sync-api/src/routes/arr.rs` to the
  contracts/api.md shape: `client` (enabled, username, password_set, env, connection with
  `hosts` from `candidate_hosts`, download_dir) and `applications`. Leave out `items` and
  `clients_seen`. Rewrite the existing `GET /arr` tests in
  `putio-sync-api/tests/arr_settings.rs`.
- [X] T015 [P] [US3] Add `saveArrClient(body)` to `putio-sync-api/ui/api.js` and change
  `arrStatus()` to read the new shape.
- [X] T016 [US3] Rewrite the "Download client" section in `putio-sync-api/ui/pages/settings.js`:
  - a switch, a username field and a write-only password field;
  - Generate uses `crypto.getRandomValues` (32 characters from `[A-Za-z0-9]`), with reveal and
    copy until the form is saved or left;
  - env fields are read-only with "set by environment";
  - below the form, the connection values to copy;
  - after a password change, a warning (the FR-008 update offer is wired in T040);
  - drop the old "Set enabled = true … then restart" hint.

**Checkpoint**: SC-003 for the download client.

## Phase 4: User Story 1 - One-click setup of a discovered *arr (P1) 🎯 MVP

**Goal**: discover, then preview, then set up one Transmission client that passes the *arr's
test.

**Independent test**: mock a Radarr and a Sonarr. Both are discovered. After setup, each has one
enabled putio-sync client with the right fields, and the result names the app and its version.

- [X] T017 [P] [US1] Write the discovery tests first in `putio-sync-arr/src/discovery.rs`:
  - a `/ping` OK plus `<title>Radarr</title>` gives kind radarr;
  - a non-*arr page is ignored;
  - `initialize.json` is never requested (a mockito `expect(0)`);
  - a 3 s per-probe timeout and a 10 s overall deadline (tokio `test-util` paused time);
  - tier dedupe: a `container` hit hides a later `host` hit of the same kind and port;
  - a second concurrent run returns `Busy`;
  - `tried` lists every URL probed.
- [X] T018 [US1] Implement `discover(saved_urls, browser_host, http)` in
  `putio-sync-arr/src/discovery.rs`, with a `tokio::task::JoinSet` and the
  `ArrService.discovery: tokio::sync::Mutex<()>` single-flight (`try_lock` → `Busy`).
  Candidates: saved applications, then `radarr` and `sonarr`, then the gateway, `localhost` and
  the browser host, each on 7878 and 8989 over http. Returns `DiscoveredInstance { kind, url,
  found_by: saved|container|host|browser, version: None }`. Depends on T010.
- [X] T019 [P] [US1] Write the setup tests first in `putio-sync-arr/src/setup.rs`, against a
  mockito Radarr:
  - preview returns `action: create`, `client_name: putio-sync`, `category: radarr`;
  - setup tests candidate hosts in order until one passes, then POSTs exactly one client with
    the body from contracts/arr-api.md (`priority` 1, `movieCategory`; Sonarr uses
    `tvCategory`);
  - an invalid category is refused before any request;
  - the result has `outcome: created`, `host`, and the app name and version.
- [X] T020 [US1] Implement `preview(req)` and `run(req)` in `putio-sync-arr/src/setup.rs` for
  the create path:
  - check the address and key with `system_status`;
  - read the clients and the schema;
  - build the body from the schema's `Transmission` entry;
  - try each `candidate_hosts` entry with `test_download_client` (5 s each);
  - create the client;
  - the `SetupPlan` and `SetupResult` types are as in data-model.md;
  - count `putio_sync_arr_setup_total`;
  - log `kind`, `url`, `client_id` and `result`.

  Depends on T008, T009, T010, T011.
- [X] T021 [US1] Download client off or no password (FR-023a). Test first, then handle it in
  `run`:
  - preview reports `download_client.will_enable` and `will_generate_password`;
  - on run, the api layer generates a password with two joined `Uuid::new_v4().simple()`
    values (R11), unless there's one in the file or env;
  - it saves with `save_arr_section` and calls `apply` before the *arr test;
  - the password never appears in a response.

  Put it in `putio-sync-api/src/routes/arr.rs`, and the tests in
  `putio-sync-api/tests/arr_settings.rs`.
- [X] T022 [US1] "Remember" (FR-023b). Test first, then: after a successful setup with
  `remember`, save the application with its key, updating one with the same normalized URL;
  without `remember`, write nothing to disk. If that write fails, the result is success in the
  *arr plus a "not saved" warning. Code in `putio-sync-api/src/routes/arr.rs`, tests in
  `putio-sync-api/tests/arr_settings.rs`.
- [X] T023 [US1] Add the routes `POST /arr/discover` (409 when busy), `POST /arr/setup/preview`
  and `POST /arr/setup` to `putio-sync-api/src/routes/arr.rs`, in the protected router. Pass
  the request's `Host` header as the browser host. Add route tests to
  `putio-sync-api/tests/arr_settings.rs`, including 401 without auth.
- [X] T024 [P] [US1] Add `discoverArr()`, `previewArrSetup(req)` and `runArrSetup(req)` to
  `putio-sync-api/ui/api.js`.
- [X] T025 [US1] Add the setup view to `putio-sync-api/ui/pages/settings.js`:
  - a "Set up automatically" button;
  - a discovery list with the addresses tried;
  - a password-style API key field, the category (default `radarr`/`sonarr`), an optional host
    override, and the "Remember" checkbox;
  - Preview shows the plan, including the download-client enable/generate notice;
  - Set up shows the result;
  - a late result for a closed view is dropped.

**Checkpoint**: MVP. SC-001 and SC-002 can be checked (quickstart steps 3 and 4).

> **MVP implementation notes (2026-10-28, uncommitted):**
> - FR-006 ("Set or generate a password first") is checked in the `PUT /arr/client` handler, not in
>   `Config::validate` as T002 says, so an existing on-without-password config still loads.
> - Already done ahead of their phase: update in place and the name clash `putio-sync (2)` (part of
>   T030), and the `arr_cannot_reach` / `arr_test_failed` split (part of T034). US2/US5 still need
>   undo, the blackhole, unchanged detection and bare-host normalization.
> - Discovery tests use real short timeouts, not paused time.
> - Item counts and last contact are gone from Settings until the Status card (T044, T045).
> - `toml` (workspace) is now a dev-dependency of putio-sync-api, for tests.

## Phase 5: User Story 2 - Manual address entry (P1)

**Goal**: setup works without discovery, including a bare host and a URL base.

**Independent test**: with nothing discovered, set up a mocked Sonarr at `http://host:port/sonarr`.

- [X] T026 [US2] Write the tests first in `putio-sync-arr/src/setup.rs`:
  - a full URL with a path is kept as the base;
  - a bare host is tried as `http://host:7878`, then `http://host:8989`, and the first that
    answers as an *arr is used;
  - a non-*arr answer gives `not_arr` "Not a Radarr or Sonarr server" with nothing changed.
- [X] T027 [US2] Implement the address normalization in `putio-sync-arr/src/setup.rs` and use it
  in `preview` and `run`. Depends on T020.
- [X] T028 [US2] In `putio-sync-api/ui/pages/settings.js`, keep the manual URL and key fields
  always visible in the setup view, and show "No Radarr or Sonarr found" with the addresses
  tried.

## Phase 6: User Story 5 - Existing download clients handled safely (P2)

**Goal**: update in place, no duplicates, the blackhole only when the operator opts in, and
nothing left behind on failure.

**Independent test**: seed a putio-sync client with a wrong password, a Torrent Blackhole and a
qBittorrent client. Only the first is changed, unless "Disable Torrent Blackhole" is ticked.

- [X] T029 [US5] Write the tests first in `putio-sync-arr/src/setup.rs`:
  - a matching client is updated with PUT, keeping its `id`, `tags` and unknown fields;
  - a non-matching client named `putio-sync` is left alone, and `putio-sync (2)` is used;
  - "already set up" (R8): fields equal and the test passes gives `unchanged` with no write;
  - running twice leaves one client (SC-005);
  - the preview lists every existing client with `points_at_putio_sync`;
  - `blackhole_enabled` is reported;
  - the blackhole is disabled with `enable=false`, PUT `forceSave=true`, only when
    `disable_blackhole` is set and the main write succeeded;
  - the qBittorrent client is never touched.
- [X] T030 [US5] Implement update in place, the name clash, unchanged, and disabling the
  blackhole in `putio-sync-arr/src/setup.rs`. Depends on T020.
- [X] T031 [US5] Undo (R7). Test first, then: a create that fails after a passing test deletes
  any client it created; an update that fails restores the fetched body with
  `forceSave=true`; the result is `arr_refused` with the *arr's messages. Code in
  `putio-sync-arr/src/setup.rs`.
- [X] T032 [US5] Add the "Disable Torrent Blackhole" checkbox (unticked by default) and the
  existing-clients list to the preview in `putio-sync-api/ui/pages/settings.js`.

## Phase 7: User Story 8 - Clear results and failure messages (P2)

**Goal**: each failure has its own message and a next step, and none leaves a failing client.

**Independent test**: cause each FR-026 reason against mocks, and check the message and that no
client remains.

- [X] T033 [US8] Write the tests first in `putio-sync-api/tests/arr_settings.rs`, one per reason:
  `unreachable`, `timed_out`, `untrusted_certificate`, `unauthorized`, `not_arr`,
  `unsupported_version`, `invalid_category`, `arr_cannot_reach` (with `hosts_tried`),
  `arr_test_failed` (with the *arr's message), `arr_refused` and `config_write_failed`. Each
  checks that the mock *arr ends with no new failing client (SC-006).
- [X] T034 [US8] Make `run` in `putio-sync-arr/src/setup.rs` return `arr_cannot_reach` when every
  host's test fails on connectivity, and `arr_test_failed` otherwise. Strip keys and passwords
  from all `detail` strings.
- [X] T035 [US8] Add a reason → message + "what to check next" map to
  `putio-sync-api/ui/pages/settings.js`, replacing `TEST_REASONS`. `arr_cannot_reach` shows a
  host override field.

> **US8 notes:** `timed_out` and `untrusted_certificate` have no route test. A timeout would
> cost 10 s per run (the arr_api unit test covers its mapping), and a TLS test server would need
> a new dependency. Both reasons are mapped and shown like the others.

## Phase 8: User Story 4 - Applications add, edit and delete (P2)

**Goal**: Prowlarr-style application forms, and the category read from the *arr and used for
blocklisting.

**Independent test**: add a mocked Radarr whose putio-sync client has category `movies`. The row
shows `movies`, a failed `movies` item is blocklisted in it, and edit and delete update the file.

- [X] T036 [P] [US4] Category cache (R4). Test first, then in `putio-sync-arr/src/clients.rs`:
  - `read_clients(server)` fills `ClientInfo { categories, client_present, read_at,
    last_error, path_check }` from the matching clients' `movieCategory`/`tvCategory`;
  - a failed read keeps the old value and sets `last_error`;
  - `ArrService::spawn_category_refresh()` runs at startup, then every 15 minutes, for saved
    applications only.

  The cache is `ArrService.clients: RwLock<HashMap<String, ClientInfo>>`, keyed by normalized
  URL.
- [X] T037 [US4] Change `blocklist_server(category)` in `putio-sync-arr/src/service.rs` to pick
  the server whose cached categories contain the category. Fall back to the legacy typed
  `category`, then log and give up. Test it with two Radarrs that have different categories.
  Depends on T036.
- [X] T038 [US4] Write the tests first in `putio-sync-api/tests/arr_settings.rs` for:
  - `POST /arr/applications/test` (unsaved values, or `index` with an empty key);
  - `POST /arr/applications/{index}/test`, which refreshes `ClientInfo`;
  - `POST /arr/applications` (201; 400 for an invalid or duplicate URL or a missing key);
  - `PUT /arr/applications/{index}` (an empty key keeps it; 404);
  - `DELETE /arr/applications/{index}` (204; the *arr mock gets no request);
  - `legacy_category` reported in `GET /arr`;
  - `POST /arr/servers/{i}/test` is gone.
- [X] T039 [US4] Implement those routes in `putio-sync-api/src/routes/arr.rs`:
  - join host, port, ssl and url_base into `url` with `reqwest::Url`, and split it back for
    `GET /arr`;
  - write with `save_arr_section`, then `apply`, then refresh `ClientInfo`;
  - log the action and outcome without the key;
  - remove `test_server`.

  Depends on T036.
- [X] T040 [US4] UI in `putio-sync-api/ui/pages/settings.js` and `putio-sync-api/ui/api.js`:
  - the Applications list: kind, name, address, categories with their age or read error,
    "Not set up" with an auto-setup link, the legacy-category warning, and Test, Edit and
    Delete;
  - Add Radarr and Add Sonarr forms (name, host, port defaulting to 7878 or 8989, SSL, URL
    base, a write-only API key);
  - Save after a failed test asks for confirmation;
  - Delete asks for confirmation;
  - the FR-008 offer after a password change calls `POST /arr/setup` with
    `{"target": {"application": n}}` for each application with a putio-sync client.

> **US4 notes (implementation):**
>
> - The cache is keyed by the URL trimmed of `/` and lowercased (`service::url_key`). A
>   failed read keeps the last categories and `read_at` and sets `last_error` (shown in
>   `GET /arr` as `read_error`). `client_present` is null until the first good read.
> - The refresh starts from `putio_sync_api::spawn_arr_refresh`, called by
>   `api_listener::start` (after the bind) and by `create_app`. It matches clients with
>   `candidate_hosts(None, None)`, so a client on a typed host is still found by its
>   `putio-sync` name (R6).
> - Setup, add, edit and saved-application tests also refresh `ClientInfo`. An unsaved form
>   test reads the clients but doesn't store them.
> - The application form takes a host only; a value with `://` or `/` is a 400. A port left
>   empty is 7878 or 8989. The legacy `category` is kept on edit and dropped on delete.
> - Kind mismatch (a Sonarr saved as Radarr) is the existing `not_arr` reason ("found
>   Sonarr"); no new reason was added.
> - The FR-008 offer is a module flag: after a password change, the reloaded Applications list
>   shows "Update …", which runs `POST /arr/setup {target: {application: n}}` for each
>   application with a key. "Set up" also appears on a row whose last read found no
>   putio-sync client.
> - Checked by hand in the mock UI (`cargo run -p putio-sync-api --example mock_ui`, which now
>   mocks Sonarr's client list): add, duplicate refused, failed-test confirm, edit with the
>   key kept, delete with confirm, legacy warning, password offer.

## Phase 9: User Story 6 - Path-mapping help and check (P2)

**Goal**: report a wrong download-folder path before the first import.

**Independent test**: a mock that lists `/downloads` gives `visible`; one that doesn't gives
`not_visible` with its mappings.

- [X] T041 [P] [US6] Add `remote_path_mappings()` and `filesystem(path)` to
  `putio-sync-arr/src/arr_api.rs`, tested with mockito.
- [X] T042 [US6] Write the tests first, then implement `check(api, folder, client_host) ->
  PathCheck` in `putio-sync-arr/src/path_check.rs` (R10):
  1. apply a mapping whose `host` equals the client host and whose `remotePath` is a prefix;
  2. list the parent with `allowFoldersWithoutTrailingSlashes=true`;
  3. look for the child name.

  The `status` is `visible`, `not_visible` or `unknown`. Run it on application test and after
  setup (T020, T039), and store it in `ClientInfo.path_check`. Depends on T041.
- [X] T043 [US6] In `putio-sync-api/ui/pages/settings.js`, show the download folder with a
  one-line path-mapping explanation and a link to the README section, and each application's
  "Folder visible" or "<App> cannot see <folder>" with the fix.

> **US6 notes (implementation)**:
> - `path_check::check` handles `/` and `\` paths. A Windows root parent (`D:\`) keeps its
>   separator. A root folder can't be checked and is `unknown`.
> - The check needs the client's host for the mappings, so it runs only when a putio-sync
>   client is present. `ClientInfo.host` is the first matching client's host.
> - A saved application's test and a setup of a saved URL keep the result in `ClientInfo`.
>   A form test or an unsaved setup returns it without keeping it. The 15-min refresh keeps the
>   last result and doesn't re-check.
> - `SetupResult.path_check` is set by the api layer after a setup that didn't fail, so the
>   setup.rs tests are unchanged. A `not_visible` result after setup is also a toast.
> - The UI links to README `#path-mapping`. A short "Path mapping" section was added now so the
>   link works. T048 adds the Docker example. The old "Paths and the blackhole" heading is
>   now "The blackhole".
> - Verified in the mock UI: Sonarr's mappings don't cover the temp download folder, so it
>   shows "Sonarr cannot see …" with the fix and its mappings.

## Phase 10: User Story 7 - Status moves to the Status page (P3)

**Goal**: Settings shows only settings, and the counts move to Status.

**Independent test**: Settings has no counts. Status shows them with the API's values.

- [X] T044 [US7] Add `GET /arr/status` to `putio-sync-api/src/routes/arr.rs`, returning
  `enabled`, `clients_seen` and `items` (contracts/api.md), with tests in
  `putio-sync-api/tests/arr_settings.rs`.
- [X] T045 [US7] Add `arrStatusCard()` to `putio-sync-api/ui/pages/status.js` with `card(…)`. It
  shows the counts and the last contact, or "off" with a link to Settings. Add `arrRuntime()`
  to `putio-sync-api/ui/api.js`, and remove the counts and contact times from `settings.js`.

> **US7 notes (implementation)**:
> - `GET /arr/status` works without the database too: `enabled` comes from the settings and the
>   counts are zero. It's in the secrets sweep.
> - Settings had already lost the counts and contact times in the MVP's `GET /arr` rewrite, so
>   T045 only added the Status card.
> - The Status page fetches `/arr/status` alongside `/status` every 3 s. A failure only marks
>   the card unavailable. With the client off, the card links to Settings.

## Phase 11: Polish

- [X] T046 [P] Extend `putio-sync-api/tests/secrets_sweep.rs` to every new route and setup
  outcome. No API key or download-client password may appear in responses or captured logs
  (SC-008).
- [X] T047 [P] Update `config.example.toml`: `[arr]` can be edited in the UI; add the optional
  `name`; mark `category` as no longer needed (FR-030).
- [X] T048 [P] Update the "Radarr and Sonarr" section of `README.md`: the Settings forms,
  auto-setup, manual entry, the path-mapping section (with a Docker example), and keep the
  manual steps. Update the route table (FR-030).
- [X] T049 [P] Add a CHANGELOG.md entry under Unreleased.
- [X] T050 Run `make ci`, then quickstart step 2 against local Docker Radarr and Sonarr.
  Record the masked-password result from step 6 in research.md (R7).

> **Polish notes (implementation)**:
> - The secrets sweep covers `/arr/status`, `PUT /arr/client`, discovery, preview and setup
>   (created, unauthorized, unreachable, and by application index with "Remember").
> - Quickstart step 2 ran against Radarr 6.4.4 and Sonarr 4.0.20 in local Docker. All ten
>   steps passed, using the API, plus the real UI for the path check and Status card. No key or
>   password appeared in putio-sync's logs.
> - **A bug found and fixed**: Radarr 6 opens the Transmission handshake with a GET.
>   `/transmission/rpc` answered any non-POST with 405 before checking auth or the session id
>   (since feature 011), so Radarr's Test failed with "Authentication Failure". The method
>   check now runs after the handshake. The test is `a_get_handshake_gets_the_session_id` in
>   `arr_rpc.rs`, with a CHANGELOG line under Fixed. Sonarr 4 still opens with a POST.
> - R7 is verified: a PUT with the masked `********` password keeps the stored one.
> - Step 6's write failure was simulated by stopping Radarr between preview and setup. Setup
>   reported `unreachable` and the client kept its category and still passed Test.

## Dependencies

- Setup (T001) → Foundational (T002–T011) → stories.
- **US3** (T012–T016) needs T003 and T006. Do it first: US1's FR-023a reuses its write path.
- **US1** (T017–T025) needs T008–T011 and T013.
- **US2**, **US5** and **US8** each build on US1's `setup.rs` (T020). They touch the same file,
  so do them one after another.
- **US4** (T036–T040) needs only the Foundational phase. It can run alongside US1, since its
  only shared file is `routes/arr.rs`. T040 needs T023 for the FR-008 offer.
- **US6** needs T020 and T039 for its run points. **US7** needs only T014.
- Polish comes last.

## Parallel examples

- Foundational: T002, T003, T007, T008 and T011 (different files or independent sections).
- US1: T017 and T019 (tests), and T024 (api.js), alongside T018.
- US4 and US1 in parallel after Foundational. US4 owns `clients.rs`, US1 owns `discovery.rs`
  and `setup.rs`.
- Polish: T046–T049.

## Implementation strategy

1. **MVP**: Phases 1–4 (Foundational, then US3, then US1). This gives runtime settings and
   one-click setup on the NAS (SC-001, SC-002).
2. **Then**: US2 and US5, which make setup safe on the NAS's existing blackhole clients. Then
   US8.
3. **Then**: US4 (forms, and the category from the *arr), then US6, then US7.
4. **Last**: Polish. Each phase ends with `make ci` passing.
