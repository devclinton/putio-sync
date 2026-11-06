# Contract: `[dlna]` config

```toml
[dlna]
# Serve media to TVs and players on the home network (DLNA / UPnP). Off by default.
# Anyone on the LAN can browse what is shared here: there is no login.
enabled = false
# The name players show
name = "putio-sync"
# HTTP port for descriptions, browsing and streams (1900/UDP is used for discovery)
port = 8200
# Where to listen; same entries as [api] bind. Empty = every LAN IPv4 address (not
# Docker bridges). In Docker, discovery needs network_mode: host.
bind = []
# What players see: "downloads", "cat:<category>", "watch:<watch path>", "putio"
roots = ["downloads"]
# The Put.io folder shown as "Put.io" when roots has "putio" (folder id)
# putio_folder = 123456789
```

Environment overrides (same pattern as the other sections): `PUTIO_SYNC_DLNA_ENABLED`,
`PUTIO_SYNC_DLNA_NAME`, `PUTIO_SYNC_DLNA_PORT`.

Validation errors:
- `dlna.port` conflicts with `api`/`metrics` port, or is 1900.
- `dlna.roots` is empty while enabled; has an unknown key, or a `watch:` path not in
  `[watch] paths`.
- `dlna.putio_folder` is missing while `roots` has `putio`.
