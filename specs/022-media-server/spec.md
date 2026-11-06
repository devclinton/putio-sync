# Feature Specification: Media Server (DLNA) for Local Files and Put.io

**Feature Branch**: `022-media-server`

**Created**: 2026-11-05

**Status**: Draft

**Input**: User description: "022 Media server (DLNA) for local files and Put.io. putio-sync
serves media to TVs and players on the home network as a DLNA/UPnP media server. Priority 1
(first story, ships on its own): local files — the download folder (with its category
subfolders like sonarr/ and radarr/) and the watch folders already shown on the Local files
page (044) can be browsed and played over DLNA, with no Put.io calls. Priority 2: stream mode —
files left on Put.io are listed and streamed on demand from Put.io, using Put.io's MP4
conversions when a player needs them (roadmap entry 022). The server announces itself on the
LAN (SSDP), can be turned on/off in Settings with a friendly name and which roots to share, and
must not expose anything outside the chosen folders. Organising TV/movies on Put.io (023/024)
is out of scope."

## Context

- putio-sync runs on the NAS and downloads into one folder (`/downloads`), with Radarr/Sonarr
  items in a subfolder per category. The Local files page (044) already lists that folder and
  each watch folder, and keeps every path inside its root (no `..`, no symlink that escapes).
- Today, watching something on the TV needs a separate media server (Plex, Jellyfin, minidlna)
  pointed at the same folder. Many households only want "browse the folder from the TV".
- Roadmap entry 022 also asks for a mode where media stays on Put.io and is streamed from it.
  That needs the same media server, so it comes second and reuses it.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Play downloaded files on the TV (Priority: P1)

