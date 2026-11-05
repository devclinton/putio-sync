# Feature Specification: Log Viewer in the UI

**Feature Branch**: `047-log-viewer`

**Created**: 2026-11-05

**Status**: Draft

**Input**: User description: "nice to see logs from ui if possible at different level"

## Context

- putio-sync runs as a Docker container on the NAS. Today the only way to read its logs is
  `docker logs` over SSH, or the optional log file, which also needs a shell on the NAS.
- The log level comes from `[logging] level` and `module_filters` in the config file, or from
  `RUST_LOG`, which overrides both. A config poller applies a changed level from the file
  within about 10 seconds (feature 003), but only when the file existed at startup and
  `RUST_LOG` is not set. Settings › Logging already has a Level field that saves to the file.
- Features 027 (event hooks), 028 (Lua plugins), 031 (media-server refresh), 034 (download
  verification) and 041 (telemetry) each log under their own module. Several of them handle
  secrets: the Put.io token, the API token, the Put.io app secret, Radarr/Sonarr passwords,
  webhook URLs (which can carry a token), hook header and environment values, and plugin
  secret config. Existing code shortens URLs and masks hook output, but nothing checks a log
  line as a whole before it is shown to someone in a browser.
- The web UI signs in with a token kept in the browser and sent with every request. An
  unconfigured daemon runs in setup mode, where every route except the setup wizard answers
  503.

## Clarifications

### Session 2026-11-05

These were settled by the implementing session from the request and the code (the user was
not asked); each is the recommended option.

- Q: What does "keep" mean for a level change made on the Logs page? → A: It stays in effect
  until the daemon restarts or someone changes or clears it. It is not written to the config
  file; Settings › Logging › Level is still the way to make a level permanent. The page says
  so next to the choice.
- Q: Can the Logs page change the level while `RUST_LOG` is set? → A: Yes. A change from the
  page is an explicit, visible act by the operator, so it applies on top of whatever level
  is in effect, including one from `RUST_LOG`. The page shows where the base level comes
  from.
- Q: What happens to a page change when the config file's level changes? → A: The file's new
  level becomes the base; the page's change stays on top of it until it expires or is
  cleared.
- Q: How many lines are kept, and where is that set? → A: `[logging] buffer_lines`, default
  2,000, allowed 100 to 50,000. A change applies at once; shrinking drops the oldest lines.
- Q: Which "dropped lines" are counted? → A: Lines pushed out of the full buffer, and lines a
  live viewer missed because it fell behind. Both show on one metric with a reason.
- Q: How long can a timed change last, and how many module changes can be active? → A: 1 to
  1,440 minutes (a day); the page offers 30. At most 20 module changes at once; a 21st is
  refused.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Read recent logs in the browser (Priority: P1) 🎯 MVP

As the NAS operator, I want a Logs page that shows the daemon's recent log lines, so I can
see what it has been doing without opening a shell on the NAS.

**Why this priority**: This is the request. On its own it replaces `docker logs` for the
everyday "what just happened?" question.

**Independent Test**: Start the daemon, cause some activity (a sync, a test hook), open the
Logs page, and check that the matching lines are there, newest last, each with its time,
level, module and message.

**Acceptance Scenarios**:

1. **Given** a running daemon, **When** the operator opens Logs, **Then** the page shows the
   most recent lines kept (up to the configured number), each with its time, level, module
   and message, plus its structured fields (such as `file_id` or `hook`).
2. **Given** the Logs page, **When** the operator picks a level (error, warn, info, debug or
   trace), **Then** only lines at that level or more severe are shown.
3. **Given** the Logs page, **When** the operator types text in the search box, **Then** only
   lines whose message, module or fields contain that text (ignoring case) are shown.
4. **Given** the Logs page, **When** the operator picks or types a module (for example
   `putio_sync_download`, `putio_sync_hooks` or `putio_sync_plugins`), **Then** only lines
   from that module and the modules under it are shown. The module list offers the modules
   seen in the kept lines.
5. **Given** filters are set, **When** the operator reloads the page or shares its address,
   **Then** the same filters are applied again.
6. **Given** a browser that is not signed in, **When** it asks for the page's data, **Then**
   it is refused like every other protected page, and the UI asks it to sign in.

---

### User Story 2 - Follow the logs live (Priority: P1)

