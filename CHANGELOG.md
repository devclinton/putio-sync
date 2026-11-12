# Changelog

User-facing changes to putio-sync. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Every change that a user would notice adds a line under **Unreleased** in the
same commit. Builds from main (the edge snap, the `:main` Docker image) carry
this file and show the Unreleased section with the build; a release renames it
to the version. Changes from before October 2026 are in the per-crate
changelogs, such as [putio-sync-cli/CHANGELOG.md](putio-sync-cli/CHANGELOG.md).

## [Unreleased]

### Added

- Sign-in options: besides the API token, sign in with a username and password (on the
  sign-in page, or as HTTP basic auth), or with Google, GitHub or Discord. Each provider
  has an allow-list of account names or verified emails. Set them up in the new
  Settings › Sign-in, which shows the redirect URL to register and has Sign out
  everywhere. Browsers stay signed in with an HttpOnly, SameSite=Strict cookie for
  `session_hours` (default a week); writes also need a CSRF header. Passwords are stored
  as argon2id hashes, client secrets are masked, failed sign-ins count toward the lockout,
  and the doctor reports a provider that is on but incomplete. The token, Tailscale
  sign-in and the Radarr/Sonarr and Transmission APIs work as before.
- Email notifications: `[notifications.smtp]` sends the events you choose as plain-text
  email through your mail server, over STARTTLS (the default, never falling back to plain
  text), TLS or, for a relay on your LAN, no encryption and no login. Repeats of one kind
  are grouped into a summary like the notifier plugins do. The password is masked like
  other secrets.
- Two new events: `transfer.stuck`, once when a Put.io transfer stalls, and
  `doctor.problem`, once when a doctor check becomes a problem. Event hooks and Lua and
  WebAssembly plugins get them, and the bundled ntfy, Discord, Telegram and Pushover
  notifiers send both by default.
- Settings › Notifications: email settings with a test button, and the bundled notifier
  plugins on one page, each with its state, settings at a glance, events and a test
  button. It replaces the notifications card in Settings › General.
- Plugin workbench in Settings › Plugins: make a Lua plugin from a starter (webhook
  notifier, store counter, tick poller), edit its `plugin.toml` and `main.lua` with line
  numbers, check and save them (the old files kept as `.bak`) and reload only that plugin.
  Bundled plugins can be copied to edit; WebAssembly plugins show their manifest and can be
  replaced. A Run panel calls `on_event` with a sample of any event, `on_tick`, `health`, a
  settings action or a sync rule, for Lua and WebAssembly plugins, and shows the result or
  the error with its line, time, memory, log lines, HTTP requests (secrets masked) and the
  store before and after. Runs are dry by default: requests are recorded, not sent, and the
  store is a scratch copy. The last 20 runs can be run again. Editing is off until
  `plugin_editor = true` or **Edit plugins here**.
- A "Writing and debugging plugins" guide in the docs, and three starter plugins with a
  README each in `examples/plugins/`. The Plugins page, the workbench and the hooks card
  link to the matching docs pages.

## [0.2.2] - 2026-11-10

### Added

- A project website at <https://devclinton.github.io/putio-sync/>: a landing page with
  install steps for every platform and screenshots, and the docs as a searchable book
  built from the repository's Markdown. It loads nothing from other sites. The snap,
  Unraid, Umbrel and CasaOS listings link to it.
- The docs on the website in 19 more languages, machine-translated and marked as such,
  with a language picker in the menu bar.
- Stream mode: `[download] mode = "stream"`, or `mode` on a folder rule, keeps media on
  Put.io instead of downloading it, and the media server's Put.io root plays it from
  there. New videos that aren't MP4 get an MP4 version requested from Put.io
  (`stream_mp4`, on by default). Radarr/Sonarr items still download. Settings ›
  Downloads has the mode and warns, with a fix button, when the media server or its
  Put.io root is off; the rule editor and preview show the mode. The
  `putio_sync_stream_mp4_requests_total` metric counts MP4 requests.
- Organise TV shows on Put.io: with `[organise] tv = true`, streamed episodes are planned
  to move into `TV Shows/Show/Season NN/` as `Show - S02E05 - Title.mkv`, read from the
  release name. The Put.io page's **Organise** panel lists the moves to apply, undo or
  dismiss, and **Find files** plans moves for files streamed earlier; `apply = "auto"`
  moves at once. A target outside stream mode, a name already taken or a file gone from
  Put.io is refused and shown. Plugins' folder and rename rules can place files instead.
  The name reader was checked against 400 real Sonarr and 400 Radarr imports. `doctor`
  warns when organising is on but nothing is streamed; `putio_sync_organise_moves_total`
  counts the moves.
- Organise movies on Put.io: with `[organise] movies = true`, streamed films are planned
  to move into `Movies/Movie (Year)/Movie (Year).mkv`, read from the release name or, for
  an obfuscated file, its release folder. Parts (`CD2`) and subtitle languages are kept.
  Trailers, featurettes, behind-the-scenes, deleted scenes and interviews go into the
  film's `Trailers`, `Featurettes`… folder, which Plex and Jellyfin read as extras;
  samples are left where they are. Movie moves use the same Organise panel, undo, auto
  mode and plugin rules as TV, with a Kind column. Checked against 400 real Radarr
  imports: 349 get Radarr's title and year, the rest differ only by metadata, and none
  is read as an episode.

