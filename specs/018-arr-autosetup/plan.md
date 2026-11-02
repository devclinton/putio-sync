# Implementation Plan: Radarr/Sonarr Settings and Auto-Setup

**Branch**: `018-arr-autosetup` | **Date**: 2026-10-28 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/018-arr-autosetup/spec.md`

## Summary

The Settings → Radarr / Sonarr card becomes two editable forms that apply without a restart.
Auto-setup configures putio-sync inside each *arr through its API.

- **Download client** (US3): on/off, username, and password with Generate. A new `[arr]` writer
  in core saves it (R1), and `ArrService` swaps it in at runtime. `/transmission/rpc` now reads
  `enabled` and the password per request (R2).
- **Applications** (US4): Add, Edit, Delete and Test for Radarr and Sonarr, in the style of
  Prowlarr. They're stored as today's `[[arr.servers]]` with an optional `name`. The typed
  `category` becomes a legacy, ignored field (R3). The category is read from the *arr's own
  putio-sync client: at startup, every 15 min, and on test, save and setup. Blocklisting uses
  that value (R4).
- **Auto-setup** (US1, US2, US5):
  - A bounded discovery: keyless `/ping` plus the HTML title (R9). Manual entry is always
    available.
  - A preview of what will change.
  - Then the setup itself. It tests every candidate host with the *arr's own download-client
    test (R5) and creates or updates one Transmission client only after a pass, so a failure
    leaves nothing behind (R7).
  - It turns on the download client and generates a password server-side when needed (R11). It
    disables Torrent Blackhole only when that box is ticked, and saves the key only with
    "Remember".
- **Path check** (US6): the *arr lists the parent of the download folder after applying its
  Remote Path Mappings (R10).
- **Status** (US7): item counts and last contact move to a new card on the Status page,
  `GET /arr/status`.

## Technical Context

**Language/Version**: Rust 1.96 (`rust-toolchain.toml`), edition 2021. UI: ES2020 modules, no
build step.

**Primary Dependencies**: only existing ones (tokio, reqwest, serde, axum, toml_edit, uuid,
tracing, prometheus). No new dependency: the host discovery uses `std` and `/proc/net/route`
(R5), and the password uses `uuid` v4 (R11).

**Storage**: the config file (`[arr]`). No database changes. The category cache is in memory
([data-model.md](data-model.md)).

**Testing**: `cargo test`, with the *arr APIs mocked by mockito (already a dev-dependency of the
arr and api crates). Routes are tested through the axum router (`tests/common`). The secrets
sweep is extended to the new routes.

**Target Platform**: the Linux Docker image on the NAS. Desktop builds are unaffected:
`/proc/net/route` is skipped when it's missing.

**Project Type**: Cargo workspace (daemon, HTTP API, embedded web UI).

**Performance Goals**:

- Discovery finishes within 10 s (SC-002).
- A setup finishes in under 30 s worst case (at most 5 host tests × 5 s, plus a few API calls).
- The category refresh is 1 GET per application every 15 min.

**Constraints**:

- No API key or password in logs, metrics, errors or responses (FR-027).
- No network sweep (FR-017).
- A failed setup leaves no failing client in the *arr (FR-023).
- A config write failure leaves the running daemon unchanged.
- UI under 260 KB uncompressed (raised from 200 KB with this feature: merged with 040 and 042,
  the UI was 230 KB).

**Scale/Scope**: 1–4 applications. About 14 new or changed routes, 2 UI pages, 4 new modules.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Status | How |
|---|---|---|
| I. Crate boundaries | ✅ | The `[arr]` writer and config shape are in `core`. Discovery, setup, the category cache and the path check are in `putio-sync-arr` (new modules `discovery.rs`, `setup.rs`, `clients.rs`, `path_check.rs`). `api` holds only route glue and UI. The metric goes in `telemetry/arr.rs`. No upward dependencies. |
| II. Local-first verification | ✅ | Everything is checked by `make ci`. The Docker check in quickstart is manual and optional. |
| III. Tests without Put.io | ✅ | The *arr APIs are mocked with mockito, and no Put.io call is added. There's no bug fix here, so no red-first test is required. |
| IV. Download integrity | ✅ n/a | Nothing deletes files. Turning the client off keeps tracked items (feature 011 behaviour). |
| V. Observable | ✅ | Each setup, save, delete and client change is logged with structured fields (FR-029). New counter `putio_sync_arr_setup_total{kind,result}` with a fixed label set (R13). |
| VI. Secrets stay secret | ✅ | Keys and passwords are write-only. `Debug` stays redacted on `ArrServer` and `ArrConfig`. Error details are built without the key. The generated password is never returned. All new routes are protected, and the secrets sweep covers them. |
| VII. Simplicity | ✅ | No new dependency. The config shape is kept (no migration). The category cache is in memory. "Test before write" replaces most rollback logic (R7). |
| Config documented | ✅ | `config.example.toml` notes that `[arr]` is editable in the UI, adds the optional `name`, and marks `category` as no longer needed (FR-030). |
| Hot reload keeps downloads | ✅ | `ArrService::apply` swaps settings only. Tracked items and in-flight downloads are untouched (R2). |

**Post-design re-check (after Phase 1)**: still all pass. The design added no crate, no
dependency and no unbounded label. One point to watch during implementation: `ArrService` is
now built whenever a database exists. It must not start polling the *arr apps while the
download client is off, apart from the 15-min category refresh for saved applications, which
that feature itself needs.

## Project Structure

### Documentation (this feature)

```text
specs/018-arr-autosetup/
├── spec.md
├── plan.md              # this file
├── research.md          # R1–R13
├── data-model.md        # config shape, runtime state, setup flow
├── quickstart.md        # make ci + manual Docker check
├── contracts/
│   ├── api.md           # putio-sync routes
│   └── arr-api.md       # Radarr/Sonarr calls putio-sync makes
├── checklists/requirements.md
└── tasks.md             # /speckit-tasks (not yet)
```

### Source Code (repository root)

```text
putio-sync-core/src/config.rs        # ArrServer.name, optional category, validate(), resolve_arr env flags,
                                     # save_arr_section (R1), settings_table comment
