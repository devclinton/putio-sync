# Research: Update Checker and Self-Update (055)

Decisions behind [plan.md](plan.md). The clarify answers in [spec.md](spec.md#clarifications)
are taken as given; this file settles how to carry them out.

## R1. Where the version comes from

- **Decision**: `GET https://api.github.com/repos/devclinton/putio-sync/releases?per_page=20`,
  unauthenticated, with `User-Agent: putio-sync/<version>` and `If-None-Match` from the last
  answer (a 304 doesn't count against the rate limit). Drafts and pre-releases are skipped.
  Versions are `vX.Y.Z` tags compared as three integers by a ~20-line parser; a tag that doesn't
  parse is ignored.
- **Rationale**: one request returns the newest release and the ones in between (the spec shows
  every release since the running one). The list endpoint costs no more than `/releases/latest`,
  and the repository is public, so 60 requests an hour per IP is plenty.
- **Alternatives**: `/releases/latest` (gives only one release, so in-between notes would need
  more calls); the `semver` crate (pre-releases are skipped, so plain X.Y.Z is all we parse,
  which isn't worth a dependency, Principle VII); an Atom feed (no assets).

## R2. Release notes

- **Decision**: the release body is the CHANGELOG section, because `release.yml` writes it with
  `scripts/changelog-section.sh`. The API puts the body under a `## [<ver>] - <date>` heading
  and runs it through the existing `changelog::parse`, so the UI renders it with the What's new
  code (`ui/pages/news.js`, same inline-markdown subset, same `ReleaseView` shape).
- **Rationale**: no markdown dependency and no second renderer. The notes look like What's new.
- **Alternatives**: rendering GitHub's markdown to HTML (a new dependency, and HTML from the
  network in the UI).

## R3. Patch format and tooling

- **Decision**: bsdiff patches (crate `bsdiff` 0.2, pure Rust, BSD-2) compressed with xz.
  Patches are applied at runtime with `lzma-rs` (pure Rust xz decoder) and `bsdiff::patch`. CI
  builds them with a small example program in the new crate
  (`cargo run -p putio-sync-update --example make-patch -- old new out.bsdiff`), then
  `xz -9e --check=crc64`.
- **Rationale**: building and applying the patch with the same crate rules out format drift.
  Both runtime crates are pure Rust, so the static musl, macOS and Windows builds need no C
  library (liblzma). lzma-rs decodes everything standard `xz` writes.
- **Alternatives**: `xz2`/`liblzma` crates (C liblzma, which complicates the static musl build
  for no runtime gain, since we only decode); `qbsdiff` (bsdiff-4 format with bzip2 framing, and
  we want xz); xdelta3 (C).

## R4. Full download when there is no patch

- **Decision**: the patch job also publishes a bare, xz-compressed binary per target,
  `putio-sync-<ver>-<target>.xz`, which the updater uses when there is no patch from the running
  version. The existing `.tar.gz` and `.zip` archives stay as they are, for people and for
  winget.
- **Rationale**: the updater then needs only an xz decoder. Unpacking archives would add the
  `tar` and `zip` crates (Principle VII) to get one file out. The download is about the size of
  the archive.
- **Deviation from the brief**: the brief says "falls back to the full archive". This takes the
  same full download in a form that is simpler to apply. Reversible: the updater could unpack
  the archive instead, at the cost of two crates.

## R5. Patch set and naming

- **Decision**: for each of the 5 archive targets (`linux-x86_64`, `linux-aarch64`,
  `macos-arm64`, `windows-x64`, `windows-arm64`) and each of the 3 newest earlier releases that
  have a bare binary for that target: `putio-sync-<from>-to-<to>-<target>.bsdiff.xz`. The
  previous binaries come from the previous releases' `.xz` assets, or from their archives
  for releases older than 055. At most 15 patches per release.
- **Rationale**: the clarify answer (last 3). Each name holds both versions, so the updater can
  find its patch without an index file.
- **Alternatives**: a JSON manifest (one more file to sign; `SHA256SUMS` already lists every
  asset).

## R6. Signing

- **Decision**: minisign. The workflow signs `SHA256SUMS` with the secret key in
  `MINISIGN_SECRET_KEY` (and `MINISIGN_PASSWORD`), using the `minisign` CLI from Ubuntu's
  archive, and uploads `SHA256SUMS.minisig`. The public key is a constant in
  `putio-sync-update/src/verify.rs`. Runtime verification uses `minisign-verify` (pure Rust, no
  dependencies). `SHA256SUMS` gains a line for each patch, each `.xz` full binary, and each
  uncompressed binary (`putio-sync-<ver>-<target>.bin`), which is the hash of the result.
- **Rationale**: the clarify answer. minisign keys are small and the verifier is tiny. Signing
  one file covers every asset. The workflow stops when the key is missing, so a release is never
  published unsigned.
- **Key rotation**: out of scope (spec). A new key would ship in a release signed with the old
  one.

## R7. Replacing the running binary and restarting

- **Decision**:
  - **Unix**: write `putio-sync.new` in the same folder (same filesystem), fsync, `chmod` to
    match, `rename(current → putio-sync.old)`, then `rename(new → current)`. Shut the daemon's
    services down the way Ctrl+C does, then `exec(2)` the new binary with the same arguments and
    environment (`std::os::unix::process::CommandExt::exec`). The PID stays the same, so systemd,
    launchd or a terminal sees the same process.
  - **Windows**: `MoveFileEx(putio-sync.exe → putio-sync.old.exe)` (allowed on a running
    image), then write the new file under the original name. Shut down, spawn the new binary
    with the same arguments, detached (`DETACHED_PROCESS`), and exit 0. The `.old.exe` is
    deleted after the first healthy start (FR-018).
- **Rationale**: `exec` is the only restart that works under every Unix supervisor without
  knowing which one it is. On Windows there is no exec, and the zip and winget installs run from
  a terminal or the Startup shortcut, so a detached spawn is the equivalent.
- **Alternatives**: asking the service manager to restart (needs to know which manager, and
  privileges); a helper process (an extra binary to ship).

## R8. Rollback

- **Decision**: before the swap the old process writes `update-pending.json` in the config
  folder: `{from, to, old_path, started_at, starts: 0}`. It also copies the SQLite database
  with `VACUUM INTO 'putio-sync.db.pre-<to>'`. At start, before anything else, the binary reads
  the marker:
  - **Own version == `to`, `starts == 0`**: set `starts = 1` and enter trial mode. It has
    90 seconds to become healthy (database open, daemon initialized, API listener bound). If it
    gets there, delete the marker and the `.old` binary, and keep the database copy until the
    next update. A setup error, a panic (panic hook) or the deadline triggers rollback.
  - **Own version == `to`, `starts >= 1`**: the last trial died without cleaning up (crash,
    kill, power loss), and a supervisor or the user started it again. Roll back.
  - **Rollback**: move `.old` back to the original name, restore the database copy, rewrite
    the marker as `{rolled_back: to, reason}`, and exec (Unix) or spawn (Windows) the old binary.
    The old binary shows the reason and won't install `to` automatically again (FR-020).
- **Rationale**: this covers crashes at start under a supervisor (the next start sees
  `starts >= 1`) and without one (the next manual start). The database copy covers a new
  version that changes the schema. Today's migrations only add tables and columns, but that is
  a convention, not a guarantee.
- **Alternatives**: a watchdog parent (systemd and launchd kill the group when the main PID
  exits).

## R9. The `.pkg` and `-setup.exe` installs

- **Decision**: "Update now" downloads `putio-sync-<ver>-macos-arm64.pkg` or
  `putio-sync-<ver>-windows-<arch>-setup.exe` into the config folder's `updates/`, verifies it
  against the signed `SHA256SUMS`, and opens it: `open <pkg>` on macOS,
  `ShellExecuteW("runas", setup.exe)` on Windows. That only works when the daemon runs in the
  user's desktop session. Otherwise (the setup.exe "service" task runs as SYSTEM) the UI shows
  the downloaded file's path and the step instead. No automatic install (spec US4 scenario 4).
- **Rationale**: the clarify answer. The installers own their files and their service setup.
- **Detecting a desktop session**: Windows, `WTSGetActiveConsoleSessionId` equals our session;
  macOS, launched by the user's launchd agent (`XPC_SERVICE_NAME` is
  `com.devclinton.putio-sync`) and not as root.

## R10. Channel detection

Checked in order. The first match wins. Markers that the plan's tasks confirm on a real install
are marked ✱.

| Channel | Marker | Instruction |
|---------|--------|-------------|
| snap | `SNAP` and `SNAP_NAME=putio-sync` | `sudo snap refresh putio-sync` (snapd also refreshes it by itself) |
| Flatpak | `FLATPAK_ID` | `flatpak update com.putio.PutioSync` (not published, see 052) |
| Unraid | container and `HOST_OS=Unraid` (set by Unraid's Docker manager) ✱ | Docker tab → "Check for updates" → "apply update" |
| CasaOS | container and `PUTIO_SYNC_INSTALL=casaos` (added to our compose file) | the app's update in the CasaOS dashboard, or bump the tag to the new version |
| Umbrel | container and `PUTIO_SYNC_INSTALL=umbrel` (added to our compose file) | update from the Umbrel App Store |
| container | `PUTIO_SYNC_IMAGE_VARIANT` set, or `/.dockerenv`, or `/proc/1/cgroup` naming docker, containerd or kubepods | `docker pull <image>:<variant>` and recreate, or `:<variant-prefix><new ver>` if pinned. No variant: plain warning |
| Homebrew | canonical exe path contains `/Cellar/putio-sync/` | `brew upgrade putio-sync` |
| .deb | exe is `/usr/bin/putio-sync` and listed in `/var/lib/dpkg/info/putio-sync.list` | download link to `putio-sync_<ver>_<arch>.deb` and `sudo dpkg -i …` |
| winget | exe path under `%LOCALAPPDATA%\Microsoft\WinGet\Packages\putio.putio-sync_` ✱ | `winget upgrade putio.putio-sync` |
| macOS pkg | exe is `/usr/local/bin/putio-sync` and `pkgutil --pkg-info com.devclinton.putio-sync` succeeds (or the receipt plist exists in `/var/db/receipts/`) | installer flow (R9) |
| Windows setup | uninstall key `HKLM\…\Uninstall\{6F0B8D2A-6C1E-4F57-9C38-5B1D2E7A4C90}_is1` with `InstallLocation` equal to the exe's folder | installer flow (R9) |
| GitHub archive | none of the above, and the version is a release build (`PUTIO_SYNC_GIT_SHA` set) | self-update |
| unknown | a dev build (`cargo run`, no `PUTIO_SYNC_GIT_SHA`) | link to the release page, no button |

- **Image variant**: `Dockerfile` sets `ENV PUTIO_SYNC_IMAGE_VARIANT=latest`. Each `Dockerfile.dhi`
  stage sets its own (`dhi`, `dhi-ffmpeg`, `dev`, `dev-ffmpeg`). `PUTIO_SYNC_IMAGE` is
  set from a build argument, default `ghcr.io/devclinton/putio-sync`. The versioned tag for
  variant `v` is `<ver>` for `latest` and `<v>-<ver>` for the others (see `docker-dhi.yml`
  `prefix`).
- Detection runs once at start and needs no network. `PUTIO_SYNC_INSTALL` also works as a
  manual override for any channel, for installs the markers get wrong.

## R11. "No transfer running"

- **Decision**: idle means `DownloadManager::active_workers() == 0` and no pipeline step is
  running. An automatic update re-checks every minute while it waits. A manual one with work
  running answers `409 busy` unless the request says `interrupt: true`. Partial downloads resume
  after the restart (Principle IV).

## R12. Where the code lives

- **Decision**: a new crate, `putio-sync-update`, owns checking, channel detection, download,
  verification, patching, swap, trial and rollback. Its dependencies: `core` (config), `telemetry`
  (metrics), `reqwest`, `sha2`, `bsdiff`, `lzma-rs`, `minisign-verify`, and `windows-sys` on
  Windows. Check state goes in a `db` repository (`updates()`), like `whats_new()`. `api` gets
  `routes/system/update.rs`. `cli` wires the service in `commands/daemon.rs` and calls the trial
  check first thing in `main`.
- **Rationale**: Principle I. Nothing in the update crate depends on `api`, `daemon` or `cli`.

## Deviations

What shipped differs from the plan above in these ways.

- **Feature number**: the roadmap brief said 054. It became 055, a number the plugin workbench
  also uses, so `ROADMAP.md` has two "055" entries. The spec folder name tells them apart.
- **Delivery**: the work landed in several PRs instead of one per story: #6 and #9 (check,
  channels, release notes), #23 (Update now, with the Phase 5 release pipeline that started
  in #21), #34 (automatic updates) and #35 (polish). #21 was closed after #23 took its work.
- **Signing key (R6)**: `RELEASE_PUBKEY` is empty and the `MINISIGN_SECRET_KEY` and
  `MINISIGN_PASSWORD` secrets are not set. Until the maintainer makes the key, every
  self-update fails the signature check and `release-binaries.yml` stops at "Sign
  SHA256SUMS". The code pointed to a MAINTAINING.md heading that doesn't exist; it now names
  "MINISIGN_SECRET_KEY and MINISIGN_PASSWORD".
- **Versions (R5)**: the asset scripts (`release-unpack.sh`, `release-patches.sh`) accept only
  plain `X.Y.Z` versions. A tag like `v0.4.1-test` builds, but the patch step finds no
  binaries and fails. Pre-release tags are not supported.
- **Windows test**: the full OS matrix exposed a flaky `a_missing_database_is_exit_3`
  (`putio-sync-cli/tests/archive.rs`, Windows OS error 32 on deleting a just-closed database).
  #28 fixed it by never creating the database; #34 and #35 needed that fix before they passed.
- **Compose check (T048)**: `docker compose config` on the Umbrel file fails because its
  `app_proxy` service comes from Umbrel itself. That was already true before 055.