The operator turns on "Media server" in Settings and gives it a name ("NAS putio-sync"). The TV
or a player app (VLC, Kodi, a smart TV's media app) on the home network finds that name without
any address typed in. Opening it shows the shared roots ("Downloads" and any other root that was
picked). Inside, the folders look the same as on disk (`sonarr/`, `radarr/`, show folders), and
picking a video plays it, with seeking forwards and back.

**Why this priority**: Most of the value is here, it needs no Put.io calls, and it ships alone.

**Independent Test**: With temp roots holding a small video, an audio file, a subfolder and a
partial download, a UPnP client on the same machine finds the server, browses it, sees only the
finished media, and plays a byte range of the video that matches the file on disk.

**Acceptance Scenarios**:

1. **Given** the media server is off (the default), **When** a player searches the network,
   **Then** putio-sync is not found and nothing listens for media requests.
2. **Given** it is turned on with the name "NAS putio-sync", **When** a player searches,
   **Then** it appears under that name within 10 seconds, with no restart of putio-sync.
3. **Given** Downloads is shared, **When** the player opens it, **Then** it sees the folders
   and media files with their names, folders first, then files by name.
4. **Given** a partial download (`.x.mkv.downloading`), a hidden file, or a file that is not
   media (`.nfo`, `.txt`, `.torrent`), **When** the folder is listed, **Then** none of them
   appear.
5. **Given** a video is playing, **When** the viewer seeks to the middle, **Then** playback
   continues from there without restarting from the start.
6. **Given** a download finishes while a player is browsing, **When** the player opens that
   folder again, **Then** the new file is listed.
7. **Given** the server is turned off in Settings, **When** a player tries to browse or play,
   **Then** it gets nothing, and players stop seeing the server.

---

### User Story 2 - Choose what is shared and keep the rest private (Priority: P1)

In Settings the operator picks which roots are shared: Downloads, each category subfolder on its
own, and each watch folder. The default shares Downloads only. Nothing outside a shared root can
be listed or fetched, whatever a player sends.

**Why this priority**: Turning the server on must not open the whole NAS to the network. It is
part of the first release, not a later hardening step.

**Independent Test**: Requests for paths with `..`, encoded `..`, absolute paths, and a symlink
inside a root that points outside it are all refused, and nothing from outside the root is sent.

**Acceptance Scenarios**:

1. **Given** only `radarr/` is shared, **When** a player browses, **Then** it sees `radarr/` and
   not `sonarr/` or the rest of Downloads.
2. **Given** a request for any item outside the shared roots, **When** it is made, **Then** it
   is refused and logged as a warning with the requesting address, and no bytes are sent.
3. **Given** a symlink in a shared root that points outside every root, **When** the folder is
   listed, **Then** the link is not shown, and fetching it directly is refused.
4. **Given** a shared root is removed in Settings, **When** a player opens an item from it,
   **Then** the item is refused, even if the player kept a link to it.

---

### User Story 3 - See that it works (Priority: P2)

On the Settings section the operator sees whether the server is running, the address players
use, how many players have browsed in the last hour, and how many streams are playing now. If it
cannot start (the port is taken, no network), the reason is shown there and in the log, and the
rest of putio-sync keeps running.

**Why this priority**: The server runs unattended on the NAS. When the TV "can't find it", this
is where the operator looks first.

**Independent Test**: Start the server on a taken port; the status says why and the daemon is
otherwise healthy. Free the port and save; the status shows running.

**Acceptance Scenarios**:

1. **Given** the server is running, **When** Settings is opened, **Then** it shows "Running",
   the address, and the current stream count.
2. **Given** the port is in use, **When** the server is turned on, **Then** Settings shows the
   error and putio-sync's downloads carry on.
3. **Given** streams are playing, **When** the metrics are scraped, **Then** active streams,
   bytes sent and requests (by result) are there, with no file names in the labels.

---

### User Story 4 - Stream what is still on Put.io (Priority: P2)

The operator turns on "Put.io" as a shared root, choosing one Put.io folder (for example the
folder downloads land in before they are synced, or a "Movies" folder kept on Put.io). The TV
shows a "Put.io" root next to Downloads with that folder's contents. Playing a file streams it
from Put.io. If Put.io has an MP4 version of a video, the player gets that version, because more
TVs can play it; the original is used when there is no MP4.

**Why this priority**: It is what roadmap 022 is for (media left on Put.io), but it calls
Put.io and only some households want it, so it comes after local files.

**Independent Test**: Against a mock Put.io with a folder of two videos (one with an MP4
version), the "Put.io" root lists both, playing the converted one fetches the MP4 stream,
playing the other fetches the original, and seeking works on both.

**Acceptance Scenarios**:

1. **Given** Put.io sharing is on with folder "Movies", **When** the player opens "Put.io",
   **Then** it sees the folders and media files in "Movies" on Put.io, and nothing above it.
2. **Given** a video with a finished MP4 version, **When** it is played, **Then** the MP4
   version is streamed.
3. **Given** a video with no MP4 version, **When** it is played, **Then** the original is
   streamed.
4. **Given** Put.io is unreachable or the token is invalid, **When** the player opens "Put.io",
   **Then** the local roots still work, and the Put.io root is empty, with the reason in the
   log and on the Settings status.
5. **Given** a file is deleted on Put.io (for example after it was synced), **When** the player
   opens the folder again after the listing is refreshed, **Then** the file is gone from it.
6. **Given** Put.io sharing is off, **When** players browse, **Then** putio-sync makes no Put.io
   calls for the media server.

### Edge Cases

- A file is deleted or renamed on disk while it is playing: the stream ends cleanly, and the
  next browse no longer shows it.
- A folder with thousands of files: the player gets it in pages, as it asks, without
  putio-sync loading the whole folder for each page.
- File names with spaces, accents, emoji, `&`, `<` or `#`: they are shown as on disk and play.
- Two players stream the same file at once: both play.
- The NAS has more than one network (LAN plus a Docker bridge): players on the LAN find the
  server at the LAN address, not at the bridge address.
- In Docker, network discovery needs the container on the host network. The docs say so, and the
  Settings status says when the server is running but cannot announce itself.
- A player asks for a byte range past the end of the file: refused with the right error, not a
  crash.
- A Put.io link expires while a long film is playing: the next range request gets a fresh link,
  and playback carries on.
- The friendly name is empty or very long: an empty name falls back to "putio-sync"; a long one
  is cut to what players show.
- Settings change (name, roots) while a stream is playing: the playing stream is not cut, unless
  its root stopped being shared.

## Requirements *(mandatory)*

### Functional Requirements

**Server and discovery**

- **FR-001**: putio-sync MUST be able to act as a standard home-network media server (DLNA /
  UPnP AV) that common TVs and player apps can find, browse and play from.
- **FR-002**: The media server MUST be off by default, and turning it on or off, or changing
  its settings, MUST take effect without restarting putio-sync and without stopping downloads.
- **FR-003**: When on, it MUST announce itself on the local network under the friendly name set
  in Settings, answer players that search for media servers, and say goodbye when it is turned
  off or putio-sync stops.
- **FR-004**: The operator MUST be able to set the friendly name, the port, and the network
  address it serves on (default: all LAN addresses), in Settings and in the config file.
- **FR-005**: The server MUST keep a stable identity across restarts, so players that remember
  it find it again.

**Browsing and playing local files**

- **FR-006**: The server MUST show each shared root as a top-level folder, named after the root
  ("Downloads", the category name, or the watch folder's path).
- **FR-007**: Inside a root it MUST show subfolders and media files as they are on disk, folders
  first, then files, each sorted by name.
- **FR-008**: It MUST show only media files (video, audio, image), by file type, and MUST hide
  partial downloads, hidden files and other files.
- **FR-009**: Each item MUST report what players need to decide whether they can play it: its
  media type, size, and (for video and audio) its format.
- **FR-010**: Playing a file MUST support byte ranges, so players can seek and resume.
- **FR-011**: Listings MUST reflect the disk when a player browses; new or removed files show
  up on the next browse, without a restart or a scan the operator starts.
- **FR-012**: Large folders MUST be returned in the pages a player asks for.
- **FR-013**: For local roots, the media server MUST NOT call Put.io.

**What is shared**

- **FR-014**: The operator MUST be able to choose the shared roots from: Downloads, each
  category subfolder of Downloads, each watch folder, and (Story 4) one Put.io folder. The
  default is Downloads only.
- **FR-015**: The server MUST refuse every request for an item outside the shared roots,
  including paths with `..` (plain or encoded), absolute paths, and symlinks that resolve
  outside every shared root; the same path rules as the Local files page (044) apply.
- **FR-016**: Refused requests MUST be logged as warnings with the requesting address and the
  reason, and MUST NOT send any bytes.
- **FR-017**: The media server MUST be read-only: no player request can delete, rename, move or
  upload anything.

**Status and observability**

- **FR-018**: Settings MUST show the server's state (off, running, failed with reason), the
  address players use, and the number of streams playing now.
- **FR-019**: If the server cannot start, putio-sync MUST log why and keep running everything
  else.
- **FR-020**: The daemon MUST expose metrics for active streams, bytes sent, and requests by
  kind and result, with bounded labels (no file names, paths or player addresses).
- **FR-021**: Logs MUST record each stream start and end with root, size, bytes sent and
  duration as structured fields, and MUST NOT include the API token, the Put.io token or Put.io
  download links.

**Put.io stream mode (Story 4)**

- **FR-022**: With Put.io sharing on, the server MUST show a "Put.io" root listing the chosen
  Put.io folder and its subfolders, with the same media-only rule as local roots.
- **FR-023**: Playing a Put.io file MUST stream it from Put.io on demand, with byte ranges, and
  without storing the whole file on the NAS.
- **FR-024**: For a video with a finished MP4 version on Put.io, the server MUST stream the MP4
  version; otherwise it MUST stream the original.
- **FR-025**: Put.io listings MAY be kept for a short time to limit calls, but a change on
  Put.io MUST show within 5 minutes.
- **FR-026**: If Put.io fails, the Put.io root MUST show empty with the reason in the log and
  Settings, and local roots MUST keep working.

### Key Entities

- **Media server settings**: on/off, friendly name, port, address, shared roots, Put.io folder.
  Stored with the rest of the config and editable in Settings.
- **Shared root**: a named top-level folder players see; points at a local folder or at one
  Put.io folder.
- **Media item**: a folder or a media file under a shared root, with name, media type, size and
  format; identified by its path inside the root, never by a path outside it.
- **Stream**: one player playing one item: root, item size, bytes sent, start and end.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: After the server is turned on, a TV or player on the same network finds it by
  name within 10 seconds, with no address typed in.
- **SC-002**: A 4 GB video in Downloads starts playing on the TV within 3 seconds of being
  picked, and seeking to any point resumes within 3 seconds.
- **SC-003**: At least VLC, Kodi and one smart TV's built-in media app can browse and play from
  it.
- **SC-004**: No request, of any form, returns a file or folder outside the shared roots; every
  traversal and symlink case in the tests is refused.
- **SC-005**: With the server on and three streams playing, downloads and the web UI are no
  slower than with it off, and the server adds no more than 50 MB of memory.
- **SC-006**: A file that finishes downloading is visible on the TV the next time its folder is
  opened.
- **SC-007**: With Put.io sharing on, a video on Put.io with an MP4 version starts playing
  within 5 seconds.

## Assumptions

- Players are on the same home network, and the network allows device discovery. On the NAS,
  the Docker container uses host networking for discovery; documenting this is part of the
  feature.
- The media server has no login. Like other home DLNA servers, anyone on the LAN can browse
  the shared roots. This is why it is off by default, read-only, and limited to the chosen
  roots. Serving Put.io data without a login is a choice the plan must record against the
  constitution's rule that Put.io data needs authentication (VI).
- putio-sync does not transcode. Files play as they are. For Put.io, its MP4 conversions are the
  answer to "my TV cannot play this". Asking Put.io to make a conversion is out of scope.
- Watch folders mostly hold `.torrent` and `.magnet` files, which are not media. They are
  offered as roots because the request asked for them, and they show only media files that
  happen to be there.
- There is no library view (by show, by genre, recently added, artwork, metadata). Folders are
  shown as they are on disk. Organising TV and films on Put.io (023, 024) is a separate feature.
- Subtitle files next to a video (`.srt`) are not offered to players in this feature. They are
  part of subtitles (025).
- One Put.io folder can be shared. More than one, and hiding files that are also on disk, can
  come later if needed.
- Discovery uses the standard multicast announcements of home media servers. No cloud relay and
  no access from outside the home.
