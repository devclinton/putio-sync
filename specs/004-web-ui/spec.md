# Feature Specification: Web UI Served by the API

**Feature Branch**: `004-web-ui`

**Created**: 2026-10-25

**Status**: Draft

**Input**: User description: "Web UI served by putio-sync-api."

## Context

- putio-sync runs unattended on the NAS (Docker, home LAN). Today the operator checks on it
  by SSH-ing in and running CLI commands (`status`, `queue`, `history`, `transfers`), or by
  reading logs and, since feature 003, metrics. Nothing lets them see or act on it from a
  browser.
- The `putio-sync-api` crate already serves JSON over HTTP: Put.io files and transfers
  (list, details, delete, add, start, stop), the download queue (list, reorder, skip, pause
  and resume all, clear completed), active downloads (start one for a Put.io file, list,
  progress, cancel), workers, the watcher (status, scan, start, stop), local files, history,
  stats (downloads, transfers, disk) and settings (download, watcher, logging,
  notifications, read and update). It has an unauthenticated `/health`.
- Three things stand between that API and a usable UI:
  1. **The NAS doesn't run the API.** The container runs the `daemon` command, which
     starts no HTTP server. The `server` command serves the API, but its background work
     is a different, smaller engine: it only polls completed transfers, it does not run the
     folder sync, it uses its own download manager (separate from the one the API handlers
     use), it passes no watcher to the API (so the watcher routes report config values and
     can't scan), and it opens its database relative to the working directory instead of
     the download folder.
  2. **Auth is optional.** The API checks `api.auth_token` when set; when unset it allows
     every request ("development mode"). The `[api]` section is not in
     `config.example.toml`. Constitution Principle VI requires auth on anything that reads
     Put.io data or changes state.
  3. **No way to trigger a sync.** The API can start a download for one Put.io file but has
     no "sync now" action, and no single status summary (the CLI `status` command builds one
     from several sources).
- The UI must be testable against a mocked backend (Constitution III), like the rest of the
  API.

## Clarifications

### Session 2026-10-25

- Q: Which command should the NAS run so that one process does the daemon work and serves
  the UI? → A: The `daemon` command gains an optional API+UI listener that runs on the
  daemon's own engine: the same download manager, watcher and database at
  `download_dir/.putiosync.db`. `server` stays a thin development mode. All other
  Assumptions stand.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - See what the daemon is doing (Priority: P1) 🎯 MVP

As the NAS operator, I want to open a page on the LAN and see at a glance whether
putio-sync is healthy and what it is doing right now, so that I don't have to SSH in to
answer "is it working?".

**Why this priority**: It's the question asked most often, and it is the reason the UI
exists. Everything else builds on the same page.

**Independent Test**: Start the API against a mocked Put.io and a seeded database with a
few queued, downloading and failed items. Open the UI, sign in, and check that every item
in the acceptance scenarios is shown with the seeded values.

**Acceptance Scenarios**:

1. **Given** the UI is open and signed in, **When** the status page loads, **Then** it
   shows: version and uptime; when the last sync ran and whether it succeeded; when the next
   one is due; queue counts by state (pending, downloading, completed, failed, paused);
   each active download with name, size, progress, speed and estimated time left; busy and
   configured workers; the watcher's state and folders; and free disk space in the download
   folder.
2. **Given** the status page is open, **When** a download progresses or finishes,
   **Then** the page reflects it within 5 s without the operator reloading.
3. **Given** the API process can't reach the database or Put.io, **When** the status page
   loads, **Then** it says which part is unavailable and still shows the rest, instead of
   showing an empty page or an error for everything.
4. **Given** the operator is on a phone on the LAN, **When** they open the status page,
   **Then** it is readable and usable at phone width without horizontal scrolling.

---

### User Story 2 - Only the operator can see or change anything (Priority: P1)

As the operator, I want the UI and the API behind a secret that only I have, so that a
guest device on the LAN can't browse my Put.io account or delete files.

**Why this priority**: The UI is the first thing that invites a browser to the API. Opening
it to the LAN without auth is a security regression (Constitution VI), so it ships with
US1, not after.

**Independent Test**: Start the API with a token. Fetch the status data without a token and
with a wrong token (both refused). Sign in through the UI with the right token (works),
sign out (refused again). Start the API with no token configured and check that protected
pages and data are refused with a message saying how to set one.

**Acceptance Scenarios**:

1. **Given** no API token is configured, **When** the API starts, **Then** it logs one
   clear warning naming the setting, and every page or request except health and the
   sign-in page is refused until a token is set. There is no "open" mode.
2. **Given** a token is configured, **When** someone opens the UI without signing in,
   **Then** they see only the sign-in page.
3. **Given** the sign-in page, **When** the operator enters the token, **Then** they are
   signed in and stay signed in across page reloads in that browser until they sign out or
   the token changes.
4. **Given** a wrong token, **When** it is entered, **Then** sign-in fails with a message
   that does not reveal anything about the token, and repeated wrong attempts are slowed
   down.
5. **Given** any page, **When** its content, its data or the server logs are inspected,
   **Then** neither the API token nor the Put.io token appears in them. Settings pages
   show that a token is set, never its value.

---

### User Story 3 - Manage the queue and active downloads (Priority: P2)

As the operator, I want to reorder, skip, pause and resume queued downloads, cancel an
active one, and start a sync on demand, so that I can fix "the wrong thing is downloading"
without a terminal.

**Why this priority**: It's the main reason to act rather than just look. It depends on
US1's view of the queue.

**Independent Test**: With a seeded queue, use the UI to move an item to the top, skip
another, pause all, resume all, clear completed, cancel an active download and press
"sync now". Each action is reflected in the UI within 5 s and in the backend state.

**Acceptance Scenarios**:

1. **Given** a queue with several pending items, **When** the operator moves one up, down,
   to first or to last, **Then** the order changes and the workers pick the new order.
2. **Given** a pending item, **When** the operator skips it, **Then** it is not downloaded
   and shows as skipped.
3. **Given** pending items and downloads in progress, **When** the operator pauses all,
   **Then** pending items become paused and no new download starts; downloads already
   running finish and are shown as such. **When** the operator resumes all, **Then** paused
   items become pending again and workers pick them up.
4. **Given** an active download, **When** the operator cancels it, **Then** it stops, no
   partial file is reported as complete, and the item is shown as cancelled.
5. **Given** the operator presses "sync now", **When** a sync is not already running,
   **Then** a sync starts immediately and the status page shows it running and then its
   result. If one is already running, the UI says so and no second sync starts.
6. **Given** any action fails (for example the database is locked), **When** the UI gets
   the error, **Then** it shows a message naming the action and the reason, and the page
   stays usable.

---

### User Story 4 - Browse Put.io and manage transfers (Priority: P2)

As the operator, I want to browse my Put.io folders, queue a file for download, and see
and manage transfers (add a magnet or .torrent, start, stop, remove), so that the
occasional manual grab doesn't need the Put.io site and then a CLI command.

**Why this priority**: Useful, but the daemon already handles the common path
automatically. A manual queue action is what today's `download` command does.

**Independent Test**: Against a mocked Put.io with a two-level folder tree and a few
transfers, browse into a folder, queue one file, add a magnet link, stop and start a
transfer, remove one. Each shows the result in the UI and the mock sees the expected calls.

**Acceptance Scenarios**:

1. **Given** the Put.io page, **When** it opens, **Then** it lists the configured sync
   folder's contents with name, size, type and date, and the operator can open subfolders
   and go back up.
2. **Given** a file or folder on Put.io, **When** the operator chooses "download", **Then**
   it is added to the queue (a folder adds its files) and the queue page shows it.
3. **Given** the transfers page, **When** it opens, **Then** it lists transfers with name,
   state, progress and size, and offers start, stop and remove for each.
4. **Given** the transfers page, **When** the operator pastes a magnet link or picks a
   .torrent file, **Then** the transfer is added and appears in the list.
5. **Given** a large folder (hundreds of entries), **When** it is opened, **Then** the list
   appears within 2 s and the operator can find an entry by typing part of its name.

---

### User Story 5 - Look back at history (Priority: P3)

As the operator, I want to see what was downloaded or deleted and when, with totals, so
that "did last night's episode come through?" is a glance, not a database query.

**Why this priority**: Read-only and already available through the CLI; it rounds the UI
out.

**Independent Test**: With a seeded history of downloads and deletes, open the history
page, filter by action, page through results, and check the totals match the seed.

**Acceptance Scenarios**:

1. **Given** the history page, **When** it opens, **Then** it lists entries newest first
   with name, action, size, duration, speed and time, with paging.
2. **Given** the history page, **When** the operator filters by downloaded or deleted, or
   by a name fragment, **Then** only matching entries are shown.
3. **Given** the history page, **Then** it shows totals: downloads, deletes, bytes, and
   average speed.

---

### User Story 6 - View and change settings (Priority: P3)

As the operator, I want to see the effective settings and change the safe ones (download,
watcher, logging, notifications) from the browser, so that turning on debug logging or
changing the worker count doesn't need a file edit and a restart.

**Why this priority**: The API already supports it, and settings changes are rare.

**Independent Test**: Open the settings page, change the worker count and the log level,
save, and check the config file and the running process both reflect the change. Check that
the auth token fields show "set" or "not set" and can't be read.

**Acceptance Scenarios**:

1. **Given** the settings page, **When** it opens, **Then** it shows the download, watcher,
   logging and notification settings currently in effect, grouped as in the config file,
   with each setting's meaning explained briefly.
2. **Given** a change to a setting, **When** the operator saves, **Then** it is validated
   (invalid values are rejected with the reason), written to the config file, and applied
   as the API already does today (for example, worker count scales the pool; log level
   applies live).
3. **Given** a setting that needs a restart to take effect, **When** it is saved, **Then**
   the UI says so, naming the setting; a setting that applies live is not called
   restart-only, and a save of settings that all apply live shows no restart note.
4. **Given** the settings page, **Then** the Put.io token, app password and API token are
   never shown or editable. The page says whether each is set.

---

### Edge Cases

- **API unreachable from the browser** (process restarting, weekly backup restart): the UI
  shows a "disconnected" state and reconnects by itself when the API returns, keeping the
  operator signed in.
- **Token changed while signed in**: the next request is refused and the UI returns to the
  sign-in page with a message.
- **No database** (API started without one): queue, history and status sections that
  need it say "not available", and Put.io browsing still works.
- **Put.io unreachable**: Put.io pages show the error and a retry; the status page shows
  the last known sync result and flags Put.io as unreachable.
- **Two operators acting at once** (two browser tabs): each action is applied in the order
  the API receives it; the other tab sees the new state on its next refresh. No action is
  silently lost or applied twice.
- **Destructive actions** (delete on Put.io, delete a local file, remove a transfer, clear
  completed, cancel a download): each needs a confirmation naming what will be removed.
  Deleting from Put.io is never offered for a file that is still queued or downloading.
- **Worker count lowered while downloads run**: today's rule applies: each busy worker
  finishes its current file and then exits, so no download is interrupted. The UI shows
  the worker count falling as those files finish. A partial file resumes from its bytes on
  disk whenever it is picked, with no priority boost.
- **Browser without scripting or an old browser**: the sign-in page explains that a current
  browser is required; nothing else is served.
- **Bound to all interfaces**: that is the default in Docker. The README says that the UI
  port must only be published to trusted networks, and that the token is the only barrier.
- **Served over plain HTTP**: the token travels unencrypted on the LAN. The README says so
  and recommends a reverse proxy with TLS for anything beyond a home LAN. TLS termination
  is out of scope.

## Requirements *(mandatory)*

### Functional Requirements

**Serving**

- **FR-001**: The UI MUST be served by the same process and port as the JSON API, with no
  separate install, web server or files to deploy. The Docker image and the packaged
  binaries MUST include it.
- **FR-002**: The process the NAS runs MUST be able to serve the UI while doing the full
  daemon work (scheduled folder sync, download workers, watch folders, metrics) from one
  shared engine, so that what the UI shows and controls is the same queue, workers and
  watcher that are doing the work. The `daemon` command gains an optional API+UI listener
  that runs on the daemon's own engine (the same download manager, watcher and database at
  `download_dir/.putiosync.db`). The `server` command stays as a thin development mode and
  is not changed by this feature.
