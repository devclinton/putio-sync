# Feature Specification: Update Checker and Self-Update

**Feature Branch**: `055-update-checker`

**Created**: 2026-11-10

**Status**: Draft

**Input**: ROADMAP entry "055 Update checker and self-update" (numbered 055 because
`specs/054-project-site` took 054): check GitHub Releases for a newer version on start and once
a day (can be turned off), and show it in the web UI with that release's notes, next to What's
new. A binary downloaded from GitHub updates itself (an "Update now" button, plus an opt-in
automatic update that waits until no transfer is running), using a small LZMA patch from the
running version when the release has one and the full archive otherwise, checking SHA-256,
swapping the binary, restarting, and rolling back if the new version fails to start. Managed
installs (snap, apt, Homebrew, winget, Flatpak, Docker, Unraid/CasaOS/Umbrel) never touch their
own files; they detect the channel and show the exact update command or step.

## Context

What 053 ships on each GitHub release (`devclinton/putio-sync`, first tagged as v0.2.0), which
this feature reads and adds to:

| Platform | Asset | Installs to |
|----------|-------|-------------|
| Linux x86_64 / aarch64 | `putio-sync-<ver>-linux-<x86_64\|aarch64>.tar.gz` (static musl binary, LICENSE, README, CHANGELOG) | wherever the user unpacks it |
| macOS arm64 | `putio-sync-<ver>-macos-arm64.tar.gz` | wherever the user unpacks it |
| macOS arm64 | `putio-sync-<ver>-macos-arm64.pkg` (unsigned until a Developer ID exists) | `/usr/local/bin/putio-sync`, launchd agent `com.devclinton.putio-sync` |
| Windows x64 / arm64 | `putio-sync-v<ver>-<x86_64\|aarch64>-pc-windows-msvc.zip` (winget installs this zip; the name keeps the leading `v`) | wherever the user unpacks it, or winget's folder |
| Windows x64 / arm64 | `putio-sync-<ver>-windows-<x64\|arm64>-setup.exe` (Inno Setup, admin, no Authenticode) | `Program Files\putio-sync`, a SYSTEM `schtasks` task or a Startup shortcut |
| Debian amd64 / arm64 | `putio-sync_<ver>_<amd64\|arm64>.deb` plus `.sha256` | `dpkg -i`; there is no apt repository |
| All | `SHA256SUMS` over every asset above | — |

Other channels: the snap `putio-sync` (strict confinement, amd64 and arm64), Homebrew tap
`devclinton/putio-sync`, winget `putio.putio-sync`, and container images at
`ghcr.io/devclinton/putio-sync` (`:latest`, `:<ver>` alpine; `:dhi`, `:dhi-<ver>`,
`:dhi-ffmpeg`, `:dev`, `:dev-ffmpeg` hardened variants built from `dhi.io` bases). Unraid,
CasaOS and Umbrel run that image, CasaOS and Umbrel pinned to a version. Flatpak was dropped in
052 and is not published, so it gets detection only. Nothing is code-signed today: no Apple
Developer ID and no Authenticode.

## Clarifications

### Session 2026-11-10

- Q: Which patch format should self-update use for the LZMA-compressed patches? → A: bsdiff
  binary diffs compressed with xz (LZMA2).
- Q: From how many earlier releases should each new release ship a patch? → A: the last 3
  releases; anything older downloads the full archive.
- Q: Is a SHA-256 check against `SHA256SUMS` enough, or should releases also be signed? → A:
  sign `SHA256SUMS` with a minisign key in the release workflow; the public key is built into
  the binary, and the updater refuses a list with a missing or bad signature.
- Q: Should the macOS `.pkg` and Windows `-setup.exe` installs update themselves too? → A: yes,
  by running the new installer: download it, check it against the signed list, and open it so
  the OS asks for admin rights once. No silent swap; rollback is the installer's job.
- Q: How should a container learn which image and tag to name in the `docker pull` command? →
  A: bake the image variant (`latest`, `dhi`, `dhi-ffmpeg`, `dev`, `dev-ffmpeg`) into each image
  at build time; the notice shows that variant's rolling tag, and the new version's tag for a
  version-pinned install (CasaOS, Umbrel).
- Settled without a question (defaults from the brief, open to change in review):
  - Check interval: first check about a minute after start, then every 24 hours; only on/off is
    configurable.
  - Replacing a running `.exe` on Windows: rename the running `putio-sync.exe` aside (Windows
    allows renaming a running file), write the new one under the original name, start it, and
    delete the old file on the next successful start. Linux and macOS replace the file with an
    atomic rename.
  - Channel detection: see FR-009.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - See that a newer version is out (Priority: P1)

