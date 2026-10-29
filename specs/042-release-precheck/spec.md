# Feature Specification: Release Pre-check

**Feature Branch**: `042-release-precheck`

**Created**: 2026-10-27

**Status**: Draft

**Input**: User description: "i noticed through stats the fake torrents wasted 60gb of traffic over
week. if there a way we can filter before going to putio? can we detect it contains just exes
locally and with minimal bandwidth and deps?" and "they are mostly make exes/7z instead of
videos".

## Context

- Feature 011 made putio-sync a Transmission download client for Radarr and Sonarr. An add
  arrives as a magnet link, a URL, or the contents of a .torrent file. The watch folder (blackhole)
  takes `.torrent` and `.magnet` files.
- Fake releases are common. The NAS download folder holds many 1.4 GB `.exe`, `.scr` and `.7z`
  files named like TV episodes. The Stats page showed about 60 GB of traffic spent on them in one
  week.
- Today every release goes to Put.io first. Put.io downloads all of it, and only then does
  putio-sync look at the files. `blocked_extensions` stops executables from reaching the NAS, but
  it is a block list that does not include `.7z`, `.zip` or `.rar`, so archive fakes are
  downloaded to the NAS in full. When everything is blocked, 011 fails the item and asks the *arr
  to blocklist it, but only after Put.io has done the work.
- The file list of a release is often available before anything is downloaded:
  - a .torrent file lists every file and its size inside the file itself;
  - a URL from an indexer almost always points to a small .torrent file (a few KB). putio-sync
    already fetches it locally (capped at 10 MB) and sends the contents to Put.io. Some indexers
    answer with a redirect to a magnet link instead;
  - a magnet link carries only the info hash. The file list has to be looked up somewhere else.
- 009 folder rules give each Put.io folder a file-type filter, with presets such as "Video only".
  An *arr category is a Put.io folder, so it can already carry a rule.

## Clarifications

- Q: Is the check per category or per folder? → A: It is set per *arr category and per watch
  folder, on by default. The file types it looks for come from the 009 rule on that category's
  Put.io folder when that rule is an allow list (for example "Audio only" for a Lidarr category).
  Otherwise it looks for video.
- Q: Fetch magnet metadata from BitTorrent peers (DHT)? → A: No. It needs new dependencies and
  puts the NAS's IP in the swarm of every release, which Put.io exists to avoid.
- Q: What happens to a magnet whose file list can't be found in time? → A: It goes to Put.io
  unchecked and is checked there before anything downloads to the NAS. Telemetry shows how often
  this happens and how many of those turn out to be fake, to decide later whether more lookups
  are worth it.
- Q: How do we learn about fakes the check misses? → A: The operator can flag any release as fake
  from the web UI, and flag a rejection as wrong. Each flag records the release's traits, so the
  rule can be tuned from real data.
- Q: How is a rejected *arr item reported? → A: The add is accepted and the item is tracked as
  failed straight away, without a Put.io transfer. 011's failure and blocklist path then reports it
  and asks the *arr to blocklist it, so the *arr searches again. A refused add would not be
  blocklisted by the *arr.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - A fake .torrent never reaches Put.io (Priority: P1)

Sonarr grabs a release and sends its .torrent (as file contents, or as a URL putio-sync fetches).
The torrent holds `Show.S01E03.1080p.exe` (1.4 GB) and a `.txt`. putio-sync reads the file list
locally, sees no video, and does not send it to Put.io. The item shows in Sonarr's queue as failed
with a clear reason. With *arr API access set up, Sonarr blocklists the release and searches for
another one.

**Why this priority**: Most *arr grabs from indexers are .torrent files or URLs to them, so this
covers most of the waste with no extra network use.

**Independent Test**: Against a mocked Put.io and a mocked *arr API, add a .torrent with only an
`.exe`, one with only a `.7z`, and one with a 2 GB `.mkv`. Check that only the third creates a
Put.io transfer, that the other two are listed as failed with a reason, and that each was
blocklisted once.

**Acceptance Scenarios**:

1. **Given** the check is on for category `sonarr`, **When** Sonarr adds a .torrent whose files
   are only executables, **Then** no Put.io transfer is created, the item is listed under its
   info hash as failed, and the error says "rejected before Put.io: no video file (found .exe)".
