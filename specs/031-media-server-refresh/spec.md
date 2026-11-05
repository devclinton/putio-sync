# Feature Specification: Media Server Refresh

**Feature Branch**: `031-media-server-refresh` (built on `claude/distracted-darwin-51cb86`)

**Created**: 2026-11-04

**Status**: Draft

**Input**: User description: "031 Media server refresh (from ROADMAP.md): After a download
lands, tell Plex to rescan just that folder, so new episodes and movies show up within
seconds. Then Jellyfin and Emby. Includes signing in to Plex (PIN / plex.tv link flow, not
a pasted token), choosing the library, and mapping paths when the media server sees the
folder under a different path. Built as bundled Lua plugins on 028 (like the
ntfy/Discord/Telegram/Pushover notifiers), adding small general host features where the
plugin API can't do it cleanly (interactive sign-in actions, config fields with choices
fetched from the server, a path-mapping field type, faster per-plugin ticks, plugin health
checks for the doctor, plugin counters for metrics). Group refreshes: one scan per folder
within a short window. Doctor (server reachable, token valid, path mapping resolves), a
test button, metrics. Never log Plex/Jellyfin/Emby tokens."

## Context

- 027 put every event on one bus (`EVENTS.md`). `download.finished` carries the file's
  `local_path` on the daemon's disk; `sync.completed` ends a sync run.
- 028 added sandboxed Lua plugins (`PLUGINS.md`): a manifest with typed config fields and
  secrets, HTTP only to approved hosts, a per-plugin store, a 60 s `on_tick`, a test
  button, a doctor check and run metrics. The plan is to build integrations as bundled
  plugins on that base rather than as native code.
- 031 is the first integration that needs more than "POST on an event": a sign-in that
  takes several steps with the user in the loop, a library picker whose choices come from
  the server, a path mapping, a grouping window shorter than a minute, health checks the
  doctor can run, and counters of its own. Each of these is added to the plugin host as a
  small, general feature that later integrations reuse, and recorded below under
  "Host additions".
- When downloads go through Radarr or Sonarr (018), the *arr app imports them into the
  library and can tell the media server itself (its own Connect setting). This feature is
  for files putio-sync saves straight into a library folder; files outside every library
  are skipped and counted, so turning it on in an *arr setup does no harm.
- An unconfigured daemon runs in setup mode, where every route except the wizard answers
  503.

## Clarifications

### Session 2026-11-04

Answered with the recommended option, without a live user; listed in the hand-over note
for review.

- Q: One plugin per server type, or one "media server" plugin with a type field? → A: One
  bundled plugin each: `plex`, `jellyfin`, `emby`. Each has its own sign-in and settings
  form, and a user may run two at once (Plex and Jellyfin on the same library is common).
  Jellyfin and Emby share code, like the notifiers share `notifier.lua`.
- Q: How does Jellyfin or Emby sign in? → A: With an API key made in the server's
  dashboard (a secret field). That is how both document third-party access, and neither has
  a plex.tv-style PIN for API keys. Jellyfin Quick Connect sign-in is a follow-up; the
  action mechanism built here is enough for it.
- Q: Which Plex sign-in? → A: The PIN flow. "Sign in with Plex" asks plex.tv for a PIN,
  shows the 4-character code with a link to `plex.tv/link` and a direct link that opens
  the Plex sign-in page with the code filled in, and checks every 2 seconds until the user
  approves it or 10 minutes pass. The token is saved as a secret; it is never pasted or
  shown. A pasted token still works for people who prefer it (the field is a normal
  secret).
- Q: What does "choosing the library" mean when files can land in several? → A: A list of
  libraries to watch, fetched from the server; empty means "every library". A folder is
  refreshed in the library whose folder holds it. A folder in no chosen library is skipped
  and counted.
- Q: What is grouped, and for how long? → A: Refreshes are grouped per folder (the
  folder holding the finished file). A folder is refreshed once no new file landed in it for
  `delay_seconds` (default 20, 0 to 600), at most `max_wait_seconds` (default 300) after its
  first file, and at once when a sync run ends. A season of 20 episodes saved in one run
  is one refresh of that season's folder. Jellyfin and Emby send all due folders in one
  request.
