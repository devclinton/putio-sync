# Feature Specification: Plugin Workbench

**Feature Branch**: `055-plugin-workbench`

**Created**: 2026-11-10

**Status**: Ready for planning

**Input**: User description: "we should have a very basic plugin editor/debugger tools in the app. Unsure
what that looks like, but feel important. Also we should document this with some examples in the
repo but in docs as well. The UI should link to docs where it can"

The brief is ROADMAP.md › "055 Plugin workbench: a basic editor and debugger".

## Context

- Plugins (features 028, 031, 048 and 029) are folders in the plugins folder: `plugin.toml` with
  `main.lua` (Lua), or a WebAssembly module (`runtime = "wasm"`, 029) built from Rust, Go or another
  language, whose manifest may sit inside the module. Seven Lua plugins ship with putio-sync; an
  operator adds more by putting a folder there and pressing **Reload plugins**, or with **Install
  a plugin**, which takes a URL, a `.wasm` file, or a `plugin.toml` with its entry file (029).
- Besides events, a plugin can answer sync hooks (028): `filter` (download, stream or skip a
  file), `rename` (the local name) and `folder` (the local folder), each called with a file, and
  can call `on_tick`, `health` and settings actions (031). With `[permissions.files]` (029) it may
  read, and for `w:` folders write, files under approved folders.
- Events include `transfer.stuck` and `doctor.problem` (030).
- Today a plugin is written in a text editor on the machine (or the NAS share) that holds the
  plugins folder. The only feedback in the app is the **Broken** state with a manifest error after
  a reload, and **Send test event**, which sends one `plugin.test` event for real and shows the
  result with up to 20 log lines.
- A test run makes real HTTP calls and changes the plugin's real store, so trying a notifier
  sends real notifications and trying a stateful plugin changes its state.
- Plugin docs are PLUGINS.md and EVENTS.md, which the project site (054) publishes as docs pages.
  The web UI hardly links to them: one link to a README anchor on GitHub and some text that says
  "see EVENTS.md" with no link.
- `examples/plugins/` holds five sync hook examples (three Lua, a Rust and a Go WebAssembly
  plugin), with a README and a test that loads them. None of them is an event plugin.

## Clarifications

### Session 2026-11-10

Asked of the user before the spec was written:

- Q: How much editing should the web UI do? → A: Edit in the UI: create from templates, edit
  `plugin.toml` and `main.lua` in the browser, check and reload, behind a setting.
- Q: What does a debugger run do with HTTP requests by default? → A: Record them and do not send
  them (a dry run with a scratch store), with a **Send for real** choice.
- Q: How to proceed? → A: A roadmap brief, then this spec.

Settled by the session from the request and the code (recommended options, not asked):

- Q: Is the editor setting a security boundary? → A: No. Anyone with the API token can already
  change settings, so the setting only keeps the editor out of the way and prevents edits by
  mistake. Plugins edited in the UI run in the same sandbox as any other plugin. The docs say so.
- Q: Can a plugin with errors be saved? → A: Yes, so work in progress is not lost. The errors show
  next to the editor, and the plugin shows **Broken** until they are fixed, the same as a file
  edited on disk.
- Q: What answer does a recorded (not sent) HTTP request get? → A: Status 200 with an empty body
  unless the person fills in a reply status and body for the run; the result marks every such
  request "not sent".
- Q: Does **Send for real** also use the real store? → A: Yes. It runs like a live event, with
  the approved hosts and the plugin's real store. A dry run never changes the real store.
- Q: Which docs do links open? → A: The docs pages on the project site, in a new tab. The site
  holds one current version of the docs.

### Session 2026-11-11

Updated for 028 sync hooks and 029 WebAssembly plugins (from the parent session's brief, not
asked):

- Q: Are WebAssembly plugins covered? → A: Yes, by the Run panel, the same as Lua. They are not
  editable as source: the workbench shows their manifest and offers a re-install or replace
  through the existing install form (URL, module or files).
- Q: What does a sync hook run take? → A: A sample file (id, name, size, extension, folder and the
  built-in decision), editable like an event payload; the result shows the answer as the sync
  would read it, or why it would be ignored.
- Q: What does a dry run do with `putio.fs.write`? → A: It is recorded and not written, like an
  HTTP request; reads, lists and checks work on the approved folders.
- Q: Which last 20 runs can be run again? → A: The Run panel's own last 20 runs, kept in memory
  with what was called, so any of them can be run again as it was.
