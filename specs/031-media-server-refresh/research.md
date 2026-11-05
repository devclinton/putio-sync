# Research: Media Server Refresh

## R1 Plex: PIN sign-in

- **Decision**: `POST https://plex.tv/api/v2/pins` (non-strong, so the 4-character code
  also works at `plex.tv/link`) with `Accept: application/json`, `X-Plex-Product:
  putio-sync`, `X-Plex-Client-Identifier: <id>`. Show `code`, a link to
  `https://plex.tv/link` and `https://app.plex.tv/auth#?clientID=<id>&code=<code>&context%5Bdevice%5D%5Bproduct%5D=putio-sync`.
  Poll `GET https://plex.tv/api/v2/pins/<id>` (same client id) every 2 s; `authToken`
  non-null means done. Then `GET https://plex.tv/api/v2/user` with `X-Plex-Token` for the
  account name. The client id is made once (random hex from `math.random` seeded by
  Lua 5.4 and the time) and kept in the plugin store.
- **Rationale**: the documented flow for third-party apps; no password or token handled
  by the user.
- **Alternatives**: strong PIN (only the long link works, no `plex.tv/link` code);
  username/password to `users/sign_in` (deprecated, needs 2FA handling); pasted token
  (still possible through the secret field).

## R2 Plex: servers, libraries, refresh

- **Decision**: servers from `GET https://plex.tv/api/v2/resources?includeHttps=1` (items
  whose `provides` contains `server`; each `connections[]` uri, local first, relay
  skipped). Libraries from `GET <server>/library/sections` (`MediaContainer.Directory[]`:
  `key`, `title`, `type`, `Location[].path`). Refresh with `GET
  <server>/library/sections/<key>/refresh?path=<url-encoded folder>`. Reachability with
  `GET <server>/identity` (no token), token check with `/library/sections` (401 = refused).
  Token always in `X-Plex-Token` header, never in the query.
- **Rationale**: the partial-scan endpoint Plex's own "scan on change" uses; one folder, so
  seconds instead of a library scan.

## R3 Jellyfin and Emby

- **Decision**: `POST <server>/Library/Media/Updated` with `{"Updates":[{"Path":…,
  "UpdateType":"Created"}, …]}` (several folders per request), libraries from `GET
  /Library/VirtualFolders` (`Name`, `ItemId`, `Locations[]`), health from `GET
  /System/Info/Public` (reachable) and `GET /System/Info` (key accepted). Jellyfin sends
  `Authorization: MediaBrowser Client="putio-sync", Device="putio-sync",
  DeviceId="putio-sync", Version="…", Token="<key>"`; Emby sends `X-Emby-Token: <key>`
  and uses the `/emby` path prefix. One shared script with a `flavor` switch.
- **Rationale**: both servers' documented "a file changed at this path" call; they find
  the library from the path themselves, so the picker only limits which paths are sent.
- **Alternatives**: `/Items/<id>/Refresh` per library (a scan of the whole library).

## R4 Grouping

- **Decision**: a pending folder keeps `first`, `last`, `count`, `tries`. Due when
  `now - last >= delay_seconds` or `now - first >= max_wait_seconds`; all due on
  `sync.completed`. A failed refresh keeps the folder with `tries + 1` and a new `first =
  last = now`; at 2 tries it is dropped and counted. Tick every 5 s (`tick_seconds = 5`).
- **Rationale**: a season saved in one run lands within a minute or so between files and
  ends with `sync.completed`, which flushes at once; the quiet window covers the watch
  folder and single files.

## R5 Running actions and health

- **Decision**: `Job::Call { function, args, reply }` on a worker of its own built from a
  spec of the saved settings merged with the request's non-secret overrides, `allowed =
  hosts_needed(merged)`. One call per plugin at a time (`busy` set in the service, 409).
  The worker uses the plugin's shared store. The result value is converted to JSON and
  every string in it is scrubbed with the plugin's secrets before it leaves the service;
  `save` is kept apart, checked against the manifest (`check_values` on the merged
  settings) and handed to the API.
- **Rationale**: no interference with the running plugin's queue or state; a slow sign-in
  poll never delays refreshes. The cost (a fresh Lua state per call, about 1 ms plus
  `init`) is small next to an HTTP call.
- **Alternatives**: running on the live slot (blocks events; can't use unsaved form
  values); keeping sign-in state in the daemon (more state, lost on reload; the page-driven
  `next` keeps the daemon stateless).

## R6 Path rules

- **Decision**: `from` must be absolute (`/…`); `to` absolute POSIX or Windows (`C:\…`,
  `C:/…`, `\\host\share`). Normalise by trimming trailing separators. Match the longest
  `from` that equals the path or is followed by `/`. A Windows `to` turns the remainder's `/`
  into `\`. `within(path, folder)` compares on separator boundaries, without case when either
  side is Windows-style. Stored in TOML as an array of `{from, to}` tables; the core check
  allows config values that are arrays of tables with string values.
- **Rationale**: matches how Docker volume mappings and Sonarr's "remote path mappings"
  are thought of; boundary matching avoids `/downloads2` matching `/downloads`.

## R7 UI

- **Decision**: `path_map` edited as a textarea, one `from => to` rule per line (small code,
  copy-paste friendly); action buttons in the card; Load next to a field with `choices`
  becomes a `<select>` (text/url) or tick list (string_list). The page follows `next` with
  `setTimeout`, stops after 10 minutes, when the card leaves the page or on `done`. Pure
  helpers (parse/format rules, merge an action answer into a row) live in `plugins.js` with
  node tests; DOM in `settings.js`.
- **Rationale**: bundle has about 34 KB left; a textarea and a few buttons cost about 4 KB.
