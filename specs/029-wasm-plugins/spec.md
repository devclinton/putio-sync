# Feature Specification: WebAssembly Plugins

**Feature Directory**: `specs/029-wasm-plugins`
**Created**: 2026-11-10
**Status**: Implemented
**Input**: Roadmap 029. Sandboxed plugins compiled to WebAssembly, so they can be written in
Go (TinyGo), Rust, JavaScript and other languages. They get the same events and API as the Lua
plugins, including 028's sync hooks (filter, rename, folder). Permissions are set per plugin
(network, file paths). Plugins can be installed from a file or a URL. Example plugins in Rust
and TinyGo ship under `examples/plugins/`.

## User Scenarios & Testing

### User Story 1 - Run a plugin written in another language (Priority: P1)

A user who writes Rust or Go builds a plugin to a `.wasm` file, puts it in a plugin folder with
a `plugin.toml` that says `runtime = "wasm"`, and turns it on in Settings › Plugins. It
receives the same events as a Lua plugin, can log, call approved hosts, keep values in its
store, run on a tick and answer the sync rules (filter, rename, folder).

**Why this priority**: The whole feature exists so plugins are not tied to Lua.

**Independent Test**: Load the shipped Rust and Go examples, send them events and sync-rule
questions, and check their answers and log lines.

**Acceptance Scenarios**:

1. **Given** a WebAssembly plugin that handles `download.finished`, **When** a download
   completes, **Then** the plugin runs and its run (result, time taken, logs on test) shows in
   Settings exactly as for a Lua plugin.
2. **Given** a WebAssembly plugin that declares the `filter` hook, **When** the sync asks
   about a file, **Then** its answer is used, and an error, a timeout or a bad answer falls
   back to the built-in rule, with the same 3-failures pause as Lua.
3. **Given** a plugin that loops forever or grows its memory past the limit, **When** it runs,
   **Then** the call stops at the plugin's time or memory limit, the run reads
   "time limit reached" or names the memory limit, and the next job starts a fresh instance.
4. **Given** a broken `.wasm` file (not WebAssembly, missing a declared function, an import
   the host does not offer), **When** plugins load, **Then** the plugin shows as Broken with
   a message naming the problem and the other plugins run.

### User Story 2 - Decide what each plugin may touch (Priority: P1)

A user sees, for each plugin, which hosts it may call and which folders it may read or write.
Turning the plugin on and saving approves exactly those; a plugin cannot reach any other
host, folder or file, whatever language it is written in.

**Why this priority**: Plugins come from strangers; the sandbox is what makes installing one
safe.

**Independent Test**: A plugin asks to read a file outside its approved folders and gets an
error; inside them it succeeds; before approval the plugin waits in "Needs approval".

**Acceptance Scenarios**:

1. **Given** a manifest that asks to read `{download_dir}` and write a folder set in its
   settings, **When** the user enables it and saves, **Then** both folders are approved and
   listed on the plugin; a later change of the setting needs a new save before it applies.
2. **Given** an approved read folder, **When** the plugin reads `../` out of it or follows a
   link that leads out of it, **Then** the read is refused.
3. **Given** a plugin with no file permission, **When** it asks for any file, **Then** it is
   refused; WebAssembly plugins get no other way to the disk, clock, environment or network.

### User Story 3 - Install a plugin from a file or a URL (Priority: P2)

A user installs a plugin from Settings › Plugins by choosing its files (a `plugin.toml` and
its entry, or one `.wasm` that carries its manifest) or by pasting a URL. The plugin appears
off, with its permissions listed, ready to be checked and turned on.

**Why this priority**: Copying folders onto a NAS is the main hurdle for most users, but
plugins still work without this.

**Independent Test**: Install the Rust example from its file and from a URL served by a
test server; it shows up disabled; installing it again is refused unless "replace" is asked.

**Acceptance Scenarios**:

1. **Given** a URL to a `plugin.toml`, **When** the user installs it, **Then** the manifest and
   the entry next to it are fetched, checked and written to `<plugins folder>/<name>/`.
2. **Given** a plugin of that name is already installed, **When** the user installs again
   without replacing, **Then** nothing is written and the message says so.
3. **Given** a file that is not a valid plugin (bad manifest, entry too large, not
   WebAssembly for a wasm runtime), **When** the user installs it, **Then** nothing is written
   and the message names the problem.

