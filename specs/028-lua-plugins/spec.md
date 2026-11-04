# Feature Specification: Lua Plugins

**Feature Branch**: `028-lua-plugins` (built on `claude/inspiring-davinci-dc6601`)

**Created**: 2026-11-04

**Status**: Draft

**Input**: User description: "028 Lua plugins: embedded, sandboxed Lua plugins on top of
the 027 event bus. (1) Event-handler plugins (MVP): Lua files in a plugins folder, each
with a manifest (name, version, events handled, permissions, typed config fields with a
secret flag); a sandbox with no `os`, `io` or native `require`; a time limit per call and
a memory limit per plugin; a small host API (log, config with secrets resolved by the host
and never logged, HTTP only to declared and approved hosts, a small key-value store per
plugin, time); a failing or slow plugin never stops a sync. (2) A Plugins settings group
(list, enable/disable, edit config, see permissions, send test event), doctor checks,
metrics for runs, failures and timeouts, reload without a restart. (3) Bundled example
plugins proving 030 notifications: ntfy and Discord notifiers that group repeats, Telegram
and Pushover if cheap; no SMTP in Lua. (4) Synchronous extension points (file filters,
rename rules, folder rules) only if they fit, otherwise split off and recorded here. The
host API and manifest are designed so 029 (WebAssembly plugins) can offer the same events
and API."

## Context

- 027 added one process-wide event bus (`download.*`, `transfer.*`, `sync.completed`,
  `storage.low`, `hook.test`), documented in `EVENTS.md`, and hooks (commands and
  webhooks) as its first consumer. Plugins are the second consumer of the same events, in
  the same JSON shape.
- 030 (notifications) and 031 (media-server refresh) are meant to be built as plugins, so
  the host API must be enough for a notifier: config with secrets, outbound HTTP, JSON,
  state that survives a restart, and a way to group repeats over time.
- 029 (WebAssembly plugins) comes later with "the same events and API". The manifest and
  the host API are therefore described in language-neutral terms (JSON-shaped values),
  and the manifest names its runtime and entry file.
- An unconfigured daemon runs in setup mode, where every route except the wizard answers
  503.

## Clarifications

### Session 2026-11-04

Answered with the recommended option, without a live user; listed in the hand-over note
for review.

- Q: Are the synchronous extension points (file filters, rename rules, folder rules) in
  this feature? → A: No. They are split into a follow-up (kept in `ROADMAP.md` under 028).
  They run inside the sync path, which is synchronous code shared by the CLI and the
  daemon, and each needs its own fallback rules, UI to pick a script per rule, and tight
  limits. The event-handler runtime built here (sandbox, limits, manifest, host API) is
  what they will reuse.
- Q: What is a plugin on disk? → A: A folder holding a `plugin.toml` manifest and the
  entry file it names (`main.lua`). One folder per plugin, so 029 can use the same manifest
  with `runtime = "wasm"` and a `.wasm` entry.
- Q: How does the user approve a plugin's network access? → A: Enabling a plugin approves
  the hosts it would contact at that moment: the hosts listed in the manifest plus the
  hosts of any URL config fields the manifest names. The approved list is stored with the
  plugin's settings. A request to any other host is refused. If a later manifest or config
  change needs a host that is not approved, requests to it are refused, the plugin's
  status says "needs approval", and the doctor warns, until the user saves the plugin again.
- Q: How do notifiers group repeats when the host only calls them on events? → A: The host
  also calls an optional `on_tick` handler about once a minute, and a plugin's Lua state
  lives between calls, so a plugin can hold a batch in memory and flush it on a tick. The
  bundled notifiers send the first event of a kind at once and fold the rest that arrive
  within a window (default 5 minutes) into one summary message.
- Q: Which bundled notifiers? → A: ntfy, Discord, Telegram and Pushover. Each is one
  HTTP POST with a small JSON or form body, so the last two cost little once the first two
  exist. Email stays out of Lua (030 does it natively).
