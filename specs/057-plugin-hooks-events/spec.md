# Feature Specification: More Plugin Hooks and Events

**Feature Branch**: `057-plugin-hooks-events`

**Created**: 2026-11-10

**Status**: Ready for planning

**Input**: User description: "what are some more hooks/events we could open for plugins?", then
"Yes, add brief". The brief is ROADMAP.md › "057 More plugin hooks and events".

## Context

- Plugins (028, 031, 048) and event hooks get the same 14 event types in EVENTS.md: downloads
  finished or failed, transfers added, finished or failed, `sync.completed`, `storage.low`, three
  verify events, `pipeline.step_failed`, `pipeline.completed`, and the two test events. Plugins
  also have `on_tick`, `on_action` and `health`.
- Plugins can only react. They cannot take part in an item's pipeline (038); only command and
  webhook hooks can be pipeline steps.
- `pipeline.completed` is sent only when an item ends with warnings. No event says that an item is
  fully ready (downloaded, verified, subtitled, translated). The bundled media-server plugins
  refresh on `download.finished` per file and wait for a folder to go quiet to make up for it.
- The subtitles, translation, archive, Radarr/Sonarr and cleanup parts of putio-sync send no
  events. Put.io outages, a refused token, scheduled doctor problems, stalled Put.io transfers and
  a low local disk are seen only in logs, metrics, the Status page or the doctor.
- Roadmap 030 (Notifications) needs stuck-download and doctor-problem events. Roadmap 055 (Plugin
  workbench) builds its event picker from the event reference, so new types show up there.
- Sync-time decision hooks (file filters, rename and folder rules) are roadmap 028 and are not
  part of this feature.

## Clarifications

### Session 2026-11-10

Settled by the session from the brief and the code (recommended options, not asked):

- Q: Change `pipeline.completed` so it is sent for every item? → A: No. Add `pipeline.finished`
  for every item and leave `pipeline.completed` as it is, so existing hooks and plugins keep
  working (schema version 1 only adds types).
- Q: Should a local download that makes no progress get its own `download.stalled` event? →
  A: No (asked of the user). The existing stall check on Put.io transfers sends
  `transfer.stalled`, and that is the stuck-download event for 030. No new no-progress limit is
  added for local downloads.
- Q: Do plugins subscribed to `"*"` get `download.progress`? → A: No. It is sent only to hooks and
  plugins that name it, so `"*"` subscribers are not flooded.
- Q: Does `config.changed` carry values? → A: No, only the names of the changed sections, so no
  secret can leak through it.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Know when an item is fully ready (Priority: P1) 🎯 MVP

As the operator, I want one event when an item has gone through its whole pipeline, with whether
it ended cleanly, so a media server refreshes once per item and a notification says "ready", not
"one file of twelve downloaded".

**Why this priority**: It closes the largest gap in today's events, is small, and the bundled
media-server plugins can use it at once.

**Independent Test**: Sync a folder of three files with a subtitles stage on. Exactly one
`pipeline.finished` is sent, after the subtitles stage, with `result` `success` and the three
files. An event hook subscribed to it gets it once.

**Acceptance Scenarios**:

1. **Given** an item whose steps all succeed, **When** it leaves the pipeline, **Then** one
   `pipeline.finished` is sent with `result` `success`, its key, name, category, source, folder
   and files.
2. **Given** an item whose optional step was given up, **When** it leaves the pipeline, **Then**
   `pipeline.finished` is sent with `result` `warnings` and the given-up steps, and
   `pipeline.completed` is still sent as today.
3. **Given** an item skipped in `pre_download` or skipped by the person on the Active page,
   **When** it leaves the pipeline, **Then** `pipeline.finished` is sent with `result` `skipped`.
4. **Given** an item that a restart interrupted, **When** it finishes after the restart, **Then**
   `pipeline.finished` is sent once, not once per start.
5. **Given** a bundled media-server plugin subscribed to `pipeline.finished`, **When** an item
   finishes, **Then** it refreshes that item's folder once, without waiting for the folder to go
   quiet.

---

### User Story 2 - Write a pipeline step as a plugin (Priority: P1)

As someone extending putio-sync, I want a plugin to be a step in an item's pipeline, so I can
check or change an item with the plugin sandbox and its settings instead of a script on the NAS.

**Why this priority**: It is the one change that lets plugins take part in what a sync does, and
it reuses the pipeline's existing rules for required and optional steps.

**Independent Test**: Put the example step plugin in the plugins folder, list it in the
`pre_download` stage, and add a transfer whose name it rejects. The item is skipped and its
queued files are removed. Make the plugin raise an error: the item waits on the Active page with
the error, and Retry runs the plugin again.

