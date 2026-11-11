# Feature Specification: Lua Plugins, Sync Extension Points

**Feature Branch**: built on `claude/022-stream-mode-dlna-4b51d8` (no branch of its own)

**Created**: 2026-11-10

**Status**: Draft

**Input**: User description: "028 Lua plugins: sync extension points. Plugins that decide
things during a sync, called synchronously with a short time limit: custom file filters, rename
rules and folder rules. Reuse the existing plugin manifest, sandbox and putio API (PLUGINS.md).
A plugin that fails or is slow falls back to the built-in rule. Ship a working example plugin
for each extension point under examples/plugins/, linked from PLUGINS.md and loaded by a test.
Keep the extension-point interface independent of the Lua runtime so 029 (WebAssembly) can
implement it."

## Context

- Event-handler plugins exist (spec `028-lua-plugins`): a folder with `plugin.toml` and a Lua
  script, run in a sandbox on its own thread for events such as "download finished". They never
  hold up a sync, because they only react after the fact.
- The built-in rules decide three things for each file a sync finds on Put.io:
  1. **Whether** it is downloaded: folder rules, patterns and the type filter give "download",
     "stream" (keep on Put.io), "blocked" or "skipped".
  2. **What it is called** locally: the Put.io name, made safe for the filesystem.
  3. **Which folder** it lands in: the same path below the download folder as on Put.io.
- Some households want rules the built-in settings cannot express: "skip samples under 50 MB",
  "strip the release group from names", "put each show in `TV/<Show>/Season NN`". These need a
  plugin that is asked during the sync and whose answer is used.
- WebAssembly plugins (029) come next and are to answer the same questions, so the questions
  and their answers are defined apart from Lua.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - A custom file filter (Priority: P1)

The operator installs a filter plugin and turns it on in Settings › Plugins. On the next sync
the plugin is asked about each file the sync finds, sees what the built-in rules decided, and
can say "download", "stream", "skip" or nothing (keep the built-in decision). Files it skips
are not queued.

**Why this priority**: Deciding what to fetch is the most common need, and it exercises the
whole path: manifest, call during the sync, time limit, fallback.

**Independent Test**: With a mock Put.io holding `movie.mkv` (2 GB) and `sample.mkv` (10 MB)
and the example size filter on with a 50 MB minimum, a sync pass queues only `movie.mkv` and
reports `sample.mkv` as blocked.

**Acceptance Scenarios**:

1. **Given** no plugin declares a filter, **When** a sync runs, **Then** decisions are exactly
   the built-in ones.
2. **Given** a filter plugin that returns "skip" for a file the rules would download, **When**
   a sync runs, **Then** the file is not queued and counts as blocked, with the plugin named
   as the reason.
3. **Given** a filter plugin that returns nothing for a file, **Then** the built-in decision
   stands.
4. **Given** a filter plugin that returns "download" for a file the type filter blocked,
   **Then** the file is queued.
5. **Given** a filter plugin that raises an error, runs past its time limit or returns an
   unknown answer, **Then** the built-in decision stands, the failure is logged and counted,
   and the sync goes on.

### User Story 2 - A rename rule (Priority: P2)

The operator turns on a rename plugin. When a file is downloaded, the plugin is asked for its
local name; the file is saved under that name. Nothing is renamed on Put.io.

**Why this priority**: Clean names are the second most asked-for rule, and the answer is
easy to check.

**Independent Test**: With the example rename plugin on, a download of
`Show.S01E02.1080p.WEB.x264-GRP.mkv` lands as `Show S01E02.mkv`.

**Acceptance Scenarios**:

1. **Given** a rename plugin returns a new name, **When** the file downloads, **Then** it is
   saved under that name in its folder.
2. **Given** the plugin returns nothing, an empty name, a name with a path separator, `.` or
   `..`, or fails or times out, **Then** the Put.io name is used.
3. **Given** a file requested by Radarr or Sonarr (they expect the exact path), **Then** no
   rename plugin is asked.

### User Story 3 - A folder rule (Priority: P2)

The operator turns on a folder plugin. When a file is downloaded, the plugin is asked for the
folder, below the download folder, to save it in.

**Why this priority**: Same shape as rename, for library layout.

**Independent Test**: With the example folder plugin on, `Show.S01E02.mkv` found in the Put.io
root lands in `TV/Show/Season 01/`.

**Acceptance Scenarios**:

1. **Given** a folder plugin returns `TV/Show/Season 01`, **When** the file downloads,
   **Then** it is saved there, creating the folders.
2. **Given** the plugin returns an absolute path, a path with `..`, or fails or times out,
   **Then** the file lands in its usual folder.
3. **Given** a watch-folder download staged by the processing pipeline, **Then** staging uses
   the plugin's folder as the place the item will live.
4. **Given** a Radarr/Sonarr file, **Then** no folder plugin is asked.

