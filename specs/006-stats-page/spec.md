# Feature Specification: Stats Page

**Feature Branch**: `006-stats-page`

**Created**: 2026-10-26

**Status**: Draft

**Input**: User description: "A stats page with diagrams, filters by file types (maybe torrent
DHT servers), etc. Nice to see how bandwidth is over time, when downloads happen the most,
what types of downloads are happening."

## Context

- Feature 004 gave the daemon a token-protected web UI (Status, Queue, Put.io, Transfers,
  History, Settings). History is a paged list of rows; nothing in the UI summarises it.
- The daemon already keeps a history of every finished download: when it finished, the file
  name and extension, its size, how long it took and its average speed. That is enough for
  charts of volume over time, time-of-day patterns and file types, with no new data.
- It does not record where a file came from. Put.io's transfer list says, for each transfer,
  whether it was a torrent or a URL, which tracker it announced to, and whether the torrent is
  private. Put.io does not say anything about DHT peers or nodes; the nearest signal is the
  private flag, since private torrents don't use DHT.
- Feature 003 exports Prometheus metrics that a Grafana dashboard charts. Those are totals with
  no per-file detail, so they can't be filtered by file type, and the UI can't rely on a
  Prometheus server being reachable.

## Clarifications

### Session 2026-10-26

- Q: Where does the page get its data? → A: The daemon's own download history only; no
  Prometheus queries. Bandwidth over time is reconstructed from each download's start and
  finish.
- Q: Should downloads record where they came from? → A: Yes: the transfer type (torrent, URL,
  other), the tracker's host name only (never the full tracker address, which can carry a
  private passkey) and the private flag. Only downloads after this ships carry it; older ones
  show as "unknown".
- Q: (follow-up from the user) Filter by source "folder and what folder, manual upload,
  etc."? → A: Yes. Each download also records its **origin** — which local watch folder its
  torrent came from, added by hand through the web UI, or added on Put.io directly — and the
  top-level **Put.io folder** it was synced from. Both are breakdowns and filters.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - See bandwidth over time (Priority: P1) 🎯 MVP

As the NAS operator, I want a chart of how many bytes putio-sync pulled down over time, so
that I can see how busy it has been and spot quiet or heavy days.

**Why this priority**: It's the first thing asked for, and the page, its sign-in, its date
range and the shared summary numbers all come with it.

**Independent Test**: Seed the history with downloads spread over 30 days, open the Stats
page, and check the chart and totals against the seeded bytes and durations.

**Acceptance Scenarios**:

1. **Given** the operator is signed in, **When** they open Stats, **Then** they see, for the
   last 7 days by default: total files, total bytes, total download time, average speed, and
   a chart of bytes (and average throughput) per time bucket.
2. **Given** the Stats page is open, **When** the operator picks another range (24 hours,
   7 days, 30 days, 90 days, 1 year, all time), **Then** the totals and chart update, and the
   buckets are sized so the chart has between about 24 and 120 bars (hours for 24 h, 6-hour
   blocks for 7 d, days for 30 and 90 d, weeks for a year, longer blocks beyond).
3. **Given** a download that ran for two hours across two hourly buckets, **When** the chart
   uses hourly buckets, **Then** its bytes are split between the two buckets in proportion to
   the time spent in each.
4. **Given** no downloads in the range, **When** the page loads, **Then** it says there is
   nothing to show for that range instead of drawing an empty or broken chart.
5. **Given** the operator hovers over or taps a bar, **When** it is selected, **Then** the
   page shows that bucket's start time, bytes, file count and average throughput.

---

### User Story 2 - See when downloads happen (Priority: P2)

As the operator, I want a heatmap of downloads by day of week and hour of day, so that I
can see when putio-sync is busiest.

**Why this priority**: Explicitly asked for, and it uses the same history as Story 1.

**Independent Test**: Seed downloads at known local times and check the heatmap cells.

**Acceptance Scenarios**:

1. **Given** the Stats page is open, **When** the heatmap renders, **Then** it shows a 7 × 24
   grid (Monday first, hours 0–23) where each cell's shade reflects the bytes finished in that
   slot over the selected range, and the operator can switch the measure to file count.
2. **Given** the operator's browser is in a time zone other than UTC, **When** the heatmap
   renders, **Then** slots are in the browser's local time.
3. **Given** a cell is hovered or tapped, **When** it is selected, **Then** the page shows
   the day, hour, file count and bytes for that cell.

---

### User Story 3 - See what kinds of files are downloaded (Priority: P2)

As the operator, I want to see file counts and bytes by file type, and filter the whole page
by type, so that I know what I'm downloading.