- Q: Does a Run panel run wait for a busy plugin? → A: No. It runs on a worker of its own, like a
  settings action, so a live event queue does not delay it and it does not delay the queue.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Try a plugin without side effects (Priority: P1) 🎯 MVP

As someone writing or fixing a plugin, I want to run it on an event of my choice and see exactly
what it did (its result, its log lines, the HTTP requests it would make, and what it would store),
without sending real notifications or changing its saved state.

**Why this priority**: It is the debugger half of the request and is useful on its own, for
plugins written on disk as well as in the editor. It needs no new way of storing plugins.

**Independent Test**: With the editor setting off, open a plugin's tab, open **Run**, pick
`download.finished`, run. The result lists the plugin's log lines and each HTTP request it made,
marked "not sent"; no request reaches the server; the plugin's store is unchanged afterwards.

**Acceptance Scenarios**:

1. **Given** a working plugin's tab, **When** I open **Run** and pick an event type, **Then** an
   example payload for that type is filled in for me to edit, and **Run** is a dry run.
2. **Given** a dry run, **When** the plugin calls `putio.http.request`, **Then** the request is
   not sent, the plugin gets status 200 with an empty body (or the reply I set for the run), and
   the result lists the request's method, host, path, headers and body with secrets masked.
3. **Given** a dry run, **When** the plugin writes to its store, **Then** the result shows the
   store before and after the run, and the plugin's real store is unchanged.
4. **Given** a plugin that raises an error, **When** I run it, **Then** the result shows the error
   message with the file and line it came from, the duration and the log lines up to the error.
5. **Given** a plugin that runs past its time limit or memory limit, **When** I run it, **Then** the
   result says which limit stopped it, just as a live run would.
6. **Given** I choose **Send for real**, **When** I run, **Then** requests go to approved hosts only
   and the real store is used, and the result shows each real response's status and timing.
7. **Given** **Run**, **When** I choose `on_tick`, `health`, one of the plugin's actions, or one of
   its sync hooks (`filter`, `rename`, `folder`) with a sample file instead of an event, **Then**
   that function is called the same way, and the result shows what it returned.
8. **Given** the Run panel's last runs, **When** I pick one, **Then** I can run it again as it was.
9. **Given** a WebAssembly plugin, **When** I run it, **Then** the Run panel works the same way, with
   the module's memory in place of Lua's.

---

### User Story 2 - Write and edit a plugin in the app (Priority: P1)

As the operator, I want to create a plugin from a starter template and edit its two files in the
browser, so I can make a small plugin without shell access to the NAS.

**Why this priority**: It is the editor half of the request. It builds on the Run panel, which is
how an edit is tried.

**Independent Test**: Turn on the editor setting. Press **New plugin**, pick the webhook starter,
name it `my-hook`. The folder appears in the plugins folder, the plugin is listed as available,
and **Edit** opens both files. Change a log line, save, run it in the Run panel and see the new
line.

**Acceptance Scenarios**:

1. **Given** the editor setting is off, **When** I open the Plugins page, **Then** there is no **New
   plugin**, **Edit** or **Copy to edit**, and the page says where to turn the editor on.
2. **Given** the editor setting is on, **When** I press **New plugin**, choose a starter and give a
   name, **Then** a folder with that name and the starter's `plugin.toml` and `main.lua` is created
   in the plugins folder, with the name filled into the manifest, and the plugin is listed off.
3. **Given** a plugin from the plugins folder, **When** I press **Edit**, **Then** both files open in
   a code area with line numbers, and unsaved changes are marked.
4. **Given** edited text, **When** I press **Check**, **Then** manifest mistakes are listed with the
   same `file: field: message` text a reload gives, and Lua syntax errors with their line, without
   saving anything.
5. **Given** edited text, **When** I press **Save**, **Then** both files are written, the previous
   version is kept, that plugin alone is reloaded, and any errors are shown; a plugin with errors
   shows **Broken** until fixed.
6. **Given** the editor is open with unsaved changes, **When** I press **Run**, **Then** the run uses
   the text in the editor, not the saved files, and says so.
7. **Given** a WebAssembly plugin, **When** I open it, **Then** its manifest is shown read-only with
   the install form to re-install or replace it from a URL, a module or files.
8. **Given** a bundled plugin, **When** I open it, **Then** its files are shown read-only with
   **Copy to edit**, which creates a folder of the same name that replaces the bundled one, as a
   folder plugin with a bundled name does today.
9. **Given** a saved edit that adds a host to the manifest, **When** the plugin is enabled, **Then**
   it shows **Needs approval** until that host is approved, as with any manifest change.
