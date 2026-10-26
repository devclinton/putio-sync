# Quickstart: Radarr/Sonarr Download Client (011)

## Automated

```bash
TMPDIR=<scratch> make ci
```

The tests that cover this feature:

- `putio-sync-arr` unit tests: info hash from hex and base32 magnets, and from .torrent bytes,
  checked against a known hash; the item view mapping for each state.
- `putio-sync-arr/tests/service.rs`:
  - add → `putio` → `local` → `done`;
  - a blocked-only item → `failed` with a blocklist call;
  - remove with and without "delete data";
  - restart persistence (a new service over the same DB).
- `putio-sync-arr/tests/arr_api.rs`: status, queue lookup by hash, delete with blocklist;
  401 and unreachable.
- `putio-sync-api/tests/arr_rpc.rs`: the 404 / 503 / 401 / 409 gate, then a full *arr-style
  session: session-get, add a magnet with download-dir `<target>/sonarr`, get, remove.
- `putio-sync-api/tests/arr_settings.rs`: `GET /arr` and the server test. The secrets sweep
  covers the password and API keys.

## Manual, against the mock UI

```bash
cargo run -p putio-sync-api --example mock_ui
```

Open Settings and check that the "Radarr / Sonarr" card shows the connection values, the last
contact times and the Test buttons.

## Manual, with a real Radarr/Sonarr (only with the user's OK)

1. Set `[arr] enabled = true` and `PUTIO_SYNC_ARR_PASSWORD`, then restart putio-sync.
2. In Sonarr, go to Settings → Download Clients → + → Transmission:
   - Host: the NAS IP; Port: 15432; URL Base: `/transmission/`;
   - Username: `putio-sync`; Password: the value set above; Category: `sonarr`.

   Click Test, which should pass. Disable Torrent Blackhole once this works, or keep it at a
   lower priority.
3. Grab one episode. The Activity queue should show it downloading, then completed, then
   imported, and then gone from Put.io and `/downloads/sonarr`.
4. Optional: add `[[arr.servers]]` for Sonarr with its API key, then use Settings → Test.
