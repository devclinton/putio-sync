# Research: 049 Radarr/Sonarr automatic setup including path mapping

## R1 How to find where the app sees putio-sync's folder

- **Decision**: create `<download folder>/putio-sync-check-<12 hex>` (a new empty folder) on
  putio-sync's side, then walk the app's filesystem browser (`GET /api/v3/filesystem`) looking
  for a folder whose child has that exact name. The folder listed is the download folder as
  the app sees it. A drop guard removes the marker however the search ends.
- **Rationale**: a random name proves it is the same folder, not just a folder with the same
  name (`/downloads` in two containers can be two different volumes). No dot prefix: the
  filesystem browser may hide dot folders.
- **Alternatives**: matching by folder name only (false positives); asking the operator for
  the app's path (not automatic); reading Docker mounts (needs the Docker socket).

## R2 Verify "visible" with the marker too

- **Decision**: the folder step first lists the path the app reads after its mappings
  (018's `arr_path`). If the marker is in it, the folder is truly visible: no change. Otherwise
  search (R1).
- **Rationale**: 018's check only looks for the folder's name in its parent, so an unrelated
  `/downloads` in the app's container looks visible. One extra listing fixes that.

## R3 Search order and limits

- **Decision**: a breadth-first walk with a front-of-queue boost. Seeds: ancestors of the app's
  root folders (`GET /api/v3/rootfolder`), nearest first, then the top of the filesystem (`/`,
  or the root folders' drive on Windows). Children named like putio-sync's folder (e.g.
  `downloads`) or a common mount name (`data`, `downloads`, `media`, `mnt`, `storage`, `share`,
  `torrents`, `volume1`, `user`) go to the front. System folders are skipped (`proc`, `sys`,
  `dev`, `etc`, `usr`, `bin`, `sbin`, `lib*`, `run`, `boot`, `var`, `tmp`, `root`, `config`,
  `app`, `opt`, `snap`). At most 60 listings, depth 6, 20 s in all. A failed listing is
  skipped. Paths come from each entry's `path`, so the app's separator is kept.
- **Rationale**: Docker layouts are shallow; the boost finds them in a few calls; the limits
  bound the worst case (SC-003).

## R4 One change, re-derived before writing

- **Decision**: the proposal is computed from the check and the search:
  - visible (R2) → `none`
  - not found → `not_found` (with where it looked)
  - found at the same path and a mapping for the client host covers it → `remove` that mapping
  - found elsewhere and a mapping for the client host covers the folder → `update` it (by id)
  - found elsewhere, no such mapping → `create` {host, remote = folder + `/`, local = found +
    sep}
  The apply route recomputes the proposal and writes only if it equals the one posted;
  otherwise it returns `changed` with the new proposal. After a write it re-runs the check.
- **Rationale**: Radarr refuses a second mapping for the same host and remote path; recomputing
  makes a double click or a stale page harmless (FR-010). The host is the one in the app's
  putio-sync client, since mappings are matched on it.

## R5 Recommended settings

- **Decision**: `GET /api/v3/config/downloadclient` → `enableCompletedDownloadHandling`;
  `GET /api/v3/config/mediamanagement` → `importExtraFiles` and `extraFileExtensions` (needs
  `srt`). Missing keys mean the app doesn't have the setting (newer versions removed the
  Completed Download Handling switch): skipped. Turn on PUTs the whole fetched body back to
  `/api/v3/config/<section>/<id>` with only that key changed (and `srt` appended to the
  comma-separated list).
- **Rationale**: whole-body PUT keeps every other setting (FR-012).

## R6 The guided flow in the UI

- **Decision**: one `guided(target, box)` renderer in `arr.js`: it calls `POST /arr/setup`,
  then `POST /arr/setup/check` (folder + recommendations), and draws a line per step with the
  action buttons. Confirming a mapping uses the existing `confirm()` dialog. Rows get **Set up**
  whenever they have a key (was: only when no client). "Set up automatically" runs the same
  steps after a successful run, with the typed address and key.
- **Rationale**: one renderer reused twice keeps the bundle addition small (~5 KiB).

## R7 Client host when only checking

- **Decision**: `/arr/setup/check` takes an optional `host`; the UI passes the setup result's
  host. Without one, the app's download clients are read and the host of putio-sync's client
  is used; with no such client the reason is `no_client`.
