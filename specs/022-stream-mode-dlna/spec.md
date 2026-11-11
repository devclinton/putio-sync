# Feature Specification: Stream Mode with DLNA

**Feature Branch**: `022-stream-mode-dlna`

**Created**: 2026-11-10

**Status**: Draft

**Input**: User description: "022 Stream mode with DLNA (roadmap entry 022). A mode that leaves
media on Put.io instead of downloading it: putio-sync manages the files on Put.io and serves them
to the home network through the existing DLNA media server (spec 022-media-server). Stream mode
is chosen for the whole account or per folder (as a folder rule setting, inherited like the
others). Files in stream mode are never queued for download and never deleted after download;
the type filter and patterns still apply, so junk can still be deleted from Put.io.
Radarr/Sonarr items always download. When a video arrives in stream mode and isn't already MP4,
putio-sync asks Put.io to make an MP4 version (on by default, can be turned off). The media
server's Put.io root defaults to the synced Put.io folder when no folder is chosen. Settings
shows the mode, warns when stream mode is used but the media server or its Put.io root is off,
and offers to turn them on. Leave a clean seam for 023/024 (organising TV shows and movies on
Put.io) without building them."

## Context

- putio-sync mirrors one Put.io folder (the synced folder, the account root by default) to
  the NAS download folder. Each new file is queued, downloaded, checked and, by default,
  deleted from Put.io.
- The media server (spec `022-media-server`) already shows local roots to TVs and players, and
  can show one Put.io folder as a "Put.io" root. Playing a Put.io file streams it from Put.io
  through putio-sync, using Put.io's MP4 version when there is one.
- What is missing is the mode itself: today every file that passes the rules is downloaded, so
  a household that wants its library to stay on Put.io (small NAS, no disk, or a big
  collection) has to skip folders by hand and then pick the Put.io folder for the media server
  separately. And a file with no MP4 version plays only on players that read its format.
- Organising TV shows and movies on Put.io (023, 024) is the next step for this mode and is out
  of scope here, except that this feature leaves the place where it will run.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Keep media on Put.io and watch it on the TV (Priority: P1)

The operator sets the mode to "Stream from Put.io" in Settings › Downloads. New transfers
finish on Put.io and stay there: putio-sync downloads none of them. The TV, through the media
server, shows them under "Put.io", and they play from Put.io.

**Why this priority**: This is the feature. Without it, nothing else in the story matters.

**Independent Test**: With a mock Put.io holding two videos in the synced folder and stream
mode on, a sync pass queues nothing and deletes nothing, the pass reports both files as
streamed, and the media server's Put.io root lists both.

**Acceptance Scenarios**:

1. **Given** the mode is "Download to the NAS" (the default), **When** a file arrives on
   Put.io, **Then** it is downloaded as today.
2. **Given** the mode is "Stream from Put.io", **When** a sync pass finds a new media file that
   the rules accept, **Then** it is not queued, not downloaded and not deleted from Put.io, and
   it is counted as "streamed" in the pass.
3. **Given** stream mode and the media server on with the Put.io root shared and no Put.io folder
   chosen, **When** a player opens "Put.io", **Then** it sees the synced Put.io folder.
4. **Given** stream mode, **When** a file is refused by the type filter or a pattern and "delete
   blocked files" is on, **Then** it is deleted from Put.io as in download mode.
5. **Given** stream mode, **When** the storage free-up runs, **Then** it never picks a file that
   was kept on Put.io for streaming (it only deletes files already downloaded).
6. **Given** files were streamed and the mode is switched back to "Download to the NAS",
   **When** the next sync runs, **Then** those files are queued and downloaded like any new file.

---

### User Story 2 - Stream some folders, download others (Priority: P1)

The operator keeps the account in download mode and adds a folder rule on `Movies` with mode
"Stream". Files below `Movies` stay on Put.io; everything else is downloaded. A deeper rule can
set "Download" again for one subfolder.

**Why this priority**: Many households want a mix: TV episodes on the NAS for the week, films
left on Put.io. Folder rules are how every other setting is chosen per folder.

**Independent Test**: With rules `Movies` = stream and `Movies/Keep` = download, a pass over a
tree with files in the root, in `Movies` and in `Movies/Keep` queues the root and `Movies/Keep`
files and streams the `Movies` file; the rules preview shows the same, naming the deciding rule.

**Acceptance Scenarios**:

1. **Given** a rule on `Movies` with mode "Stream", **When** a file lands in `Movies/Film/`,
   **Then** it is streamed, and the rules preview names `Movies` as the rule that decided.
