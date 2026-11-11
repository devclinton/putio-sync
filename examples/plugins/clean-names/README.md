# clean-names

A **rename** rule for videos:

| Put.io name | Local name |
|---|---|
| `Show.S01E02.1080p.WEB.x264-GRP.mkv` | `Show S01E02.mkv` |
| `The.Movie.2019.1080p.BluRay.x264.mp4` | `The Movie (2019).mp4` |
| `holiday.mkv` | unchanged |

Only the extensions in **Extensions to rename** are touched. Files Radarr or Sonarr asked
for keep their names, and a name numbered after a clash is kept on later attempts.
