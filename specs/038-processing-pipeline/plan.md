# Implementation Plan: Processing Pipeline and Its View

**Branch**: `038-processing-pipeline` | **Date**: 2026-11-05 | **Spec**: [spec.md](spec.md)

**Input**: `specs/038-processing-pipeline/spec.md`.

**Base**: Build on `claude/039-pause-hold`, or on main once 039 is merged. This feature uses
three things from 039:
- its item unit (`transfer:<id>`, folder and file items in `putio-sync-download/src/pipeline.rs`);
- holds (`pipeline_holds`);
- the `pipeline_tick` loop and the Active page.

## Summary

A new crate, `putio-sync-pipeline`, runs the steps after (and one gate before) the local
download. It stores the state of each item, step and attempt in SQLite. `pipeline_tick` (039)
drives it each tick:
- a newly seen item gets its pre-download gate;
- an item whose NAS stage just finished starts its post-download stages.

Pipeline hooks reuse the 027 command and webhook runners, with a stage, an order, a category
filter, and a required/optional choice. The *arr view (011) asks the pipeline whether a
file-changing step is still due, and holds `percentDone` under 1. In watch-folder mode the
download goes to a hidden staging folder, which is renamed into the category folder at the end.
The Active page row shows the current step, and the row expands to a timeline. Recently
finished keeps the timeline.

## Technical Context

**Language/Version**: Rust 1.96 (edition 2021).

**Primary Dependencies**: Only existing ones: `tokio`, `sqlx` (SQLite), `serde`, `tracing`,
`prometheus`, and the `putio-sync-hooks` runners. No new crates.

**Storage**: New tables `pipeline_items`, `pipeline_steps` and `pipeline_attempts`
([data-model.md](data-model.md)). Retention follows `[transfers] finished_retention_hours`
(039) for finished items.

**Testing**: `cargo test`. Hooks are tiny shell scripts in temp dirs, URL hooks use wiremock,
the *arr view is tested through its JSON, and nothing calls Put.io.

**Target Platform**: The Linux NAS (Docker) and desktop installs.

**Performance Goals**: With no steps configured, there is no added latency (SC-001). The gate
check is one indexed query per item per tick.

**Constraints**:
- Step output is kept to the last 200 lines or 64 KB.
- Items run with a concurrency of 2 by default.
- A restart is safe: interrupted steps run again, and done steps don't.

**Scale/Scope**: Tens of items in flight, and a few steps each.

## Constitution Check

| Principle | Check |
|-----------|-------|
| I. Crate boundaries | Orchestration goes in the new `pipeline` crate, which owns "what happens to an item after download". It depends on `core`, `db` and `hooks`. The `download` crate exposes item transitions and does not depend on `pipeline`. `daemon` connects them in `pipeline_tick`. `arr` reads a `PipelineGate` trait from `core`, which `pipeline` implements, so `arr` does not depend on `pipeline`. `api` serves the routes. |
| II. Local-first | `make ci`. |
| III. Tests without Put.io | Hooks are temp scripts or wiremock. Items are seeded in the DB. |
| IV. Download integrity | An item enters post-download stages only when 039 reports all of its files done (verified and renamed). The staging rename is a single `rename(2)` on one filesystem, and a cross-filesystem setup is refused (FR-015). |
| V. Observable | Metrics: `putio_sync_pipeline_steps_total{stage,kind,result}`, `putio_sync_pipeline_step_seconds{stage,kind}` (histogram) and `putio_sync_pipeline_items_waiting{stage}`. Step start and end are logged with fields. Output passes through `log_redact`. |
| VI. Secrets | Hook env and headers use 027's masking in the API. Output tails are redacted before they are stored. |
| VII. Simplicity | No new dependency. The pre-download gate reuses 039's hold rows, not a second blocking mechanism. Config adds `[pipeline] concurrency`, `optional_retries` and `staging`, plus hook fields `stage`, `required` and `categories`. Each one answers an FR. |

Post-design re-check: passes, and nothing needs to go in Complexity Tracking.

## Project Structure

```text
putio-sync-pipeline/              # new crate
├── src/
│   ├── lib.rs                    # Pipeline service: on_new_item, on_downloaded, retry, skip, configure
│   ├── model.rs                  # Stage, StepKind, StepState, ItemState; the stage order
│   ├── store.rs                  # sqlx queries over the three tables
│   ├── plan.rs                   # which steps an item gets (config + category); skipped reasons
│   ├── runner.rs                 # runs one step attempt (hook via putio-sync-hooks), output tail
│   ├── staging.rs                # watch-folder staging dir + final rename
│   └── gate.rs                   # PipelineGate impl: file-changing step due? current step label
└── tests/
    ├── stages.rs                 # order, skip-when-unconfigured, branches API, restart
    ├── hooks.rs                  # env/JSON input, exit codes, skip code 10, timeout
    ├── retry.rs                  # required holds, optional gives up after N, retry/skip
    └── staging.rs                # nothing visible until one rename; cross-fs refused

putio-sync-core/src/config.rs     # [pipeline] section; HookConfig gains stage/required/categories
putio-sync-core/src/pipeline_gate.rs  # trait PipelineGate (used by arr)
putio-sync-hooks/src/{command,webhook}.rs  # run_step(): item payload, exit code → StepOutcome
putio-sync-db/src/schema.rs       # three tables
putio-sync-download/src/pipeline.rs  # expose item keys + "all files done" transition; staging target
putio-sync-daemon/src/daemon.rs   # build Pipeline, call it from pipeline_tick
putio-sync-arr/src/view.rs        # hold percentDone < 1 and set errorString-free status message
putio-sync-api/src/routes/active.rs  # current step on rows; GET /active/{key}/timeline; retry/skip
putio-sync-api/ui/…               # timeline panel; hook editor stage/required/categories
EVENTS.md, README.md, config.example.toml, CHANGELOG.md
```

**Structure Decision**: a new crate (Principle I). The pipeline does not belong to `download`
(bytes) or `hooks` (running one hook).

## Complexity Tracking

None.
