# Feature Specification: Event Hooks

**Feature Branch**: `027-event-hooks` (built on `claude/hungry-heyrovsky-9bcd85`)

**Created**: 2026-11-04

**Status**: Draft

**Input**: User description: "027 Event hooks: run a command or call a webhook on events —
download finished or failed; transfer added, finished or failed; sync run complete; Put.io
storage low. The event is sent as JSON (stdin for commands, the body for webhooks) so hooks
can be written in any language. Hooks have a timeout, their output is logged, and a failing
hook never stops a sync. The event model is a versioned, documented JSON schema delivered
through one internal event bus that hooks consume (028 Lua plugins, 030 notifications and
031 media-server refresh will subscribe to the same bus later). Config: per-hook event
filters, command vs URL, webhook headers, timeout. Validation and a doctor check. A Hooks
settings group in the UI with a "send test event" button. Prometheus metrics for hook runs
and failures. Security: commands run as the daemon user; webhook URLs may carry secrets so
they are never logged in full and tokens are never printed."

## Context

- The roadmap places 027 under 028 (Lua plugins), 030 (notifications) and 031 (media
  server refresh). Those subscribe to the same events in-process, so the events are a
  product surface of their own, not a detail of hooks.
- A `[notifications]` table exists in the config with a `webhook` backend, but nothing in
  the daemon ever sends it. It stays as it is; 030 replaces it. Hooks do not read it.
- 033 tracks Put.io storage and has a `warn_free_percent` threshold; the storage-low event
  uses it.
- An unconfigured daemon runs in setup mode, where every route except the wizard answers
  503.

## Clarifications

### Session 2026-11-04

Answered with the recommended option, without a live user; listed in the hand-over note
for review.

- Q: Are failed hooks retried? → A: Webhooks are retried twice (after 5 s and 30 s) on a
  connection error, a timeout or a 5xx/429 answer; a 4xx is final. Commands are not
  retried, since a command may have done half its work.
- Q: How is a command run — through a shell, or as a program with arguments? → A: As a
  program with an argument list, no shell. A user who wants a shell writes
  `["/bin/sh", "-c", "…"]` themselves.
- Q: Does a command inherit the daemon's environment? → A: No. It gets a small fixed
  environment (`PATH`, `HOME`, `LANG`, `TZ` when set, and the `PUTIO_SYNC_EVENT_*`
  variables) so the Put.io token and other secrets in the daemon's environment never
  reach hook code. Extra variables can be set per hook.
- Q: When does storage-low fire? → A: Once when free space drops below the warning
  threshold, and again only after it has gone back above the threshold first, so a NAS
  sitting at 9 % does not fire every 15 minutes.
- Q: Which processes fire hooks? → A: The long-running server (with or without
  `--download`) and the `daemon` command. One-off CLI commands (`download`, `sync`) do
  not, so a cron job and the daemon never both fire for the same file.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Run my script when a download lands (Priority: P1)

A user wants a Python script to run whenever a file finishes downloading (to move it, tag
it, or tell another program). They add a hook with the script's path and the
"download finished" event. Each time a download completes, the script starts with the
event as JSON on its standard input, and its output shows in the daemon's log.

**Why this priority**: it is the core of the feature and the most common use; the event
bus, the schema and the command runner all come with it.

**Independent Test**: configure a hook that runs a script writing its stdin to a file;
download one file; the file holds one event of type `download.finished` with the file's
name, size and local path, and the log shows the hook ran and what it printed.

**Acceptance Scenarios**:

1. **Given** a hook for `download.finished` that runs `/config/hooks/on-done.py`, **When**
   a file finishes downloading, **Then** the script runs once with the event JSON on
   stdin and the variables `PUTIO_SYNC_EVENT_TYPE` and `PUTIO_SYNC_EVENT_ID` set.
2. **Given** that script prints lines to stdout and stderr, **Then** the daemon log shows
   them under the hook's name, cut to a few kilobytes.
3. **Given** a script that exits with a non-zero code, **Then** the log records a failed
   run with the exit code, the failure counter goes up, and the download stays finished.
4. **Given** a script that runs past the hook's timeout, **Then** it is stopped, the run
   is recorded as a timeout, and nothing else waits on it.
5. **Given** a hook that only listens to `download.failed`, **When** a download finishes,
   **Then** the hook does not run.
6. **Given** ten downloads finishing at once, **Then** each event reaches the hook once,
   in the order they happened, and downloads never wait for the hook.

---

### User Story 2 - Call a webhook (Priority: P1)

A user runs Home Assistant (or n8n, or a small Go service) and wants it told about events
over HTTP. They add a hook with a URL, pick the events, and add an `Authorization` header.
Each event is sent as a JSON POST body.

