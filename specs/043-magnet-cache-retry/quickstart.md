# Quickstart: Hold Magnets and Ask the Cache Again (043)

## Automated (all local, `make ci`)

| Scenario | Test | Expected |
|---|---|---|
| Miss then hit, real release | `putio-sync-arr/tests/hold.rs` | add makes no Put.io call; after `release_held(now + 30 s)` with the cache serving a passing .torrent, one `transfers/add` and the check `passed` |
| Miss then hit, fake | same | after the re-ask, item `failed`, blocklisted, report row, no Put.io call |
| Hold runs out | same | at `now + 300 s`, `transfers/add` called, check stays `pending` with the transfer id |
| Stalling cache, holding on | same | add returns in about `first_lookup_secs`; holding off gives the full `lookup_timeout_secs` |
| Remove while held | same | no Put.io call, hold row gone, check `gone` |
| Restart | same, new `ArrService` on the same DB | hold continues; an expired hold is released on the first pass |
| Burst | same, 20 held | the pass asks one at a time, oldest first |
| dry_run | same | a failing re-ask goes to Put.io, logged as "would reject" |
| Queue view | `putio-sync-arr/src/view.rs` unit test | `HELD` → status 3, 0%, no error |
| Settings | `putio-sync-core` config tests | defaults, ranges, `first_lookup_secs > lookup_timeout_secs` rejected |
| Metrics | telemetry contract test | new families and the `ask` label present |

Caches are mockito servers that answer 404 (or stall) on the first request and 200 with a
synthetic .torrent later. The hold pass takes `now` as an argument, so no test sleeps.

## Manual, on the NAS

1. Deploy with the defaults and grab a magnet-only release in Sonarr.
2. Sonarr's queue shows it as queued; the Queue page shows "Waiting for file list".
3. Within a few minutes it moves to Put.io (or fails as rejected).
4. Read `putio_sync_precheck_held_total` after a week: `found_*` should be most of it (SC-002).
