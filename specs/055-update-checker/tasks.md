# Tasks: 055 Update Checker and Self-Update

**Input**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md),
[data-model.md](data-model.md), [contracts/api.md](contracts/api.md),
[contracts/release-assets.md](contracts/release-assets.md), [quickstart.md](quickstart.md)

**Tests**: included (Constitution III: no real GitHub, no network). Write each test task first
and see it fail. GitHub is a `mockito` server; binaries in swap tests are small scripts in a
`tempfile` folder that print a version.

**Rule**: a release build can never be pointed at another releases server or public key.
`PUTIO_SYNC_UPDATE_API`, `PUTIO_SYNC_UPDATE_PUBKEY` and `PUTIO_SYNC_TEST_FAIL_START` exist only
under `cfg(debug_assertions)` (plan, Constitution Check).

## Phase 1: Setup

- [X] T001 Create the crate `putio-sync-update/` (`Cargo.toml` with `version.workspace`, `edition.workspace`; deps `putio-sync-core`, `putio-sync-telemetry`, `reqwest` (workspace), `tokio` (workspace), `serde`, `serde_json` (workspace), `sha2 = "0.10"`, `tracing`, `bsdiff = "0.2"`, `lzma-rs = "0.3"`, `minisign-verify = "0.2"`, and `[target.'cfg(windows)'.dependencies] windows-sys` with `Win32_Storage_FileSystem`, `Win32_UI_Shell`, `Win32_System_RemoteDesktop`, `Win32_System_Threading`; dev-deps `mockito = "1.2"`, `tempfile = "3.8"`, `tokio` with `test-util`), `src/lib.rs` with empty modules `version`, `github`, `channel`, `verify`, `fetch`, `swap`, `trial`, `installer`, and add it to `members` in the root `Cargo.toml`
- [X] T002 [P] Add `UpdatesConfig { check: bool = true, auto_install: bool = false }` as `[updates]` on `Config` in `putio-sync-core/src/config.rs` (serde default, skip-serializing defaults like the other sections so an existing config round-trips unchanged), with a test that a config without `[updates]` loads, has `check = true`, and serializes byte-identical; document both keys in `config.example.toml`
- [X] T003 [P] Add `putio-sync-telemetry/src/update.rs` with `putio_sync_update_checks_total{result="ok|not_modified|error|rate_limited"}`, `putio_sync_update_available` (gauge 0/1), `putio_sync_update_attempts_total{kind="patch|full|installer", result="done|failed|rolled_back"}`, `putio_sync_update_verify_failures_total{stage="signature|download|binary"}`, following the label-enum pattern in `putio-sync-telemetry/src/labels.rs`, and register them in `lib.rs`

## Phase 2: Foundational

- [X] T004 [P] Write failing tests, then implement `Version` in `putio-sync-update/src/version.rs`: parses `v1.2.3` and `1.2.3`, rejects `1.2`, `1.2.3-rc1`, `1.2.3.4` and empty; orders numerically (`0.10.0 > 0.9.9`); `Display` without the `v`; `current()` from `env!("CARGO_PKG_VERSION")`
- [X] T005 [P] Write failing tests, then implement `Target` in `putio-sync-update/src/fetch.rs`: `Target::current()` gives `linux-x86_64`, `linux-aarch64`, `macos-arm64`, `windows-x64` or `windows-arm64` from `cfg!(target_os/target_arch)` and `None` on anything else (Intel macOS included); asset name helpers `full(ver) = "putio-sync-<ver>-<target>.xz"`, `patch(from, to) = "putio-sync-<from>-to-<to>-<target>.bsdiff.xz"`, `bin(ver) = "putio-sync-<ver>-<target>.bin"` (contracts/release-assets.md)
- [X] T006 Add the `update_state` table to `putio-sync-db/src/schema.rs` (`CREATE TABLE IF NOT EXISTS update_state (id INTEGER PRIMARY KEY CHECK (id = 1), last_attempt_at TEXT, last_success_at TEXT, last_error TEXT, etag TEXT, releases_json TEXT, dismissed_version TEXT, last_rolled_back TEXT, rollback_reason TEXT)`), create it in `migrate()` in `putio-sync-db/src/lib.rs`, and add `putio-sync-db/src/updates.rs` with `Database::updates()` → `get()`, `record_check(attempt, success, error, etag, releases_json)`, `dismiss(version)`, `record_rollback(version, reason)`, `clear_rollback_message()`; tests on a temp database for each, including that a row survives reopening

