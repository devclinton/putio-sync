# Research: Media Server (DLNA)

## R1. UPnP stack: write it, or use a crate

- **Decision**: Hand-write the MediaServer on axum and tokio.
- **Rationale**: A MediaServer that players accept needs a small set of pieces:
  - SSDP NOTIFY and M-SEARCH replies;
  - two static description documents;
  - ContentDirectory `Browse`, `GetSearchCapabilities`, `GetSortCapabilities` and
    `GetSystemUpdateID`;
  - ConnectionManager `GetProtocolInfo`, `GetCurrentConnectionIDs` and
    `GetCurrentConnectionInfo`.

  That is a few hundred lines on top of what the workspace has. The Rust UPnP crates found
  (`rupnp`, `upnp-client`) are control points (clients), not servers, and the server-side
  crates are unmaintained.
- **Alternatives**:
  - Bundling minidlna or gmediarender: a second process, no confinement to 044's rules, and no
    Put.io root.
  - Rust UPnP crates: client-side, or abandoned.

## R2. SSDP

- **Decision**:
  - Multicast `239.255.255.250:1900`, joined on each LAN IPv4 interface from `if-addrs`.
    Loopback and Docker bridges (`docker0`, `br-*`, `veth*`) are skipped unless `bind` names
    them.
  - Sockets use `SO_REUSEADDR`/`SO_REUSEPORT` (socket2), so the server works next to another
    SSDP user, such as the NAS's own.
  - NOTIFY `ssdp:alive` goes out 3 times at start, then every 15 min with `CACHE-CONTROL:
    max-age=1800`. NOTIFY `ssdp:byebye` goes out on stop and on disable.
  - The server answers M-SEARCH for `ssdp:all`, `upnp:rootdevice`, the device UUID,
    `urn:schemas-upnp-org:device:MediaServer:1`, and both service types. It waits a random
    delay up to `MX` (capped at 5 s).
  - `LOCATION` uses the address of the interface that got the search, so a player on the LAN
    never gets the Docker bridge address.
- **Rationale**: This is the UPnP Device Architecture 1.1 behavior that TVs expect. Choosing
  the address per interface handles the NAS with several networks (spec edge case).
- **IPv6**: not in this feature. Every target player supports IPv4 SSDP.

## R3. SOAP parsing

- **Decision**:
  - Use `quick-xml` to read the action name and arguments from the envelope.
  - Write responses from string templates, escaping every value.
  - DIDL-Lite is escaped and then embedded as a string in `Result`, so it is escaped twice, as
    the spec requires.
- **Rationale**: Players send namespaced envelopes with varying prefixes. Hand-splitting
  strings breaks on that, and `quick-xml` is small and has no dependencies of its own. Writing
  needs no library.

## R4. Object IDs

- **Decision**:
  - `0` is the root container.
  - Every other ID is `<root-key>/<rel path>`, percent-encoded as one opaque string. `root-key`
    is `downloads`, `cat:<name>`, `watch:<n>` or `putio`.
  - Put.io items use `putio/<file id>`, and the parent ID comes from the cached listing.
  - Each request decodes the ID and checks it again: the root must be shared now, and local
    paths go through `local_files::resolve`.
- **Rationale**: IDs that stay stable across restarts let players keep bookmarks (FR-005, US2
  scenario 4). Because the path is checked on each request, a remembered ID never gets around
  a root that has been unshared. No ID table is kept.

## R5. Media filter and DLNA profiles

- **Decision**:
  - The extension decides the media class: `object.item.videoItem`, `audioItem` or
    `imageItem`. The mime comes from `mime_guess`, with fixes players need: `.mkv` →
    `video/x-matroska`, `.ts` → `video/mp2t`, `.m4v` → `video/mp4`.
  - `protocolInfo` is `http-get:*:<mime>:DLNA.ORG_OP=01;DLNA.ORG_CI=0;DLNA.ORG_FLAGS=01700000000000000000000000000000`.
    It sets no `DLNA.ORG_PN` profile, because a wrong profile makes some TVs refuse a file they
    could play.
  - Hidden files (leading `.`), partials (`.downloading`), symlinks that resolve outside the
    root, and other extensions are left out.
- **Rationale**: Omitting `PN` is what minidlna and Jellyfin do for files they don't probe.
  `OP=01` advertises byte-range seeking.

## R6. Local streaming

- **Decision**:
  - Use `tower_http::services::ServeFile` on the resolved path. It handles `Range`, `416`,
    `HEAD`, `Last-Modified` and `If-Range`.
  - DLNA headers are added: `transferMode.dlna.org: Streaming` and `contentFeatures.dlna.org`
    (the same string as protocolInfo's fourth field).
  - The body is wrapped to count bytes for metrics and for the stream log.
- **Rationale**: This reuses tested range handling. A file removed mid-stream ends the body
  with an I/O error, so the connection closes and the player stops (spec edge case).

## R7. Put.io root

- **Decision**:
  - Listing calls `list_files(parent_id)` from the client. The result is cached per folder for
    60 s (FR-025 allows up to 5 min) and capped at 200 folders.
  - The root is the configured `putio_folder` (a folder ID). An item's parent chain must reach
    it, which is checked against the cache and filled in by a `get_file_details` walk when
    missing. Otherwise the request is refused.
  - On play, the server calls `GET /files/{id}/mp4`. When its status is `COMPLETED`, it
    streams `/files/{id}/mp4/download`; otherwise `/files/{id}/download`.
  - The proxy uses the authenticated client with the player's `Range` header. It follows the
    redirect on the server side and passes back `206`, `Content-Range` and `Content-Length`.
    It sends a fresh request for each range, so expired CDN links don't matter (spec edge
    case).
  - Browse results report the size of the chosen version, so seeking maths agree.
- **Rationale**: Proxying keeps the token and the signed links off the LAN (Complexity
  Tracking). It costs NAS bandwidth equal to the stream rate, which is fine on a home LAN.
- **Alternative**: a 302 redirect to Put.io's CDN. Rejected: many TVs don't follow redirects
  for media, and the link would be visible to every player.

## R8. Hot apply and identity

- **Decision**:
  - `MediaServer::update(DlnaConfig)` compares old and new settings:
    - a change of name or roots only swaps shared state, and the next NOTIFY carries the new
      name;
    - a change of port or bind rebinds, after a byebye;
    - disabling sends byebye, closes the listeners and keeps playing streams only until their
      root is checked again on the next range request.
  - The UUID is made once and saved to `<config dir>/dlna-uuid`.
- **Rationale**: FR-002 and FR-005. Streams of roots that are still shared survive a settings
  save (spec edge case).

## R9. Docker

- **Decision**: The docs say to use `network_mode: host` for discovery. When the container's
  only interfaces are bridges, the status shows "running, but players can't find it (no LAN
  interface; use host networking)". With a bridge, a player can still be pointed at
  `http://<nas>:<port>/description.xml` by hand.
- **Rationale**: Multicast does not cross the Docker bridge. Saying so in the UI saves a
  support round trip.
