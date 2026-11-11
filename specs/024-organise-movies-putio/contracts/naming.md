# Contract: movie naming

`plan_target(config, name, folder)` with `movies = true`, `movies_folder = "Movies"`:

| Folder | Name | Target |
|---|---|---|
| `` | `Movie.Name.2019.1080p.BluRay.x264-GRP.mkv` | `Movies/Movie Name (2019)/Movie Name (2019).mkv` |
| `Movie.Name.2019.1080p.BluRay-GRP` | `abc123.mkv` | `Movies/Movie Name (2019)/Movie Name (2019).mkv` |
| `` | `Blade.Runner.2049.2017.2160p.mkv` | `Movies/Blade Runner 2049 (2017)/Blade Runner 2049 (2017).mkv` |
| `` | `Movie.2019.CD2.avi` | `Movies/Movie (2019)/Movie (2019) - part2.avi` |
| `` | `Movie.2019.1080p.en.srt` | `Movies/Movie (2019)/Movie (2019).en.srt` |
| `Movie.2019.1080p-GRP/Featurettes` | `Making Of.mkv` | `Movies/Movie (2019)/Featurettes/Making Of.mkv` |
| `` | `Movie.2019.Trailer.1080p.mkv` | `Movies/Movie (2019)/Trailers/Movie.2019.Trailer.1080p.mkv` |
| `Movie.2019.1080p-GRP/Sample` | `movie-sample.mkv` | none |
| `` | `Show.S01E02.mkv` | none (with only movies on) |
| `` | `Home Video.mp4`, `movie.nfo` | none |
| `Movies/Movie (2019)` | `Movie (2019).mkv` | none (already there) |

With both kinds on, TV is tried first. Names are made safe with 023's `sanitize_name`.