- Q: Do `init` and `on_tick` calls have the same limits as event calls? → A: Yes: the
  same time and memory limit, recorded as runs (event type `tick` or `init`) in the history
  and metrics, so a plugin that misbehaves on ticks is as visible as one that misbehaves on
  events.
- Q: What happens to a plugin's stored data when it is disabled or its folder removed? →
  A: It is kept. Disabling stops calls only; the store file stays in the data folder until
  the user deletes it, so re-enabling or reinstalling a plugin picks up where it left off.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Get a phone notification when a download lands (Priority: P1)

A user wants a push notification on their phone through ntfy when a file finishes
downloading or fails. They open Settings › Plugins, see the bundled "ntfy" plugin, fill in
the server and topic (and an access token, if their topic is protected), see that it will
contact `ntfy.sh`, enable it, and click "Send test event". A notification arrives. From then
on, each finished or failed download sends one; when twenty finish within a few minutes,
they get the first one and then a single "and 19 more downloads finished" message.

**Why this priority**: it is the case 030 needs and proves the whole runtime: manifest,
config with a secret, approved HTTP, state between calls, and ticks.

**Independent Test**: point the ntfy plugin at a local test server, enable it, send ten
`download.finished` events within a minute, advance a tick past the window; the server
receives one message for the first event and one summary naming the other nine.

**Acceptance Scenarios**:

1. **Given** the bundled ntfy plugin enabled with a topic, **When** a download finishes,
   **Then** the ntfy server receives one message naming the file, within a few seconds.
2. **Given** more downloads of the same kind finish within the grouping window, **Then**
   no message is sent for each; on the first tick after the window ends one summary is
   sent with the count and the first few names.
3. **Given** a protected topic with an access token, **Then** the token is sent as the
   server expects and never appears in the log, the UI after saving, the doctor, the API
   or the metrics.
4. **Given** the ntfy server is down, **Then** the plugin run is recorded as failed with a
   short error, the download stays finished, and the next event is tried again.

---

### User Story 2 - Write my own plugin (Priority: P1)

A user who knows a little Lua wants to react to events in a way no bundled plugin does,
for example post to a home-automation URL only for big files. They create a folder in the
plugins folder with a `plugin.toml` (name, version, events, the hosts it calls, its config
fields) and a `main.lua` that returns a table with an `on_event` function. They click
"Reload" in Settings › Plugins; the plugin appears, disabled, with its permissions listed.
They enable it, and it runs for each event it declared.

**Why this priority**: the feature's purpose is user-written logic; the bundled plugins are
built the same way.

**Independent Test**: write a plugin that stores each event's id in its key-value store;
reload; enable; emit three events; the store holds three ids, and they survive a reload.

**Acceptance Scenarios**:

1. **Given** a valid plugin folder, **When** the user reloads, **Then** the plugin is
   listed with its name, version, description, events, permissions and config fields,
   disabled until the user enables it.
2. **Given** an invalid manifest (missing name, bad name characters, unknown event type,
   unknown permission, bad config field type, missing entry file) or a script that does
   not load, **Then** the plugin is listed as broken with a message naming the file and
   the problem, and other plugins load normally.
3. **Given** a script that reads `os`, `io`, `debug`, `package`, `require`, `load` of
   binary chunks, `dofile` or `loadfile`, **Then** these are not available and the call
   fails with an error that says so.
4. **Given** `putio.log.info("hi", { size = 10 })`, **Then** the daemon log shows the line
   with the plugin's name and the fields.
5. **Given** a running daemon, **When** the user edits a plugin's script and clicks
   Reload, **Then** the next event runs the new script, without a restart and without
   stopping downloads.

---

### User Story 3 - A broken plugin never breaks the sync (Priority: P1)

A user installs a plugin with a bug: an infinite loop, a huge table, an error on every
call, or a slow HTTP call. Downloads, sync runs and other plugins carry on as before; the
bad plugin's runs are recorded as timeouts or failures, and the doctor points at it.

