# Contract: Listen addresses API

All routes need the API token (or a Tailscale identity) and answer 503 in setup mode.

## GET /listen

What this process listens on now.

```json
{
  "api": {
    "configured": ["eth0:15432", "127.0.0.1:15432"],
    "effective": ["eth0:15432", "127.0.0.1:15432"],
    "listening": ["192.168.1.20:15432", "127.0.0.1:15432"],
    "entries": [
      { "entry": "eth0:15432", "addresses": ["192.168.1.20:15432"] },
      { "entry": "127.0.0.1:15432", "addresses": ["127.0.0.1:15432"] }
    ],
    "port": 15432,
    "tailnet_only": false,
    "env_override": false
  },
  "metrics": {
    "enabled": true,
    "configured": ["0.0.0.0:9742"],
    "effective": ["0.0.0.0:9742"],
    "entries": [{ "entry": "0.0.0.0:9742", "addresses": ["0.0.0.0:9742"] }],
    "env_override": false
  },
  "engine": "daemon",
  "server_flags": false
}
```

- `effective` is the list after env and tailnet-only (and `server` flags: `server_flags`
  true). An entry that gives nothing has `"waiting": "<reason>"` instead of addresses.
- `listening` is the bound list of this process (empty while waiting). Metrics bound
  addresses are not tracked across processes; `entries` shows what the list gives now.

## GET /settings/network

```json
{ "api_bind": ["0.0.0.0:15432"], "metrics_bind": ["0.0.0.0:9742"] }
```

## PUT /settings/network

Same body. 400 with `error` when an entry is malformed, a list rule fails, or no API entry
gives an address now (`No API address gives an address now: eth5:15432 (no such interface
eth5)`). 200 `{ "message": "Network settings saved. Needs a restart: api_bind." }` plus a
note when an env variable or tailnet-only overrides the saved list.

## POST /settings/network/preview

The assistant's generic preview (valid, error, changes, risks).

## POST /settings/network/check

Body as PUT. Never saves.

```json
{
  "valid": true,
  "error": null,
  "api": [{ "entry": "eth0:15432", "addresses": ["192.168.1.20:15432"] }],
  "metrics": [{ "entry": "0.0.0.0:9742", "addresses": ["0.0.0.0:9742"] }],
  "this_page": "kept",
  "listened_on": "0.0.0.0:15432",
  "ui_at": ["http://192.168.1.20:15432/"]
}
```

- `this_page`: `kept` | `cut_off` | `unknown` (no `ListenedOn`, or a wildcard listener
  reached by name). The UI confirms on `cut_off` and `unknown` when the API list changed.
- `ui_at`: `http://` URLs for the API addresses the new list gives now (wildcards shown as
  the Host header's name with the new port, else `localhost`).
- `valid: false` with `error` for anything PUT would refuse.

## server flags

| Flags | Listens on |
|-------|------------|
| none | `api_listen()` (configured list, or tailnet-only) |
| `--bind H` | `H:<first configured port>` |
| `--bind H --port P` | `H:P` |
| `--port P` | every `api_listen()` entry with port P |
| `--bind H:P2 --port P` | error: give the port once |

## Doctor check `listen`

- OK: "Listening on 192.168.1.20:15432, 127.0.0.1:15432." (from the config's resolution)
- Warning: an interface entry with no address ("eth5:15432: no such interface eth5").
- Warning: identity sign-in on and an API entry on loopback: "Only `tailscale serve`
  should reach putio-sync on loopback: another local proxy could forward forged identity
  headers."
