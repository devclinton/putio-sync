# Tasks: Plugin Links and a Plugin Registry

**Input**: Design documents from `specs/059-plugin-registry/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/api.md,
contracts/registry-index.md, contracts/cli-and-manifest.md, quickstart.md

**Tests**: included. The constitution (III) and the success criteria (SC-002, SC-004, SC-006) ask
for automated tests; registry tests use `mockito` and archives built in the test, never GitHub.
Write each story's tests first and see them fail.

**Registry repository**: most of US4 is already drafted at `~/dev/putio-sync-plugin-registry`
(README, CONTRIBUTING, REVIEW, templates, examples, `scripts/registry.py`, CI). Its tasks below
finish and publish it. Publishing (creating the GitHub repository, pushing, making a release) is
outward-facing: ask the maintainer before each step.

## Format: `[ID] [P?] [Story] Description`

## Phase 1: Setup

- [ ] T001 Add `sha2 = "0.10"`, `flate2 = "1"`, `tar = "0.4"`, `zip = { version = "2", default-features = false, features = ["deflate"] }` and `semver = "1"` to putio-sync-plugins/Cargo.toml; `cargo build -p putio-sync-plugins` passes (plan.md › Primary Dependencies)
- [ ] T002 Create empty modules putio-sync-plugins/src/registry/{mod.rs,index.rs,fetch.rs,archive.rs,install.rs} with header doc comments in the crate's style, and `pub mod registry;` in putio-sync-plugins/src/lib.rs

## Phase 2: Foundational (blocks all stories)

- [x] T003 Add `pub fn valid_link(url: &str) -> Result<(), &'static str>` to putio-sync-core/src/plugins.rs: at most 300 characters; `https://` with a host, or `http://` with a loopback or private host (127.0.0.0/8, ::1, `localhost`, 10/8, 172.16/12, 192.168/16, `*.local`); anything else, including `javascript:`, `data:` and `ftp:`, is refused with "must be an https address" (data-model.md › link rule)
- [x] T004 [P] Unit tests for `valid_link` in putio-sync-core/src/plugins.rs: https ok; http to 127.0.0.1, 192.168.1.10 and nas.local ok; http to example.com, javascript:, data:, a 301-character https URL and `https://` with no host refused

## Phase 3: User Story 1 – See who made a plugin and where its docs are (P1) 🎯 MVP

**Independent test**: a folder plugin with all six fields shows them on its tab with working links; without them it loads unchanged; `docs = "ftp://x"` makes it **Broken** with `plugin.toml: docs: …`.

### Tests

- [x] T005 [P] [US1] Manifest tests in putio-sync-plugins/src/manifest.rs: all six fields parse; none set still parses (SC-003); `author` over 100 characters or with a control character refused; `license` must match `[A-Za-z0-9.+() -]{1,60}`; each of `homepage`, `docs`, `repository`, `issues` refused by the link rule with the message `plugin.toml: <field>: must be an https address`
- [x] T006 [P] [US1] Test in putio-sync-plugins/src/discover.rs that every bundled manifest sets `author`, `license`, `docs` and `repository`, and still loads
- [x] T007 [P] [US1] CLI tests in putio-sync-cli/tests/plugins_check.rs with `assert_cmd` and `assert_fs`: a good folder prints `<name> <version>: ok`, exit 0; a bad link and a Lua syntax error print one line each and `2 problems`, exit 1; a missing folder exits 2; `--json` prints `{"name","version","problems"}` (contracts/cli-and-manifest.md)
- [x] T008 [P] [US1] Node tests in putio-sync-api/tests/ui/plugins.test.mjs for `linkOk(url)` (same rule as T003) and `links(status)` (returns `[{label, href}]` for Docs, Website, Source, Report a problem in that order, skipping unset and refused links)

### Implementation

