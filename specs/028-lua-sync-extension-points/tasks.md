# Tasks: Lua Plugins, Sync Extension Points

**Input**: `specs/028-lua-sync-extension-points/` (plan.md, spec.md, research.md, data-model.md, contracts/)

**Tests**: requested by the spec (FR-012, independent tests per story).

## Phase 1: Setup

- [X] T001 Create `examples/plugins/` with a README index in examples/plugins/README.md

## Phase 2: Foundational (blocks all stories)

- [X] T002 Add `putio-sync-core/src/sync_hooks.rs`: `Hook` (`filter`|`rename`|`folder`, `as_str`, `FromStr`, `ALL`), `HookFile` (id, name, size, extension, folder, decision, reason; Serialize), `FilterAnswer`, `HookFuture`, trait `SyncHooks` (`provides`, `filter`, `rename`, `folder`), and `parse_filter`/`parse_name`/`parse_folder` ("name: not empty, not `.`/`..`, no `/` or `\`, at most 255 bytes"; "folder: no `..` segment, no leading `/`, empty segments dropped"), with unit tests; export from putio-sync-core/src/lib.rs
- [X] T003 Add `hook_timeout_ms` (default 500, "50 to 5000") with validation and `hook_timeout()` to putio-sync-core/src/plugins.rs; document in config.example.toml
- [X] T004 Add `hooks: Vec<String>` to `Manifest` in putio-sync-plugins/src/manifest.rs: known names only, no repeats, `events` may be empty when hooks are listed; `has_hook()`; manifest tests
- [X] T005 In putio-sync-plugins/src/worker.rs: `on_event` required only when `events` is not empty; each declared hook function required; `Job::Call` gains an optional per-call limit; successful hook calls stay out of the run history, failed ones are recorded
- [X] T006 Add putio-sync-plugins/src/hooks.rs: per-plugin hook slot + failure state (3 in a row → paused 5 minutes), `ask(hook, file)` over the slots in name order with `tokio::time::timeout(limit + 250 ms)`; implement `SyncHooks` for `PluginsService`
- [X] T007 Keep hook slots in `apply()` next to event slots and show `hooks` and `hook_timeout_ms` in `PluginStatus` in putio-sync-plugins/src/service.rs
- [X] T008 Add `set_sync_hooks(Arc<dyn SyncHooks>)` to `DownloadManager` in putio-sync-download/src/manager.rs and wire it in putio-sync-cli/src/commands/daemon.rs

## Phase 3: US1 custom file filter (P1) MVP

- [X] T009 [US1] Ask the filter in `enqueue_tree_inner` after `rules.decide` in putio-sync-download/src/manager.rs; `skip` → Blocked ("skipped by plugin <name>", never deleted from Put.io), `download`/`stream` override
- [X] T010 [P] [US1] Example examples/plugins/min-size-filter/ (plugin.toml, main.lua, README.md): skip video files below a configurable size
- [X] T011 [US1] Test in putio-sync-download/tests/sync_hooks.rs: fake `SyncHooks` skips a file, widens a blocked one; none → built-in
- [X] T012 [US1] Tests in putio-sync-plugins/tests/hooks.rs: error, timeout, bad answer fall back; pause after 3; hook-only plugin loads; missing hook function is Broken; events are not delayed

## Phase 4: US2 rename rule (P2)

- [X] T013 [US2] Ask rename in `download_file` (not for `fixed_path` or `save_as`) in putio-sync-download/src/manager.rs
- [X] T014 [P] [US2] Example examples/plugins/clean-names/: strip release tags after `SxxEyy` or the year
- [X] T015 [US2] Download test with a rename answer in putio-sync-download/tests/sync_hooks.rs

## Phase 5: US3 folder rule (P2)

- [X] T016 [US3] Ask folder in `download_file` before the pipeline redirect (not for `fixed_path`) in putio-sync-download/src/manager.rs
- [X] T017 [P] [US3] Example examples/plugins/tv-folders/: `TV/<Show>/Season NN` for episode names
- [X] T018 [US3] Download test with a folder answer in putio-sync-download/tests/sync_hooks.rs

## Phase 6: US4 writing one (P3)

- [X] T019 [US4] putio-sync-plugins/tests/examples.rs loads every folder in examples/plugins/ and checks each example's answers
- [X] T020 [US4] Settings tab shows "Sync rules: …" and a "Sync rule time limit (ms)" field for plugins with hooks in putio-sync-api/ui/settings/plugins.js; run `make i18n`
- [X] T021 [US4] PLUGINS.md: "Sync rules" section, the three functions, limits, fallback, the examples; replace the "Later" note

## Phase 7: Polish

- [X] T022 CHANGELOG.md entry; mark 028 done in ROADMAP.md
- [X] T023 `cargo fmt`, `cargo clippy` and tests for the touched crates

## Dependencies

Phase 2 before all stories. US2 and US3 share `download_file` (sequential). Examples (T010,
T014, T017) are independent of each other.

## Implementation Strategy

MVP is US1 (filter). US2/US3 add the download-time hooks; US4 docs and UI.

## Deviations

- T007: `PluginStatus` gains `hooks`; `hook_timeout_ms` reaches the UI through the plugin's
  settings, as `timeout_secs` does, not through the status.
- T012: "events aren't delayed" is by design (a second worker per plugin with hooks) and
  has no test of its own. A broken hook plugin shows as Broken after its first call,
  because workers build their Lua state on the first job.
- T020: the two new UI strings are in every catalog in English only; atranslate-cli is
  not installed here.