2. **Given** the same, **When** the .torrent holds only `.7z`, `.zip` or `.rar` files (including
   split parts such as `.r00` or `.7z.001`), **Then** it is rejected the same way, naming the
   archive types found.
3. **Given** the same, **When** the .torrent holds one video smaller than the minimum size and no
   other video, **Then** it is rejected with "only a small video (12 MB, minimum 50 MB)".
4. **Given** the same, **When** the .torrent holds a video of at least the minimum size, plus
   `.nfo`, `.exe` or sample files, **Then** it passes and goes to Put.io as today. The existing
   filters still keep the `.exe` off the NAS.
5. **Given** *arr API access is set up for the category, **When** an item is rejected, **Then**
   putio-sync asks that *arr once to remove and blocklist it, as in 011.
6. **Given** an indexer URL, **When** Sonarr adds it, **Then** putio-sync fetches the .torrent
   itself and checks it as above. Put.io never receives the URL.
7. **Given** an indexer URL that redirects to a magnet link, **When** Sonarr adds it, **Then**
   putio-sync follows the redirect, takes the magnet, and checks it as a magnet (User Story 2).

---

### User Story 2 - Magnet links are checked using a few KB (Priority: P1)

Radarr sends a magnet link. putio-sync looks up the release's .torrent by info hash from public
.torrent caches over plain HTTP, within a short time limit. If found, it checks the file list as in
User Story 1. If not found in time, it falls back as set in FR-012.

**Why this priority**: Some indexers only give magnets. Without a lookup, every magnet fake costs
a full download on Put.io.

**Independent Test**: With mocked caches, add a magnet whose .torrent a cache returns with only an
`.exe`, a magnet whose .torrent has a real video, and a magnet no cache knows. Check the first is
rejected, the second goes to Put.io, and the third follows the fallback. Check that a cache
returning a .torrent with a different info hash is ignored.

**Acceptance Scenarios**:

1. **Given** a magnet whose .torrent a cache returns, **When** it is added, **Then** the file
   list is checked locally and the result is the same as for that .torrent.
2. **Given** a cache returns data whose info hash does not match the magnet, **When** it is
   checked, **Then** the data is ignored and the next cache is tried.
3. **Given** no cache answers within the time limit, **When** the magnet is added, **Then** the
   fallback in FR-012 applies, and the add still answers the *arr within its request timeout.
4. **Given** the cache lookup is turned off, **When** a magnet is added, **Then** no request is
   made to any cache and the fallback applies.

---

### User Story 3 - Releases that reach Put.io unchecked are caught before the NAS downloads them (Priority: P2)

When the file list could not be known in advance (a magnet no cache knew), the release goes to
Put.io. putio-sync checks the same rule as soon as Put.io shows the transfer's files. A release
that fails is cancelled or deleted on Put.io, reported failed and blocklisted, and nothing is
downloaded to the NAS.

**Why this priority**: This is the safety net. It stops the NAS-side waste (the archive fakes
that `blocked_extensions` lets through today) even when the Put.io-side waste can't be avoided.

**Independent Test**: Against a mocked Put.io, add an unchecked magnet whose finished transfer
holds only a `.7z`. Check that no file is queued locally, the Put.io files are deleted, and the
item is failed and blocklisted.

**Acceptance Scenarios**:

1. **Given** an unchecked item whose Put.io files become visible, **When** they fail the rule,
   **Then** putio-sync cancels the transfer if it is still running, deletes its Put.io files,
   and fails the item with a reason ending "(checked on Put.io)".
2. **Given** an unchecked item whose files pass, **When** the transfer completes, **Then** it
   downloads as today.
3. **Given** a .torrent-checked item that passed, **When** it completes on Put.io, **Then** it is
   not checked again.

---

### User Story 4 - Watch-folder files are checked too (Priority: P2)

A `.torrent` or `.magnet` file dropped into a watch folder is checked in the same way before
upload. A rejected file is not uploaded. It is renamed with a `.rejected` suffix so it is not
picked up again and the operator can see it. There is no *arr to tell, so the rejection is logged
and counted.

**Why this priority**: Blackhole users get the same fakes, but have no automatic blocklist.