**Acceptance Scenarios**:

1. **Given** a plugin with `on_item` listed as a step in a stage, **When** an item reaches that
   stage, **Then** the plugin is called once with the same item fields a command step gets.
2. **Given** `on_item` returns nothing or `done`, **When** the call ends, **Then** the step is done.
3. **Given** `on_item` returns `skip` in `pre_download`, **When** the call ends, **Then** the item
   is skipped and its queued files are removed; in any other stage `skip` counts as done, as for
   command steps.
4. **Given** `on_item` raises an error or runs past its time limit, **When** the step is required,
   **Then** the item waits for Retry or Skip on the Active page with the error; **When** the step
   is optional, **Then** it is retried and given up as optional steps are today.
5. **Given** a stage lists a plugin that is missing, off, broken or has no `on_item`, **When** the
   config is loaded, **Then** the stage step is reported by the doctor and on the Plugins page, and
   an item reaching it fails that step with a message naming the plugin.
6. **Given** a step plugin and its own events, **When** both are due, **Then** they share the
   plugin's one queue, and a slow plugin delays only its own work.

---

### User Story 3 - Hear about trouble without watching the UI (Priority: P2)

As the operator, I want events when putio-sync cannot reach Put.io or an *arr app, when the
token is refused, when a transfer stalls, when the scheduled doctor finds a new problem, and when
the local disk runs low, so a notifier plugin tells me before I notice nothing is syncing.

**Why this priority**: These are what roadmap 030 needs for notifications, and they reuse the
checks putio-sync already runs.

**Independent Test**: With the Discord plugin subscribed to the health events, stop the mock
Put.io server. One `putio.unreachable` arrives; start it again and one `putio.recovered` arrives.
Fill the test disk past the warning line and `local_disk.low` arrives once.

**Acceptance Scenarios**:

1. **Given** Put.io stops answering, **When** calls have failed for the outage threshold, **Then**
   `putio.unreachable` is sent once, and `putio.recovered` is sent once when a call succeeds again,
   with how long it was down.
2. **Given** Put.io refuses the token, **When** that is seen, **Then** `auth.expired` is sent once
   until a working token is in place.
3. **Given** a Put.io transfer passes the stall threshold, **When** the stall check flags it,
   **Then** `transfer.stalled` is sent once for that transfer.
4. **Given** a scheduled doctor run, **When** a check is a problem that was not a problem in the
   run before, **Then** `doctor.problem` is sent for that check; a problem that stays a problem is
   not sent again.
5. **Given** the download folder's disk falls below the free-space warning, **When** that is
   seen, **Then** `local_disk.low` is sent once, and `local_disk.recovered` once it is back above.
6. **Given** a Radarr or Sonarr connection, **When** it stops answering and later answers again,
   **Then** `arr.unreachable` and then `arr.recovered` are sent, once each.
7. **Given** Put.io storage was low, **When** it is back above the warning line, **Then**
   `storage.recovered` is sent once.

---

### User Story 4 - Follow each part of the work (Priority: P3)

As someone writing plugins and hooks, I want events for the start of a sync and a download, for
download progress, and for subtitles, translation, archive extraction and Put.io cleanup, so I
can build dashboards, audits and notifications without reading logs.

**Why this priority**: Useful and cheap once the event plumbing for stories 1–3 exists, but no
current feature waits on them.

**Independent Test**: Subscribe a test plugin to all the events in this story and run a sync of
one archive with subtitles on and delete-after-download on. It receives, in order:
`sync.started`, `download.started`, `download.finished`, `archive.extracted`, `subtitles.found`
(or `subtitles.missing`), `cleanup.deleted`, `sync.completed`.

**Acceptance Scenarios**:

1. **Given** a sync run starts, **When** it begins, **Then** `sync.started` is sent with its
   trigger, before any of its downloads start.
2. **Given** a download starts or resumes, **When** the first bytes are requested, **Then**
   `download.started` is sent with whether it is resuming.
3. **Given** a plugin subscribes to `download.progress` by name, **When** a file downloads, **Then**
   it gets at most one progress event per file every 10 seconds; a plugin subscribed to `"*"`
   does not get it.
4. **Given** the subtitles stage looks for a video's languages, **When** it ends, **Then**
   `subtitles.found` lists the languages saved and `subtitles.missing` lists those it could not
   find.
5. **Given** a translation runs, **When** it ends, **Then** `translation.finished` or
   `translation.failed` is sent with the languages and, on failure, the error with secrets removed.
6. **Given** an archive is extracted, **When** it ends, **Then** `archive.extracted` or
   `archive.failed` is sent with the archive and the folder it went into.