**Why this priority**: the other half of the roadmap item; many users have no shell
access to the NAS, only a URL to call.

**Independent Test**: point a hook at a local test server; trigger a sync run; the server
receives one POST with `Content-Type: application/json`, the configured header, and a body
of type `sync.completed`.

**Acceptance Scenarios**:

1. **Given** a webhook hook with a header `Authorization: Bearer abc`, **When** an event it
   listens to happens, **Then** the URL gets a POST with that header, a JSON body, and a
   `User-Agent` naming putio-sync and its version.
2. **Given** a 2xx answer, **Then** the run counts as a success.
3. **Given** a 500 answer or no connection, **Then** the call is tried again twice, then
   recorded as failed; a 400 answer is recorded as failed at once.
4. **Given** a URL with a token in its query string, **Then** no log line, error, API
   answer or metric shows the token: the URL appears as scheme, host and a shortened path
   only.
5. **Given** a header value, **Then** it is never shown in the log, the doctor, the API or
   the UI after saving; the UI shows it as set, with a way to replace it.

---

### User Story 3 - Know what each event looks like (Priority: P1)

A developer writing a hook (or, later, a Lua plugin) needs to know exactly which fields
each event carries. The events follow a versioned schema, published in the docs and served
by the daemon, with an example for each event type.

**Why this priority**: 028, 030 and 031 build on these events; changing them later breaks
every hook users wrote, so the shape has to be deliberate from the start.

**Independent Test**: fetch the schema from the daemon; every event the daemon sends in
the tests matches it, and every event type in the schema has an example in the docs.

**Acceptance Scenarios**:

1. **Given** any event, **Then** it carries a schema version, a unique id, its type, when
   it happened, which putio-sync sent it (version and host name), and a `data` object
   whose fields depend on the type.
2. **Given** the docs, **Then** they list every event type with its fields, their types,
   which are always present, and an example.
3. **Given** a later release adds a field, **Then** the schema version stays the same;
   removing or renaming a field raises it.
4. **Given** an event, **Then** it never holds a token, a password or a header value.

---

### User Story 4 - Set hooks up from Settings and test them (Priority: P2)

A user without access to the config file opens Settings › Hooks, adds a hook (name,
command or URL, events, headers, timeout), saves it, and clicks "Send test event". The
result (success or failure, exit code or HTTP status, time taken, the first part of the
output) shows next to the hook straight away.

**Why this priority**: makes the feature usable without editing files and gives a fast
way to check a hook before a real event comes.

**Independent Test**: add a webhook hook in the UI pointing at a test server, save, click
Send test event; the server gets a `hook.test` event and the UI shows "200 OK in 40 ms".

**Acceptance Scenarios**:

1. **Given** the Hooks group, **Then** each hook shows its name, kind, target (URL
   shortened), events, whether it is enabled, and its last run result.
2. **Given** an invalid hook (no target, both a command and a URL, an unknown event, a
   timeout out of range, a relative command path, a URL that is not http or https),
   **When** the user saves, **Then** the save is refused with a message naming the hook and
   the field, and nothing is written.
3. **Given** a valid save, **Then** the new hooks apply to the next event without a
   restart.
4. **Given** "Send test event", **Then** only that hook receives a `hook.test` event, and
   the result shows within the hook's timeout.
5. **Given** a hook that is disabled, **Then** it receives no events but can still be
   tested.

---

### User Story 5 - Events for transfers, sync runs and low storage (Priority: P2)

A user wants to hear when a transfer is added to Put.io, finishes or fails, when a sync
run ends (with how many files it found and queued), and when Put.io storage runs low.

**Why this priority**: completes the roadmap's event list; each event is small once the
bus exists.

**Acceptance Scenarios**:

1. **Given** a transfer added through putio-sync or seen for the first time on Put.io,
   **Then** one `transfer.added` event is sent with its id, name, origin (Radarr or
   Sonarr, a watch folder, added from putio-sync, or added on Put.io directly) and size
   when known.
2. **Given** a transfer that completes or errors on Put.io, **Then** one
   `transfer.finished` or `transfer.failed` event is sent, with the error message for a
   failure.
3. **Given** a sync run, **When** it ends (with or without errors), **Then** one
   `sync.completed` event is sent with its duration, the counts of files seen, queued and
   skipped, blocked and failed, the error for a failed run, and whether it was started by
   the schedule or by hand ("Sync now").
4. **Given** free Put.io space falling below the warning threshold, **Then** one
   `storage.low` event is sent with used, total and free space and the threshold; no
   second one comes until space has first recovered above the threshold.
