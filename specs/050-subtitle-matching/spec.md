# Feature Specification: Subtitle Matching and Source Modes

**Feature Branch**: `050-subtitle-matching`

**Created**: 2026-11-05

**Status**: Draft (extends 025 Subtitles; builds on 038, the processing pipeline)

**Input**: The part of roadmap entry "025 Subtitles" marked "Not built yet":
> the OpenSubtitles API lookup by IMDb/TMDb ID, the filename parse, the confidence threshold, and the
> other modes (Download only apart from transcription off, Whisper only, Both with `.whisper.srt`).

The roadmap text is the requirement:

> Downloads are matched in order of confidence, with no guessing model: (1) **Hash**: Put.io's subtitle
> lookup by the file's OpenSubtitles hash. A hash match is the exact release, so timing is right.
> (2) **IDs**: the OpenSubtitles API (user's own API key) queried by the IMDb or TMDb ID, season and
> episode that Radarr/Sonarr already know, with results ranked by how closely the release name matches
> (group, source, resolution). (3) **Name**: for files outside the *arrs, a rule-based filename parse
> (title, year, `SxxEyy`) used for the same query. A match below a set confidence is skipped, not saved.
> The user picks a mode, globally and per category (and optionally per language): **Download only**
> (never transcribe), **Download, then Whisper** (transcribe only when no download matches),
> **Whisper only** (always transcribe, never download), **Both** (keep the download and a transcription
> side by side). Subtitles are saved next to the video as `<video>.<lang>.srt`; in **Both** mode the
> transcription is `<video>.<lang>.whisper.srt` so neither overwrites the other.

## Context

- Feature 025 is built. Today, for each video, putio-sync asks Put.io for its subtitle list (folder,
  embedded, OpenSubtitles), saves the best one per wanted language, and, when no wanted language was
  found and transcription is on, sends the audio to a Whisper server. That is the **Download, then
  Whisper** mode. Subtitles can be turned off per category, and the Local files page can fetch them by hand.
- What 025 cannot do: ask OpenSubtitles directly when Put.io finds nothing, tell a good match from a
  poor one, choose how transcription is used, or keep a download and a transcription side by side.
- Radarr and Sonarr know which movie or episode a download is for (including its IMDb and TMDb IDs and
  season and episode). putio-sync already talks to them as their download client, but it does not read
  that information today.
- Feature 026 (translation) will read the subtitles saved here. It needs to tell a transcription from
  a download, because a transcription is less reliable as a source to translate from.
- Put.io's own subtitle list is identified by the Put.io file, and Put.io files are often deleted after
  download. A match that works from the local file alone keeps working after that.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Choose how subtitles are made: four modes (Priority: P1)

The operator picks a mode: **Download only**, **Download, then Whisper**, **Whisper only** or **Both**.
The choice is made once for everything, can be overridden for a category (for example Whisper only for
`anime`), and, if wanted, for one language. Existing setups keep working with no change: today's
behaviour is **Download, then Whisper**, and with transcription off it is **Download only**.

**Why this priority**: It is the visible change, it needs no OpenSubtitles account, it works with
Put.io's subtitles as built, and it is the one that makes transcriptions recognizable for translation (026).

**Independent Test**: Against a mock Put.io offering an English subtitle and a mock Whisper server:
in Both, `movie.en.srt` (download) and `movie.en.whisper.srt` (transcription) both exist; in Whisper
only, no Put.io subtitle is requested and the transcription is `movie.en.srt`; in Download only, the
Whisper server is never called; in Download then Whisper with an English subtitle available, Whisper
is not called.

**Acceptance Scenarios**:

1. **Given** the mode is Download only, **When** no subtitle is found, **Then** nothing is
   transcribed, the video is reported as "none found", and the item is not held back.
2. **Given** the mode is Download, then Whisper, **When** a wanted-language subtitle is found or already
   exists, **Then** Whisper is not called; **When** none is found, **Then** it is transcribed (today's
   behaviour, unchanged).
3. **Given** the mode is Whisper only, **When** the stage runs, **Then** Put.io and OpenSubtitles are
   not asked for subtitles, the audio is transcribed, and the result is `<video>.<lang>.srt`.
4. **Given** the mode is Both, **When** the stage runs, **Then** a download is fetched as usual and a
   transcription is made even if the download was found, saved as `<video>.<lang>.whisper.srt`.
5. **Given** `<video>.<lang>.srt` already exists (from the torrent, say), **When** the mode is Both or
   Whisper only, **Then** it is kept untouched and the transcription still runs and is saved without
   overwriting anything: as `<video>.<lang>.whisper.srt` in Both, and in Whisper only it is kept as is
   and reported "already present" (no transcription replaces it).
6. **Given** a category has its own mode, **When** an item of that category runs, **Then** that mode
   applies; a mode set for a language overrides the category and global modes for that language.
7. **Given** a mode that needs Whisper (Whisper only, Both) while transcription is off or has no server
   URL, **When** the settings are saved, **Then** they are refused with a clear message.
8. **Given** a `.whisper.srt` file next to a video, **When** a later feature or a person looks at
   it, **Then** it is recognizable as a transcription from its name, and it is never treated as the
   language's main subtitle (so 025's "already exists, keep it" rule does not mistake it for a download).

