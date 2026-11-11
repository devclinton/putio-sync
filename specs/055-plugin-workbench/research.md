# Research: Plugin Workbench

## R1 Where a run happens

- **Decision**: a Run panel run builds a fresh state on a one-off worker, the way settings actions
  and health checks already do (`call_once`), with a `Tap` in its `Env`. The worker calls one
  function through `Job::Call`, then stops.
- **Rationale**: the live worker keeps its state and queue untouched, so a run neither waits for
  queued events nor delays them; plugins that are off or broken in their settings can still be
  run; unsaved editor text needs a fresh state anyway; Lua and WebAssembly share `Job::Call`, so
  both runtimes work with no runtime-specific code.
- **Alternatives**: a new job on the live worker (a run would wait behind the queue and share
  live state, and a dry run would have to swap the store and HTTP client of a live state); a
  separate runner beside the worker (duplicates the build and call code).
- One run per plugin at a time, through the busy guard actions already use.

## R2 Dry-run seams

- HTTP: `host::request` is the single path for `putio.http.request` in Lua and WebAssembly. With a
  tap it checks the host as usual, then either records the request and answers with the run's
  reply (default status 200, empty body) or sends it and records the status and time.
- Hosts: a dry run may call every host the manifest needs (nothing leaves the machine); a real
  run only the approved ones, as a live event.
- Files: `Files::write` is the single write path. With a dry tap it checks the path and size as
  usual, then records the write (path and size) instead of writing.
- Store: a dry run opens a scratch `Store` (no file) filled from a snapshot of the real one; a
  real run uses the plugin's shared store. The result has both snapshots.
- Metrics and history: a worker with a tap does not count runs or keep them in the plugin's run
  list (FR-009).
- Log lines: the cap is 20 for live runs; a workbench worker thread raises it to 200 with a
  thread-local limit, since the worker thread is its own.
- Secrets: the plugin's secret values are scrubbed from errors and log lines (as today), and from
  recorded URLs, headers, bodies and store snapshots. `Authorization`, `Cookie`, `Proxy-Authorization`
  and headers whose name contains `token`, `key` or `secret` are masked whole.

## R3 Memory

- Lua: `Lua::used_memory()` after the call. WebAssembly: the size of the module's linear memory
  (`Memory::data_size`). Shown as "memory in use", not a peak: neither runtime keeps a peak, and
  the limits are what stop a run.

## R4 The error line

- Lua errors already read `main.lua:12: message` (the chunk is named `@main.lua`); syntax errors
  from a check read the same. The UI finds `<file>:<line>:` and links the line in the editor. A
  WebAssembly trap has no source line; the result shows the trap text.

## R5 Writing plugin files

- **Decision**: `source.rs` writes each file to `<file>.tmp` in the same folder, renames the
  current file to `<file>.bak`, then renames the temporary file into place; the folder is created
  under the plugins folder only, from a checked name. Save carries the version (length and
  modified time) read at open; a mismatch answers 409 unless `force` is set.
- A save then calls `reload_one(name)`: rediscover, and update only that plugin's workers, so the
  other plugins keep their state.
- Check: `Manifest::parse(text, label)` gives the same `file: field: message` text as a reload;
  Lua syntax is checked by compiling the script (`Lua::load(..).into_function()`) in a fresh
  sandboxed state without running it.
- Bundled plugins are concatenations of shared Lua and their `main.lua`; **Copy to edit** writes
  that full text as `main.lua`, so the copy works on its own.

## R6 Starters

- **Decision**: the starters live in `putio-sync-plugins/starters/` and are built in with
  `include_str!`; `examples/plugins/<name>/` has the same two files plus a README, and a test
  checks the files are equal and that each example loads and passes a dry run.
- **Rationale**: the Docker build copies crate folders only, so the binary cannot read
  `examples/` at build time. The five existing examples are sync hooks; none is an event, store
  or tick plugin, so the three starters are new.

## R7 Docs links

- The docs book is published at `https://devclinton.github.io/putio-sync/docs/` from the latest
  release tag, so it matches released builds. The helper is `docs(page, anchor, text)` in the
  lazy module `ui/workbench.js`; the one static link (the hooks card's EVENTS.md) is written out
  where it is used.
- Targets: `plugin-workbench.html` (the guide), `plugins.html` (reference, with anchors for the
  manifest, the `putio` API and sync hooks), `events.html#<type>` for the picked event type.

## R8 UI budget

- Static UI: 499,691 B of 499,712 (21 B left). The workbench (Run panel, editor, starters,
  toggle, WebAssembly view, links) is `settings/workbench.js`, imported when the "Plugin
  workbench" panel is opened. It also holds the install form, so the static Plugins card swaps
  one lazy panel for another. The hooks card link and the panel are paid for by trimming
  comments in static files; the size test checks the result.
