# Quickstart: Tailscale Awareness

## Automated

```bash
TMPDIR=<scratch> make ci
cargo test -p putio-sync-tailscale
cargo test -p putio-sync-api --test tailscale
```

The tests run a fake LocalAPI on a temp unix socket; Tailscale need not be installed.

## By hand (a machine with Tailscale)

1. Start the daemon, open Settings → Tailscale: state running, MagicDNS name, addresses.
2. `tailscale serve --bg 15432`, reload: the HTTPS address appears first.
3. Turn on identity sign-in (allow-list fills with your login), save. Open the HTTPS
   address in a private window: Status page, header shows your login.
4. From a LAN browser without the token: sign-in page.
5. Turn on tailnet-only, confirm, restart: the LAN address no longer answers; the tailnet
   address does.

## Docker

- Socket from the host or a sidecar: mount `/var/run/tailscale:/var/run/tailscale:ro`.
- Sidecar: `network_mode: service:tailscale`, share the socket volume, and run
  `tailscale serve` in the sidecar.
