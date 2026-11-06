# Contract: API routes (auth required, like other settings)

## `GET /settings/media-server`

```json
{
  "enabled": false,
  "name": "putio-sync",
  "port": 8200,
  "bind": [],
  "roots": ["downloads"],
  "putio_folder": null,
  "available_roots": [
    {"key": "downloads", "title": "Downloads", "path": "/downloads"},
    {"key": "cat:radarr", "title": "radarr", "path": "/downloads/radarr"},
    {"key": "watch:/watches/sonarr", "title": "/watches/sonarr", "path": "/watches/sonarr"},
    {"key": "putio", "title": "Put.io", "path": null}
  ]
}
```

`available_roots` lists:
- Downloads;
- each existing category subfolder (the category names from the download rules plus folders
  on disk);
- each `[watch] paths` entry when watching is on;
- `putio`.

## `PUT /settings/media-server`

The body has the same fields minus `available_roots`.
- Returns `200` with `SuccessResponse` and the saved view.
- Applied at once (research R8).
- `400` when `Config::validate` fails, with the field error.
- The same preview flow as the other settings groups (`POST /settings/media-server/preview`).

## `GET /media-server/status`

```json
{
  "state": "running",
  "reason": null,
  "addresses": ["http://192.168.1.10:8200/description.xml"],
  "streams_active": 1,
  "players_last_hour": 2,
  "putio": {"state": "ok", "reason": null}
}
```

`state` is one of `off`, `starting`, `running` or `failed`. While running, `reason` may carry
a warning, for example: "No LAN interface: players can't find it. Use host networking."

## UI: Settings › Media server

- **Toggle**: on or off.
- **Name**.
- **Port**.
- **Listen on**: the bind list editor from 037.
- **Shared folders**: checkboxes from `available_roots`. Picking Put.io opens a folder picker
  that uses the existing Put.io browse routes.
- **Status line** (polled every 5 s while the section is open): state, addresses, streams
  playing, players in the last hour, and the Put.io state.
- A warning under the toggle: "Anyone on your home network can browse and play what you share
  here."
