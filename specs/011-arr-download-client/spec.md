# Feature Specification: Radarr/Sonarr Download Client

**Feature Branch**: `011-arr-download-client`

**Created**: 2026-10-26

**Status**: Draft

**Input**: User description: "Better integration with radarr and sonarr would be nice too (right
now just watching a folder). True integration would be nice." The user chose option A from the
research: putio-sync acts as a Transmission download client that Radarr and Sonarr talk to
directly.

## Context

- Today Radarr and Sonarr use putio-sync through blackhole folders. They drop a .torrent or
  .magnet file into a watched folder. putio-sync uploads it to Put.io, and the regular sync later
  downloads the result. The *arr apps find the files by scanning the download folder.
- With a blackhole, the *arr apps see nothing after the grab: no progress, no completion signal
  and no failures. Nothing is cleaned up on Put.io after import.
- The download folder on the NAS holds many fake releases: 1.4 GB `.exe` and `.scr` files named
  like TV episodes. With a blackhole there is no way to tell the *arr apps that a release was
  bad, so they never blocklist it or look for another.
- Radarr and Sonarr ship a built-in Transmission client. It talks to Transmission's RPC protocol
  over HTTP and needs a small set of calls: read the session settings, add a torrent, list
  torrents, remove a torrent, and set per-torrent options. Other Put.io bridges (for example
  `putioarr`) show that emulating this protocol works with unmodified *arr apps.
- On the NAS, putio-sync, Radarr and Sonarr all see the download folder at the same path
  (`/downloads`), so the path putio-sync reports is valid in each container.
- The regular sync mirrors the Put.io folder tree under the local download folder. A transfer
  saved into a Put.io folder named `sonarr` therefore lands in `<download folder>/sonarr/<name>`.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Radarr and Sonarr send releases straight to putio-sync (Priority: P1)

The operator adds putio-sync to Radarr and Sonarr as a "Transmission" download client. They use
putio-sync's host and port, URL base `/transmission/`, the putio-sync download-client username and
password, and a category (`radarr`, `sonarr`). The *arr's "Test" button succeeds. When the *arr
grabs a release, it sends the torrent or magnet to putio-sync. putio-sync adds it to Put.io in the
folder for that category, and the item appears in the *arr's queue.

**Why this priority**: Nothing else works until the *arr apps can hand releases to putio-sync
and find them again.

**Independent Test**: Against a mocked Put.io, act as an *arr client. Run the session handshake,
add a magnet and a .torrent file with a category, then list torrents. Check that each item comes
back under the info hash the *arr computed itself, with the expected download folder. Check that
Put.io received one transfer per item, saved in a folder named after the category.

**Acceptance Scenarios**:

1. **Given** the download client is enabled with a username and password, **When** an *arr
   tests the connection with those credentials, **Then** the test passes.
2. **Given** wrong or missing credentials, **When** any request arrives, **Then** it is
   refused and nothing is added to Put.io.
3. **Given** a magnet link with category `sonarr`, **When** the *arr adds it, **Then** a Put.io
   transfer is created in the `sonarr` folder (created if missing), and listing torrents returns
   an item whose hash equals the magnet's info hash.
4. **Given** a .torrent file, **When** the *arr adds it, **Then** the item's hash is the info hash
   of that file, the same value the *arr computes itself.
5. **Given** a hash that putio-sync already tracks, **When** the *arr adds it again, **Then** the
   reply says it is a duplicate, and no second Put.io transfer is created.
6. **Given** the download client is disabled, **When** a request arrives, **Then** it is refused
   and the blackhole watch keeps working as before.

---

### User Story 2 - The *arr apps see progress and completion, then import (Priority: P1)

While Put.io downloads the torrent and putio-sync then downloads the files to the NAS, the *arr
queue shows the item as downloading with a rising percentage. Once every file is on local disk,
the item shows as completed with an output path the *arr can read. The *arr imports it on its
own, with no manual scan.

