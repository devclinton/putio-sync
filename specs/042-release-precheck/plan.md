# Implementation Plan: Release Pre-check

**Branch**: `042-release-precheck` | **Date**: 2026-10-28 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/042-release-precheck/spec.md`

## Summary

Before a release reaches Put.io, putio-sync reads its file list. It rejects the release when no
file is a wanted type (video by default, or the 009 rule's allow list) of at least the minimum
size.

**Where the file list comes from**:

- **.torrent files**: read locally from the bytes.
- **URLs**: the .torrent is fetched locally, as today. A redirect to `magnet:` is now followed.
- **Magnets**: looked up by info hash from public .torrent caches (itorrents.net by default),
  within 10 s, and kept only when the hash matches. A miss goes to Put.io unchecked. It is
  checked there when the transfer completes, before the sync can queue a file.

**What happens to a rejected release**:

- **From Radarr/Sonarr**: the add is accepted, but no transfer is created. The item is tracked as
  failed, and 011's blocklist path makes the *arr search again. Radarr skips a release whose add
  fails without blocklisting it, so refusing the add would loop.
- **From the watch folder**: the file is renamed `.rejected`.

**Recording**:

- Every check is recorded (`release_checks`), and so are every rejection and operator flag
  (`release_reports`).
- Metrics and a new Stats endpoint show rejections, bytes avoided, cache hit rate and how often
  the magnet fallbacks turn out to be fake.
- The web UI can flag a release "Fake", which can also fail and blocklist it, or "Not fake". The
  reports export as CSV/JSON.

Decisions are in [research.md](research.md) (R1–R12).

## Technical Context

**Language/Version**: Rust 1.96 (`rust-toolchain.toml`), edition 2021. UI: ES2020 modules with no
build step.

**Primary Dependencies**: existing ones only: tokio, reqwest (no new features: R5 asks for identity
encoding), serde, serde_json, sqlx, axum, prometheus, tracing, sha1. `sha1` moves from
`putio-sync-arr` to the new crate.

**Storage**: SQLite. Two new tables, `release_checks` and `release_reports`
([data-model.md](data-model.md)). The `arr_items` schema doesn't change.

**Testing**: `cargo test`, with mockito for Put.io, the caches, indexer URLs and the *arr API.
The .torrent fixtures are synthetic. API tests go through the axum router, and the UI is checked
with `examples/mock_ui.rs`.

**Target Platform**: Linux Docker on the NAS. Desktop builds get the same checks for the watch
folder.

**Project Type**: Cargo workspace (daemon, HTTP API, embedded web UI).

**Performance Goals**:

- a .torrent or URL check adds under 5 ms of CPU to an add;
- a magnet check takes at most 10 s and about 10–100 KB of traffic;
- the sync gate costs one indexed `EXISTS` query per sync pass when nothing is pending.

**Constraints**:

- the *arr add answers within 15 s (SC-003);
- no URL, magnet or .torrent bytes in logs, reports or responses;
- no names or hashes in metric labels.

**Scale/Scope**:

- tens of grabs a day;
- reports kept 400 days, so a few thousand rows.

## Constitution Check

| Principle | Status |
|---|---|
| I. Crate boundaries | **New crate `putio-sync-precheck`** (R1) owns .torrent reading, the rule and source lookups. It depends on `core` (config, `RuleSet`), `client` (`models::tracker_host`, the read-only tree walk and `transfers/list`) and `telemetry`. `core` already depends on `client`, so this adds no new edge in the crate graph. Used by `arr` and `file-manager`. **Tables and repositories**: in `db`. **Config**: in `core`. **The sync gate**: in `download`. **Flag and stats routes**: thin glue in `api`. No upward dependencies. ✅ |
| II. Local-first | Everything runs under `make ci`. ✅ |
| III. Tests without Put.io | mockito covers Put.io, the caches and the indexers, and the .torrents are synthetic. The live check of R3 is a manual quickstart step only. ✅ |
| IV. Download integrity | Nothing is downloaded locally for a rejected release. The check on Put.io, and "Flag Fake + fail and blocklist", delete Put.io files that will never be downloaded (see Complexity Tracking). `dry_run` covers both destructive paths. ✅ (one recorded deviation, two paths) |
| V. Observable | Seven metric families with bounded labels (R12). Structured logs carry `hash`, `name`, `source`, `stage` and `reason`. ✅ |
| VI. Secrets | Indexer URLs carry API keys and are never logged or stored. Cache requests send only the hash. `secrets_sweep` is extended. ✅ |
| VII. Simplicity | No new dependency. One new crate (justified). The config knobs are each needed by a spec FR. There is one cache by default, and more only if the telemetry shows a need. ✅ |

Post-design re-check: unchanged.

## Project Structure

### Documentation (this feature)

```text
specs/042-release-precheck/
├── spec.md, plan.md, research.md, data-model.md, quickstart.md
├── contracts/api.md
└── checklists/requirements.md
```

### Source Code (repository root)

```text
Cargo.toml                                    # add member putio-sync-precheck
putio-sync-precheck/                          # NEW crate
  src/lib.rs                                  # Prechecker: check(source, policy) -> Outcome
  src/metainfo.rs                             # bencode reader, info hash (moved from arr), files, private, trackers
  src/rule.rs                                 # Policy, Verdict, Reason, archive/executable tables (pure)
  src/policy.rs                               # Policy from [precheck] + RuleSet for a folder (R9)
  src/fetch.rs                                # URL fetch with magnet-redirect stop (moved from ArrService)
  src/lookup.rs                               # cache lookup with total deadline, hash check
  tests/metainfo.rs, tests/rule.rs, tests/lookup.rs
