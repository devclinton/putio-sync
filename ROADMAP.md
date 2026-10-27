# Roadmap

Features are built with Spec Kit, one numbered feature per branch under
`specs/`. Shipped changes are in [CHANGELOG.md](CHANGELOG.md). This file lists
what is queued, in the order planned. Each entry is the starting brief for
`/speckit-specify`.

## In progress

| # | Feature | State |
|---|---------|-------|
| 010 | Put.io doctor: health checks and fixes | Branch `010-putio-doctor` |

## Queued: onboarding and setup

### 013 First-run setup wizard

When the daemon has no Put.io token or no usable download folder, the web UI
opens a step-by-step setup instead of the dashboard:

1. Connect Put.io (OAuth, without copying a token by hand).
2. Choose the download folder, with checks that it is writable and how much
   space is free.
3. Choose what to download, using the folder rule presets from 009.
4. Optional: a watch folder, Radarr/Sonarr, metrics.
5. A dry-run sync that shows what would be downloaded before anything starts.

The CLI gets the same flow as `putio-sync setup`, for headless installs. It
works the same in Docker, the snap and a plain binary.

### 014 Guided tour and in-app help

A short, skippable tour of each page on first visit, re-runnable from
Settings. Help panels and tooltips explain each setting in plain words, with
links to the guides from 016.

### 015 Configuration assistant

Settings explain what each option does and what it affects. They validate
values before saving, and preview the change against the current config.
They warn about risky combinations (for example deleting from Put.io after
download with no local copy). They can also reset a section to defaults and
import or export the config.

### 016 Onboarding guides

Getting-started guides for each way to run putio-sync (Unraid/Docker, snap,
Homebrew, binary). Task guides for Radarr/Sonarr, folder rules, watch
folders, metrics and Grafana, and troubleshooting with the doctor. They live
in `docs/` and are linked from the UI.

### 017 What's new

After an upgrade, the web UI shows the CHANGELOG.md entries since the version
the user last saw. The changelog is built into the binary.

## Queued: packaging

- **Minimal runtime image** on Docker Hardened Images (dhi.io), plus a
  separate dev image. Settle which registry the NAS uses first.
- **arm64 snap**: build on an arm64 runner or through Snapcraft's build
  service; CI builds amd64 only today.
- **Flathub**: needs a public repository and probably a desktop wrapper
  around the web UI.
- **Homebrew workflow**: `.github/workflows/homebrew.yml` is not valid YAML
  and fails on every push.