**Why this priority**: the same rule as hooks; a plugin runs in the daemon's process, so
the limits must hold.

**Independent Test**: enable a plugin whose `on_event` loops forever; emit events while
a download runs; the download finishes in normal time, each plugin call ends as a timeout
within the plugin's limit, and the timeout counter rises.

**Acceptance Scenarios**:

1. **Given** a handler that loops forever, **Then** the call is stopped after the plugin's
   time limit (default 10 s) and recorded as a timeout; the next event runs normally.
2. **Given** a handler that allocates without end, **Then** the call fails with an
   out-of-memory error at the plugin's memory limit (default 32 MiB), the plugin's state is
   reset by reloading it, and the daemon's memory is not exhausted.
3. **Given** a handler that raises an error, **Then** the run is recorded as failed with
   the error message (secrets masked) and the plugin keeps running for later events.
4. **Given** a slow plugin and many events, **Then** its queue holds at most 1 000
   events, the oldest are dropped and counted, and other plugins and hooks are not slowed.
5. **Given** a plugin that fails to load, **Then** the daemon starts and runs normally.

---

### User Story 4 - Manage plugins from Settings (Priority: P2)

A user opens Settings › Plugins and sees every plugin found (bundled and their own) with
its state (enabled, disabled, broken, needs approval), version, events, the hosts it may
contact, and its last runs. They can enable or disable it, edit its config (secret fields
masked once saved), send it a test event and see the result, and reload all plugins.

**Why this priority**: makes the feature usable without editing files.

**Acceptance Scenarios**:

1. **Given** the Plugins group, **Then** each plugin shows its name, version,
   description, state, events, network hosts, config fields with their types and help
   text, and its last runs (time, event, result, duration, error).
2. **Given** a config value of the wrong type, a missing required field, or a URL field
   that is not http or https, **When** the user saves, **Then** the save is refused naming
   the plugin and the field, and nothing is written.
3. **Given** a secret field, **Then** after saving it shows as set, with a way to replace
   it, and the real value never comes back from the API or the export.
4. **Given** "Send test event", **Then** only that plugin receives one `plugin.test`
   event, enabled or not, and the result (success, failure or timeout, duration, the
   error, and the lines it logged) shows straight away.
5. **Given** "Reload", **Then** all plugins are read from disk again; plugins whose name
   is unchanged keep their queue, run history and key-value store.

---

### User Story 5 - See plugins in the doctor and in monitoring (Priority: P3)

A user wants a broken plugin noticed without opening Settings. The doctor has a Plugins
check and the metrics endpoint counts plugin runs, failures, timeouts and dropped events.

**Acceptance Scenarios**:

1. **Given** an enabled plugin that is broken (manifest or script error), **Then** the
   doctor reports a problem naming it and the error.
2. **Given** an enabled plugin that needs approval, misses a required config value, or
   whose recent runs failed or timed out, **Then** the doctor warns, naming it.
3. **Given** no enabled plugins, **Then** the check passes and says so.
4. **Given** the metrics endpoint, **Then** it has counters of plugin runs by plugin,
   event type and result (success, failure, timeout), a run-time histogram per plugin, and
   a counter of events dropped per plugin.

### Edge Cases

- **Two plugins with one name**: a user plugin with the same name as a bundled one
  replaces it (logged once); two user folders with the same manifest name are both listed
  as broken ("duplicate name").
- **Plugins folder missing**: no user plugins; bundled ones are still listed. The folder
  is not created until the user asks (it is documented).
- **Order**: events reach one plugin in the order they were emitted, one call at a time;
  plugins run independently of each other and of hooks.
- **Ticks**: `on_tick` is only called for plugins that define it, about every 60 seconds,
  queued like an event (a slow plugin gets ticks late, never two at once).
- **Shutdown**: queued events and ticks are not delivered; nothing is kept across restarts
  except the key-value store, which is also kept when a plugin is disabled or removed.
