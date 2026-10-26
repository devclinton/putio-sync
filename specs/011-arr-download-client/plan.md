# Implementation Plan: Radarr/Sonarr Download Client

**Branch**: `011-arr-download-client` | **Date**: 2026-10-26 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/011-arr-download-client/spec.md`

## Summary

putio-sync answers the Transmission RPC subset that Radarr's and Sonarr's built-in Transmission
client uses, at `POST /transmission/rpc` on the existing API port. The route has its own Basic
auth and the session-id handshake.

- **Adding**: an add request hashes the magnet or .torrent (the same info hash the *arr
  computes), creates the Put.io transfer in the folder named by the *arr's category (the path
  below the reported download folder), and records the item in a new `arr_items` table.
- **Reporting state**: a lazy, single-flight refresh, run at most every 20 s when the *arr
  polls, does the following:
  - moves each item through the states `putio` → `local` → `done`, or → `failed`;
  - queues a completed transfer's subtree right away through the sync's own filters, and
    records the files it accepted;
  - reports an item finished only when every accepted file is complete on disk.
- **Failures**: they go back as `errorString`. With an optional `[[arr.servers]]` API key for
  the category, putio-sync also asks the *arr to remove the item with blocklisting.
- **Removal**: removal from the *arr cancels the transfer, deletes the Put.io files, drops
  pending downloads and, with "delete data", deletes the local output.
- **Settings**: a Settings card shows the values to enter in the *arr apps, when each *arr last
  made contact, and a connection test for each configured *arr API.

Decisions are in [research.md](research.md) (R1–R12).

## Technical Context

**Language/Version**: Rust 1.96 (`rust-toolchain.toml`), edition 2021. UI: ES2020 modules, no
build step.

**Primary Dependencies**: existing ones (tokio, reqwest, serde, axum, sqlx, prometheus, tracing,
base64). One new dependency: `sha1 = "0.10"` (R4).

**Storage**: SQLite, with two new tables, `arr_items` and `arr_item_files`
([data-model.md](data-model.md)).

**Testing**: `cargo test`. Put.io and the *arr APIs are mocked with mockito, which is already
used. The RPC is tested through the axum router (the pattern used in existing api tests). The
local download state is seeded straight into the test DB.

**Target Platform**: Linux Docker on the NAS; desktop builds are unaffected.

**Project Type**: Cargo workspace (daemon, HTTP API, embedded web UI).

**Performance Goals**: at most one Put.io `transfers/list` call per 20 s while an *arr polls.
`torrent-get` without a refresh answers from SQLite.

**Constraints**:

- No secret, magnet link or .torrent content in logs.
- No hash or name in metric labels.
- Local deletes only under the target directory.

**Scale/Scope**: tens of items in flight, hundreds tracked.

## Constitution Check

| Principle | Status |
|---|---|
| I. Crate boundaries | New crate `putio-sync-arr` owns the *arr integration: info hash, the Transmission model and dispatch, the item service, and the *arr API client. It depends on `client`, `db`, `download`, `core` and `telemetry`. The new tables and repository go in `db`; config in `core`; new Put.io calls (`create_folder`, upload from bytes) in `client`; the per-file decision report in `download`; metrics in `telemetry`. `api` holds only the HTTP glue (auth gate, handshake, JSON in and out) and the `/arr` routes. No upward dependencies. ✅ |
| II. Local-first | `make ci` covers everything. ✅ |
| III. Tests without Put.io | mockito for Put.io and the *arr apps; synthetic .torrent bytes; DB-seeded local state. ✅ |
| IV. Download integrity | "Finished" requires every accepted file to be `completed` in `downloads`, which is only written after the atomic rename. Put.io deletes before a local copy exists happen only on an explicit *arr remove (see Complexity Tracking). Local deletes are confined to the item's own output path. ✅ (one recorded deviation) |
| V. Observable | Three metric families with bounded labels; structured logs with `hash`, `name` and `transfer_id`. ✅ |
| VI. Secrets | The RPC has its own auth; the `/arr` routes sit behind the bearer token. Password and API keys are redacted in `Debug`, kept by settings saves, absent from responses (tested by extending `tests/secrets_sweep.rs`). ✅ |
| VII. Simplicity | One new dependency, justified (R4). A lazy refresh instead of a daemon loop (R7). No path-mapping knob (R5). A new crate, justified by I: this is a distinct integration, and putting it in `api` would breach I. ✅ |

Post-design re-check: unchanged.

## Project Structure

### Documentation (this feature)

```text
specs/011-arr-download-client/
├── spec.md, plan.md, research.md, data-model.md, quickstart.md
├── contracts/transmission-rpc.md, contracts/api.md
├── checklists/requirements.md
└── tasks.md
```

### Source Code (repository root)

```text
Cargo.toml                                   # add member putio-sync-arr
putio-sync-arr/                              # NEW crate
  src/lib.rs                                 # pub use; ArrService
  src/infohash.rs                            # magnet btih (hex/base32), bencode info span + SHA-1
  src/rpc.rs                                 # Transmission request/response types, dispatch(method,args)
  src/service.rs                             # add/list/remove/refresh, state machine, local delete
  src/view.rs                                # item → Transmission torrent fields (pure)
  src/arr_api.rs                             # *arr v3 client: system/status, queue/details, queue delete
  src/contact.rs                             # last-contact map by User-Agent
  tests/service.rs                           # mocked Put.io + temp DB + temp dir
  tests/arr_api.rs                           # mocked *arr
