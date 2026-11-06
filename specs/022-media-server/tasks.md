# Tasks: Media Server (DLNA) for Local Files and Put.io

**Input**: Design documents from `/specs/022-media-server/`.

**Prerequisites**:
- [plan.md](plan.md)
- [spec.md](spec.md)
- [research.md](research.md)
- [data-model.md](data-model.md)
- [contracts/](contracts/)
- [quickstart.md](quickstart.md)

**Tests**: Required. Constitution III says tests must not need Put.io, and bug fixes start
red, so each story writes its tests first.

## Phase 1: Setup

- [x] T001 Create the crate `putio-sync-dlna` (`Cargo.toml` with workspace package fields; deps `putio-sync-core`, `putio-sync-file-manager`, `putio-sync-client`, `tokio`, `axum` 0.7, `tower-http` 0.5 with `fs`, `socket2` 0.6, `if-addrs`, `uuid` (v4), `mime_guess`, `reqwest`, `quick-xml`, `tracing`, `prometheus`, `thiserror`, `percent-encoding`, `rand`; dev-deps `tempfile`, `wiremock`, `serde_json`) and add it to `members` in `Cargo.toml`
- [x] T002 [P] Add an empty module layout in `putio-sync-dlna/src/lib.rs` (`ssdp`, `description`, `soap`, `content`, `media`, `stream_local`, `putio`, `metrics`) with a crate doc comment naming feature 022

## Phase 2: Foundational

- [x] T003 Add `DlnaConfig` to `putio-sync-core/src/config.rs`:
  - fields `enabled` (default `false`), `name` (default `"putio-sync"`; trimmed, empty → default, cut to 64 chars), `port` (default `8200`), `bind` (`BindList`, default empty = every LAN IPv4 address, not Docker bridges), `roots` (default `["downloads"]`), `putio_folder: Option<i64>`;
  - env overrides `PUTIO_SYNC_DLNA_ENABLED`, `PUTIO_SYNC_DLNA_NAME`, `PUTIO_SYNC_DLNA_PORT`;
  - validation in `Config::validate`:
    - "1–65535, not 1900, and not the API or metrics port";
    - roots: each `downloads`, `cat:<name>` (no `/`), `watch:<path>` (one of `[watch] paths`) or `putio`; at least one if enabled; no duplicates;
    - `putio_folder` required when roots has `putio`;
  - unit tests in the same file for each rule
- [x] T004 [P] Document `[dlna]` in `config.example.toml`, exactly as in `contracts/config.md`
- [x] T005 [P] Add media classification to `putio-sync-dlna/src/media.rs`:
  - `classify(name) -> Option<MediaKind{Video,Audio,Image}>` with mime; the `mime_guess` fixes `.mkv`→`video/x-matroska`, `.ts`→`video/mp2t`, `.m4v`→`video/mp4`;
  - hidden (leading `.`) and `.downloading` names → `None`;
  - `protocol_info(mime)` and `content_features()` per research R5 (no `DLNA.ORG_PN`);
  - unit tests
- [x] T006 [P] Add the SOAP layer to `putio-sync-dlna/src/soap.rs`:
  - `parse_action(body) -> (service, action, args)` using `quick-xml`, prefix-agnostic;
  - `envelope(action, service, &[(name, value)])` with escaping;
  - `fault(code, description)` for UPnPError 401, 701 and 720;
  - unit tests with envelopes using `s:`, `SOAP-ENV:` and default namespaces
- [x] T007 [P] Add object IDs to `putio-sync-dlna/src/content.rs`:
  - `ObjectId::parse`/`format`: `"0"` root; else `percent-encode(key) + "/" + percent-encode(rel)`;
  - root keys `downloads`, `cat:<name>`, `watch:<path>`, `putio`;
  - round-trip tests, including names with spaces, `&`, `#`, `%`, emoji
- [x] T008 [P] Add DIDL-Lite building to `putio-sync-dlna/src/content.rs`:
  - containers (`id`, `parentID`, `title`, optional `childCount`) and items (`upnp:class`, one `res` with `protocolInfo`, `size`, URL);
  - every value escaped;
  - unit test that the escaped result parses back with `quick-xml`