**Why this priority**: This is the point of real integration. The *arr knows exactly when an
item is ready and where it is.

**Independent Test**: Against a mocked Put.io, take a tracked item through these steps and list
torrents after each one: Put.io downloading, then Put.io completed, then local files queued,
then downloading, then all downloaded. Check that status, remaining bytes and the "finished" flag
move as expected. Check that the item reports completed only after every downloadable file is
complete on disk, and that its output path is the folder or file the sync wrote.

**Acceptance Scenarios**:

1. **Given** a tracked item whose Put.io transfer is queued or downloading, **When** the *arr
   lists torrents, **Then** the item is reported as downloading, its progress reflects Put.io's
   progress, and it is not finished.
2. **Given** the Put.io transfer has completed but the local download has not, **When** the *arr
   lists torrents, **Then** the item is still downloading, its progress reflects the local
   download, and it is not finished.
3. **Given** every downloadable file of the item is complete on local disk, **When** the *arr
   lists torrents, **Then** the item is finished with nothing left to download, and its output
   path is `<reported folder>/<item name>`, which exists on disk.
4. **Given** putio-sync restarts at any point, **When** the *arr lists torrents, **Then** every
   tracked item is still reported, with the same hash and a correct state.
5. **Given** a finished item, **When** the *arr checks whether the item can be removed after
   import, **Then** putio-sync's answer allows removal (Put.io does the seeding, so local
   seeding goals do not apply).

---

### User Story 3 - Removing an item in the *arr cleans up Put.io (Priority: P2)

After the *arr imports an item, it removes it from the download client, as it does with any
torrent client when "Remove Completed" is on. putio-sync then deletes the item's files from
Put.io and from the local download folder. The operator can also remove an unfinished item from
the *arr queue, and putio-sync cancels it on Put.io.

**Why this priority**: Without this, Put.io storage and the download folder fill with copies of
media the library already has.

**Independent Test**: Against a mocked Put.io and a temporary download folder, remove a finished
item with "delete data". Check that its Put.io files are deleted, its local files are gone and it
no longer appears in the list. Remove an unfinished item and check that its Put.io transfer is
cancelled and its pending local downloads are dropped.

**Acceptance Scenarios**:

1. **Given** a finished item, **When** the *arr removes it with "delete data", **Then** its
   files are deleted from Put.io and from local disk, and it is no longer listed.
2. **Given** a finished item, **When** the *arr removes it without "delete data", **Then** its
   Put.io files are deleted, its local files are kept, and it is no longer listed.
3. **Given** an unfinished item, **When** the *arr removes it, **Then** its Put.io transfer is
   cancelled, any Put.io files and pending local downloads for it are removed, and it is no
   longer listed. Its local files are deleted only when "delete data" was asked for.
4. **Given** Put.io refuses a delete, **When** the *arr removes an item, **Then** the item
   is still dropped from the list, and the failure is logged with the item's name and
   transfer id so it can be cleaned up by hand.
5. **Given** a remove request for a hash putio-sync does not track, **When** it arrives,
   **Then** it succeeds and changes nothing.

---

### User Story 4 - Bad releases are reported back so the *arr picks another (Priority: P2)

If a Put.io transfer fails, or it completes but holds nothing downloadable (for example, every
file is a blocked `.exe`), putio-sync reports the item to the *arr with an error message. If the
operator has also given putio-sync that *arr's address and API key, putio-sync asks the *arr to
remove the item and blocklist the release. The *arr then searches for another release on its own
(when its "Redownload failed" setting is on).

**Why this priority**: This fixes the problem behind the fake `.exe` releases. On its own, the
Transmission protocol lets a client report only a warning, not a failure, so automatic blocklisting
needs the *arr's own API.

**Independent Test**: Against a mocked Put.io and a mocked *arr API, make a tracked item's
transfer fail and make another complete with only blocked files. Check that both are listed with
an error message. With *arr API access configured, check that putio-sync looked up each item in
the *arr's queue by its hash and asked for removal with blocklisting exactly once.

