# Quickstart: Subtitles (025)

## Try Put.io subtitles (US1)

1. Settings, Subtitles card: add `en` (and `es`), Save. No ffmpeg is needed for this part.
2. Download a movie through the watch folder or a transfer. In the Active page the Subtitles stage shows a
   `videos` line and one line per video; when done, `Movie.en.srt` sits next to the video.
3. With Radarr or Sonarr in Transmission mode, make sure "Import Extra Files" includes `srt`; the `.srt` is imported
   with the video.
4. With post-download delete on, the Put.io file stays until the stage ends (Active page shows "Waiting for
   subtitles"; `pipeline_deferred_deletes` holds the row), then is deleted.

## Try transcription (US2)

1. Run a Whisper server, for example speaches: `docker run -p 8000:8000 ghcr.io/speaches-ai/speaches:latest-cpu`.
2. Settings, Subtitles: turn on transcription, URL `http://<host>:8000`, model (for example
   `Systran/faster-whisper-small`), press Test. Expected: `ok` and the ffmpeg path. whisper.cpp users enter the
   full `/inference` URL.
3. Download a video Put.io has no subtitle for. The step shows chunk progress; the result is
   `Movie.<language>.srt`.
4. On macOS without ffmpeg: `brew install ffmpeg`, or set `ffmpeg_path`.

## Per category and by hand (US3)

1. Add `radarr` to `excluded_categories`; a radarr download shows the stage skipped, "not for this category".
2. Local files page, open a video's row menu, "Fetch subtitles". If Put.io no longer has the file the page asks
   before starting a transcription.

## Verify (developer)

- `make ci`.
- Edge-DB migration: copy a DB that has 038 tables, start the daemon, check `pipeline_attempts` row count is unchanged.
- First run against the real Put.io API: record the real `source` strings and the download path in
  `contracts/putio-subtitles.md` (open questions in research.md).
- Docker: `docker build` and note the image size increase from ffmpeg in research R2.
