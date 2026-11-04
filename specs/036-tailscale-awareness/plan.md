# Implementation Plan: Tailscale Awareness

**Branch**: `036-tailscale-awareness` (worktree branch `claude/quizzical-shtern-aa4514`) | **Date**: 2026-11-03 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/036-tailscale-awareness/spec.md`

## Summary

A new crate `putio-sync-tailscale` talks to the local Tailscale daemon: over its LocalAPI
unix socket (a minimal HTTP/1.0 client on `tokio::net::UnixStream`, no new dependency), or,
when the socket is not there, through the `tailscale` CLI's JSON output. It parses the
node's status, `serve` config and `whois` answers into small types, and a
`TailscaleService` caches status for 10 s and `whois` per address for 60 s, with a 3 s
timeout on every call. `core` gains a `[tailscale]` group (`tailnet_only`,
`identity_sign_in`, `allowed_users`, `socket`). The API's auth middleware, when a request
has no valid token and identity sign-in is on, asks the service for the request's identity:
`serve`'s `Tailscale-User-Login` header when the peer (from `ConnectInfo`, never assumed)
is loopback, or `whois` of a tailnet peer address; an allowed login lets the request in
and is attached as an extension. `GET /auth/status` reports the identity; `GET /tailscale`
returns the details for the card and the mobile app. The daemon's listener binds the
tailnet IPv4 address with `[api] bind`'s port when `tailnet_only` is on, retrying in the
background until Tailscale is ready. A settings group `tailscale` (GET/PUT/preview,
assistant) refuses `tailnet_only = true` without a tailnet address. The doctor gets a
Tailscale check, telemetry an identity sign-in counter, and Radarr/Sonarr auto-setup puts
the tailnet address first among candidate hosts. The UI gets a Tailscale group on
Settings with a details panel, a confirm before tailnet-only, and the allow-list prefilled;
the shell skips the sign-in page for an identified browser and shows who is signed in.

## Technical Context

**Language/Version**: Rust stable (edition 2021); browser JavaScript (ES modules, no build)

**Primary Dependencies**: existing only (`tokio` with `net` and `process`, `serde_json`,
`axum`, `tracing`, `prometheus` via telemetry). No new external crate (research R2).

**Storage**: none. Config only (`[tailscale]`).

**Testing**: `cargo test`: parsing tests on recorded LocalAPI JSON; a fake LocalAPI server on
a `UnixListener` in a temp dir (status, serve-config, whois, hang, junk); a fake CLI script
on unix; middleware tests with `ConnectInfo` set by hand (loopback + header, LAN + header,
tailnet peer via fake whois, tagged node, not allowed, daemon down, no `ConnectInfo`);
config validation; settings round trip; `/tailscale` and `/auth/status` integration tests;
setup-mode 503; doctor check unit tests; listener bind resolution tests; UI asset tests.
Nothing needs Tailscale installed (FR-017).

**Target Platform**: Linux (Docker on a NAS, snap), macOS and Windows desktops. The socket
client is `cfg(unix)`; elsewhere only the CLI is tried.

**Project Type**: Cargo workspace with an embedded web UI.

**Performance Goals**: at most one LocalAPI call per 10 s for status and one per peer
address per 60 s for `whois`; each call bounded by 3 s.

**Constraints**: fail closed (FR-007); identity only from a real loopback peer or a
`whois`-resolved tailnet peer; never write the Tailscale config; UI bundle stays under the
384 KB cap (about 310 KB today).

**Scale/Scope**: a handful of tailnet devices; caches hold at most a few hundred entries
(pruned on insert).

## Constitution Check

| Principle | Check |
|-----------|-------|
| I. Crate boundaries | Talking to tailscaled is its own concern: new crate `putio-sync-tailscale`, used by `api`, `cli` (listener) and `doctor`. It depends only on `tokio`, `serde`, `tracing`. `[tailscale]` config lives in `core`; the counter in `telemetry`. No upward dependency. Pass. |
| II. Local-first verification | Covered by `make ci`; the Dockerfile gets the new crate's lines. Pass. |
| III. Tests without Put.io | No Put.io involved; Tailscale is faked by a local socket server and a fake CLI. Pass. |
| IV. Download integrity | Not touched. Pass. |
| V. Observable by default | `putio_sync_tailscale_identity_total{via,result}` (bounded labels), structured logs for detection failures, waits for the tailnet address and identity refusals. Pass. |
| VI. Secrets stay secret | Protected routes keep auth; identity is an extra way in, off by default, allow-listed, fail closed. `/tailscale` needs auth. No tokens in responses. Logins are logged at debug/info only. Pass. |
| VII. Simplicity | Four config keys, each needed by the spec. No HTTP/unix-socket crate: a 60-line HTTP/1.0 client is enough for three GETs. Pass. |

Post-design re-check: unchanged, all pass.

## Project Structure

### Documentation (this feature)

```text
specs/036-tailscale-awareness/
├── spec.md
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/tailscale-api.md
└── tasks.md
```

### Source Code (repository root)

```text
Cargo.toml                                  # + putio-sync-tailscale member
Dockerfile                                  # + the new crate's COPY / stub lines
config.example.toml, README.md              # + [tailscale], Docker/sidecar/serve docs
putio-sync-tailscale/                       # NEW crate
├── Cargo.toml
└── src/
    ├── lib.rs                              # TailscaleService (caches, timeouts), identity checks
    ├── model.rs                            # Status, ServeEntry, Identity, State; JSON parsing
    ├── localapi.rs                         # unix-socket HTTP/1.0 GET (cfg(unix))
    ├── cli.rs                              # `tailscale ... --json` fallback
    └── tests.rs                            # fake socket server / fake CLI tests
putio-sync-core/src/config.rs               # TailscaleConfig + validate + resolve
putio-sync-telemetry/src/tailscale.rs       # identity counter
putio-sync-doctor/src/checks/net.rs         # Tailscale check (+ CheckId::Tailscale)
putio-sync-api/src/lib.rs                   # AppState.tailscale, /tailscale route
putio-sync-api/src/middleware/auth.rs       # identity path
putio-sync-api/src/routes/system/auth.rs    # identity in /auth/status
putio-sync-api/src/routes/system/tailscale.rs # GET /tailscale
putio-sync-api/src/routes/system/settings.rs, assistant.rs  # tailscale group
putio-sync-api/src/routes/arr.rs            # tailnet address first in candidate hosts
putio-sync-cli/src/api_listener.rs          # tailnet-only bind with retry
putio-sync-api/ui/tailscale.js              # NEW: group definition, details panel, confirm
putio-sync-api/ui/pages/settings.js         # tailscale group, beforeSave hook
putio-sync-api/ui/api.js, app.js, index.html# identity sign-in in the shell
putio-sync-api/ui/pages/login.js            # identified → straight in
putio-sync-api/src/ui.rs                    # asset list
putio-sync-api/tests/tailscale.rs           # NEW integration tests
```

**Structure Decision**: one new crate for the Tailscale client (Principle I); everything
else in the crate that owns each concern.

## Design for 037

`putio_sync_tailscale::tailnet_bind(&Status, port)` turns a status into a socket address
or a reason. 037's address list can treat "tailnet" as one more address kind resolved by
this function, and `tailnet_only = true` can migrate to a list holding only that entry.
The listener's retry loop is written per address so 037 can run one per entry.

## Complexity Tracking

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| New crate `putio-sync-tailscale` | Used by api, cli and doctor; owns one concern | Putting it in `core` would pull process and socket code into the shared config crate; in `api` the doctor could not use it |
