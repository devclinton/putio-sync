# Tasks: 054 Project Website

**Input**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md)

## Phase 1: Setup

- [ ] T001 Turn on Pages with "GitHub Actions" as the source (maintainer, repository settings)
- [ ] T002 `site/book/book.toml` and `SUMMARY.md`; `site/tools/build.sh` that builds into `target/site`

## Phase 2: US2 Docs (P1)

- [ ] T003 [US2] ANCHOR comments around each top-level manual section in README.md
- [ ] T004 [US2] One chapter per anchor, plus INSTALLATION, PLUGINS, EVENTS, CHANGELOG included whole
- [ ] T005 [US2] Rewrite cross-file Markdown links to chapter pages in the build
- [ ] T006 [US2] Keep mdBook's fonts local; check the built docs make no third-party requests

## Phase 3: US1 Landing page (P1)

- [ ] T007 [US1] `site/index.html` and `style.css`: pitch, not-affiliated line, highlights, light/dark, phone width
- [ ] T008 [US1] Install section listing every platform with commands and links; `install.js` filter
- [ ] T009 [US1] Version stamping in `build.sh`

## Phase 4: Deploy

- [ ] T010 `.github/workflows/pages.yml` with the FR-002 triggers, lychee and axe checks, deploy
- [ ] T011 README link to the site near the top; CHANGELOG Added line

## Phase 5: US3 Screenshots (P2)

- [ ] T012 [US3] Find or build a demo harness on the fake Put.io server (research R5)
- [ ] T013 [US3] `site/tools/screenshots.mjs`; commit the images; use them on the landing page
- [ ] T014 [US3] Use the same images in README and the store listings (STORES.md, Umbrel `gallery`, CasaOS `screenshot_link`, snap)

## Phase 6: US4 Translations (P3)

- [ ] T015 [US4] Landing page strings in es, zh, hi, fr, ar (RTL), machine-translated and marked

## Phase 7: Polish

- [ ] T016 Set the site as the website URL in the snap, Unraid, Umbrel and CasaOS files
- [ ] T017 Lighthouse on the live site (SC-001); fix what falls short
