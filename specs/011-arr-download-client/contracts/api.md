# Contract: putio-sync API additions (bearer-token protected)

## `GET /arr`

Returns the download-client settings and status for the Settings page. It never includes the
password or API keys.

```json
{
  "enabled": true,
  "password_set": true,
  "username": "putio-sync",
  "url_base": "/transmission/",
  "download_dir": "/downloads",
  "clients_seen": [{"agent": "Sonarr", "last_contact": "2026-10-26T20:01:02Z"}],
  "servers": [
    {"index": 0, "kind": "sonarr", "url": "http://192.168.1.50:8989", "category": "sonarr",
     "api_key_set": true}
  ],
  "items": {"putio": 1, "local": 0, "done": 3, "failed": 0}
}
```

## `POST /arr/servers/{index}/test`

Calls `GET {url}/api/v3/system/status` on that server with its API key, with a 10 s timeout.

- 200 `{"ok": true, "app": "Sonarr", "version": "4.0.15.2941"}`.
- 200 `{"ok": false, "reason": "unreachable" | "unauthorized" | "not_arr" | "no_api_key",
  "detail": "<message without the key>"}`.
- 404 when the index does not exist.

`not_arr` covers a reply without `appName` Radarr or Sonarr, or a kind mismatch (the detail
names the app found).