- Q: Is anything native? → A: No integration code is native. The host gains six general
  features (below). Nothing in the brief needs native code: the PIN flow, library listing,
  refresh calls and health checks are plain HTTPS calls a plugin can make.
- Q: Which secret values does an action see when the form holds a new, unsaved secret? →
  A: The saved ones only. Only non-secret form values travel with an action, so a typed
  secret never goes anywhere before Save; an action that needs a secret the user has not
  saved yet says so ("Save the API key first").
- Q: Who drives a multi-step sign-in: the daemon or the page? → A: The page. Each step is
  one short action call; the result's `next` tells the page which action to call after how
  many seconds and with what input. Closing the page stops the flow, and the daemon keeps
  no sign-in state. A flow ends after `next` has been followed for 10 minutes.
- Q: How many action calls may run at once for one plugin? → A: One. A second call while
  one runs answers 409, like a second test. The doctor's health call uses the same rule
  and reports "busy, skipped" rather than waiting.

## Host additions

Each is general: it names no media server and is documented in `PLUGINS.md` for any
plugin. All are additive, so `api_version` stays 1.

1. **Actions** (sign-in and other buttons). A manifest lists `[[actions]]` (`id`, `label`,
   `help`, `hidden`). Settings shows each one not `hidden` as a button. Pressing it calls the plugin's
   `on_action(id, input)` and shows what it returns: a message, a link to open, a code to
   show, a request to call another action after some seconds (with input for it, so a
   multi-step flow such as a PIN poll needs no stored state), and values to save into the
   plugin's settings. Saved values go through the same checks as a Settings save; secret
   values are stored as secrets and only "saved" comes back to the browser.
2. **Choices from the plugin**. A config field may name an action in `choices`. Settings
   shows a "Load" button next to the field that calls the action and offers the
   `{value, label}` list it returns: a drop-down for a text or URL field, a list of ticks
   for a list field. A typed value still works.