5. **Given** a daemon restart, **Then** transfers that were already known before the
   restart do not send `transfer.added` again.

---

### User Story 6 - See hooks in the doctor and in monitoring (Priority: P3)

A user wants a broken hook to be noticed: a script deleted from disk, a URL that has
failed for a day. The doctor has a Hooks check, and the metrics endpoint counts hook runs
and failures per hook.

**Acceptance Scenarios**:

1. **Given** a command hook whose program is missing or not executable, **Then** the
   doctor reports a problem naming the hook.
2. **Given** a hook whose recent runs have failed, **Then** the doctor warns with the last
   error and when it happened.
3. **Given** no hooks, **Then** the doctor's Hooks check passes and says none are set up.
4. **Given** the metrics endpoint, **Then** it has counters for hook runs by hook, event
   type and result (success, failure, timeout), a run-time histogram per hook, a counter
   of events dropped because a hook's queue was full, and a counter of events emitted by
   type.

### Edge Cases

- **Slow hook, many events**: each hook has its own queue (1 000 events). When it is
  full the oldest event is dropped for that hook only, logged and counted; other hooks
  and the sync are not slowed.
- **Order**: events reach one hook in the order they were emitted; a hook runs one event
  at a time. Different hooks run independently of each other.
- **Shutdown**: on a clean stop the daemon waits up to 10 seconds for running hooks, then
  stops them; queued events that were not sent are logged as dropped.
- **Hook output**: stdout and stderr are each kept to the first 4 KiB for the log and the
  test result; anything longer is cut with a note of how much was cut.
- **Command that ignores stdin**: allowed; a closed pipe is not an error.
- **Command that does not exist**: the run fails with "not found"; the doctor flags it.
- **Webhook redirects**: followed up to 3 times, same host only, so headers are not sent
  elsewhere.
- **Large events**: no event is larger than 64 KiB; a sync run's event holds counts, not
  file lists.
- **Hook name**: required, unique, letters, digits, `-` and `_`, up to 40 characters; it
  is the metric label, so it stays short and bounded.
- **Wildcard**: `events = ["*"]` subscribes to every event type, including types added in
  later releases; `hook.test` only reaches a hook through the test button.
- **Setup mode**: the hooks routes answer 503 like every other route.
- **Downloads by the `--download` thread**: events from the background download thread
  reach the same hooks as events from the API process.
- **Settings import**: a settings file with hooks is validated like a save; masked header
  values from an export are refused, since the real value is not in the file.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The daemon MUST publish events through one internal event stream that any
  number of consumers can subscribe to, with hooks as the first consumer.
- **FR-002**: Event types MUST be `download.finished`, `download.failed`,
  `transfer.added`, `transfer.finished`, `transfer.failed`, `sync.completed`,
  `storage.low` and `hook.test`.
- **FR-003**: Every event MUST carry `schema_version` (an integer, 1 at launch), `id`
  (unique), `type`, `occurred_at` (UTC, RFC 3339), `source` (app name, version, host
  name) and `data`.
- **FR-004**: The event schema MUST be published as a machine-readable document in the
  repository, served by the daemon, and described in the docs with an example per type.
- **FR-005**: Adding a field MUST NOT change the schema version; removing, renaming or
  changing the type of a field MUST raise it.
- **FR-006**: Events MUST NOT contain tokens, passwords or header values.
- **FR-007**: The configuration MUST accept a list of hooks, each with a name, an enabled
  flag, the event types it listens to (or `*`), exactly one of a command (an absolute
  program path and arguments) or a URL, optional webhook headers, optional extra
  environment variables for commands, and a timeout (default 30 s, 1 s to 10 min).
- **FR-008**: A command hook MUST run as the daemon's user with the event JSON on stdin
  and a fixed small environment plus `PUTIO_SYNC_EVENT_TYPE`, `PUTIO_SYNC_EVENT_ID` and
  `PUTIO_SYNC_HOOK_NAME`; it MUST NOT inherit the daemon's other variables.
- **FR-009**: A command's exit code 0 MUST count as success; any other code, a failure
  to start, or a timeout MUST count as failure.
- **FR-010**: A webhook hook MUST POST the event JSON with `Content-Type:
  application/json`, the configured headers, and a putio-sync `User-Agent`; a 2xx answer
  MUST count as success.
- **FR-011**: A webhook MUST be retried twice (5 s, then 30 s later) on a connection
  error, timeout, 429 or 5xx, and MUST NOT be retried on another 4xx.
- **FR-012**: A hook run past its timeout MUST be stopped (the process killed, the request
  dropped) and recorded as a timeout.