- **Key-value store limits**: keys up to 128 bytes, values are strings, numbers, booleans
  or tables that convert to JSON, at most 64 KiB per value and 1 MiB per plugin; a write
  past a limit fails with an error the script can catch.
- **HTTP limits**: only `http` and `https`; one request at a time per plugin (a plugin
  runs one call at a time); each request ends within the call's remaining
  time; redirects only to the same host; response bodies over 1 MiB are cut and flagged.
- **Secrets in errors**: HTTP errors are built from the status or the kind of error and
  the request's host, never from the full URL, since a Discord webhook URL is itself a
  secret.
- **Setup mode**: the plugin routes answer 503 like every other route.
- **One-off CLI commands**: plugins do not run (same rule as hooks).
- **Wildcard**: `events = ["*"]` receives every event type, including types added later;
  `plugin.test` reaches every plugin through the test button only.
- **Settings import/export**: plugin settings are part of the settings file; secret values
  are masked on export and a mask keeps the current value on import, refused when there is
  none.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The daemon MUST load plugins from a plugins folder (default `plugins` next to
  the config file, configurable) and from a set of plugins bundled into the program.
- **FR-002**: A plugin MUST be a folder with a `plugin.toml` manifest holding: `name`
  (letters, digits, `-`, `_`, up to 40 characters), `version`, optional `description`,
  `runtime` (`lua`; `wasm` reserved for 029), `entry` (default `main.lua`), `events` (event
  types or `*`), `permissions`, and `config` fields.
- **FR-003**: Manifest permissions MUST be: `http` with a list of `hosts` and a list of
  `host_from_config` (names of URL config fields whose host is allowed), and `store`
  (the key-value store). A plugin MUST NOT be able to use an API whose permission it did
  not declare.
- **FR-004**: Config fields MUST each have a key, a `type` (`string`, `integer`, `number`,
  `boolean`, `url`, `string_list`), optional `default`, `required`, `secret`, `label` and
  `help`; the host MUST validate stored values against them.
- **FR-005**: Plugin settings (enabled, approved hosts, timeout, memory limit, config
  values) MUST be stored in the main config file, and saves from Settings MUST apply
  without a restart.
- **FR-006**: Enabling a plugin from Settings MUST record the hosts it would contact at
  that moment as approved; requests to other hosts MUST be refused, and a plugin needing
  an unapproved host MUST show "needs approval".
- **FR-007**: Lua MUST run embedded, built from source into the program (no system
  library), with only the safe standard libraries (`string`, `table`, `math` and `utf8`)
  and without `os`, `io`, `debug`, `coroutine` (a coroutine escapes the time limit),
  `package`, `require`, `dofile`, `loadfile`, or loading binary chunks.
- **FR-008**: Each call into a plugin MUST stop at the plugin's time limit (default 10 s,
  1 s to 60 s), counting time spent in host calls, and be recorded as a timeout.
- **FR-009**: Each plugin MUST have a memory limit (default 32 MiB, 4 MiB to 256 MiB);
  passing it fails the call, and the plugin is reloaded fresh before the next call.
- **FR-010**: Each plugin MUST have its own Lua state, its own queue of at most 1 000
  events (drop oldest, logged and counted), and run one call at a time on its own thread,
  so a plugin never blocks the sync, downloads, hooks or another plugin.
- **FR-011**: The entry script MUST return a table; the host MUST call its `on_event(event)`
  for each matching event (the event as a table in the 027 JSON shape), its `on_tick()`
  about every 60 seconds if defined, and its `init()` once after loading if defined.
