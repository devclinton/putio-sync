# Feature Specification: Listen Addresses and Interfaces

**Feature Branch**: `037-listen-addresses` (built on `claude/agitated-chandrasekhar-4d9e06`)

**Created**: 2026-11-03

**Status**: Draft

**Input**: User description: "037 Listen address and interface. `[api] bind` takes one
`ip:port` today. Allow several addresses, binding by interface name (`eth0`, `tailscale0`)
so a changing IP does not matter, IPv6, and a separate address for `/metrics`. Settable
from the UI, with a warning before saving an address that would lock the UI out. Build on
036: tailnet-only becomes one address kind (`tailnet`), `server --port/--bind` and the
reported port (Tailscale card, Radarr/Sonarr host) follow the addresses actually listened
on, and `server` honours the configured addresses (incl. tailnet only) when no flag is
given."

## Context

- The API and web UI listen on one `[api] bind` address, `ip:port`, default
  `0.0.0.0:15432`. Prometheus metrics have their own listener, `[metrics] bind`, default
  `0.0.0.0:9742`, also a single `ip:port`.
- 036 added `[tailscale] tailnet_only`: the daemon listens on this node's tailnet IPv4
  address with `[api] bind`'s port, waits up to 5 minutes for Tailscale at start, and never
  falls back to another address. Saving it is refused when Tailscale gives no address.
- Known 036 limits this feature fixes: `server --port N` makes the Tailscale card and the
  Radarr/Sonarr host list report `[api] bind`'s port instead of N, and `server` ignores
  tailnet-only and uses its `--bind` (default `0.0.0.0`).
- 036's identity sign-in trusts `tailscale serve`'s identity headers from loopback peers.
- 013's setup mode serves the setup wizard on the API address when the config is
  incomplete. The snap CI test checks setup mode on `127.0.0.1:15432`.
- Docker (bridge network) and the snap use the defaults; existing installs must keep
  listening where they do today.

## Decisions

- Q: How are several addresses written? → A: `[api] bind` (and `[metrics] bind`) take
  either one address, as today, or a list of addresses. One address keeps meaning exactly
  what it means today.
- Q: What can an address be? → A: `host:port` where host is an IPv4 address, an IPv6
  address in brackets (`[::]:15432`, `[fd00::5]:15432`), a network interface name
  (`eth0:15432`, `tailscale0:15432`, meaning every address that interface has), or the
  word `tailnet` (`tailnet:15432`, this node's Tailscale address, as 036's tailnet-only
  finds it).
- Q: What becomes of `[tailscale] tailnet_only`? → A: it stays, as a shorthand: when on,
  the API listens on `tailnet:<port>` for each port of `[api] bind` and nothing else. The
  Tailscale card keeps its switch. The listen settings show that tailnet-only overrides
  the list.
- Q: When do new addresses apply? → A: a saved address list applies at the next start, as
  `tailnet_only` does today. While running, interface and `tailnet` addresses are checked
  again every 30 seconds: a new address is listened on and a vanished one closed, so a
  changed IP (DHCP, re-registered Tailscale node) needs no restart.
- Q: What if an interface or Tailscale has no address at start? → A: that entry waits and
  keeps checking; other entries listen at once. Nothing ever falls back to all addresses.
  Unlike 036, a tailnet entry no longer gives up after 5 minutes: it logs an error then and
  keeps checking every 30 seconds.
- Q: What does `server` listen on? → A: with no `--bind` and no `--port`, the configured
  `[api] bind` list (and tailnet-only). `--bind ADDR` replaces the list with that one
  address (any of the forms above; port from `--port`, else the first configured port).
  `--port N` alone keeps the configured hosts with port N.
- Q: The lockout warning? → A: before saving, the UI shows the addresses the new list
  gives now and, when the address the browser is using would no longer be listened on
  after a restart, a confirm naming where the UI will be. A save where no entry gives any
  address now is refused.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Listen on several addresses, including IPv6 (Priority: P1)