7. **Given** delete-after-download removes a file or folder from Put.io, **When** it is removed,
   **Then** `cleanup.deleted` is sent with what was removed and why.

---

### User Story 5 - React to the daemon and its settings (Priority: P3)

As someone writing a plugin, I want to know when the daemon starts and stops, when the config
changes, and when my plugin's own settings change or it is about to unload, so it can check new
settings and send what it has grouped before it stops.

**Why this priority**: It rounds out the plugin model; the bundled notifiers lose grouped events
on a restart today, but rarely.

**Independent Test**: A plugin that groups events and sends them on `on_unload`. Queue two events,
press **Reload plugins**, and the grouped message is sent before the reload. Change the plugin's
settings and `on_config_changed` is called once.

**Acceptance Scenarios**:

1. **Given** the daemon starts, **When** plugins and hooks are ready, **Then** `daemon.started` is
   sent once with the version.
2. **Given** the daemon is asked to stop, **When** it begins stopping, **Then** `daemon.stopping`
   is sent, and plugins get a short time to handle it before they are stopped.
3. **Given** the config is saved or reloaded with changes, **When** it is applied, **Then**
   `config.changed` is sent with the names of the changed sections and no values.
4. **Given** a plugin's settings are saved, **When** the plugin has `on_config_changed`, **Then** it
   is called once with the new settings, secrets included as for `putio.config`.
5. **Given** a plugin is about to be reloaded, turned off or stopped, **When** it has `on_unload`,
   **Then** it is called once with the plugin's time limit, before the state is dropped.

---

### Edge Cases

- A flapping connection (Put.io or an *arr app up and down within a minute): unreachable and
  recovered are sent at most once each per outage, an outage starts only after the threshold,
  and repeats are not sent within the threshold.
- A plugin subscribed to a type that an older putio-sync does not know: the manifest error names
  the type, as today; the docs say which version added each type.
- A plugin that is both a step and subscribed to `pipeline.finished`: it gets the event after its
  own step has run.
- `on_unload` or `daemon.stopping` handling that runs past the time limit: it is stopped, and the
  stop or reload goes ahead; shutdown never waits longer than the plugins' limits.
- `on_unload` when the plugin broke (memory limit, error): it is not called on a state that was
  already dropped.
- A sync with nothing to do: `sync.started` and `sync.completed` are still sent as a pair.
- A download that fails before its first byte: `download.started` is still sent before
  `download.failed`, so every failed download has a start.
- Many files finishing at once: `pipeline.finished` is per item, not per file, so a season pack is
  one event.
- Events sent while a plugin is reloading wait in its queue as they do today; past 1 000 waiting
  events, the oldest are dropped and counted.
- A doctor check that goes from problem to warning: no event; only new problems are sent.

## Requirements *(mandatory)*

### Functional Requirements

**Event model**

- **FR-001**: Every new event type MUST use the existing envelope and schema version 1, be listed
  in the event reference with a table of its fields and an example, and be in the
  machine-readable schema.
- **FR-002**: Existing event types MUST keep their fields and when they are sent.
- **FR-003**: Every new event type MUST be available to event hooks and to plugins, and be
  selectable in the Plugins page and the hooks settings.
- **FR-004**: Event fields MUST NOT hold tokens or other secrets; errors MUST have secrets removed
  as today.

**Item ready (story 1)**

- **FR-005**: `pipeline.finished` MUST be sent once per item when it leaves the pipeline, with
  `result` `success`, `warnings` or `skipped`, the item's key, name, category, source, folder,
  files, given-up steps and how long the item took.
- **FR-006**: The bundled media-server plugins MUST be able to refresh on `pipeline.finished`,
  as their default when a pipeline is on.

**Plugin steps (story 2)**

- **FR-007**: A pipeline stage MUST accept a plugin by name as a step, in the same list and order
  as command and webhook steps, with the same required/optional setting.
- **FR-008**: A step plugin's `on_item` MUST get the same item fields as a command step and MUST
  be able to return done, skip, or fail with a message.
- **FR-009**: Skip MUST skip the item only in `pre_download`; elsewhere it MUST count as done.
- **FR-010**: Errors, timeouts and memory-limit stops MUST be treated as step failures, with
  `pipeline.step_failed` sent as for other steps.
- **FR-011**: A step naming a plugin that is missing, off, broken or has no `on_item` MUST be
  reported by the doctor and on the Plugins page, and MUST fail at that step with a message naming
  the plugin.
- **FR-012**: Step calls MUST run on the plugin's own queue with its limits, so a slow step plugin
  never holds up another plugin, hook or item outside that stage.