- Sync rules for plugins: a plugin can declare `hooks = ["filter", "rename", "folder"]`
  to decide during a sync whether a file is downloaded, streamed or skipped, what it is
  called locally and which folder it goes in. Each call has `hook_timeout_ms` (500 ms by
  default, shown in Settings › Plugins as **Sync rule time limit**); a plugin that
  fails, is slow or answers wrong is passed over and the built-in rule applies, and
  after 3 failures in a row its sync rules rest for 5 minutes. Files a plugin skips are
  never deleted from Put.io, and Radarr/Sonarr files are left alone. Three working
  examples are in `examples/plugins/`: `min-size-filter`, `clean-names` and
  `tv-folders`. See PLUGINS.md.
- WebAssembly plugins: a plugin with `runtime = "wasm"` in its `plugin.toml` is a
  module built from Rust, Go or another language, with the same events, sync rules,
  settings, actions, store and time and memory limits as Lua. It runs in a sandbox
  (wasmi, about 1.4 MB on the binary) that offers only the plugin API: WASI calls for
  files, sockets or the environment answer "not supported". Examples in Rust
  (`rust-clean-names`) and Go (`go-min-size-filter`) with build steps are in
  `examples/plugins/`.
- File access for plugins: `[permissions.files]` names folders a plugin may read or
  write, fixed or from its settings, and `putio.fs.read/write/list/exists` reach them
  (Lua and WebAssembly alike). Settings › Plugins lists the folders, and turning the
  plugin on approves them, as with hosts; `..` and links cannot lead out.
- Install a plugin from Settings › Plugins or `POST /plugins/install`: from a URL (a
  `.wasm` or a `plugin.toml` with its entry beside it) or from files. It starts off,
  and an existing plugin is replaced only when asked.

### Changed

- The media server's Put.io root no longer needs `putio_folder`: without one it shows
  the folder the sync mirrors.

### Fixed

- The Docker image sets `HOME=/config`, so it no longer logs "Failed to create log
  directory /.putiosync/logs: Permission denied" at every start when run as a user
  other than 99 (`--user`). The Docker Compose, Umbrel and CasaOS listings set it
  too, for 0.2.1. File logs are now kept in `/config/.putiosync/logs`.
- The README, snap description and website give the daemon's idle memory on a NAS as
  about 33 MB; they said 11 MB. The docs site's dark themes have readable link and
  code colors, links in text are underlined, and code blocks can be scrolled from the
  keyboard.
- The Windows install steps no longer say `winget install`: putio-sync isn't in winget
  yet, so they point at the installer and the zip. The winget package ID is now
  `devclinton.putio-sync` and the Flatpak ID `io.github.devclinton.putio-sync`, so
  neither suggests Put.io publishes them.

## [0.2.1] - 2026-11-09

### Fixed

- The Windows, `.deb` and x86_64 Linux downloads build again. Their build failures kept
  0.2.0 from being published, so 0.2.1 is the first release with every download. On
  Windows the disk space metrics now come from the volume holding the download folder.

## [0.2.0] - 2026-11-09

### Added

- The Snap Store description has a short summary of the main features in Spanish,
  Chinese, Hindi, French and Arabic (machine-translated), after the English text.
- Release downloads for every platform, each listed in a `SHA256SUMS` file: standalone
  Linux binaries for x86_64 and arm64 that need no libraries at all (static, web UI built
  in), a macOS (Apple silicon) installer that can start the daemon at login, and Windows
  x64 and arm64 installers that run it in the background at startup or when you sign in
  and add a Start menu link to the web UI. The snap is now built for arm64 too.
- Hardened container images on Docker Hardened Images, with no shell or package manager:
  `ghcr.io/devclinton/putio-sync:dhi-<version>`, `:dhi-ffmpeg-<version>` with ffmpeg for
  subtitles, and `:dev-<version>` / `:dev-ffmpeg-<version>` with a shell for debugging.
  `latest`, `<version>` and `main` stay the Alpine image.
- Home-server app store listings: an Unraid Community Applications template, Umbrel and
  CasaOS packages, and a Docker Compose file for TrueNAS SCALE, Runtipi, Portainer and
  Dockge custom apps (`packaging/STORES.md`). The project is MIT licensed, now with a
  `LICENSE` file.
- The web UI speaks 27 languages. It follows the browser's language, and **Settings ›
  General › Language** picks another for this browser, switching at once without a reload.
  Arabic and Hebrew read right to left. The translations are machine-made; names, paths,
  logs and daemon messages stay as they are.

- Subtitle source modes and OpenSubtitles matching. Choose per category or language whether
  subtitles are downloaded only, downloaded then transcribed if missing (the default,
  unchanged), transcribed only, or both (the transcription is saved as `Movie.en.whisper.srt`
  beside the downloaded `Movie.en.srt`). With your own OpenSubtitles API key, what Put.io
  lacks is looked up by the file's fingerprint, then by the IMDb or TMDb id Radarr or Sonarr
  recorded, then by the title in the file name. Each match is scored and one under the
  threshold (`min_confidence`, default 65) is not saved; the step shows the score or how close
  the best match came. Set it in **Settings › Downloads › Subtitles**, which also has a
  **Test OpenSubtitles** button. New metrics: `putio_sync_subtitles_skipped_total` and
  `putio_sync_opensubtitles_requests_total`.
