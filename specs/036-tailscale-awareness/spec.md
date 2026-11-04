# Feature Specification: Tailscale Awareness

**Feature Branch**: `036-tailscale-awareness` (built on `claude/quizzical-shtern-aa4514`)

**Created**: 2026-11-03

**Status**: Draft

**Input**: User description: "036 Tailscale awareness. Detect Tailscale on the host and offer
to listen on the tailnet address only, use Tailscale identity to sign in (the user and node
from Tailscale, so no password inside the tailnet), and show the MagicDNS name and HTTPS
address in the UI and the mobile app. It should work with `tailscale serve`, and with
Tailscale in a sidecar container."

## Context

- putio-sync's API and web UI listen on one `[api] bind` address (`0.0.0.0:15432` by
  default). Every protected request needs the API token; the web UI asks for it on its
  sign-in page and keeps it in the browser.
- It runs on a NAS in Docker (bridge network), as a snap, or as a plain binary. Tailscale
  can be on the host, inside the container (Unraid's Tailscale integration), or in a
  sidecar container that shares its network.
- Tailscale's local daemon answers questions about the node over a local socket (or its
  CLI): whether it is running and signed in, the node's tailnet addresses, its MagicDNS
  name, whether HTTPS certificates are available, what `tailscale serve` forwards, and who
  (user and device) owns a given tailnet address.
- `tailscale serve` proxies `https://<node>.<tailnet>.ts.net` to a local port and adds the
  signed-in user's login and name as request headers. It connects from the loopback
  address. In userspace-networking mode (common in sidecars) the node has no tailnet
  address of its own and `serve` is the only way in.
- 037 (next) widens `[api] bind`: several addresses, interface names, IPv6, a separate
  `/metrics` address, set from the UI with a lockout warning. This feature does not do any
  of that.
- 013's setup mode: an unconfigured daemon serves only the setup wizard; other routes 503.
- 018's Radarr/Sonarr auto-setup tries a list of candidate hosts for "the putio-sync host".
- 035 (mobile app) is not built yet.

## Decisions

- Q: Does putio-sync change the Tailscale setup (run `tailscale serve`, sign in)? → A: no.
  It only reads. Where a step is needed it shows the exact command for the user to run.
- Q: Who may sign in with their Tailscale identity? → A: only the logins listed in an
  allow-list. Turning identity sign-in on from the UI fills the list with the login that
  owns this node. Tagged devices (no user) are never signed in. An empty list signs nobody
  in, and the Tailscale card and doctor say so.
- Q: Is identity sign-in on by default? → A: no, it is opt-in, like tailnet-only listening.
  The API token keeps working in every mode, for scripts and Radarr/Sonarr.
- Q: Which requests may carry a Tailscale identity? → A: (a) a request from the loopback
  address carrying `tailscale serve`'s identity headers, and (b) a request whose source
  is a tailnet address that the local Tailscale daemon resolves to a user. Headers from any
  other source are ignored. A request through Tailscale Funnel carries no identity and
  needs the token.
- Q: How does "listen on the tailnet address only" relate to `[api] bind`? → A: a
  separate switch. When on, the listener binds this node's tailnet IPv4 address with the
  port from `[api] bind`. 037 can later fold it into its address list as one more address
  kind.
- Q: The mobile app? → A: it does not exist yet. The Tailscale details are served by the
  API, so 035 can show them; the app itself is out of scope.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - See whether Tailscale is here and its addresses (Priority: P1)

Settings has a Tailscale card. It says whether Tailscale was found and how putio-sync
reached it, its state (running, signed out, stopped), the node's MagicDNS name and tailnet
addresses, and the addresses that open putio-sync over the tailnet: the plain
`http://<magicdns-name>:<port>` and, when `tailscale serve` forwards to putio-sync, the
`https://<magicdns-name>` address. When HTTPS is available but `serve` is not set up, the
card shows the command to run. When Tailscale is not found, the card says how to make it
visible (socket mount for Docker, sidecar).

**Why this priority**: every other story starts from knowing whether Tailscale is there.

**Independent Test**: with a fake Tailscale daemon that reports a running node
`nas.tail1234.ts.net` with `100.101.102.103` and a `serve` entry for port 15432, open the
card: it shows running, the name, the address, `http://nas.tail1234.ts.net:15432` and
`https://nas.tail1234.ts.net`.

**Acceptance Scenarios**:

1. **Given** no Tailscale daemon reachable, **Then** the card says not found, with how to
   connect a socket or sidecar, and nothing else on the page breaks.
2. **Given** Tailscale is installed but signed out or stopped, **Then** the card shows that
   state and no addresses.
3. **Given** HTTPS certificates are available and no `serve` entry points at putio-sync's
   port, **Then** the card shows the `tailscale serve` command for this port.
4. **Given** userspace networking (no tailnet address on this machine), **Then** the card
   says direct tailnet access is not possible and `tailscale serve` is the way in.
