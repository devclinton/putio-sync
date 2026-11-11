# Feature Specification: Organise TV Shows on Put.io

**Feature Branch**: `claude/022-stream-mode-dlna-4b51d8` (stacked on 022 and 028; no branch of its own)

**Created**: 2026-11-10

**Status**: Draft

**Input**: User description: "023 Organise TV shows on Put.io. For stream mode (022): move
finished downloads that stream mode keeps on Put.io into a `Show/Season NN/` layout on Put.io,
with episodes named `Show - SNNEMM - Title`. Names are matched from the release name (reuse
the 050 release-name parser). Show a preview before anything moves, and support an undo.
028's rename and folder plugin hooks may override the built-in naming. Make the organiser
generic so 024 (movies) slots in as a second kind."

## Context

- In stream mode (022) finished transfers stay on Put.io. They keep the names and folders the
  release gave them (`Show.Name.S02E05.1080p.WEB-DL.x264-GRP/…mkv`), so the "Put.io" root the
  media server shows is a heap of release folders, not a library.
- Each newly streamed file already gets a list of one-off steps (022's "arrived steps"; today
  only the MP4 request). Organising is one more such step.
- putio-sync already reads release names for subtitle matching (050): title, year, season and
  episode. Lua plugins (028) can already answer "what name" and "what folder" for a download.
- Organising movies (024) comes next and must fit beside TV shows without a redesign.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Preview and apply a tidy TV layout (Priority: P1)

The operator turns on "Organise TV shows" in the stream settings. As new episodes arrive on
Put.io, putio-sync works out where each belongs — `TV Shows/Show Name/Season 02/Show Name -
S02E05 - Episode Title.mkv` — and lists the planned moves. Nothing moves yet. The operator
reviews the list and applies all or some of it; the files are renamed and moved on Put.io.

**Why this priority**: The preview-then-move flow is the feature; it delivers the tidy library
with no risk of surprise moves.

**Independent Test**: With a mock Put.io holding an episode in a release folder and organising
on, a sync pass records one planned move and makes no rename or move call; applying it makes
the folder, rename and move calls and marks the move applied.

**Acceptance Scenarios**:

1. **Given** organising is off (the default), **When** an episode is streamed, **Then** no move
   is planned.
2. **Given** organising is on in "preview" mode, **When** `Show.Name.S02E05.Title.Here.1080p.WEB-DL.x264-GRP.mkv`
   is streamed, **Then** a planned move is listed from its current folder and name to
   `TV Shows/Show Name/Season 02/Show Name - S02E05 - Title Here.mkv`, and the file is not
   touched on Put.io.
3. **Given** planned moves, **When** the operator applies some of them, **Then** only those files
   are renamed and moved, missing folders are created, and each applied move is shown as done.
4. **Given** a file whose name is not an episode (no season and episode number), **When** it is
   streamed, **Then** no move is planned for it.
5. **Given** planned moves, **When** the operator dismisses one, **Then** it leaves the list and
   the file stays where it is.

---

### User Story 2 - Undo a move (Priority: P1)

The operator applied a move and does not like the result (wrong show match, wanted the old
name). They undo it; the file goes back to its original folder with its original name.

**Why this priority**: The brief asks for undo; without it a wrong match is a manual cleanup on
Put.io.

**Independent Test**: After applying a move against a mock Put.io, undoing it renames the file
back and moves it to the folder it came from, and the move shows as undone.

**Acceptance Scenarios**:

1. **Given** an applied move, **When** the operator undoes it, **Then** the file is renamed to its
   old name and moved back to its old folder, and the move is marked undone.
2. **Given** an applied move whose old folder no longer exists, **When** the operator undoes it,
   **Then** the undo fails with a clear reason and the file stays where it is.
3. **Given** an undone move, **When** the operator looks at the list, **Then** it can be applied
   again.

---

### User Story 3 - Move without asking (Priority: P2)

