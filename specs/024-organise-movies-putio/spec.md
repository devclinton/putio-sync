# Feature Specification: Organise Movies on Put.io

**Feature Branch**: `claude/022-stream-mode-dlna-4b51d8` (stacked on 022, 028 and 023; no branch of its own)

**Created**: 2026-11-10

**Status**: Draft

**Input**: User description: "024 Organise movies on Put.io. The same as 023, for movies:
`Movie (Year)/Movie (Year).ext`, with extras and samples kept apart or removed. A second kind
in the 023 organiser, with the same plan, apply, undo, auto and plugin paths and the same
Organise panel."

## Context

- 023 moves TV episodes that stream mode keeps on Put.io into `TV Shows/Show/Season NN/`.
  Its organiser (plan, apply, undo, dismiss, find, auto mode, plugin answers) does not care
  what kind of media a move is for.
- Movies arrive the same way: release folders such as
  `Movie.Name.2019.1080p.BluRay.x264-GRP/` holding the film, sometimes a `Sample/` clip,
  sometimes `Featurettes/` or a trailer.
- Plex and Jellyfin both read `Movie (Year)/Movie (Year).ext`, and both read extras from
  subfolders named `Trailers`, `Featurettes`, `Behind The Scenes`, `Deleted Scenes` and
  `Interviews` next to the film.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Preview and apply a tidy movie layout (Priority: P1)

The operator turns on "Organise movies" on the Organise panel. As films arrive on Put.io,
putio-sync plans moves such as `Movies/Movie Name (2019)/Movie Name (2019).mkv` and lists
them beside TV moves. Nothing moves until the operator applies them (or chose "move at once").

**Why this priority**: This is the feature.

**Independent Test**: With a mock Put.io holding a film in a release folder and movies on, a
sync pass records one planned move of kind `movie` and makes no rename or move call;
applying it creates the folders, renames and moves the file.

**Acceptance Scenarios**:

1. **Given** movies are off (the default), **When** a film is streamed, **Then** no movie move
   is planned.
2. **Given** movies are on, **When** `Movie.Name.2019.1080p.BluRay.x264-GRP.mkv` is streamed,
   **Then** a move to `Movies/Movie Name (2019)/Movie Name (2019).mkv` is planned.
3. **Given** a file named only `abc123.mkv` inside `Movie.Name.2019.1080p.BluRay-GRP/`,
   **When** it is streamed, **Then** the film's name and year come from the release folder.
4. **Given** a name without a year (`Home Video.mp4`) or a non-video file (`movie.nfo`),
   **When** it is streamed, **Then** no movie move is planned.
5. **Given** a TV episode name (`Show.S01E02.mkv`), **When** it is streamed with movies on and TV
   off, **Then** no movie move is planned for it.
6. **Given** a film split in parts (`Movie.2019.CD1.avi`), **Then** the target keeps the part:
   `Movie (2019) - part1.avi`.
7. **Given** a subtitle `Movie.2019.1080p.en.srt`, **Then** it goes next to the film as
   `Movie (2019).en.srt`.

### User Story 2 - Extras kept apart, samples left out (Priority: P1)

Release folders may carry trailers, featurettes and sample clips. Extras go into a subfolder
of the film that media servers read as extras, never beside the film where they would show as
a second copy. Samples are never moved into the library.

**Independent Test**: A trailer and a sample in a film's release folder: the trailer is planned
into `Movies/Movie (2019)/Trailers/`, the sample gets no move.

**Acceptance Scenarios**:

1. **Given** `Movie.2019.1080p-GRP/Featurettes/Making Of.mkv`, **Then** it is planned to
   `Movies/Movie (2019)/Featurettes/Making Of.mkv`.
2. **Given** `Movie.2019.Trailer.1080p.mkv`, **Then** it is planned into `Movies/Movie (2019)/Trailers/`.
3. **Given** `Movie.2019.1080p-GRP/Sample/movie-sample.mkv` or `Movie.2019.SAMPLE.mkv`, **Then**
   no move is planned; the clip stays out of the library.
4. **Given** a film whose title contains an extra word before its year (`The.Interview.2014.1080p.mkv`),
   **Then** it is organised as the film, not as an extra.

### User Story 3 - Same controls as TV (Priority: P2)

Movie moves use the same list, undo, dismiss, "find" scan, "move at once" and plugin
answers as TV moves.

**Acceptance Scenarios**:

1. **Given** an applied movie move, **When** it is undone, **Then** the file returns to its old
   folder and name.
2. **Given** `apply = "auto"`, **Then** movie moves are made in the sync pass that plans them.
3. **Given** plugins answer a folder or name, **Then** their answer wins, as for TV.
4. **Given** films streamed before movies were turned on, **When** the operator presses the find
   button, **Then** moves are planned for them.
5. **Given** the movies folder is outside stream mode, **When** a move is applied, **Then** it is
   refused, as for TV.

### Edge Cases

- A title that is itself a year-like number (`Blade.Runner.2049.2017.mkv`, `1917.2019.mkv`) keeps
  that number in the title.
- A file already in place (`Movies/Movie (2019)/Movie (2019).mkv`) gets no move.
- With both kinds on, an episode name is always read as TV, never as a film.
- Characters Put.io or media servers cannot take (`:`, `?`) are replaced as for TV.
- Folder names such as `Best of 2019` with no quality word after the year are not read as a
  film's release folder.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Settings MUST add "organise movies" (off by default) and a movies folder
  (default `Movies`), checked like the TV folder (relative, no `..`).
- **FR-002**: A video or subtitle name with a title and a year, and no season/episode number,
  MUST plan `<movies folder>/Title (Year)/Title (Year)[ - partN][.lang].ext`.
- **FR-003**: A file without a year MUST take the title and year from the nearest release
  folder above it that has a year followed by a quality or source word.
- **FR-004**: Extras (trailer, featurette, behind the scenes, making of, deleted scene,
  interview), found by a word after the year or by the folder they sit in, MUST be planned into
  the film's matching extras subfolder, keeping their own file name.
- **FR-005**: Samples MUST NOT be planned.
- **FR-006**: Movie moves MUST share TV's plan, apply, undo, dismiss, scan, auto, plugin and
  stream-mode checks, and be listed and counted with kind `movie`.
- **FR-007**: The Organise panel MUST offer the movies toggle and folder and say which kind a
  move is.
- **FR-008**: The doctor MUST warn when movies are organised but nothing is streamed, as for TV.

### Key Entities

- **Organise kind**: now `tv` or `movie`; stored on each move and used as the metric label.
- **Movie name**: title, year, optional part, read from a release name.

## Success Criteria *(mandatory)*

- **SC-001**: Real movie release names from a Radarr library are given the title and year
  Radarr chose for at least 85% of them; none is read as a TV episode.
- **SC-002**: Real Sonarr episode names never produce a movie move.
- **SC-003**: Every movie move can be undone with one action, as for TV.
- **SC-004**: The panel loads on demand; the eager UI stays under its 488 KiB budget.

## Assumptions

- Removing samples from Put.io is out of scope: they are left where they are, outside the
  library folder, which keeps them "apart" with nothing deleted. 022's junk patterns can
  already delete them when wanted.
- Editions (`Director's Cut`) and IMDb/TMDb ids in names are not added; a metadata lookup is
  out of scope, as in 023.
- Names that carry a year but are not films (home videos) can be misread; preview mode, the
  default, lets the operator dismiss them.