2. **Given** that rule and a rule on `Movies/Keep` with mode "Download", **When** a file lands
   in `Movies/Keep`, **Then** it is downloaded.
3. **Given** the account mode is "Stream", **When** a rule on a folder sets "Download", **Then**
   that folder is downloaded and the rest is streamed.
4. **Given** a folder rule that skips a folder, **When** that folder is also in stream mode,
   **Then** it is skipped (skip wins; nothing below it is streamed or downloaded).

---

### User Story 3 - Players that need MP4 get it (Priority: P2)

When a video that isn't MP4 (for example `.mkv` or `.avi`) arrives in stream mode, putio-sync
asks Put.io to make its MP4 version. Once Put.io has it, players get the MP4 version, which more
TVs can play. The operator can turn this off.

**Why this priority**: Many TVs' built-in players can't read MKV. Put.io converts on request;
without the request most files never get an MP4 version.

**Independent Test**: Against a mock Put.io, a pass in stream mode with `a.mkv`, `b.mp4` and
`c.srt` asks for an MP4 version of `a.mkv` only, once; a second pass asks again for nothing.
With the setting off, no request is made.

**Acceptance Scenarios**:

1. **Given** stream mode with "Ask Put.io for MP4 versions" on (the default), **When** an MKV
   video newly arrives, **Then** putio-sync asks Put.io for its MP4 version once.
2. **Given** a video that already is MP4, a non-video file, or a video whose MP4 version is
   ready or being made, **When** it arrives, **Then** no conversion is asked for.
3. **Given** the request fails (Put.io down, limit reached), **When** the next sync runs,
   **Then** it is tried again, at most once per sync pass, and the failure is logged as a
   warning without stopping the pass.
4. **Given** the setting is off, **When** videos arrive in stream mode, **Then** no conversion
   is asked for.
5. **Given** the MP4 version is ready, **When** a player plays the file, **Then** it gets the
   MP4 version (as the media server already does).

---

### User Story 4 - Settings tell me what's missing (Priority: P2)

On Settings › Downloads, choosing "Stream from Put.io" shows that players reach these files
through the media server. If the media server is off, or on without the Put.io root, a warning
says so with one button that turns them on (and shares the Put.io root) and saves.

**Why this priority**: Stream mode with the media server off leaves files nowhere to watch.
The operator should not need to know two settings sections are linked.

**Independent Test**: With stream mode on and the media server off, the Downloads section shows
the warning; pressing the button saves the media server as on with the Put.io root shared, and
the warning goes.

**Acceptance Scenarios**:

1. **Given** stream mode is used (account mode or any folder rule) and the media server is off,
   **When** Settings › Downloads is open, **Then** a warning says players can't see the
   streamed files, with a button "Turn on the media server".
2. **Given** the media server is on but the Put.io root isn't shared, **When** the section is
   open, **Then** the warning says so, with a button "Share Put.io".
3. **Given** the button is pressed, **When** the save succeeds, **Then** the media server is on
   with the Put.io root shared, and the warning is gone.
4. **Given** download mode with no stream rule, **When** the section is open, **Then** no
   warning is shown.
5. **Given** the folder rules editor, **When** a rule is edited, **Then** its mode can be set to
   "Inherit", "Download" or "Stream", and the effective mode and the rule it comes from are
   shown like the other settings.

---

### Edge Cases

- A stream-mode file that is already queued or downloading when the mode changes: it carries on
  (the mode decides what is queued next, not what is already in the queue).
- A file already downloaded before the folder switched to stream mode: nothing is done to the
  local copy or the Put.io copy.
- A Radarr/Sonarr item whose folder is in stream mode: it is downloaded anyway, because the
  *arr can only import local files, and putio-sync logs once that stream mode was ignored for it.
- Stream mode with "delete after download" on: nothing is deleted, because nothing was
  downloaded.
- The empty-folder cleanup: a folder holding stream-mode files is not empty, so it is left;
  emptied folders are cleaned up as before.
- The Put.io root was chosen by hand (`putio_folder` set): it is kept; the default to the synced
  folder applies only when none is chosen.
- A conversion request for a file that Put.io deleted in between: it fails quietly (warning)
  and is not retried for that file.
- Thousands of files arriving in stream mode at once (a first sync of an existing library):
  conversion requests are spread out, a bounded number per pass, so the sync is not held up.

## Requirements *(mandatory)*

### Functional Requirements

**Mode**

- **FR-001**: The operator MUST be able to choose the mode for the whole account: "Download to
  the NAS" (default) or "Stream from Put.io", in Settings and in the config file.