putio-sync-arr/src/infohash.rs                # keeps from_magnet; from_torrent re-exported from precheck
putio-sync-arr/src/service.rs                 # add: precheck → reject as failed item (R7) or add + pending check;
                                              # refresh: check on Put.io before move_to_local; skip null transfer_id
putio-sync-file-manager/src/watcher.rs        # precheck before upload; .rejected rename; pending check for fallbacks;
                                              # check-on-Put.io pass at scan start (R8)
putio-sync-download/src/manager.rs            # sync gate: skip files of transfers with a pending check (R8)
putio-sync-db/src/schema.rs, src/releases.rs  # release_checks, release_reports, ReleasesRepository, prune
putio-sync-core/src/config.rs                 # [precheck] PrecheckConfig, overrides, validation, env, save keeps comments
putio-sync-telemetry/src/precheck.rs          # R12 families + contract test entries
putio-sync-api/src/routes/releases.rs         # flags (POST/DELETE/GET), reports export
putio-sync-api/src/routes/system/stats.rs     # GET /stats/releases
putio-sync-api/src/routes/system/settings.rs  # precheck block + effective view
putio-sync-api/ui/pages/stats.js              # Rejected releases + Magnet lookups tiles, reports summary
putio-sync-api/ui/pages/{queue,history,putio}.js  # Flag menu (Fake / Not fake / fail and blocklist), Flagged badge
putio-sync-api/ui/pages/settings.js           # "Release check" card (own render function)
putio-sync-api/examples/mock_ui.rs            # seeded checks, reports, flags
putio-sync-daemon/src/daemon.rs               # daily prune of release tables
config.example.toml, README.md, CHANGELOG.md  # [precheck] docs, metrics, the 60 GB story
```

**Structure Decision**: the existing workspace plus one checking crate, shared by the *arr
integration and the watch folder.

## Coordination

- **011 (arr download client)**:
  - rejected items have a NULL `transfer_id` (R7), so `refresh` and `remove` need guards;
  - `fetch_torrent` moves into the precheck crate;
  - update the 011 data-model note.
- **009 (folder rules)**: read-only use of `RuleSet::resolve`. No change to 009.
- **006 (stats)**:
  - new endpoint `/stats/releases`, so `/stats/summary` doesn't change;
  - the tiles reuse the page's range control;
  - tracker host extraction reuses `putio_sync_client::models::tracker_host`.
- **034 (download verification) and 038 (pipeline)**: none now. 038 can show the check as a
  "Pre-check" step later.
- **039–041 (other worktrees)**: they touch `settings.js` and `stats.js`. Keep the new UI in its
  own render functions to keep merges small.

## Complexity Tracking

| Deviation | Why needed | Simpler alternative rejected because |
|---|---|---|
| IV: the check on Put.io, and "Flag Fake + fail and blocklist", delete an item's Put.io files with no local copy | They are the fake files the feature exists to drop. Keeping them means the regular sync, or a later rule change, downloads them. The flag path is an explicit operator choice on one *arr item | Leaving them for the operator to delete brings back the storage and NAS-traffic problem. `dry_run = true` checks, flags and counts without deleting, on both paths. Each delete is logged with the hash, name and transfer id |
| New crate | Constitution I: shared by `arr` and `file-manager`, which can't depend on each other | `core` would take on network behaviour, and copying the parser into each would duplicate it |
| Sync gate in `download` | Without it, the sync can download a fallback's files before the check on Put.io (R8) | A staging folder on Put.io needs a move per item and conflicts with 009 paths |