- [x] T009 Add `SharedRoot` building to `putio-sync-dlna/src/lib.rs`: `roots(&Config) -> Vec<SharedRoot{key,title,source}>` in the order downloads, `cat:*` by name, `watch:*` in config order, `putio`; a missing `cat:`/`watch:` folder → an empty container, with a WARN logged once
- [x] T010 Add the metrics to `putio-sync-dlna/src/metrics.rs`: `putio_sync_dlna_up`, `putio_sync_dlna_streams_active{root_kind}`, `putio_sync_dlna_bytes_sent_total{root_kind}`, `putio_sync_dlna_requests_total{kind,result}` (contracts/upnp.md); register them on the shared registry the same way `putio-sync-telemetry` does

## Phase 3: User Story 1 – Play downloaded files on the TV (P1) 🎯 MVP

**Goal**: The server is found on the LAN, browses Downloads and plays files with seeking.

**Independent test**: A loopback client finds the server by M-SEARCH, browses a temp root,
sees only finished media, and gets the right bytes for a range.

### Tests

- [x] T011 [P] [US1] Write `putio-sync-dlna/tests/discovery.rs`:
  - an M-SEARCH for `urn:schemas-upnp-org:device:MediaServer:1` on loopback gets a 200 reply with `LOCATION` and `USN`;
  - `GET LOCATION` has the friendly name escaped;
  - `stop()` sends `ssdp:byebye`;
  - skip with a printed reason when multicast is not routable
- [x] T012 [P] [US1] Write `putio-sync-dlna/tests/browse.rs`:
  - root `0` lists the shared roots;
  - `downloads/` lists folders first, then files, by name case-insensitive;
  - `.x.mkv.downloading`, `.hidden.mp4`, `a.nfo` and `b.torrent` are hidden;
  - paging with `StartingIndex`/`RequestedCount` (0 = all, capped at 500) reports the full `TotalMatches`;
  - `BrowseMetadata` on an item;
  - a file added after the first browse appears on the next
- [x] T013 [P] [US1] Write `putio-sync-dlna/tests/stream.rs`:
  - full GET equals the file;
  - `Range: bytes=100-199` → 206 with those bytes and `Content-Range`;
  - a range past the end → 416 with `bytes */<size>`;
  - `HEAD` carries no body;
  - DLNA headers present;
  - a file deleted mid-stream ends the body with an error

### Implementation

- [x] T014 [US1] Write the description documents in `putio-sync-dlna/src/description.rs`: the device XML (MediaServer:1, friendlyName escaped, UDN, both services) and the static `cds.xml`/`cms.xml` SCPDs per contracts/upnp.md
- [x] T015 [US1] Add ContentDirectory `Browse` for local roots to `putio-sync-dlna/src/content.rs`:
  - decode the ObjectId, check that the root is shared now, `local_files::resolve(root, rel)`;
  - list with `std::fs::read_dir`, filter with `media::classify`; drop symlinks whose target resolves outside the root;
  - sort, page, build DIDL;
  - unknown or refused ID → fault 701, logged at WARN with `peer`, `object_id` (truncated to 200 chars) and `reason`;
  - also `GetSystemUpdateID`, `GetSearchCapabilities`, `GetSortCapabilities`
