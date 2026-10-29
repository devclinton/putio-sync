# Feature Specification: Radarr/Sonarr Settings and Auto-Setup

**Feature Branch**: `018-arr-autosetup`

**Created**: 2026-10-26

**Status**: Draft

**Input**: User description: "it would be nice to have an autosetup radar/sonarr button (with
discovery and ability to manually specify address). just spec now, dont plan or do anything".
Later the user agreed to fold in a rework of the Radarr / Sonarr settings, so they work the way
the *arr ecosystem is normally set up: forms in the UI with Test and Save, the category kept
only on the *arr side, status moved off the Settings page, and help with path mapping.

## Clarifications

### Session 2026-10-26

- Q: May the web UI save the *arr API key to the config file? → A: Only when the operator ticks
  "Remember" in auto-setup; otherwise the key is used for that setup only. (Saving an
  application through the Add/Edit form is an explicit save and keeps the key.)
- Q: What should setup do with an enabled Torrent Blackhole client in the *arr? → A: Offer an
  opt-in "Disable Torrent Blackhole" checkbox, unticked by default.
- Q: What if putio-sync's download client is disabled or has no password? → A: Setup turns the
  client on and generates a password itself.
- Q: How should Radarr/Sonarr settings be managed? → A: Like the rest of the *arr ecosystem
  (Prowlarr, Overseerr, qBittorrent): edited in the UI with Test and Save, not only in the config
  file. The category is read from the *arr, not typed into putio-sync. Item counts and last
  contact move to the Status page.

## Context

- Feature 011 made putio-sync act as a Transmission download client for Radarr and Sonarr, at
  URL base `/transmission/` on the web UI port, with its own username and password.
- Feature 011 also lets putio-sync call each *arr's API (an "application": kind, address, API
  key, category) so failed releases are removed and blocklisted in the *arr.
- Today's Settings → "Radarr / Sonarr" card has these problems:
  1. It is read-only. The download client and the applications are set in the config file
     (`[arr]`, `[[arr.servers]]`), followed by a restart. The *arr apps, Prowlarr, Overseerr and
     qBittorrent are all set up in their UI instead: a form, then Test, then Save.
  2. Applications only have a Test button. Prowlarr and Overseerr use an "Add Radarr / Add
     Sonarr" form (host, port, SSL, URL base, API key, Test, Save) with edit and delete.
  3. The category is typed into putio-sync for each application, and must match the category
     set on the download client inside Radarr/Sonarr. In the ecosystem the category lives only on
     the *arr side.
  4. Status is mixed into settings: item counts and "last contact".
  5. The download folder is shown with no help on path mapping: the folder putio-sync reports
     must be visible to Radarr/Sonarr at the same path, or the *arr needs a Remote Path Mapping.
  6. Connecting an *arr means copying host, port, URL base, username and password by hand. The
     password cannot be copied because the page never shows it, and the host the browser uses is
     not always one the *arr can reach from its container.
- Radarr's and Sonarr's API, reached with the API key from their Settings → General page, can
  list, add, update and test download clients, and list a folder as the *arr sees it.
- On the user's NAS, putio-sync, Radarr and Sonarr run as Docker containers on one network, on
  the default ports (7878, 8989), and all three see the download folder at `/downloads`. Both
  *arr apps use a Torrent Blackhole today and have disabled qBittorrent/Transmission clients.
- The Settings page already saves forms to the config file, keeps secrets it does not show, and
  says whether each change applies now or after a restart.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Set up a discovered Radarr or Sonarr in one click (Priority: P1)

In Settings → Radarr / Sonarr → Applications, the operator clicks "Set up automatically".
putio-sync looks for Radarr and Sonarr instances it can reach and lists each with its kind,
address and, once known, version. The operator picks one, pastes its API key, checks the
proposed category (`radarr` or `sonarr`), and clicks "Set up". putio-sync adds itself to that
*arr as a Transmission download client with the right host, port, SSL, URL base, username,
password and category, runs the *arr's own test of that client, and shows the result.