As the operator, I want new lines to appear as they are written, and to pause that while I
read, so I can watch a download or a hook run as it happens.

**Why this priority**: Watching live is what `docker logs -f` is used for most; without it
the page is only a snapshot.

**Independent Test**: Open Logs, trigger a sync, and check that its lines appear within a
couple of seconds without reloading. Pause, trigger another, check nothing new is added;
resume, and check the missed lines appear in order.

**Acceptance Scenarios**:

1. **Given** Logs is open with live tail on, **When** the daemon logs a line that passes the
   filters, **Then** it appears at the bottom within 2 seconds, and the view stays scrolled
   to the bottom unless the operator has scrolled up.
2. **Given** live tail, **When** the operator presses Pause, **Then** the shown lines stop
   changing and the page counts how many new lines are waiting. **When** they press Resume,
   **Then** the waiting lines are added in order.
3. **Given** live tail, **When** the connection drops (daemon restart, network), **Then** the
   page says so, retries, and on reconnecting fills in lines it missed that are still kept,
   without showing any twice.
4. **Given** a viewer that falls so far behind that lines are lost, **When** it catches up,
   **Then** the page shows a marker saying how many lines were skipped.

---

### User Story 3 - Change the log level from the UI (Priority: P1)

As the operator, I want to turn on debug or trace logging, for everything or for one module,
straight from the Logs page, and have it go back by itself, so I can look at a problem in
detail without editing the config or restarting, and without leaving verbose logging on.

**Why this priority**: The user asked for logs "at different level". Debug and trace lines
only exist while they are turned on, so the viewer is only half useful without this.

**Independent Test**: On Logs, set `putio_sync_hooks` to debug for 30 minutes, send a test
event, and check debug lines from hooks appear while other modules stay at info. Move the
clock past 30 minutes (in a test) and check the level is back to what it was.

**Acceptance Scenarios**:

1. **Given** the Logs page, **When** the operator looks at the level controls, **Then** they
   see the level in effect, where its base comes from (config file or `RUST_LOG`), and any
   change made from the page with the time it ends.
2. **Given** the level controls, **When** the operator sets the overall level, or a level for
   one module, and chooses "for 30 minutes", **Then** the change takes effect within a second
   with no restart, and is undone automatically 30 minutes later.
3. **Given** the same, **When** the operator chooses "keep", **Then** the change stays until
   the daemon restarts or someone clears it, and the page says it is not saved to the config
   file.
4. **Given** a change from the page, **When** the operator presses "Revert now", **Then** the
   level goes back to the base at once.
5. **Given** an invalid module name or level, **When** it is submitted, **Then** it is refused
   with a message that says what is wrong, and the level in effect does not change.
6. **Given** a level change (made, expired or reverted), **Then** the daemon logs one info
   line saying the old and new level and why.
7. **Given** debug or trace is not enabled for a module, **Then** no debug or trace lines from
   it are kept or shown: turning debug on does not reveal debug lines from before.

---

### User Story 4 - Secrets never reach the viewer (Priority: P1)

As the operator, I want to be sure the Logs page can never show a token, password, secret or
webhook address, even at trace level, so that opening it on a shared screen or pasting what
it shows into an issue is safe.

**Why this priority**: The page shows log lines to a browser, and at trace level libraries
can log request headers and URLs. A leak here undoes features 004 and 027's secret handling.

**Independent Test**: Configure every kind of secret with a known value, turn on trace, make
the daemon log lines that contain each one (in the message and in fields), and check that
neither the page's data, the live stream nor a download contains any of the values.

**Acceptance Scenarios**:

1. **Given** a log line that contains a configured secret value (Put.io token, API token,
   Put.io app secret, Radarr/Sonarr password, notification or hook webhook URL, hook header
   or environment value, plugin secret config value), **When** it is kept, **Then** each value
   is replaced by `********` before it is stored.
2. **Given** a line with an `Authorization`, `Cookie` or bearer value, a URL with user info
   or a query string, or a field whose name says it holds a secret (token, secret, password,
   key, authorization, cookie), **When** it is kept, **Then** the value is masked or the URL
   shortened, even if the value is not a configured secret.
3. **Given** a secret is changed in Settings, **When** a later line contains the new value,
   **Then** it is masked too.