### User Story 4 - Writing one (Priority: P3)

A plugin author copies an example from `examples/plugins/`, reads `PLUGINS.md`, and writes a
plugin that declares one or more of the three extension points in `plugin.toml`. Settings shows
which sync rules a plugin provides.

**Independent Test**: The three examples load, pass the manifest checks and answer as
documented in an automated test.

**Acceptance Scenarios**:

1. **Given** a manifest declares an extension point its script does not define, **Then** the
   plugin shows as Broken with a message naming the function.
2. **Given** a plugin that only declares extension points (no events), **Then** it loads.
3. **Given** a plugin on its tab in Settings, **Then** the tab lists its sync rules and offers a
   time limit for them.

### Edge Cases

- Several plugins declare the same extension point: they are asked in name order and the first
  answer wins; a plugin that answers nothing, fails or times out is passed over.
- A plugin keeps failing or timing out: after 3 failures in a row its extension points are
  paused for 5 minutes (built-in rules apply), so a broken plugin cannot slow every file of a
  big sync. A success resets the count.
- A busy plugin: its event handling never delays its sync answers, and the reverse.
- A plugin is disabled, reloaded or its settings change during a sync: files after the change
  use the new set of plugins; nothing waits for the change.
- A rename answer collides with an existing file: the usual "if the file exists" choice for the
  folder applies to the new name.
- A file kept for an earlier attempt under a numbered name keeps that name; rename is not asked
  again.
- The rules preview in Settings and `--dry-run` show the built-in decisions only.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: A plugin manifest MUST be able to declare the extension points it provides:
  `filter`, `rename` and `folder`. Unknown names MUST be refused with the field named.
- **FR-002**: A manifest that declares at least one extension point MAY list no events, and its
  script then need not define the event handler. A declared extension point whose function is
  missing MUST make the plugin Broken.
- **FR-003**: The filter MUST be asked, during a sync pass, for each file the pass would decide
  on, after the built-in rules, with the file's id, name, size, extension, Put.io folder and the
  built-in decision and reason. Its answer MUST be one of download, stream, skip or none.
- **FR-004**: Rename and folder MUST be asked when a file's download starts, with the same file
  description, and MUST NOT be asked for Radarr/Sonarr files or files already given a numbered
  name.
- **FR-005**: Every call MUST have a time limit, per plugin, defaulting to 500 ms, settable from
  50 to 5000 ms. A call past it MUST be abandoned and treated as a failure.
- **FR-006**: An error, a timeout or an invalid answer MUST leave the built-in result in place
  for that file, be logged with the plugin, extension point and reason, and be counted.
- **FR-007**: After 3 failures in a row, a plugin's extension points MUST be skipped for 5
  minutes, logged once when the pause starts.
- **FR-008**: Answers MUST be checked: a name is one plain file name (no separators, not `.` or
  `..`, at most 255 bytes) and a folder is a relative path without `..` segments; both are made
  filesystem-safe like Put.io names.
- **FR-009**: Extension points MUST run in the same sandbox, with the same memory limit, store,
  approved hosts and `putio` API as the plugin's events, and MUST NOT wait behind its queued
  events.
- **FR-010**: The interface between the sync and the plugins MUST name only the extension
  points, the file description and the answers, not Lua, so another runtime can provide it.
- **FR-011**: The plugins API and Settings MUST show the extension points each plugin provides
  and its time limit for them; new UI text MUST go through the translation catalogs.
- **FR-012**: The repository MUST ship one working example plugin per extension point under
  `examples/plugins/`, linked from `PLUGINS.md`, and an automated test MUST load and run each.
- **FR-013**: Metrics MUST count extension-point calls by plugin, extension point and result,
  with bounded labels.

### Key Entities

- **Extension point**: one of filter, rename, folder; what is asked and when.
- **File description**: what a plugin is told about a file (id, name, size, extension, Put.io
  folder, built-in decision and reason).
- **Answer**: filter: download / stream / skip / none; rename: a name or none; folder: a
  relative folder or none.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: With the example filter on, a sync of 1 000 files takes at most 1 second longer
  than without it on a typical NAS.
- **SC-002**: A plugin that never answers adds at most 3 time limits (1.5 s by default) to a
  sync of any size, then the sync runs at built-in speed for 5 minutes.
- **SC-003**: Each example plugin works when copied unchanged into the plugins folder and
  turned on.
- **SC-004**: No sync stops or skips a file because a plugin failed.

## Assumptions

- Answers are not cached between calls; plugins that need state use their store.
- Rename and folder change only the local copy; Put.io is not touched.
- Rules preview and dry runs leave plugins out, since asking them for every file of a preview
  could be slow; this is documented.
- The per-plugin limit lives in `[[plugins]]` as `hook_timeout_ms` next to `timeout_secs`.
