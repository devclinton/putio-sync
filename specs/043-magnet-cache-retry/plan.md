# Implementation Plan: Hold Magnets and Ask the Cache Again

**Branch**: `043-magnet-cache-retry` | **Date**: 2026-10-28 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/043-magnet-cache-retry/spec.md`

## Summary

When a magnet's first cache ask misses, putio-sync accepts the *arr add but doesn't create a
Put.io transfer yet. It keeps the magnet and asks the cache again every 30 s for up to 5 minutes.

- **Found on a re-ask**: the file list is checked exactly as 042 checks it. A fake is failed and
  blocklisted with no Put.io call. A real release is added to Put.io.
- **Still missing when the hold ends**: the magnet goes to Put.io with a pending check, as 042
  does after a miss today.

With holding on, the first ask during the add gets 3 s instead of 10 s, because hits answer in
under 2 s and a miss is retried anyway.

Research ([research.md](research.md)) found that itorrents.net fetches hashes it doesn't have:
16 of 17 first-time misses were there within about 4.5 minutes. R9 covers the design questions.
Peer lookup (DHT) is out of scope.

## Technical Context

**Language/Version**: Rust 1.96 (`rust-toolchain.toml`), edition 2021. UI: ES2020 modules with no
build step.

**Primary Dependencies**: existing ones only (tokio, sqlx, reqwest, prometheus, tracing, serde).
No new crate and no new dependency.

**Storage**: SQLite. One new table, `held_magnets` ([data-model.md](data-model.md)).
`arr_items` and `release_checks` keep their schema; a held item is an `arr_items` row in state
`putio` with no transfer.

**Testing**: `cargo test`. The caches are mockito servers that miss first and serve a synthetic
.torrent later; Put.io is mockito as in 042. Time is controlled by passing `now` into the
hold pass, not by sleeping.

**Target Platform**: Linux Docker on the NAS; the same code runs on desktop builds.

**Project Type**: Cargo workspace (daemon, HTTP API, embedded web UI).

**Performance Goals**:

- a magnet add that misses answers in about 3 s plus Put.io-free bookkeeping (SC-003);
- a hold pass with nothing due costs one indexed query.

**Constraints**:

- re-asks run one at a time, oldest due first (FR-008);
- no magnet text in logs, metrics, reports or API responses (it is kept only in `held_magnets`);
- no hashes or names in metric labels.

**Scale/Scope**: tens of grabs a day; a season search can hold 20+ magnets at once.

## Constitution Check

| Principle | Status |
|---|---|
| I. Crate boundaries | The hold logic lives in `arr` (`ArrService`), next to the add and reject paths it reuses. The table and repository go in `db`, the settings in `core`, the counters in `telemetry`. The ticker that drives hold passes is a few lines of glue where `api` builds `ArrService`. The watch-folder part (US4) goes in `file-manager` when 042 US4 lands. No new edges. ✅ |
| II. Local-first | Everything runs under `make ci`. ✅ |
| III. Tests without Put.io | Caches and Put.io are mockito servers; .torrents are synthetic. No network, no real hashes. ✅ |
| IV. Download integrity | Nothing is deleted. A held reject never creates a transfer. `dry_run` makes a held reject go to Put.io, as a dry-run reject does in 042. ✅ |
| V. Observable | Two new metric families with bounded labels ([contracts/api.md](contracts/api.md)). Logs carry `hash`, `name`, `attempt`, `result`. ✅ |
| VI. Secrets | The magnet is stored locally only, as `processed_torrents` already does for watch magnets. It is never logged or returned. ✅ |
| VII. Simplicity | No dependency. Three settings, each needed by an FR (hold time, interval, first-ask limit). One table. ✅ |

Post-design re-check: unchanged.

## Design

### Add path (`ArrService::add`, `precheck`)

- `precheck` takes the first-ask limit from the new setting when holding is on
  (`min(first_lookup_secs, lookup_timeout_secs, left)`), else 042's limit.
- On `Lookup::Missed` with holding on and the release eligible (FR-002), `add` returns a new
  `Held` branch instead of adding to Put.io:
  1. insert the arr item with `transfer_id = NULL`, `state = putio`, `putio_status = "HELD"`;
  2. insert the pending check (`stage = before`, no transfer, `lookup` = the first result);
  3. insert the `held_magnets` row with the magnet, `check_id`, `next_at = now + interval`,
     `until_at = now + hold`;
  4. answer the *arr with `torrent-added`, as for any add.
- The `add_lock` and the duplicate check already cover a second add of the same hash (FR-012).

### Hold pass (`ArrService::release_held(now)`)

Runs from a ticker every 10 s (and once at start, FR-009). It takes the rows with
`next_at <= now`, oldest first, and for each:

| Case | Action |
|---|---|
| Arr item gone or no longer `HELD` | delete the hold row |
| Precheck now off or category not checked (FR-013) | send to Put.io unchecked; close the check as `passed` with reason `unchecked` |
| `now >= until_at` | send to Put.io; set the check's `transfer_id` so 042 US3 checks it on Put.io (FR-006) |
| Otherwise | `lookup(hash, lookup_timeout_secs)` |
| → found, check passes (or dry-run reject) | send to Put.io; settle the check `passed`, `lookup = found` (FR-005) |
| → found, check rejects | reuse 042's reject tail on the existing item: `set_state(Failed)`, blocklist, report; settle the check `rejected` (FR-004) |
| → missed | `attempts += 1`, `next_at = now + interval` |

"Send to Put.io" is the tail of today's `add`, factored out into one function: category folder,
`add_transfer_url`, set `transfer_id` and clear `HELD`. A refused add fails the item with Put.io's
message (FR-014).

### Other paths

- `remove`: a held item has no transfer, so the cancel/delete steps are skipped (the 042 null
  guard). It also deletes the hold row; `resolve_pending_by_hash(…, "gone")` already closes the
  check (FR-011).
- `view.rs`: `putio_status == "HELD"` maps to `STATUS_QUEUED` with `percentDone 0` and no
  error (FR-010). `follows_putio` already skips items with no transfer.
- UI: the Queue page shows held items as "Waiting for file list (n min left)" from the arr item
  list's new `held` field.

### Settings

```toml
[precheck]
hold_magnet_secs = 300       # 0 = off; max 3600
hold_retry_secs = 30         # 10..=600
first_lookup_secs = 3        # 1..=lookup_timeout_secs; used only when holding is on
```

## Project Structure

### Documentation (this feature)

```text
specs/043-magnet-cache-retry/
├── spec.md, plan.md, research.md, data-model.md, quickstart.md
├── contracts/api.md
└── checklists/requirements.md
```

### Source Code (repository root)

```text
putio-sync-core/src/config.rs                 # hold_magnet_secs, hold_retry_secs, first_lookup_secs + validation, env
putio-sync-db/src/schema.rs, src/held.rs      # held_magnets table, HeldRepository (insert, due, reschedule, delete, list)
putio-sync-db/src/releases.rs                 # settle_check (found on re-ask), set_check_transfer (hold ran out)
putio-sync-arr/src/service.rs                 # Held branch in add; release_held(now); send_to_putio factored out;
                                              # remove deletes the hold; first-ask limit
putio-sync-arr/src/view.rs                    # HELD → queued
putio-sync-api/src/lib.rs                     # spawn the hold ticker next to ArrRpc
putio-sync-api/src/routes/arr.rs              # `held` on items; settings block
putio-sync-api/src/routes/system/stats.rs     # `held` block in /stats/releases
putio-sync-api/ui/pages/{queue,stats,settings}.js  # held label, tiles, three settings
putio-sync-telemetry/src/lib.rs               # precheck_held_total{result}, precheck_held gauge
putio-sync-file-manager/src/watcher.rs        # US4, after 042 US4
config.example.toml, README.md, CHANGELOG.md
```

**Structure Decision**: no new crate. The hold is an *arr service concern built from 042's
pieces.

## Coordination

- **042**: merged first; 043 is rebased on it.
  042's `release_checks` rows gain two settle paths but no schema change.
- **011**: a new `putio_status` value `HELD`, only meaningful while `transfer_id` is NULL.
- **038 (pipeline)**: could show "Waiting for file list" as a pre-download step later.

## Complexity Tracking

None.
