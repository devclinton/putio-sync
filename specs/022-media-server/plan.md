# Implementation Plan: Media Server (DLNA) for Local Files and Put.io

**Branch**: `022-media-server` | **Date**: 2026-11-05 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `/specs/022-media-server/spec.md`

## Summary

A new crate, `putio-sync-dlna`, makes putio-sync a UPnP AV MediaServer. It holds:

- SSDP discovery on UDP multicast;
- an HTTP server for the device and service descriptions;
- the ContentDirectory `Browse` and ConnectionManager SOAP actions;
- the media byte streams.

Local roots (US1–US3) are served from disk through 044's `local_files::resolve`, so every path
stays inside a shared root. The Put.io root (US4) lists one Put.io folder through the existing
client. Playback is proxied, with ranges, to the file's MP4 version when Put.io has one and to
the original otherwise. The daemon starts the server, Settings gets a "Media server" section,
and a save applies at once without a restart. It is off by default.

## Technical Context

**Language/Version**: Rust 1.96 (edition 2021), workspace toolchain

**Primary Dependencies**:
- Existing: `tokio` (UDP and TCP), `axum` 0.7 (HTTP), `tower-http` 0.5 (the `fs` feature is
  added for `ServeFile` ranges), `socket2` (multicast join and `SO_REUSEADDR`), `if-addrs`
  (LAN addresses), `uuid`, `mime_guess`, `reqwest` (Put.io proxy).
- New: `quick-xml` (reading SOAP requests; see research R3).

**Storage**: The server's UUID is kept in `<config dir>/dlna-uuid`. Settings live in the TOML
`[dlna]` section. No database change.

**Testing**: `cargo test`. Tests use temp roots, a loopback UPnP client written in the tests
(an SSDP M-SEARCH plus SOAP Browse over HTTP), and `wiremock` standing in for Put.io.

**Target Platform**: The Linux NAS in Docker (host network for discovery) and desktop installs.

**Project Type**: A daemon with an HTTP API and a web UI (workspace crate + Settings page).

**Performance Goals**: Discovery within 10 s. Playback starts within 3 s locally and 5 s from
Put.io. 3 streams at once without slowing downloads.

**Constraints**:
- The server adds at most 50 MB of memory.
- Streams are sent in chunks; no file is read whole.
- A Browse reads only the folder asked for.
- Labels have bounded cardinality.

**Scale/Scope**: Folders with up to thousands of entries, sent in pages. A handful of players
on one LAN.

## Constitution Check

| Principle | Check |
|-----------|-------|
| I. Crate boundaries | UPnP, SSDP and streaming go in the new `dlna` crate, which owns serving media to players. `dlna` depends on `core` (config), `file-manager` (`resolve`, the media-only filter) and `client` (Put.io listing and links). Nothing depends on `dlna` except `daemon` (starts it) and `api` (Settings, status). No upward edges. |
| II. Local-first | Everything is covered by `make ci`. The SSDP tests run on loopback multicast and skip with a message when the host has no multicast route (seen in some CI sandboxes). |
| III. Tests without Put.io | Put.io listing and proxy tests use `wiremock`. Local tests use temp dirs. |
| IV. Download integrity | Read-only. Partial `.downloading` files are never listed or served (FR-008). Nothing is deleted. |
| V. Observable | Metrics: `putio_sync_dlna_streams_active`, `putio_sync_dlna_bytes_sent_total{root_kind}`, `putio_sync_dlna_requests_total{kind,result}`. Stream start and end are logged with structured fields. The Settings status shows state and reason. |
| VI. Secrets | **Deviation, see Complexity Tracking.** The DLNA port has no auth, but it serves Put.io data in US4. Put.io links and tokens never reach players: playback is proxied, and logs redact links. |
| VII. Simplicity | Hand-written UPnP over existing axum and tokio. No UPnP framework (research R1). One new dependency, `quick-xml`. Config knobs: `enabled`, `name`, `port`, `bind`, `roots`, `putio_folder`. Each answers a spec requirement. |

Post-design re-check: passes. The deviation from VI is recorded below.

## Project Structure

### Documentation (this feature)

```text
specs/022-media-server/
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── upnp.md          # SSDP, descriptions, Browse, stream URLs
│   ├── config.md        # [dlna] TOML section
│   └── api.md           # Settings + status routes
└── tasks.md             # /speckit-tasks
```

### Source Code (repository root)

```text
putio-sync-dlna/                 # new crate
├── Cargo.toml
├── src/
│   ├── lib.rs                   # MediaServer handle: start, update(config), status, stop
│   ├── ssdp.rs                  # NOTIFY alive/byebye, M-SEARCH replies
│   ├── description.rs           # device + service XML (static templates, escaped name)
│   ├── soap.rs                  # parse action, build envelopes and faults
│   ├── content.rs               # Browse: object ids ↔ (root, rel), paging, DIDL-Lite
│   ├── media.rs                 # media-only filter, DLNA profile and mime per extension
│   ├── stream_local.rs          # ranged file stream through resolve
│   ├── putio.rs                 # Put.io root: cached listings, MP4 choice, ranged proxy
│   └── metrics.rs
└── tests/
    ├── discovery.rs             # M-SEARCH finds it; byebye on stop
    ├── browse.rs                # roots, sorting, paging, hidden/partial/non-media
    ├── confine.rs               # traversal, encoded `..`, symlink escape, unshared root
    ├── stream.rs                # ranges, 416, file removed mid-stream
    └── putio.rs                 # wiremock: listing, MP4 vs original, failure, cache expiry

putio-sync-core/src/config.rs    # DlnaConfig + validation + defaults
putio-sync-file-manager/src/local_files.rs  # reuse resolve; expose is_partial/is_hidden helpers
putio-sync-daemon/src/daemon.rs  # build MediaServer, hand to API state
putio-sync-api/src/routes/system/media_server.rs  # GET/PUT /settings/media-server, GET /media-server/status
putio-sync-api/ui/…              # Settings › Media server section
config.example.toml, README.md, INSTALLATION.md (Docker host network), CHANGELOG.md
```

**Structure Decision**: a new workspace crate, because serving UPnP is a new concern that
belongs to no existing crate (Principle I). The API and daemon only wire it up.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| VI: Put.io data is served without auth on the DLNA port (US4) | DLNA has no authentication: TVs and players cannot send tokens. Every home DLNA server works like this. | Putting a token in the stream URLs would leak in player logs and still be readable on the LAN. Leaving US4 out drops the roadmap's reason for 022. Mitigations: Put.io sharing is off by default and opt-in on its own; one chosen folder only; read-only; the port can be bound to one LAN address; and Put.io links and tokens are never sent to players. |