- [x] T016 [US1] Add ConnectionManager to `putio-sync-dlna/src/content.rs` or `soap.rs`: `GetProtocolInfo` (Source = every known mime's protocolInfo), `GetCurrentConnectionIDs` → `0`, `GetCurrentConnectionInfo` → static
- [x] T017 [US1] Write local streaming in `putio-sync-dlna/src/stream_local.rs`:
  - resolve the same way as Browse (refused → 404 with no body);
  - `tower_http::services::ServeFile`, plus `transferMode.dlna.org: Streaming` and `contentFeatures.dlna.org`;
  - a body wrapper that counts bytes, updates `streams_active` and `bytes_sent_total{root_kind="local"}`, and logs the end at INFO with `root`, `size`, `bytes_sent`, `duration_ms`, `result`
- [x] T018 [US1] Write the HTTP router in `putio-sync-dlna/src/lib.rs`:
  - `/description.xml`, `/cds.xml`, `/cms.xml`, `POST /ctl/cds`, `POST /ctl/cms`, `GET|HEAD /media/*id`;
  - `SUBSCRIBE`/`UNSUBSCRIBE /evt/*` → 200 with `SID` and `TIMEOUT: Second-1800`;
  - everything else 404;
  - `requests_total` per kind and result
- [x] T019 [US1] Write SSDP in `putio-sync-dlna/src/ssdp.rs`:
  - a socket per LAN IPv4 interface (from `if-addrs`; loopback, `docker0`, `br-*` and `veth*` skipped unless `bind` names them), joined to 239.255.255.250:1900 with `SO_REUSEADDR`/`SO_REUSEPORT`;
  - NOTIFY alive 3× at start, then every 900 s, `max-age=1800`; byebye on stop;
  - M-SEARCH replies for the STs in contracts/upnp.md, after a random delay of at most `min(MX,5)` s, with `LOCATION` = the receiving interface's address
- [x] T020 [US1] Add the `MediaServer` handle to `putio-sync-dlna/src/lib.rs`:
  - `start(config, client)`, `update(DlnaConfig)` per research R8, `status() -> MediaServerStatus`, `stop()`;
  - the UUID is loaded from or created in `<config dir>/dlna-uuid`;
  - binds through the `[dlna] bind` entries;
  - a bind error → `failed(reason)`, logged, and the daemon keeps running
- [x] T021 [US1] Start it in `putio-sync-daemon/src/daemon.rs`: build the `MediaServer` from config (off → idle handle), hand it to API state, call `stop()` on shutdown so byebye goes out

**Checkpoint**: US1 tests pass. VLC finds and plays from Downloads (quickstart §3).

## Phase 4: User Story 2 – Choose what is shared (P1)

**Goal**: Only the chosen roots are reachable, whatever a player sends.

**Independent test**: Every traversal and symlink case is refused, with nothing sent.

- [x] T022 [P] [US2] Write `putio-sync-dlna/tests/confine.rs`, for both Browse and `/media`:
  - `downloads/..`, encoded `%2E%2E`, `downloads//etc`, an absolute rel;
  - a symlink in the root pointing outside the root (hidden from listings, refused direct);
  - a `cat:sonarr` root that cannot reach `radarr/`;
  - an ID from a root that was unshared after `update()`;
  - each refused with fault 701 or 404, a WARN, and no file bytes
- [x] T023 [US2] Make `update()` in `putio-sync-dlna/src/lib.rs` swap the roots atomically, bump `SystemUpdateID`, and make every request check the current roots (no cache of resolved paths)
- [x] T024 [US2] Add `GET`/`PUT /settings/media-server` and `POST /settings/media-server/preview` in `putio-sync-api/src/routes/system/media_server.rs`:
  - the view with `available_roots` per contracts/api.md;
  - validate via `Config::validate`, save, then call `MediaServer::update`;
  - register in `putio-sync-api/src/routes/system/mod.rs`
- [x] T025 [P] [US2] Write API tests in `putio-sync-api/tests/media_server.rs`: auth is required; `available_roots` lists Downloads, the category folders and watch paths only when watching is on; a bad port, an unknown root or a missing `putio_folder` → 400 naming the field; a save applies without a restart
- [x] T026 [US2] Add the UI section Settings › Media server in `putio-sync-api/ui/`:
  - toggle, name, port, the bind editor from 037, shared-folder checkboxes from `available_roots`;
  - the warning "Anyone on your home network can browse and play what you share here.";
  - UI tests next to the other settings sections

**Checkpoint**: Sharing only `radarr/` hides `sonarr/` in VLC.

## Phase 5: User Story 3 – See that it works (P2)

- [x] T027 [P] [US3] Write tests in `putio-sync-dlna/tests/discovery.rs` and `putio-sync-api/tests/media_server.rs`:
  - a taken port → `failed` with the reason, and the daemon state is still healthy;
  - after freeing the port and calling `update`, the state is `running`;
  - `streams_active` is 1 during a stream;
  - `players_last_hour` counts distinct peers;
  - no metric label carries a name or path
- [x] T028 [US3] Add `GET /media-server/status` in `putio-sync-api/src/routes/system/media_server.rs`, returning `state`, `reason`, `addresses`, `streams_active`, `players_last_hour`, `putio`; set the warning "No LAN interface: players can't find it. Use host networking." when only bridges exist
- [x] T029 [US3] Add the status line to the Media server UI section in `putio-sync-api/ui/`, polled every 5 s while open

## Phase 6: User Story 4 – Stream what is still on Put.io (P2)

**Goal**: A "Put.io" root lists one Put.io folder and streams it, MP4 when there is one.

**Independent test**: Against wiremock, the listing works, the MP4 version is chosen when
converted and the original otherwise, ranges pass through, and a failure leaves the local
roots working.

- [x] T030 [P] [US4] Write `putio-sync-dlna/tests/putio.rs` (wiremock):
  - the folder listing becomes DIDL under `putio/`;
  - an item outside `putio_folder`'s tree → 701;
  - `/files/{id}/mp4` `COMPLETED` → streams `/files/{id}/mp4/download` with the player's `Range`;
  - not converted → `/files/{id}/download`;
  - `206`, `Content-Range` and `Content-Length` passed through;
  - Put.io 500 → 502 on stream and an empty root on Browse, while `downloads/` still works;
  - the listing is refreshed after the 60 s cache (paused tokio time);
  - no token or link in the response headers or body;
  - Put.io off → zero requests to the mock
- [x] T031 [US4] Add the Put.io MP4 calls to `putio-sync-client/src/files.rs` (`mp4_status(id)` for `GET /files/{id}/mp4`, `mp4_download_url(id)`), with wiremock unit tests
- [x] T032 [US4] Add the Put.io root to `putio-sync-dlna/src/putio.rs`:
  - per-folder listing cache for 60 s, capped at 200 folders;
  - the parent chain checked against `putio_folder` (filled by `get_file_details` when missing);
  - Browse items with the size of the chosen version
- [x] T033 [US4] Add the Put.io stream proxy to `putio-sync-dlna/src/putio.rs`:
  - an authenticated request with the player's `Range`, following redirects server-side, a fresh request per range;
  - pass back the status and range headers; the body is counted as `root_kind="putio"`;
  - the log has `version=mp4|original` and redacts links
- [x] T034 [US4] Add the Put.io folder picker to the Media server UI section in `putio-sync-api/ui/`, using the existing Put.io browse routes, and the `putio` state on the status line

## Phase 7: Polish

- [x] T035 [P] Update `README.md` (a Media server section) and `INSTALLATION.md` (Docker `network_mode: host` for discovery; manual `description.xml` URL with bridge networking)
- [x] T036 [P] Add a `CHANGELOG.md` entry under Unreleased › Added
- [ ] T037 Run `make ci` and fix everything it reports
- [ ] T038 Run quickstart §2 locally with curl, and §3 with VLC; note results in the spec status

## Dependencies

- Setup → Foundational → US1.
- US2 depends on US1's server (T015, T017, T020).
- US3 depends on T020.
- US4 depends on T015, T017 and T020, and is independent of US2's UI except T034.
- Polish comes last.

## Parallel examples

- Foundational: T004, T005, T006, T007 and T008 touch different files.
- US1 tests: T011, T012 and T013 together.
- US2: T022 and T025 while T024 is written.

## Implementation strategy

- MVP = Phases 1–4 (US1 + US2). Confinement ships with the first release (spec US2).
- Then US3, then US4.

## Deviations

Where the build differed from the plan, and why.

- **mockito, not wiremock** (T001, T030, T031): the repo's tests all use `mockito`, so
  the new tests do too.
- **Put.io failure on Browse is fault 720, not an empty root** (T030): `contracts/upnp.md`
  says `720` for a Put.io failure; the contract wins over the task text. Downloads still browse
  and play, and a stream request still answers `502`.
- **Listing cache tested with a short TTL** (T030): `Options.putio_ttl` (default 60 s) is set
  to 600 ms in the test and the test sleeps, instead of pausing tokio time (the mock server
  and the HTTP stack need real time).
- **Each Put.io call is capped at 10 s** (T032, T033): the client retries 5xx and network errors
  with backoff for about a minute, longer than any player waits. The cap turns that into a
  fault (Browse) or a `502` (stream) at once.
- **`bind` entries carry a port that is ignored** (T003): the `[api] bind` syntax needs one, but
  the media server always listens on `dlna.port`; only the host part of each entry is used.
- **Under `server` the settings are saved but nothing starts** (T024): `AppState.media_server` is
  attached only by `daemon`, so `PUT /settings/media-server` there saves and says so; the
  status reads `off`.
- **The configuration assistant does not list the group** (T024): `POST
  /settings/media-server/preview` works, but `GET /settings/assistant` still lists its twelve
  groups; adding a thirteenth is a separate change to its help and defaults tables.
- **Dev-dependencies** (T001): `quick-xml`, `tracing-subscriber` and `putio-sync-telemetry`
  were added for the tests, and `wiremock` was left out.
- **Real-player checks (quickstart §3 to §5) are the operator's**: nothing here can drive VLC,
  Kodi or a TV.
