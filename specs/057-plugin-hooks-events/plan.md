# Implementation Plan: More plugin hooks and events

**Branch**: `feat/057-plugin-hooks-events` | **Date**: 2026-11-27 | **Spec**: [spec.md](spec.md)
**Input**: Feature specification from `specs/057-plugin-hooks-events/spec.md`

## Summary

Add 20 event types to the existing bus (`putio_sync_core::events`), all in schema version 1,
each with a payload struct, a sample, a schema entry, an EVENTS.md section and a test that
triggers it. `pipeline.finished` is sent by the pipeline runner once per item that leaves the
pipeline; the bundled media-server plugins refresh on it. A `[[hooks]]` entry with a `stage`
may name a `plugin` in place of a command or URL: the pipeline asks through a small trait in
core (`StepPlugins`), which the plugins service implements by queueing an `on_item` call on
the plugin's own worker. Outage events share one edge detector (`putio_sync_core::health`):
the Put.io client reports each attempt to an observer, so core, which the client cannot
depend on, turns failures into `putio.unreachable` / `putio.recovered` and a 401 into
`auth.expired`. The other emitters sit where the fact is known: the download manager
(`sync.started`, `download.started`, `download.progress`, `cleanup.deleted`), the storage
refresh (`storage.recovered`), the doctor's Disk space check (`local_disk.*`), the arr
service's periodic check (`arr.*`), the subtitles and translation stages, and the daemon
command (`daemon.*`, `config.changed`). Plugins gain `on_config_changed` and `on_unload`,
both bounded by the plugin's time limit.

Not built, with the reason:

- `transfer.stalled`: the stall check already sends `transfer.stuck`; a second name for the
  same fact would break FR-002's spirit and double every notification.
- `doctor.problem`: already sent by the doctor; unchanged.
- `archive.extracted` / `archive.failed`: putio-sync has no archive extraction, so there is
  nothing to announce. They come with that feature.

## Technical Context

**Language/Version**: Rust stable (edition 2021); Lua 5.4 for the bundled plugins; plain
JavaScript modules for the web UI
**Primary Dependencies**: none new
**Storage**: none new; plugin state stays in the plugin store; no migration
**Testing**: per-crate integration tests against mockito and in-process fakes; the shared
process-wide bus is filtered by unique ids, names or URLs; `putio-sync-core/tests/events_doc.rs`
keeps EVENTS.md, the samples and `schema.json` in step; `node --test` for the hook editor
**Target Platform**: Linux, macOS, Windows; full and lite builds
**Project Type**: CLI and daemon in a Cargo workspace
**Performance Goals**: `download.progress` costs one file-size read per file per 10 s and
nothing when no hook or plugin lists it; outage detection is two atomics per Put.io attempt
**Constraints**: existing event types keep their fields and timing (FR-002); no secrets in
events (FR-004); a stop waits for plugins no longer than their time limits (FR-026)
**Scale/Scope**: 20 new event types (36 in all), 3 plugin callbacks, 1 new hook target, 2
example plugins, 21 new UI strings in 26 languages

## Constitution Check

| Principle | Status | How |
|---|---|---|
| I. Crate boundaries | Pass | Events, `health` and the `StepPlugins` trait live in `core`. The pipeline does not depend on the plugins crate; the daemon hands the service over. The client stays below core and only offers an attempt observer. |
| II. Local-first | Pass | Every check runs with `cargo test` and `node --test`; no service is needed. Translating the new UI strings is a maintainer step. |
| III. Tests without Put.io | Pass | Put.io is mockito; Radarr/Sonarr are mockito or a closed port; the plugin step is a fake `StepPlugins` in the pipeline tests and a real Lua plugin in the plugins tests. |
| IV. Download integrity | Pass | The progress ticker only reads the temp file's size; the transfer future is polled as before. |
| V. Observable | Pass | New events are counted by `putio_sync_events_total{type}` (bounded: the type list). Plugin run kinds gain `item`, `config_changed`, `unload` (bounded). |
| VI. Secrets | Pass | `config.changed` carries section names only; `auth.expired` carries a fixed message; arr URLs are cut to scheme, host and port; errors go through the bus's cut and redaction. Tests assert no token in the JSON. |
| VII. Simplicity | Pass with note | No new dependency, crate or config section. One new hook key (`plugin`), one manifest key (`step`), one plugin setting (`wait_for_pipeline`). |

