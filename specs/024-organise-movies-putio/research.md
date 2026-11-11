# Research: Organise Movies on Put.io

## Layout
- **Decision**: `Movies/Title (Year)/Title (Year)[ - partN][.lang].ext`.
- **Rationale**: Plex, Jellyfin, Emby and Radarr's default all read it.
- **Alternatives**: `{imdb-tt…}` or `{edition-…}` tags need a lookup or are Plex-only; left out.

## Extras
- **Decision**: extras go into a subfolder of the film: `Trailers`, `Featurettes`,
  `Behind The Scenes`, `Deleted Scenes`, `Interviews`; the file keeps its own name.
- **Rationale**: these names are read as extras by both Plex and Jellyfin. A word decides the
  folder: `trailer`/`teaser` → Trailers; `featurette`/`extra(s)`/`bonus` → Featurettes;
  `behind the scenes`/`making of`/`bts` → Behind The Scenes; `deleted` → Deleted Scenes;
  `interview` → Interviews. The words count only after the title's year, or in the folder the
  file sits in, so `The.Interview.2014` stays a film.

## Samples
- **Decision**: no move for a name with the word `sample` after the year, or inside a
  `Sample`/`Samples` folder.
- **Rationale**: the roadmap allows "kept apart or removed". Deleting on Put.io cannot be undone
  and would need a new action in the moves table; leaving the clip outside the library folder
  keeps it apart with nothing lost. 022's junk patterns already delete samples when wanted.

## Reading the name
- **Decision**: a `Movie` reader over 023's word splitter: the last year before the first
  quality/source/codec/tag word ends the title (`Blade Runner 2049 2017`); a name with a season
  or episode word is never a film; video and subtitle extensions only. With no year in the file
  name, the nearest folder whose year is followed by a quality word names the film.
- **Rationale**: same rules as 050's `ReleaseInfo`, but with letter case kept, like `Episode`.

## Real-data check
- **Decision**: a test with real Radarr names from the NAS library (folder, file, Radarr's
  title and year) and a check that 023's Sonarr names give no movie move.