**Why this priority**: This is the quick path. It removes the manual copy step, including the
password the operator cannot see, and the guesswork about which host the *arr can reach.

**Independent Test**: Against two mocked *arr APIs (one Radarr, one Sonarr) at candidate
addresses, start discovery and check both are listed. Set up each with a valid API key. Check
each mock received one new, enabled Transmission client with putio-sync's URL base, username,
password and the chosen category, and that the result is a success naming the *arr and version.

**Acceptance Scenarios**:

1. **Given** a Radarr answers at a candidate address, **When** the operator starts auto-setup,
   **Then** that Radarr is listed with its kind and address within the discovery time limit.
2. **Given** a listed Radarr and its valid API key, **When** the operator clicks "Set up",
   **Then** the Radarr has a new, enabled Transmission download client pointing at putio-sync
   with category `radarr` (or the one entered), and the result shows success with the Radarr's
   name and version.
3. **Given** setup succeeded, **When** the operator presses "Test" on that client inside Radarr,
   **Then** the test passes with no further changes.
4. **Given** a Radarr and a Sonarr were found, **When** the operator sets up each, **Then** each
   gets its own client and category, and setting up one does not change the other.
5. **Given** setup succeeded, **When** the Radarr next grabs a release, **Then** it reaches
   putio-sync through the download client (feature 011).
6. **Given** putio-sync's download client is disabled or has no password, **When** the operator
   opens auto-setup, **Then** the view says the download client will be turned on and, if no
   password is set, that one will be generated; **When** setup runs, **Then** both happen and
   take effect without a restart. The generated password is never sent to the browser.
7. **Given** "Remember" is ticked, **When** setup succeeds, **Then** the *arr is added to the
   Applications list with its API key saved. **Given** it is not ticked, **Then** the key is
   discarded after setup, and the *arr is not added to the list.

---

### User Story 2 - Enter an address by hand when discovery finds nothing (Priority: P1)

Discovery may find nothing: the *arr runs on another machine, on a custom port, behind a URL
base (for example `http://nas:7878/radarr`), or on a network putio-sync cannot see. The operator
can always type the address and API key instead.

**Why this priority**: Discovery can only guess. Without a manual path, auto-setup fails for any
install that is not on default ports next to putio-sync.

**Independent Test**: With no *arr at any candidate address, start discovery and check it says
nothing was found and offers manual entry. Enter the address of a mocked Sonarr under a URL base,
with a valid key, and check setup succeeds as in User Story 1.

**Acceptance Scenarios**:

1. **Given** no *arr is found, **When** discovery finishes, **Then** the operator sees "No
   Radarr or Sonarr found", the addresses tried, and the manual-entry fields.
2. **Given** discovery found some instances, **When** the operator wants a different one,
   **Then** manual entry is still available.
3. **Given** a bare host (for example `nas`), **When** submitted, **Then** putio-sync tries HTTP
   on the default Radarr and Sonarr ports and uses the one that answers as an *arr.
4. **Given** an address that answers but is not Radarr or Sonarr, **When** submitted, **Then**
   the result says "Not a Radarr or Sonarr server" and nothing is changed.

---

### User Story 3 - Manage the download client in the UI (Priority: P1)

The "Download client" section of the card is a form: an on/off switch, the username, and a
password field with a "Generate" button. Saving writes them to the config file and takes effect
without a restart. Below the form, the connection details to copy into an *arr by hand: type
Transmission, host, port, SSL and URL base.

**Why this priority**: Today the download client can only be turned on by editing the config file
and restarting, which every other app in the ecosystem avoids.

**Independent Test**: With the download client off and no password, turn it on, click Generate,
and save. Check the config file holds the new state and a password, other settings and secrets
are unchanged, the Transmission endpoint now accepts the new credentials without a restart, and
the password is not returned by any later response.

**Acceptance Scenarios**:

