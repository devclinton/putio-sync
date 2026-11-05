# Implementation Plan: Log Viewer in the UI

**Branch**: `047-log-viewer` | **Date**: 2026-11-05 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/047-log-viewer/spec.md`

## Summary

A capture layer joins the tracing subscriber that `init_logging` builds. It sits behind the
reloadable level filter, which already exists (feature 003), so it only sees lines the level
in effect lets through. For each event it:

1. formats the message and fields;
2. runs them through a redaction pass (known secret values, then credential patterns, then
   URL shortening);
3. cuts the result to 4 KB;
4. pushes it into a bounded in-memory ring buffer and onto a broadcast channel.

Both live in `putio-sync-core` (`log_capture.rs`, `log_redact.rs`). The level filter gains a
small override state: an overall level and up to 20 per-module levels, each until a time or
until restart, layered over the base directive from the config file or `RUST_LOG`. A timer
task undoes timed overrides.

`putio-sync-api` adds protected routes under `/logs`:

- the kept lines, with filters;
- an SSE stream that resumes from a sequence number;
- level status, set and clear.

The doctor adds a "Log level" check, telemetry adds `putio_sync_log_lines_dropped_total{reason}`,
the UI adds a Logs page (`pages/logs.js`), and Settings › Logging gains `buffer_lines`.
Decisions are in [research.md](research.md).

## Technical Context

**Language/Version**: Rust 1.96 (`rust-toolchain.toml`), edition 2021. The UI is vanilla
ES modules.

**Primary Dependencies**:
- Existing: tracing, tracing-subscriber (reload, env-filter), tokio (broadcast, time), axum
  0.7 (`response::sse`), serde_json, prometheus.
- New in `putio-sync-api` only: `futures-util`, already in `Cargo.lock` through axum and
  hyper. It provides the `Stream` the SSE response takes (R5). No new crate enters the build.

**Storage**: memory only. The ring buffer holds up to `buffer_lines` lines of at most 4 KB
each.

**Testing**:
- `cargo test`. Unit tests in core cover redaction, the buffer, override composition and
  expiry (with explicit `now`).
- API integration tests (`putio-sync-api/tests/logs.rs`) cover auth, filters, SSE resume and
  level set/clear.
- A secrets test logs every secret kind at trace level and checks `/logs`, the stream and
  the download text.
- There is a doctor unit test, a telemetry test, and `ui_assets` keeps the 384 KB cap.

**Target Platform**: Linux container on the NAS; macOS for development.

**Project Type**: Cargo workspace (CLI and daemon) with an embedded web UI.

**Performance Goals**: capturing a line takes one short mutex hold and one channel send, and
never awaits. A new line reaches an open page within 2 s.

**Constraints**:
- The UI bundle is about 378 KB of its 384 KB cap, so the Logs page JS and CSS must stay
  under about 9 KB together.
- `settings.js` gets one field only (the 046 split is in flight).
- The capture layer must never log. A line logged from inside it would recurse.

**Scale/Scope**: one operator, a few browsers at once; 100 to 50,000 lines kept.

## Constitution Check

| Principle | Check |
|-----------|-------|
| I. Crate boundaries | Capture, redaction and overrides are logging concerns, so they live in `core` (logging is already there). Routes are in `api`, the check in `doctor` and the metric in `telemetry` (which `core` already depends on). Nothing depends upward. ✅ |
| II. Local-first | Everything is covered by `make ci`. ✅ |
| III. Tests without Put.io | No Put.io calls; tests use the existing mocked `test_app`. ✅ |
| IV. Download integrity | Not touched. ✅ |
| V. Observable by default | Adds a dropped-lines counter with a bounded `reason` label. Level changes log structured `old`/`new`/`reason`. ✅ |
| VI. Secrets stay secret | This is the main risk. Every line passes redaction before it is stored, the endpoints are protected, and tests cover each secret kind. ✅ |
| VII. Simplicity | No regex crate (hand-written scanner, R3). One new config key with a default. `futures-util` is justified above. ✅ |

Post-design re-check: unchanged, no violations.

## Project Structure

### Documentation (this feature)

```text
specs/047-log-viewer/
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/logs-api.md
└── tasks.md
```

### Source Code (repository root)

```text
putio-sync-core/src/
├── log_capture.rs      # new: LogLine, ring buffer, broadcast, capture Layer, query
├── log_redact.rs       # new: secret registry + redaction pass
├── logging.rs          # + buffer_lines, capture layer in init_logging, overrides
└── config.rs           # + Config::secret_values()
putio-sync-telemetry/src/
├── logs.rs             # new: log_lines_dropped_total{reason}
└── lib.rs              # register it
putio-sync-doctor/src/
├── report.rs           # + CheckId::LogLevel
└── checks/local.rs     # + log_level check
putio-sync-api/src/
├── routes/system/logs.rs      # new: GET /logs, GET /logs/stream, GET|PUT|DELETE /logs/level
├── routes/system/settings.rs  # + buffer_lines, set_capacity and secrets refresh on save
└── lib.rs                     # nest /logs, refresh secrets in AppState::new
putio-sync-api/ui/
├── pages/logs.js       # new page
├── index.html, app.js  # nav entry + route
├── app.css             # a few rules for the log list
└── pages/settings.js   # + one field: buffer_lines
putio-sync-cli/src/main.rs     # set secrets before init_logging
putio-sync-api/tests/logs.rs   # new integration tests
```

**Structure Decision**: existing workspace layout; the new code goes in two new core
modules, one route module and one page.

## Complexity Tracking

No violations.
