# Contract: bundled media plugins

All three: off by default; events `download.finished`, `sync.completed`; `tick_seconds = 5`;
fields `server` (url), `libraries` (string_list, choices), `path_map` (path_map),
`delay_seconds` (integer, default 20, 0..600), `max_wait_seconds` (integer, default 300);
counters `refreshes`, `refresh_failures`, `folders_grouped`, `skipped`; `health()`.

| Plugin | Secret | Actions | Hosts |
|--------|--------|---------|-------|
| plex | `token` | `sign_in`, `sign_in_poll`, `load_servers`, `load_libraries` | `plex.tv` + `server` |
| jellyfin | `api_key` | `load_libraries` | `server` |
| emby | `api_key` | `load_libraries` | `server` |

`sign_in_poll` is `hidden` (reached through `next`); `load_servers` and `load_libraries` are `hidden` too (reached through the fields' Load buttons).

## Plex calls

Every call: `Accept: application/json`, `X-Plex-Product: putio-sync`,
`X-Plex-Client-Identifier: <store client_id>`; with a token, `X-Plex-Token: <token>`.

| Purpose | Request |
|---------|---------|
| PIN | `POST https://plex.tv/api/v2/pins` → `{id, code}` |
| Poll | `GET https://plex.tv/api/v2/pins/<id>` → `{authToken}` |
| Account | `GET https://plex.tv/api/v2/user` → `{username}` |
| Servers | `GET https://plex.tv/api/v2/resources?includeHttps=1` |
| Reachable | `GET <server>/identity` |
| Libraries | `GET <server>/library/sections` |
| Refresh | `GET <server>/library/sections/<key>/refresh?path=<enc>` |

Library choice values are section keys, labels "Title (folder, …)".

## Jellyfin / Emby calls

Prefix: `""` (Jellyfin) or `/emby` (Emby). Auth: Jellyfin `Authorization: MediaBrowser
Client="putio-sync", Device="putio-sync", DeviceId="putio-sync", Version="<v>",
Token="<key>"`; Emby `X-Emby-Token: <key>`.

| Purpose | Request |
|---------|---------|
| Reachable | `GET <server><prefix>/System/Info/Public` |
| Key accepted | `GET <server><prefix>/System/Info` |
| Libraries | `GET <server><prefix>/Library/VirtualFolders` → `[{Name, ItemId, Locations}]` |
| Refresh | `POST <server><prefix>/Library/Media/Updated` `{"Updates":[{"Path":…,"UpdateType":"Created"}]}` |

Library choice values are library names.

## Health items

1. Server reachable (fail if not, or if `server` is empty).
2. Token/key accepted (fail on 401/403; warn if none saved).
3. Download folder maps into a chosen library (warn if not; skipped when
   `putio.download_dir` is empty).