A user opens the web UI and sees that a newer version is available: its number, date and
release notes, next to What's new. This works the same on every install type.

**Why this priority**: it is the base for the other stories, and the only part every channel
gets. Without it most users never learn that a fix is out.

**Independent Test**: point the checker at a mock releases endpoint that lists a newer version
and check that the notice appears with that release's notes. Point it at one listing the
running version and check that nothing appears.

**Acceptance Scenarios**:

1. **Given** the running version is 0.2.0 and the newest release is 0.3.0, **When** the daemon
   starts, **Then** the web UI shows "0.3.0 is available" with its release notes next to What's
   new.
2. **Given** the newest release is the running version, a draft or a pre-release, **When** the
   check runs, **Then** no notice is shown.
3. **Given** the user turned update checks off, **When** the daemon runs for days, **Then** it
   never contacts GitHub for releases.
4. **Given** the check failed (offline, rate limited), **When** the user opens the page,
   **Then** it shows when the last check succeeded and the error, and the rest of the app is
   unaffected.
5. **Given** the user dismisses the notice for 0.3.0, **When** 0.3.1 comes out, **Then** the
   notice comes back for 0.3.1.

---

### User Story 2 - Update a managed install with the right command (Priority: P2)

On a snap, .deb, Homebrew, winget, Docker, Unraid, CasaOS or Umbrel install, the notice says
how this install was set up and shows the exact command or step to update it, with a copy
button. putio-sync does not change its own files on these installs.

**Why this priority**: these channels are where most users run putio-sync (the NAS image above
all), and a wrong instruction is worse than none.

**Independent Test**: run the detection with each channel's markers faked (environment, path,
container files) and check the instruction shown for each one.

**Acceptance Scenarios**:

1. **Given** a snap install, **Then** the notice shows `sudo snap refresh putio-sync`.
2. **Given** a .deb install, **Then** the notice links the new `.deb` for this architecture and
   shows the `dpkg -i` line.
3. **Given** a Homebrew install, **Then** it shows `brew upgrade putio-sync`.
4. **Given** a winget install, **Then** it shows `winget upgrade putio.putio-sync`.
5. **Given** a container running the `dhi` image variant, **Then** it shows
   `docker pull ghcr.io/devclinton/putio-sync:dhi` and a reminder to recreate the container,
   and mentions `:dhi-<new version>` for installs that pin a version.
6. **Given** a container with no baked-in variant (an image built by someone else), **Then** it
   shows a plain warning to pull the newer image the way it was installed, with no guessed
   command.
7. **Given** CasaOS or Umbrel, which pin a version tag (`:0.2.0`), **Then** the shown tag is the
   new version's, not the old one.