- **FR-003**: Whether the UI/API listener is on, and its bind address and port, MUST be
  settable in the TOML config with environment overrides, documented in
  `config.example.toml` and the README. Default: on, all interfaces, the existing API port
  (15432). The `[api]` section, including `auth_token`, MUST be added to
  `config.example.toml`.
- **FR-004**: Failure to open the UI/API listener MUST NOT stop the daemon work. It is
  logged once at ERROR.

**Access control**

- **FR-005**: Every page and every request other than health and the sign-in page MUST
  require the configured API token. When no token is configured, the API MUST refuse
  protected requests and log one warning naming the setting. The existing behaviour of
  allowing everything when no token is set MUST be removed.
- **FR-006**: The UI MUST offer a sign-in page that takes the token, keeps the operator
  signed in across reloads in that browser, and offers sign-out. The existing header-based
  token check for API clients MUST keep working.
- **FR-007**: Failed sign-ins MUST be rate-limited (at most 5 attempts per minute per
  client) and logged without the attempted value.
- **FR-008**: No page, response, or log line MAY contain the API token, the Put.io token or
  the app password. Settings responses MUST report only whether each is set.
- **FR-009**: Cross-origin access to the API MUST be limited to the UI's own origin. The
  current allow-everything setting MUST be removed.

**Status**

