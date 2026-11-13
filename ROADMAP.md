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

### 056 Usage profile in the setup wizard

Goal: a new user is set up and syncing in under 60 seconds. The wizard asks
four things and detects or recommends the rest:

1. **Language** (first, before the setup code): the 051 UI languages,
   starting on the browser's language, switching the wizard at once. Saved
   as the UI language; every wizard page keeps a switcher.
2. **Setup code**: the console prints a setup link with the code in it
   (`http://host:port/setup#code=…`), so opening it skips typing the code.
3. **Put.io link** (013 device code).
4. **How will you use putio-sync?** (one screen): what is downloaded
   (movies, TV, music, ebooks and audiobooks, software, other), and how it
   arrives (Radarr, Sonarr or Lidarr found by 018 discovery and pre-ticked;
   added by hand on Put.io; a watch folder). Kinds are pre-ticked from the
   Put.io account's top-level folder names (`Movies`, `TV`, `Music`…).

Then a review screen with **Start syncing**. Each recommendation has a
one-line reason and can be changed or turned off; "I'll set it up myself"
keeps today's step-by-step wizard. The download folder step is shown only
when the detected folder (`/downloads` in a container) fails the 013 check,
and the dry run becomes a link on the review screen instead of a step.

What the answers set:

- **Folder rules**: a rule per ticked kind with its type preset (video,
  audio, ebooks; "everything" for software), on the Put.io folder the kind
  was found in. The default `blocked_extensions` list blocks `exe`, `msi`,
  `dmg`, `pkg`, `apk` and `jar`, so a software profile must lift it there,
  or nothing downloads.
- **Radarr/Sonarr/Lidarr**: `[arr]` on with a generated password, each app
  set up through the 018/049 flow (download client, path mapping), with
  Completed Download Handling and Import Extra Files listed as
  recommendations to turn on.
- **Release pre-check** (042): `min_size_mb` defaults to 50, which turns
  away music, ebook and software releases. Set it per category and watch
  folder for each kind (for example 5 MB for music, 1 MB for ebooks, off for
  software).
- **Delete from Put.io after download**: recommended on when an *arr app
  imports the files (Put.io is a staging area), off otherwise (Put.io is the
  library). Shown as a choice that deletes, with the 015 warning, and with
  empty-folder cleanup (005) turned on with it.
- **Subtitles** (025): video profiles get the UI language (plus English
  when it differs), download-only, using Put.io's subtitles, which need no
  key. Music, ebook and software categories are excluded. OpenSubtitles,
  transcription and translation stay off; they need keys or a server.
- **Watch folder**: on with the detected folder (`/watches` in a container)
  when ticked.
- **Left at defaults** and not asked: workers, verify, existing files
  (resume), retries, bandwidth, stall checks, storage free-up (it deletes),
  scheduled doctor (on), logging, release check. Metrics, DLNA, Tailscale,
  notifications, hooks and plugins move out of the wizard to Settings. The
  review screen mentions DLNA for video profiles, and
  Tailscale identity sign-in when a Tailscale socket is found.

The answers are saved, so Settings can run the profile again later and
preview the differences as 015 does, without overwriting changes the user
made by hand.

## Queued: features

### 019 Sign-in options (done)

Besides the API token: a username and password (a sign-in form, and HTTP basic
auth), and sign-in with Google, GitHub or Discord, each with an allow-list of
accounts or verified emails. Browsers get a signed session cookie; the token
stays for scripts and Radarr/Sonarr. Set up in Settings › Sign-in. See
`specs/019-sign-in-options/`.

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

### 038 Processing pipeline and its view (done)

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

### 055 Update checker and self-update (done)

Checks GitHub Releases on start and once a day (can be turned off) and shows a newer
version, with its release notes, next to What's new. A binary downloaded from GitHub
updates itself with Update now, or automatically once no transfer is running when that
setting is on: it fetches a bsdiff patch (`.xz` full binary when there is none), checks it
against the minisign-signed `SHA256SUMS`, swaps the binary, restarts, and rolls back if the
new version fails to start. Snap, apt, Homebrew, winget, Flatpak, Docker and
Unraid/CasaOS/Umbrel installs show their own update command or step instead. The release
workflow builds the patches and signs `SHA256SUMS`. Left for the maintainer: the release
signing key (`RELEASE_PUBKEY` and the `MINISIGN_*` secrets; until then every self-update
fails the signature check and releases stop at signing), and checking channel detection
and Update now on real installs (T023, T041). See `specs/055-update-checker/`.

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

### 057 More plugin hooks and events

Plugins can only watch today: the 14 event types in EVENTS.md, plus
`on_tick`, `on_action` and `health`. This adds events and callbacks, in this
order. New event types stay in schema version 1 (added types only), go into
`schema.json` so the 055 event picker shows them, and each gets a sample
payload.

1. **`pipeline.finished`** for every item that leaves the pipeline, with
   `result` (`success`, `warnings`, `skipped`), the item's files and
   `warnings`. `pipeline.completed` is sent only with warnings, so nothing
   says an item is fully ready (downloaded, subtitled, translated). The
   bundled media-server plugins move to it and drop their folder grouping
   where they can.
