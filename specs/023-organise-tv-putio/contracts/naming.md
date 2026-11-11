# Naming contract (feature 023)

`organise::plan_target(config, name, folder) -> Option<Target { kind, folder, name }>`

| Put.io folder / name | Target folder (tv_folder = `TV Shows`) | Target name |
|---|---|---|
| `Show.Name.S02E05.Title.Here.1080p.WEB-DL.x264-GRP.mkv` | `TV Shows/Show Name/Season 02` | `Show Name - S02E05 - Title Here.mkv` |
| `the.office.us.s01e02.720p.hdtv.mkv` | `TV Shows/The Office Us/Season 01` | `The Office Us - S01E02.mkv` |
| `Doctor.Who.2005.S03E04.720p.HDTV.x264-GRP.mkv` | `TV Shows/Doctor Who (2005)/Season 03` | `Doctor Who (2005) - S03E04.mkv` |
| `Show.S01E01E02.Pilot.1080p.mkv` | `TV Shows/Show/Season 01` | `Show - S01E01-E02 - Pilot.mkv` |
| `Show.1x05.mkv` | `TV Shows/Show/Season 01` | `Show - S01E05.mkv` |
| `Show.Name.S01.1080p.BluRay-GRP` / `S01E03.mkv` | `TV Shows/Show Name/Season 01` | `Show Name - S01E03.mkv` |
| `[SubsPlease] Show S02E01 (1080p).mkv` | `TV Shows/Show/Season 02` | `Show - S02E01.mkv` |
| `Show.S01E02.en.srt` | `TV Shows/Show/Season 01` | `Show - S01E02.en.srt` |
| `Show.S00E01.Special.mkv` | `TV Shows/Show/Season 00` | `Show - S00E01 - Special.mkv` |
| `Movie.2019.1080p.mkv`, `README.txt` | none | none |

Rules: words that are all lowercase get a capital first letter; other words keep their case.
Names are made safe with `rules::sanitize_name` (`/ \ : * ? " < > |` → `_`). The title stops at
the first resolution, source, codec or release tag word (`proper`, `repack`, `amzn`, `ddp5`…).
A target equal to the current folder and name gives none.

Checked against 400 real Sonarr imports and 400 Radarr imports from the operator's library:
352→359 of 400 episodes give Sonarr's show and episode numbers (the rest differ only by TVDB
naming, scene numbering or anime absolute numbers, which need a metadata lookup), and no movie
is read as an episode. A season-pack folder of the same season names the show over the file
(obfuscated `pfa-taemh.s01e17.mkv`); `S01E21-22` and `S01 E22` read as one range / one episode;
language tags (`DANiSH`, `MULTi`) end a title. Ten of those names are unit tests.