- Keep folders out of the empty-folder cleanup. **Keep folder** on a folder's rule card on
  the Put.io page (or **Keep out of cleanup** in the rule editor, `keep = true` in a
  folder rule) keeps it and everything below it; **Keep patterns** in Settings › Put.io ›
  Put.io cleanup (`[cleanup] keep_patterns`) keep every folder whose name or path matches,
  in the Only / Never download pattern language. The daily run, **Remove empty folders**
  and the doctor all leave kept folders alone. The preview now lists the empty folders it
  keeps and why (protected, too new, a rule or a pattern), and so does
  `putio-sync files clean-empty --dry-run`.
- Subtitle translation. When a wanted language has no subtitle, the new Translating stage
  makes one from a subtitle the video already has, through a chat model you run (Ollama,
  LM Studio, a hosted API) or your own service that answers now or calls back later at a
  one-time address. Set it up in **Settings › Downloads › Translation**, with a test button
  for each. **Translate** on Local files does it for a video already on the NAS. Timestamps
  are always kept from the source, and an existing subtitle is never overwritten.
- Subtitles. Set the languages you want in **Settings › Downloads › Subtitles** and each
  downloaded video gets `Movie.en.srt` files next to it, from what Put.io has (folder files,
  embedded tracks, OpenSubtitles) and, when you turn it on, from your own Whisper-style
  speech to text server (needs ffmpeg; the Docker image has it). Existing subtitle files are
  never overwritten, a category can be left out, and the Put.io file is kept until the
  Subtitles stage ends (24 hours at most). **Fetch subtitles** on Local files does it for a
  video already on the NAS.
- A processing pipeline. A hook with a `stage` is a step that runs for a download before it
  counts as finished: before the files start (holding the transfer), or on the NAS before
  and after processing. The Active page lists each item's steps and attempts with **Retry**
  and **Skip**; a required step that fails waits for you, an optional one is retried and
  then given up with a warning. Radarr and Sonarr see an item as 99% done until its steps
  pass, and watch-folder downloads are built in a hidden `.putio-sync-staging` folder and
  moved into place with one rename. Settings › Integrations has the steps and a Processing
  pipeline card. With no steps, nothing changes.
- **Media server**: putio-sync can show your downloaded files, and one Put.io folder, to
  TVs, VLC and Kodi on your home network (DLNA / UPnP). Turn it on in **Settings › Media
  server**, choose which folders are shared (Downloads, a category such as `radarr`, a
  watch folder or Put.io) and see whether it is running, the address players use and how many
  streams are playing. Players browse and seek; nothing can be changed or deleted from them.
  Run Docker with host networking so players can find it (see INSTALLATION.md).
- Plugins have their own page, **Settings › Plugins**. Each plugin that is on has a tab
  with its settings, buttons, test event and last run, marked **!** when it is broken,
  waits for approval or needs settings. Plugins that are off are one short list with
  **Enable**, which names the hosts the plugin will contact and turns it on once you
  approve them; **Disable** on a tab puts it back in the list with its settings kept.
  Old links to the plugins card, and the doctor's plugin hints, open the new page;
  unsaved edits now survive **Reload plugins**.
- A plugin sign-in code (Sign in with Plex) shows a **Copy** button and **Copy and open
  plex.tv/link**, which copies the code and opens the page where you enter it. The link stays
  while the page waits for the sign-in. Over plain http, where the browser can't copy, the
  code is selected for you to copy by hand.
- Radarr/Sonarr **Set up** now does the whole setup: after the download client it finds
  where the app sees putio-sync's download folder and proposes the Remote Path Mapping to
  create, change or remove, which it makes once you confirm and then checks again. It also
  shows Completed Download Handling and Import Extra Files (with `srt`) as recommendations,
  each with **Turn on**. Every saved application with a key has **Set up**, and running it
  again fixes drift without making duplicates. `putio_sync_arr_path_mapping_total` counts
  the mappings it changes.
- A **Logs** page in the web UI shows the daemon's recent log lines, kept in memory
  (`[logging] buffer_lines`, 2,000 by default, also under **Settings › Advanced › Logging**).
  Filter by level, module and text; follow new lines live with Pause and Resume; copy
  or download what is shown; and follow a `file_id`, transfer, hook, plugin or verify
  run to its page. The page can also turn up the level, for everything or one module,
  for 30 minutes or until restart, without touching the config file. Tokens, the
  Put.io app secret, webhook and hook URLs, hook headers, plugin secrets and
  `Authorization` values are masked before a line is kept. The doctor warns while
  debug or trace is on, and `putio_sync_log_lines_dropped_total` counts lines that
  left memory unseen.
- The Put.io app ID that turns on linking with a code at put.io/link can be
  entered in the web wizard's **Connect Put.io** step and in a new
  **Settings › Put.io › Put.io** card, with the register link beside it. Saving it turns
  the link on without a restart. Settings shows where the ID in use comes from
  (this setting, `PUTIO_SYNC_APP_ID` or the build) and is read-only while the
  environment variable sets it. `PUTIO_SYNC_APP_ID` now overrides
  `[putio] app_id`, as other environment variables override the config, and an
  app ID that isn't a number is refused.