putio-sync-core/src/config.rs                # [arr] ArrConfig, ArrServer, env overrides, redaction, save keeps secrets
putio-sync-client/src/files.rs               # create_folder; upload from bytes
putio-sync-db/src/schema.rs, src/arr.rs      # arr_items, arr_item_files, ArrRepository
putio-sync-download/src/manager.rs           # enqueue_tree_report(root) -> per-file decisions
putio-sync-telemetry/src/arr.rs              # three families + contract test entry
putio-sync-api/src/lib.rs                    # AppState.arr; public /transmission/rpc; protected /arr
putio-sync-api/src/routes/arr.rs             # rpc gate + handshake; GET /arr; POST test
putio-sync-api/tests/arr_rpc.rs              # handshake/auth/add/get/remove through the router
putio-sync-api/tests/arr_settings.rs         # /arr + test endpoint, no secret leak
putio-sync-api/ui/pages/settings.js          # "Radarr / Sonarr" card (own render function)
putio-sync-api/examples/mock_ui.rs           # fixtures for the card
config.example.toml, README.md               # [arr] section, setup steps, metrics
```

**Structure Decision**: existing workspace plus one integration crate.

## Coordination

- **009 (folder rules)**: rebase after it merges. `enqueue_tree_report` must expose the rule's
  decision (accepted, blocked with its extension, skipped). This feature only uses
  "accepted / not accepted + reason". The local path is unchanged, because 009 keeps local
  path = Put.io path.
- **012 (UI themes)**: the Settings card uses existing classes and lives in its own function to
  keep merge conflicts small.
- **010 (doctor)**: no overlap expected.

## Complexity Tracking

| Deviation | Why needed | Simpler alternative rejected because |
|---|---|---|
| IV: an *arr remove of an unfinished item deletes its Put.io files before any local copy exists | The *arr remove is the user cancelling the release (a bad grab, or a manual remove) | Keeping the files would leave orphans that the sync then downloads, which defeats the remove. Without "delete data", unfinished items keep their Put.io files (FR-017). |
| IV: no `--dry-run` for the remove | The remove is triggered by the *arr over the RPC, per item | There is no CLI surface to put a flag on. Each remove is logged with the hash, name and transfer id. |
| New crate | Constitution I | Putting it in `api` breaks "behavior not in api glue", and `daemon` is not used in `server` mode. |