putio-sync-arr/src/
├── arr_api.rs                       # + download_clients, schema, test_client, create/update/delete,
│                                    #   remote_path_mappings, filesystem, ping/title probes; finer error kinds
├── service.rs                       # settings in RwLock + apply(); clients cache; blocklist_server by cache
├── clients.rs        (new)          # "points at putio-sync" match (R6), category read + 15-min refresh (R4)
├── discovery.rs      (new)          # candidates, probes, tier dedupe, single-flight (R9)
├── setup.rs          (new)          # host candidates (R5), plan/preview, test-before-write, undo, blackhole (R7, R8)
└── path_check.rs     (new)          # R10
putio-sync-api/src/
├── lib.rs                           # build ArrService whenever a DB exists; apply on config change
└── routes/arr.rs                    # GET /arr, /arr/status, PUT /arr/client, applications CRUD + test,
                                     # discover, setup/preview, setup; RPC gate per request
putio-sync-api/ui/
├── api.js                           # new calls, drop testArrServer
├── pages/settings.js                # Download client form, Applications list + Add/Edit form, setup view
└── pages/status.js                  # Radarr / Sonarr card
putio-sync-telemetry/src/arr.rs      # arr_setup counter
putio-sync-api/tests/arr_settings.rs # rewritten for the new contract
putio-sync-api/tests/arr_rpc.rs      # + on/off and password change without restart
putio-sync-api/tests/secrets_sweep.rs# + new routes
config.example.toml, README.md (Radarr and Sonarr, path mapping), CHANGELOG.md
```

**Structure decision**: Use the existing crates. The feature extends feature 011's `arr` crate
and its routes. It adds no new crate.

## Complexity Tracking

None. There are no constitution violations.

## Decisions worth a look before `/speckit-tasks`

1. **The `GET /arr` shape changes**, and `POST /arr/servers/{i}/test` is replaced. The bundled
   UI is the only caller, so there's no compatibility shim (contracts/api.md, "Removed").
2. **Discovery dedupe uses tiers, not instance identity**, because the instance ID needs the key.
   Two separate Radarrs on the same port on different hosts would show as one. Manual entry
   covers that case (R9).
3. **Restoring a client relies on Servarr keeping a masked `********` password on PUT**. This is
   unverified, and quickstart step 6 checks it. The fallback is already planned (R7).
4. **The category cache isn't persisted**. After a restart it's re-read at once. Until that read
   finishes, blocklisting falls back to the legacy typed category, if there is one (R4).
