# Plan: qBittorrent API emulation (067)

## Approach

Mirror 011: the arr crate builds the answers, the API crate owns HTTP and sign-in.

1. `putio-sync-arr/src/service.rs`: `Snapshot` (item, local progress, pipeline gate, added
   and changed times) is what both views are built from. `snapshots`, `add_in` (an add with
   a category and a save path), `categories` and `files_on_disk` are new;
   `torrents()` (Transmission) maps the same snapshots.
2. `putio-sync-arr/src/qbit.rs`: pure functions from a `Snapshot` to qBittorrent's JSON:
   `state`, `matches_filter`, `torrent`, `properties`, `files`, `trackers`, `categories`,
   `transfer_info`, `preferences`, `build_info`, `maindata`. Unit tests cover the mapping.
3. `putio-sync-db`: `ArrRepo::times` for `added_on` and `completion_on`.
4. `putio-sync-api/src/routes/qbit.rs`: `Sessions` (the SID store) and one handler for
   `/api/v2/*path`. It parses the query, a form body or a multipart body by hand (axum's
   multipart feature is not enabled), checks the session, and dispatches.
5. Sign-in reuses `rpc_limiter`, `token_matches` and `client_ip` from 011. Metrics reuse
   `putio_sync_arr_rpc_requests_total` and its labels.
6. `GET /arr` gains `client.qbittorrent`; Settings shows the type and URL under the
   Transmission details.

## Tests

- Unit: state and filter mapping, fixed fields, files, maindata; the session store
  (expiry, cap, password change), the multipart parser.
- `putio-sync-api/tests/qbit_api.rs`: sign-in and its security through the router, then one
  contract test per tool (Radarr/Sonarr, autobrr, cross-seed, phone apps) with Put.io
  mocked.

## Risks

- The request shapes are from memory. A tool that sends something else gets a 404 or a
  400 with a plain body, and the Status page shows that it called.