**Independent Test**: Put a fake `.torrent` and a real one in a temporary watch folder, run one
scan against a mocked Put.io, and check that only the real one was uploaded and the fake was
renamed.

**Acceptance Scenarios**:

1. **Given** the check is on for a watch folder, **When** a fake `.torrent` appears, **Then**
   it is not uploaded, it becomes `<name>.torrent.rejected`, and a warning names the file and the
   reason.
2. **Given** a `.magnet` file, **When** it is scanned, **Then** the cache lookup and fallback
   from User Story 2 apply.
3. **Given** the check is off for a watch folder, **When** files appear, **Then** behaviour is as
   today.

---

### User Story 5 - See what the check saved (Priority: P3)

The Stats page shows how many releases were rejected in the selected range and how many bytes
were avoided, split by where the check happened (before Put.io, or on Put.io) and by reason. The
History or Queue view shows each rejected item with its reason.

**Why this priority**: The user found the problem through Stats. The same page should show that
it is fixed.

**Independent Test**: Seed rejections in the mock UI server and check the tile totals and the
split by stage and reason.

**Acceptance Scenarios**:

1. **Given** rejections in the last 7 days, **When** the operator opens Stats with that range,
   **Then** a tile shows the count and total size of rejected releases.
2. **Given** a rejection made on Put.io, **When** it is counted, **Then** it counts as "on
   Put.io", and its bytes count as avoided on the NAS only.
3. **Given** magnets were added in the range, **When** the operator opens Stats, **Then** it
   shows how many magnets were checked from a cache, how many fell back to Put.io, and how many
   of the fallbacks turned out to be fake and the bytes they cost on Put.io. Cache results
   (found, not found, timed out, wrong hash) show per cache.

---

### User Story 6 - Flag fakes the check missed, and wrong rejections (Priority: P3)

The operator sees a release that got through but is fake (for example a large `.mkv` that is
really a password-protected archive). From the Put.io page, the Queue or History, they flag it
"Fake". putio-sync records the release's traits for analysis. For an *arr item, the operator can
also fail it and blocklist it in the *arr in the same step. The operator can also flag a rejected
item "Not fake" when the check was wrong.

**Why this priority**: The check only knows what fakes looked like so far. Flags give real data
on new kinds of fakes and on false rejections, so the rule and its defaults can be tuned.

**Independent Test**: In the mock UI server, flag a passed item "Fake" and a rejected item "Not
fake". Check that both records hold the traits listed in FR-025, that the *arr was asked to
blocklist the first only when chosen, and that the export lists both.

**Acceptance Scenarios**:

1. **Given** a release on the Put.io page, in the Queue or in History, **When** the operator
   flags it "Fake" with an optional note, **Then** a fake report is saved with the release's
   traits, and the item shows as flagged.
2. **Given** a flagged *arr item, **When** the operator also picks "fail and blocklist", **Then**
   it is failed and blocklisted through 011's path, and its Put.io files are deleted.
3. **Given** a rejected item, **When** the operator flags it "Not fake", **Then** a report is
   saved as a false rejection. It does not resend the release; the operator searches again in the
   *arr.
4. **Given** saved reports, **When** the operator opens Stats, **Then** it shows counts of fakes
   caught by the check, fakes flagged by hand, and false rejections, with the most common file
   types and tracker hosts among fakes. The reports can be downloaded as CSV or JSON.

### Edge Cases

- A video with a double extension (`Episode.mkv.exe`) counts as an executable, not a video,
  using the last extension, as 009 does.
- A real release made of RAR parts (older scene releases): rejected by default. A category can
  allow archives as a release type (FR-004) if the operator unpacks them.
- A season pack with many videos: it passes when at least one video meets the minimum size.
- A .torrent with a single file: the file list is that one file.
- A .torrent that can't be parsed: rejected as "not a valid .torrent", as 011 does today.
- A URL that needs indexer cookies or returns an HTML error page: treated as an invalid .torrent,
  as today. The URL is never logged.
