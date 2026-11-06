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

### 021 Keep folders out of empty-folder cleanup

A way to tell the empty-folder cleanup and the doctor to leave a folder
alone: a "keep" toggle on the Put.io page and in folder rules, plus ignore
patterns in Settings. Kept folders show in the cleanup preview with the reason.

### 022 Stream mode with DLNA

A mode that leaves media on Put.io instead of downloading it. putio-sync
manages the files on Put.io and serves them to the home network as a DLNA
media server, so TVs and players can browse and play them. It streams from
Put.io on demand, using Put.io's MP4 conversions where a player needs them.

### 023 Organise TV shows on Put.io

For stream mode: move finished downloads into a `Show/Season NN/` layout on
Put.io, with episodes named `Show - SNNEMM - Title`. Names are matched from the
release name, with a preview before anything moves and an undo.

### 024 Organise movies on Put.io

The same for movies: `Movie (Year)/Movie (Year).ext`, with extras and samples
kept apart or removed.

### 025 Subtitles (done)

Fetch subtitles for downloaded video in the languages the user picks: first
from what Put.io already finds on OpenSubtitles, then, when none exist and
automatic transcription is on, by sending the audio to a Whisper server.
putio-sync does not run Whisper itself; it calls any server with the
OpenAI-style transcription API (faster-whisper-server/speaches, whisper.cpp's
server), set by URL. Subtitles are saved next to the video as `.<lang>.srt`.

### 026 Subtitle translation

Translate subtitles into the languages wanted through an LLM: a base URL,
model and optional API key, using the OpenAI-compatible chat API so Ollama,
Open WebUI, LM Studio and hosted APIs all work. Subtitles are translated in
chunks with timestamps kept exactly, and a test button checks the connection
and model.

### 028 Lua plugins: sync extension points

Event-handler plugins are done (see PLUGINS.md). Still to do: plugins that
decide things during a sync, called synchronously with a short time limit:
custom file filters, rename rules and folder rules. They reuse the plugin
manifest, sandbox and `putio` API, and need a safe answer when a plugin fails
or is slow (fall back to the built-in rule).

### 029 WebAssembly plugins

Sandboxed plugins compiled to WebAssembly, so they can be written in Go
(TinyGo), Rust, JavaScript and other languages. Same events and API as 028,
with permissions per plugin (network, file paths) and install from a file or
URL.

### 030 Notifications

Send notifications for finished, failed and stuck downloads, failed transfers,
low Put.io storage and doctor problems. The ntfy, Discord, Telegram and
Pushover channels already exist as bundled plugins from 028 (each picks its
events, has a test button, and groups repeats). Still to do: email (SMTP),
which cannot be a plugin, stuck-download and doctor-problem events, and a
simpler Notifications page over the bundled channels.

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

### Magnet file lists from peers (deferred from 043)

When the .torrent caches never have a magnet's file list, fetch it from
BitTorrent peers (DHT and the metadata extension), with no piece data. Deferred
in 043: it reveals the NAS's IP to the swarm and costs about 93 crates. The
Stats page's "Held magnets" tile shows how often holds run out, which is what
this would fix. Research is in `specs/043-magnet-cache-retry/research.md`.

## Queued: packaging

- **Minimal runtime image** on Docker Hardened Images (dhi.io), plus a
  separate dev image. Settle which registry the NAS uses first.
- **arm64 snap**: build on an arm64 runner or through Snapcraft's build
  service; CI builds amd64 only today.
- **Flathub**: needs a public repository and probably a desktop wrapper
  around the web UI.
- **Homebrew workflow**: `.github/workflows/homebrew.yml` is not valid YAML
  and fails on every push.
