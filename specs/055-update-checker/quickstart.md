# Quickstart: checking 055 by hand

Automated tests cover each piece (`cargo test -p putio-sync-update`, and the `/update` route
tests in `putio-sync-api/tests/`) against a mockito releases server and a scratch install
folder. These manual steps check the pieces together on a real machine.

## Prerequisites

- `make ci` passes.
- Two release builds of different versions, for example the current tree built as `0.9.0`
  and as `0.9.1` (set `version` in the workspace `Cargo.toml`), and
  `PUTIO_SYNC_GIT_SHA=test` set so the build counts as a release build.
- A test minisign key pair, built in with `PUTIO_SYNC_UPDATE_PUBKEY` (debug builds only; release
  builds ignore it).

## 1. Notice and managed channels (US1, US2)

1. Serve a fake release tree: `scripts/fake-release.sh 0.9.1 <dir>` writes the GitHub API JSON,
   the assets, `SHA256SUMS` and the signature, and serves them on `localhost:8099`.
2. Run the 0.9.0 daemon with `PUTIO_SYNC_UPDATE_API=http://localhost:8099` (debug-only
   override).
3. Within about a minute, the What's new area shows "0.9.1 is available" with its notes.
4. Restart it with `SNAP=/snap/putio-sync/x1 SNAP_NAME=putio-sync`. The notice shows
   `sudo snap refresh putio-sync` and no button. Repeat with `PUTIO_SYNC_IMAGE_VARIANT=dhi` and
   `/.dockerenv` faked through `PUTIO_SYNC_INSTALL=container`, and with
   `PUTIO_SYNC_INSTALL=casaos`. Each shows the instruction from research R10.
5. Turn checks off in Settings, restart, and check the fake server's log: no request.

## 2. Self-update (US3)

1. Copy the 0.9.0 binary into a scratch folder and run it from there.
2. Click "Update now". The log shows a patch download, two passing hash checks, the swap and
   the exec. `GET /status` reports 0.9.1, and `putio-sync.old` is gone after the trial.
3. Repeat with the patch removed from the fake release. It downloads the `.xz` full binary.
4. Corrupt one byte of the patch. The update fails at "verifying" and the binary is unchanged.
5. Build a 0.9.1 that exits at start (`PUTIO_SYNC_TEST_FAIL_START=1`, debug only) and update to
   it. Within 90 s the 0.9.0 binary is back and the UI says "rolled back".
6. Start a large download and click "Update now". The UI asks to confirm the interruption.
   After confirming, the download resumes from its partial file on 0.9.1.

## 3. Automatic update (US4)

Turn on "Install updates automatically" while a download runs. The attempt shows
`waiting_for_idle` and starts within a minute of the download finishing.

## 4. Release workflow

`gh workflow run release-binaries.yml -f version=v0.9.1` (no upload). The artifacts contain the
`.xz` binaries, up to 15 `.bsdiff.xz` patches, `SHA256SUMS` with `.bin` lines, and
`SHA256SUMS.minisig`, and `minisign -V` passes with the public key.

Windows and macOS: run step 2 once on each before the first release with 055 (the swap and
restart code differs per OS).