- A redirect chain that ends in a magnet after several HTTP hops: followed up to a small limit.
- A cache that is slow, down, or returns HTML: skipped; the time limit covers all caches.
- The same info hash is added again after a rejection: it is reported as the existing failed item
  (011's duplicate rule), so it does not hit the caches again.
- The operator removes a rejected item in the *arr: it is dropped as in 011. Nothing on Put.io
  needs cleaning.

## Requirements *(mandatory)*

### Functional Requirements

**The rule**

- **FR-001**: A release MUST pass the check only when its file list holds at least one wanted
  file of at least the minimum size. A release whose files are only executables, archives, small
  wanted files or other types MUST be rejected.
- **FR-002**: Wanted files MUST be videos by default, using the 009 "Video only" preset list. When
  the category's Put.io folder (or the watch folder's upload folder) has a 009 rule whose filter is
  an allow list, its allowed types MUST be the wanted types instead.
- **FR-003**: The minimum size MUST default to 50 MB and MUST be settable per category and per
  watch folder.
- **FR-004**: The check MUST be on by default for every *arr category and watch folder, and MUST
  be possible to turn off for each one. A category MAY also allow archives as wanted files.
- **FR-005**: Executables and archives MUST be detected by their last extension. Archive types
  MUST include `.7z`, `.zip`, `.rar` and their split parts (`.001`, `.r00`…).
- **FR-006**: The default `blocked_extensions` list MUST NOT change, so plain sync is unaffected.

**Where the file list comes from**

- **FR-007**: For .torrent contents, the file list and sizes MUST be read locally, with no network
  use.
- **FR-008**: For URLs, putio-sync MUST fetch the .torrent locally (as 011 does) and check it.
  Put.io MUST NOT receive the URL. A redirect to a magnet link MUST be followed, and the magnet
  checked as in FR-009.
- **FR-009**: For magnets, putio-sync MUST look up the .torrent by info hash from a configurable
  list of public .torrent caches, over HTTP, within a total time limit (default 10 seconds). It
  MUST use the result only when its info hash matches the magnet. The lookup MUST be possible to
  turn off.
- **FR-010**: Each cache lookup MUST send only the info hash. It MUST NOT send the magnet's
  trackers, the release name, or any indexer URL or key.
- **FR-011**: putio-sync MUST NOT fetch metadata from BitTorrent peers or the DHT. The NAS never
  joins a release's swarm.
- **FR-012**: When a magnet's file list cannot be found in time, putio-sync MUST send it to Put.io
  unchecked and check it there (User Story 3, FR-015). Real releases are never lost this way, and
  fakes still never reach the NAS.

**Rejecting**

- **FR-013**: A rejected *arr item MUST be accepted by the add call, tracked under its info hash
  as failed with the reason, and never sent to Put.io. It MUST then go through 011's failure and
  blocklist path (FR-014, FR-020 of 011).
