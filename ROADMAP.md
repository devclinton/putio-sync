# Roadmap

Features are built with Spec Kit, one numbered feature per branch under
`specs/`. Shipped changes are in [CHANGELOG.md](CHANGELOG.md). This file lists
what is queued, in the order planned. Each entry is the starting brief for
`/speckit-specify`.

## Queued: onboarding and setup

### 014 Guided tour and in-app help

A short, skippable tour of each page on first visit, re-runnable from
Settings. Help panels and tooltips explain each setting in plain words, with
links to the guides from 016.

### 016 Onboarding guides

Getting-started guides for each way to run putio-sync (Unraid/Docker, snap,
Homebrew, binary). Task guides for Radarr/Sonarr, folder rules, watch
folders, metrics and Grafana, and troubleshooting with the doctor. They live
in `docs/` and are linked from the UI.

## Queued: features

### 019 Sign-in options

The web UI and API are protected by one API token today. Add HTTP basic auth
with a username and password, and sign-in through OAuth providers: Google,
GitHub, and Discord if its OAuth stays free. Limit who may sign in by account
or email, and keep the API token for scripts and Radarr/Sonarr.

### 023 Organise TV shows on Put.io (done)

For stream mode: files kept on Put.io move into `TV Shows/Show/Season NN/`, with
episodes named `Show - SNNEMM - Title`, read from the release name. Moves wait
on the Put.io page's **Organise** panel unless `apply = "auto"`, and each can be
undone; plugins' folder and rename rules may place files instead. Shows whose
names need a metadata lookup (TVDB titles, scene or anime absolute numbering)
keep the release's name. See `specs/023-organise-tv-putio/`.

### 024 Organise movies on Put.io (done)

The same for movies: `Movie (Year)/Movie (Year).ext`, read from the release
name or its release folder. Extras go into the film's `Trailers`,
`Featurettes`… folders; samples are left out of the library, not deleted.
Titles that need a metadata lookup (TMDB names, years one off) keep the
release's name. See `specs/024-organise-movies-putio/`.

### 025 Subtitles (partly done)

Built so far (spec `specs/025-subtitles`): Put.io's own subtitle list for the file
(OpenSubtitles, folder and embedded), then Whisper when no wanted language is
found; per-category off; Fetch subtitles from Local files. Built in 050
(`specs/050-subtitle-matching`): the four modes (**Download only**, **Download,
then Whisper**, **Whisper only**, **Both** with `.whisper.srt`), the OpenSubtitles
API lookup by fingerprint, by IMDb/TMDb ID and by the parsed file name, and the
confidence threshold. Not checked against the live services yet: the 050 tasks
list a maintainer check with a real key.

Get subtitles for downloaded video in the languages the user picks, from two
kinds of source: downloads and Whisper transcription.

Downloads are matched in order of confidence, with no guessing model:

1. **Hash**: Put.io's subtitle lookup by the file's OpenSubtitles hash
   (`opensubtitles_hash`). A hash match is the exact release, so timing is
   right.
