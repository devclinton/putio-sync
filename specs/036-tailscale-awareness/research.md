# Research: Tailscale Awareness

## R1: How to ask Tailscale about the node

- **Decision**: LocalAPI over the unix socket first (`/var/run/tailscale/tailscaled.sock`,
  overridable by `[tailscale] socket` or `PUTIO_SYNC_TAILSCALE_SOCKET`), then the CLI
  (`tailscale status --json`, `tailscale serve status --json`, `tailscale whois --json
  <ip>`), found on `PATH` or at the macOS app path.
- **Calls**: `GET /localapi/v0/status`, `GET /localapi/v0/serve-config`,
  `GET /localapi/v0/whois?addr=<ip>` (404 = unknown peer). Requests carry
  `Host: local-tailscaled.sock` and `Sec-Tailscale: localapi`.
- **Rationale**: the socket is what a Docker container gets by mounting
  `/var/run/tailscale` (host or sidecar), and it needs no binary in the image. The CLI
  covers desktops (macOS app) where the socket is elsewhere or token-protected.
- **Alternatives**: CLI only (needs the binary in the image: rejected); reading
  interface addresses (gives no name, HTTPS or identity: rejected); `tsnet` (Go only).

## R2: HTTP over a unix socket without a new crate

- **Decision**: write `GET <path> HTTP/1.0` with `Connection: close` on a
  `tokio::net::UnixStream`, read to EOF (capped at 1 MiB), split the head at `\r\n\r\n`,
  parse the status code; tolerate `Transfer-Encoding: chunked` by de-chunking.
- **Rationale**: three GET calls with small JSON bodies; `reqwest` has no unix socket
  support and `hyper`/`hyperlocal` would be new direct dependencies (Principle VII).

## R3: Which identities to trust

- **Decision**:
  - `serve` path: only when the peer address from `ConnectInfo` is loopback, read
    `Tailscale-User-Login` (and `Tailscale-User-Name` for display). No `ConnectInfo` means
    no identity: the router must be served with connect info for this to work, and tests
    insert it explicitly.
  - `whois` path: when the peer address is in `100.64.0.0/10` or `fd7a:115c:a1e0::/48`,
    ask `whois`; accept when the node has no tags and the user's login is allowed.
  - Logins compare case-insensitively after trimming.
- **Rationale**: this is Tailscale's own model (`serve` sets the headers from `whois`
  and drops client-sent ones; tagged nodes get no user headers). Funnel traffic has no
  identity headers.
- **Known limit**: another proxy on loopback could forge the headers; documented.

## R4: Middleware order

- **Decision**: rate-limit check → valid token passes → identity (if on) passes → old
  behavior (no token: 401 uncounted; wrong token: counted). Identity is attached as a
  request extension.
- **Rationale**: a stale token in the browser doesn't block an identified user; the token
  stays first so scripts never wait on a `whois`.

## R5: Tailnet-only bind

- **Decision**: `tailnet_bind(status, port)`: needs state Running, not userspace (`TUN`
  true), and an IPv4 address in `TailscaleIPs`. The listener tries once at start; if not
  ready it spawns a task that tries every 5 s for 5 minutes, logging the reason each
  minute, then logs an error. It never falls back to `[api] bind`.
- **Rationale**: sidecars start in any order; falling back would silently expose the LAN.

## R6: HTTPS address and `serve`

- **Decision**: HTTPS is available when status `CertDomains` is non-empty. A `serve` entry
  counts when a `Web` handler's `Proxy` target is `http://127.0.0.1:<port>`,
  `http://localhost:<port>`, `localhost:<port>`, or `<port>`, for our port. The HTTPS
  address is `https://<host>` (port omitted when 443) plus the handler path. With
  HTTPS on and no entry, the hint is `tailscale serve --bg <port>`. Funnel is reported
  when `AllowFunnel` has the host:port.

## R7: Settings group

- **Decision**: group `tailscale` with fields `tailnet_only` (needs restart),
  `identity_sign_in`, `allowed_users`, `socket` (live: the service is reconfigured on save).
  PUT refuses `tailnet_only` turning on unless a fresh detection gives a tailnet bind.
  Validation in core: allowed logins non-empty without whitespace; `socket` non-empty.
