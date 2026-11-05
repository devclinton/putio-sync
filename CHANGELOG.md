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