1. **Given** the Settings page, **When** the operator opens the Radarr / Sonarr card, **Then**
   the Download client section shows the on/off state, the username, and the password as "set"
   or "not set" (never its value).
2. **Given** the operator clicks Generate, **When** the password field fills, **Then** the new
   password can be revealed and copied until the form is saved or left, so it can be typed into
   an *arr by hand.
3. **Given** the operator saves with the password field left empty, **When** a password is
   already set, **Then** the existing password is kept.
4. **Given** the password changed, **When** the save succeeds, **Then** the page warns that any
   *arr set up with the old password must be updated, and offers to update the remembered
   applications that have a putio-sync client (User Story 4).
5. **Given** the password or on/off state is set by an environment variable, **When** the card
   opens, **Then** that field is read-only and says "set by environment".
6. **Given** the operator turns the client on with no password, **When** they save, **Then** the
   save is refused with "Set or generate a password first".

---

### User Story 4 - Add, edit and delete applications like Prowlarr and Overseerr (Priority: P2)

The "Applications" section lists the Radarr and Sonarr instances putio-sync knows about. Each
row shows the kind, a name, the address, the category read from that *arr, and whether its
putio-sync download client is present. "Add Radarr" and "Add Sonarr" open a form with host,
port, SSL, URL base and API key, plus Test and Save. Rows can be edited and deleted.

**Why this priority**: Applications let putio-sync blocklist failed releases (feature 011).
Editing them by config file and restart is the main thing users of other *arr tools do not
expect.

**Independent Test**: Against a mocked Radarr whose putio-sync client has category `movies`, add
it through the form, Test, then Save. Check the row shows category `movies` read from the *arr,
the config file holds the application with its key, and a failed release with category `movies`
is blocklisted in that Radarr. Edit the port, then delete the row, and check the config file
each time.

**Acceptance Scenarios**:

1. **Given** the Add Radarr form with a valid address and key, **When** the operator clicks Test,
   **Then** it shows the *arr's name and version, or the reason it failed (as in feature 011).
2. **Given** a passing test, **When** the operator clicks Save, **Then** the application is
   saved to the config file, listed, and used for blocklisting without a restart.
3. **Given** a saved application, **When** the list loads or the operator tests it, **Then** the
   category shown is the one on putio-sync's download client inside that *arr. If that *arr has
   no client pointing at putio-sync, the row says "Not set up" and offers auto-setup for it.
4. **Given** an existing application, **When** the operator edits it and leaves the API key
   field empty, **Then** the saved key is kept. The key is never shown, only "set".
5. **Given** an existing application, **When** the operator deletes it and confirms, **Then** it
   is removed from the config file. Nothing is changed inside the *arr.
6. **Given** a Save whose test fails, **When** the operator clicks Save, **Then** the page warns
   that the test failed and saves only if the operator confirms.
7. **Given** applications listed in the config file by an earlier version (`[[arr.servers]]`
   with a `category`), **When** the card loads, **Then** they appear in the list, and the
   category read from the *arr is used; a typed category that differs is shown as a warning.

---

### User Story 5 - Existing download clients in the *arr are handled safely (Priority: P2)

The *arr may already have download clients: putio-sync from an earlier setup, a Torrent
Blackhole, or other torrent clients. Before changing anything, auto-setup shows what it found and
what it will do. Running it again updates putio-sync's client instead of adding another.

**Why this priority**: The NAS already has blackhole and disabled clients. A careless setup would
leave two routes for the same release, or overwrite a client still in use.

**Independent Test**: Seed a mocked *arr with (a) a Transmission client pointing at putio-sync
with a wrong password, (b) a Torrent Blackhole and (c) an unrelated qBittorrent client. Run setup.
Check (a) is updated in place, (b) is unchanged unless "Disable Torrent Blackhole" was ticked,
(c) is unchanged, and the summary listed all three.

**Acceptance Scenarios**:

