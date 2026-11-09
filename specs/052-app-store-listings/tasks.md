# Tasks: 052 Home-Server App Store Listings

**Input**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md)

## Phase 1: Setup

- [X] T001 Add `LICENSE` (MIT, matching `Cargo.toml`)
- [X] T002 Replace the `devclinton` GitHub owner with `devclinton` in workflows, `Cargo.toml`, docs and packaging files

## Phase 2: US1 Unraid Community Applications (P1)

- [X] T003 [US1] `packaging/unraid/putio-sync.xml`: ports 15432 and 9742, `/config`, `/downloads`, `/watches`, masked `PUTIO_SYNC_API_TOKEN`, `RUST_LOG`, icon, Support and Project URLs, "unofficial" overview
- [ ] T004 [US1] Load the template on an Unraid server, re-save it from the Docker tab and copy the saved XML back (maintainer)

## Phase 3: US2 Umbrel and CasaOS (P2)

- [X] T005 [P] [US2] `packaging/umbrel/putio-sync/` with `app_proxy`, 1000:1000, `${APP_PASSWORD}` as the token, shared Downloads folder
- [X] T006 [P] [US2] `packaging/casaos/putio-sync/docker-compose.yml` with `x-casaos` metadata and `$PUID:$PGID`
- [X] T007 [US2] Check the image reaches setup mode as 1000:1000 with the token (research R5)
- [ ] T008 [US2] After the first release, pin the Umbrel image digest (maintainer)

## Phase 4: US3 Generic compose (P2)

- [X] T009 [US3] `packaging/docker-compose/compose.yaml` with a health check; README's Docker Compose section and INSTALLATION's Home servers section point to it

## Phase 5: US4 Discovery (P3)

- [X] T010 [US4] Drafts for awesome-selfhosted, selfh.st, r/selfhosted, r/putio and Put.io, kept outside the repository for the maintainer

## Phase 6: Polish

- [X] T011 `packaging/STORES.md` with status, submission and per-release steps
- [X] T012 Drop Flathub from README, INSTALLATION and ROADMAP; CHANGELOG Added line
- [X] T013 Validate: `docker compose config -q` for each compose file, XML parse, `make ci`