- **FR-012**: The host API MUST be a `putio` table with: `log.debug/info/warn/error(message,
  fields)`, `config` (read-only values, secrets included), `http.request(options)`,
  `store.get/set/delete/keys`, `time.now()` and `time.iso(seconds)`, `json.encode/decode`,
  and `plugin.name`, `plugin.version`, `version` (putio-sync's).
- **FR-013**: `putio.http.request` MUST take `method`, `url`, `headers`, `body`, `timeout`
  and return `status`, `headers`, `body`, or `nil` and an error message; only approved
  hosts, http(s) only, same-host redirects only, bodies cut at 1 MiB.
- **FR-014**: Secret config values MUST NOT appear in logs, run errors, the API, the doctor,
  metrics or exports; text a plugin logs and its errors MUST have its secret values masked.
- **FR-015**: Every call MUST be logged with the plugin name, event type and id, result
  (success, failure, timeout) and duration; failures with the error.
- **FR-016**: Settings MUST have a Plugins group to list, enable, disable, configure and
  test plugins and to reload them; the API MUST list plugins with state, manifest
  summary, approved hosts and last 20 runs, without secret values.
- **FR-017**: "Send test event" MUST send one `plugin.test` event (with the plugin's name
  and a message) to that plugin only and return its result with the lines it logged.
- **FR-018**: Reload MUST re-read every plugin from disk without a restart; a plugin
  keeping its name keeps its queue, history and store.
- **FR-019**: The doctor MUST have a Plugins check: broken enabled plugins are a problem;
  needing approval, missing required config, and recent failures or timeouts are warnings.
- **FR-020**: Metrics MUST include `putio_sync_plugin_runs_total{plugin,event,result}`,
  `putio_sync_plugin_run_duration_seconds{plugin}` and
  `putio_sync_plugin_events_dropped_total{plugin}`.
- **FR-021**: The event schema MUST gain `plugin.test`, added without changing the schema
  version.
- **FR-022**: The program MUST bundle ntfy, Discord, Telegram and Pushover notifier
  plugins, disabled by default, each sending finished and failed downloads, failed
  transfers, failed sync runs and low storage (events chosen by a config field), grouping
  repeats of one event type within a window into one summary.
- **FR-023**: The README, both example configs, `EVENTS.md` and a plugin guide MUST
  describe plugins, the manifest, the host API, the sandbox and its limits, and the
  bundled plugins; the guide MUST describe the API in terms 029 can offer unchanged.
- **FR-024**: Plugins MUST run in the long-running server and the `daemon` command, not in
  one-off CLI commands.

### Key Entities

- **Plugin manifest**: name, version, description, runtime, entry, events, permissions,
  config fields.
- **Plugin settings**: name, enabled, approved hosts, timeout, memory limit, config values.
- **Plugin**: manifest, source (bundled or folder), state (enabled, disabled, broken,
  needs approval), queue, run history, store.
- **Plugin run**: plugin, event type and id (or `tick` / `init`), start time, duration, result,
  error, logged lines (up to 20, test runs only).
- **Plugin store**: string keys to JSON values, per plugin, kept on disk.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: With a plugin that loops forever, downloads and sync runs finish in the same
  time as with no plugins, and each of its calls ends within its time limit plus 1 second.
- **SC-002**: A user can enable the bundled ntfy plugin and receive a test notification in
  under two minutes, without a restart.
- **SC-003**: Twenty matching events within the grouping window produce at most two
  messages from a bundled notifier.
- **SC-004**: No test, log line, API answer, export or metric contains a secret config
  value.
- **SC-005**: A plugin that fails to load or crashes leaves every other plugin, hook and
  sync running.

## Assumptions

- Plugin settings live in `[[plugins]]` tables in the config file (like `[[hooks]]`); the
  UI writes the same tables. A `plugins_dir` key sets the folder.
- Secret values are stored in the config file like hook headers; reading them from the
  environment or a secrets file is not part of this feature.
- The store is a JSON file per plugin in the daemon's data folder; it is not part of the
  settings export.
- Run history is kept in memory (last 20 per plugin), like hooks.
- Plugins cannot run commands, read or write files, or open sockets other than through
  `putio.http.request`.
- The time limit is wall-clock time per call; the instruction-count hook enforces it in
  Lua code and the remaining time bounds each HTTP request.
- No plugin installation from a URL or marketplace (029 adds install from a file or URL).
- The synchronous extension points (file filters, rename rules, folder rules) are a
  follow-up, kept in the roadmap under 028.