- **FR-014**: The reason MUST say the check ran before Put.io or on Put.io, and name what was
  found (the types, or the largest wanted file's size and the minimum).
- **FR-015**: For an item that reached Put.io unchecked, putio-sync MUST check its files as soon
  as Put.io shows them, and before queueing any local download. A failing item MUST have its
  transfer cancelled (if still running) and its Put.io files deleted.
- **FR-016**: A rejected watch-folder file MUST NOT be uploaded. It MUST be renamed with a
  `.rejected` suffix and MUST NOT be picked up again.

**Observability and settings**

- **FR-017**: The daemon MUST expose metrics for checks by source (torrent, URL, magnet, watch
  folder), outcome (passed, rejected, unknown) and stage (before Put.io, on Put.io), and for bytes
  avoided by stage. Bytes avoided are the release's total size from its file list. Labels MUST NOT
  include names or hashes.
- **FR-018**: Each rejection MUST be logged with the item name, info hash, source and reason.
  URLs, magnet links and .torrent contents MUST NOT be logged.
- **FR-019**: The Stats page MUST show, for the selected range, the count and bytes of rejected
  releases, split by stage and reason.
- **FR-020**: The Settings page MUST show, per category and watch folder, whether the check is on,
  the wanted types in force and where they come from (video default or a 009 rule), and the
  minimum size. The cache lookup setting and cache list MUST be shown and settable.
- **FR-021**: All new settings MUST be in the example configuration and the README.

**Fallback telemetry**

- **FR-022**: The daemon MUST count magnet lookups per cache by result (found, not found, timed
  out, wrong hash, error) and record lookup time. It MUST count magnets sent to Put.io unchecked,
  and what the check on Put.io later found for them (passed, rejected), with the bytes Put.io
  downloaded for the rejected ones.
- **FR-023**: The Stats page MUST show the share of magnets that fell back to Put.io and how many
  of those were fake, over the selected range (User Story 5, scenario 3).

**Flagging**

- **FR-024**: The web UI MUST let the operator flag a release "Fake" from the Put.io page, the
  Queue and History, and flag a rejected item "Not fake". A flag MAY carry a short note. A flag
  MUST be possible to undo.
- **FR-025**: Every rejection and every flag MUST save a fake report with: kind (rejected by the
  check, flagged fake, flagged not fake), when, source (torrent, URL, magnet, watch folder),
  category or watch folder, stage, release name, info hash, total size, number of files, the file
  types with their counts and largest sizes, the largest wanted file's size, tracker host names,
  and the private flag. It MUST NOT store magnet links, indexer URLs, API keys or .torrent
  contents.
- **FR-026**: For an *arr item flagged "Fake", the operator MUST be able to fail and blocklist it
  in the same step, through 011's path. A plain-sync release flagged "Fake" is only recorded.
- **FR-027**: Reports MUST be kept for at least 1 year and MUST be downloadable as CSV and JSON.
  Stats MUST summarise them as in User Story 6, scenario 4.

### Key Entities

- **Release check result**: source, stage, outcome, reason, and the release's total size. Stored
  in its own record, linked to the 011 item or the Put.io transfer, and counted for Stats. The
  minimum size used is kept on the fake report of a rejection.
- **Fake report**: one per rejection or flag, with the traits in FR-025, an optional note, and a
  link to the 011 item or watch file when there is one.
- **Release check settings**: per category and per watch folder: on/off, minimum size, allow
  archives. Global: cache lookup on/off, cache list, time limit.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: In tests, no .torrent or URL release made only of executables, archives or small
  videos creates a Put.io transfer.
- **SC-002**: Checking a .torrent or URL uses no network beyond fetching the .torrent itself.
  Checking a magnet uses under 100 KB unless a cache returns a large .torrent.
- **SC-003**: An add answers the *arr within 15 seconds in every case, including when every
  cache is down.
- **SC-004**: No release whose files fail the rule is ever downloaded to the NAS, whatever its
  source: an *arr .torrent, URL or magnet, or a watch-folder file. Releases added to Put.io by
  hand go through plain sync and are not checked.
- **SC-005**: Over a week of normal grabs on the NAS, Put.io traffic spent on fake releases falls
  from about 60 GB to only what magnet fallbacks cost, and NAS traffic on them falls to zero.
- **SC-006**: No real release in the tests (a single video, a season pack, a video with extras) is
  rejected.
- **SC-007**: After a month of use, the Stats page answers "how often do magnets fall back to
  Put.io, and how many of those are fake" without reading logs.
- **SC-008**: Flagging a release takes one click plus an optional note, and the report export
  holds no magnet link, URL or key.

## Assumptions

- The size of a video is the size listed in the .torrent. A fake with a large file named `.mkv`
  that is not really a video is out of scope (it would need the file's contents).
- Public .torrent caches are unreliable and some go offline. The list is configurable, and a
  miss falls back as FR-012 says. Sending an info hash to a cache tells that cache which release
  is wanted. This is a small privacy cost, and it can be turned off.
- Whether Put.io shows a transfer's file list before the transfer completes is not known yet. The
  plan will confirm it. If Put.io only shows files at completion, FR-015 still saves all NAS
  traffic, but Put.io's own traffic for that release is spent.
- No new heavy dependencies: the .torrent reading reuses 011's info-hash code, and the cache
  lookup uses the HTTP client already in use.
- Download verification (034) and the processing pipeline (038) are separate. The check runs
  before the pipeline starts. 038 MAY later show it as a "Pre-check" step.
- Tracker host names and the private flag in fake reports come from the release's file or from
  006's source records, keeping only host names as 006 does. A release with no known source is
  still reported, with those fields empty.
- Sending fake reports to anyone else (a shared fake list) is out of scope. Reports stay on the
  NAS.
- Choosing between several Radarr/Sonarr releases, or scoring them, stays the *arr's job.