10. **Given** unsaved changes, **When** I leave the editor or the page, **Then** I am asked first.

---

### User Story 3 - Learn from examples and docs (Priority: P2)

As someone new to plugins, I want a guide and a few small working examples, and links from the
app to the right part of the docs, so I can write a plugin without reading the code.

**Why this priority**: The request asks for it, and it makes the first two stories usable by
people other than the maintainer. It does not block them.

**Independent Test**: Open the docs site's "Writing and debugging plugins" page; it walks through
the webhook example and the Run panel. `examples/plugins/` holds the same starters as **New
plugin**, each with a README. From the Plugins page, Run panel and editor, each help link opens the
matching docs section.

**Acceptance Scenarios**:

1. **Given** the docs site, **When** I open the Extending section, **Then** a "Writing and debugging
   plugins" guide covers creating a plugin from a starter, editing, checking, a dry run, reading
   the result, **Send for real**, and what the editor setting does and does not protect.
2. **Given** the repository, **When** I open `examples/plugins/`, **Then** it holds the webhook
   notifier, store counter and tick poller, each a working plugin folder with a short README, and
   they match the starters in **New plugin**.
3. **Given** the Plugins page, the editor and the Run panel, **When** I press a help link, **Then**
   the matching docs section opens in a new tab: the plugin reference, the `putio` API table, the
   manifest fields, the event reference for the picked event type, and the guide.
4. **Given** the hooks card's "see EVENTS.md" text, **When** it is shown, **Then** EVENTS.md is a
   link to the event reference.

---

### Edge Cases

- The plugins folder is read-only (a read-only mount, or a snap or container path that cannot be
  written): **New plugin**, **Save** and **Copy to edit** fail with a message naming the folder,
  and nothing is half-written.
- **New plugin** or **Copy to edit** with a name that is taken or not allowed: refused with the
  naming rule (letters, digits, `-` and `_`, up to 40), and no folder is created.
- A file changed on disk after it was opened in the editor: **Save** warns and offers to reload the
  file or overwrite it.
- A plugin folder with more files than `plugin.toml` and `main.lua` (for example shared Lua): only
  those two are edited; other files are listed but not editable.
- A dry run of a plugin whose `on_event` decodes the HTTP reply: it gets the empty body or the reply
  set for the run, so the docs show how to set a reply.
- A plugin that is busy with a live event: the Run panel runs on a worker of its own and does not
  wait. Two Run panel runs of one plugin at a time are refused, like two settings actions.
- A dry run of a plugin with `[permissions.files]`: writes are recorded and not done; reads see
  the real files in approved folders.
- The edited plugin is enabled and has queued events: a save reloads it; queued events are kept and
  run on the new version, or dropped and counted if it is now broken.
- A huge payload or result: the event payload and each recorded body are limited (1 MiB, as for
  HTTP bodies) and cut with a note.
- Secrets typed into the payload or appearing in requests or log lines are masked in the result.
- The editor is turned off while a file is open: the open editor becomes read-only and its unsaved
  text is kept on screen to copy.

## Requirements *(mandatory)*

### Functional Requirements

**Run panel (debugger)**

- **FR-001**: Each plugin that loads MUST have a Run panel on its Plugins tab, whether the editor
  setting is on or off and whether the plugin is enabled or not.
- **FR-002**: The Run panel MUST offer every event type in the event reference (including
  `transfer.stuck` and `doctor.problem`), each with an example payload that can be edited, and also
  `on_tick`, `health`, each of the plugin's actions, and each sync hook it provides (`filter`,
  `rename`, `folder`) with an editable sample file.
- **FR-003**: A run MUST be a dry run unless **Send for real** is chosen for that run.
- **FR-004**: In a dry run, HTTP requests MUST NOT be sent; each MUST be answered with status 200
  and an empty body, or with a reply status and body set for the run, and recorded as "not sent".
- **FR-005**: In a dry run, the store MUST be a copy of the plugin's store taken at the start of the
  run and discarded afterwards. `putio.fs.write` MUST be recorded and not done.
- **FR-006**: With **Send for real**, a run MUST behave like a live event: approved hosts only and
  the real store.
- **FR-007**: The result MUST show success or the error with its file and line, which limit stopped
  the run if one did, the duration, the memory in use at the end of the run, up to 200 log lines, each HTTP request (method,
  host, path, headers, body, and for real requests the status and time), and the store before and
  after.
