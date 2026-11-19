# Tasks: Onboarding guides

**Input**: Design documents from `specs/016-onboarding-guides/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/ui-links.md,
quickstart.md

**Tests**:
- Guides: the site build, lychee and a11y, plus walk-throughs.
- UI links: a node test (R5) and a channel unit test.

**PRs**: US1 → PR A, US2 → PR B, US3 → PR C, US4 → PR D (plan.md, Delivery). Each guide's
walk-through is recorded as a checklist in its PR body.

## Format: `[ID] [P?] [Story] Description`

## Phase 1: Setup

- [ ] T001 Confirm mdBook 0.5.4 and atranslate are available locally (`mdbook --version`; `curl -fsS http://localhost:8400/health`), and that `site/tools/build.sh` builds main cleanly into `target/site`
- [ ] T002 Read the reference chapters the guides will link to (`site/book/src/installation.md`, `docker.md`, `web-ui.md`, `radarr-and-sonarr.md`, `media-server.md`, `metrics.md`, `configuration.md`) and note each section's anchor id from the built HTML in `target/site/docs/`

## Phase 2: Foundational

- [ ] T003 Add a "Guides" part to `site/book/src/SUMMARY.md` right after `[Introduction](introduction.md)`, with `- [Guides](guides/index.md)` and the chapters nested under it in the order of plan.md
- [ ] T004 Write `site/book/src/guides/index.md`:
  - three short sections: "I'm new", "I'm adding something" and "I'm switching";
  - each lists its guides with one line saying who the guide is for;
  - CasaOS and Umbrel are mentioned under Docker, with a link to their app stores.

  Guides not written yet are left out of the index and SUMMARY until their PR.
- [ ] T005 Write the shared guide conventions as a comment block at the top of `site/book/src/guides/index.md` (HTML comment, not rendered):
  - each guide opens with "You'll need" and "You'll end with";
  - steps are numbered;
  - UI labels are in bold, exactly as the UI shows them;
  - reference details are linked, not copied;
  - tokens are placeholders;
  - the untested note follows research R9.

**Checkpoint**: the book builds with the Guides part and the index.

## Phase 3: User Story 1, set up on my platform (Priority: P1) 🎯 MVP

**Goal**: one page per platform, from install to a first finished sync and updates (FR-001, FR-002, FR-005, FR-006).

**Independent Test**: follow the guide on a clean machine of that platform; the reader ends with a test file synced and knows how it updates (quickstart.md, Walk-throughs).

- [ ] T006 [P] [US1] Write `site/book/src/guides/unraid.md`:
  - install from the Community Applications template in `packaging/unraid`;
  - paths and ports, the web UI on port 15432, and signing in to put.io in the setup wizard;
  - choose the folder and run Sync now;
  - updates via Unraid's Docker page.
- [ ] T007 [P] [US1] Write `site/book/src/guides/truenas.md`:
  - install the community app from `packaging/truenas` (its questions in `questions.yaml`), and storage and port choices;
  - the first sync;
  - updates via TrueNAS Apps.
- [ ] T008 [P] [US1] Write `site/book/src/guides/raspberry-pi.md`:
  - a 64-bit OS is required (research R7), checked with `uname -m` showing `aarch64`;
  - Docker, or the aarch64 tarball from the latest release, run as a systemd service;
  - the first sync and updates;
  - the R9 untested note at the top.
- [ ] T009 [P] [US1] Write `site/book/src/guides/docker.md`:
  - `docker run` and the Compose file in `packaging/docker-compose`, with volumes, ports, the API token and PUID/PGID if the image uses them (check `Dockerfile`);
  - the first sync and updates (`docker compose pull && docker compose up -d`);
  - other NAS boxes (Synology, QNAP) as a short note.
- [ ] T010 [P] [US1] Write `site/book/src/guides/macos.md`: Homebrew (tap, install, `brew services start`) or the .pkg, then opening the web UI, signing in, the first sync, and updates via `brew upgrade` or the in-app update for .pkg
- [ ] T011 [P] [US1] Write `site/book/src/guides/windows.md`: the installer or winget, opening the web UI, signing in, the first sync, and updates (the in-app installer update, or `winget upgrade`)
- [ ] T012 [P] [US1] Write `site/book/src/guides/linux.md`: sections for snap (with its interface connections), .deb and the standalone binary (with a systemd unit), each going to the same "open the web UI and sign in" steps; and updates per method
- [ ] T013 [US1] Add the seven setup guides to `site/book/src/SUMMARY.md` and `site/book/src/guides/index.md`, then run `site/tools/build.sh`, lychee and `npm run a11y` (quickstart.md) and fix any failures
- [ ] T014 [US1] Walk through macOS (this Mac), Unraid (a second container on Mnemosyne with its own port and config path, removed afterwards) and Docker. Correct each guide where a step differs, and time SC-001. Never print the NAS token or config.
- [ ] T015 [US1] Refresh docs translations with `site/tools/translate-docs.py` (quickstart.md) and commit `site/i18n/*.json`
- [ ] T016 [US1] Add a CHANGELOG `[Unreleased]` line, and open draft PR A with a walk-through checklist per guide: Claude for macOS/Unraid/Docker, the maintainer for Windows/Linux/TrueNAS, and the Pi marked untested

