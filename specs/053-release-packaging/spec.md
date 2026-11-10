# Feature Specification: Release Binaries, Installers and Hardened Images

**Feature Branch**: `053-release-packaging`

**Created**: 2026-11-09

**Status**: Draft (workflows written and checked locally; GitHub Actions can't run on the
private repository yet, so no workflow has run end to end)

**Input**: "Extend putio-sync's release packaging": standalone binaries for macOS arm64,
Linux x86_64/arm64 and Windows x86_64/arm64, a Windows installer, a macOS installer with a
launchd option, checksums on GitHub releases, an arm64 snap, and Docker Hardened Images
(dhi.io) images. Covers the two "Queued: packaging" items in ROADMAP.md.

## Context

- Releases so far had Windows zips, `.deb` files, a Homebrew source formula and an amd64
  snap. `release.yml` called the per-platform workflows as reusable workflows, but none of
  them declared `workflow_call`, so a tag push could not run them. Their `release:
  published` triggers never fired either, because a release created with `GITHUB_TOKEN`
  starts no workflows.
- Linux release builds used glibc and OpenSSL (reqwest's default native-tls). SQLite
  (sqlx `bundled`), Lua (mlua `vendored`) and ring compile from vendored C, so OpenSSL was
  the only system library in the way of a static binary.
- The NAS pulls `ghcr.io/devclinton/putio-sync:main` (the alpine image with ffmpeg). The
  maintainer chose to keep that image and its tags as they are, and to publish the dhi.io
  images under new tag prefixes, with and without ffmpeg.
- No Apple Developer ID: the macOS package ships unsigned and the README explains the
  Gatekeeper step. Signing turns on when the secrets are added.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Download a Linux binary that runs anywhere (Priority: P1)

A Linux user on any distribution, or on a minimal host with no libc packages, downloads the
tarball for their architecture, extracts it and runs `putio-sync`.

**Independent Test**: `file putio-sync` says "statically linked", it runs in an empty
`scratch`-like container, and the daemon serves the embedded web UI.

**Acceptance Scenarios**:

1. **Given** the x86_64 or aarch64 tarball, **When** the binary runs on a host without
   OpenSSL, **Then** it starts and reaches Put.io over TLS.
2. **Given** no config, **When** `putio-sync daemon` starts, **Then** the web UI on port
   15432 serves the setup wizard from the binary itself.
3. **Given** subtitle transcription is on and ffmpeg is missing, **Then** the existing
   "ffmpeg not found" error explains how to install it. ffmpeg stays optional.

### User Story 2 - Install on Windows (Priority: P1)

A Windows user runs `putio-sync-<ver>-windows-<x64|arm64>-setup.exe`, picks how it runs,
and opens the web UI from the Start menu.

**Acceptance Scenarios**:

1. **Given** the default choice, **When** setup finishes, **Then** a `putio-sync` scheduled
   task runs the daemon as SYSTEM at startup (config and log in `%ProgramData%\putio-sync`),
   and it is running.
2. **Given** "Start when I sign in", **Then** a Startup shortcut runs the daemon minimized
   with the user's `%USERPROFILE%\.putiosync` config.
3. **Given** any choice, **Then** the Start menu has "Put.io Sync Web UI" opening
   `http://localhost:15432/`, and uninstalling removes the task, the shortcut and the PATH
   entry.

### User Story 3 - Install on macOS (Priority: P2)

A Mac user opens `putio-sync-<ver>-macos-arm64.pkg`, gets past Gatekeeper once, and the
daemon starts at login.

**Acceptance Scenarios**:

1. **Given** the default choices, **Then** `/usr/local/bin/putio-sync` exists, a LaunchAgent
   starts the daemon for the logged-in user now and at every login, and its log is
   `~/Library/Logs/putio-sync.log`.
2. **Given** "Start at login" is unchecked under Customize, **Then** only the binary and the
   web UI link are installed.
3. `sudo /usr/local/share/putio-sync/uninstall.sh` removes everything the package added.

### User Story 4 - Verify downloads (Priority: P2)

Every release asset is listed in `SHA256SUMS` on the release; `sha256sum -c` checks them.

### User Story 5 - arm64 snap (Priority: P2)

`sudo snap install putio-sync` works on a Raspberry Pi or arm64 server; the snap job builds
and tests amd64 and arm64 on native runners and publishes both.

### User Story 6 - Hardened container images (Priority: P3)

A security-minded user pulls `ghcr.io/devclinton/putio-sync:dhi-<ver>` (no shell, no
package manager, distroless static base) or `:dhi-ffmpeg-<ver>` for subtitles; someone
debugging pulls `:dev-<ver>` or `:dev-ffmpeg-<ver>`, which have a shell and apk.

**Acceptance Scenarios**:

1. All four run as 99:100 with the same volumes, ports and command as the alpine image, so
   switching an Unraid template is a tag change.
2. `:main`, `:latest` and `:<ver>` remain the alpine image the NAS deploys from.

## Requirements

- **FR-001**: Linux binaries are static musl executables with no `NEEDED` entries; TLS is
  rustls with Mozilla's roots plus the OS store when there is one.
- **FR-002**: The web UI stays embedded (`include_bytes!`), so no binary needs files beside it.
- **FR-003**: One release entry point: a `v*` tag runs `release.yml`, which drafts the
  release, uploads every asset, publishes it, then runs Homebrew and winget.
- **FR-004**: Every reusable workflow can also run by hand as a dry run that keeps its
  output as workflow artifacts and touches no release, store or registry.
- **FR-005**: The Windows zips keep their names (winget's manifest installs them).
- **FR-006**: Nothing the NAS deploys from changes.

## Out of scope

- Flatpak and AUR (another session; 052 dropped Flathub). `release.yml` no longer calls
  `flatpak.yml`, which still runs by hand.
- Apple Developer ID signing and notarization (no certificate yet), Authenticode signing.
- Switching winget from the zip to the Inno Setup installer.
- macOS x86_64.