1. **Given** a Transmission client already pointing at putio-sync (same host, port and URL base,
   or created by auto-setup), **When** setup runs, **Then** it is updated, not duplicated.
2. **Given** a client with the name putio-sync would use but pointing elsewhere, **When** setup
   runs, **Then** it is not overwritten; putio-sync uses another name and says so.
3. **Given** an enabled Torrent Blackhole client, **When** the operator reviews the setup,
   **Then** the summary warns that releases may still go to the blackhole and offers a "Disable
   Torrent Blackhole" checkbox, unticked by default.
4. **Given** that box is ticked, **When** setup succeeds, **Then** the blackhole client is
   disabled (not deleted) and the result says so. Unticked, or on failure, it is unchanged.
5. **Given** other enabled torrent clients, **When** setup runs, **Then** they are unchanged, and
   putio-sync's client gets the highest priority the *arr allows.
6. **Given** putio-sync's client already exists and is correct, **When** setup runs again,
   **Then** nothing changes and the result says it was already set up.

---

### User Story 6 - Path mapping help and check (Priority: P2)

Next to the reported download folder, the card explains that Radarr/Sonarr must see this folder
at the same path, or need a Remote Path Mapping, with a short example for Docker. For each
application, putio-sync checks whether that *arr can see the download folder, and shows the
result.

**Why this priority**: A wrong path is the most common reason a working download client never
imports. The *arr only reports it after a download fails to import.

**Independent Test**: Against a mocked Radarr that can list `/downloads`, check the result is OK.
Against one that cannot, check the result says the folder is not visible to Radarr, and shows
any Remote Path Mapping it has for putio-sync's host.

**Acceptance Scenarios**:

1. **Given** the Download client section, **When** the operator reads it, **Then** the download
   folder is shown with a one-line explanation and a link to the README's path-mapping section.
2. **Given** a saved application, **When** it is tested or set up, **Then** putio-sync asks that
   *arr whether it can see the download folder, applying the *arr's Remote Path Mappings for
   putio-sync's host, and shows "Folder visible" or "Radarr cannot see /downloads".
3. **Given** the folder is not visible, **When** the result shows, **Then** it says what to do:
   mount the same folder at the same path in the *arr's container, or add a Remote Path Mapping
   in the *arr from the putio-sync path to the path the *arr uses.

---

### User Story 7 - Status moves to the Status page (Priority: P3)

Download-client item counts (on Put.io, downloading, done, failed) and each *arr's last contact
move from Settings to the Status page. Settings keeps only forms and the values to copy.

**Why this priority**: It keeps Settings to settings. The information still matters, so it moves
rather than going away.

**Independent Test**: Load Settings and check it shows no item counts or last-contact times.
Load Status and check both appear there, with the same values the API reports.

**Acceptance Scenarios**:

1. **Given** the download client is on, **When** the operator opens Status, **Then** a "Radarr /
   Sonarr" card shows item counts by state and when each *arr last made contact.
2. **Given** the download client is off, **When** the operator opens Status, **Then** the card
   says it is off and links to Settings.

---

### User Story 8 - Clear results and failure messages (Priority: P2)

Every test, save and setup ends with one clear result: what was created or changed, the *arr's
own test result, and on failure a reason the operator can act on.

**Why this priority**: These actions touch another app's settings. The operator must know
exactly what happened, especially when it did not work.

**Independent Test**: Against mocked *arr APIs, cause each failure in turn: unreachable, wrong
key, not an *arr, unsupported version, the *arr cannot reach putio-sync, the *arr's test fails
for another reason, the *arr refuses the change, the config file cannot be written. Check each
message, and that no failure leaves a new, failing client in the *arr.

**Acceptance Scenarios**:

1. **Given** the address does not answer, **When** a test or setup runs, **Then** the result is
   "Unreachable" (or "Unreachable (timed out)") with the address tried.
2. **Given** a wrong API key, **When** a test or setup runs, **Then** the result is "API key
   refused" and nothing is changed.