- Download verification: each download is checked against Put.io's CRC32 (or
  its size where Put.io has none) before it gets its real name. The CRC32 is
  worked out while the file streams, so it is not read twice. A file that does
  not match is downloaded again up to `verify_max_redownloads` times (default 2),
  then fails with reason `integrity` and is never deleted from Put.io.
  **Verify everything** on the History page checks files already downloaded at
  `verify_read_limit_mb_per_sec`, can be paused and stopped, waits while the
  bandwidth schedule pauses downloads, and queues mismatches to be downloaded
  over (or only reports them). Local files can verify ticked files and folders.
  History shows how each download was checked, the Queue and Active pages show
  **Verifying**, the doctor gains a **Download verification** check, and there
  are new events (`verify.passed`, `verify.failed`, `verify.redownload`; the
  schema version stays 1), `putio_sync_verify_*` metrics, `/verify` routes and
  four settings under Settings › Download.
- Media server refresh: bundled Plex, Jellyfin and Emby plugins (off until
  turned on) ask the server to rescan just the folder of each finished download,
  grouped per folder over a short window, so new episodes and movies show up
  within seconds. Plex signs in with a code at plex.tv/link instead of a pasted
  token; Settings loads the servers and libraries to pick from, and path rules
  map the download folder to the path the media server sees. The doctor checks
  the server, the token and the path rules; tokens are never logged. For plugin
  authors: settings action buttons, fields with choices, a `path_map` field
  type, `tick_seconds`, a `health()` check, `putio.path`, `putio.download_dir`
  and `putio.metrics.count` (`putio_sync_plugin_counter_total`).
- Lua plugins: small sandboxed Lua scripts in a plugins folder (default
  `plugins` next to the config file), each with a `plugin.toml` manifest that
  names its events, permissions and config fields. Plugins run on the same
  events as hooks, under a time and memory limit per call, without `os`, `io`
  or `require`; a failing or slow plugin never holds up a sync. They may only
  call the hosts they declare and you approve, and keep a small key-value
  store. Secrets are masked and never logged. `[[plugins]]` entries turn them
  on and hold their settings. Bundled ntfy, Discord, Telegram and Pushover
  notifiers (off until turned on) send the first event of a kind at once and
  group later repeats into one summary. Settings › Plugins lists them with
  their state and the hosts they contact, edits their settings, approves
  hosts on save, sends a test event and reloads the folder without a restart
  (`GET /plugins`, `POST /plugins/test`, `POST /plugins/reload`,
  `/settings/plugins`). New event type `plugin.test`, a doctor check, Plugins,
  and metrics `putio_sync_plugin_runs_total`,
  `putio_sync_plugin_run_duration_seconds` and
  `putio_sync_plugin_events_dropped_total`. See `PLUGINS.md` (feature 028).
- Event hooks: `[[hooks]]` entries run a command or call a webhook when a
  download finishes or fails, a Put.io transfer is added, finishes or fails, a
  sync run ends, or Put.io storage runs low. The event goes as JSON (stdin for
  a command, the POST body for a webhook) in a versioned schema documented in
  `EVENTS.md` and served at `GET /events/schema`. Per hook: event filters,
  headers or env, and a timeout. Hooks never hold up a sync; webhooks are
  retried on errors, and output is logged. Settings › Event hooks edits them
  with **Send test event** (`GET /hooks`, `POST /hooks/test`). Header and env
  values are masked and webhook URLs cut down everywhere they are shown or
  logged. New metrics `putio_sync_events_total`,
  `putio_sync_hook_runs_total`, `putio_sync_hook_run_duration_seconds` and
  `putio_sync_hook_events_dropped_total`, and a doctor check, Event hooks
  (feature 027).
- Bandwidth schedules: `[[download.bandwidth_schedule]]` entries set the
  download limit by time of day and day of week (`days`, `start`, `end`,
  `limit` as bytes per second, `"unlimited"` or `"paused"`), in the daemon's
  local time; the first matching entry wins and an end before the start runs
  past midnight. The Status page has a Bandwidth card with the limit in force,
  why, the next change and the daemon's time, and **Pause 1 h**, **Slow 1 h**
  (`slow_limit_bytes_per_sec`, default 1 MiB/s) and **Resume schedule**, which
  survive a restart (`GET /bandwidth`, `PUT`/`DELETE /bandwidth/override`).
  Settings › Download has a schedule editor. Mistakes are refused with the
  entry and field named; overlaps, a schedule paused all week and limits under
  64 KiB/s are warned about. New gauges
  `putio_sync_download_bandwidth_limit_bytes_per_second` and
  `putio_sync_download_bandwidth_override`, and a doctor check, Bandwidth
  limit (feature 032).

- Only download and Never download patterns in folder rules (`include` and
  `exclude`), with AND, OR and NOT: `*.mkv OR *.mp4 -sample`, `*.{mkv,mp4}`,
  `*.mkv,*.mp4`, and `Sample/*` matched against the path from the Put.io root.
  The language is the one of the web UI's filter boxes, which now also take a
  comma as OR, `{a,b}` and paths. The Put.io page's rule card shows both and
  where they come from, the editor sets or clears them, file badges follow
  them, and Preview lists the files that would download. Settings › Download
  has **Preview patterns** for the account-wide values. A pattern with an
  unclosed quote or brace is refused when saved and when the config loads;
  Settings and the doctor's Config check warn about a stray OR, literal
  brackets, and a delete-blocked rule with an Only download pattern (feature
  020).