**Acceptance Scenarios**:

1. **Given** a tracked item whose Put.io transfer is in the error state, **When** the *arr
   lists torrents, **Then** the item carries an error message that includes Put.io's reason.
2. **Given** a tracked item whose completed transfer holds only blocked or no files, **When**
   the *arr lists torrents, **Then** the item carries an error message saying it holds nothing
   downloadable and naming the blocked extensions found.
3. **Given** *arr API access is configured for the item's category, **When** an item becomes
   failed as above, **Then** putio-sync asks that *arr once to remove the matching queue entry
   with blocklisting, and logs the outcome.
4. **Given** no *arr API access is configured, **When** an item fails, **Then** only the
   error message is reported. The operator can remove and blocklist it from the *arr's queue.
5. **Given** the *arr API is unreachable or rejects the key, **When** putio-sync tries to
   blocklist, **Then** the error message stays on the item, the failure is logged without the
   key, and it is retried at the next check, up to a limit.

---

### User Story 5 - Settings, connection details and a connection test in the web UI (Priority: P3)

On the web UI's Settings page, a "Radarr / Sonarr" section shows:

- whether the download client is enabled;
- the values to enter in the *arr apps: host, port, URL base, username, and whether a password
  is set;
- the download folder putio-sync reports;
- when each *arr last contacted putio-sync;
- for each configured *arr API connection, a "Test" button that reports success with the *arr's
  name and version, or why it failed.

**Why this priority**: Setup is otherwise done blind. This section shows the exact values to
type in, and whether each side can reach the other.

**Independent Test**: Against mocked *arr APIs, load the settings section and run the connection
test against a reachable *arr, an unreachable one and one with a wrong key. Check the reported
results, and check that no password or API key appears in any API response or page.

**Acceptance Scenarios**:

1. **Given** the download client is configured, **When** the operator opens Settings, **Then**
   the section shows the connection values to enter in the *arr apps, and shows only
   "set"/"not set" for the password.
2. **Given** a configured *arr API connection, **When** the operator clicks Test, **Then** the
   result shows the *arr's application name and version, or a clear reason: unreachable,
   wrong key, or not a Radarr/Sonarr server.
3. **Given** an *arr has called putio-sync, **When** the operator opens Settings, **Then** it
   shows when that *arr last made contact.
4. **Given** a settings save from the web UI, **When** the config file is rewritten, **Then**
   any download-client password and *arr API keys in the file are kept unchanged.

### Edge Cases

- A magnet's info hash is in base32 rather than hex: it is converted, so the reported hash
  matches what the *arr expects.
- The *arr sends a download folder outside the folder putio-sync reports: the add is refused
  with a clear error, and nothing is added to Put.io.
- A category's Put.io folder is deleted by hand while items are in flight: it is created again
  on the next add. Items whose transfer is gone are reported with an error.
- A transfer is removed on Put.io directly, outside putio-sync: the item is reported with an
  error ("transfer no longer on Put.io") until the *arr removes it.
- The regular sync's blocked-extension guard skips some files of an item: the item completes
  with the remaining files. Only an item with no downloadable files at all counts as failed.
- `delete_after_download` is on, so the sync already deleted the Put.io files: removing the item
  skips the Put.io delete without error.
- Put.io is unreachable when the *arr lists torrents: items keep their last known state, and the
  request still answers.
- The same release reaches putio-sync both through the blackhole and through the download
  client: Put.io refuses or deduplicates the second transfer. The blackhole copy is not tracked
  as an *arr item.
- The *arr sends per-torrent seeding options or "move to top of queue": these are accepted and
  ignored, because Put.io handles seeding and ordering.
- Several *arr apps share one category: they see each other's items, as they would with a real
  Transmission. Separate categories per app are recommended.