The user lists the addresses putio-sync should listen on, for example the LAN address and
loopback, or `0.0.0.0:15432` and `[::]:15432` for IPv4 and IPv6. After a restart the API
and UI answer on each of them, and the log says which ones.

**Why this priority**: it is the core of the feature; everything else is a kind of address
or a way to set it.

**Independent Test**: set `bind = ["127.0.0.1:P", "[::1]:P"]`, start the daemon, and reach
`/health` on both `http://127.0.0.1:P` and `http://[::1]:P`.

**Acceptance Scenarios**:

1. **Given** `bind = "0.0.0.0:15432"` (or no `bind`), **Then** the daemon listens exactly as
   today.
2. **Given** a list of two addresses, **Then** both answer, and the startup log names both.
3. **Given** an entry whose port is taken, **Then** that entry's failure is logged and the
   other entries still listen.
4. **Given** a malformed entry (`nope`, `1.2.3.4`, `[::1]`, `eth0` without a port, a port
   above 65535), **Then** config validation fails with the entry and the forms allowed.
5. **Given** an empty list, **Then** validation fails ("at least one address").
6. **Given** the same address twice, or a wildcard (`0.0.0.0`) with a specific IPv4 address
   on the same port, **Then** validation fails naming the overlap.

---

### User Story 2 - Bind by interface name (Priority: P1)

The user writes `eth0:15432` instead of the NAS's current IP. putio-sync listens on
whatever addresses `eth0` has; when DHCP changes the IP, it follows without a restart.
`tailscale0:15432` works the same for a host-mode Tailscale.

**Why this priority**: the roadmap's main reason for the feature: a changing IP must not
break the UI.

**Independent Test**: with the loopback interface's name (`lo` / `lo0`) and a free port,
the daemon listens on 127.0.0.1 (and ::1 if present) for that port.

**Acceptance Scenarios**:

1. **Given** `eth0:15432` and eth0 has `192.168.1.20` and `fe80::1`, **Then** putio-sync
   listens on `192.168.1.20:15432`; link-local IPv6 addresses are skipped.
2. **Given** eth0's address changes while running, **Then** within 30 seconds the new
   address answers and the old one is closed, logged once each.
3. **Given** the interface does not exist or has no address at start, **Then** that entry
   logs that it is waiting and listens as soon as an address appears; other entries are
   not held up.
4. **Given** `tailnet:15432`, **Then** it listens on the Tailscale IPv4 address, waiting for
   Tailscale as 036 does, and follows a changed tailnet address.

---

### User Story 3 - Set the addresses from the UI with a lockout warning (Priority: P1)