An operator who trusts the naming switches from "preview" to "move at once". New episodes are
organised as they arrive; each move is still listed and can be undone.

**Why this priority**: Convenience for steady use once the preview has earned trust.

**Independent Test**: With "move at once", a sync pass over a new episode makes the rename and
move calls in the same pass and lists the move as applied.

**Acceptance Scenarios**:

1. **Given** "move at once", **When** an episode is streamed, **Then** it is moved in that pass and
   listed as applied, with undo available.
2. **Given** "move at once" and a Put.io error during the move, **When** the pass ends, **Then** the
   move is retried on the next pass, and after it keeps failing it is listed as failed with the
   reason.

---

### User Story 4 - Organise what is already there (Priority: P2)

Episodes streamed before organising was turned on are planned too, on request: the operator
asks putio-sync to look for episodes, and the ones it knows about that have no move yet are
added to the preview.

**Why this priority**: Without it only new arrivals get organised.

**Independent Test**: With two streamed files known before organising was on, "Find episodes"
plans a move for the episode and none for the non-episode.

**Acceptance Scenarios**:

1. **Given** streamed files with no planned move, **When** the operator asks to find episodes,
   **Then** each episode among them gets a planned move and the count found is reported.
2. **Given** a file that already has a move (planned, applied or undone), **When** the operator
   asks again, **Then** it is not planned twice.

---

### User Story 5 - A plugin decides the name or folder (Priority: P3)

The operator runs a Lua plugin with a `rename` or `folder` sync rule (028). For streamed files,
the plugin's answer replaces the built-in name or folder in the plan, and the plan says it
came from a plugin.

**Why this priority**: Lets unusual libraries (anime numbering, per-language folders) keep their
own layout without a new built-in option.

**Independent Test**: With a fake plugin that answers a folder for every file, the planned move
uses that folder and is marked as set by a plugin.

**Acceptance Scenarios**:

1. **Given** a plugin answers a folder, **When** a move is planned, **Then** the plan uses the
   plugin's folder (relative to the synced Put.io folder) and the built-in name.
2. **Given** a plugin answers a name, **When** a move is planned, **Then** the plan uses that name.
3. **Given** "use plugins" is off, **When** a move is planned, **Then** plugins are not asked.

### Edge Cases

- **Name already taken**: the target folder already holds a file with the target name → the move
  fails with "already exists" and nothing changes.
- **File gone**: the file was deleted or moved on Put.io before the move → the move fails with
  "no longer on Put.io".
- **Already in place**: the file is already at the target folder and name → no move is planned.
- **Season pack**: the file is `S01E03.mkv` inside a folder named `Show.Name.S01.1080p` → the
  show name (and season, when the file lacks it) comes from the folder.
- **Multi-episode file**: `S01E01E02` → `Show - S01E01-E02 - Title`.
- **Specials**: season 0 → `Season 00`, named like any other season (the Plex and Jellyfin
  convention for specials is a `Season 00` folder or `Specials`; `Season 00` keeps the pattern).
- **Show with a year**: `Doctor.Who.2005.S03E04` → `Doctor Who (2005)`.
- **No episode title**: `Show - S01E02.mkv`.
- **Subtitle beside the episode**: `Show.S01E02.en.srt` → `Show - S01E02.en.srt`, so players
  still pair it with the video.
- **Target folder not in stream mode**: when folder rules would make the target folder a
  download folder, a move there would make the next sync download the file. The plan is refused
  with that reason.
- **Characters Put.io or players dislike** (`/`, `:` …) are made safe in names, as for other
  names putio-sync writes.