3. **Given** the *arr cannot reach putio-sync at any host putio-sync proposed, **When** the *arr
   tests the new client, **Then** the client is removed again (or restored), and the result is
   "Radarr could not reach putio-sync" with the hosts tried and a field to enter the host.
4. **Given** the *arr's test of the client reports another problem, **When** setup finishes,
   **Then** the result shows the *arr's own message, and the client is removed or restored.

### Edge Cases

- putio-sync runs on the host network or outside Docker: the host proposed to the *arr is the
  machine's LAN address, not a container name.
- The web UI is reached through a reverse proxy or a name the *arr cannot resolve: auto-setup
  tries the other candidate hosts and reports which one the *arr accepted.
- HTTPS *arr addresses are accepted. An untrusted certificate is reported as such, not as
  "unreachable".
- Discovery does not guess URL bases; instances behind one are added by hand.
- Two instances of the same kind (Radarr and Radarr 4K): each is listed, set up and saved on its
  own. Setup warns when the category is already used by another application, because the
  category decides which *arr is asked to blocklist a failed item.
- An application whose *arr has two putio-sync clients with different categories: both
  categories are shown and used.
- The category read from an *arr cannot be fetched (the *arr is down): the last known category
  is used and shown with its age.
- The category entered during auto-setup is empty or not a valid folder name: refused before
  anything is sent.
- The download-client password is changed while *arr apps use the old one: their requests are
  refused until updated (User Story 3, scenario 4).
- Turning the download client off while items are tracked: items stay tracked and the
  Transmission endpoint refuses requests, as in feature 011.
- An *arr version too old for the client options putio-sync sets: refused with the version found
  and the minimum supported.
- Discovery started twice, or the page left during it: one discovery runs at a time, and a late
  result for a closed page is dropped.
- The config file is read-only (for example mounted read-only): saves fail with "Could not write
  the config file" and nothing changes in the running daemon.

## Requirements *(mandatory)*

### Functional Requirements

**Settings layout**

- **FR-001**: The Radarr / Sonarr card on Settings MUST have two sections: "Download client"
  and "Applications". It MUST NOT show item counts or last-contact times.
- **FR-002**: The Status page MUST show a "Radarr / Sonarr" card with download-client item counts
  by state and each *arr's last contact, moved from Settings.

**Download client section**

- **FR-003**: The section MUST be a form with an on/off switch, a username and a write-only
  password field with a "Generate" button, saved to the config file with Save.
- **FR-004**: Saving MUST take effect without a restart and MUST keep every other setting and
  secret in the config file. Leaving the password field empty MUST keep the current password.
- **FR-005**: Generate MUST create a strong random password in the form. It MAY be revealed and
  copied until the form is saved or left. After saving, the password MUST only be shown as
  "set".
- **FR-006**: Turning the client on without a password MUST be refused with a clear message.
- **FR-007**: A field set by an environment variable MUST be read-only in the form and say "set
  by environment".
- **FR-008**: After a password change, the page MUST warn that *arr apps set up with the old one
  must be updated, and MUST offer to update the putio-sync client in each saved application.
- **FR-009**: Below the form, the section MUST show the values to enter in an *arr by hand: type
  Transmission, host, port, SSL and URL base, and the reported download folder with path-mapping
  guidance (FR-024).

**Applications section**

- **FR-010**: The section MUST list saved applications with kind, name, address, the category
  read from the *arr (FR-014), whether a putio-sync client is present in it, and Test, Edit and
  Delete actions.
- **FR-011**: "Add Radarr" and "Add Sonarr" MUST open a form with name, host, port (default
  7878 or 8989), SSL, URL base and API key, with Test and Save. Edit MUST open the same form.
- **FR-012**: Save MUST write the application, with its API key, to the config file and use it
  without a restart. Saving after a failed test MUST require confirmation.
- **FR-013**: Delete MUST ask for confirmation and remove the application from the config file
  only. It MUST NOT change anything inside the *arr.
