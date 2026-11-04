# Tasks: Listen Addresses and Interfaces

**Input**: Design documents from `specs/037-listen-addresses/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/listen-api.md

**Tests**: included; the constitution (III) and FR-018 require them.

## Phase 1: Setup

- [ ] T001 Add `if-addrs = "0.15"` to `putio-sync-core/Cargo.toml` and `socket2 = { version = "0.6", features = ["all"] }` to `putio-sync-api/Cargo.toml`; `cargo check` builds

## Phase 2: Foundational (blocks every story)

- [ ] T002 Create `putio-sync-core/src/listen.rs`: `ListenAddr` (`Ip(SocketAddr)`, `Interface { name, port }`, `Tailnet { port }`) with `FromStr` ("an interface name is 1–64 characters from `[A-Za-z0-9._-]` and not all digits/dots", port 1–65535, `tailnet` case-insensitive), `Display`, `port()`, `is_dynamic()`; `BindList` serde (string or array; one entry serialises as a string); `parse_list` for comma-separated env values; `validate_list(field, &[ListenAddr])` ("1–16 entries; no duplicate; per port, an IPv4 wildcard excludes other IPv4 `Ip` entries and an IPv6 wildcard other IPv6 `Ip` entries"); `validate_apart(api, metrics)`; `interface_addrs(name)` (IPv4 + non-link-local IPv6) and `interface_exists(name)`; unit tests for every form and error; register in `putio-sync-core/src/lib.rs`
- [ ] T003 Change `[api] bind` to `Option<BindList>` and `[metrics] bind` to `BindList` in `putio-sync-core/src/config.rs`; `ApiSettings.bind: Vec<ListenAddr>` with `port()`; `resolve_api` / new `resolve_metrics` returning `MetricsListen { settings, bind }` read env lists; `Config::api_listen()` applying `tailnet_only` (one `tailnet:<port>` per distinct port); call the list rules from `Config::validate`; update config tests (single string unchanged, list, env list, tailnet-only)
- [ ] T004 Drop `bind` from `MetricsSettings` and `server::bind`, add `server::router(path, telemetry)` in `putio-sync-telemetry/src/server.rs`; fix `putio-sync-core/src/setup.rs` (wizard metrics bind) and every compile error from T003 across the workspace (`resolve_api().bind.port()` → `.port()` for now)
- [ ] T005 Create `putio-sync-api/src/listen.rs`: `Bound` (shared ordered list), `ListenedOn(SocketAddr)` extension, `resolve(entry, &TailscaleService) -> Gives`, `bind_one` via socket2 (v6-only on IPv6, reuseaddr on unix), and `Supervisor::spawn(name, entries, tailscale, router) -> ListenerHandle { bound, stop(grace) }` with the tick schedule (1 s for 5 min then 30 s, only while dynamic or unbound), per-address logs on change, never binding unnamed addresses; tests on loopback: two entries, one port busy, a resolver whose answer changes (test seam), stop frees the port
- [ ] T006 Add `listening: Bound` and `api_port()` (first bound port, else `api_listen()`'s first port, else 15432) and `with_listening(Bound)` to `AppState` in `putio-sync-api/src/lib.rs`; export `listen`

**Checkpoint**: the workspace builds; config and supervisor tests pass.

## Phase 3: User Story 1 - Several addresses and IPv6 (P1) 🎯 MVP

**Goal**: the daemon listens on every listed address. **Independent test**: `bind = ["127.0.0.1:P", "[::1]:P"]`, both answer.

- [ ] T007 [US1] Rewrite `putio-sync-cli/src/api_listener.rs` on the supervisor: `config.api_listen()`, the router with `AppState.listening`, logs; remove the 036 wait loop (the supervisor waits); keep its tests' intent (tailnet waits, never falls back) against the supervisor
- [ ] T008 [P] [US1] Metrics listener on the supervisor in `putio-sync-cli/src/telemetry.rs` (its own `TailscaleService` from `[tailscale] socket`)
- [ ] T009 [US1] Setup mode on the supervisor in `putio-sync-cli/src/commands/setup_mode.rs`: banner for the first bound address (wait up to 10 s), graceful stop before the API starts
- [ ] T010 [US1] Integration test in `putio-sync-api/tests/network.rs`: supervisor with the real router on `127.0.0.1` and `[::1]` (skip IPv6 when the host has none), `/health` on both, `Bound` lists both

## Phase 4: User Story 2 - Interface names (P1)

**Goal**: `eth0:P` follows the interface. **Independent test**: the loopback interface's name binds 127.0.0.1.

- [ ] T011 [US2] Test in `putio-sync-core/src/listen.rs`: the interface holding 127.0.0.1 resolves to it; an unknown name gives nothing
- [ ] T012 [US2] Test in `putio-sync-api/src/listen.rs`: an interface entry for the loopback interface binds; a missing interface waits and logs, others bind
- [ ] T013 [US2] Tailnet entries: the 036 tests (fake tailscaled under `/tmp`) through the supervisor in `putio-sync-cli/src/api_listener.rs` or `putio-sync-api/src/listen.rs`: waits, then binds the tailnet address

## Phase 5: User Story 3 - UI with lockout warning (P1)

**Goal**: Settings → Network edits the lists, warns before cutting the page off. **Independent test**: change to `192.0.2.1:P` from `127.0.0.1:P`, check says `cut_off`.

- [ ] T014 [US3] Create `putio-sync-api/src/routes/system/network.rs`: `GET /listen`, `GET/PUT /settings/network`, `POST /settings/network/preview`, `POST /settings/network/check` per contracts/listen-api.md (refuse when no API entry gives an address now; restart message; env/tailnet-only notes; `this_page` from `ListenedOn` and `Host`; `warnings` when identity sign-in is on and an API entry is on loopback); mount in the system router; add `network` to `groups()` in `putio-sync-api/src/routes/system/settings.rs` (nothing live)
- [ ] T015 [US3] Make sure `ListenedOn` reaches handlers: `api_listener` / server / tests attach it; `create_app` without it gives `unknown`
- [ ] T016 [US3] Integration tests in `putio-sync-api/tests/network.rs`: GET/PUT round trip writes a list to the file and one entry as a string; malformed entry 400; nothing-resolves 400; check `kept` / `cut_off` / `unknown`; `/listen` shape; 401 without token; 503 in setup mode
- [ ] T017 [P] [US3] Create `putio-sync-api/ui/network.js` (Listening-now panel from `/listen`, overrides notes, `networkBeforeSave` calling `/settings/network/check` and confirming on `cut_off`/`unknown` with `ui_at`); add the `network` group (two list fields) to `putio-sync-api/ui/pages/settings.js`; register the asset in `putio-sync-api/src/ui.rs` and its asset tests
- [ ] T018 [P] [US3] Doctor `listen` check in `putio-sync-doctor/src/checks/net.rs` + `CheckId::Listen` ("Listening addresses", Connectivity) in `putio-sync-doctor/src/report.rs`: waiting interface entries; identity sign-in with a loopback API entry; tailscale check uses `api_listen()` tailnet entries; unit tests
- [ ] T019 [US3] Update settings/assistant tests listing the groups (`putio-sync-api/tests/settings_assistant.rs` and any group-count test)

## Phase 6: User Story 4 - Ports reported where the UI is (P2)

- [ ] T020 [US4] Use `state.api_port()` in `putio-sync-api/src/routes/system/tailscale.rs` (details, hints, the tailnet-only save in settings.rs), `putio-sync-api/src/routes/arr.rs` (`listen_port`), `spawn_arr_refresh` in `putio-sync-api/src/lib.rs`, and `putio-sync-api/src/routes/setup.rs`; the Tailscale `loopback_only` check uses the effective list
- [ ] T021 [US4] Tests: `api_port()` from bound and fallback; `/tailscale` reports the bound port in `putio-sync-api/tests/tailscale.rs`

## Phase 7: User Story 5 - `server` uses the configured addresses (P2)

- [ ] T022 [US5] `--bind`/`--port` optional in `putio-sync-cli/src/cli.rs`; flag merging (`server_listen(config, bind, port)`) per the contract table with unit tests; `putio-sync-cli/src/commands/server.rs` on the supervisor with graceful Ctrl-C, `AppState.listening` wired (expose a `create_app` variant taking `Bound` in `putio-sync-api/src/lib.rs`); fix `main.rs` dispatch and the doc example

## Phase 8: Polish

- [ ] T023 [P] `config.example.toml`: `[api] bind` and `[metrics] bind` forms, lists, interfaces, IPv6, tailnet, env lists
- [ ] T024 [P] README: listen addresses section, `server` flags, API table rows (`/listen`, `/settings/network`), the loopback + identity caveat; update the 036 tailnet-only text (keeps waiting, `server` honours it)
- [ ] T025 CHANGELOG entry; remove 037 from `ROADMAP.md`
- [ ] T026 Merge `origin/main`, `TMPDIR=<scratch> make ci`, push

## Dependencies

- T001 → T002 → T003 → T004 → T005 → T006 → all stories.
- US1 (T007–T010) before US2 tests T012–T013 (they use the supervisor wiring) and US5.
- US3 needs T006 (`Bound`), independent of US4/US5. US4 needs T006. US5 needs T005–T006.
- Polish last.

## Parallel examples

- After T006: T008, T017, T018 touch different files.
- T023 and T024 together.

## Implementation strategy

MVP is US1 (several addresses through one supervisor). Then US2 (interfaces, mostly tests
of the same code), US3 (UI), US4/US5 (036's limits), polish. All stories are in scope.
