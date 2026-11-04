# Implementation Plan: Bandwidth Schedules

**Branch**: `032-bandwidth-schedules` (worktree branch `claude/strange-feistel-755f52`) | **Date**: 2026-11-04 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/032-bandwidth-schedules/spec.md`

## Summary

`core` gains a `bandwidth` module: the `Limit` type (`unlimited`, `paused`, bytes per
second), schedule entries as written in `[[download.bandwidth_schedule]]`, their strict
parse (errors naming the entry and field) and warnings (overlap, paused all week, very low
limits), the timed `Override` (pause or slow, with an end time), and a pure `Policy` that
answers "what is the limit at this local time, from where, and when does it change next".

The existing `putio_sync_download::RateLimiter` becomes the one shared limiter: a token
bucket behind a mutex, used through `&self`, holding the `Policy` and the override. The
download manager owns one, passes it to every stream (instead of a number that each stream
turned into its own limiter), stops claiming queue items while it reads paused, and swaps
its policy in `update_config`. Each `consume` re-reads the limit at most once a second, so
schedule boundaries, saves and quick actions reach running downloads within about a
second. The limiter sets the two new gauges whenever the limit changes.

The override is persisted in the 039 `daemon_flags` table as JSON under
`bandwidth_override` (text getters added to `FlagsRepository`), restored when the daemon or
server starts, ignored once past its end.

API: `GET /bandwidth`, `PUT /bandwidth/override`, `DELETE /bandwidth/override`, and a
`bandwidth` section in `GET /status`. Settings › Download gains the schedule and slow
limit (same `PUT /settings/download`, validated by `Config::validate` like everything
else). The doctor gains a `Bandwidth` check; the assistant gains the schedule warnings, so
the Settings banner and the doctor's Config check show them. UI: a Bandwidth card on the
Status page with the quick actions, and a schedule editor in Settings › Download.

## Technical Context

**Language/Version**: Rust stable (edition 2021); browser ES modules for the UI

**Primary Dependencies**: none new. `chrono` (already in core, download, doctor) gives
local time and the UTC offset.

**Storage**: config TOML (two new `[download]` keys); one new row key in the existing
`daemon_flags` table, no migration.

**Testing**: core unit tests for parsing, `Policy::at`, `next_change`, wrap past midnight
and warnings, with fixed `NaiveDateTime`s; download tests for the shared limiter (total
across two consumers, live limit change, pause and resume) with an injected clock; API
route tests (`/bandwidth`, `/status`, settings save refusal, setup-mode 503); telemetry
contract; doctor check unit test; UI node test for the schedule editor helpers.

**Target Platform**: Linux NAS daemon (Docker), desktop CLI

**Project Type**: Cargo workspace: daemon + HTTP API + web UI

**Performance Goals**: `consume` takes one uncontended mutex per chunk; the policy is
evaluated at most once per second.

**Constraints**: UI bundle cap 384 KB; setup mode answers 503 for every non-wizard route
(the fallback already does it for new routes).

**Scale/Scope**: up to 50 schedule entries (validated).

## Constitution Check

| Principle | Status | Notes |
|-----------|--------|-------|
| I. Crate Boundaries | ✅ | Schedule logic in `core` (used by download, api, doctor); the limiter in `download`; persistence in `db`; the API maps bodies; the UI displays. |
| II. Local-First Verification | ✅ | `make ci` covers it all. |
| III. Tests Without Put.io | ✅ | Limiter tests use synthetic byte counts and the existing mockito server; the clock is injected. |
| IV. Download Integrity | ✅ | A pause only stops reading; partial files keep the temp path and resume path. Nothing is deleted. |
| V. Observable by Default | ✅ | Two gauges (`putio_sync_download_bandwidth_limit_bytes_per_second`, `putio_sync_download_bandwidth_override`), no labels; limit changes logged with `limit` and `source` fields. |
| VI. Secrets Stay Secret | ✅ | No secrets involved. |
| VII. Simplicity | ✅ | No new dependency; the existing limiter is reused and made shared rather than adding a second one; one new doctor check because the spec asks for one. |

Post-design re-check: unchanged, all ✅.

## Project Structure

### Documentation (this feature)

```text
specs/032-bandwidth-schedules/
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── config.md
│   └── api.md
├── checklists/requirements.md
└── tasks.md
```

### Source Code (repository root)

```text
putio-sync-core/src/bandwidth.rs        # new: Limit, ScheduleEntry, Policy, Override, check
putio-sync-core/src/config.rs           # [download] bandwidth_schedule, slow_limit; validate
putio-sync-core/src/assistant.rs        # schedule warnings as risks
putio-sync-db/src/pipeline.rs           # text flags; BANDWIDTH_OVERRIDE key
putio-sync-telemetry/src/{lib,download}.rs, tests/contract.rs   # two gauges
putio-sync-download/src/rate_limiter.rs # shared limiter with policy and override
putio-sync-download/src/stream.rs       # take &RateLimiter
putio-sync-download/src/manager.rs      # own the limiter, pause gate, override + restore
putio-sync-daemon/src/daemon.rs, putio-sync-cli/src/commands/{daemon,server}.rs  # restore at start
putio-sync-doctor/src/{report.rs,checks/mod.rs,checks/sync.rs}   # Bandwidth check
putio-sync-api/src/routes/system/bandwidth.rs  # new routes
putio-sync-api/src/routes/system/{status,settings}.rs, routes/local/workers.rs
putio-sync-api/ui/bandwidth.js          # new: card, quick actions, schedule editor helpers
putio-sync-api/ui/pages/{status,settings}.js
README.md, config.example.toml, example-config.toml, CHANGELOG.md
```

**Structure Decision**: existing workspace layout; one new core module, one new route
module, one new UI module.

## Complexity Tracking

None.