---

### User Story 2 - Find the right subtitle on OpenSubtitles (Priority: P2)

When Put.io has no subtitle for a wanted language, putio-sync asks the OpenSubtitles service directly,
using the operator's own OpenSubtitles API key. It looks first for an exact match on the video file
(a file fingerprint), then by the movie's or the show's IMDb or TMDb ID with season and episode, which
Radarr or Sonarr already know. Results are ranked by how closely their release name matches the
video's release name: release group, source (BluRay, WEB-DL, ...) and resolution. A result that
matches the file is saved with full confidence; one below the confidence threshold is skipped, not saved.

**Why this priority**: It covers the gap Put.io leaves (no subtitle, or only a mismatched one), and the
exact and ID matches give subtitles that are in sync, but it needs an account and a key and depends on a
daily download quota.

**Independent Test**: Against a mock OpenSubtitles API (and a fake source of IDs standing in for
Radarr/Sonarr), a video whose Put.io list is empty is found by file fingerprint first, then by ID; with
two results for an episode, the one whose release group, source and resolution match the video is
saved, and a result matching nothing is not saved.

**Acceptance Scenarios**:

1. **Given** the mode downloads, a language is still missing after Put.io, and an OpenSubtitles key is
   set, **When** the video's fingerprint matches a result, **Then** that subtitle is saved with full
   confidence, whatever its release name.
2. **Given** no fingerprint match and Radarr/Sonarr know the video's IDs, **When** the stage runs,
   **Then** OpenSubtitles is searched by IMDb or TMDb ID (and season and episode for an episode), and
   the results are ranked by group, source and resolution against the video's release name.
3. **Given** the best result scores below the confidence threshold, **When** the stage runs, **Then** it
   is not saved, the timeline says "best match below threshold" with the score, and the language is
   reported missing (and Whisper is used if the mode says so).
4. **Given** the OpenSubtitles download quota is used up, **When** a download is needed, **Then** no
   further downloads are tried until the quota resets, the step says so, it is not retried as a failure,
   and the mode's fallback (Whisper in Download, then Whisper) still applies.
5. **Given** a season pack, **When** the stage runs, **Then** each episode is matched to its own season
   and episode, not to the pack's.
6. **Given** no OpenSubtitles key is set, **When** the stage runs, **Then** this part is skipped
   silently and the Put.io-only behaviour of 025 applies.
7. **Given** the operator presses "Test" next to the OpenSubtitles settings, **Then** the key (and login,
   if set) is checked and the result shows, including the remaining download quota when known.
8. **Given** an existing subtitle for the language, **When** the stage runs, **Then** nothing is searched
   or downloaded for it and nothing is overwritten.

---

### User Story 3 - Match files that did not come through Radarr or Sonarr (Priority: P3)

For a video with no IDs from Radarr or Sonarr (a manual download, a watch-folder file, a file on the
Local files page), putio-sync reads the title, year and season and episode (`SxxEyy`) from the file name
with fixed rules and uses them for the same OpenSubtitles search. The same ranking and threshold apply,
and a name-based match must score higher on the release name than an ID match to be saved, because a
title found by name can point at the wrong film.

**Why this priority**: It extends US2 to the rest of the library; it is the least certain source of a
match, so it comes last.