## Requirements *(mandatory)*

### Functional Requirements

**Download client protocol**

- **FR-001**: putio-sync MUST answer the subset of the Transmission RPC protocol that Radarr's
  and Sonarr's Transmission clients use: session settings, adding a torrent from a magnet/URL or
  from file contents, listing torrents with the fields they request, removing torrents (with or
  without data), and setting per-torrent options. It MUST answer at the URL base the *arr apps
  use by default (`/transmission/`) on the existing web UI/API port.
- **FR-002**: The download client MUST follow the protocol's session handshake, so an
  unmodified *arr client can connect.
- **FR-003**: The download client MUST require a username and password configured for it. It
  MUST be off unless enabled and a password is set. The web UI's bearer token MUST NOT be needed
  for, or accepted on, these requests.
- **FR-004**: putio-sync MUST report a client version that makes the *arr apps put each category
  in its own subfolder of the download folder.
- **FR-005**: putio-sync MUST report its local download folder (the sync's target directory) as
  the client's download folder.

**Adding items**

- **FR-006**: When an item is added, putio-sync MUST work out its info hash: from the magnet
  link (hex or base32), or from the .torrent contents (the same hash the *arr computes). It MUST
  record the item persistently with that hash, its name, its category and its Put.io transfer.
- **FR-007**: The folder the *arr asks for MUST be inside the reported download folder. The part
  below that folder (for example `sonarr`) becomes the Put.io folder the transfer is saved to,
  created if missing, under the Put.io folder the watch uploads into. A folder outside the
  download folder MUST be refused.
- **FR-008**: Adding a hash that is already tracked MUST return the existing item as a
  duplicate, without creating another Put.io transfer.

**Reporting state**

- **FR-009**: Listing torrents MUST return every tracked item. Each item MUST include its hash,
  its name, the folder it is reported in, total size, remaining bytes, estimated time left,
  status, a finished flag, and an error message when it has one.
- **FR-010**: An item MUST be reported as finished, with nothing remaining, only when its Put.io
  transfer has completed and every downloadable file under it is complete on local disk.
- **FR-011**: Until then, an item MUST be reported as downloading (or queued before Put.io starts
  it), with progress based on Put.io's progress and then on the local download.
- **FR-012**: Once known, the item name MUST equal the name of the file or folder the sync
  writes on disk, so that `<reported folder>/<item name>` is the real output path.
- **FR-013**: A finished item MUST be reported in a way that lets the *arr's "Remove Completed"
  behaviour remove it after import.
- **FR-014**: An item MUST carry an error message when its Put.io transfer is in error, when the
  transfer no longer exists on Put.io, or when it completed but has no downloadable files. In the
  blocked-files case, the message MUST name the blocked extensions.

**Removing items**

- **FR-015**: Removing an item MUST stop tracking it. It MUST cancel the Put.io transfer if
  unfinished and delete the item's Put.io files. It MUST also drop pending local downloads for
  those files.
- **FR-016**: When removal asks for data to be deleted, putio-sync MUST also delete the item's
  local files and folder, and nothing outside its output path.
- **FR-017**: Removal without data deletion MUST keep local files. In that case it MUST delete
  the Put.io files only when the local copy is complete; otherwise it MUST leave them for the
  regular sync.
- **FR-018**: A failure to delete on Put.io MUST NOT keep the item tracked. It MUST be logged
  with the item's name and transfer id.

**Reporting failures to the *arr apps**

- **FR-019**: The operator MAY configure one or more *arr API connections, each with a kind
  (Radarr or Sonarr), an address, an API key and the category it uses.
- **FR-020**: When an item fails (FR-014) and an *arr API connection exists for its category,
  putio-sync MUST find the item in that *arr's queue by its hash. It MUST ask the *arr to remove
  the item from the client and blocklist the release. It MUST do this at most once per item
  after a success, and retry a limited number of times after an error.
- **FR-021**: The blackhole watch MUST keep working unchanged, whether or not the download
  client is enabled.

**Secrets, settings and observability**

- **FR-022**: The download-client password and *arr API keys MUST be settable through the config
  file or environment variables. They MUST NOT appear in logs, metrics, error messages or API
  responses. A settings save from the web UI MUST keep them.
- **FR-023**: The web UI Settings page MUST show the download client's state, the connection
  values for the *arr apps (password shown only as set/not set), the reported download folder,
  and when each *arr last made contact.
- **FR-024**: The web UI MUST offer a connection test for each configured *arr API connection.
  It reports the *arr's name and version on success, or the reason for failure.
- **FR-025**: The daemon MUST expose metrics for download-client requests by method and outcome,
  tracked items by state, and blocklist requests to the *arr apps by outcome. Metric labels MUST
  NOT include item names or hashes.
- **FR-026**: Adding, completing, failing and removing an item MUST each be logged with the
  item's name, hash and Put.io transfer id. Magnet links and .torrent contents MUST NOT be logged.
- **FR-027**: All new settings MUST be documented in the example configuration and the README,
  with setup steps for Radarr and Sonarr.

### Key Entities

- **Download-client item**: a release the *arr apps sent to putio-sync. It holds the info hash
  (identity), name, category, Put.io transfer id, Put.io file id once known, when it was added,
  its last known state and error, and whether it was already reported to the *arr for
  blocklisting. It is stored persistently and removed when the *arr removes the item.
- **Download-client settings**: enabled flag, username, password (secret), and optional *arr API
  connections.
- ***arr API connection**: kind (Radarr/Sonarr), address, API key (secret), category, and a last
  contact time (kept in memory).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: An unmodified Radarr and Sonarr can be set up with the built-in Transmission client
  using only the values shown on putio-sync's Settings page, and their "Test" button passes.
- **SC-002**: A grabbed release appears in the *arr queue within one *arr refresh cycle, and is
  imported with no manual scan within one refresh cycle after its last file lands on disk.
- **SC-003**: In the automated tests, no item is ever reported finished while any of its
  downloadable files is still missing or partial on local disk.
- **SC-004**: After the *arr imports and removes an item, no copy of it is left on Put.io or in
  the download folder (with "delete data").
- **SC-005**: A release whose files are all blocked is blocklisted in the *arr within one status
  check after its Put.io transfer completes, when *arr API access is configured.
- **SC-006**: No password or API key appears in any log line, metric, error message or API
  response produced by the automated tests.
- **SC-007**: Existing blackhole users see no change in behaviour.

## Assumptions

- Radarr and Sonarr v3 or later (current Radarr 5/6, Sonarr 4) with their built-in Transmission
  client. Other *arr apps that share that client (Lidarr, Readarr, Whisparr) will probably work
  but are not tested.
- The download client listens on the existing web UI/API port, so it needs no new port or
  container mapping. Like the rest of the API it is plain HTTP, meant for the home LAN.
- When the *arr apps see the download folder at a different path than putio-sync does, the
  operator uses the *arr's own "Remote Path Mappings". putio-sync always reports its own path.
  On the user's NAS the paths are identical (`/downloads`).
- Put.io does the seeding. Seed ratio and seed time options from the *arr apps are accepted and
  ignored.
- Category subfolders on Put.io live in the folder the watch uploads into (the Put.io root when
  none is set), so the regular sync downloads them to `<download folder>/<category>/`. If feature
  009 (per-folder download rules) sends a folder elsewhere, the reported path MUST follow the
  rule. The plan for this feature coordinates with 009.
- Setting the download-client password and *arr API keys from the web UI is out of scope. They
  come from the config file or environment, like the Put.io and API tokens.
- Showing the *arr apps' own queue, history or calendar in putio-sync's UI is out of scope.
- Usenet/SABnzbd emulation, a qBittorrent API, and receiving *arr webhooks are out of scope.
