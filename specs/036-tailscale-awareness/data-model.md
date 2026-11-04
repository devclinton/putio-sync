# Data Model: Tailscale Awareness

No database changes.

## TailscaleConfig (`[tailscale]`, core)

| Field | Type | Default | Rule |
|-------|------|---------|------|
| `tailnet_only` | bool | false | applies at the next start |
| `identity_sign_in` | bool | false | applies at once |
| `allowed_users` | list of logins | [] | each non-empty, no whitespace; compared case-insensitively |
| `socket` | path, optional | `/var/run/tailscale/tailscaled.sock` | non-empty; env `PUTIO_SYNC_TAILSCALE_SOCKET` wins |

## Status (tailscale crate)

- `state`: `not_found` (no socket, no CLI) · `unreachable` (error or timeout) · `stopped`
  · `needs_login` · `starting` · `running`
- `via`: `socket` | `cli`
- `version`, `dns_name` (no trailing dot), `host_name`, `tailnet`, `magic_dns` (bool),
  `addresses` (strings), `https` (bool, CertDomains non-empty), `userspace` (bool, !TUN),
  `owner_login`
- `serve`: list of `ServeEntry { url, path, target, funnel }`
- `error`: text when unreachable

## Identity

- `login`, `name`, `device` (node name, may be empty for `serve`), `via`: `serve` | `whois`

## State transitions

Detection is stateless apart from the caches: status cached 10 s, `whois` per IP 60 s
(including "unknown" answers), cleared when `socket` changes.