- **FR-014**: putio-sync MUST NOT ask for a category for an application. It MUST read the
  category from that *arr's download client(s) pointing at putio-sync, when the application is
  saved or tested and at regular intervals, and keep the last known value. Blocklisting
  (feature 011) MUST pick the application whose putio-sync client uses the failed item's
  category.
- **FR-015**: Applications listed in the config file by an earlier version MUST keep working. A
  typed `category` there MUST be ignored in favour of the one read from the *arr, with a warning
  in the list when they differ.

**Auto-setup: discovery and manual entry**

- **FR-016**: The Applications section MUST offer "Set up automatically", which starts discovery
  and opens the setup view. Applications with no putio-sync client MUST also offer it.
- **FR-017**: Discovery MUST check a bounded list of candidate addresses, not sweep the network:
  saved applications; the names `radarr` and `sonarr` (Docker containers on the same network);
  the machine putio-sync runs on and its Docker host; and the host the browser uses for the web
  UI, each on ports 7878 and 8989. It MUST identify each answer as Radarr or Sonarr without an
  API key, list nothing else, finish within 10 seconds, list every address tried, and list an
  instance found at two addresses once.
- **FR-018**: The setup view MUST always offer manual entry of an address and API key. A full
  URL (scheme, host, port, path) or a bare host MUST be accepted; for a bare host, HTTP on the
  default ports MUST be tried.

**Auto-setup: changes in the *arr**

- **FR-019**: Before changing anything, setup MUST check the address and key, show the *arr's
  kind and version, and summarise what it will do: create or update which client with which name
  and category, and which existing clients it found.
- **FR-020**: Setup MUST create, or update in place, one enabled Transmission download client in
  the *arr with putio-sync's host, port, SSL, URL base, username, password and the chosen
  category (default `radarr`/`sonarr`, editable, must be a valid folder name), at the highest
  priority the *arr allows. Clients that do not point at putio-sync MUST NOT be changed, except
  as FR-021 allows.
- **FR-021**: An enabled Torrent Blackhole client MUST be shown with a warning and a "Disable
  Torrent Blackhole" checkbox, unticked by default. Only when ticked and setup succeeds MUST it
  be disabled (never deleted).
- **FR-022**: To choose the host the *arr should use, setup MUST try its candidate hosts
  (container name, LAN address, browser host) with the *arr's own download-client test and save
  the first that passes. The operator MUST be able to override the host.
- **FR-023**: If the *arr's test fails, setup MUST NOT leave a new failing client behind, MUST
  restore an updated client to its previous settings, and MUST report the *arr's message.
- **FR-023a**: If putio-sync's download client is off or has no password, setup MUST say so in
  advance, then turn it on and, when no password is set from the config file or environment,
  generate one, save both to the config file, and apply them without a restart. An environment
  password MUST be used as is. The generated password MUST NOT be sent to the browser.
- **FR-023b**: Setup MUST offer a "Remember" choice, off by default. Only when it is ticked and
  setup succeeds MUST the *arr be saved as an application with its API key (updating one with
  the same address). Otherwise the key MUST be used for that setup only and never written to
  disk.

**Path mapping**

- **FR-024**: The download folder MUST be shown with a short explanation of path mapping and a
  link to the README section on it.
- **FR-025**: When an application is tested or set up, putio-sync MUST ask that *arr whether it
  can see the download folder, applying the *arr's Remote Path Mappings for putio-sync's host,
  and MUST show the result with the fix when it cannot.

**Results, secrets and docs**