- [x] T009 [US1] Add `author`, `license`, `homepage`, `docs`, `repository`, `issues` (`String`, `#[serde(default)]`) to `Manifest` in putio-sync-plugins/src/manifest.rs and check them in `Manifest::check` with the rules in T005, using `putio_sync_core::plugins::valid_link`; keep `deny_unknown_fields`
- [x] T010 [US1] Add `links: Links` (`author`, `license`, `homepage`, `docs`, `repository`, `issues`, each `Option<String>`, `skip_serializing_if = "Option::is_none"`) to `PluginStatus` and fill it in `status()` in putio-sync-plugins/src/service.rs (contracts/api.md › GET /plugins)
- [x] T011 [P] [US1] Set `author = "devclinton"`, `license = "MIT"`, `repository = "https://github.com/devclinton/putio-sync"`, `issues = "https://github.com/devclinton/putio-sync/issues"` and `docs` (the plugin's section of the docs site's plugins page) in each putio-sync-plugins/bundled/*/plugin.toml
- [x] T012 [US1] Make `from_folder` `pub(crate)` in putio-sync-plugins/src/discover.rs, and add `pub fn check_folder(path: &Path) -> (Option<Manifest>, Vec<Problem>)` to putio-sync-plugins/src/source.rs that runs `from_folder` (manifest, entry file, size, WebAssembly magic) and, for a Lua plugin, the workbench's `syntax` compile without running it (research.md R10; reuses 055's `source::check` rules)
- [x] T013 [US1] Add `Plugins { #[command(subcommand)] command: PluginsCommand }` with `Check { folder: PathBuf, #[arg(long)] json: bool }` to putio-sync-cli/src/cli.rs and implement it in a new putio-sync-cli/src/commands/plugins.rs using `source::check_folder`; it loads no config and needs no token; register it in putio-sync-cli/src/commands/mod.rs
- [x] T014 [US1] Add `linkOk` and `links` to putio-sync-api/ui/plugins.js
- [x] T015 [US1] In putio-sync-api/ui/settings/plugins.js draw a links row on each plugin tab and under each **Available plugins** line: author and license as text, links from `links()` as `<a target="_blank" rel="noopener noreferrer">`, nothing when no field is set; link "Needs settings" and settings errors to `docs` when set (FR-005)
- [x] T016 [P] [US1] Links row CSS in putio-sync-api/ui/app.css
- [x] T017 [P] [US1] Add the new UI strings to putio-sync-api/ui/i18n/en.json
- [x] T018 [P] [US1] Document the six fields and `putio-sync plugins check` in PLUGINS.md › Writing a plugin (the docs site includes PLUGINS.md through site/book/src/plugins.md)

**Checkpoint**: US1 is shippable on its own (links, bundled metadata, `plugins check`).

## Phase 4: User Story 2 – Find and install a reviewed plugin (P1)

**Independent test**: with `[plugin_registry] url` pointing at a local registry, **More plugins** lists the entry; **Install** writes `plugins/<name>/` with `.registry.json`, off; a tampered archive writes nothing; registry off makes no request.

### Tests

> **Built differently (US2):** releases are `.tar.gz`/`.tgz` or a `.wasm` carrying its manifest (no `.zip`); fetch, install and the record live in `registry/mod.rs` with `install::install_with` writing `.registry.json`, so there is no `fetch.rs` or `registry/install.rs`; no preview route, since the Install confirm shows what the list says and the archive check refuses anything else; no events filter; no telemetry counters (T040). Version ordering uses a small `x.y.z` parser, not the `semver` crate.

- [x] T019 [P] [US2] Index tests in putio-sync-plugins/src/registry/index.rs: the JSON from contracts/registry-index.md parses; unknown fields ignored; `schema_version` 2 refused with "registry needs a newer putio-sync"; a duplicate name (case-insensitive) refuses the whole index; a bundled name, a bad link or no release drops only that entry and counts `skipped`; `offered(version)` picks the newest release whose `min_putio_sync` ≤ the running version, else `needs_newer` with the lowest `min_putio_sync`
- [x] T020 [P] [US2] Archive tests in putio-sync-plugins/src/registry/archive.rs with archives built in the test (`tar`, `zip`): good `.tar.gz` and `.zip` pass; wrong SHA-256 refused before opening; absolute path, `..`, a symlink, a hard link, two top folders, a file at the top level, 201 files, and over 5 MB unpacked each refused with their own message; manifest name, version, events or hosts differing from the entry refused; missing entry script refused (research.md R5, SC-002)
- [x] T021 [P] [US2] Fetch tests in putio-sync-plugins/src/registry/fetch.rs with `mockito`: index over 2 MB refused; archive over 5 MB refused (read stops at the limit); 6 redirects refused; a redirect from https to http on a public host refused; `User-Agent` is `putio-sync/<version>`
- [x] T022 [P] [US2] Registry tests in putio-sync-plugins/src/registry/mod.rs with `mockito` and `tempfile`: second list within 6 h makes no request, `refresh` does; cache survives a new `Registry` from `plugin-data/registry-index.json`; unreachable registry returns the cached list with `fetched_at` and `error`; changing `url` drops the cache; `enabled = false` makes zero requests (mock `expect(0)`, SC-004); states `available`, `needs_newer`, `name_in_use` (hand-made folder, and bundled name) are reported as data-model.md › Registry state says
- [x] T023 [P] [US2] Install tests in putio-sync-plugins/src/registry/install.rs: install writes `plugins/<name>/` and `.registry.json` with `registry`, `version`, `sha256`, `installed_at`, `files`; any failed check leaves `plugins/` byte-identical; a hand-made folder of that name is never touched; a read-only plugins folder reports the error and leaves no `.install-*`; two installs of one name at once: the second gets "in progress"; the plugin is not enabled and no host approved (FR-024)
- [x] T024 [P] [US2] API tests in putio-sync-api/tests/plugins_registry.rs: `GET /plugins/registry`, `POST /plugins/registry/preview` and `POST /plugins/registry/install` without a token are 401; responses match contracts/api.md, including 409 for `name_in_use`/`needs_newer`/`withdrawn` and 422 with the check's text for a refused archive; `GET /plugins` shows `registry` for the installed plugin and makes no registry request
- [x] T025 [P] [US2] Node tests in putio-sync-api/tests/ui/plugins.test.mjs for `filterEntries(entries, text, events)` (matches name, description or author, case-insensitive; events filter keeps entries handling any chosen event) and `entryAction(entry)` (Install, Name in use, Needs putio-sync X, From the registry)

### Implementation

- [x] T026 [US2] Add `PluginRegistryConfig { enabled: bool (default true), url: String (default "https://raw.githubusercontent.com/devclinton/putio-sync-plugin-registry/main/index.json") }` as `plugin_registry` on `Config` in putio-sync-core/src/config.rs, `#[serde(default)]`, with `url` checked by `valid_link` in config validation
- [x] T027 [US2] Implement `Index`, `Entry`, `Hosts`, `Release` and `Index::parse(bytes) -> Result<(Index, skipped), String>` and `Entry::offered(running: &semver::Version)` in putio-sync-plugins/src/registry/index.rs per contracts/registry-index.md
- [x] T028 [US2] Implement the registry HTTP client in putio-sync-plugins/src/registry/fetch.rs: 30 s timeout, custom redirect policy (at most 5, each https or http to a local host by `valid_link`), `get_limited(url, max_bytes)` that streams and stops past the limit (research.md R4)
- [x] T029 [US2] Implement `check_archive(bytes, url, entry, release) -> Result<Unpacked, String>` in putio-sync-plugins/src/registry/archive.rs: SHA-256 first, then `.tar.gz`/`.tgz` or `.zip` read into memory with the rules in T020, then `Manifest::parse` of `plugin.toml` compared with the entry (research.md R5)
- [x] T030 [US2] Implement `install(dir, entry, unpacked, registry_url)` in putio-sync-plugins/src/registry/install.rs: write to `plugins/.install-<name>/`, write `.registry.json`, rename to `plugins/<name>/`, remove the staging folder on any error; and `InstallRecord::read(folder)` (data-model.md › Install record)
- [x] T031 [US2] Implement `Registry` in putio-sync-plugins/src/registry/mod.rs: holds config, data dir and plugins dir; `list(refresh)` with the 6 h cache in `plugin-data/registry-index.json` (`{url, fetched_at, index}`) and one fetch at a time; `state_of(entry, found)`; a per-name install lock; `preview` and `install` that use T028–T030 then call the service's reload
- [x] T032 [US2] Wire `Registry` into `PluginsService` in putio-sync-plugins/src/service.rs (built in `from_config`, updated on settings change); add `registry: Option<InstallState>` to `PluginStatus` from the install record and the cached index only
- [x] T033 [US2] On start, remove leftover `plugins/.install-*` folders in putio-sync-plugins/src/discover.rs (or the service start), logging each
- [x] T034 [US2] Add `GET /registry`, `POST /registry/preview` and `POST /registry/install` to the router in putio-sync-api/src/routes/system/plugins.rs per contracts/api.md
- [x] T035 [US2] Accept and return `plugin_registry` in putio-sync-api/src/routes/system/settings.rs; a `url` change drops the cache
- [x] T036 [US2] Add `filterEntries` and `entryAction` to putio-sync-api/ui/plugins.js
- [x] T037 [US2] Add **More plugins** to putio-sync-api/ui/settings/plugins.js below **Available plugins**: the note on what "reviewed" means; search box and events filter; one row per entry (name, version, description, author, hosts, links, action); **Refresh**; the error and list age when unreachable; "turned off" with a link to the setting when disabled; **Install** opens a confirm with the hosts, calls `/plugins/registry/install`, then redraws; all registry text through `textContent`, only `linkOk` links clickable (FR-026)
- [x] T038 [US2] Registry switch and address fields in the Plugins section's settings in putio-sync-api/ui/settings/plugins.js
- [x] T039 [P] [US2] **More plugins** list CSS in putio-sync-api/ui/app.css
- [ ] T040 (dropped: the 029 install route has no counters either; revisit with 058 usage stats) [P] [US2] Registry fetch and change counters `putio_sync_plugin_registry_fetches_total{result="ok|error|cached"}` and `putio_sync_plugin_registry_changes_total{kind="install|update|remove", result="ok|error"}` in putio-sync-telemetry/src/plugins.rs, with tests, and count them from putio-sync-plugins/src/registry/mod.rs (R12)
- [x] T041 [P] [US2] Document `[plugin_registry]` in config.example.toml (after `plugins_dir`) and example-config.toml
- [x] T042 [P] [US2] Add the new UI strings to putio-sync-api/ui/i18n/en.json

**Checkpoint**: install from a local registry works end to end (quickstart.md checks 3–8).

## Phase 5: User Story 3 – Keep registry plugins up to date (P2)

**Independent test**: install 1.0.0, list 1.1.0 with one more host, refresh: **Update to 1.1.0** with the new host; update keeps settings and shows **Needs approval**; a broken 1.2.0 rolls back.

### Tests

- [ ] T043 [P] [US3] Update tests in putio-sync-plugins/src/registry/install.rs: update replaces files and the record; `[[plugins]]` settings, secrets, approved hosts and `plugin-data/<name>.json` unchanged (SC-006); a new version that loads **Broken** is swapped back and the error returned; leftover `.previous-<name>` with `<name>` missing is restored on start, and removed when `<name>` exists; `changed` is true after editing, adding or deleting a file (dot-files ignored); update with `changed` and without `replace_changed` refused; update with a SHA-256 other than the offered one refused
- [ ] T044 [P] [US3] Diff tests in putio-sync-plugins/src/registry/install.rs: `diff(old_manifest, new_manifest)` gives hosts, settings and events added and removed; for a plugin not installed everything is added
- [ ] T045 [P] [US3] Remove tests: a folder with a record is deleted and its settings kept; a folder without a record gives "not installed from the registry"
- [ ] T046 [P] [US3] Doctor test in putio-sync-doctor/src/checks/plugins.rs: an installed plugin whose cached entry is withdrawn warns with the reason; one missing from the cached index warns "No longer listed"; the check makes no registry request
- [ ] T047 [P] [US3] API tests in putio-sync-api/tests/plugins_registry.rs for `POST /plugins/registry/update` and `/remove` (401 without token, 409 and 422 cases from contracts/api.md)
- [ ] T048 [P] [US3] Node tests in putio-sync-api/tests/ui/plugins.test.mjs for `diffText(diff)` (one line per non-empty group, "No changes to hosts, settings or events" when all empty)

### Implementation

- [ ] T049 [US3] Implement `diff` and `changed(folder, record)` in putio-sync-plugins/src/registry/install.rs
- [ ] T050 [US3] Implement `update(dir, name, unpacked, expected_sha256, replace_changed)` in putio-sync-plugins/src/registry/install.rs: rename `plugins/<name>` to `plugins/.previous-<name>`, install the new one, reload that plugin (`PluginsService::reload_one`, new in putio-sync-plugins/src/service.rs), swap back if it is Broken, else delete `.previous-<name>` (research.md R6)
- [ ] T051 [US3] Implement `remove(dir, name)` in putio-sync-plugins/src/registry/install.rs and the `.previous-*` clean-up and restore on start in putio-sync-plugins/src/discover.rs
- [ ] T052 [US3] Fill `InstallState.update`, `withdrawn` and `state: unlisted` from the cached index in putio-sync-plugins/src/service.rs
- [ ] T053 [US3] Add `POST /registry/update` and `POST /registry/remove` to putio-sync-api/src/routes/system/plugins.rs per contracts/api.md
- [ ] T054 [US3] Warn on withdrawn and unlisted registry installs in putio-sync-doctor/src/checks/plugins.rs, from the service's cached index
- [ ] T055 [US3] Add `diffText` to putio-sync-api/ui/plugins.js
- [ ] T056 [US3] In putio-sync-api/ui/settings/plugins.js: **Update available** on the tab and line; **Update** calls preview, shows `diffText` and the release notes link, warns when `changed`, then calls update with the preview's `sha256`; **Remove** asks first then calls remove; the withdrawn reason and "No longer listed" on the tab
- [ ] T057 [P] [US3] Add the new UI strings to putio-sync-api/ui/i18n/en.json

**Checkpoint**: quickstart.md checks 9–11 pass.

## Phase 6: User Story 4 – Contribute a plugin to the registry (P2)

**Independent test**: following CONTRIBUTING.md with the example plugin gives a passing `registry.py check`; a wrong checksum, name or manifest fails with a clear message.

- [ ] T058 [P] [US4] Bring ~/dev/putio-sync-plugin-registry/scripts/registry.py in line with contracts/registry-index.md: release `url` must end `.tar.gz`, `.tgz` or `.zip` in `check_entry`; `withdrawn` ≤ 300 characters (done); run `check --offline examples/entries/*.toml`
- [ ] T059 [P] [US4] Add ~/dev/putio-sync-plugin-registry/tests/test_registry.py (stdlib `unittest`, archives built in the test, `download` replaced): good archive passes; wrong SHA-256, link, `..`, two top folders, manifest name/version/events/hosts mismatch each fail; run it in .github/workflows/check.yml
- [ ] T060 [US4] Run `putio-sync plugins check` (from T013) on ~/dev/putio-sync-plugin-registry/examples/webhook-notifier and examples/download-counter and fix any problem; then load both in a local daemon and use **Send test event** (quickstart.md check 2)
- [ ] T061 [US4] Ask the maintainer, then create the public repository `devclinton/putio-sync-plugin-registry`, commit (no Claude attribution) and push ~/dev/putio-sync-plugin-registry
- [ ] T062 [US4] Ask the maintainer, then create the `examples-v1.0.0` release with the archives whose SHA-256 values are in ~/dev/putio-sync-plugin-registry/examples/entries/*.toml (rebuild them and update the entries if those files are gone), and check them with `registry.py check examples/entries/*.toml` without `--offline`
- [ ] T063 [US4] After the first putio-sync release with `plugins check`, set the `PUTIO_SYNC_VERSION` repository variable so CI runs it (~/dev/putio-sync-plugin-registry/.github/workflows/check.yml)
- [ ] T064 [P] [US4] Link the registry's CONTRIBUTING.md from PLUGINS.md and README.md in the putio-sync repository

## Phase 7: Polish

- [ ] T065 [P] Document the registry in PLUGINS.md: More plugins, what "reviewed" means, install checks, updates, remove, the setting, and that every plugin needs host approval (FR-040)
- [ ] T066 [P] Translate the new strings with scripts/i18n-translate.mjs into every putio-sync-api/ui/i18n/*.json; putio-sync-api/tests/ui_i18n.rs passes
- [ ] T067 [P] CHANGELOG.md entry under Unreleased for links, `plugins check`, the registry, install and updates
- [ ] T068 [P] Mark 059 done in ROADMAP.md (move the brief to the shipped list in the file's usual way)
- [ ] T069 Run quickstart.md checks 1–11 against a local daemon and a local registry
- [ ] T070 Run `make ci` and fix anything it reports

## Dependencies

- Phase 1 → Phase 2 → stories.
- **US1** needs only Phase 2. It ships alone.
- **US2** needs Phase 2 and T009 (manifest fields, used to compare archives with entries) and T012 (`check_folder`); otherwise independent of US1's UI.
- **US3** needs US2 (install record, registry, routes).
- **US4**: T058–T059 can run any time; T060 needs T013; T061–T062 need the maintainer's OK; T063 needs a release containing T013.
- Polish after the stories it documents.

## Parallel examples

- Phase 3: T005, T006, T007 and T008 together; then T011, T016, T017 and T018 alongside T009–T015.
- Phase 4: T019–T025 together (each its own file); T039–T042 alongside T026–T038.
- Phase 5: T043–T048 together.
- Phase 6: T058 and T059 any time, in the other repository.

## Implementation strategy

1. **MVP**: Phases 1–3 (US1). Plugins carry links, bundled plugins fill them in, and authors get
   `plugins check`. This also unblocks the registry's examples (T060).
2. **Install**: Phase 4 (US2), checked against a local registry (quickstart.md 3–8).
3. **Updates**: Phase 5 (US3).
4. **Publish the registry**: Phase 6, with the maintainer's OK at T061 and T062.
5. Polish and `make ci`.
