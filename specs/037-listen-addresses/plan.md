# Implementation Plan: Listen Addresses and Interfaces

**Branch**: `037-listen-addresses` (worktree branch `claude/agitated-chandrasekhar-4d9e06`) | **Date**: 2026-11-03 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/037-listen-addresses/spec.md`

## Summary

`core` gains a `listen` module: a `ListenAddr` (`Ip(SocketAddr)`, `Interface { name, port }`,
`Tailnet { port }`) parsed from `host:port`, a `BindList` that deserialises from one string
or a list of strings (and serialises back to a string when it holds one entry, so existing
files do not change shape), list validation (empty, more than 16, duplicates, overlapping
wildcards, API/metrics clash), and `interface_addrs(name)` reading the OS's interface table
through the small `if-addrs` crate (link-local IPv6 dropped). `ApiSettings.bind` and the
metrics bind become `Vec<ListenAddr>`; `Config::api_listen()` applies `tailnet_only` (one
`tailnet:<port>` per distinct port).

`putio-sync-api` gains `listen`: a supervisor that turns a list of entries into running
listeners. Each tick it resolves every entry (IP: itself; interface: its addresses now;
tailnet: `tailnet_bind` from 036 through `TailscaleService`), binds addresses not yet bound
(IPv6 wildcards with `IPV6_V6ONLY` via `socket2`), stops listeners whose address vanished,
and records the bound addresses in a shared `Bound` list. It ticks every second for the
first 5 minutes (Tailscale/sidecar start-up, a port lingering after setup mode) and every
30 s after, only when the list has interface or tailnet entries or something has not bound
yet. Every listener serves the same router with a `ListenedOn(addr)` extension, so a
handler knows which local address a request arrived on. It stops gracefully on request
(setup mode hands its port to the API).

The daemon listener (`cli/api_listener.rs`), setup mode, `server`, and the metrics listener
all use the supervisor. `AppState` holds the API's `Bound`; `AppState::api_port()` returns
the first bound port, else the first configured port, and replaces every
`resolve_api().bind.port()` (Tailscale details and hints, Radarr/Sonarr hosts and
category refresh, doctor). `server` takes `--bind`/`--port` as overrides of the configured
list.

A `network` settings group (`GET/PUT /settings/network`, preview, assistant, export) edits
`[api] bind` and `[metrics] bind` as lists. `GET /listen` returns what is listened on now and
the overrides; `POST /settings/network/check` resolves a proposed body entry by entry and
says whether the request's `ListenedOn` address would still be served. A save where no API
entry gives an address now is refused. The doctor gets a `listen` check (entries with no
address; identity sign-in on with a loopback API address). The UI gets a Network group with
a live panel and a confirm before a save that cuts the page off.

## Technical Context

**Language/Version**: Rust stable (edition 2021); browser JavaScript (ES modules, no build)

**Primary Dependencies**: existing plus `if-addrs` 0.15 (interface addresses, core) and
`socket2` 0.6 as a direct dependency (already in the tree through tokio/hyper; api) for
`IPV6_V6ONLY`. See research R1, R2.

**Storage**: none. Config only (`[api] bind`, `[metrics] bind`).

**Testing**: `cargo test`: parse/display/serde round trips for every address form and
error; list validation; env lists; `api_listen` with tailnet-only; supervisor tests on
loopback with port 0 replaced by a free port (bind two entries, one failing, follow a fake
resolver whose answer changes, stop); interface resolution of the loopback interface
(`lo`/`lo0`, found by looking for the interface holding 127.0.0.1); `/settings/network`
round trip, refusal, check (`ListenedOn` covered / cut off / missing); `/listen`;
`api_port()` from bound and fallback; `/tailscale` port from bound; setup-mode 503 for the
new routes; doctor check units; `server` flag merging units; UI asset tests. No test needs
an interface other than loopback (FR-018).

**Target Platform**: Linux (Docker, snap), macOS, Windows. `if-addrs` covers all three.

**Project Type**: Cargo workspace with an embedded web UI.

**Performance Goals**: one interface-table read and (with tailnet entries) one cached
Tailscale status per tick; ticks every 30 s in steady state.

**Constraints**: never bind an address that no entry names; defaults and single-address
configs unchanged (SC-001); UI bundle under the 384 KB cap; unix sockets in tests under
`/tmp`.

**Scale/Scope**: up to 16 entries per list; a handful of interfaces.

## Constitution Check

| Principle | Check |
|-----------|-------|
| I. Crate boundaries | Address parsing, list rules and interface lookup are shared config (`core`, used by api, cli, doctor). Running listeners is HTTP serving (`api`); `cli` only chooses the list and the router. Telemetry keeps its router; its `MetricsSettings` drops `bind`, which `core` now resolves. No upward dependency. Pass. |
| II. Local-first verification | Covered by `make ci`; Dockerfile needs no new crate lines (no new workspace member). Pass. |
| III. Tests without Put.io | Listener tests bind loopback; Tailscale faked as in 036. Pass. |
| IV. Download integrity | Not touched. Pass. |
| V. Observable by default | Structured logs per entry (bound, waiting with reason, failed, closed); metrics bind configurable as before, now with the same forms. Pass. |
| VI. Secrets stay secret | New routes require auth; no tokens in responses or logs. Doctor warns about loopback + identity sign-in (036's trust assumption). Pass. |
| VII. Simplicity | One supervisor for all four listeners instead of four bind paths. Two dependencies justified in research (R1, R2). No new config key: `bind` widens. Pass. |

Post-design re-check: unchanged, all pass.

## Project Structure

### Documentation (this feature)

```text
specs/037-listen-addresses/
├── spec.md
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/listen-api.md
└── tasks.md
```

### Source Code (repository root)

```text
putio-sync-core/Cargo.toml                   # + if-addrs
putio-sync-core/src/listen.rs                # NEW: ListenAddr, BindList, rules, interface_addrs
putio-sync-core/src/config.rs                # ApiConfig/MetricsConfig bind: BindList; resolve_*; api_listen; validate
putio-sync-core/src/setup.rs                 # metrics bind from the wizard
putio-sync-telemetry/src/server.rs           # MetricsSettings without bind; router()
putio-sync-api/Cargo.toml                    # + socket2
putio-sync-api/src/listen.rs                 # NEW: supervisor, Bound, ListenedOn, resolve
putio-sync-api/src/lib.rs                    # AppState.listening + api_port(); /listen route
putio-sync-api/src/routes/system/network.rs  # NEW: GET /listen, /settings/network{,/preview,/check}
putio-sync-api/src/routes/system/settings.rs # network group in groups()
putio-sync-api/src/routes/system/assistant.rs# network in export/import (if group-driven, nothing)
putio-sync-api/src/routes/system/tailscale.rs# port from api_port(); tailnet entries
putio-sync-api/src/routes/arr.rs, setup.rs   # port from api_port()
putio-sync-doctor/src/checks/net.rs          # listen check; tailscale check uses the list
putio-sync-doctor/src/report.rs              # CheckId::Listen
putio-sync-cli/src/api_listener.rs           # supervisor
putio-sync-cli/src/telemetry.rs              # supervisor for metrics
putio-sync-cli/src/commands/setup_mode.rs    # supervisor with stop
putio-sync-cli/src/commands/server.rs, cli.rs# --bind/--port overrides
putio-sync-api/ui/network.js                 # NEW: Network group panel + confirm
putio-sync-api/ui/pages/settings.js          # network group
putio-sync-api/src/ui.rs                     # asset list
putio-sync-api/tests/network.rs              # NEW integration tests
config.example.toml, README.md, CHANGELOG.md, ROADMAP.md
```

**Structure Decision**: parsing in `core`, serving in `api`, wiring in `cli`, following the
constitution's crate ownership.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| New dependency `if-addrs` | Interface addresses on Linux, macOS and Windows | `getifaddrs` through `libc` needs `unsafe` and a separate Windows path; parsing `ip`/`ifconfig` output is fragile and absent in slim containers |
| `socket2` as a direct dependency | `IPV6_V6ONLY` so `[::]` and `0.0.0.0` can share a port | tokio's `TcpSocket` has no `set_only_v6`; already compiled in through tokio |