- **FR-010**: The UI MUST show one status view with: version, uptime, last sync time and
  outcome, next sync time, sync-in-progress flag, queue counts by state, active downloads
  (name, size, bytes done, speed, time left), busy and configured workers, watcher state and
  folders, disk free and total for the download folder, and whether the database and Put.io
  are reachable. A single status request MUST provide all of it (new capability; today it
  needs several requests and some values aren't exposed at all).
- **FR-011**: Status, queue, active downloads and transfers MUST refresh in the browser
  within 5 s of a change without a manual reload, while using at most one request per
  second per open page.

**Queue and downloads**

- **FR-012**: The UI MUST list the queue with name, size, path, state, priority and age,
  and offer: move up, move down, move to first, move to last, skip, pause all, resume all,
  clear completed, and cancel an active download. These use existing capabilities.
- **FR-013**: The UI MUST offer "sync now", which starts a folder sync immediately if none
  is running and reports its result. This is a new capability; the API MUST expose it, and
  MUST refuse a second sync while one runs.
- **FR-014**: The UI MUST let the operator start a download of a Put.io file or folder
  (folder adds its files recursively, honouring the blocked-extension and pattern filters).

**Put.io**

- **FR-015**: The UI MUST browse Put.io folders from the configured sync folder, show
  file details, filter the current folder by name, and offer download, and delete with
  confirmation.
- **FR-016**: The UI MUST list transfers with name, state, progress and size, and offer add
  (magnet link or .torrent upload), start, stop, and remove with confirmation.

**History and settings**

- **FR-017**: The UI MUST show history newest first with paging and filters by action and
  name fragment, plus totals. Filtering by name fragment is a new capability.
- **FR-018**: The UI MUST show and let the operator change the download, watcher, logging
  and notification settings the API already exposes, with validation messages and a note
  when a restart is needed. Saved settings MUST be written to the config file and applied
  as they are today. The save message MUST name which changed settings apply now and which
  need a restart, for the engine the API runs on (`daemon` or `server`). A save MUST NOT
  write a token the file didn't hold: `[auth]` and `api.auth_token` stay as they are on
  disk, so a `PUTIO_SYNC_API_TOKEN` value never reaches the file.

**Quality**

- **FR-019**: The UI MUST be usable at phone width (no horizontal scrolling) and in the
  current versions of mainstream desktop and mobile browsers.
- **FR-020**: Every destructive action MUST ask for confirmation naming the target. The
  UI MUST NOT offer a Put.io delete for a file that is queued or downloading.
- **FR-021**: The UI and the new API capabilities MUST be testable against a mocked Put.io
  and a temporary database, with no network access (Constitution III). The automated tests
  MUST cover: sign-in refused and accepted, the status summary, each queue action, sync
  now, and that no token appears in any response.
- **FR-022**: The README MUST document the UI: how to reach it, the token, the port to
  publish on the NAS, the plain-HTTP warning, and the settings added by this feature.
  `config.example.toml` MUST carry the new and the missing `[api]` settings with safe
  defaults.

### Key Entities

- **Operator session**: the signed-in state in one browser. It is derived from the
  configured API token, lasts until sign-out or a token change, and is never stored in a
  way that reveals the token on screen or in logs.
- **Status summary**: one snapshot of the running process: build identity, uptime, last
  and next sync, queue counts, active downloads, workers, watcher, disk, and the
  reachability of the database and Put.io.
- **Queue item**: a pending, downloading, completed, failed, paused or skipped download,
  with its priority and position.
- **Active download**: a queue item being worked on, with bytes done, speed and time left.
- **Put.io file and transfer**: entries from the operator's Put.io account as the API
  already models them, with the actions offered on each.
- **History entry**: one completed download or deletion with size, duration, speed and time.
- **Settings groups**: download, watcher, logging and notifications, with per-setting
  validation and a flag for "applies live" or "needs restart".

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: From a browser on the LAN, the operator can answer each of these in under 30 s
  with no SSH or CLI: Is it healthy? When did it last sync? What is downloading now, and
  how fast? How many files are waiting? Is the disk filling up?
- **SC-002**: The status page is interactive within 2 s of opening on the LAN, and changes
  in the backend appear on it within 5 s.
- **SC-003**: With a token configured, 100% of requests for protected pages or data without
  the token are refused, verified by an automated test over every route. With no token
  configured, the same holds, and the startup log names the setting.
- **SC-004**: An automated check over every UI page, every API response, and the server
  logs during a full test run finds no occurrence of the API token, Put.io token or app
  password.
- **SC-005**: Each queue action, sync now, and each transfer action completes end to end in
  the mocked-backend test suite, and the UI shows the result within 5 s.
- **SC-006**: The deployed NAS container needs only a port published and a token set to
  offer the UI; no other files, services or containers are added.
- **SC-007**: `make ci` passes, including the new UI and API tests, without network access.

## Assumptions

- **One operator, one secret.** The UI has a single role (the operator) and a single
  shared secret: the existing `api.auth_token`. User accounts, roles, OAuth or an external
  identity provider are out of scope. If a token isn't set, the operator sets one; the
  process does not generate or print one.
- **Plain HTTP on a trusted LAN.** TLS, reverse-proxy integration and exposure to the
  internet are out of scope. The README says the token is sent in clear on the LAN.
- **Default exposure.** The listener defaults to all interfaces on port 15432, matching
  the metrics decision in feature 003: in Docker, exposure is decided by which port the
  container publishes. The operator publishes the port on Unraid; this feature documents
  that step and doesn't do it.
- **Metrics stay separate.** The UI may show a link to the metrics endpoint but does not
  replace Grafana. Charts over time are out of scope; the status page shows current values.
- **Logs are not in the UI.** Live log viewing is out of scope; log level changes are
  (through settings), and logs go to Loki later as noted in feature 003.
- **Existing API shapes are reused where they fit.** The queue, downloads, workers,
  transfers, history, stats and settings data already exposed are enough for US3–US6.
  New capabilities are limited to: one status summary (FR-010), sync now (FR-013), history
  name filter (FR-017), rate-limited sign-in (FR-007), and the serving and auth changes in
  FR-002, FR-003, FR-005 and FR-009. Which other routes the plan touches is a plan
  decision.
- **Delete on Put.io is offered, with guards.** Browsing Put.io would be half a feature
  without it, and the CLI already offers it; confirmation and the queued/downloading guard
  (FR-020) keep it in line with Constitution IV. Local-file deletion through the UI is out
  of scope for this feature (the API route stays as is).
- **The legacy `/api/*` placeholder routes** are not used by the UI and may be removed by
  the plan.
- **Removing "open mode"** (FR-005) affects nobody in practice: the NAS runs `daemon`,
  which serves no API today, and the API's own tests set a token.
- **Packaging.** The UI ships inside the binary and image. How it is built is a plan
  decision; the constraint is that `make ci` builds and tests it with one command, and
  that the Docker build needs no extra services.
