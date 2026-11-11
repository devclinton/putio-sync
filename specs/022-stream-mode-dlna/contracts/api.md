# Contract: API

No new routes.

- Folder rule JSON (`GET/PUT /settings/rules`, rule objects): optional `"mode": "download" |
  "stream"`.
- Effective settings JSON (rules resolve/preview): `"mode": {"value": "stream", "from":
  "Movies"}` alongside the other `Sourced` fields.
- Rules preview: decision `"stream"` per item and a `"stream"` count.
- Download settings (`[download]` through the settings form): `mode`, `stream_mp4`.
- Media server settings (`GET/PUT /settings/media-server`): unchanged; `putio_folder: null`
  with `putio` in `roots` is now accepted and means the synced folder.