Settings gets a Network group with the API addresses and the metrics addresses (one per
line). The preview shows what each line gives now (the addresses an interface has, "waiting:
no such interface"). Saving says a restart is needed. When the browser's current address
would not be listened on after the restart, a confirm says so first and names the
addresses the UI will be at.

**Why this priority**: the roadmap asks for it, and a wrong address on a headless NAS locks
the user out, so the warning matters as much as the field.

**Independent Test**: open Settings over `http://127.0.0.1:P`, change the API addresses to
`192.0.2.1:P` only: the preview flags that this page would be cut off and the save asks
first.

**Acceptance Scenarios**:

1. **Given** the group is opened, **Then** it shows the configured lists and the addresses
   listened on right now.
2. **Given** a change that keeps the browser's address, **Then** saving needs no confirm and
   the message says a restart applies it.
3. **Given** a change that drops it (other IP, other port, loopback only from a LAN
   browser), **Then** the confirm names the new addresses; cancelling saves nothing.
4. **Given** a list where no entry gives an address now, **Then** the save is refused with
   the reason.
5. **Given** tailnet-only is on, **Then** the group says the API list is overridden by it.
6. **Given** a list that would only listen on loopback while identity sign-in is on, **Then**
   the preview and the doctor warn that only `tailscale serve` should reach putio-sync on
   loopback.
7. **Given** the environment overrides an address (`PUTIO_SYNC_API_BIND` /
   `PUTIO_SYNC_METRICS_BIND`), **Then** the group says the environment wins.

---

### User Story 4 - Ports reported where the UI really is (Priority: P2)

The Tailscale card's addresses, its `tailscale serve` hint, and the Radarr/Sonarr
"putio-sync host" and port use the addresses actually listened on, so `server --port 8080`
and a list with a non-default port report the right port.

**Why this priority**: fixes 036's known limit; wrong ports make auto-setup point
Radarr/Sonarr at nothing.

**Independent Test**: start `server --port P`: `GET /tailscale` reports port P and the
Radarr/Sonarr setup uses P.

**Acceptance Scenarios**:

1. **Given** `server --port P`, **Then** `/tailscale` and the Radarr/Sonarr client use port P.
2. **Given** a list with two ports, **Then** the first entry's port is "the" port reported
   and the Network group lists every address listened on.
3. **Given** the listener has not bound yet (waiting for an interface), **Then** the
   configured first port is reported.

---

### User Story 5 - `server` uses the configured addresses (Priority: P2)

`putio-sync server` with no address flags listens where the config says, including
tailnet-only, so it behaves like the daemon.

**Independent Test**: config `bind = "127.0.0.1:P"`, run `server` with no flags: it listens
on `127.0.0.1:P`.

**Acceptance Scenarios**:

1. **Given** no flags, **Then** the configured list (or tailnet-only) is used.
2. **Given** `--bind eth0` (or an IP), **Then** only that address, with `--port` or the
   configured port.
3. **Given** `--port P` alone, **Then** the configured hosts with port P.

### Edge Cases

- `[::]` on a host with IPv6 disabled: that entry fails to bind and is logged; the rest
  run.
- `[::]:P` together with `0.0.0.0:P`: on systems where `[::]` also takes IPv4 the second
  bind would fail, so IPv6 wildcard sockets accept IPv6 only and both entries work.
- An interface with several IPv4 addresses: each is listened on.
- An interface name that looks like a host name (`nas:15432`): names that are not
  interfaces are treated as missing interfaces (waiting), never resolved through DNS; the
  preview says "no such interface".
- Setup mode (013) listens on the same list; an interface still waiting means setup mode
  waits for it, with the reason in the log, and the snap's default `127.0.0.1` keeps
  working.
- A list shared between API and metrics on the same port: validation fails naming both.
- Metrics follow the same rules but are not covered by the lockout confirm (they are not
  the UI).
- The current listener is a wildcard (`0.0.0.0:P`) and the browser used a name
  (`nas.local`): the exact address cannot be told, so the check says it could not check
  and the UI confirms anyway; an IP in the address bar is checked exactly.
- Through `tailscale serve` or another reverse proxy, the browser's address is the proxy's
  and the check compares the address the request actually arrived on.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: `[api] bind` and `[metrics] bind` MUST accept one address (string) or a list
  of addresses. The defaults (`0.0.0.0:15432`, `0.0.0.0:9742`) and one-address configs MUST
  behave as today.
- **FR-002**: An address MUST be `IPv4:port`, `[IPv6]:port`, `interface:port` or
  `tailnet:port`; anything else MUST fail validation with the entry and the allowed forms.
  Lists MUST be non-empty, at most 16 entries, without duplicates or overlapping
  wildcards on one port, and API and metrics MUST NOT share a concrete address and port.
- **FR-003**: `PUTIO_SYNC_API_BIND` and `PUTIO_SYNC_METRICS_BIND` MUST accept one address or
  a comma-separated list and win over the file, as today.
- **FR-004**: The daemon MUST listen on every address the entries give, each independently:
  one entry failing to bind or waiting MUST NOT stop others, and nothing MUST fall back to
  all addresses.
- **FR-005**: An interface entry MUST listen on the interface's IPv4 addresses and its
  non-link-local IPv6 addresses. A missing interface or one without addresses MUST wait.
- **FR-006**: Interface and `tailnet` entries MUST be re-checked every 30 seconds while
  running; new addresses MUST be listened on and vanished ones closed, logged at info.
- **FR-007**: `[tailscale] tailnet_only = true` MUST mean the API listens on `tailnet:<port>`
  for each distinct port of the API list, and nothing else; 036's refusal to save it
  without a tailnet address stays.
- **FR-008**: IPv6 wildcard listeners MUST accept IPv6 only so `0.0.0.0:P` and `[::]:P` can
  be listed together.
- **FR-009**: The running process MUST know which addresses it listens on; the Tailscale
  details and Radarr/Sonarr auto-setup MUST use the first listened address's port (the
  configured first port while nothing listens yet).
