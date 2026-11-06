# Quickstart: Subtitle Matching and Source Modes (050)

## Modes (US1)

1. Settings, Subtitles: pick the mode. To try Both: turn transcription on (025 quickstart), set the mode to Both, Save.
2. Download a video that Put.io has an `en` subtitle for. Expected next to the video: `Movie.en.srt` (download) and,
   after the transcription step, `Movie.en.whisper.srt`.
3. Whisper only for one category: add the category under overrides. A download of that category shows no Put.io
   call; the file is `Movie.<lang>.srt`.
4. Download only with transcription on: no request reaches the Whisper server.
5. Saving Both with transcription off is refused with a message.

## OpenSubtitles (US2)

1. Create your own OpenSubtitles API consumer key (service account page). Settings, Subtitles, OpenSubtitles: key
   (and login for a larger quota), press Test. Expected: `key accepted` and the remaining downloads.
2. Download a release for which Put.io lists nothing. In the timeline: `es: os_hash 100` or `os_ids 80 (group,
   source)`. Below the threshold: `best match below threshold (60)`; nothing is saved.
3. Tune: read the logged scores (`subtitle match` lines) and move `min_confidence`.
4. Quota: when it runs out the step says `opensubtitles: quota used up until <time>`; the item still finishes.

## Name only (US3)

Put `Show.Name.S02E05.1080p.WEB-DL.x264-GRP.mkv` into the watch folder (no *arr). The log shows `level=name` with the
parsed title, season and episode. Local files page, Fetch subtitles on a video: same levels and modes.

## Verify (developer)

`cargo test -p putio-sync-subtitles -p putio-sync-core -p putio-sync-arr -p putio-sync-pipeline`, then `make ci`
(locally, by the maintainer). Live check against a real key and the *arr APIs: task T070, not part of CI.