**Checkpoint**: PR A merged once the maintainer's walk-throughs are ticked; the setup guides are live.

## Phase 4: User Story 2, add a piece to a running setup (Priority: P2)

**Goal**: task guides (FR-003, FR-005).

**Independent Test**: on a running install, follow one guide; the piece works at the end.

- [ ] T017 [P] [US2] Write `site/book/src/guides/radarr-sonarr.md`:
  - Settings › Radarr and Sonarr, with Add Radarr/Add Sonarr and Test and Save;
  - categories and folders, and path mapping when the apps run in other containers;
  - Prowlarr: indexers are added in Prowlarr and synced to the apps, and putio-sync needs no Prowlarr setup;
  - check with a manual search and import.
- [ ] T018 [P] [US2] Write `site/book/src/guides/folder-rules.md`: the video, audio and ebook presets, blocked file types, skip folders and per-folder rules, with one worked example (TV only, no samples)
- [ ] T019 [P] [US2] Write `site/book/src/guides/watch-folders.md`: what a watch folder does (uploads .torrent and .magnet files to put.io), setting one up in the UI and in config, and the cleanup of uploaded files
- [ ] T020 [P] [US2] Write `site/book/src/guides/media-server.md`: turning on the DLNA media server, finding it from a TV, and the multicast/switch caveat for when it isn't found
- [ ] T021 [P] [US2] Write `site/book/src/guides/metrics-grafana.md`:
  - turn on metrics (`:9742/metrics`);
  - a Prometheus scrape config;
  - import the dashboard from `packaging/grafana`;
  - a few useful queries, linked to `metrics.md`.
- [ ] T022 [P] [US2] Write `site/book/src/guides/troubleshooting.md`:
  - run the doctor from the UI and from the CLI (`putio-sync doctor`);
  - read the Logs page and change verbosity;
  - common causes: token, disk space, paths, permissions, and put.io limits;
  - how to report a bug with logs, with secrets removed.
- [ ] T023 [US2] Add the task guides to `SUMMARY.md` and the index; build, lychee and a11y
- [ ] T024 [US2] Walk through:
  - Radarr/Sonarr, the media server and metrics on Mnemosyne, using a test category and removing it afterwards;
  - folder rules, watch folders and troubleshooting on the Mac.

  Correct steps that differ.
- [ ] T025 [US2] Refresh translations, add a CHANGELOG line, and open draft PR B with the walk-through checklist

## Phase 5: User Story 3, switch from a local client (Priority: P3)

**Goal**: switching guides (FR-004).

**Independent Test**: from a Sonarr plus old-client setup, follow the guide; new grabs go to putio-sync and land in the same library folders.

- [ ] T026 [US3] Write a shared "What changes when put.io does the downloading" section:
  - no local seeding or ratio; put.io handles that;
  - storage on put.io;
  - finished files are brought home by putio-sync.

  It lives at the top of `site/book/src/guides/from-qbittorrent.md`, and the other switching guides link to it.