2. **IDs**: the OpenSubtitles API (user's own API key) queried by the IMDb or
   TMDb ID, season and episode that Radarr/Sonarr already know, with results
   ranked by how closely the release name matches (group, source,
   resolution).
3. **Name**: for files outside the *arrs, a rule-based filename parse (title,
   year, `SxxEyy`) used for the same query.

A match below a set confidence is skipped, not saved.

Whisper: putio-sync does not run Whisper itself; it calls any server with the
OpenAI-style transcription API (faster-whisper-server/speaches, whisper.cpp's
server), set by URL.

The user picks a mode, globally and per category (and optionally per
language):

- **Download only**: never transcribe.
- **Download, then Whisper**: transcribe only when no download matches.
- **Whisper only**: always transcribe, never download.
- **Both**: keep the download and a transcription side by side.

Subtitles are saved next to the video as `<video>.<lang>.srt`; in **Both**
mode the transcription is `<video>.<lang>.whisper.srt` so neither overwrites
the other.

### 026 Subtitle translation (done)

Translate subtitles into the languages wanted through an LLM: a base URL,
model and optional API key, using the OpenAI-compatible chat API so Ollama,
Open WebUI, LM Studio and hosted APIs all work. Subtitles are translated in
chunks with timestamps kept exactly, and a test button checks the connection
and model. A user-supplied service that answers now or calls back is supported too.
See `specs/026-subtitle-translation/tasks.md` for the deviations.

### 028 Lua plugins: sync extension points (done)

Event-handler plugins and sync rules are both done (see PLUGINS.md, "Sync
rules"): a plugin can declare a file filter, a rename rule and a folder rule,
each called with a short time limit and passed over when it fails or is slow.
Examples for each are in `examples/plugins/`. See
`specs/028-lua-sync-extension-points/` for the design.

### 029 WebAssembly plugins (done)

Plugins can be WebAssembly modules (`runtime = "wasm"`) with the same events,
sync rules and API as 028, run by wasmi (an interpreter, about 1.4 MB, so every
release target still builds). Per-plugin folder permissions (`putio.fs`) for
both runtimes, and install from a file or URL. Rust and Go examples are in
`examples/plugins/`; no JavaScript example yet. See `specs/029-wasm-plugins/`.

### 030 Notifications (done)

Email over SMTP (lettre with rustls, about 0.85 MB), `transfer.stuck` and
`doctor.problem` events for hooks and both plugin runtimes, and a Settings ›
Notifications page over email and the bundled ntfy, Discord, Telegram and
Pushover plugins. See `specs/030-notifications/`.

### 035 Mobile app

The web UI wrapped with Capacitor as an iOS and Android app: connect to a
putio-sync server by address or QR code, push notifications for the events
from 030, and share links or torrent files from other apps straight to Put.io.

### 038 Processing pipeline and its view

Every item goes through a pipeline of steps, and the UI shows where it is:

Pre-download hooks → Downloading → Pre-processing hooks → Generating
subtitles → Translating (one branch per language, run in parallel, for
example Japanese and Spanish) → Post-processing hooks → Notifications.

- The Queue page shows each item's current step and progress. Expanding an
  item shows the whole pipeline as a timeline: each step's state (waiting,
  running, done, failed, skipped), how long it took, and its log; the
  translation branches side by side. Failed steps can be retried on their
  own. Finished items keep their timeline in History.
- Steps that are not configured are skipped, so the plain pipeline is just
  Downloading. Subtitles (025), translation (026) and hooks (027) plug in as
  steps, so this lands before them or with 025.
- The pipeline is the same for items from Radarr/Sonarr, the watch folder and
  plain sync.

Radarr and Sonarr must not import an item before its pipeline is done:

- Transmission mode (preferred): the item reports as still downloading, at
  under 100%, until the last step that changes files (subtitles,
  translation, post-processing hooks) has finished. Radarr/Sonarr then see it
  complete and import video and subtitles together. The extra steps show in
  their queue as the item's status message.
- Watch-folder (blackhole) mode: work happens in a hidden staging folder on
  the same disk, and the finished folder is moved into place in one rename,
  so a folder scan never sees a half-finished item.
- Subtitles are named `<video>.<lang>.srt` so Radarr/Sonarr import them as
  extra files (with `srt` in "Import Extra Files"). If Bazarr already handles
  subtitles, the subtitle steps can be turned off per category.
- A failed optional step (for example one translation) does not hold the
  import back forever: after a set number of retries the item completes
  without it, and the failure shows on the item and in notifications.

### 051 Multi-language web UI (done)

The web UI is translated through catalogs keyed by its English text, in every base language
the atranslate service supports, with a Language card in Settings. Left for later: review of
the machine translations by native speakers, plural forms, and translating messages that
come from the daemon. A translated Snap Store listing is not possible: the store keeps one
summary and description per snap (see `specs/051-ui-i18n/research.md`); a Flathub listing
could carry translations through AppStream.

### 055 Update checker and self-update

Check GitHub Releases for a newer version on start and once a day (can be
turned off), and show it in the web UI with that release's notes, next to
What's new. What happens next depends on how putio-sync was installed:

- A binary downloaded from GitHub (Windows, macOS, Linux) updates itself. An
  "Update now" button, plus an opt-in setting to update automatically, which
  waits until no transfer is running. It downloads a small LZMA patch from the
  running version when the release has one, and falls back to the full
  archive. It checks the SHA-256, swaps the binary, restarts, and rolls back if
  the new version fails to start.
- Managed installs (snap, apt, Homebrew, winget, Flatpak, Docker, and
  Unraid/CasaOS/Umbrel) never touch their own files. They detect the channel
  and show the exact update command or step. Docker tries to detect the image
  and tag so it can name the right `docker pull`, and otherwise shows a plain
  warning.

Depends on 053's release binaries. The release workflow also builds the
patches.

### 055 Plugin workbench (done)

A basic editor and debugger on the Plugins page, with no breakpoints or stepping. New
plugin from three starters (webhook notifier, store counter, tick poller); an editor with
line numbers that checks, saves (keeping `.bak` files) and reloads only that plugin, off by
default behind `plugin_editor`; Copy to edit for bundled plugins and a read-only manifest
with re-install for WASM ones. A Run panel for events, `on_tick`, `health`, settings
actions and sync rules, for Lua and WASM, dry by default (requests recorded, scratch store),
with the last 20 runs kept for Run again. A "Writing and debugging plugins" guide in the
docs book, the starters in `examples/plugins/`, and docs links from the UI. The whole
workbench loads with `import()`, outside the always-loaded UI. See
`specs/055-plugin-workbench/`.

### Magnet file lists from peers (deferred from 043)

When the .torrent caches never have a magnet's file list, fetch it from
BitTorrent peers (DHT and the metadata extension), with no piece data. Deferred
in 043: it reveals the NAS's IP to the swarm and costs about 93 crates. The
Stats page's "Held magnets" tile shows how often holds run out, which is what
this would fix. Research is in `specs/043-magnet-cache-retry/research.md`.
