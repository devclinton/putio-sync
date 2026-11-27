# Tasks: qBittorrent API emulation (067)

## Phase 1: Answers

- [x] T001 `Snapshot`, `snapshots`, `add_in`, `categories`, `files_on_disk` in the arr service
- [x] T002 `ArrRepo::times`
- [x] T003 `qbit.rs`: states, filters, torrent, properties, files, trackers, categories,
      preferences, build info, transfer info, maindata
- [x] T004 Unit tests for the mapping and the fixed answers

## Phase 2: HTTP and sign-in

- [x] T005 `Sessions`: random SID, idle and total expiry, cap, ends on a password change
- [x] T006 `/api/v2/*path`: query, form and multipart parsing; body limits
- [x] T007 Sign-in with the shared limiter and constant-time compare; cookie attributes
- [x] T008 POST-only changes and the cross-site check
- [x] T009 The routes of FR-003, the ignored list, 404 for the rest; metrics labels

## Phase 3: Tests per tool

- [x] T010 Sign-in and security tests through the router
- [x] T011 Contract: Radarr and Sonarr (magnet and .torrent)
- [x] T012 Contract: autobrr
- [x] T013 Contract: cross-seed
- [x] T014 Contract: phone apps

## Phase 4: UI and docs

- [x] T015 `client.qbittorrent` on `GET /arr`; Settings shows the type and URL
- [x] T016 The new strings in all 26 locales
- [x] T017 Guides: autobrr, cross-seed, phone apps; a section in the Radarr/Sonarr guide;
      `guides.js` and SUMMARY
- [x] T018 README, CHANGELOG, ROADMAP
- [x] T019 All checks pass

## Not done

- [ ] T020 Replace the reconstructed request shapes with captures from the real tools
