# Research: Lua Plugins, Sync Extension Points

## R1 Where the interface lives

- **Decision**: `putio-sync-core::sync_hooks` holds `Hook`, `HookFile`, `FilterAnswer`, the
  `SyncHooks` trait (boxed futures, like `PathRedirect`) and the answer checks
  (`parse_filter`, `parse_name`, `parse_folder`) that work on JSON values.
- **Rationale**: `download` calls it and `plugins` implements it; neither depends on the other.
  029's WebAssembly runtime returns JSON too and reuses the checks.
- **Alternatives**: trait in `download` (would make `plugins` depend on `download`); a Lua-typed
  interface (ties 029 to Lua).

## R2 Which worker runs a hook

- **Decision**: a second `Slot` per enabled plugin that declares hooks, built from the same
  `Spec`, sharing the plugin's store and counters. Hook calls are `Job::Call` with a per-job
  limit.
- **Rationale**: event jobs can be queued 1 000 deep at 10 s each; a sync answer must not wait
  behind them (FR-009). The worker machinery (limit, rebuild after timeout or memory error,
  scrubbing) is reused as it is.
- **Alternatives**: one Lua state for both (queueing); a fresh state per call (re-runs the
  script and `init` for every file, too slow).

## R3 Waiting for the answer

- **Decision**: the caller waits on the job's oneshot with `tokio::time::timeout(limit + 250 ms)`.
  The worker's own deadline stops the Lua code at the limit; the extra wait covers a call
  queued behind another one. A missed wait counts as a timeout.
- **Rationale**: the sync never waits more than about one limit per plugin per file.

## R4 Failure pause

- **Decision**: per plugin, a count of failures in a row; at 3 the hooks are skipped for 5
  minutes, logged once at warn. A success resets the count; a settings change or reload makes
  new slots, which start clean.
- **Rationale**: SC-002; a dead network host must not add 0.5 s to each of 5 000 files.

## R5 Several plugins

- **Decision**: name order; the first plugin that gives a valid non-empty answer wins.
- **Rationale**: predictable and documented; chaining renames is not asked for.

## R6 When rename and folder run

- **Decision**: in `download_file`, before the pipeline redirect: the folder answer replaces
  `relative_path` as the local folder (the redirect then stages inside it), and the rename
  answer replaces the name unless `save_as` is set. `fixed_path` (Radarr/Sonarr) skips both.
  Folder rules (`on_existing`, delete after download) still resolve by the Put.io folder.
- **Rationale**: the download's real path is recorded from `target_path`, so the rest of the
  code (history, delete-after-download, pipeline) follows. The queue keeps Put.io's folder,
  which the rules and the arr matching use.
- **Alternatives**: decide at enqueue and store in the queue (changes what `relative_path`
  means for rules, transfers and the pipeline).

## R7 What the filter sees

- **Decision**: the filter is asked for every file the built-in rules decided on (download,
  stream, blocked or skipped), after the release gate and before the queue/downloaded checks.
  A `skip` makes it `Blocked` with reason "skipped by plugin <name>"; `download` and `stream`
  override. Blocked-by-plugin files are never deleted from Put.io by `delete_blocked`.
- **Rationale**: plugins can both narrow and widen; deleting remote files on a plugin's word
  is too risky.

## R8 Preview and dry run

- **Decision**: left out; documented.
- **Rationale**: a preview of a big tree would call plugins per file on a request path.
