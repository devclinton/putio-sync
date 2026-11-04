# Research: Listen Addresses and Interfaces

## R1 Reading interface addresses

- Decision: `if-addrs` 0.15 in `core`; `interface_addrs(name)` returns the interface's IPv4
  addresses and its IPv6 addresses that are not link-local (`fe80::/10`), in OS order.
- Rationale: small, maintained, covers Linux/macOS (getifaddrs) and Windows
  (GetAdaptersAddresses) with no `unsafe` in our code. Its deps (`libc`, `windows-sys`) are
  already in the tree.
- Alternatives: `libc::getifaddrs` (unsafe, unix only), `nix::ifaddrs` (unix only, feature
  pull), parsing `ip -j addr` (missing in slim images), `network-interface` (heavier).

## R2 IPv6 wildcard and IPv4 on one port

- Decision: bind through `socket2`: create the socket, set `SO_REUSEADDR` on unix (as tokio's
  `TcpListener::bind` does), set `IPV6_V6ONLY` on every IPv6 socket, bind, listen(1024),
  non-blocking, `tokio::net::TcpListener::from_std`.
- Rationale: Linux defaults to dual-stack `[::]`, which makes a following `0.0.0.0:P` fail
  with "address in use". V6-only makes each entry mean what it says. Users who want both
  list both.
- Alternatives: leave the OS default (order-dependent failures); tokio `TcpSocket` (no
  v6-only setter).

## R3 Following changing addresses

- Decision: one supervisor task per listener set. Tick every 1 s for the first 5 minutes,
  then every 30 s, while any entry is dynamic (interface or tailnet) or any wanted address
  is not bound. Static lists that bound everything stop ticking.
- On each tick: wanted = union of resolutions; bind wanted−bound (a failure is logged when
  the error for that address changes, so a busy port is not logged every second); stop
  bound−wanted (graceful: stop accepting; open connections finish).
- Waiting entries log a warning the first time and when the reason changes, and an error
  once after 5 minutes ("still no address"), then keep checking every 30 s.
- This replaces 036's "give up after 5 minutes" for tailnet-only: the listener now keeps
  waiting, which is what a sidecar that starts late needs. Never falls back.
- Alternatives: netlink / route-socket notifications (platform-specific, overkill for a
  NAS whose IP changes rarely).

## R4 Which local address a request used

- Decision: each bound listener serves `router.layer(Extension(ListenedOn(addr)))`, where
  `addr` is the listener's own local address (for `0.0.0.0:P` that is the wildcard).
  The check treats a wildcard `ListenedOn` as "any address of that family on P" and
  compares it with the new list's resolutions: covered when the new list binds the same
  wildcard, or (for a concrete address) the same address or the family's wildcard on the
  same port.
- With a wildcard `ListenedOn` and a new list without the wildcard, the request's real
  destination IP is unknown; the check then uses the `Host` header when it is an IP
  literal, else reports `unknown` and the UI confirms anyway (spec: "could not check").
- Rationale: changing the connect-info type would touch every `ConnectInfo<SocketAddr>`
  user (auth limiter, Tailscale identity). An extension per listener is local to the
  supervisor.

## R5 `server` flags

- Decision: `--bind` and `--port` become optional. Neither → `config.api_listen()` (the
  list, or tailnet-only). `--bind HOST` → one entry of that host (IP, `[v6]`, interface or
  `tailnet`; a full `host:port` is accepted too) with `--port`, else the first configured
  port. `--port P` alone → every entry of `api_listen()` with port P (dedup).
- A change for users who ran `server` with no flags and a non-default `[api] bind`: they now
  get their configured address. Called out in CHANGELOG.

## R6 Port reported to others

- Decision: `AppState.listening: Bound` (shared `Arc<RwLock<Vec<SocketAddr>>>`, in entry
  order). `api_port()` = first bound address's port, else `api_listen()`'s first port,
  else 15432. The Tailscale details' URLs and serve hint, Radarr/Sonarr hosts and category
  refresh, and the setup wizard's arr step use it. The doctor (no AppState) keeps the
  configured first port.

## R7 Setup mode

- Decision: setup mode runs the supervisor on `api_listen()` of the partial config (default
  when unparsable) with the setup router, prints the banner for the first bound address
  (waiting up to 10 s, else prints that it is waiting), and stops the supervisor
  gracefully (5 s) before the API starts. Port reuse right after is covered by the API
  supervisor's 1 s ticks.

## R8 Saving lists

- Decision: `BindList` serialises a single entry as a string, several as an array. The
  settings PUT writes `[api] bind` always (even when equal to the default) once the user
  saves the group, as other groups do; `toml_edit` keeps comments.
- Env: `PUTIO_SYNC_API_BIND` / `PUTIO_SYNC_METRICS_BIND` split on commas, entries trimmed.