**Independent Test**: With file names such as `Show.Name.S02E05.1080p.WEB-DL.x264-GRP.mkv` and
`Movie.Title.2019.2160p.BluRay.x265-GRP.mkv`, the parse gives the expected title, year, season,
episode, resolution, source and group, and a mock OpenSubtitles API is queried with them. A name that
cannot be parsed (no title) searches nothing.

**Acceptance Scenarios**:

1. **Given** a video outside the *arrs whose name has a title and `SxxEyy` (or a year), **When** the
   stage runs and nothing was found earlier, **Then** OpenSubtitles is searched by that title,
   season and episode (or year).
2. **Given** a name that gives no usable title, **When** the stage runs, **Then** no search is made and
   the step says "name could not be read".
3. **Given** a name-based result whose title does not match the parsed title closely, **When**
   ranking, **Then** it is not saved even if its release name matches.
4. **Given** the Local files "Fetch subtitles" action, **When** pressed on a video, **Then** it uses
   the same three levels (the file fingerprint, IDs when the file is still linked to an *arr download,
   then the name) and the same modes.

### Edge Cases

- Files under about 128 KiB cannot be fingerprinted: the fingerprint step is skipped, the rest goes on.
- The fingerprint is computed from the local file, so it works after the Put.io file is deleted.
- A Radarr/Sonarr lookup that fails or times out is not an error: the name parse is used instead,
  and the step notes "IDs not available".
- A download with several episodes in one file (`S01E01E02`): the first episode is used for the search.
- Hearing-impaired and forced results are never used as a language's main subtitle (025 rule); the
  search asks for them only when they are wanted under their own names.
- A result flagged as machine-translated is not saved.
- Mode change while an item is running: each step reads the current settings when it starts, as 025 does.
- OpenSubtitles returns the same subtitle for two languages' searches: it is saved once, under its own language.
- Whisper only and an existing `<video>.<lang>.whisper.srt`: kept, nothing replaced.
- Both mode and the detected transcription language is not wanted: it is still saved, as
  `.<detected>.whisper.srt`, so translation (026) can use it.
- Secrets: the OpenSubtitles key and password never appear in logs, API responses, errors or metrics.

## Requirements *(mandatory)*

### Functional Requirements

**Modes**

- **FR-001**: There MUST be four modes: Download only, Download then Whisper, Whisper only, Both.
- **FR-002**: The mode MUST be settable globally, per category, and optionally per wanted language.
  The most specific wins: language, then category, then global.
- **FR-003**: Settings written for 025 (no mode set) MUST keep their behaviour: Download then Whisper
  when transcription is on, Download only when it is off. The existing `excluded_categories` still
  turns the stage off for a category and wins over any mode.
