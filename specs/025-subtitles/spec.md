# Feature Specification: Subtitles

**Feature Branch**: `025-subtitles`

**Created**: 2026-11-05

**Status**: Draft (builds on 038, the processing pipeline)

**Input**: Roadmap entry 025:
> Fetch subtitles for downloaded video in the languages the user picks: first from what Put.io
> already finds on OpenSubtitles, then, when none exist and automatic transcription is on, by
> sending the audio to a Whisper server. putio-sync does not run Whisper itself; it calls any
> server with the OpenAI-style transcription API (faster-whisper-server/speaches,
> whisper.cpp's server), set by URL. Subtitles are saved next to the video as
> `.<lang>.srt`.

## Context

- Put.io looks up subtitles for each video: from OpenSubtitles, from `.srt` files in the same
  folder, and from tracks inside the video file. Put.io can return any of them as SRT. Today
  putio-sync ignores this.
- Feature 038 adds a Subtitles stage to each item's pipeline, after the download and before
  Translating and Post-processing. In Transmission mode, Radarr and Sonarr wait for that stage
  to finish. Subtitles named `<video>.<lang>.srt` are then imported as extra files.
- Put.io files are often deleted after download (post-download delete). Put.io's subtitle list
  is only there while the file is on Put.io.
- Feature 026 (translation) uses this stage's output. When no subtitle exists in a wanted
  language, 026 makes one from a subtitle in another language.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Subtitles from Put.io in my languages (Priority: P1)

The operator picks the subtitle languages they want, for example English and Spanish. For each
downloaded video, putio-sync asks Put.io which subtitles it has. For each wanted language, it
saves the best one next to the video as `<video>.<lang>.srt`, before Radarr/Sonarr import.

**Why this priority**: It costs nothing extra (Put.io already did the search), it covers most
videos, and it ships alone.

**Independent Test**: Against a mock Put.io that offers English and French subtitles for a
video, with English and Spanish wanted, `movie.en.srt` is saved next to the video, Spanish is
reported as missing, and French is not saved.

**Acceptance Scenarios**:

1. **Given** English is wanted and Put.io has an English subtitle, **When** the video's item
   reaches the Subtitles stage, **Then** `<video>.en.srt` is written next to the video, and the
   timeline shows the step as done, with its source (OpenSubtitles, folder or embedded).
2. **Given** Put.io has several English subtitles, **When** one is picked, **Then** it is
   picked in this order: embedded in the file or a folder file (these match the release), then
   OpenSubtitles.
3. **Given** `<video>.en.srt` already exists, for example from the torrent, **When** the stage
   runs, **Then** it is kept and nothing is fetched for English.
4. **Given** no subtitle languages are set, **When** items download, **Then** the Subtitles
   stage is skipped as "not configured".
5. **Given** an item has several videos (a season pack), **When** the stage runs, **Then** each
   video gets its own subtitles, and the timeline shows one line per video.
6. **Given** post-download delete is on, **When** an item downloads, **Then** subtitles are
   fetched before the file is deleted from Put.io, and the delete waits for that.

---

### User Story 2 - Transcribe when Put.io has none (Priority: P2)

The operator turns on automatic transcription and enters the URL of a Whisper server on the
home network (and a model name and key if the server needs them). When no subtitle in any
wanted language can be found, putio-sync sends the video's audio to that server and saves what
comes back as `<video>.<lang>.srt`, in the language the audio is in.

**Why this priority**: It fills the gaps that OpenSubtitles leaves, mostly new or rare
releases, but it needs a server the user runs and takes minutes per video.

**Independent Test**: Against a mock transcription server, a video with no Put.io subtitles
sends its audio, gets SRT text back, and saves it with the language the server reports.

**Acceptance Scenarios**:

1. **Given** transcription is on and no subtitle in a wanted language exists, **When** the stage
   runs, **Then** the audio is sent to the server, and the result is saved as
   `<video>.<detected lang>.srt`.
2. **Given** the detected language is not a wanted one, **When** the result comes back,
   **Then** it is still saved, so translation (026) can make the wanted languages from it, and
   the timeline says so.
3. **Given** the server is down or times out, **When** the step runs, **Then** it fails like
   any optional step (038): it is retried, then the item goes on without it and is marked
   completed with warnings.
4. **Given** transcription is off, **When** no subtitle exists, **Then** the step ends as done
   with "none found", and the item is not held back.
5. **Given** the operator presses "Test" next to the server URL, **Then** a short sample is sent
   and the result (it works and returns text, or the error) is shown.

---

### User Story 3 - Choose per category, and fetch later by hand (Priority: P3)

The operator turns subtitles off for a category (for example `radarr`, because Bazarr handles
it there). The Local files page (044) gets a "Fetch subtitles" action on a video, which runs the
same steps for a file that is already on disk.

**Why this priority**: It avoids clashing with Bazarr, and covers videos downloaded before this
feature. Neither is needed for the first release.

**Acceptance Scenarios**:

1. **Given** subtitles are off for `radarr`, **When** a `radarr` item downloads, **Then** the
   Subtitles stage is skipped with "not for this category".
2. **Given** a video on disk whose Put.io file is gone, **When** "Fetch subtitles" is pressed,
   **Then** only transcription is possible, and the UI says so before it starts.

### Edge Cases

- A video file with no audio track, or audio the server can't read: the step fails with the
  server's message.
- A very long video (over 3 hours): the audio is sent in parts, and the parts' timestamps are
  joined without gaps or overlaps.
- Put.io returns a subtitle that isn't valid SRT (wrong encoding, VTT): text is turned into
  UTF-8 SRT. If that fails, the next candidate is tried.
- Language codes: Put.io's three-letter codes (`eng`, `spa`) and the server's two-letter codes
  (`en`) are mapped to one form for file names. Files use two-letter ISO 639-1 codes where one
  exists, since that is what Radarr/Sonarr parse.
- A forced or SDH subtitle: these are named `<video>.<lang>.forced.srt` and `<video>.<lang>.sdh.srt`
  when Put.io marks them, and they never count as the language's main subtitle.
- Non-video files in an item (samples, extras): videos under 100 MB, or with "sample" in the
  name, are skipped.
- Staging (038 watch-folder mode): subtitles are written inside the staging folder, so they
  arrive with the video in the same rename.

## Requirements *(mandatory)*

### Functional Requirements

**Languages and placement**

- **FR-001**: The operator MUST be able to choose an ordered list of wanted subtitle languages.
  The default is an empty list, which means the stage is off.
- **FR-002**: Subtitles MUST be saved next to their video as `<video name>.<lang>.srt`
  (`.forced` and `.sdh` when marked), UTF-8, using two-letter codes where they exist.
- **FR-003**: A subtitle that already exists for a wanted language MUST be kept, and nothing
  MUST be fetched for that language.
- **FR-004**: Subtitles MUST run as the Subtitles stage of the 038 pipeline. The step changes
  files, so in both modes Radarr/Sonarr wait for it.

**From Put.io**

- **FR-005**: For each video in an item, putio-sync MUST get Put.io's list of subtitles and save
  the best one for each wanted language. Best means embedded or folder first, then
  OpenSubtitles, then Put.io's own order.
- **FR-006**: Put.io subtitles MUST be fetched before the file is deleted from Put.io. When
  post-download delete is on, the delete MUST wait until the Subtitles stage is done or given up.
- **FR-007**: If a subtitle fails to download or convert, the next candidate MUST be tried.

**Transcription**

- **FR-008**: The operator MUST be able to turn on transcription and set the server URL, an
  optional model name, an optional API key and a timeout. Transcription is off by default.
- **FR-009**: When transcription is on and no wanted-language subtitle was found for a video,
  putio-sync MUST send the video's audio to the server, using the OpenAI-style transcription
  API, and save the returned SRT as `<video>.<detected lang>.srt`.
- **FR-010**: Long audio MUST be sent in parts, and the timestamps joined, so the result has no
  gaps or overlaps.
- **FR-011**: Settings MUST have a Test button that checks the server with a short sample and
  shows the result.
- **FR-012**: Transcription steps MUST be optional in 038 terms: retried, then given up, never
  holding an item forever.

**Per-category and by hand**

- **FR-013**: Subtitles MUST be able to be turned off per category.
- **FR-014**: The Local files page MUST have a "Fetch subtitles" action on a video. It runs the
  same steps for that file, using Put.io when the file is still there.

**Observability**

- **FR-015**: Metrics MUST count subtitles saved by source (putio_embedded, putio_folder,
  opensubtitles, transcribed) and language, and transcription seconds. Language is bounded to
  the configured list, plus `other`.
- **FR-016**: Logs MUST record each saved subtitle with video, language, source and size. The
  API key MUST never appear.

### Key Entities

- **Subtitle settings**: wanted languages, transcription on or off, server URL, model, key,
  timeout, and categories turned off.
- **Subtitle candidate**: what Put.io offers for a video: language, source, name, forced or SDH.
- **Saved subtitle**: the file written next to the video: language, source, flags, size.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: For videos where Put.io has a subtitle in a wanted language, the subtitle is
  saved before Radarr/Sonarr import, in 100% of the tested cases.
- **SC-002**: Radarr and Sonarr import the `.srt` files together with the video, when "Import
  Extra Files" includes `srt`.
- **SC-003**: Fetching subtitles from Put.io adds under 10 seconds per video to the time before
  import.
- **SC-004**: A transcription failure never holds an item back for longer than the 038 retry
  policy.
- **SC-005**: No existing subtitle file is ever overwritten.

## Assumptions

- Put.io's subtitle API gives, per file, a list of subtitles with key, language, name and
  source, and each one can be downloaded as SRT. Its exact shape is checked during planning.
- The audio is pulled from the local file. That needs an audio tool on the NAS (for example
  ffmpeg in the Docker image), which planning must justify. The alternative is to send the video
  file itself, which is much larger.
- The transcription server speaks the OpenAI `audio/transcriptions` API with
  `response_format=srt` or `verbose_json`. faster-whisper-server/speaches and whisper.cpp's
  server do.
- Bazarr users turn the stage off per category. putio-sync doesn't talk to Bazarr.
- Picking among several OpenSubtitles results uses Put.io's order. No hash matching beyond what
  Put.io does.