### User Story 4 - Learn from working examples (Priority: P3)

A plugin author finds a Rust and a Go example under `examples/plugins/`, each with build
steps and linked from PLUGINS.md, and the test suite loads the checked-in Rust `.wasm` and builds and loads the Go one
(when Go is installed), so the examples cannot rot.

**Acceptance Scenarios**:

1. **Given** the repository, **When** the tests run, **Then** both examples load and answer
   an event and a sync rule.

### Edge Cases

- A `.wasm` larger than the size cap is refused at load and at install.
- A plugin that imports WASI calls for files, sockets or the environment gets "not
  supported" answers, never real access.
- A plugin that exits (`proc_exit`) or traps fails that run; the next job starts fresh.
- A host call made after the call's time limit fails with "time limit reached".
- Install with no plugins folder configured is refused with a message.
- An install URL that redirects to another host, or answers with an error, writes nothing.

## Requirements

### Functional Requirements

- **FR-001**: A manifest with `runtime = "wasm"` MUST load a WebAssembly module from `entry`
  (default `plugin.wasm`) through the same discovery, manifest checks, settings, states,
  event dispatch, ticks, tests, actions, health checks, sync hooks and run history as Lua.
- **FR-002**: The host API offered to WebAssembly MUST cover host API v1 (log, config,
  http, store, time, json, path, metrics, download_dir, plugin, version, api_version) with
  the same rules: secrets scrubbed, approved hosts only, size and count limits.
- **FR-003**: Each WebAssembly call MUST be bound by the plugin's time limit (the hook limit
  for sync rules) and its memory limit; going over stops the call and the instance is rebuilt.
- **FR-004**: Manifests MUST be able to ask for file access: folders to read and folders to
  write, fixed or taken from a config field. Enabling and saving approves the folders needed;
  unapproved folders put the plugin in "Needs approval".
- **FR-005**: File calls MUST be limited to approved folders after resolving `..` and links,
  with a size cap per read and write. Lua plugins MUST get the same file calls.
- **FR-006**: WebAssembly plugins MUST get no host access beyond the host API: WASI calls
  other than clocks, random numbers and writing to stdout/stderr (kept as log lines) answer
  "not supported".
- **FR-007**: The API and Settings MUST install a plugin from uploaded files or from a URL,
  disabled, refusing to overwrite an existing folder unless asked to replace it, and reload.
- **FR-008**: Example plugins in Rust and Go (TinyGo-compatible) MUST ship with build steps
  and a test that loads them: the Rust `.wasm` is checked in, the Go one is built by the
  test when Go is installed.
- **FR-009**: New UI strings MUST be in the i18n catalogs; the UI MUST stay under 488 KiB.

### Key Entities

- **WebAssembly plugin**: a plugin folder whose manifest says `runtime = "wasm"` and whose
  entry is a module that exports the plugin's functions under the plugin ABI.
- **File permission**: read and write folder lists in the manifest, fixed or from config
  fields, and the approved folders in the plugin's settings.
- **Plugin package**: what an install takes: a manifest plus entry, or one `.wasm` with an
  embedded manifest, from uploaded bytes or a URL.

## Success Criteria

- **SC-001**: Both shipped examples (Rust, Go) load and pass their event and sync-rule tests
  on every test run.
- **SC-002**: A runaway WebAssembly plugin is stopped within its time limit plus 0.5 s, and a
  memory-hungry one at its memory limit, without affecting other plugins.
- **SC-003**: No WebAssembly or Lua plugin can read or write outside its approved folders in
  the permission tests (`..`, absolute paths, links).
- **SC-004**: Installing a plugin takes one action in Settings and leaves it off until enabled.
- **SC-005**: The release binary grows by no more than 3 MB and every release target still
  builds (no C or JIT toolchain needed).

## Assumptions

- The plugin ABI is putio-sync's own (JSON in and out through linear memory) rather than the
  component model; see research.md.
- JavaScript plugins need a JS engine compiled to WebAssembly (such as Javy's output); that
  works under this ABI but no JavaScript example ships here.
- Install fetches over http or https with the same size caps; there is no plugin catalogue.
- Approval stays the existing "enable and save" step; there is no per-folder prompt.
