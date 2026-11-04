# Tasks: Tailscale Awareness

**Input**: Design documents from `specs/036-tailscale-awareness/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/tailscale-api.md

**Tests**: included. The constitution (III) and FR-017 require them, and the fake daemon
is the only way to exercise this feature.

## Phase 1: Setup

- [X] T001 Create crate `putio-sync-tailscale` (Cargo.toml with tokio `net`,`process`,`time`,`io-util`; serde; serde_json; tracing; dev: tempfile) and add it to the workspace members in `Cargo.toml` and to the `Dockerfile` COPY/stub lines
- [X] T002 Add `TailscaleConfig` (`tailnet_only` bool default false, `identity_sign_in` bool default false, `allowed_users` list of logins default empty, `socket` optional path) to `putio-sync-core/src/config.rs`, with validation ("each non-empty, no whitespace"; socket non-empty), `resolve_tailscale()` applying `PUTIO_SYNC_TAILSCALE_SOCKET`, default socket `/var/run/tailscale/tailscaled.sock`, and unit tests; document `[tailscale]` in `config.example.toml`

## Phase 2: Foundational

- [X] T003 [P] Model and parsing in `putio-sync-tailscale/src/model.rs`: `State` (`not_found`, `unreachable`, `stopped`, `needs_login`, `starting`, `running`), `Status`, `ServeEntry`, `Identity`, `Via`; parse LocalAPI/CLI status JSON (BackendState, TUN, Self.DNSName without trailing dot, TailscaleIPs, CertDomains, CurrentTailnet, User map → owner login, Version), serve config JSON (Web handlers' Proxy targets for a port, AllowFunnel), whois JSON (UserProfile.LoginName/DisplayName, Node.Name, Node.Tags); unit tests on recorded JSON
- [X] T004 [P] Unix-socket HTTP/1.0 GET client in `putio-sync-tailscale/src/localapi.rs` (`cfg(unix)`, `Host: local-tailscaled.sock`, `Sec-Tailscale: localapi`, 1 MiB cap, chunked tolerated, status code returned)
- [X] T005 [P] CLI fallback in `putio-sync-tailscale/src/cli.rs`: run `tailscale status --json`, `tailscale serve status --json`, `tailscale whois --json <ip>` with a 3 s timeout; binary from an explicit path or `PATH` or the macOS app path
- [X] T006 `TailscaleService` in `putio-sync-tailscale/src/lib.rs`: socket then CLI, 3 s timeout per call, status cache 10 s, whois cache 60 s per IP (pruned, cleared on `configure`), `status()`, `whois(ip)`, `identity_for(peer: Option<SocketAddr>, login_header, name_header)` implementing research R3, `is_tailnet_ip`, `tailnet_bind(&Status, port)`, `serve_urls`; tests with a fake LocalAPI on a temp `UnixListener` (running, needs login, hang → unreachable within the timeout, junk, whois 404, tagged node) and a fake CLI script, in `putio-sync-tailscale/src/tests.rs`
- [X] T007 [P] Telemetry counter `putio_sync_tailscale_identity_total{via,result}` in `putio-sync-telemetry/src/tailscale.rs` (+ registration in `lib.rs`, test)
- [X] T008 Add `tailscale: Arc<TailscaleService>` to `AppState` in `putio-sync-api/src/lib.rs`, built from the config

## Phase 3: User Story 1 - See whether Tailscale is here and its addresses (P1)

**Goal**: Settings shows the Tailscale card with state, name, addresses, HTTPS and hints.

**Independent Test**: fake daemon reports a running node with a serve entry → `GET /tailscale` and the card show running, the name, the addresses and the HTTPS URL.

- [X] T009 [US1] `GET /tailscale` in `putio-sync-api/src/routes/system/tailscale.rs` per contracts/tailscale-api.md (urls, hints for not found / needs login / userspace / HTTPS without serve / HTTPS off, tailnet_bind or its error, settings, identity of this request); route in the protected router in `putio-sync-api/src/lib.rs`
- [X] T010 [US1] Integration tests in `putio-sync-api/tests/tailscale.rs`: details from a fake socket; not found; needs login; token required; setup mode answers 503
- [X] T011 [US1] Settings group `tailscale` (GET/PUT/preview) in `putio-sync-api/src/routes/system/settings.rs` and its help in `assistant.rs`; live fields `identity_sign_in`, `allowed_users`, `socket` (service reconfigured on save), `tailnet_only` needs a restart; tests in `putio-sync-api/tests/settings.rs` and the group list in `tests/settings_assistant.rs`
- [X] T012 [US1] UI: `putio-sync-api/ui/tailscale.js` (group definition with a details panel from `GET /tailscale`), wired into `putio-sync-api/ui/pages/settings.js`; asset listed in `putio-sync-api/src/ui.rs`; styles in `app.css`

## Phase 4: User Story 2 - Sign in with Tailscale identity (P1)

**Goal**: an allowed tailnet user goes straight in; everybody else needs the token.

**Independent Test**: loopback + `Tailscale-User-Login: alice@example.com` reaches `/status` without a token; mallory, LAN source, or no `ConnectInfo` get 401.

- [X] T013 [US2] Identity path in `putio-sync-api/src/middleware/auth.rs` per research R4; attach `Identity` as an extension; count `allowed`/`refused`; log refusals at info
- [X] T014 [US2] `/auth/status` reports `tailscale` identity and `tailscale_sign_in` in `putio-sync-api/src/routes/system/auth.rs`
- [X] T015 [US2] Integration tests in `putio-sync-api/tests/tailscale.rs`: serve header allowed; not allowed; header from LAN; no ConnectInfo; whois via fake socket allowed; tagged; daemon down → 401; sign-in off ignores identity; empty allow-list; token still works; `/auth/status` identity
- [X] T016 [US2] UI shell: `putio-sync-api/ui/api.js` keeps the identity from `/auth/status`; `app.js` routes identified users past the sign-in page and shows "login (Tailscale)" in the header (`index.html`); a 401 clears it; `pages/login.js` goes straight in when identified
- [X] T017 [US2] UI: turning identity sign-in on with an empty allow-list fills it with the node owner's login (`ui/tailscale.js`)
- [X] T018 [US2] Doctor check `tailscale` (Connectivity) in `putio-sync-doctor/src/checks/net.rs` and `report.rs`: ok when no feature on or all well; warning for empty allow-list with identity on, or a feature on and daemon not running/reachable; unit tests

## Phase 5: User Story 3 - Listen on the tailnet address only (P2)

**Goal**: with `tailnet_only`, the daemon listens only on the tailnet IPv4 address.

**Independent Test**: fake status with `100.101.102.103` → resolved bind is `100.101.102.103:<port>`; without an address the listener waits and never binds `[api] bind`.

- [X] T019 [US3] Listener in `putio-sync-cli/src/api_listener.rs`: when `tailnet_only`, resolve with `tailnet_bind`; if not ready, retry every 5 s for 5 minutes in the background with a log each minute, then an error log; never fall back; unit test of the resolution and the no-fallback path
- [X] T020 [US3] PUT `/settings/tailscale` refuses `tailnet_only` turning on without a tailnet bind (400 with the reason); save message says a restart is needed; tests
- [X] T021 [US3] UI confirm before saving `tailnet_only` on, naming the addresses the UI moves to (`ui/tailscale.js` + a `beforeSave` hook in `pages/settings.js`)
- [X] T022 [US3] Radarr/Sonarr candidate hosts put the tailnet address first when `tailnet_only` is on (`putio-sync-api/src/routes/arr.rs`, `lib.rs` refresh); test

## Phase 6: User Story 4 - Tailscale details for other clients (P3)

- [ ] T023 [US4] Document `GET /tailscale` for the mobile app and scripts in README (API section); covered by T009/T010

## Phase 7: Polish

- [ ] T024 README: Tailscale section (socket mount for Docker, sidecar layout, Unraid integration, `tailscale serve`, loopback trust, Funnel needs the token, bridge-network caveat)
- [ ] T025 `CHANGELOG.md` entry; remove 036 from `ROADMAP.md`
- [ ] T026 UI asset checks (`node --check`, size cap) pass; check the card and identity flow in the browser against a mock backend
- [ ] T027 Merge `origin/main`, run `TMPDIR=<scratch> make ci`, push

## Dependencies

- T001–T002 → T003–T008 → stories. US1 (T009–T012) and US2 (T013–T018) both need T006/T008; US2's UI reuses T012's module. US3 needs T006 and T011.
- Parallel: T003, T004, T005, T007 are separate files.

## Implementation Strategy

MVP is US1 + US2 (both P1). Then US3, then US4/polish. All tasks are in scope for this branch.