- **Many files**: steps run in bounded batches per pass (022's limit of 20), so a big backlog
  spreads over passes; applying a large selection runs one file at a time.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Organising MUST be off by default, and MUST apply only to files stream mode keeps
  on Put.io. Downloads, *arr files and local files are never organised.
- **FR-002**: When on, each newly streamed file whose name (or, failing that, its folder's name)
  gives a show title, a season and an episode MUST get a planned move to
  `<TV folder>/<Show>/Season NN/<Show> - SNNEEE[-EEEE] - <Episode title>.<ext>`, where the
  episode title part is left out when the name has none, and the year is added as `Show (YYYY)`
  when the release name has one.
- **FR-003**: The TV folder MUST be a setting, a Put.io path relative to the synced folder,
  `TV Shows` by default. Missing folders on the way MUST be created when a move is applied.
- **FR-004**: The apply mode MUST be a setting: "preview" (default; moves wait for the operator)
  or "move at once" (moves are applied in the pass that plans them).
- **FR-005**: The operator MUST be able to list planned, applied, undone and failed moves with
  the current and target folder and name of each, and apply, undo or dismiss selected ones.
- **FR-006**: Applying a move MUST record the file's original folder and name before changing
  anything, so the move can be undone.
- **FR-007**: Undo MUST rename the file back and move it to its original folder, and MUST fail
  without changing anything when that folder is gone.
- **FR-008**: A move MUST NOT overwrite or duplicate an existing name in the target folder; it
  fails with the reason instead.
- **FR-009**: The operator MUST be able to plan moves for streamed files already known that have
  no move yet ("Find episodes").
- **FR-010**: When "use plugins" is on (default), the plugins' folder and rename answers (028)
  MUST replace the built-in folder and name; the move MUST say a plugin set it. A plugin answer
  MUST be planned even for a file the built-in reader can't place.
- **FR-011**: A move whose target folder is not in stream mode under the folder rules MUST be
  refused with that reason.
- **FR-012**: The organiser MUST name the kind of each move (TV episode now; movie in 024), so a
  second kind adds a reader and a layout without changing the plan, apply or undo flow.
- **FR-013**: Moves applied, undone and failed MUST be logged with the file id and both paths,
  and counted in a metric by kind and result.
- **FR-014**: The settings MUST be saved in the config file, documented in the example config,
  editable in Settings, and apply without a restart.
- **FR-015**: Every new UI string MUST go through the translation catalogs.

### Key Entities

- **Organise move**: one file's plan: the Put.io file, its kind, its current folder and name,
  its target folder and name, who set the target (built-in or plugin), its state (planned,
  applied, undone, failed), the original parent folder recorded on apply, and the last error.
- **Organise settings**: on or off for TV, the TV folder, the apply mode, and whether plugins
  are asked.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: In preview mode, no file on Put.io is renamed or moved until the operator applies
  its move (0 move calls in tests before apply).
- **SC-002**: Every applied move can be undone and puts the file back exactly (same folder,
  same name) when the old folder still exists.
- **SC-003**: Common release-name forms (`S01E02`, `1x02`, multi-episode, with year, season-pack
  folders, bracketed group prefixes) are planned to the expected target in tests.
- **SC-004**: Turning organising on or off, or changing its folder or mode, applies from the next
  sync pass without a restart.
- **SC-005**: Adding movies (024) needs a new kind and its naming only; plan storage, the list,
  apply and undo stay as they are.

## Assumptions

- The operator's media players read the `Show/Season NN/Show - SNNEEE - Title` layout (Plex,
  Jellyfin, Kodi and Emby all do).
- Episode titles come from the release name only; there is no online lookup (TVDB/TMDb) in
  this feature. A release with no title words after the episode number gets no title part.
- Stream files kept by 022 are *arr-free by design (the *arr path always downloads), so *arr
  data does not apply to these files; the built-in release-name reader is the source.
- Folders left empty after a move are not removed here; the existing Put.io empty-folder cleanup
  (005) removes them.
- Moves keep the Put.io file id, so the stream record, MP4 versions and the media server keep
  working for moved files.
- A plugin's folder answer is read relative to the synced Put.io folder, the Put.io counterpart
  of the download folder it is relative to for downloads.
- Out of scope: movies (024), organising downloaded files on the NAS, renaming folders other
  than creating the target ones.