4. **Given** the Logs page data, the live stream and the downloaded file, **Then** all three
   come from the same masked lines.

---

### User Story 5 - Copy, download and follow links (Priority: P2)

As the operator, I want to copy or download exactly the lines I am looking at, and to jump
from a line to the item it is about, so I can report a problem or look at the item without
hunting for it.

**Why this priority**: Useful, but the page is worth having without it.

**Independent Test**: Filter to one module, press Download, and check the file holds exactly
the lines shown, as text. Find a line with a `file_id` and check its link opens the page for
that file.

**Acceptance Scenarios**:

1. **Given** lines are shown, **When** the operator presses Copy, **Then** the shown lines (as
   filtered) are copied as plain text, one per line, and the page confirms how many.
2. **Given** lines are shown, **When** the operator presses Download, **Then** a text file
   named with the date and time is saved with the same lines.
3. **Given** a line with a Put.io file id, transfer id, hook name, plugin name or verify run
   id, **When** it is shown, **Then** that value is a link to the page that shows the item.
4. **Given** a line without such a field, **Then** it shows no link.

---

### User Story 6 - Don't leave verbose logging on (Priority: P3)

As the operator, I want the doctor to tell me when debug or trace is on, and the metrics to
count lines the viewer lost, so a forgotten setting or a too-small buffer gets noticed.

**Why this priority**: Housekeeping that keeps the feature from causing its own problems.

**Independent Test**: Turn on debug with "keep", run the doctor, and check it warns and names
the setting. Fill the buffer and check the dropped-lines count goes up.

**Acceptance Scenarios**:

1. **Given** debug or trace is in effect for any module (from the file, `RUST_LOG` or the
   page), **When** the doctor runs, **Then** it shows a warning naming the level, where it
   comes from, and when a page change ends, with a hint on how to turn it off.
2. **Given** only info or less is in effect, **When** the doctor runs, **Then** the check
   passes.
3. **Given** the buffer is full, **When** new lines push old ones out, **Then** the metrics
   count them as dropped with reason `evicted`; lines a live viewer missed are counted with
   reason `lagged`.

---

### Edge Cases

- Setup mode: the Logs page and its data answer like every other page in setup mode (503);
  the wizard is unchanged.
- A very long line (a big JSON body at trace) is cut to a fixed length with a marker, so one
  line cannot use the whole buffer.
- A burst of thousands of lines a second at trace: logging itself must not slow down or
  block; the page shows what it can and counts what it lost.
- Several browsers watching at once: each gets its own stream; one slow viewer does not
  hold up the others or the daemon.
- `[logging] buffer_lines` out of range: refused on save with the allowed range.
- The daemon restarts while a "30 minutes" change is active: the change is gone (it was never
  saved), and the page shows the base level.
- A change from the page while the config file also changes the level: the file sets the
  new base; the page's change stays on top.
- Lines logged before logging was set up, or by CLI commands that do not run the daemon, are
  not shown; the page is for the running daemon or server only.
- Clearing the view on the page hides the lines already shown for that browser only; it
  does not empty the daemon's buffer.

## Requirements *(mandatory)*

### Functional Requirements

**Keeping lines**

- **FR-001**: The daemon (and `server`) MUST keep the most recent log lines in memory, up to
  `[logging] buffer_lines` (default 2,000; 100 to 50,000). The oldest line is dropped when the
  buffer is full.
- **FR-002**: Each kept line MUST have an increasing sequence number, its time, level, module
  (target), message and structured fields.
- **FR-003**: Only lines the level in effect lets through are kept; debug and trace lines are
  kept only while that level is on for their module.
- **FR-004**: Keeping a line MUST NOT block the code that logs it. A line longer than 4 KB is
  cut, with a marker.
- **FR-005**: A change to `buffer_lines` MUST apply without a restart; shrinking drops the
  oldest lines.

**Secrets**

- **FR-006**: Every line MUST pass through a redaction step before it is kept. Nothing reads
  lines from before that step.
- **FR-007**: Redaction MUST replace every configured secret value of 6 or more characters,
  wherever it appears in the message or fields, with `********`. The list of values MUST be
  refreshed when settings are saved or the config is reloaded.