**Why this priority**: Explicitly asked for; the filter also sharpens Stories 1 and 2.

**Independent Test**: Seed downloads with several extensions and check the breakdown and the
filtered totals.

**Acceptance Scenarios**:

1. **Given** the Stats page is open, **When** the type breakdown renders, **Then** it lists
   file types grouped into categories (video, audio, archive, image, document, subtitle,
   other), with each category's file count, bytes and share, and the top extensions inside
   each.
2. **Given** the operator picks a category or a single extension in the filter, **When** the
   filter applies, **Then** every chart and total on the page covers only those files.
3. **Given** files with no extension, **When** they're counted, **Then** they appear as
   "(none)" under "other".

---

### User Story 4 - See where downloads came from (Priority: P3)

As the operator, I want to see downloads split by how they reached Put.io (a watch folder,
and which one; added by hand in the UI; added on Put.io itself), by the Put.io folder they
were synced from, by kind (torrent or URL), by tracker, and by public or private torrent,
and filter by any of these, so that I know which sources I rely on.

**Why this priority**: The user flagged it as a "maybe". It needs the daemon to start
recording a new fact, and only downloads from then on have it.

**Independent Test**: With a mocked Put.io transfer list holding a torrent from a private
tracker, a public torrent and a URL transfer, run a sync, finish the downloads, and check
that the Stats page splits them correctly and that no full tracker address is stored.

**Acceptance Scenarios**:

1. **Given** a sync queues a file that belongs to a Put.io transfer, **When** the file is
   queued, **Then** the daemon records the transfer's type, its tracker's host name and its
   private flag for that file.
2. **Given** the tracker address carries a passkey in its path or query, **When** the source
   is recorded, **Then** only the host name is stored and nothing else from the address
   appears in storage, logs or API responses.
3. **Given** the Stats page is open, **When** the source breakdown renders, **Then** it shows
   files and bytes by type (torrent, URL, other, unknown), by tracker host (top 10, the rest
   as "other trackers"), and public vs private.
4. **Given** the operator picks a source type, tracker or private/public in the filter,
   **When** it applies, **Then** every chart and total covers only those files.
5. **Given** a file whose transfer is no longer on Put.io, or that was downloaded before this
   feature, **When** it's counted, **Then** it shows as "unknown" rather than being dropped.
6. **Given** the transfer list can't be fetched during a sync, **When** the sync runs,
   **Then** files are queued and downloaded as before, with source "unknown", and a warning
   is logged.
7. **Given** the watcher uploads a torrent from the watch folder `/watch/tv`, **When** its
   files are later synced, **Then** they show under origin "Watch folder: tv" and can be
   filtered to that folder.
8. **Given** a transfer added from the web UI, and another added on the Put.io website,
   **When** their files are synced, **Then** they show as "Added in putio-sync" and
   "Added on Put.io" respectively.
9. **Given** files synced from the Put.io folders `Movies` and `TV`, **When** the folder
   breakdown renders, **Then** each top-level folder is a row (files in the Put.io root show
   as "(root)") and picking one filters the page.

---

### Edge Cases

- A download with a recorded duration of 0 or none: all its bytes go into the bucket it
  finished in.
- A download that started before the selected range and finished inside it: only the bytes
  that fall inside the range count toward the bandwidth chart; totals and breakdowns count
  the file by its finish time.
- A range with thousands of downloads: responses stay small because the server sends bucket
  totals, never history rows.
- Deleted-file history rows (post-download deletes) are not downloads and are never counted.
- A filter that matches nothing: every chart says there's nothing to show; the filter stays
  set so the operator can clear it.
- A narrow phone screen: charts fit the width without sideways scrolling; the heatmap
  remains readable (cells may shrink, labels thin out).
- Daylight-saving changes inside the range: the heatmap uses the browser's current UTC
  offset for the whole range; an hour's shift around the change is acceptable.
- An expired or missing API token: the page goes to sign-in like every other page.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The UI MUST have a Stats page, reachable from the navigation on desktop and
  phone, behind the same sign-in as the other pages.
- **FR-002**: The page MUST show summary totals for the selected range and filters: files,
  bytes, download time and average speed (bytes ÷ download time).
- **FR-003**: The page MUST chart bytes downloaded per time bucket over the selected range,
  spreading each download's bytes evenly over the time between its start (finish minus
  duration) and its finish, clipped to the range.
- **FR-004**: The page MUST show a day-of-week × hour-of-day heatmap of downloads, by bytes or
  by file count, in the browser's local time.