- **FR-010**: `server` with no `--bind`/`--port` MUST use the configured list and
  tailnet-only; `--bind` MUST replace the list with one address of any form; `--port`
  MUST set the port of every entry.
- **FR-011**: Setup mode MUST listen on the same resolved list as the daemon would.
- **FR-012**: Settings MUST have a `network` group (GET, PUT, preview, assistant, export)
  with the API list, the metrics list, and read-only: the addresses listened on now,
  whether tailnet-only overrides the API list, and whether the environment overrides
  either list.
- **FR-013**: The `network` preview MUST report, for each entry, what it gives now (addresses
  or the reason it would wait), and whether the request's own local address (the address
  and port it arrived on) is still covered by the new API list, with the API addresses the
  UI would be at.
- **FR-014**: A `network` save MUST be refused when no API entry gives an address now, and
  its message MUST say a restart is needed. The UI MUST confirm before a save the check
  marks as cutting off the current page, or as unable to tell.
- **FR-015**: The doctor MUST warn when identity sign-in is on and the API listens on
  loopback, saying only `tailscale serve` may reach putio-sync there; and when an
  interface or `tailnet` entry has no address.
- **FR-016**: Startup MUST log each address listened on, each entry waiting and why, and
  each bind failure, as structured fields.
- **FR-017**: `config.example.toml` and README MUST document the forms, lists, interface
  following, IPv6, the `server` flags and the loopback/identity caveat.
- **FR-018**: Tests MUST NOT need a particular network interface beyond loopback.

### Key Entities

- **Listen address (entry)**: one configured item: kind (IP, interface, tailnet), the
  IP or interface name, and the port.
- **Listen plan**: the entries for the API or metrics after env and tailnet-only, and what
  each gives now (socket addresses, or the reason it waits).
- **Bound addresses**: the socket addresses the process is listening on right now, per
  listener (API, metrics).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Every existing config (single address or none) listens on exactly the same
  address after upgrade; the Docker and snap CI checks pass unchanged.
- **SC-002**: After the bound interface's IP changes, the UI answers on the new IP within
  30 seconds without a restart.
- **SC-003**: A save that would cut the current browser off always shows a confirm first;
  a save where nothing would listen is always refused.
- **SC-004**: With `server --port N`, every address putio-sync shows or gives to
  Radarr/Sonarr uses N.
- **SC-005**: API and UI are reachable over IPv6 by listing an IPv6 address, alongside IPv4.

## Assumptions

- Interface addresses are read from the operating system; containers on a bridge network
  see only their own interfaces (`eth0` inside the container), which is what they can bind.
- "The" port reported to Radarr/Sonarr and the Tailscale card is the first API address's
  port; users with several ports put the one others should use first.
- 30 seconds is an acceptable delay to follow an IP change.
- Metrics `path` and `enabled` keep their current settings; the Network group shows only the
  addresses (metrics enable/path stay where they are configured today).
- Mobile app (035) is out of scope.