- Listen addresses and interfaces. `[api] bind` and `[metrics] bind` take one
  address or a list, each `ip:port`, `[ipv6]:port`, an interface name such as
  `eth0:15432` (every address the interface has, followed as it changes) or
  `tailnet:15432` (this machine's Tailscale address). An entry without an
  address waits and is checked again; nothing ever falls back to all
  addresses. `PUTIO_SYNC_API_BIND` and `PUTIO_SYNC_METRICS_BIND` take a
  comma-separated list. `tailnet_only` now means `tailnet:<port>` and keeps
  waiting for Tailscale instead of giving up after 5 minutes. `GET /listen`
  shows each entry and what it listens on now; `/settings/network` saves both
  lists, refusing a list with nothing to listen on, and
  `/settings/network/check` says whether the page you are on would still reach
  putio-sync. Settings has a Network group with both lists and what
  putio-sync listens on now, and asks before saving a list that would cut the
  page off. The doctor's new "Listening addresses" check warns about entries
  with no address and about Tailscale sign-in with the API on loopback.
  `server` now listens on the configured addresses (tailnet-only included)
  when no flag is given; `--bind` (an IP, interface or `tailnet`) replaces the
  list and `--port` sets the port of every entry. The Tailscale card and the
  Radarr/Sonarr host report the port actually listened on (feature 037).
- Tailscale awareness. Settings has a Tailscale card: whether Tailscale is on
  this machine (through tailscaled's socket, or the `tailscale` CLI), the
  MagicDNS name, the tailnet addresses, the HTTPS address from
  `tailscale serve`, and hints when something is missing. Opt-in sign-in
  with Tailscale identity (`[tailscale] identity_sign_in`, off by default):
  users on the allow-list (`allowed_users`; the card has a button to add this
  machine's owner) open the UI without the API token, through
  `tailscale serve` or straight over the tailnet, and the top bar shows who
  they are; everybody else still needs the token, and tagged
  devices and Funnel never get in this way. Opt-in `tailnet_only` makes the
  daemon listen on the tailnet IPv4 address instead of `[api] bind`, waiting
  for Tailscale when it starts later, and never falling back to the LAN;
  Radarr/Sonarr setup then offers the tailnet address first. Works
  with Tailscale in a sidecar container when its socket is mounted. Also
  `GET /tailscale`, a `tailscale` doctor check, and
  `putio_sync_tailscale_identity_total` in `/metrics` (feature 036).
- Put.io storage management. The Put.io page opens with a storage card: used,
  free and total space, the biggest top-level items, and the latest free-up
  runs with the files each one deleted. Below a warning level of free space
  (10% by default) the card, the Status page and the doctor warn. **Free up
  space** previews the oldest files that are already downloaded, with a local
  copy of the same size, and are not queued, shared or part of an unfinished
  transfer, until a target (20% free by default) is reached; only after you
  confirm are exactly those files deleted, each checked again first. Deleted
  files show in History. An opt-in daily run (`[storage] auto_free`, off by
  default) does the same at `run_at` when free space is below the warning
  level. Also `putio-sync files free-space [--dry-run] [-y]`, a Storage card
  in Settings, storage gauges and free-up counters in `/metrics`, and new API
  routes under `/putio/storage` (feature 033).
- A What's new panel in the web UI. After an upgrade, the first page opened
  lists the changelog entries the new build added; closing it marks them seen
  for every browser. The whole changelog, built into the binary, is on a
  What's new page linked from the version on the Status page. A new install
  shows no panel. New API routes: `GET /whats-new` and
  `POST /whats-new/seen` (feature 017).
- A configuration assistant on the Settings page. Each card previews what a
  save would change (old and new value, and whether it applies now or needs a
  restart) or why it would be refused; a preview never writes the config file.
  Each field says what it affects. Risky combinations are flagged in the
  preview, in a banner at the top of Settings and in the doctor's config
  check: for example deleting from Put.io after download into a temporary
  folder or a container layer, a watch folder inside the download folder, or a
  cleanup with no minimum age. A Defaults button fills a card with its
  defaults without saving (the download folder is kept). Backup and restore
  exports the settings as a TOML file and imports one over the current
  settings, with a preview first; the file never holds credentials or the
  Radarr/Sonarr section, and an imported file's credentials are ignored. New
  API routes: `POST /settings/{group}/preview`, `GET /settings/assistant`,
  `GET /settings/export` and `POST /settings/import` (feature 015).
- A first-run setup wizard. A daemon with no config, no Put.io token, a
  rejected token or an unusable download folder no longer exits: it stays up
  in setup mode, prints a one-time setup code to its console (`docker logs`,
  `snap logs`), and the web UI walks through connecting Put.io (device link at
  put.io/link, or paste a token), choosing and checking the download folder
  (free space, writable), choosing what to download (a preset for the whole
  account plus one per top-level folder), optional extras (watch folder,
  Radarr/Sonarr, metrics) and a dry run of what would be downloaded. "Start
  syncing" writes the config once, creates the API token and starts syncing
  without a restart. `putio-sync setup` runs the same steps in a terminal,
  with flags for headless installs. New settings: `[putio] app_id` and
  `[download] preset` (feature 013).
- The Put.io, Transfers and Local files pages can be filtered and sorted. The
  filter takes wildcards and AND, OR and NOT: `-*photoshop* *exe*` shows names
  containing "exe" but not "photoshop", `*.mkv OR *.mp4 -sample` shows mkv or
  mp4 files that aren't samples, and a plain word matches anywhere in the name.
  Click a column heading (or use the Sort menu at phone width) to sort; folders
  stay first. The filter and sort stay in the page address, so a reload keeps
  them. Select all now takes only the rows the filter shows, and Local files
  gets row checkboxes with a bulk Delete (feature 045).
- An Active page listing every transfer on its way to the NAS, once each, in
  its Put.io or NAS stage, with counts by state that filter the list. The
  Status page's Active transfers card summarises it, and the
  `putio_sync_pipeline_items` metric counts items by stage and state
  (feature 039).
- Pause and resume on the Active page, one item or a selection at a time. Pausing a
  transfer that is still on Put.io puts it **On hold**: Put.io keeps downloading it
  (Put.io has no pause), nothing is sent to Put.io, and its files are queued paused when
  they arrive. Radarr and Sonarr see a paused item as stopped, not failed. Rows on the
  Active page link to their Transfers and Queue rows, and the Queue page can be filtered
  to one transfer, folder or file and pauses and resumes single files (feature 039).
- A **Pause NAS downloads** switch on the Active and Queue pages (and
  `putio-sync queue pause` / `queue resume` with no file ids) stops every running NAS
  download and starts none until it is turned off. It survives a restart, a banner
  shows on the Active, Queue and Status pages while it is on, and `putio_sync_nas_downloads_paused` reports it (feature 039).
- A **Recently finished** list on the Active page shows items that left the pipeline in the
  last `[transfers] finished_retention_hours` (24 by default), done or failed with the
  reason, and how long each spent on Put.io, waiting for sync and on the NAS (feature 039).
- A Local files page in the web UI lists the download folder (with its
  `radarr/` and `sonarr/` folders) and each watch folder when watching is on,
  with sizes and times, and marks partial `.downloading` files and rejected
  watch files. Files and folders can be deleted (after a confirmation that
  shows a folder's size) or renamed, and a `.rejected` watch file can be
  re-checked: it gets its name back and the watcher checks it again on its
  next scan. A partial that is still downloading can't be deleted. This
  replaces the old `/local/files` API routes, whose path check could be
  passed with `..`; every path now stays inside its folder, and symlinks are
  not followed out of it (feature 044).
- Fake releases are caught before they reach Put.io. When Radarr or Sonarr
  sends a .torrent (or an indexer URL) that has no video file of at least
  50 MB, such as an `.exe` or a password-protected `.7z`, it's listed as failed
  with the reason and blocklisted, with no Put.io transfer created. Configure it
  under `[precheck]` (`enabled`, `dry_run`, `min_size_mb`, `allow_archives`, and
  per-category or per-watch-folder overrides). Magnet links are checked by
  looking their .torrent up in a public cache (itorrents.net by default; only
  the info hash is sent, and `magnet_lookup = false` turns it off). A magnet
  that isn't found goes to Put.io, and its files are checked there before
  any reach the NAS: a fake is deleted from Put.io and fails with "(checked
  on Put.io)". Watch-folder `.torrent` and `.magnet` files are checked the
  same way: a fake isn't uploaded and is renamed `<file>.rejected`. The Stats
  page shows how many releases were rejected and the bytes kept off the NAS,
  split by where they were checked and why, and how many magnets were found in
  a cache or sent to Put.io, with the fakes among those. History lists the
  rejected releases with the reason. A release the check missed can be
  flagged fake from the Put.io, Queue or History pages, with a note. For a
  Radarr or Sonarr item, the flag can also fail it and blocklist it. A wrong
  rejection can be marked not fake, and any flag can be undone. The Stats page
  sums up the reports and downloads them as CSV or JSON, with the file types,
  sizes and tracker host but never a link or key. Settings → Release check
  edits all of this and shows, for each category and watch folder, the
  settings and wanted file types it ends up with. Release records older than
  400 days are pruned daily (feature 042).
- A magnet link that no cache knows yet is held instead of going straight to
  Put.io. Sonarr or Radarr sees it as queued, and the Queue page lists it under
  "Waiting for a file list". The cache is asked again every 30 s for up to
  5 minutes. If the file list turns up it's checked like a .torrent, so a fake
  never reaches Put.io. If it doesn't, the magnet goes to Put.io and is checked
  there as before. The first ask now waits 3 s instead of up to 10 s. Set
  `hold_magnet_secs = 0` under `[precheck]` to turn holding off
  (`hold_retry_secs` and `first_lookup_secs` set the other two times; all
  three are on the Settings page's Release check card). A `.magnet` file in a
  watch folder is held the same way and stays in the folder; deleting it ends
  the hold. The Stats page's "Held magnets" tile shows how holds ended and how
  long the found ones took (feature 043).
- Metrics for putio-sync's own CPU, memory, threads, open files and disk I/O
  (`putio_sync_process_*`, Linux and Docker only), and
  `putio_sync_sync_in_progress`, which is 1 while a sync runs (feature 041).
- A Grafana dashboard for those figures,
  `packaging/grafana/putio-sync-resources.json` (`putio-sync resources`): CPU,
  memory, threads, open files against the limit, disk read and write and uptime,
  with restarts and new builds marked, next to downloads and syncs. Import it and
  pick your Prometheus data source; see README → Grafana dashboard (feature 041).
- `putio-sync archive` keeps a copy of your history outside Prometheus:
  `archive metrics` saves each day's 5-minute CPU, memory and activity figures
  as an OpenMetrics file, `archive db` saves a compressed copy of the database,
  and `archive summary` writes daily and monthly usage CSVs. Files go in
  `<config dir>/archive`; `scripts/archive/unraid-user-script.sh` runs them on a
  schedule. See README → Long-term archive (feature 041).
- A choice for files already on disk, `[download] on_existing`: resume
  (the default) continues partial files and skips complete ones, checked
  against Put.io's CRC32; overwrite replaces the old file once the new copy
  is complete; keep both saves the new file as `name (1).ext`. Set it in
  Settings, per folder rule, or with `--on-existing`. History shows the
  outcome and where the file was saved (feature 040).
- Radarr and Sonarr can be set up from Settings → Radarr / Sonarr without
  editing the config or restarting. "Set up automatically" finds them, then
  adds or updates putio-sync's download client inside each app, testing it
  first so a failure leaves nothing behind. The download client and the
  applications are now forms (Add, Edit, Test, Delete). The category for
  blocklisting is read from each app, and Test checks that the app can see the
  download folder, with help for path mappings. Item counts and last contact
  moved to a Radarr / Sonarr card on the Status page (feature 018).
- A Doctor page that checks the Put.io account and the sync setup: sign-in,
  storage, failed or stalled transfers, empty folders, duplicate files and
  leftover .torrent files, with one-click fixes (feature 010).
- Per-folder download rules: a folder and everything below it can use a type
  preset (video, audio, ebooks, no binaries, everything, or your own), extra
  allowed or blocked extensions, skip, and delete settings. Set and preview
  them from the Put.io and Settings pages (feature 009).
- Radarr and Sonarr can add putio-sync as a Transmission download client.
  Items go through Put.io to `<target>/<category>/`, and failed releases can be
  blocklisted in Radarr or Sonarr. Set it up under Settings → Radarr / Sonarr
  (feature 011).
- A redesigned web UI with light and dark modes, seven accent colours, a
  sidebar on desktop and a tab bar on phones (feature 012).
- Snap package with the daemon as a system service, published to the edge
  channel from main.
- Multi-select and bulk actions on the Put.io and Transfers pages (feature 008).
- Stalled Put.io transfers are flagged, with Re-add and Copy magnet actions
  (feature 007).
- A Stats page with bandwidth over time, a download heatmap, file types and
  sources (feature 006).
- Empty Put.io folders can be removed on demand, with a dry run, or daily.
  The daily run is off by default (feature 005).
- A web UI served by the daemon on port 15432: status, queue, Put.io files and
  transfers, history and settings, signed in with an API token (feature 004).
- Prometheus metrics on port 9742, with alerts and a Grafana dashboard, and a
  log level that can be changed while running (feature 003).
- `CHANGELOG.md`, shipped with every build.

### Fixed

- When a pipeline stage had parallel branches (such as one subtitle translation per
  language), a branch could stop before it started with "database is locked" and leave
  the item waiting for the operator; branches now wait their turn for the database.
- A command hook whose script had just been written could fail to start on Linux
  with "text file busy"; the start is now retried briefly.
- `cargo build --release` (and so `make ci`) failed on macOS after the October
  2026 Command Line Tools update: the stripped `sqlx` proc macro would not load.
  Proc macros and build scripts are no longer stripped; shipped binaries still
  are.
- The Homebrew formula workflow is valid YAML again, so GitHub no longer reports
  it as a failed run on every push. It still runs only when a release is
  published or when started by hand.
- The word "null" showed on the page in History › Verification, Settings ›
  Tailscale › This machine and the Put.io storage card, and could show in the
  Settings › Network list, the Radarr/Sonarr setup plan and test result, and
  the setup wizard's dry run. These parts now leave out what isn't there.
- A test now covers a **Verify everything** run waiting while the bandwidth
  schedule pauses downloads, and carrying on when it resumes (feature 034).
- A hooks test that timed the first run against 2 s failed now and then under
  a full CI run; it now allows 4 s (feature 027).
- Without a Put.io app ID, the setup wizard and `putio-sync setup` used to
  drop the put.io/link option with no word why. They now say the device link
  is off and how to turn it on (register an app at app.put.io/oauth and set
  `[putio] app_id` or `PUTIO_SYNC_APP_ID`), and so does the doctor when the
  sign-in fails. Its sign-in hint now points at `putio-sync setup`, since
  `putio-sync login` needs an app of your own too.
- `putio-sync setup` no longer shows the Put.io token as you type or paste it
  in a terminal. `putio-sync login` no longer writes the token exchange
  response (which holds the access token) to the debug log or to an error, and
  a failed request no longer shows the URL carrying the client secret.

- `GET /local/workers` reported each worker's share of the bandwidth limit;
  `total_bandwidth_limit_bps` is now the limit in force (null when unlimited,
  0 while paused) and the per-worker value is null (feature 032).

- `[download] whitelist_patterns` and `blacklist_patterns` (Only download and
  Never download in Settings) were saved but never applied.
- A fake release rejected right after Radarr or Sonarr grabbed it is now
  blocklisted. Before, putio-sync asked before the *arr had listed the item in
  its queue, gave up, and left it stuck as "downloading" with no new search.
  It now looks again for up to 10 minutes.
- The Radarr / Sonarr settings card no longer shows a stray "null" under its
  title.
- The Local files page no longer deletes or renames putio-sync's own database
  (`.putiosync.db` and its `-wal`, `-shm` and `-journal` files) when it sits in
  the download folder; they show as in use, like a partial that's downloading.
- `resume_enabled = false` and `--no-resume` now start partial downloads
  over; before, they did nothing. `--skip-existing` now works in daemon mode
  (feature 040).
- Radarr 6 can now connect to the download client. Radarr 6 opens the
  Transmission handshake with a GET, which putio-sync refused, so its Test
  failed with "Authentication Failure".
- Empty-folder cleanup now removes nested empty folders on Put.io. Put.io
  marks folders only by their type, so every folder was read as a file and
  kept. The doctor's empty-folder and duplicate checks and recursive delete
  had the same problem.
- Interrupted downloads resume, download settings from the config are
  honoured, and failed transfers can be retried (feature 004).
- Problems seen in the live NAS logs (feature 002).
- The Docker image builds again with the Radarr/Sonarr and doctor crates.

### Changed

- The README opens with a short overview, highlights, an install table and a Docker
  Compose quick start; the full manual follows.
- HTTPS uses rustls instead of the system's OpenSSL, with Mozilla's root certificates built
  in plus the system's own store, so a self-signed or company CA you trust still works.
- The edge snap is no longer rebuilt on every push to main; it is published when a
  maintainer runs the snap workflow, so it can trail main by a few commits. The `:main`
  Docker image is rebuilt only when code or the Dockerfile changes, not for docs-only
  commits.
- **Pause all** and **Resume all** on the Queue page are replaced by the Pause NAS
  downloads switch, and `POST /local/queue/pause` and `/resume` now turn that switch on
  and off instead of changing every row. Upgrade note: rows paused by the old Pause all
  stay paused after the upgrade, so resume them once from the Queue page. The pipeline
  adds the `pipeline_holds`, `pipeline_finished` and `daemon_flags` tables, created on
  start, and `GET /status` and `GET /local/queue` gain `nas_paused` (feature 039).
- Settings is split into seven sections (General, Put.io, Downloads, Rules and
  filters, Integrations, Network and access, Advanced), shown as a side list on
  a wide screen and a row of buttons on a phone, each with a line of help. Each
  section has its own address (`#/settings/downloads`), and
  `?field=download.verify` jumps to one setting and highlights it. A search box
  finds a setting by its label or help. A section shows *unsaved* while one of
  its cards has unsaved changes and a count of its problems, and the browser
  asks before closing the tab with unsaved changes. Rarely changed fields are
  hidden until **Show advanced** is on for that section. The Download group is
  shown as four cards (Downloads, Bandwidth, Verification, File filters). The
  warnings banner, the doctor, What's new, the History and Status pages and
  folder rules link straight to the setting, and the docs and messages name
  settings as "Settings › Section › Card".

- The CLI's `--filter` (`list`, `download`, `delete`, `queue add`) now takes
  the folder-rule pattern language instead of a single glob, so
  `--filter "*.mkv,*.mp4"` (the README's own example) matches either, case
  never matters, `-*sample*` leaves files out, and a word without `*` matches
  anywhere in the name. An unclosed quote or brace now stops the command
  instead of matching nothing (feature 020).
- A resumed download is checked by reading back only the part already on disk,
  then hashing the rest as it streams, instead of reading the whole file again
  after the download; `verify.passed` now comes just before `download.finished`.
- `bandwidth_limit_bytes_per_sec` is now the total for all downloads, shared
  by the workers, and a change applies to running downloads within about a
  second instead of from the next file. `bandwidth_limit_bytes_per_sec = 0` is
  refused; leave it out for unlimited. `download --bandwidth-limit` holds its
  limit for the run and ignores the schedule (feature 032).

- Only download and Never download now apply: files that don't match
  `whitelist_patterns`, or match `blacklist_patterns`, are blocked like a
  blocked type, logged with the pattern, listed under Recently blocked and
  deleted from Put.io under a rule with `delete_blocked`. Check a value you set
  before with **Preview patterns** in Settings › Download.
- `skip_existing` is replaced by `on_existing` and still read (`true` is
  resume, `false` overwrite). A file of the same size but a different
  checksum is no longer treated as downloaded (feature 040).
- The snap stores its config in `/var/snap/putio-sync/common/config.toml`,
  shared by the CLI (run with `sudo`) and the daemon. The armhf snap is no
  longer built.