8. **Given** Unraid, CasaOS or Umbrel, **Then** the notice names that platform's own update step
   (the app store's update button) before any docker command.

---

### User Story 3 - Update a GitHub binary with one click (Priority: P3)

A user who unpacked a release archive (Linux, macOS or Windows) clicks "Update now". putio-sync
downloads the update, checks it, replaces its own binary, restarts, and the UI comes back on the
new version. If the new version does not start, the old one comes back by itself.

**Why this priority**: it saves those users a manual download, but they are fewer than the
managed-channel users, and it is the riskiest part.

**Independent Test**: serve a fake release from a local server, run "Update now" against a
scratch install folder, and check the binary swap, the restart and the rollback when the new
binary is made to fail on start.

**Acceptance Scenarios**:

1. **Given** a newer release with a patch from the running version, **When** the user clicks
   "Update now", **Then** only the patch is downloaded, the rebuilt binary matches the release's
   SHA-256, and the daemon restarts on the new version.
2. **Given** a newer release with no patch from the running version, **Then** the full archive
   for this platform is downloaded and checked instead.
3. **Given** the downloaded file or the rebuilt binary fails its SHA-256 check, **Then** nothing
   is replaced and the UI shows the error.
4. **Given** the new binary exits or does not answer its health check within the start window,
   **Then** the previous binary is put back, restarted, and the UI says the update was rolled
   back and why.
5. **Given** a transfer is running when the user clicks "Update now", **Then** the UI says
   transfers will be interrupted and asks to confirm; downloads resume from their partial files
   after the restart.
6. **Given** the binary's folder is not writable by the daemon user, **Then** the button is
   replaced with the manual steps and the reason.
7. **Given** an install from the macOS `.pkg` or the Windows `-setup.exe`, **When** the user
   clicks "Update now", **Then** the new installer for this architecture is downloaded,
   checked, and opened, and the OS asks for admin rights; putio-sync does not replace its own
   files.

---

### User Story 4 - Update automatically (Priority: P4)

A user of a GitHub binary turns on "Install updates automatically". When a newer release is
found, putio-sync waits until no transfer is running, then updates as in Story 3.

**Why this priority**: a convenience on top of Story 3, off by default.

**Independent Test**: with the setting on and a transfer running, a newer release is found;
check that the update waits for the transfer to finish and then runs.

**Acceptance Scenarios**:

1. **Given** the setting is on and no transfer is running, **When** a newer release is found,
   **Then** the update starts without user action and is recorded in the log and on the page.
2. **Given** the setting is on and a transfer is running, **Then** the update waits and starts
   once the queue is idle.
3. **Given** an automatic update was rolled back, **Then** it does not retry the same version
   automatically; "Update now" still can.
4. **Given** a managed install, or a `.pkg` or `-setup.exe` install (which needs admin rights
   each time), **Then** the setting is not offered.

### Edge Cases

- A development build (no release version, or a version newer than any release) never shows
  an update notice.
- GitHub's unauthenticated API rate limit is hit: back off to the next scheduled check, and
  never retry in a tight loop.
- Several releases came out since the running one: the notice shows the newest, with the notes
  of each release in between.
- The machine sleeps through the daily check: it runs at the next wake, not a backlog of
  missed checks.
- A release is missing this platform's asset (a build failed): no self-update is offered for
  it; the notice still shows.
- The daemon is killed in the middle of a swap: on the next start it finds a complete binary
  (the old one or the new one), never a partial file.
- The API is behind a reverse proxy or the UI is opened from a phone: the notice and the button
  work the same; only authenticated users can start an update.
- Disk full while downloading: the update fails cleanly and the current binary is untouched.
- Unknown install type: treated as managed, showing a link to the release page and no button.

## Requirements *(mandatory)*

### Functional Requirements

**Checking**

- **FR-001**: The daemon MUST check the GitHub Releases of `devclinton/putio-sync` for the
  newest published, non-draft, non-pre-release version about a minute after start and then
  every 24 hours.
- **FR-002**: Update checks MUST be on by default and MUST be turnable off in Settings and in
  the TOML config (documented in `config.example.toml`); when off, the daemon MUST make no
  request to GitHub for releases.
- **FR-003**: The check MUST send no user data beyond what an HTTP request needs (a User-Agent
  with the version); no install id, no telemetry.
- **FR-004**: The result (newest version, its date, its notes, when the check ran, any error)
  MUST be kept so that a restart does not need a new request to show it.
- **FR-005**: Users MUST be able to run a check now from the UI.

**Showing**

- **FR-006**: The web UI MUST show an update notice next to What's new when a newer version is
  known, with the version, date and rendered release notes of every release since the running
  one, and a link to the release page.
- **FR-007**: Users MUST be able to dismiss the notice for one version; a later version shows it
  again.
- **FR-008**: The notice MUST state the detected install channel and what will happen ("updates
  itself" or "update with: …").

**Channel detection and instructions**

- **FR-009**: The daemon MUST detect its install channel at start, checked in this order, from
  the path of its own executable and its environment: snap (`SNAP` set), Flatpak (`FLATPAK_ID`
  set), container (the baked-in image variant, or `/.dockerenv` or a container cgroup), and
  within a container Unraid, CasaOS or Umbrel where their environment says so; Homebrew
  (executable under a Homebrew `Cellar`); .deb (`/usr/bin/putio-sync` listed by
  `/var/lib/dpkg/info/putio-sync.list`); winget (executable under winget's `Packages` folder);
  the macOS `.pkg` (`/usr/local/bin/putio-sync` with the `com.devclinton.putio-sync` package
  receipt); the Windows `-setup.exe` (its uninstall registry entry for the 053 `AppId`); and
  otherwise a GitHub archive. The plan's research confirms each marker on a real install.
- **FR-010**: For every managed channel the daemon MUST NOT modify, download into or replace
  its own install files, and MUST show that channel's update command or step.
- **FR-011**: Each container image MUST carry its variant (`latest`, `dhi`, `dhi-ffmpeg`, `dev`,
  `dev-ffmpeg`), set at build time. The daemon MUST show `docker pull` with that variant's
  rolling tag, and the new version's tag as the alternative for pinned installs (the only form
  for CasaOS and Umbrel). With no variant, it MUST show a plain warning with no guessed
  command. It MUST NOT read the Docker socket.
- **FR-012**: The detected channel MUST be visible in the doctor or About output so a wrong
  detection can be reported.

**Self-update (GitHub archive installs only)**

- **FR-013**: For a GitHub archive install, the UI MUST offer "Update now" to authenticated
  users. For a `.pkg` or `-setup.exe` install, "Update now" MUST download the new installer for
  this architecture, verify it as in FR-015, and open it; it MUST NOT replace files itself.
- **FR-014**: The updater MUST download a patch (bsdiff, xz-compressed) from the running version
  to the new one when the release has one for this platform, and otherwise the full archive for
  this platform and architecture.
- **FR-015**: The updater MUST verify the minisign signature on the release's `SHA256SUMS`
  against the public key built into the running binary, then check the SHA-256 of what it
  downloaded against that list and the SHA-256 of the resulting binary against the listed
  binary hash. A missing or bad signature, or any mismatch, MUST stop the update with no change.
- **FR-016**: The updater MUST keep the previous binary, put the new one in place so that the
  install folder always holds one complete binary, and restart the daemon on the new binary.
- **FR-017**: After the restart the new version MUST report healthy within a start window, or
  the previous binary MUST be restored and started, and the UI MUST say the update was rolled
  back and why.
- **FR-018**: The updater MUST work while the binary is running: on Linux and macOS by writing
  the new binary next to the old one and renaming it into place; on Windows by renaming the
  running `putio-sync.exe` aside, writing the new one under the original name, and deleting the
  old file after the new version's first healthy start.
- **FR-019**: When transfers are running, a manual update MUST warn and ask to confirm; an
  automatic one MUST wait until no transfer is running.
- **FR-020**: An opt-in setting "Install updates automatically" (off by default) MUST start an
  update when a newer version is found and the queue is idle. A version that was rolled back
  MUST NOT be retried automatically.
- **FR-021**: Each check, update, verification failure and rollback MUST be logged with
  structured fields and counted in Prometheus metrics with bounded labels.

**Release pipeline**

- **FR-022**: The release workflow MUST build, for each archive target (Linux x86_64 and
  aarch64, macOS arm64, Windows x64 and arm64), bsdiff + xz patches from each of the last 3
  releases to the new one; list them and the bare binary hashes in `SHA256SUMS`; and publish a
  minisign signature of `SHA256SUMS` next to it. It MUST NOT rename any asset 053 already
  publishes (winget and the docs depend on those names). The signing key is a repository
  secret; without it the workflow fails rather than publish an unsigned list.

### Key Entities

- **Release info**: version, publish date, notes, page link, assets with their SHA-256; the
  newest one and those since the running version.
- **Update check state**: last attempt, last success, last error, dismissed version.
- **Install channel**: one of GitHub archive, macOS pkg, Windows installer, snap, deb, Homebrew,
  winget, Flatpak, container (with image and tag when known), Unraid, CasaOS, Umbrel, unknown;
  plus whether it can update itself.
- **Update attempt**: from and to version, patch or full, state (downloading, verifying,
  swapping, restarting, done, failed, rolled back), error.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Within one day of a release, a running instance with checks on shows the notice,
  with no user action.
- **SC-002**: Every channel in the Context table gets either a working one-click update or an
  instruction that, copied as is, updates that install; checked once by hand per channel.
- **SC-003**: A patch update downloads at most a quarter of the full archive's size for a
  one-release step.
- **SC-004**: A one-click update, from click to the UI back on the new version, takes under one
  minute on a home connection.
- **SC-005**: In tests, a new binary that fails to start is rolled back in every case, and a
  file that fails its checksum never replaces the running binary.
- **SC-006**: With checks off, the daemon makes zero requests to GitHub's release API.

## Assumptions

- GitHub Releases is the only source of versions; release notes come from the release body.
  The repository is public, so unauthenticated requests work (60 per hour per IP is far more
  than one a day).
- "Transfer running" means anything in the download queue is active; partial downloads resume
  after a restart as they already do (Principle IV).
- The restart reuses the way the daemon is already started (service manager, launchd agent,
  scheduled task, or a plain terminal), so after a swap it re-executes itself in place.
- macOS archives are unsigned: replacing the binary in place does not add a quarantine flag,
  because the daemon writes the file itself.
- Intel macOS has no release asset (053 builds arm64 only), so it gets no self-update.
- Releases made before this feature (v0.2.0 and earlier) have no signature, so a version
  running 055 can only self-update to a release that has one. The first release with 055 is
  installed by hand.
- Out of scope: Apple Developer ID and Authenticode signing (still waiting on certificates, as
  in 053), rotating the minisign key, updating the Lua/Wasm plugins, downgrades, and
  pre-release channels.