- **FR-005**: The page MUST show file counts and bytes per file category and per extension.
- **FR-006**: The page MUST show file counts and bytes per source type, per tracker host and
  for public vs private torrents.
- **FR-007**: The page MUST offer filters for date range (presets: 24 h, 7 d, 30 d, 90 d,
  1 y, all), file category or extension, source type, tracker host, and private/public. Every
  chart and total MUST honour all active filters.
- **FR-008**: The selected range and filters MUST survive a page reload and be shareable as
  a link.
- **FR-009**: The server MUST answer stats requests with pre-aggregated data (totals,
  buckets, cells, groups), never with individual history rows, and MUST require the API
  token.
- **FR-010**: When the daemon queues a file from Put.io, it MUST record the file's source:
  the transfer type, the tracker's host name and the private flag, matched from Put.io's
  transfer list. It MUST fetch that list at most once per sync.
- **FR-011**: The daemon MUST NOT store, log or return a full tracker address, magnet link
  or source URL; only the host name.
- **FR-016**: When the watcher or the web UI creates a Put.io transfer, the daemon MUST
  remember the transfer's origin (the watch folder it came from, or "added in putio-sync"),
  so the files it later syncs carry that origin. Transfers it didn't create are "added on
  Put.io"; files with no matching transfer are "unknown".
- **FR-017**: When the daemon queues a file, it MUST record the top-level Put.io folder it
  was found in, and the page MUST offer a breakdown and filter by origin (including which
  watch folder) and by Put.io folder.
- **FR-012**: If the source can't be determined, the file MUST still be queued and
  downloaded, with source "unknown".
- **FR-013**: Charts MUST have text alternatives (an accessible label and the hovered or
  tapped value as text) and MUST NOT rely on colour alone to tell series apart.
- **FR-014**: The UI MUST stay within its 150 KB total size budget and add no third-party
  chart library.
- **FR-015**: The mock UI server MUST seed enough history (several weeks, several types,
  several sources) for every chart on the page to show data.

### Key Entities

- **Download record** (existing): one finished download — finish time, file name, extension,
  size, duration, average speed. Gains a link to its source.
- **Download source** (new): for a Put.io file, where it came from — type (torrent, URL,
  other), tracker host name (optional), private flag, origin (watch folder and which one,
  added in putio-sync, added on Put.io, unknown) and top-level Put.io folder. Recorded when
  the file is queued.
- **Transfer origin** (new): for a transfer putio-sync created, whether the watcher (and
  from which watch folder) or the web UI created it.
- **File category**: a fixed mapping from extension to category (video, audio, archive,
  image, document, subtitle, other).
- **Stats summary**: what the page receives for one range and filter set — totals, time
  buckets, heatmap cells, type groups and source groups.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: On the NAS, the Stats page shows all of its charts within 2 seconds of opening
  it, for a history of 20,000 downloads.
- **SC-002**: The operator can answer "how much did it download last week, and on which day
  was it busiest?" from the page in under 30 seconds, without the CLI.
- **SC-003**: Every response the page fetches is under 50 KB regardless of how many downloads
  are in the range.
- **SC-004**: The page works on a 375-pixel-wide phone screen with no sideways scrolling.
- **SC-005**: After a sync with a private-tracker torrent, no part of the tracker address
  other than its host name exists in the database, logs or any API response.
- **SC-006**: The whole UI stays under 150 KB.

## Assumptions

- The operator's browser time zone is the one they want to see times in; the page sends its
  current UTC offset with each request.
- Category mapping uses common extensions (mkv, mp4, avi, mov, m4v, ts, webm → video; mp3,
  flac, m4a, aac, ogg, opus, wav → audio; zip, rar, 7z, tar, gz → archive; jpg, jpeg, png,
  gif, webp → image; pdf, epub, txt, nfo, doc, docx → document; srt, sub, ass, idx, vtt →
  subtitle). It isn't configurable.
- A file belongs to a transfer when it is the transfer's file, or sits inside the transfer's
  folder on Put.io. Files added to Put.io some other way (uploads, copies) are "unknown".
- Old history rows get no source; there is no backfill.
- Transfers added with the CLI `create` command open no database, so their files show as
  "added on Put.io". Files uploaded straight to Put.io (not transfers) and files whose
  transfer Put.io already cleared before the sync show as "unknown"; the two can't be told
  apart.
- A watch folder is shown by its last path component; the full configured path is in the
  tooltip. Watch folder paths are local configuration, not secrets.
- History is never pruned by this feature; the page aggregates whatever is stored.
- Prometheus and the Grafana dashboard stay as they are; this page doesn't replace them.
