# Contract: Transmission RPC subset

`POST /transmission/rpc` on the API port. The route is public as far as the bearer token goes,
and is guarded by Basic auth and the session handshake. Only POST is accepted; any other method
gets 405.

## Request gate, in order

1. The download client is not enabled → **404** (the route behaves as if absent).
2. Enabled, but no password is configured → **503**, body `download client has no password`.
3. The client IP is rate-limited by `AuthLimiter` → **429**.
4. Basic credentials are missing or wrong → **401** with `WWW-Authenticate: Basic
   realm="putio-sync"`. Wrong credentials count against the limiter; missing ones do not,
   as with the bearer token.
5. `X-Transmission-Session-Id` is missing or wrong → **409**, with header
   `X-Transmission-Session-Id: <id>` and an empty body.
6. Otherwise the JSON body `{"method": str, "arguments": obj?, "tag": int?}` is dispatched.
   The reply is **200** `{"result": "success" | "<error text>", "arguments": {...}, "tag": <same>}`.

A body that is not valid JSON gets 200 `{"result": "invalid request"}`.

## Methods

### `session-get`

```json
{"result":"success","arguments":{
  "version":"3.00 (putio-sync)","rpc-version":17,"rpc-version-minimum":14,
  "download-dir":"<target directory>",
  "seedRatioLimit":0,"seedRatioLimited":false,
  "idle-seeding-limit":0,"idle-seeding-limit-enabled":true}}
```

### `torrent-add`

Arguments are `filename` (a magnet or http(s) .torrent URL) **or** `metainfo` (base64), plus
optional `download-dir`, `paused` (ignored) and `labels` (ignored).

- Success: `{"torrent-added": {"id": n, "name": s, "hashString": h}}`.
- Hash already tracked: `{"torrent-duplicate": {...same fields}}`. Put.io is not called.
- Errors are put in the `result` text:
  - `invalid magnet link`;
  - `invalid torrent file`;
  - `download-dir must be inside <target>`;
  - `Put.io refused the transfer: <reason>`.

### `torrent-get`

The arguments are `fields` (ignored: every known field is returned) and optional `ids` (hash
strings in any case, or numeric ids). The reply is `{"torrents": [ ... ]}`, with each item
shaped as in data-model.md "Transmission view". Unknown ids are left out.

### `torrent-remove`

The arguments are `ids` and `delete-local-data` (bool, default false). The reply is `{}`.
Unknown ids are ignored.

### `torrent-set`, `queue-move-top`, `queue-move-bottom`, `torrent-start`, `torrent-stop`, `session-set`, `session-stats`

These are accepted and reply `{}`. `session-stats` replies with zeroed counters.

### Any other method

`{"result": "method not supported"}`.
