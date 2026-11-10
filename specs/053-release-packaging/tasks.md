# Tasks: 053 Release Packaging

**Input**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md)

## Phase 1: Setup

- [X] T001 reqwest without default features: `rustls-tls` + `rustls-tls-native-roots`; `Cargo.lock` loses `openssl`, `openssl-sys` and `native-tls` (research R1)
- [X] T002 `/dist/` in `.gitignore` and `.dockerignore`; `.gitattributes` keeps `*.cmd` CRLF

## Phase 2: US1 Static Linux binary (P1)

- [X] T003 [US1] `Dockerfile.dhi` `builder` and `binary` stages on `dhi.io/rust:1.96-alpine3.22-dev`; the build fails if `readelf -d` lists any `NEEDED` entry
- [X] T004 [US1] Build the aarch64 binary locally: `file` says statically linked, it runs in busybox (no libc, no CA bundle), reaches Put.io over TLS and serves the setup wizard
- [ ] T005 [US1] Build and run the x86_64 binary (the local builder has no amd64; CI only)
- [X] T006 [US1] `release-binaries.yml` `build-linux` on native runners; `make dist-linux`

## Phase 3: US2 Windows installer (P1)

- [X] T007 [US2] `packaging/windows/putio-sync.iss`: SYSTEM startup task (default), sign-in shortcut or manual, optional PATH, Start-menu web UI link, uninstall cleanup (research R4)
- [X] T008 [US2] `putio-sync-service.cmd` with the log in `%ProgramData%\putio-sync`
- [X] T009 [US2] `build-windows` keeps the winget zip names and adds the Inno Setup installer
- [ ] T010 [US2] Compile both installers and install the x64 one on Windows (CI and maintainer)

## Phase 4: US3 macOS package (P2)

- [X] T011 [US3] `packaging/macos/`: `build-pkg.sh`, `distribution.xml` with the "Start at login" choice, LaunchAgent plist, postinstall, `uninstall.sh`, welcome page (research R5)
- [X] T012 [US3] Build the `.pkg` locally and inspect its payload, scripts and plist
- [ ] T013 [US3] Install it on a clean Mac, get past Gatekeeper, check the agent starts and the uninstaller removes everything (maintainer)
- [X] T014 [US3] `build-macos` job, signing only when `MACOS_INSTALLER_*` secrets are set; `make dist-macos`

## Phase 5: US4 Checksums and the release chain (P2)

- [X] T015 [US4] `checksums` job writes `SHA256SUMS` and uploads it with every asset
- [X] T016 [US4] `release.yml`: draft release, builds upload into it, publish, then Homebrew and winget; per-platform workflows get `workflow_call` (research R7)
- [ ] T017 [US4] Run `release-binaries.yml` by hand as a dry run, then cut a tag (CI and maintainer; Actions is off for the private repository)

## Phase 6: US5 arm64 snap (P2)

- [X] T018 [US5] `snap.yml` amd64 + arm64 matrix for build and test, `workflow_call` with `channel` (research R6)
- [ ] T019 [US5] First arm64 snap build and store upload (CI)

## Phase 7: US6 Hardened images (P3)

- [X] T020 [US6] `Dockerfile.dhi` `runtime`, `runtime-ffmpeg`, `dev`, `dev-ffmpeg` stages as 99:100 with the alpine image's volumes, ports and command (research R3)
- [X] T021 [US6] Build all four for arm64 and check setup mode, uid/gid, ffmpeg 8.0.1 and tzdata
- [X] T022 [US6] `docker-dhi.yml`: per-arch push by digest, merged into prefixed tags; tags and manual runs only; `make docker-dhi`
- [ ] T023 [US6] Add `DHI_USERNAME` / `DHI_TOKEN` secrets and run the workflow (maintainer)

## Phase 8: Polish

- [X] T024 INSTALLATION.md: macOS and Windows installers with the Gatekeeper and SmartScreen steps, standalone Linux binary, `SHA256SUMS`, hardened image tags
- [X] T025 CHANGELOG Added and Changed lines; drop "Queued: packaging" from ROADMAP.md
- [X] T026 Validate: actionlint on the workflows, `make ci`