## Project Structure

### Documentation (this feature)

```text
specs/057-plugin-hooks-events/
├── spec.md
├── plan.md              # this file
└── tasks.md
```

### Source Code (repository root)

```text
putio-sync-core/src/
├── events.rs            # 20 new types, payloads, samples; is_opt_in / matched_by
├── events/schema.json   # the same, for GET /events/schema and the 055 event picker
├── health.rs            # new: Outage, Once, Edge, PutioWatch
├── step_plugins.rs      # new: StepPlugins trait, StepAnswer, parse_answer
├── hooks.rs             # `plugin` target, HookKind::Plugin, validation
└── restart.rs           # changed_sections (names only)
putio-sync-client/src/client.rs      # observe_attempts(AttemptObserver)
putio-sync-pipeline/src/
├── runner.rs, lib.rs, store.rs      # pipeline.finished; plugin steps
├── subtitles.rs                     # subtitles.found / subtitles.missing
└── translate.rs, callback.rs        # translation.finished / translation.failed
putio-sync-plugins/src/
├── worker.rs, service.rs            # on_item, on_config_changed, on_unload; shutdown
├── steps.rs                         # new: StepPlugins for PluginsService
└── manifest.rs                      # step = true
putio-sync-plugins/bundled/          # mediaserver.lua, plex/jellyfin/emby plugin.toml
putio-sync-download/src/manager.rs   # sync.started, download.started/progress, cleanup.deleted
putio-sync-storage/src/lib.rs        # storage.recovered
putio-sync-doctor/src/               # local_disk.*; Plugins check for step plugins
putio-sync-arr/src/service.rs        # arr.unreachable / arr.recovered
putio-sync-hooks/src/service.rs      # opt-in matching for download.progress
putio-sync-cli/src/commands/daemon.rs  # wiring; daemon.*, config.changed; plugin shutdown
putio-sync-api/ui/                   # hooks.js labels and Plugin kind; i18n/*.json
examples/plugins/                    # small-item-step, batch-digest
EVENTS.md, PLUGINS.md, README.md, CHANGELOG.md, ROADMAP.md
```

**Structure Decision**: no new crate. Two small modules in core (`health`, `step_plugins`)
hold what more than one crate needs.

## Decisions

- **D1 `pipeline.finished` beside `pipeline.completed`.** The old event keeps its meaning
  (warnings only). The new one is sent for every exit, last, so "ready" has one signal.
  `files` are paths under `folder`.
- **D2 Plugin steps through a trait in core.** The pipeline stays free of Lua. A step call
  is one more job on the plugin's queue, so FR-012 holds by construction, and the plugin's
  own time limit applies in place of the hook's `timeout_secs`.
- **D3 One outage detector.** `Outage` announces a failure streak once it has lasted the
  threshold (60 s) and a recovery only when the outage was announced. Put.io feeds it per
  attempt; Radarr/Sonarr per 15-minute check, so there the outage is announced at the
  second failed check.
- **D4 `auth.expired` on 401 only**, once until a call succeeds again.
- **D5 `download.progress` is opt-in.** `EventType::is_opt_in` keeps it out of `"*"` for
  hooks and plugins; the manager's ticker is the throttle (10 s per file).
- **D6 `local_disk.*` from the doctor's Disk space check**, which already knows the folder,
  the threshold and the schedule; no second disk poller.
- **D7 `config.changed` after the change is applied**, with the differing top-level sections
  from the serialized configs, so no value can leak.
- **D8 `on_unload` is bounded and skipped for a broken state.** A reload, a turn-off and the
  daemon's stop all go through it; the daemon drains events for 2 s, then waits for the
  plugins' own limits.
- **D9 Media servers default to waiting for the pipeline** when a pipeline is known to run
  (`daemon.started` or a `pipeline.finished` was seen); `max_wait_seconds` is the way out,
  and `wait_for_pipeline = false` restores grouping.

## Complexity Tracking

Nothing to justify beyond the notes in the Constitution Check.