5. **Given** an unconfigured daemon (setup mode), **Then** the Tailscale details route
   answers 503 like other routes.

---

### User Story 2 - Sign in with Tailscale identity (Priority: P1)

The user turns on identity sign-in in the Tailscale card. Afterwards, opening the UI from a
device of an allowed user on the tailnet (directly or through `tailscale serve`) goes
straight in without the token, and the header shows who they are signed in as. Anyone else
still sees the token sign-in page.

**Why this priority**: it is the main reason to use Tailscale with putio-sync: no password
inside the tailnet.

**Independent Test**: with identity sign-in on and `alice@example.com` allowed, a request
from loopback with serve's login header `alice@example.com` reaches a protected route
without a token; the same request with `mallory@example.com`, or from a LAN address, gets
401.

**Acceptance Scenarios**:

1. **Given** identity sign-in on and alice allowed, **When** alice opens the UI through
   `tailscale serve`, **Then** she lands on the Status page and the header shows "alice
   (Tailscale)" instead of a sign-out button.
2. **Given** the same, **When** alice opens `http://<tailnet-ip>:15432` from her laptop on
   the tailnet, **Then** putio-sync asks the Tailscale daemon who owns her address and lets
   her in.
3. **Given** a request from a tagged device, a user not on the list, a non-tailnet address,
   or with serve headers from a non-loopback source, **Then** it needs the token as before.
4. **Given** identity sign-in off, **Then** identity headers and tailnet addresses are
   ignored entirely.
5. **Given** the Tailscale daemon cannot be reached for a direct tailnet request, **Then**
   the request needs the token (fail closed) and the reason is logged.
6. **Given** identity sign-in on with an empty allow-list, **Then** nobody is signed in by
   identity, and the card and the doctor warn.
7. **Given** a valid API token, **Then** it works as before in every mode.

---

### User Story 3 - Listen on the tailnet address only (Priority: P2)

The card offers "Listen on the tailnet address only". After a confirm that names the
address the UI will move to, the setting is saved, and on the next start the listener binds
the node's tailnet address (with the configured port) instead of `[api] bind`, so the LAN
can no longer reach putio-sync.

**Why this priority**: valuable for a locked-down setup, but it changes reachability and
needs a restart.

**Independent Test**: with the fake daemon reporting `100.101.102.103` and the switch on,
start the listener: it binds `100.101.102.103:<port>`; with the switch off it binds
`[api] bind`.

**Acceptance Scenarios**:

1. **Given** Tailscale running with a tailnet IPv4 address, **When** the user turns the
   switch on, **Then** a confirm says the UI will only be reachable at
   `http://<tailnet-address>:<port>` and `http://<magicdns>:<port>` after a restart, and
   saving says a restart is needed.
2. **Given** Tailscale not found or no tailnet address (userspace), **Then** turning the
   switch on is refused with the reason.
3. **Given** the switch on and Tailscale not ready at startup (sidecar still starting),
   **Then** the listener keeps trying for a while and binds as soon as the address is
   available; until then it logs why it is waiting. It never falls back to `[api] bind`.
4. **Given** the switch on, **Then** Radarr/Sonarr auto-setup offers the tailnet address
   as the putio-sync host first.

---

### User Story 4 - Tailscale details for other clients (Priority: P3)

The API serves the Tailscale details (state, name, addresses, HTTPS address, identity of
the current request) so the mobile app (035) and scripts can show the same addresses.

**Independent Test**: `GET /tailscale` with a token returns the details as JSON.

### Edge Cases

- MagicDNS off: no name is shown; addresses are shown by IP only.
- HTTPS (certificates) off in the tailnet: no HTTPS address and no `serve` hint; the card
  says HTTPS is off in the tailnet's settings.
- `serve` forwards to a different port, or to putio-sync on a path: only an entry whose
  target is this port on the loopback host counts; a path other than `/` is shown as is.
- The Tailscale daemon answers slowly or with junk: detection gives up after a short
  timeout and reports "not reachable"; it never blocks a request for longer than that.
- Identity lookups are cached briefly per address so a page load does not ask the daemon
  for every request.
- Logins are compared ignoring case and surrounding spaces.
- A reverse proxy other than `serve` on loopback could forward forged identity headers;
  the docs say identity sign-in assumes only `serve` reaches putio-sync over loopback.
- Docker bridge with Tailscale on the host: `serve` traffic arrives from the bridge
  gateway, not loopback, so it needs the token; direct tailnet requests keep their
  tailnet source address and work when the host's socket is mounted.
- The node's tailnet address changes (re-registered node): the listener uses the address
  found at start; a restart picks up the new one. Binding by interface is 037's.
- Snap confinement may hide the socket: the card says not found.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: putio-sync MUST detect a local Tailscale daemon through its local socket
  (default path, overridable in config and by environment) and, failing that, its CLI,
  and report one of: not found, not reachable, stopped, signed out (needs login), running.