2. **Plugins as pipeline steps**: `plugin.on_item(item, stage)` gets the same
   item JSON as a command step and returns `done`, `skip` (`pre_download`
   only) or an error. A stage lists the plugin by name like a hook, and the
   required/optional rules, retries and Retry/Skip on the Active page apply
   as they are. Runs on the plugin's own thread with `timeout_secs`.
3. **Health events**, which 030 needs: `transfer.stalled` (the Put.io
   transfer stall check flagged a transfer), `doctor.problem` (a scheduled doctor run found a new
   problem), `putio.unreachable` / `putio.recovered`, and `auth.expired` (the
   Put.io token was refused).
4. **Pairs and gaps**: `sync.started`, `download.started`, a throttled
   `download.progress` (at most one per file per 10 s, opt-in by name, not
   matched by `"*"`), `storage.recovered`, and `local_disk.low` /
   `local_disk.recovered` for the download folder's disk (`storage.low` is
   Put.io's).
5. **Subsystems with no events**: `subtitles.found` / `subtitles.missing`,
   `translation.finished` / `translation.failed`, `archive.extracted` /
   `archive.failed`, `arr.unreachable` / `arr.recovered`, and
   `cleanup.deleted` when delete-after-download removes a file from Put.io.
6. **Lifecycle**: `daemon.started`, `daemon.stopping` and `config.changed`
   (which sections changed, no values) as events; `plugin.on_config_changed()`
   when the plugin's own settings are saved, and `plugin.on_unload()` before a
   reload or shutdown so it can send what it has grouped, both with a short
   time limit.

The sync-time decision hooks (filter, rename, folder rules) stay in 028.
Each new callback gets a starter in `examples/plugins/` and a section in
PLUGINS.md; each event gets its table in EVENTS.md.

### 058 Crash reports, then anonymous usage stats

Nothing in putio-sync calls home today, and the README says what little
leaves the machine. Keep it that way by default: nothing is sent without the
person seeing it or turning it on.

**Part 1, crash reports to GitHub (first).** The daemon cannot open issues
itself: a token in the binary would let anyone file issues as the project.
So the person files the report from their own GitHub account:

- A panic hook writes a crash file next to the config: version, build
  (Docker, snap, Homebrew, binary…), OS and arch, uptime, the thread, the
  panic message and the backtrace, with secrets removed the same way as for
  logs, plus the last 50 log lines. Up to 10 crash files are kept.
- On the next start the web UI shows "putio-sync stopped unexpectedly" with
  **Report on GitHub**, **Copy report** and **Dismiss**. The report is shown
  in full first so names and paths can be removed. **Report on GitHub** opens
  a prefilled new-issue page (a `crash` issue form in `.github/ISSUE_TEMPLATE`)
  in the person's browser. The title holds a short crash signature from the
  top frames, and a **Search existing issues** link is shown first so repeats
  can go on the same issue. A body over the URL limit is cut, and the issue
  asks for the copied full report to be pasted.
- `putio-sync crash-report` lists, shows and opens the same reports on the
  CLI. The doctor warns about a crash in the last 7 days that has not been
  dismissed.
- Kills that run no panic hook (out of memory, SIGKILL, a container
  restart) are noted as "stopped without a report" when the last run did not
  shut down cleanly.
- Release builds keep symbol names so backtraces are readable; tests force
  a panic and check the file, the redaction and the issue URL.

**Part 2, anonymous usage stats (later, opt-in).** A once-a-day ping that is
off by default and offered on the 056 review screen and in Settings, with the
exact JSON shown before it is turned on: a random install id (not tied to
the Put.io account, reset with one button), version, build, OS and arch, and
which features are on (*arr, watch folders, pipeline stages, plugins by
bundled name only, DLNA, Tailscale) and size buckets (files per day, library
size). Never names, paths, hosts or Put.io data, and the receiver keeps no
IP addresses. Needs a
place to receive it, to be decided in the spec: a small Cloudflare Worker
(free tier) with a public summary page on the project site, or a hosted
privacy-first service such as Aptabase or PostHog. Until then, release
download and image pull counts are the usage signal.

### 059 Plugin links and a plugin registry (done)

Plugins declare who made them and where to read more: `author`, `license`, `homepage`,
`docs`, `repository` and `issues` in `plugin.toml`, shown on the plugin's tab; the bundled
plugins set them. `putio-sync plugins check FOLDER` runs the reload's checks and compiles
the Lua for authors and the registry's CI. The public registry,
`devclinton/putio-sync-plugin-registry`, lists reviewed plugins, one entry per plugin with
pinned releases and their SHA-256, plus CONTRIBUTING.md, a review checklist, an entry
template, two example plugins and checks on every pull request. Settings › Plugins ›
**More plugins** searches the list (kept 6 hours), installs a release after checking its
SHA-256 and manifest (it starts off), shows what an update changes before making it, puts
the old version back when the new one does not load, and removes registry plugins. The
doctor warns about withdrawn or delisted ones. See `specs/059-plugin-registry/`.

### Magnet file lists from peers (deferred from 043)

When the .torrent caches never have a magnet's file list, fetch it from
BitTorrent peers (DHT and the metadata extension), with no piece data. Deferred
in 043: it reveals the NAS's IP to the swarm and costs about 93 crates. The
Stats page's "Held magnets" tile shows how often holds run out, which is what
this would fix. Research is in `specs/043-magnet-cache-retry/research.md`.