- [ ] T027 [P] [US3] Finish `site/book/src/guides/from-qbittorrent.md`:
  - map qBittorrent categories to putio-sync categories and folders;
  - move watch folders;
  - in Radarr/Sonarr, add putio-sync, then disable qBittorrent (don't delete it until queues drain);
  - what happens to in-progress torrents.
- [ ] T028 [P] [US3] Write `site/book/src/guides/from-transmission.md`: the same flow, noting that putio-sync speaks the Transmission protocol (so the *arr client type stays Transmission and only host, port and credentials change), plus the default port clash if both run on one box
- [ ] T029 [P] [US3] Write `site/book/src/guides/from-deluge.md`: the same flow, with Deluge labels mapped to categories and the Label plugin's role
- [ ] T030 [P] [US3] Write `site/book/src/guides/from-usenet.md`:
  - put.io doesn't take NZBs (research R2, with the put.io help link);
  - keep SABnzbd/NZBGet for usenet and add putio-sync for torrents;
  - give the clients separate categories, and set the protocol priority in Radarr/Sonarr's delay profiles.
- [ ] T031 [US3] Add the switching guides to `SUMMARY.md` and the index; build, lychee and a11y
- [ ] T032 [US3] Walk through the qBittorrent guide against a throwaway qBittorrent container and Sonarr on this Mac (Docker), and check the other three for consistency with it
- [ ] T033 [US3] Refresh translations, add a CHANGELOG line, and open draft PR C with the walk-through checklist

## Phase 6: User Story 4, guides from inside the web UI (Priority: P4)

**Goal**: FR-008 to FR-013. Starts after PRs A to C are merged and the site is published.

**Independent Test**: quickstart.md, "UI links (PR D)".

- [ ] T034 [US4] Write the failing test `putio-sync-api/tests/ui/guides.test.mjs` (research R5). It checks that:
  - every `slug` in `ui/guides.js` matches `site/book/src/guides/<name>.md` and appears in `site/book/src/SUMMARY.md`;
  - every guide in SUMMARY is in the list;
  - every `docs("guides/…")` call in `putio-sync-api/ui/**/*.js` names a listed slug;
  - `guideFor(kind)` follows the table in contracts/ui-links.md.

  Register it where `tests/ui_js.rs` lists the node tests.
- [ ] T035 [US4] Create `putio-sync-api/ui/guides.js`. It exports:
  - `GUIDES`, a list of `{ slug, title, kind, words }`, where `kind` is "`setup`, `task` or `switch`" plus `index`, and `title` matches the chapter's H1 (data-model.md);
  - `guideFor(kind)`, implementing the contracts/ui-links.md table.

  Add `guides.js` to `ASSETS` in `putio-sync-api/src/ui.rs`.
- [ ] T036 [P] [US4] Add `Channel::Truenas` (kind `truenas`, label "TrueNAS", mode `Instructions`, with update steps "Update the app from TrueNAS Apps") to `putio-sync-update/src/channel.rs`, including `from_kind`, `kind`, `label`, `mode` and the instructions. Add unit tests for `PUTIO_SYNC_INSTALL=truenas`.
- [ ] T037 [P] [US4] Set `PUTIO_SYNC_INSTALL` to `truenas` in `packaging/truenas/putio-sync/templates/docker-compose.yaml` (`c1.environment.add_env`), and bump the app version in `packaging/truenas/putio-sync/app.yaml` if the template's version rules need it
- [ ] T038 [US4] In `putio-sync-api/ui/pages/setup.js`, add a "Setup guide" link on the wizard's first screen. It reads `GET /update` and uses `guideFor(channel.kind)`, or `guides/index` on failure.
- [ ] T039 [P] [US4] Add "Guide" links next to the existing docs links in the settings sections for Radarr and Sonarr (`ui/settings/arr.js`), folder rules, watch folders, the media server and metrics. Find each section's module with `grep -rn` on its heading.
- [ ] T040 [P] [US4] Add a "Troubleshooting guide" link to `putio-sync-api/ui/pages/doctor.js` and `putio-sync-api/ui/pages/logs.js`
- [ ] T041 [US4] Add a "Guides" group, after Settings, to `putio-sync-api/ui/palette.js`:
  - add it to `GROUPS`;
  - in `results()`, rank `GUIDES` on `words(title, words)` with the kind label as detail;
  - shown only for a non-empty query;
  - `run` opens the guide with `window.open(url, "_blank", "noopener")`.

  Extend `tests/ui/fuzzy.test.mjs` or a palette test only if a helper is pure.
- [ ] T042 [US4] Run `node scripts/i18n-extract.mjs`, then `scripts/i18n-translate.mjs` (with ATRANSLATE_URL and the atranslate-cli PATH from earlier runs), for the new UI strings in `putio-sync-api/ui/i18n/*.json`
- [ ] T043 [US4] Check in the browser against a local build (quickstart.md, steps 1–3), including with `PUTIO_SYNC_INSTALL=unraid`, at phone width and in the light theme
- [ ] T044 [US4] Run `make ci`; update the README web-UI section (guide links and the palette's Guides group), add a CHANGELOG line, mark "### 016 Onboarding guides (done)" in `ROADMAP.md`, and open draft PR D

## Phase 7: Polish

- [ ] T045 Once the maintainer has walked through a guide that was untested, remove its R9 note in a follow-up commit (Raspberry Pi stays until tested)
- [ ] T046 Point INSTALLATION.md's "Next Steps" at `guides/index` (the site version), so the install reference leads to the walkthroughs

## Dependencies

- Setup (T001–T002) → Foundational (T003–T005) → US1 → US2 → US3 → US4.
- US2 and US3 only need the Guides part from Phase 2, but they ship after US1 (the agreed order). US3's guides link the Radarr/Sonarr guide from US2.
- US4 needs every guide slug merged and published (FR-013).
- Within a story, the [P] page tasks are independent files; the SUMMARY/index, build, walk-through and PR tasks follow them.

## Parallel examples

- US1: T006–T012 (seven pages) at once, then T013.
- US2: T017–T022 at once.
- US3: T027–T030 after T026.
- US4: T036, T037, T039 and T040 alongside each other, after T035.

## Implementation strategy

MVP is US1 (PR A): with the setup guides live, a new user can get going alone. Each later
PR adds one kind of guide, and PR D wires them into the UI last.