**Health (story 3)**

- **FR-013**: The system MUST send `putio.unreachable` once after Put.io calls have failed for an
  outage threshold, and `putio.recovered` once with the outage length when they succeed again.
- **FR-014**: The system MUST send `auth.expired` once when Put.io refuses the token, and not again
  until a working token has been seen.
- **FR-015**: The existing transfer stall check MUST send `transfer.stalled` once per transfer it
  flags.
- **FR-016**: A scheduled doctor run MUST send `doctor.problem` for each check that is a problem
  now and was not in the run before.
- **FR-017**: The system MUST send `local_disk.low` and `local_disk.recovered` when the download
  folder's disk crosses the doctor's free-space warning line, once per crossing.
- **FR-018**: The system MUST send `storage.recovered` once when Put.io storage is back above the
  warning line after `storage.low`.
- **FR-019**: The system MUST send `arr.unreachable` and `arr.recovered` per Radarr or Sonarr
  connection, once per outage.

**Activity (story 4)**

- **FR-020**: `sync.started` MUST be sent at the start of each sync run, paired with its
  `sync.completed`.
- **FR-021**: `download.started` MUST be sent once per download attempt, before its finished or
  failed event, with whether it resumes.
- **FR-022**: `download.progress` MUST be sent at most once per file per 10 seconds, only to hooks
  and plugins that name it, never to `"*"`.
- **FR-023**: The subtitles, translation, archive and cleanup parts MUST send
  `subtitles.found` / `subtitles.missing`, `translation.finished` / `translation.failed`,
  `archive.extracted` / `archive.failed` and `cleanup.deleted`.

**Lifecycle (story 5)**

- **FR-024**: `daemon.started`, `daemon.stopping` and `config.changed` (section names only) MUST be
  sent.
- **FR-025**: Plugins MAY define `on_config_changed`, called once after their settings are saved,
  and `on_unload`, called once before a reload, turn-off or stop of a working plugin, each with
  the plugin's time limit.
- **FR-026**: A stop or reload MUST NOT wait longer than the plugins' time limits for these calls.

**Docs, examples and tests**

- **FR-027**: EVENTS.md MUST document every new type; PLUGINS.md MUST document `on_item`,
  `on_config_changed`, `on_unload` and step plugins.
- **FR-028**: `examples/plugins/` MUST gain a working step plugin and an `on_unload` example, each
  with a README, loaded and run by an automated test.
- **FR-029**: Each new event MUST have a test that triggers it against the mock Put.io server or
  a fake and checks it is sent once with its documented fields.
- **FR-030**: New metrics, if any, MUST keep bounded labels; new UI text MUST go through the UI's
  translation catalogs; the change MUST have a CHANGELOG.md entry.

### Key Entities

- **Event type**: a name, its fields, when it is sent, how often at most, and whether `"*"`
  matches it.
- **Item outcome**: the result of an item's pipeline (success, warnings, skipped), its files and
  the steps given up.
- **Step plugin**: a plugin named in a stage's step list, with `on_item`, a required or optional
  setting, and its own limits.
- **Health state**: per watched thing (Put.io, token, each *arr app, Put.io storage, local disk,
  each doctor check), whether it is currently in trouble and since when, so each change is sent
  once.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A season pack of 12 files with subtitles on causes exactly one `pipeline.finished`
  and one media-server refresh, where today it can cause several.
- **SC-002**: A plugin step can be added to a stage and skip or pass items, with no shell script
  on the server, in under 10 minutes from the example.
- **SC-003**: A Put.io outage of any length produces exactly one unreachable and one recovered
  notification through a bundled notifier.
- **SC-004**: Every event type in the event reference has a test that triggers it, and 100% of
  their documented fields are present in what is sent.
- **SC-005**: Existing hooks and plugins written for the current 14 event types keep working with
  no changes.
- **SC-006**: Stopping the daemon takes no longer than today plus the plugins' time limits.

## Assumptions

- Roadmap 028's decision hooks (filter, rename, folder rules) stay separate.
- A local download that hangs without failing sends no event of its own; detecting that is out
  of scope.
- The outage threshold for Put.io and *arr reachability uses the existing retry and doctor
  timings, not a new setting, unless planning finds none fits.
- The local-disk warning reuses the free-space warning line of the doctor's disk check, so the
  event and the doctor agree.
- `doctor.problem` compares with the previous scheduled run only; on-demand runs from the CLI or
  web UI do not send it.
- The media-server plugins keep `download.finished` as their trigger when no pipeline is on.
- WebAssembly plugins (029) get the same events and callbacks when they arrive.