- **FR-008**: Secret values MUST be masked everywhere in a result, as they are in log lines today.
- **FR-009**: Runs from the Run panel MUST use the plugin's time and memory limits and MUST NOT
  count as live runs in metrics or the plugin's run list. Dry runs MUST NOT go to event hooks or
  other plugins.
- **FR-010**: The Run panel's last 20 runs MUST be kept with what was called, so any of them can be
  run again.
- **FR-010a**: The Run panel MUST work for Lua and WebAssembly plugins alike.

**Editor**

- **FR-011**: A setting, off by default, MUST turn the editor on or off. It is in the Plugin
  workbench panel on the Plugins page and in `config.toml` (`plugin_editor`).
- **FR-012**: With the editor on, **New plugin** MUST create a plugin folder from one of three
  starters (webhook notifier, store counter, tick poller) with the chosen name.
- **FR-013**: **Edit** MUST open `plugin.toml` and `main.lua` of a folder plugin in a code area with
  line numbers, mark unsaved changes, and ask before unsaved changes are dropped.
- **FR-014**: **Check** MUST report manifest errors in the same words as a reload, and Lua syntax
  errors with their line, without saving.
- **FR-015**: **Save** MUST write both files so that a failed write leaves the old files in place,
  keep the previous version of each file next to it, and reload only that plugin.
- **FR-016**: **Save** MUST be allowed when there are errors; the plugin then shows **Broken** with
  the error, as for a file edited on disk.
- **FR-017**: When the editor holds unsaved text, the Run panel MUST run that text and say so.
- **FR-018**: Bundled plugins MUST be shown read-only, with **Copy to edit** creating a folder plugin
  of the same name in the plugins folder.
- **FR-019**: **Save** MUST warn when a file changed on disk since it was opened.
- **FR-019a**: WebAssembly plugins MUST NOT be editable as source; the editor MUST show their
  manifest read-only and offer a re-install or replace through the install form.
- **FR-020**: Nothing in the editor MUST write outside the plugin's own folder in the plugins
  folder.

**Docs and links**

- **FR-021**: The docs MUST gain a "Writing and debugging plugins" guide in the Extending section.
- **FR-022**: `examples/plugins/` MUST hold the three starters, each with a README, and the starters
  in **New plugin** MUST be the same files.
- **FR-023**: An automated test MUST load each example and run it on a sample event as a dry run.
- **FR-024**: The web UI MUST link to the docs site from the Plugins page, the editor, the Run panel
  (for the picked event type) and the hooks card; links open in a new tab.
- **FR-025**: PLUGINS.md MUST describe the Run panel, the editor and its setting, and link the
  guide and the examples.
- **FR-026**: New UI text MUST go through the UI's translation catalogs, and the change MUST have a
  CHANGELOG.md entry.

### Key Entities

- **Starter**: a named plugin template (webhook notifier, store counter, tick poller): a
  `plugin.toml`, a `main.lua` and a README, kept in `examples/plugins/`.
- **Run request**: a plugin, what to call (an event with its payload, `on_tick`, `health`, an
  action with its input, or a sync hook with a sample file), dry run or for real, an optional reply for recorded requests, and
  optionally unsaved file text.
- **Run result**: outcome, error and line, limit hit, duration, memory in use, log lines, recorded or
  real HTTP requests, and the store before and after.
- **Editor setting**: one on/off value in the config, off by default.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A person with no shell access can create a working plugin from a starter, change it
  and see the change in a run in under 5 minutes.
- **SC-002**: A dry run of any bundled plugin sends no network request and leaves its store
  unchanged, in 100% of runs checked by the tests.
- **SC-003**: A plugin error is shown with the file and line that raised it in every case where Lua
  reports one.
- **SC-004**: Every example in `examples/plugins/` loads and passes a dry run in the automated
  tests.
- **SC-005**: Each part of the Plugins page (list, tab, Run panel, editor) has at least one help
  link, and every help link opens a docs page that exists.
- **SC-006**: With the editor setting off, no part of the app can change a plugin's files.

## Assumptions

- The editor is plain text with line numbers; no highlighting, completion or step debugging. The
  web UI has no build step and gains no large editor library.
- Source editing covers Lua plugins; WebAssembly plugins are built elsewhere and replaced whole.
- The editor setting keeps edits from happening by mistake; it is not a security boundary, since
  the API token already allows changing settings. The sandbox is what limits plugin code.
- Docs links point at the project site's current docs, so a link can name a section that an older
  install does not have yet.
- Kept previous versions are one per file (`<file>.bak`), not a history.
- Example payloads come from the event reference examples and are kept in step with it by a test.