3. **Path-mapping field type**. A config field of type `path_map` holds a list of
   `{from, to}` rules (a path on this machine → the path another program sees). The host
   API gains `putio.path.map(path, rules)` (the longest matching `from` on a whole folder
   boundary wins; Windows-style `to` paths get `\` separators), `putio.path.parent(path)`
   and `putio.path.within(path, folder)`.
4. **Per-plugin tick interval**. A manifest may set `tick_seconds` (5 to 3600, default 60).
   The plugins ticker wakes every 5 s and queues `on_tick` for each plugin whose interval
   has passed.
5. **Health checks**. A plugin may define `health()`, returning a list of
   `{status = "ok"|"warn"|"fail", message}` items. The doctor's Plugins check calls it for
   each enabled plugin (within the plugin's time limit) and reports `fail` as a problem and
   `warn` as a warning, naming the plugin.
6. **Plugin counters**. `putio.metrics.count(name, n?)` adds to
   `putio_sync_plugin_counter_total{plugin, name}`; names are `[a-z0-9_]{1,40}` and each
   plugin may use at most 16, so label cardinality stays bounded.

Two smaller host facts are added for these plugins and are as general:
`putio.download_dir` (the download folder, so a plugin can check a mapping) and a shared
store per plugin, so an action or health call run on its own worker sees and keeps the same
stored values as the running plugin.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - New episodes show up in Plex within seconds (Priority: P1)

A user's Plex server watches `/data/TV` and `/data/Movies`, and putio-sync saves into
`/downloads/TV` and `/downloads/Movies` on the same NAS (the Plex container mounts the
download folder as `/data`). They open Settings › Plugins › plex, press **Sign in with
Plex**, approve the code in the Plex tab that opens, pick their server from the list, map
`/downloads` to `/data`, leave "Libraries" empty, enable the plugin and save. When an
episode lands in `/downloads/TV/Show/Season 01/`, Plex scans only
`/data/TV/Show/Season 01` and the episode appears within about half a minute.

**Why this priority**: Plex is the most used server and the reason for the feature.

**Independent Test**: point the plugin at a mock Plex server whose library folder is
`/data/TV`, set the mapping, send `download.finished` for
`/downloads/TV/Show/Season 01/e1.mkv`, advance past the delay; the mock receives one
`/library/sections/<id>/refresh?path=/data/TV/Show/Season%2001` with the token.

**Acceptance Scenarios**:

1. **Given** the plugin enabled with a server, a token and a mapping, **When** a file
   lands in a library folder, **Then** that file's folder (mapped) is refreshed in the
   library that holds it, once, within `delay_seconds` plus 5 seconds.
2. **Given** 20 episodes land in one season folder within the window, **Then** that
   folder is refreshed once.
3. **Given** files in two folders, **Then** each folder is refreshed once.
4. **Given** a file outside every chosen library, **Then** nothing is refreshed, it is
   logged at debug and counted as skipped.
5. **Given** the server is down or answers an error, **Then** the run is recorded as
   failed with the host and status, the folder is tried once more on the next window,
   and downloads carry on.
6. **Given** any of this, **Then** the Plex token appears in no log line, run error, API
   answer, export, doctor text or metric.

---

### User Story 2 - Sign in to Plex without copying a token (Priority: P1)

A user presses **Sign in with Plex**. A code and two links show: one opens the Plex sign-in
page with the code filled in, the other is `plex.tv/link` for signing in from a phone. After
they approve it, the page says "Signed in to Plex" within a few seconds, the Token field
shows as set, and the server list can be loaded.

**Why this priority**: a pasted token is the main reason such integrations are abandoned;
without sign-in the plugin is hard to use.

**Independent Test**: with a mock plex.tv, press the action: the PIN is requested with the
plugin's client id; the first poll returns no token, the second returns one; the result
saves the token as a secret and the API answer holds no token.

**Acceptance Scenarios**:

1. **Given** the Plex card, **When** the user presses Sign in, **Then** a code and links
   show and the page checks the PIN every 2 seconds.
2. **Given** the user approves, **Then** the token is saved as the plugin's secret, the
   card shows "set", and a message names the Plex account.
3. **Given** 10 minutes pass or the user leaves the page, **Then** checking stops and
   the card says the code expired; nothing is saved.
4. **Given** the plugin was signed in before, **When** the user signs in again, **Then**
   the new token replaces the old one.

---

### User Story 3 - Pick the server and libraries from a list (Priority: P2)

After signing in, the user presses **Load** next to "Server" and sees their Plex servers
with each address (local first). They pick one. Then **Load** next to "Libraries" lists
the server's libraries with their folders; they tick "TV Shows" only.

**Why this priority**: typing a server URL and library ids is error-prone; but the plugin
works without it (a typed URL, every library).

**Acceptance Scenarios**:

1. **Given** a signed-in plugin, **When** the user loads servers, **Then** each Plex server
   on the account is listed by name and address, and choosing one fills the Server field.
2. **Given** a server, **When** the user loads libraries, **Then** each movie, show and
   music library is listed with its folders; ticking some limits refreshes to those.
3. **Given** the plugin cannot reach plex.tv or the server, **Then** the Load button shows
   the error and the field keeps what was typed.
4. **Given** unsaved changes to the Server field, **Then** loading libraries uses the
   value in the form, not the saved one.

---

### User Story 4 - Jellyfin and Emby (Priority: P2)

A user running Jellyfin (or Emby) enables the `jellyfin` (or `emby`) plugin with the
server URL and an API key from the server's dashboard, maps paths if needed, and gets the
same per-folder refresh. The library picker lists the server's libraries.

**Acceptance Scenarios**:

1. **Given** the jellyfin plugin enabled, **When** files land in two folders within the
   window, **Then** one "media updated" request names both mapped folders.
2. **Given** the emby plugin, **Then** the same holds with Emby's API.
3. **Given** a wrong API key, **Then** the run fails with "HTTP 401" and the doctor says
   the key was refused.

---

### User Story 5 - Know it works: test button, doctor, metrics (Priority: P2)

A user presses **Send test event** on the plex card and sees "Connected to <server name>:
3 libraries; /downloads maps to /data, inside TV Shows, Movies". The doctor's Plugins check
reports a problem when the server cannot be reached or the token is refused, and a warning
when the download folder maps to no library folder. Metrics count refreshes by result.

**Acceptance Scenarios**:

1. **Given** Send test event, **Then** the plugin checks the server, the token and the
   mapping and logs one line for each; the run fails if the server or token fails.
2. **Given** an enabled media plugin and an unreachable server, **Then** the doctor's
   Plugins check is a problem naming the plugin and the host.
3. **Given** a refused token, **Then** the doctor reports a problem saying to sign in
   again.
4. **Given** the download folder maps to a folder no chosen library contains or is inside,
   **Then** the doctor warns with the mapped path and the library folders.
5. **Given** the metrics endpoint, **Then** `putio_sync_plugin_counter_total{plugin="plex",
   name="refreshes"}`, `name="refresh_failures"`, `name="folders_grouped"` (files folded into
   an already pending folder) and `name="skipped"` are present after use.

### Edge Cases

- **No mapping**: the media server sees the same paths; folders are used as they are.
- **Overlapping rules**: `/downloads` → `/data` and `/downloads/TV` → `/tv`: the longest
  matching rule wins, on whole folder names only (`/downloads2` does not match
  `/downloads`).
- **Plex on Windows**: a `to` of `D:\Media` maps `/downloads/TV/x` to `D:\Media\TV\x`;
  library folders are compared without case.
- **Trailing slashes** on rules and library folders are ignored.
- **A file straight in the download folder**: its folder is the download folder; it is
  refreshed only if that folder is inside a library.
- **A library holding the folder twice** (nested library folders, or two libraries): the
  library with the longest matching folder wins; with a tie each is refreshed.
- **Settings change or reload while folders wait**: waiting folders are dropped (logged);
  the next download of that folder refreshes it. Grouping state is not kept across a
  restart.
- **Plugin disabled**: the sign-in and Load buttons still work, so it can be set up before
  it is turned on.
- **Sign-in while the token field is being replaced in the form**: the saved token wins;
  the form shows "set".
- **Action results that save unknown keys or wrong types**: refused with the field named;
  nothing is saved.
- **Plex shared servers**: the server list includes servers shared with the account; their
  own access token is not used (the account token is), so refreshing a shared server
  usually fails with 401 and the doctor says so. Owned servers are the supported case.
- **Self-signed or plex.direct certificates**: plex.direct addresses have valid
  certificates; a self-signed server is used over http on the LAN.
- **Setup mode**: the new routes answer 503 like every other route.

## Requirements *(mandatory)*

### Functional Requirements

Host (general, for any plugin):

- **FR-001**: A manifest MUST accept `[[actions]]` with `id` (`[a-z0-9_]{1,40}`), `label`
  and `help`, at most 8 per plugin; a field's `choices` MUST name one of them.
- **FR-002**: The API MUST offer `POST /plugins/action` with the plugin name, the action
  id, an input object and the form's unsaved non-secret config values; it MUST run the
  plugin's `on_action(id, input)` on a worker of its own, within the plugin's time and
  memory limits, enabled or not, with the hosts it needs for those values allowed for that
  call, and return the result without secret values.
- **FR-003**: An action result MAY hold `message`, `code`, `open_url` (http or https),
  `choices` (a list of `{value, label}`), `next` (`{action, input, after_seconds}`),
  `done`, and `save` (config keys to values). Saved values MUST be checked against the
  manifest and the settings rules, written to the config file and applied at once; the
  answer MUST name the saved keys and give non-secret values only. Only saved secrets
  reach an action; one action call per plugin runs at a time (a second answers 409).
- **FR-004**: A config field type `path_map` MUST hold a list of `{from, to}` rules of
  absolute paths (at most 16), stored in the config file, edited in Settings as rows.
- **FR-005**: The host API MUST add `putio.path.map`, `putio.path.parent`,
  `putio.path.within`, `putio.download_dir`, and `putio.metrics.count`.
- **FR-006**: A manifest MUST accept `tick_seconds` (5 to 3600, default 60), and ticks
  MUST be queued per plugin at that interval (within 5 s).
- **FR-007**: A plugin MAY define `health()`; the doctor's Plugins check MUST call it for
  enabled plugins and report `fail` items as problems and `warn` items as warnings, and a
  health call that errors or times out as a problem.
- **FR-008**: `putio.metrics.count` MUST feed `putio_sync_plugin_counter_total{plugin,name}`
  with at most 16 names per plugin; further names MUST be refused with an error.
- **FR-009**: A plugin's store MUST be shared by every worker of that plugin (running,
  test, action, health), so writes are never lost to a second copy.

Media servers:

- **FR-010**: putio-sync MUST bundle `plex`, `jellyfin` and `emby` plugins, off by default,
  that handle `download.finished` and `sync.completed`.
- **FR-011**: On `download.finished`, a plugin MUST map the file's folder with its path
  mapping, find the chosen (or any) library holding it, and queue that folder; the folder
  MUST be refreshed once no file landed in it for `delay_seconds`, at most
  `max_wait_seconds` after its first file, or at once on `sync.completed`.
- **FR-012**: The plex plugin MUST refresh one folder in one library per request; the
  jellyfin and emby plugins MUST send all due folders in one request.
- **FR-013**: A refresh that fails MUST be retried once at the next due time, then dropped
  and counted as a failure.
- **FR-014**: The plex plugin MUST offer "Sign in with Plex" (PIN flow with a stable client
  id kept in its store), "Load servers" for the Server field and "Load libraries" for the
  Libraries field; jellyfin and emby MUST offer "Load libraries".
- **FR-015**: Each media plugin MUST define `health()`: the server is reachable, the token
  or API key is accepted, and the download folder maps into a chosen library; and its test
  event MUST run the same checks and log the results.
- **FR-016**: Each media plugin MUST count `refreshes`, `refresh_failures`,
  `folders_grouped` and `skipped`.
- **FR-017**: No Plex token, Jellyfin or Emby key MUST appear in logs, run errors, API
  answers, exports, the doctor or metrics; requests MUST send tokens in headers, not URLs.
- **FR-018**: `PLUGINS.md`, the README, both example configs and the changelog MUST
  describe the media plugins and each host addition.

### Key Entities

- **Plugin action**: id, label, help; a result (message, code, link, choices, next step,
  saved values).
- **Path rule**: `from` (a path on this machine), `to` (the path the other program sees).
- **Pending folder** (in the plugin's memory): mapped path, library, first and last file
  time, file count, tries.
- **Plugin counter**: plugin, name, value.
- **Health item**: status (ok, warn, fail), message.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: With default settings, a finished episode is asked for in the media server
  within 25 seconds of landing.
- **SC-002**: 20 files landing in one folder within the window cause exactly one refresh
  request for that folder.
- **SC-003**: A user signs in to Plex, picks a server and library, and sees a passing test
  in under three minutes without copying any token.
- **SC-004**: No token or API key appears in any log line, API answer, export, doctor
  report or metric, in tests that set them and run every path.
- **SC-005**: A media server that is down never delays a download or another plugin.

## Assumptions

- The media server can read the downloaded files under the mapped path; putio-sync does
  not check file permissions on the server side.
- Plex's "partial scan" of a folder (`refresh?path=`) and Jellyfin/Emby's "media updated"
  call are enough to pick up new files; a full library scan is never started.
- The plex.tv API (PINs, resources) and the Plex, Jellyfin and Emby HTTP APIs are tested
  against mock servers only (Constitution III); a check against real servers is listed in
  the hand-over.
- The grouping state lives in the plugin's memory; the client id lives in its store.
- An action may contact the hosts the plugin needs for the values in the form when the
  user presses it; pressing the button counts as approving them for that call, as saving
  does for the plugin's runs.