- **FR-004**: Download only MUST never call a transcription server. Whisper only MUST never ask Put.io
  or OpenSubtitles for subtitles. Download then Whisper MUST transcribe only when no wanted-language
  subtitle was found or already exists (025's rule). Both MUST do both.
- **FR-005**: A mode that needs transcription MUST be refused at save time when transcription is off or
  has no valid server URL.
- **FR-006**: In Both, the transcription MUST be saved as `<video>.<lang>.whisper.srt`; in the other
  modes it is saved as `<video>.<lang>.srt`.
- **FR-007**: A file named `<video>.<lang>.whisper.srt` MUST be recognizable as a transcription and MUST
  NOT count as the language's main subtitle when deciding whether one already exists.
- **FR-008**: No existing subtitle file is ever overwritten, in any mode.

**Matching**

- **FR-009**: When a wanted language is still missing after Put.io's list, and an OpenSubtitles key is
  set, putio-sync MUST search OpenSubtitles in this order and stop at the first level that gives a
  result at or above the threshold: (a) the file fingerprint; (b) IMDb or TMDb ID, with season and
  episode, from Radarr/Sonarr; (c) the title, year, season and episode parsed from the file name.
- **FR-010**: The fingerprint MUST be computed from the local video file, so it does not depend on the
  file still being on Put.io.
- **FR-011**: IDs MUST be taken from the Radarr or Sonarr that holds the download, when there is one. If
  it cannot be reached, the file-name level is used.
- **FR-012**: Results MUST be ranked by how closely their release name matches the video's release
  name: release group, then source, then resolution (and codec as a small tiebreak).
- **FR-013**: Every OpenSubtitles candidate MUST get a confidence from 0 to 100. A fingerprint match is
  100. A match below the configured threshold MUST NOT be saved; the skip and its score are shown on the
  step and counted. Put.io's own subtitles (025) keep being accepted as before.
- **FR-014**: The file-name parse MUST be rule-based (no learned model) and give title, year, season,
  episode, resolution, source, codec and group where present. The same parse is used on result release names.
- **FR-015**: OpenSubtitles downloads MUST respect the account's download quota: a quota that is used
  up pauses further downloads until it resets and is not a step failure. Search requests MUST be paced
  and a "too many requests" answer MUST be waited out, not hammered.
- **FR-016**: The operator MUST be able to set the OpenSubtitles API key and, optionally, a username and
  password (for a larger quota), the confidence threshold, and test the connection from Settings.
  With no key this whole part is off.
- **FR-017**: Matching MUST work for the Local files "Fetch subtitles" action as it does for downloads.
- **FR-018**: A result flagged machine-translated MUST NOT be saved.

**Observability and secrets**

- **FR-019**: Metrics MUST count subtitles saved by source (adding the fingerprint, ID and name levels
  of OpenSubtitles and keeping the 025 sources), skips by reason (below threshold, quota, no result),
  and OpenSubtitles requests by result. Label values are fixed sets.
- **FR-020**: Logs MUST record, per video and language, the level that matched, the release name
  chosen, its confidence and the reason for any skip. The API key, username and password MUST never
  appear in logs, metrics, errors or API responses (masked as in 025).

### Key Entities

- **Subtitle mode**: one of the four; resolved per video and language from the language, category and
  global settings.
- **Match source**: the level that found a subtitle: Put.io list, OpenSubtitles by fingerprint, by ID,
  by name; or transcription.
- **Release info**: title, year, season, episode, resolution, source, codec, group, parsed from a file
  or release name.
- **Media info**: IDs and season and episode for a download, from Radarr/Sonarr.
- **Candidate score**: the 0 to 100 confidence of an OpenSubtitles result, with the reasons.
- **OpenSubtitles settings**: key, optional login, threshold, quota state.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A setup configured for 025 behaves identically after this feature with no settings
  change, in 100% of the existing subtitle tests.
- **SC-002**: In each of the four modes, the tested videos end with exactly the files that mode defines
  (and nothing overwritten) in 100% of cases.
- **SC-003**: For a video with a fingerprint match on OpenSubtitles, the matched subtitle is saved
  without relying on any release-name comparison.
- **SC-004**: For tested episodes and movies with known IDs and several results, the result whose
  group, source and resolution match is picked over those that do not in 100% of cases, and a result below
  the threshold is never saved.
- **SC-005**: Looking for a subtitle on OpenSubtitles adds under 15 seconds per video (fingerprint,
  IDs, name together) on a normal connection, and never delays a download.
- **SC-006**: When the daily quota is used up, no item fails or is held back because of it.
- **SC-007**: A transcription saved by Both mode can be told from a download by file name alone.

## Assumptions

- Put.io's subtitle list is requested by Put.io file ID, not by a hash the client sends; whether Put.io
  uses the file's hash inside is not documented in its SDKs (see research R1). The "hash" level of the
  roadmap is therefore met two ways: Put.io's list stays the first stop as built, and the new
  OpenSubtitles fingerprint search uses a hash computed locally from the video file.
- The operator has (or creates) their own OpenSubtitles API consumer key; putio-sync ships none. A
  username and password are optional and give a larger quota. Quota figures are the service's and may change.
- Radarr/Sonarr already configured for putio-sync (URL and API key under the arr settings) are the
  source of IDs; no new *arr setting is needed. Downloads that did not come through them use the name.
- The default confidence threshold is 65 on a 0 to 100 scale, with scores for a match by ID alone
  (nothing else agreeing) of 60. It is a guess to be tuned on real libraries, and is settable.
- Whisper writes the language it hears. In per-language modes, the mode of the language that is heard
  (or the category and global mode when that language is not wanted) decides the transcription's
  file name.
- "Per language" modes decide what is fetched or transcribed for that wanted language; there is one
  transcription per video, not one per language.
- Bazarr users keep turning subtitles off per category. putio-sync does not talk to Bazarr.
- Translation (026) is not part of this feature. This feature only keeps `.whisper.srt` recognizable.
- No new third-party crates are needed (see the plan).
