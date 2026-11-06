# Implementation Plan: Radarr/Sonarr automatic setup including path mapping

**Branch**: `claude/arr-auto-setup-645779` | **Date**: 2026-11-05 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/049-arr-auto-setup/spec.md`

## Summary

018 adds putio-sync's download client to Radarr/Sonarr and checks, through the app's existing
Remote Path Mappings, whether it can see the download folder. 049 closes the gap:

- **Folder step** (US1, US3): putio-sync puts a uniquely named marker folder in its download
  folder and asks the app's filesystem browser where it sees that marker (R1, R2). From the
  answer it proposes one change to the app's Remote Path Mappings: create, update the existing
  mapping for putio-sync's host, or remove a mapping that hides a folder already visible at the
  same path (R3). The change is written only after the operator confirms, and the route
  re-runs the analysis before writing, so a rerun never duplicates (R4). The path check runs
  again afterwards and is kept on the saved application's row.
- **Recommended settings** (US4): Completed Download Handling and Import Extra Files with `srt`
  are read from the app's config and each that is off gets a Turn on button (R5).
- **Guided Set up** (US2): one UI flow runs the 018 setup, then the folder step, then the
  recommendations, with a result line per step. It is on every saved application with a key
  and follows a successful "Set up automatically" (R6).

## Technical Context

**Language/Version**: Rust (workspace toolchain), UI ES2020 modules without a build step.

**Primary Dependencies**: existing only (reqwest, serde, axum, uuid, tracing, prometheus).

**Storage**: none new. The path check result is kept in memory in `ClientInfo` as in 018.

**Testing**: `cargo test`; Radarr/Sonarr mocked with mockito in the arr crate (unit) and through
the axum router in `putio-sync-api/tests/arr_settings.rs`; the secrets sweep covers the new
routes.

**Target Platform**: the Linux Docker image on the NAS; any OS for the daemon.

**Project Type**: Cargo workspace (daemon, HTTP API, embedded web UI).

**Performance Goals**: the folder search lists at most 60 folders and stops after 20 s
(SC-003); typical Docker layouts are found in 2–4 listings.

**Constraints**: UI bundle under 416 KiB (about 15 KiB left), so the UI is one compact step
renderer in `ui/settings/arr.js`. No changes to plugin UI files (048).

**Scale/Scope**: a handful of applications per install; one search at a time per request.

## Constitution Check

- **I. Crate boundaries**: search, proposal, mapping writes and recommendations live in
  `putio-sync-arr` (`path_mapping.rs`, `recommend.rs`, `arr_api.rs`). The api crate only
  resolves the target and wires routes. The metric is in `putio-sync-telemetry`. PASS
- **II. Local-first verification**: `make ci` stays the gate. PASS
- **III. Tests without Put.io**: everything is tested against mock Radarr/Sonarr servers and a
  temp download folder. PASS
- **IV. Download integrity**: the marker is a new empty folder with a random name, removed by a
  drop guard; nothing else in the download folder is touched. No Put.io data involved. PASS
- **V. Observable**: mapping changes are logged with app, action and paths, and counted by
  `putio_sync_arr_path_mapping_total{result}` (bounded labels). PASS
- **VI. Secrets**: the API key stays in the request header; responses and logs carry no key or
  password; the secrets sweep is extended. PASS

Post-design re-check: unchanged, PASS.

## Project Structure

### Documentation (this feature)

```text
specs/049-arr-auto-setup/
├── spec.md
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── api.md        # putio-sync routes
│   └── arr-api.md    # Radarr/Sonarr calls
├── checklists/requirements.md
└── tasks.md
```

### Source Code (repository root)

```text
putio-sync-arr/src/
├── arr_api.rs        # + mapping create/update/delete, root folders, folder paths, config get/put
├── path_mapping.rs   # NEW: marker, search, proposal, apply
├── recommend.rs      # NEW: read and turn on recommended settings
├── setup.rs          # + pub connect() (detect over base_urls)
└── lib.rs
putio-sync-telemetry/src/  # + arr_path_mapping_total and its label enum
putio-sync-api/src/routes/arr.rs   # + /arr/setup/check, /arr/setup/mapping, /arr/setup/recommendation
putio-sync-api/ui/api.js           # + three calls
putio-sync-api/ui/settings/arr.js  # + guided steps view
putio-sync-api/tests/arr_settings.rs, secrets_sweep.rs
```

**Structure Decision**: extend the existing arr crate and routes; no new crate.

## Complexity Tracking

None.