- **FR-008**: Redaction MUST also mask, without knowing the value: bearer and basic
  credentials, `Authorization` and `Cookie` values, `token=`/`secret=`/`password=`/`key=`
  style pairs, fields whose names say they hold a secret, and URLs (shortened as elsewhere
  in putio-sync: no user info, query or deep path).
- **FR-009**: Redaction MUST have automated tests covering each secret kind listed in the
  Context, in messages and in fields.

**Reading and live tail**

- **FR-010**: A protected endpoint MUST return the kept lines, optionally only those after a
  given sequence number, at or above a level, under a module, or containing text.
- **FR-011**: A protected streaming endpoint MUST send new lines as they are kept, starting
  after a given sequence number so a reconnecting viewer gets what it missed without
  duplicates, and MUST say how many lines were skipped when a viewer falls behind.
- **FR-012**: Both endpoints MUST use the same sign-in as the rest of the API (token or
  Tailscale identity) and answer 503 in setup mode.

**Level changes**

- **FR-013**: A protected endpoint MUST report the level in effect: the base directive and
  its source (config or `RUST_LOG`), the overall override and per-module overrides from the
  page, and when each ends.
- **FR-014**: A protected endpoint MUST set an overall level or a per-module level, either
  for 1 to 1,440 minutes (the page offers 30) or until restart, and apply it within a
  second without a restart. At most 20 module overrides may be active. Levels are error, warn, info, debug, trace (and off for a
  module). Module names MUST look like Rust paths (letters, digits, `_`, `::`).
- **FR-015**: A protected endpoint MUST clear one module's override or all of them, returning
  to the base at once.
- **FR-016**: A timed override MUST end on its own when its time is up, and the level MUST go
  back to the base plus any overrides still active.
- **FR-017**: A config-file level change MUST replace the base and keep the page's overrides
  on top.
- **FR-018**: Every applied change, expiry and revert MUST log one info line with the old and
  new directive and the reason.

**The page**

- **FR-019**: The UI MUST have a Logs page in the navigation, with level, module and text
  filters (kept in the page address), live tail with Pause/Resume and a waiting count, Copy,
  Download, and Clear view.
- **FR-020**: Lines carrying a Put.io file id, transfer id, hook name, plugin name or verify
  run id MUST link to the page for that item.
- **FR-021**: The page MUST show the level controls described in User Story 3.
- **FR-022**: Settings › Logging MUST gain one field, "Lines kept for the Logs page"
  (`buffer_lines`); nothing else on the Settings page changes.

**Doctor and metrics**

- **FR-023**: The doctor MUST warn when debug or trace is in effect for any module, naming the
  source and, for a page change, when it ends.
- **FR-024**: Metrics MUST count dropped log lines with a `reason` label (`evicted`, `lagged`).
  No line content or module name is used as a label.

### Key Entities

- **Log line**: sequence number, time, level, module, message, fields (name → value), and
  whether it was cut. Already redacted.
- **Log buffer**: the bounded list of recent lines, its capacity, and the next sequence
  number.
- **Level override**: a module (or none, for overall), a level, and an end time (or none, for
  "keep").
- **Level state**: the base directive and its source, plus the active overrides.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: The operator can find the lines for a given download or hook run from the
  browser in under a minute, with no shell on the NAS.
- **SC-002**: A new line appears on an open Logs page within 2 seconds of being logged.
- **SC-003**: Turning on debug for one module takes effect within 1 second, needs no
  restart, and a 30-minute change is undone within a minute of its end.
- **SC-004**: Across the redaction tests, 0 configured secret values and 0 credential values
  appear in the page data, the live stream or a download.
- **SC-005**: Memory used by kept lines stays bounded by `buffer_lines` × 4 KB plus overhead,
  however much is logged.
- **SC-006**: The web UI bundle stays under its 384 KB cap.

## Assumptions

- Lines are kept in memory only; a restart starts with an empty buffer. The log file and
  console output are unchanged.
- Only `daemon` and `server` keep lines; other CLI commands do not.
- The live stream follows the UI's existing sign-in, which sends the token in a header, so the
  page reads the stream with a normal authenticated request rather than a browser feature
  that cannot send headers.
- Links go to existing pages (Put.io, Transfers, Settings sections, Doctor); no new detail
  pages are added.
- The parallel Settings rework (046) will move the new `buffer_lines` field into its section
  when it merges.