**Checkpoint**: the crate builds, config and table exist, `make ci` passes.

## Phase 3: US1 See that a newer version is out (P1) 🎯 MVP

**Goal**: the notice with release notes on every install type.
**Independent test**: a mock releases endpoint listing a newer version shows the notice; one listing the running version shows nothing (spec US1).

- [X] T007 [P] [US1] Write failing tests in `putio-sync-update/src/github.rs` against mockito: parses the releases list (`tag_name`, `published_at`, `html_url`, `body`, `assets[].name`); skips `draft` and `prerelease` and unparsable tags; returns the releases newer than the running version, newest first; sends `User-Agent: putio-sync/<version>` and no other identifying header (FR-003); sends `If-None-Match` and treats `304` as "not modified"; maps `403`/`429` with `x-ratelimit-remaining: 0` to `RateLimited`; times out after 30 s
- [X] T008 [US1] Implement `github::fetch_releases(base_url, etag, current)` in `putio-sync-update/src/github.rs` to pass T007. `base_url` is `https://api.github.com/repos/devclinton/putio-sync`, overridable by `PUTIO_SYNC_UPDATE_API` only under `cfg(debug_assertions)`
- [X] T009 [US1] Write failing tests, then implement `UpdateService` in `putio-sync-update/src/lib.rs`: `new(config, db, channel)`, `start()` spawns a task that checks 60 s after start and then every 24 h, measured on the wall clock so a sleep-through runs one check at wake and not a backlog (spec Edge Cases); skips every request when `updates.check` is false (FR-002, SC-006) and picks up a config change at the next tick; `check_now()` returns `Err(CheckedRecently)` within 60 s of the last one; stores each result through `db.updates().record_check` (FR-004) and loads it at start; sets the `putio_sync_update_available` gauge and counts `putio_sync_update_checks_total`; `snapshot()` returns the state for the API. Use `tokio::time::pause` in the tests
- [X] T010 [US1] Make the release-notes view: in `putio-sync-api/src/changelog.rs` add `pub fn parse_release(version, date, body) -> Release` that wraps the body as `## [<version>] - <date>` and reuses `parse`; tests with a body as `scripts/changelog-section.sh` writes it, and with an empty body (one release with no kinds)
- [X] T011 [US1] Write failing route tests in `putio-sync-api/tests/update.rs`, then add `putio-sync-api/src/routes/system/update.rs` with `GET /update`, `POST /update/check` and `POST /update/dismiss` exactly as in contracts/api.md (body shape, `409 checked_recently`, `204` on dismiss, `{"rolled_back": true}` clears the message), mount it with `.nest("/update", …)` next to `/whats-new` in `putio-sync-api/src/lib.rs`, add `with_updates(Arc<UpdateService>)` to `AppState`, and check that every route answers `401` without auth
- [X] T012 [US1] Wire it into `putio-sync-cli/src/commands/daemon.rs`: build `UpdateService` after the database is ready, `start()` it, pass it to `AppState`. Skip it in setup mode (`putio-sync-cli/src/commands/setup_mode.rs` doesn't start it)
- [X] T013 [US1] UI: in `putio-sync-api/ui/pages/news.js` fetch `GET /update` and, when `available` is set and not dismissed, show a panel above the changelog: "<version> is available", date, a link to the release, each release's notes rendered with the existing `inline()` code, a Dismiss button (`POST /update/dismiss`), "Check now" (`POST /update/check`, showing `checked_recently` as a toast), and the last check time and error when there is one. Show a dot on the What's new nav entry in `putio-sync-api/ui/app.js` when an undismissed update is known, next to the existing new-entries badge
- [X] T014 [US1] Settings: add an "Updates" card to `putio-sync-api/ui/settings/general.js` with "Check for updates" (`updates.check`) and, only when `GET /update` says `channel.mode == "swap"`, "Install updates automatically" (`updates.auto_install`); add the `updates` section to the save handler in `putio-sync-api/src/routes/system/settings.rs` with the same validation and `saved_message` pattern as the other sections, plus a route test that saving it round-trips through `config.toml`
- [X] T015 [US1] Run quickstart §1 steps 1–3 and 5 against `scripts/fake-release.sh` (written in T024; until then, a hand-made JSON file served with `python3 -m http.server`)

Done notes for T001–T015:

- T001: `bsdiff`, `lzma-rs`, `minisign-verify`, `sha2` and `windows-sys` wait for the phases that use them, and the empty modules (`verify.rs`, `swap.rs`, `trial.rs`, `installer.rs`) aren't created yet. `channel.rs` is a stub that reports an unknown install; US2 adds detection.
- T011: "checked recently" is `409`, not `429`: the web UI treats every 429 as the sign-in rate limit (contracts/api.md updated).
- T013: the What's new page has no nav entry, so instead of a nav dot a toast says a newer release is known, once per page load.
- T014: the card has only "Check for updates" for now; "Install updates automatically" arrives with the swap mode (US3/US4), and a save keeps `auto_install` as it is. The `updates` group isn't in the settings assistant's `groups()` yet.
- T015: checked against a hand-made releases file. The first check ran 60 s after start; the notice, notes, Check now (409 toast within a minute), Dismiss, and restart with checks off (no request in 80 s) all behaved. This also found a busy loop in the schedule while checks were off, now fixed.
- UI size: the update card and settings card take the English page to about 486 KiB, so the cap in `putio-sync-api/tests/ui_assets.rs` goes from 484 to 492 KiB.
- General's help text still leaves updates out, so the 26 catalogs stay valid; T044 adds it with the translations.

**Checkpoint**: US1 works on every install; managed channels show only the release link for now.

## Phase 4: US2 Update a managed install with the right command (P2)

**Goal**: the right command or step per channel, and no file changes on managed installs.
**Independent test**: fake each channel's markers and check the instruction (spec US2).

- [X] T016 [P] [US2] Write failing tests in `putio-sync-update/src/channel.rs`, one per row of research R10, with detection taking an injected `Probe { env: HashMap, exe: PathBuf, file_exists: Fn, read_file: Fn, registry: Fn }` so nothing touches the real system: snap (`SNAP` + `SNAP_NAME=putio-sync`); Flatpak (`FLATPAK_ID`); Unraid (container + `HOST_OS=Unraid`); CasaOS and Umbrel (`PUTIO_SYNC_INSTALL`); container with `PUTIO_SYNC_IMAGE_VARIANT=dhi` and `PUTIO_SYNC_IMAGE`; container from `/.dockerenv` alone (no variant); container from `/proc/1/cgroup` with `kubepods`; Homebrew (`/opt/homebrew/Cellar/putio-sync/0.3.0/bin/putio-sync`); .deb (`/usr/bin/putio-sync` in `/var/lib/dpkg/info/putio-sync.list`); winget (`C:\Users\x\AppData\Local\Microsoft\WinGet\Packages\putio.putio-sync_Microsoft.Winget.Source_8wekyb3d8bbwe\putio-sync.exe`); macOS pkg (`/usr/local/bin/putio-sync` + receipt `/var/db/receipts/com.devclinton.putio-sync.plist`); Windows setup (uninstall key `{6F0B8D2A-6C1E-4F57-9C38-5B1D2E7A4C90}_is1` whose `InstallLocation` is the exe's folder); GitHub archive (none of these, `PUTIO_SYNC_GIT_SHA` set); Unknown (no git sha); the order (snap wins over a `/usr/bin` path; a container wins over Homebrew-looking paths); `PUTIO_SYNC_INSTALL=<kind>` overrides for every kind; and `mode()` is `Swap` only for GitHub archive, `Installer` for pkg and setup, `Instructions` for the rest
- [X] T017 [US2] Implement `channel::detect(&Probe)` and `Probe::system()` (real env, `std::env::current_exe()` canonicalized, real files, the registry through `windows-sys` on Windows only) in `putio-sync-update/src/channel.rs` to pass T016
- [X] T018 [US2] Write failing tests, then implement `Channel::instruction(to, target)` in `putio-sync-update/src/channel.rs` with the exact text from research R10: `sudo snap refresh putio-sync`; `flatpak update com.putio.PutioSync`; `brew upgrade putio-sync`; `winget upgrade putio.putio-sync`; .deb link `https://github.com/devclinton/putio-sync/releases/download/v<to>/putio-sync_<to>_<amd64|arm64>.deb` plus `sudo dpkg -i putio-sync_<to>_<arch>.deb`; container `docker pull <image>:<variant>` with alternative `<image>:<to>` for `latest` and `<image>:<variant>-<to>` otherwise, plus the steps "Recreate the container after the pull."; container without a variant: no commands, steps "Pull the newer putio-sync image the way you installed it, then recreate the container."; Unraid "Docker tab → Check for updates → apply update"; CasaOS and Umbrel their dashboard update step and, for CasaOS, the version tag `<image>:<to>`; Unknown: only the release link
- [X] T019 [US2] Add `channel` and `available.instruction` to `GET /update` in `putio-sync-api/src/routes/system/update.rs`, and `install_channel` to `GET /status` in `putio-sync-api/src/routes/system/status.rs` (FR-012); route tests for a faked snap and a faked container
- [X] T020 [US2] UI: in `putio-sync-api/ui/pages/news.js` show "Installed with <label>" and, for `mode == "instructions"`, each command in a `<code>` block with a copy button, then the alternatives and steps; never an Update button on these channels (FR-010)
- [X] T021 [P] [US2] Bake in the image variant: `ENV PUTIO_SYNC_IMAGE_VARIANT=latest` and `ARG PUTIO_SYNC_IMAGE=ghcr.io/devclinton/putio-sync` / `ENV PUTIO_SYNC_IMAGE=$PUTIO_SYNC_IMAGE` in the final stage of `Dockerfile`; in `Dockerfile.dhi` set `dhi` on `runtime`, `dhi-ffmpeg` on `runtime-ffmpeg`, `dev` on `dev`, `dev-ffmpeg` on `dev-ffmpeg` (each stage re-sets it, since the ffmpeg stages derive from the others); pass `--build-arg PUTIO_SYNC_IMAGE=ghcr.io/${{ github.repository }}` in `.github/workflows/docker.yml` and `.github/workflows/docker-dhi.yml`
- [X] T022 [P] [US2] Add `PUTIO_SYNC_INSTALL: casaos` to `packaging/casaos/putio-sync/docker-compose.yml` and `PUTIO_SYNC_INSTALL: umbrel` to the app service in `packaging/umbrel/putio-sync/docker-compose.yml`; check both with `docker compose config -q`
- [ ] T023 [US2] Confirm on real installs the two ✱ markers in research R10 (Unraid `HOST_OS=Unraid` with `docker exec … env`; the winget path after `winget install putio.putio-sync`) and fix research.md and T016 if either is different (maintainer)

Done notes for T016–T022:

- T016/T017: the release-build test is a `release_build` flag on `Probe` (`PUTIO_SYNC_GIT_SHA` is set at compile time, not in the runtime env). `PUTIO_SYNC_INSTALL` kinds: `snap`, `flatpak`, `unraid`, `casaos`, `umbrel`, `container`, `homebrew`, `deb`, `winget`, `pkg`, `setup`, `archive`, `unknown`; an unknown kind logs a warning and falls back to detection. The macOS pkg check reads the receipt plist only (no `pkgutil` call). The registry read compiles for Windows (checked with clippy on `x86_64-pc-windows-msvc` in a scratch crate; the full workspace needs a Windows C toolchain for `ring`).
- T018: `instruction(to, arch)` takes the `.deb` architecture (`deb_arch()`); the .deb instruction is the download link plus `sudo dpkg -i`, without a separate step line.
- T019: `channel.label` is a string now (the container label names its variant); `image` and `variant` appear for containers only. `UpdateService::with_channel` lets route tests fake a channel.
- T020: `copyText` moved from `pages/transfers.js` to `app.js` so both pages use it. Checked in the app with `PUTIO_SYNC_INSTALL=container PUTIO_SYNC_IMAGE_VARIANT=dhi`: the card shows "Installed with Docker (dhi image).", both pulls with Copy (the copy works over plain HTTP), the recreate step, and no Update button.
- T021: checked with `docker build --check` for `Dockerfile` and each `Dockerfile.dhi` target; no image was built.
- T022: `docker compose config -q` passes for CasaOS. Umbrel's file fails it on `main` too (`app_proxy` has no image; Umbrel supplies it); without that service it passes and carries the marker.
- T023 is left for the maintainer (real Unraid and winget installs).

**Checkpoint**: every managed channel shows its own instruction.

## Phase 5: Release pipeline (blocks testing US3 end to end)

- [X] T024 [P] Write `putio-sync-update/examples/make-patch.rs` (`make-patch <old> <new> <out>`: `bsdiff::diff` written raw, no compression) with a round-trip test in `putio-sync-update/tests/patch_roundtrip.rs` that diffs two 1 MB random buffers differing in a few places, compresses with `xz` when it is on PATH (skip otherwise), decodes with `lzma-rs` and patches back byte-identical; and write `scripts/fake-release.sh <ver> <dir>` that builds the assets of contracts/release-assets.md from two local binaries, signs `SHA256SUMS` with a test key, writes a GitHub-shaped `releases` JSON and serves `<dir>` on `localhost:8099`
- [X] T025 Add the `patches` job to `.github/workflows/release-binaries.yml` per contracts/release-assets.md: needs the build jobs; downloads this run's artifacts; writes `putio-sync-<ver>-<target>.xz` (`xz -9e --check=crc64`) for the 5 targets; for the 3 newest earlier non-draft, non-pre-release releases (`gh release list --exclude-drafts --exclude-pre-releases --limit 4`, minus this one) gets each target's old binary from its `.xz` asset or, for releases before 055, out of its `.tar.gz`/`.zip` (`putio-sync-<ver>-linux-<arch>.tar.gz`, `putio-sync-<ver>-macos-arm64.tar.gz`, `putio-sync-v<ver>-<rust-triple>.zip`); runs make-patch and `xz -9e`; skips and logs a missing target; uploads them as `release-patches`. Do not rename any existing asset
- [X] T026 Extend the `checksums` job in `.github/workflows/release-binaries.yml`: needs `patches` as well; adds a `<sha256>  putio-sync-<ver>-<target>.bin` line per target (hash of the uncompressed binary) to `SHA256SUMS`; installs `minisign` (`sudo apt-get install -y minisign`); fails when `MINISIGN_SECRET_KEY` is empty; signs with `minisign -S -s key -m SHA256SUMS -t "putio-sync <ver>"` using `MINISIGN_PASSWORD`; uploads `SHA256SUMS.minisig`; adds both secrets to the header comment
- [X] T027 [P] Add `make release-patches OLD=<dir> NEW=<dir>` to `Makefile` (runs make-patch + `xz` for each binary pair, writes `SHA256SUMS` with `.bin` lines, no signing), and document creating the minisign key pair, the two secrets, and where the public key goes (`putio-sync-update/src/verify.rs`) in `MAINTAINING.md` (maintainer creates the key)

Done notes for T024–T027:

- T024: the shell side is split into `scripts/release-unpack.sh` (bare binaries out of `.xz`, `.tar.gz`, `.zip` or `.bin` assets, as `putio-sync-<ver>-<target>.bin`), `scripts/release-patches.sh` (the `.xz`, the patches and the `.bin` hash lines) and `scripts/release-sums.sh` (`SHA256SUMS`); the workflow, `make release-patches` and `fake-release.sh` all call them. Signing in `fake-release.sh` uses `examples/sign.rs` (the `minisign` crate, a dev-dependency) instead of the `minisign` tool, with a password-protected test key pair committed in `putio-sync-update/tests/data/` (password `putio-sync-test`; the crate's unencrypted keys can't be read back). `fake-release.sh` takes `OLD_BIN`, `NEW_BIN`, `FROM`, `TARGET`, `PORT` and `NO_SERVE` from the environment, serves `releases` and `download/v<ver>/<asset>`, and gives each asset `size` and `browser_download_url`. The signature verifies with the `minisign` crate against the committed public key.
- T025: the earlier releases come from `gh release list --limit 50`, filtered to versions below this one with `sort -V` and cut to 3, so re-running an old tag never patches from a newer release. An old release's `.xz` assets are used when it has any, else its archives.
- T026: the key is written to `$RUNNER_TEMP` and the password piped to `minisign`; the job fails with an error when `MINISIGN_SECRET_KEY` is empty, dry runs included, until the maintainer adds the secrets.
- T027: OLD and NEW are folders of release assets (`gh release download`) or `.bin` files; output goes to `target/release-patches/`. Checked on the real v0.2.1 → v0.2.2 macOS and Linux x86_64 archives: the patches are about 55–60% of the full `.xz` (5.9 MB against 11 MB for macOS), and each target takes about 40 s.

## Phase 6: US3 Update a GitHub binary with one click (P3)

**Goal**: download, verify, swap, restart, roll back.
**Independent test**: a fake release from a local server and a scratch install folder (spec US3).

- [ ] T028 [P] [US3] Write failing tests in `putio-sync-update/src/verify.rs`: parses `SHA256SUMS` (`<hex>  <name>` and `<hex> *<name>`); `verify_list(sums, minisig, pubkey)` accepts a list signed by the test key and rejects a missing signature, another key's signature, and a list changed after signing; `check(bytes, name)` fails when the name is missing or the hash differs. The release public key is a `const` (placeholder until T027's key exists, with a test that fails a release build if it's still the placeholder); `PUTIO_SYNC_UPDATE_PUBKEY` replaces it only under `cfg(debug_assertions)`
- [ ] T029 [US3] Implement `putio-sync-update/src/verify.rs` with `minisign-verify` to pass T028; count `putio_sync_update_verify_failures_total{stage}`
- [ ] T030 [P] [US3] Write failing tests in `putio-sync-update/src/fetch.rs` against mockito serving a fake release: picks the patch when `putio-sync-<running>-to-<to>-<target>.bsdiff.xz` is listed, else the `.xz` full binary (FR-014); checks each download's hash before decoding and the result against the `.bin` line (FR-015); a corrupted patch, a wrong `.bin` hash, a 404 and a disk-full (write to a read-only temp folder) each fail with nothing left behind but the temp file, which gets deleted; reports `download_bytes` for the UI
- [ ] T031 [US3] Implement `fetch::prepare(release, current_exe_bytes, target) -> PreparedBinary` in `putio-sync-update/src/fetch.rs` (stream to a temp file in the exe's folder, `lzma_rs::xz_decompress`, `bsdiff::patch`), to pass T030
- [ ] T032 [P] [US3] Write failing tests in `putio-sync-update/src/swap.rs` on a temp folder holding a fake `putio-sync` script: `install(prepared, exe)` leaves exactly `putio-sync` (new) and `putio-sync.old` (old), with the same permissions as before, and never a moment with no `putio-sync` (check by listing after each step through a test hook); fails with `NotWritable(path)` on a read-only folder (US3 scenario 6); on Windows (`cfg(windows)` tests) the running exe is renamed to `putio-sync.old.exe` with `MoveFileExW` while a child process holds it open
- [ ] T033 [US3] Implement `swap::install` and `swap::restart(args, env)` in `putio-sync-update/src/swap.rs`: Unix writes `putio-sync.new`, fsyncs it, copies the mode, renames old → `.old` and new → current, and `restart` calls `CommandExt::exec` with `std::env::args_os()`; Windows renames the running exe aside, writes the new file, and `restart` spawns it with `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP` and the same args, then exits 0 (research R7)
- [ ] T034 [P] [US3] Write failing tests in `putio-sync-update/src/trial.rs` on a temp config folder: `begin(from, to, old_path, db)` writes `update-pending.json` as in data-model.md and a database copy made with `VACUUM INTO`; `on_start(own_version)` with no marker does nothing; own == `to` and `starts == 0` sets `starts = 1` and returns `Trial`; own == `to` and `starts >= 1` returns `RollBack("did not start")`; own == `from` with `rolled_back` set moves it into `update_state` and deletes the marker; `commit()` deletes the marker and the `.old` binary (`.old.exe` on Windows); `roll_back(reason)` puts `.old` back, restores the database copy, and rewrites the marker as `{rolled_back, reason}`
- [ ] T035 [US3] Implement `putio-sync-update/src/trial.rs` to pass T034, plus `arm(deadline = 90 s)`: a panic hook and a timer that call `roll_back` and then `swap::restart` on the old binary. Debug builds fail at start when `PUTIO_SYNC_TEST_FAIL_START=1`
- [ ] T036 [US3] Call `trial::on_start` first thing in `putio-sync-cli/src/main.rs`, before logging setup touches files (on `RollBack`, roll back and restart at once); in `putio-sync-cli/src/commands/daemon.rs` call `trial::commit()` after `api_listener::start` returns Ok and `daemon.initialize()` succeeded, and on an `Err` from either while in trial, roll back. Add an integration test in `putio-sync-cli/tests/update_trial.rs` that runs the built binary with a marker naming its own version as `to` and `starts = 1`, and checks it restores a fake `.old` script and runs it
- [ ] T037 [US3] Implement `UpdateService::apply(interrupt)` in `putio-sync-update/src/lib.rs`: refuses with `NotSupported` unless `mode == Swap` (FR-010), `InProgress` while an attempt runs, `NoUpdate`, `Busy(n)` when `DownloadManager::active_workers() > 0` or a pipeline step is running and `interrupt` is false (FR-019). Then verify (T029), fetch (T031), `trial::begin`, `swap::install`, stop the daemon's services the way Ctrl+C does (reuse the shutdown path in `putio-sync-daemon/src/daemon.rs`), and `swap::restart`. Track `Attempt` states as in data-model.md, log each with `from`, `to`, `kind`, `stage`, and count `putio_sync_update_attempts_total`. Tests with a fake idle/busy source
- [ ] T038 [US3] Add `POST /update/apply` to `putio-sync-api/src/routes/system/update.rs` with every answer in contracts/api.md (`202`, `409 busy|not_supported|in_progress|not_writable`, `404 no_update`) and `attempt`, `busy`, `patch`, `download_bytes`, `rolled_back` in `GET /update`; route tests for each answer using a fake `UpdateService` backend
- [ ] T039 [US3] UI in `putio-sync-api/ui/pages/news.js`: "Update now" for `mode == "swap"`, showing the download size and "patch" or "full download"; on `409 busy` a confirm dialog "n transfers will be interrupted and resume after the restart" that retries with `interrupt: true`; progress from polling `GET /update` every 2 s; during `restarting` poll `GET /status` until `version` changes, then reload; show a rollback message with its reason and a Dismiss; for `not_writable` show the folder and the manual download link instead of the button
- [ ] T040 [US3] Installer channels: write failing tests, then implement `putio-sync-update/src/installer.rs`: downloads `putio-sync-<to>-macos-arm64.pkg` or `putio-sync-<to>-windows-<x64|arm64>-setup.exe` into `<config dir>/updates/`, checks it against the signed `SHA256SUMS`, and opens it (`open` on macOS, `ShellExecuteW` with `runas` on Windows) when in the user's desktop session (research R9), otherwise reports `Downloaded(path)`. Route `apply` to it for `mode == Installer`; the UI shows "The installer is open" or the path and steps
- [ ] T041 [US3] Run quickstart §2 on Linux, then on macOS and Windows (the swap and restart code differs per OS)

**Checkpoint**: GitHub archive installs update with one click and roll back by themselves.

## Phase 7: US4 Update automatically (P4)

**Goal**: install when idle, if opted in.
**Independent test**: with the setting on and a transfer running, the update waits and then runs (spec US4).

- [ ] T042 [US4] Write failing tests, then implement in `putio-sync-update/src/lib.rs`: when a check finds a newer version and `updates.auto_install` is true and `mode == Swap`, the attempt enters `waiting_for_idle`, re-checks idleness every minute, and calls `apply(false)` once idle; it never starts automatically for the version in `update_state.last_rolled_back` (FR-020, US4 scenario 3); it does nothing for other modes even with the key set (US4 scenario 4). Use `tokio::time::pause`
- [ ] T043 [US4] Show `waiting_for_idle` ("Waiting for transfers to finish") in `putio-sync-api/ui/pages/news.js`, and run quickstart §3

## Phase 8: Polish

- [ ] T044 [P] Add the UI strings from T013, T014, T020, T039, T040 and T043 to the English catalog and translate them into the other languages with `atranslate` (as in 051), in `putio-sync-api/ui/i18n/*.json`
- [ ] T045 [P] Add an "Updating" section to `INSTALLATION.md` (how each channel updates, what the notice shows, turning checks off, that versions up to 0.2.0 must install the first 055 release by hand) and point the README to it
- [ ] T046 [P] Add a CHANGELOG line under `[Unreleased]` → `### Added` in `CHANGELOG.md`
- [ ] T047 Mark 055 done in `ROADMAP.md` once merged, and record any deviations in `research.md`
- [ ] T048 Validate: `make ci`, `docker compose config -q` for the changed compose files, `actionlint` on the changed workflows if installed, and a manual run of `gh workflow run release-binaries.yml -f version=<test tag>` (quickstart §4)

## Dependencies

- Setup (T001–T003) → Foundational (T004–T006) → US1 (T007–T015).
- US2 (T016–T023) needs US1's routes and UI (T011, T013).
- The release pipeline (T024–T027) needs only T001. It can run alongside US1 and US2.
- US3 (T028–T041) needs US1 and the pipeline (T024 for fake releases, T027 for the real key). T040 needs T029 and T031.
- US4 (T042–T043) needs T037.
- Polish comes last. T044 waits for all UI strings.

## Parallel examples

- After T001: T002, T003, T004, T005 and T024 touch different files.
- US2: T016 (tests), T021 (Dockerfiles) and T022 (compose files) at once.
- US3: the test tasks T028, T030, T032 and T034 at once, then their implementations in any order before T037.

## Implementation strategy

1. **MVP = Phase 1–3 (US1)**: every install learns that a release is out and sees its notes.
   It ships alone.
2. Add US2: correct instructions for the managed channels, where most users are.
3. Pipeline, then US3: the first release with the pipeline carries the patches and the
   signature. Self-update can only reach releases after that one.
4. US4 last; it is a small layer on US3.
