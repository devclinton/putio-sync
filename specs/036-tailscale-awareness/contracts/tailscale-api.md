# Contract: Tailscale API

## GET /tailscale (token or identity; 503 in setup mode)

```json
{
  "state": "running",
  "via": "socket",
  "version": "1.76.1",
  "dns_name": "nas.tail1234.ts.net",
  "tailnet": "example.com",
  "magic_dns": true,
  "addresses": ["100.101.102.103", "fd7a:115c:a1e0::1"],
  "https": true,
  "userspace": false,
  "owner_login": "alice@example.com",
  "serve": [{"url": "https://nas.tail1234.ts.net", "path": "/", "target": "http://127.0.0.1:15432", "funnel": false}],
  "port": 15432,
  "urls": ["https://nas.tail1234.ts.net", "http://nas.tail1234.ts.net:15432", "http://100.101.102.103:15432"],
  "hints": ["..."],
  "tailnet_bind": "100.101.102.103:15432",
  "tailnet_bind_error": null,
  "settings": {"tailnet_only": false, "identity_sign_in": true, "allowed_users": ["alice@example.com"]},
  "identity": {"login": "alice@example.com", "name": "Alice", "device": "laptop", "via": "whois"},
  "error": null
}
```

- Fields that do not apply are `null` or empty; `state` is always present.
- `urls` lists the HTTPS `serve` address first, then the MagicDNS and IP addresses
  (direct ones only when not userspace and the listener is not loopback-only).

## GET /auth/status (public)

Adds `"tailscale": {"login", "name", "via"}` when identity sign-in lets this request in,
and `"tailscale_sign_in": true` when identity sign-in is on. Both are left out otherwise,
so the response is unchanged while the feature is off.

## Settings group `tailscale`

- `GET /settings/tailscale` → `{"tailnet_only": false, "identity_sign_in": false,
  "allowed_users": [], "socket": null}`
- `PUT /settings/tailscale` same body. 400 on an invalid login, and when
  `tailnet_only` turns on without a tailnet address (`error` gives the reason).
  Blank logins are dropped and a blank `socket` means the default.
- `POST /settings/tailscale/preview` same body; standard preview.

## Protected routes

Without a valid token, a request is let in when identity sign-in is on and the request's
identity is allowed (see spec FR-006). Responses are otherwise unchanged.

## Metric

`putio_sync_tailscale_identity_total{via="serve|whois",result="allowed|refused"}`
