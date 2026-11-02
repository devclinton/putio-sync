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

- An Active page listing every transfer on its way to the NAS, once each, in
  its Put.io or NAS stage, with counts by state that filter the list. The
  Status page's Active transfers card summarises it, and the
  `putio_sync_pipeline_items` metric counts items by stage and state
  (feature 039).
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

- A fake release rejected right after Radarr or Sonarr grabbed it is now
  blocklisted. Before, putio-sync asked before the *arr had listed the item in
  its queue, gave up, and left it stuck as "downloading" with no new search.
  It now looks again for up to 10 minutes.
- The Radarr / Sonarr settings card no longer shows a stray "null" under its
  title.
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

- `skip_existing` is replaced by `on_existing` and still read (`true` is
  resume, `false` overwrite). A file of the same size but a different
  checksum is no longer treated as downloaded (feature 040).
- The snap stores its config in `/var/snap/putio-sync/common/config.toml`,
  shared by the CLI (run with `sudo`) and the daemon. The armhf snap is no
  longer built.