- **FR-002**: When running, detection MUST report the node's MagicDNS name, its tailnet
  addresses, whether HTTPS certificates are available, whether it uses userspace
  networking, the login that owns the node, and the `serve` entries that forward to
  putio-sync's port.
- **FR-003**: Detection MUST time out within 3 seconds and MUST be cached for a short time
  (10 seconds) so the UI's polling does not hammer the daemon.
- **FR-004**: An authenticated `GET /tailscale` MUST return the detection, the addresses
  that open putio-sync over the tailnet, the commands for missing steps, the current
  tailnet-only and identity settings, and the Tailscale identity of the current request
  (if any). It MUST answer 503 in setup mode.
- **FR-005**: Config MUST gain a `[tailscale]` group with `tailnet_only` (default off),
  `identity_sign_in` (default off), `allowed_users` (logins, default empty) and `socket`
  (optional path). The group MUST be readable and savable from Settings with preview, like
  the other groups, and listed in the configuration assistant.
- **FR-006**: With identity sign-in on, a request without a valid token MUST be let in when
  (a) it comes from a loopback address and carries `serve`'s login header for an allowed
  login, or (b) its source is a tailnet address the Tailscale daemon resolves to an allowed
  user's non-tagged device. Every other request MUST be handled as before.
- **FR-007**: Identity sign-in MUST fail closed: an unreachable daemon, a lookup error or a
  timeout means no identity.
- **FR-008**: Identity lookups for a source address MUST be cached for at most 60 seconds.
- **FR-009**: `GET /auth/status` MUST report the request's Tailscale identity (login and
  display name) when identity sign-in lets it in, so the UI can skip the sign-in page.
- **FR-010**: The web UI MUST go straight in for an identified request and show the
  signed-in Tailscale user in the header; a later 401 MUST send it to the sign-in page as
  usual.
- **FR-011**: With `tailnet_only` on, the daemon's listener MUST bind the node's tailnet
  IPv4 address with `[api] bind`'s port, and MUST NOT fall back to `[api] bind`. When the
  address is not available at start it MUST retry (every 5 seconds, for up to 5 minutes),
  logging the reason, and then give up with an error log.
- **FR-012**: Saving `tailnet_only = true` MUST be refused (400 with the reason) when
  Tailscale is not running or has no tailnet IPv4 address. The UI MUST confirm before
  saving, naming the address the UI will move to, and the save message MUST say a restart
  is needed.
- **FR-013**: Turning identity sign-in on from the card with an empty allow-list MUST fill
  it with the node owner's login when known.
- **FR-014**: The doctor MUST report a Tailscale check: OK when no Tailscale feature is on
  or all is well; a warning when identity sign-in is on with an empty allow-list, or a
  Tailscale feature is on and the daemon is not reachable or not running.
- **FR-015**: Identity sign-ins MUST be counted in a metric labelled by path (`serve`,
  `whois`) and result (`allowed`, `refused`), and logged with the login and path at debug
  level (refusals at info), never with tokens.
- **FR-016**: Radarr/Sonarr auto-setup MUST put the tailnet address first among candidate
  hosts when `tailnet_only` is on and the address is known.
- **FR-017**: Tests MUST NOT depend on Tailscale being installed; detection is tested
  against a fake daemon.
- **FR-018**: README and `config.example.toml` MUST document the group, the Docker socket
  mount, the sidecar layout, `tailscale serve`, and the loopback trust assumption.

### Key Entities

- **Tailscale status**: state, how it was reached (socket or CLI), MagicDNS name, tailnet
  name, tailnet addresses, HTTPS available, userspace networking, owner login, serve
  entries (HTTPS port, path, target).
- **Tailscale identity**: login, display name, device name, how it was found (`serve` or
  `whois`).
- **Tailscale settings**: tailnet_only, identity_sign_in, allowed_users, socket.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: An allowed tailnet user opens the UI over the tailnet and reaches the Status
  page with no sign-in step.
- **SC-002**: A request that is not from an allowed tailnet user is never let in without
  the token (all refusal scenarios covered by tests).
- **SC-003**: The Tailscale card shows the right state for each of: not found, stopped,
  signed out, running, running in userspace mode; the details appear within 3 seconds even
  when the daemon hangs.
- **SC-004**: With tailnet-only on, the listener is not reachable on the LAN address after
  a restart, and is reachable on the tailnet address.
- **SC-005**: No test needs Tailscale installed, and `make ci` stays green.

## Assumptions

- The local Tailscale daemon's socket protocol and CLI JSON output keep their current shape
  (status, whois, serve config).
- Only IPv4 tailnet addresses are bound; IPv6 is 037's.
- `serve` is the only proxy that reaches putio-sync over loopback when identity sign-in is
  on.
- The mobile app (035) will read `GET /tailscale`; nothing app-side is built here.
- Tailnet-only and identity settings take effect: identity at once, tailnet-only on the
  next restart.
