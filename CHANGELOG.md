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

- Empty-folder cleanup now removes nested empty folders on Put.io. Put.io
  marks folders only by their type, so every folder was read as a file and
  kept. The doctor's empty-folder and duplicate checks and recursive delete
  had the same problem.
- Interrupted downloads resume, download settings from the config are
  honoured, and failed transfers can be retried (feature 004).
- Problems seen in the live NAS logs (feature 002).
- The Docker image builds again with the Radarr/Sonarr and doctor crates.

### Changed

- The snap stores its config in `/var/snap/putio-sync/common/config.toml`,
  shared by the CLI (run with `sudo`) and the daemon. The armhf snap is no
  longer built.