- **FR-013**: Every run MUST be logged with the hook name, event type and id, result,
  duration, and exit code or HTTP status; command output and webhook answer bodies MUST be
  logged cut to 4 KiB each.
- **FR-014**: A hook failing, hanging or being slow MUST NOT stop, delay or fail a sync,
  download or transfer; events MUST be handed to hooks without waiting for them.
- **FR-015**: Each hook MUST have its own bounded queue; when full, the oldest event for
  that hook MUST be dropped, logged and counted.
- **FR-016**: URLs MUST never appear in full in logs, errors, the doctor, metrics or the
  API: only the scheme, host, port and a shortened path, with no query string or user
  info. Header values MUST never appear at all after saving.
- **FR-017**: Config loading, Settings saves, settings import and the setup assistant
  MUST refuse invalid hooks (missing or duplicate name, bad name characters, no target,
  both targets, a relative command path, a non-http(s) URL, an unknown event type, an
  empty event list, a timeout out of range, a header name that is not valid), each
  naming the hook and the field.
- **FR-018**: The Settings banner and the doctor MUST warn about hooks that are valid but
  risky: a plain-`http` URL to a host that is not local, a command in a folder anyone can
  write to, a disabled hook, and a `[notifications]` webhook that is set but has no
  effect.
- **FR-019**: The doctor MUST have a Hooks check that reports missing or non-executable
  programs as a problem and recent failures as a warning, without running any hook.
- **FR-020**: Settings MUST have a Hooks group to add, edit, enable, disable and remove
  hooks, with header values masked once saved, and saves MUST apply without a restart.
- **FR-021**: The API and the UI MUST offer "Send test event" per hook, sending one
  `hook.test` event to that hook only and returning the result (success, exit code or
  status, duration, output excerpt).
- **FR-022**: The API MUST list the hooks with their last few runs (time, event type,
  result, duration), without secrets.
- **FR-023**: The metrics endpoint MUST have `putio_sync_events_total{type}`,
  `putio_sync_hook_runs_total{hook,event,result}`,
  `putio_sync_hook_run_duration_seconds{hook}` and
  `putio_sync_hook_events_dropped_total{hook}`.
- **FR-024**: `download.finished` MUST carry the Put.io file id, name, size, local path,
  duration and average speed; `download.failed` the same identity fields plus the error,
  its category (network, http, io, integrity, other) and the number of attempts made.
- **FR-025**: `transfer.*` events MUST carry the transfer id, name, origin, size when
  known, the Put.io file id when finished, and the error when failed; each transition is
  sent once per transfer.
- **FR-026**: `sync.completed` MUST carry the trigger (schedule or manual), the result
  (success or failure, with the error), start time, duration, and counts of files found,
  queued, skipped, blocked and failed.
- **FR-027**: `storage.low` MUST carry total, used and free bytes, free percent and the
  threshold, and fire once per crossing below the threshold.
- **FR-028**: The README, both example configs and the changelog MUST describe hooks, the
  events, the security model (daemon user, small environment, secrets masked), and
  include one example hook script.

### Key Entities

- **Event**: envelope (schema version, id, type, time, source) plus type-specific data.
- **Hook**: name, enabled, events, target (command with arguments and environment, or URL
  with headers), timeout.
- **Hook run**: hook, event type and id, start time, duration, result (success, failure,
  timeout), exit code or HTTP status, attempts, output excerpt, error.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A hook for an event starts within 2 seconds of the event happening when its
  queue is empty.
- **SC-002**: With a hook that sleeps forever, downloads and sync runs finish in the same
  time as with no hooks.
- **SC-003**: No test, log line, API answer or metric produced by the feature contains a
  configured header value or a URL query string.
- **SC-004**: A user can add a webhook in Settings and see a test event's result in under
  a minute, without a restart.
- **SC-005**: Every event type has a documented example that matches the published schema.

## Assumptions

- Hooks are written in TOML as `[[hooks]]` tables; the UI writes the same tables.
- One event per hook at a time; no parallel runs of the same hook.
- Run history kept in memory (last 20 runs per hook); it is not kept across restarts. The
  metrics are the long-term record.
- The webhook method is always POST; no templates for the body. Templating and
  per-service formats belong to 030.
- No signature (HMAC) on webhook bodies in this release; users who need authentication
  use a header. A signing secret can be added later without a schema change.
- `download.failed` fires once per file failure, after the download system's own
  in-place retries are used up; stopped downloads (shutdown, cancel) are not failures.
  Skipped files (already on disk) send no download event.
- Hooks do not run in one-off CLI commands.
- Webhooks use the daemon's normal outbound network; there is no allow-list of hosts.