- **FR-002**: Every folder rule MUST be able to set the mode ("Download" or "Stream") for its
  folder and everything below it; unset, it is inherited from the parent folders and then the
  account, like the other folder settings, and the deciding rule is reported.
- **FR-003**: A file in stream mode that the rules accept MUST NOT be queued, downloaded, or
  deleted from Put.io by "delete after download".
- **FR-004**: Skipping a folder MUST win over stream mode, and the type filter and the include
  and exclude patterns MUST apply in stream mode as in download mode, including deleting
  blocked files when that is on.
- **FR-005**: Each sync pass MUST count and report streamed files apart from queued, skipped and
  blocked ones, and the rules preview MUST show which files would be streamed.
- **FR-006**: Radarr/Sonarr items MUST always be downloaded, whatever the mode of their folder.
- **FR-007**: Switching a folder from stream to download mode MUST make its files eligible for
  download on the next sync, with no other step.
- **FR-008**: The storage free-up MUST NOT delete files kept on Put.io for streaming.

**Newly arrived files**

- **FR-009**: The first time a sync pass sees a file in stream mode, putio-sync MUST record it
  (file, folder, size, when) and run the "arrived" steps for it once; later passes MUST NOT run
  them again for the same file.
- **FR-010**: The arrived steps MUST be a list that later features can add to (organising TV
  shows and movies on Put.io, features 023 and 024) without changing the sync pass. A failing
  step MUST be logged and retried on a later pass, and MUST NOT stop the pass or the other
  steps.
- **FR-011**: The first arrived step MUST ask Put.io for an MP4 version of each video that is
  not MP4 and whose MP4 version is neither ready nor in progress, when "Ask Put.io for MP4
  versions" is on (default on).
- **FR-012**: Conversion requests MUST be bounded per pass (at most 20) so a first sync of a
  large library does not stall the pass; the rest are asked for on later passes.

**Media server**

- **FR-013**: When the Put.io root is shared and no Put.io folder is chosen, the media server
  MUST show the synced Put.io folder as "Put.io".
- **FR-014**: Players MUST keep getting the MP4 version when Put.io has one ready, and the
  original otherwise (unchanged from 022-media-server).

**Settings and status**

- **FR-015**: Settings › Downloads MUST show the account mode, and the folder rules editor MUST
  offer the mode per rule.
- **FR-016**: When stream mode is used anywhere and the media server is off, or on without the
  Put.io root, Settings MUST say so and offer one action that turns it on with the Put.io root
  shared.
- **FR-017**: The config check MUST warn when stream mode is used and the media server or its
  Put.io root is off, and when a stream-mode folder holds a Radarr/Sonarr category.
- **FR-018**: Every new UI string MUST go through the UI's translation catalogs.
- **FR-019**: Metrics MUST count streamed files (as a rule decision) and conversion requests by
  result, with bounded labels.

### Key Entities

- **Mode**: "download" or "stream"; set for the account and optionally per folder rule.
- **Streamed file**: a Put.io file kept for streaming: Put.io id, name, folder path, size, when
  first seen, and how its arrived steps went (MP4 asked for or not, last error).
- **Arrived step**: something done once for each newly streamed file; MP4 conversion now,
  organising later.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: With stream mode on, 100% of accepted new files stay on Put.io and none is
  written to the NAS download folder.
- **SC-002**: A file that lands on Put.io in a stream-mode folder is visible on the TV within
  the media server's listing refresh (at most 5 minutes) with no step by the operator.
- **SC-003**: Every non-MP4 video arriving in stream mode gets one conversion request, and no
  file gets more than one successful request.
- **SC-004**: An operator who turns stream mode on with the media server off can make the files
  visible to players in one click from the same Settings section.
- **SC-005**: A sync pass over 5,000 stream-mode files that were already seen makes no
  conversion or arrived-step calls to Put.io.

## Assumptions

- The media server (spec `022-media-server`) is in place and is reused as it is; this feature
  adds no second server and no new player-facing behaviour except the default Put.io folder.
- "Where a player needs them" is met by asking Put.io for MP4 versions and serving them when
  ready. putio-sync does not detect per player which format it can play, and does not
  transcode.
- Streamed files are only shown, not managed further, in this feature. Renaming and moving them
  into a show or film layout is 023 and 024, which plug into the arrived steps.
- Watch-folder transfers (torrents and magnets dropped on the NAS) follow the mode of the folder
  they land in on Put.io, like any other file.
- Put.io's MP4 conversion is free for the account and can take minutes to hours; until it is
  ready players get the original.
