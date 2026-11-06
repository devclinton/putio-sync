# Contract: `MediaLookup` and the *arr history calls

## Trait (core)

```rust
pub struct MediaRequest<'a> { pub putio_file_id: Option<i64>, pub category: Option<&'a str> }
pub struct MediaInfo {
    pub kind: MediaKind,                 // Movie | Episode
    pub title: Option<String>, pub year: Option<u16>,
    pub imdb_id: Option<String>,         // digits only; series for an episode
    pub tmdb_id: Option<u64>,
    pub episodes: Vec<(u16, u16)>,       // (season, episode) pairs of the download; empty for a movie
}
pub trait MediaLookup: Send + Sync {
    fn lookup<'a>(&'a self, req: MediaRequest<'a>) -> BoxFut<'a, Option<MediaInfo>>;   // None = unknown, never an error
}
```

`None` covers: no Put.io file ID, file not an arr download, *arr unreachable, no history, parse failure. Failures
are logged at debug/warn with the *arr name, never the API key.

## Implementation (arr crate)

1. `arr_items.get_by_file_id(putio_file_id)` -> item with `hash` and `category`. None -> `None`.
2. Pick servers: those whose `ClientInfo.categories` has the category; else all configured. Radarr and Sonarr by kind.
3. Radarr: `GET {url}/api/v3/history?downloadId={HASH}&includeMovie=true&page=1&pageSize=50` with `X-Api-Key`.
   Sonarr: `...&includeSeries=true&includeEpisode=true`. [U: parameter names on current versions; Radarr's
   `downloadId` and `includeMovie` seen in generated client docs.]
4. Take records whose `downloadId` equals the hash (case-insensitive). Movie: `movie.imdbId` (`tt` stripped),
   `movie.tmdbId`, `movie.year`, `movie.title`. Episode: `series.imdbId`, `series.tmdbId` (fallback `tvdbId` not sent
   to OpenSubtitles), `series.title`, and `(episode.seasonNumber, episode.episodeNumber)` of every record.
5. Cache by hash, 1 hour, including negative results. One request per server per hash.
6. Fallback when history is empty: `GET /api/v3/movie/{movieId}` or `/series/{seriesId}` using the record's IDs.

Timeout 10 s per call; the existing `ArrService::http()` client is reused. No call is made when no *arr is configured.

## Use in the engine

For an episode video: parse `SxxEyy` from the file name; choose the pair in `episodes` with the same season and
episode; no parse or no pair -> no IDs level for that video. For a movie video, `kind` must be Movie.