- **FR-026**: Each test, save and setup MUST end with one result: success (what changed, the
  *arr's name and version, its test passing), already set up, or a failure with one of these
  reasons and its detail: unreachable, timed out, untrusted certificate, API key refused, not a
  Radarr or Sonarr server, unsupported version, invalid category, the *arr could not reach
  putio-sync, the *arr's test failed (with its message), the *arr refused the change, or the
  config file could not be written. Each failure MUST say what to check next.
- **FR-027**: API keys MUST be entered in password-style fields and MUST NOT be shown back,
  returned in any response, written to logs or metrics, or included in error messages. Only
  "set" MAY be shown. The download-client password MUST follow the same rule, except in the form
  right after Generate (FR-005).
- **FR-028**: All new actions MUST require the same web UI authentication as the rest of
  Settings.
- **FR-029**: Each setup, application save and delete, and download-client change MUST be logged
  with what changed and the outcome, without keys or passwords.
- **FR-030**: The README's Radarr and Sonarr section MUST describe the Settings forms,
  auto-setup, manual entry and path mapping, and keep the manual steps. The example config MUST
  note that these settings can be edited in the UI and that `category` is no longer needed.

### Key Entities

- **Download-client settings** (from feature 011): on/off, username, password (secret). Now
  editable in the UI; fields set by environment are read-only.
- **Application** (feature 011's *arr API connection, reworked): kind (Radarr/Sonarr), name,
  address (host, port, SSL, URL base), API key (secret), the category or categories read from
  its putio-sync client with the time they were read, and whether that client is present.
- **Discovered instance**: kind, address, version when known, and how it was found. Kept only for
  the setup view.
- **Setup request**: target address, API key (secret, saved only when "Remember" is ticked),
  category, optional host override, and the "Disable Torrent Blackhole" and "Remember" choices.
- **Setup result**: target, outcome (created, updated, already set up, failed), reason and detail
  on failure, client name, the host the *arr accepted, the *arr's test message, and the
  path-mapping check result.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: On the user's NAS, Radarr and Sonarr can each be connected from the Settings page
  with only an API key typed in, in under 1 minute per app, with no config file edit and no
  restart, and the *arr's own download-client Test passes afterwards.
- **SC-002**: Discovery lists a Radarr and a Sonarr on default ports on the same Docker network
  in under 10 seconds.
- **SC-003**: Every Radarr / Sonarr setting that today needs a config file edit (download client
  on/off, username, password, applications) can be changed from the UI, and applies without a
  restart.
- **SC-004**: The operator never types a category into putio-sync. A failed release is
  blocklisted in the right *arr using the category read from it.
- **SC-005**: Running auto-setup twice against the same *arr leaves exactly one putio-sync client
  in it, and clients not pointing at putio-sync are unchanged unless the operator ticked
  "Disable Torrent Blackhole".
- **SC-006**: In the automated tests, every failure in FR-026 produces its own message, and none
  leaves a new, failing client in the *arr.
- **SC-007**: A wrong download-folder path is reported in Settings before the first download,
  not only after an import fails.
- **SC-008**: No API key or saved password appears in any page, API response, log line, metric or
  error message produced by the automated tests.

## Assumptions

- Builds on feature 011: the Transmission download-client emulation, the `[arr]` settings, the
  *arr API client, blocklisting and the per-application Test.
- Settings stay stored in the config file. The UI edits that file like the other Settings forms,
  and a matching environment variable still wins over the file.
- Radarr v3+ and Sonarr v3+ (current Radarr 5/6, Sonarr 4). Lidarr, Readarr and Whisparr are out
  of scope.
- The API key cannot be discovered; the operator copies it from the *arr's Settings → General.
  Reading the *arr's own files from a shared volume is out of scope.
- Discovery probes a short candidate list only. Subnet scans, mDNS/SSDP and the Docker socket are
  out of scope.
- putio-sync does not create Remote Path Mappings in the *arr; it checks and explains them.
- Deleting an application does not remove putio-sync's client from that *arr; the operator does
  that in the *arr.
- Setup never replaces an existing download-client password; only Generate and Save in the
  Download client form change it.
- The Status page card reuses the data the Settings card shows today; no new status data is
  collected.
- Feature 013 (first-run setup wizard, reserved in ROADMAP.md) lists Radarr/Sonarr as an
  optional step and can reuse this feature's setup view.
